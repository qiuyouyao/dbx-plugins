//! End-to-end tests against two real S3-compatible services (MinIO in Docker).
//! Skipped unless S3_SYNC_TEST_SOURCE_ENDPOINT / S3_SYNC_TEST_TARGET_ENDPOINT
//! are set. See backend/README or the repo docs for the docker commands.
//!
//! Run: cargo test --release integration -- --nocapture --test-threads=1

use std::sync::{Arc, Mutex};

use crate::engine::{self, EventSink};
use crate::model::{ConflictPolicy, RunSettings, TaskSide, TaskSpec};
use crate::plan;
use crate::s3::{S3Client, S3Config};
use crate::store::RunStatus;

fn test_client(endpoint_var: &str) -> Option<S3Client> {
    let endpoint = std::env::var(endpoint_var).ok()?;
    Some(
        S3Client::new(S3Config {
            endpoint,
            region: "us-east-1".into(),
            access_key: std::env::var("S3_SYNC_TEST_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".into()),
            secret_key: std::env::var("S3_SYNC_TEST_SECRET_KEY").unwrap_or_else(|_| "minioadmin".into()),
            path_style: true,
            insecure_tls: false,
        })
        .expect("test client"),
    )
}

fn collector() -> (EventSink, Arc<Mutex<Vec<serde_json::Value>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = events.clone();
    let sink: EventSink = Arc::new(move |_method, params| {
        sink_events.lock().unwrap().push(params);
    });
    (sink, events)
}

fn settings() -> RunSettings {
    // Small thresholds so multipart paths are exercised without huge uploads.
    RunSettings { object_concurrency: 4, multipart_threshold: 5 * 1024 * 1024, part_size: 5 * 1024 * 1024, part_concurrency: 2 }
}

async fn wait_finished(handle: &engine::RunHandle) -> crate::store::RunRecord {
    for _ in 0..600 {
        let snapshot = handle.snapshot();
        if snapshot.finished_at.is_some() {
            return snapshot;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("run did not finish in 60s");
}

fn unique_bucket(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4().as_simple())
}

#[tokio::test]
async fn integration_full_incremental_conflict_and_failures() {
    let Some(source) = test_client("S3_SYNC_TEST_SOURCE_ENDPOINT") else {
        eprintln!("skipping: S3_SYNC_TEST_SOURCE_ENDPOINT not set");
        return;
    };
    let Some(target) = test_client("S3_SYNC_TEST_TARGET_ENDPOINT") else {
        eprintln!("skipping: S3_SYNC_TEST_TARGET_ENDPOINT not set");
        return;
    };
    let runtime = tokio::runtime::Handle::current();
    // Keep run history and temp files out of the source tree during tests.
    let test_data_dir = std::env::temp_dir().join(format!("s3-sync-test-{}", uuid::Uuid::new_v4()));
    std::env::set_var("DBX_PLUGIN_DATA_DIR", &test_data_dir);
    let source_bucket = unique_bucket("src");
    let target_bucket = unique_bucket("dst");
    source.create_bucket(&source_bucket).await.unwrap();
    target.create_bucket(&target_bucket).await.unwrap();

    let task = TaskSpec {
        name: "integration".into(),
        source: TaskSide { connection_id: "src".into(), bucket: source_bucket.clone(), prefix: "data/".into() },
        target: TaskSide { connection_id: "dst".into(), bucket: target_bucket.clone(), prefix: "mirror/".into() },
        conflict_policy: ConflictPolicy::SourceAuthority,
    };

    // Seed: small objects (incl. space/unicode keys), one multipart-sized object.
    let small: Vec<(&str, Vec<u8>)> = vec![
        ("data/a.txt", b"alpha".to_vec()),
        ("data/nested/b.txt", b"beta".to_vec()),
        ("data/with space/中文.bin", vec![7u8; 1024]),
    ];
    for (key, body) in &small {
        source.put_object(&source_bucket, key, body.clone(), "text/plain", &Default::default()).await.unwrap();
    }
    let big = vec![42u8; 11 * 1024 * 1024]; // > 5 MiB threshold, 3 parts
    source.put_object(&source_bucket, "data/big.bin", big.clone(), "application/octet-stream", &Default::default()).await.unwrap();

    // 1) First run: everything is new.
    let plan = plan::build_plan(&source, &target, &task, || false).await.unwrap();
    assert_eq!(plan.items.len(), 4, "all four objects should be new");
    assert_eq!(plan.skip_count, 0);
    let (sink, events) = collector();
    let handle = engine::start_run(&runtime, source.clone(), target.clone(), task.clone(), plan, settings(), None, sink);
    let record = wait_finished(&handle).await;
    assert_eq!(record.status, RunStatus::Completed, "failures: {:?}", record.failures);
    assert_eq!(record.done_count, 4);
    assert!(!events.lock().unwrap().is_empty(), "progress events were emitted");

    // Target keys carry the swapped prefix and the fingerprint metadata.
    let head = target.head_object(&target_bucket, "mirror/a.txt").await.unwrap().expect("copied");
    assert_eq!(head.user_meta.get("src-etag").map(String::as_str), Some(head.etag.as_str()));
    let big_head = target.head_object(&target_bucket, "mirror/big.bin").await.unwrap().expect("big copied");
    assert_eq!(big_head.size, big.len() as u64);

    // 2) Second run: incremental judgement skips everything.
    let plan = plan::build_plan(&source, &target, &task, || false).await.unwrap();
    assert_eq!(plan.items.len(), 0, "identical objects must be skipped");
    assert_eq!(plan.skip_count, 4);

    // 3) Change one source object (same size, new content): exactly one transfer.
    source.put_object(&source_bucket, "data/a.txt", b"ALPHA".to_vec(), "text/plain", &Default::default()).await.unwrap();
    let plan = plan::build_plan(&source, &target, &task, || false).await.unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].source_key, "data/a.txt");
    let (sink, _) = collector();
    let handle = engine::start_run(&runtime, source.clone(), target.clone(), task.clone(), plan, settings(), None, sink);
    let record = wait_finished(&handle).await;
    assert_eq!(record.status, RunStatus::Completed);
    let content = {
        let path = std::env::temp_dir().join(format!("s3sync-check-{}.txt", uuid::Uuid::new_v4()));
        target.get_object_to_file(&target_bucket, "mirror/a.txt", &path).await.unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        bytes
    };
    assert_eq!(content, b"ALPHA");

    // 4) skipExisting policy never overwrites, even when source changed.
    source.put_object(&source_bucket, "data/a.txt", b"never".to_vec(), "text/plain", &Default::default()).await.unwrap();
    let skip_task = TaskSpec { conflict_policy: ConflictPolicy::SkipExisting, ..task.clone() };
    let plan = plan::build_plan(&source, &target, &skip_task, || false).await.unwrap();
    assert_eq!(plan.items.len(), 0);
    assert_eq!(plan.skip_count, 4);

    // 5) Failure path: a plan item whose source key vanished records a failure
    //    without blocking the rest, and the run still completes.
    let mut bad_plan = plan::build_plan(&source, &target, &task, || false).await.unwrap();
    bad_plan.items.push(crate::model::PlanItem {
        source_key: "data/ghost.bin".into(),
        target_key: "mirror/ghost.bin".into(),
        size: 1,
        reason: crate::model::TransferReason::New,
    });
    bad_plan.total_bytes += 1;
    let (sink, _) = collector();
    let handle = engine::start_run(&runtime, source.clone(), target.clone(), task.clone(), bad_plan, settings(), None, sink);
    let record = wait_finished(&handle).await;
    assert_eq!(record.failed_count, 1);
    assert_eq!(record.done_count, 1, "the changed a.txt still transfers");
    assert_eq!(record.failures[0].source_key, "data/ghost.bin");
    assert_eq!(record.status, RunStatus::Completed);

    // 6) Cancellation: a fresh full copy to a new bucket, cancelled immediately.
    let cancel_bucket = unique_bucket("cancel");
    target.create_bucket(&cancel_bucket).await.unwrap();
    let cancel_task = TaskSpec {
        target: TaskSide { connection_id: "dst".into(), bucket: cancel_bucket, prefix: String::new() },
        ..task.clone()
    };
    let plan = plan::build_plan(&source, &target, &cancel_task, || false).await.unwrap();
    let (sink, _) = collector();
    let handle = engine::start_run(&runtime, source.clone(), target.clone(), cancel_task, plan, settings(), None, sink);
    handle.cancel();
    let record = wait_finished(&handle).await;
    assert_eq!(record.status, RunStatus::Cancelled);
    let _ = tokio::fs::remove_dir_all(&test_data_dir).await;
}