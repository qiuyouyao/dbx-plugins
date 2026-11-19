//! Run execution: bounded concurrency, object-level retry with backoff,
//! multipart for large objects, cancellation at object/part boundaries,
//! throttled progress events and a persisted failure list.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::Semaphore;

use crate::model::{fingerprint_meta, Plan, PlanItem, RunSettings, TaskSpec};
use crate::s3::{S3Client, S3Error};
use crate::store::{self, FailureEntry, RunRecord, RunStatus};

const MAX_ATTEMPTS: u32 = 3;
/// Global memory budget per run: at most 4 whole small objects (<= threshold)
/// and 8 multipart parts buffered at once.
const SINGLE_PUT_PERMITS: usize = 4;
const PART_PERMITS: usize = 8;
const PROGRESS_THROTTLE: Duration = Duration::from_millis(300);

/// Event sink abstraction so tests can collect progress without a live
/// sidecar protocol; main.rs wires it to PluginEmitter::event.
pub type EventSink = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

#[derive(Clone)]
pub struct RunHandle {
    pub record: Arc<Mutex<RunRecord>>,
    pub cancelled: Arc<AtomicBool>,
}

impl RunHandle {
    pub fn snapshot(&self) -> RunRecord {
        self.record.lock().expect("run record lock").clone()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub fn start_run(
    runtime: &tokio::runtime::Handle,
    source: S3Client,
    target: S3Client,
    task: TaskSpec,
    plan: Plan,
    settings: RunSettings,
    retry_of: Option<String>,
    events: EventSink,
) -> RunHandle {
    let run_id = uuid::Uuid::new_v4().to_string();
    let record = RunRecord {
        run_id: run_id.clone(),
        task,
        settings: settings.clone(),
        status: RunStatus::Running,
        started_at: chrono::Utc::now(),
        finished_at: None,
        total_count: plan.items.len() as u64,
        done_count: 0,
        skip_count: plan.skip_count,
        failed_count: 0,
        total_bytes: plan.total_bytes,
        transferred_bytes: 0,
        failures: Vec::new(),
        retry_of,
    };
    let handle = RunHandle { record: Arc::new(Mutex::new(record)), cancelled: Arc::new(AtomicBool::new(false)) };
    let worker = handle.clone();
    let items = plan.items;
    let task_events = events.clone();
    runtime.spawn(async move {
        execute(worker.clone(), source, target, items, settings, task_events.clone()).await;
        let snapshot = worker.snapshot();
        if let Err(error) = store::save_run(&snapshot).await {
            eprintln!("[s3-sync] failed to persist run {}: {error}", snapshot.run_id);
        }
        let _ = tokio::fs::remove_dir_all(store::tmp_dir(&snapshot.run_id)).await;
        emit_progress(&task_events, &snapshot);
    });
    emit_progress(&events, &handle.snapshot());
    handle
}

async fn execute(
    handle: RunHandle,
    source: S3Client,
    target: S3Client,
    items: Vec<PlanItem>,
    settings: RunSettings,
    events: EventSink,
) {
    let object_slots = Arc::new(Semaphore::new(settings.object_concurrency));
    let put_slots = Arc::new(Semaphore::new(SINGLE_PUT_PERMITS));
    let part_slots = Arc::new(Semaphore::new(PART_PERMITS));
    let tmp_dir = store::tmp_dir(&handle.snapshot().run_id);
    if let Err(error) = tokio::fs::create_dir_all(&tmp_dir).await {
        eprintln!("[s3-sync] cannot create temp dir: {error}");
    }
    let last_emit = Arc::new(Mutex::new(std::time::Instant::now() - PROGRESS_THROTTLE));

    let mut futures = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        let handle = handle.clone();
        let source = source.clone();
        let target = target.clone();
        let settings = settings.clone();
        let object_slots = object_slots.clone();
        let put_slots = put_slots.clone();
        let part_slots = part_slots.clone();
        let tmp_dir = tmp_dir.clone();
        let events = events.clone();
        let last_emit = last_emit.clone();
        futures.push(async move {
            let Ok(_slot) = object_slots.acquire().await else { return };
            if handle.is_cancelled() {
                return;
            }
            let result = transfer_with_retry(&handle, &source, &target, &item, &settings, &put_slots, &part_slots, &tmp_dir, index).await;
            let mut record = handle.record.lock().expect("run record lock");
            match result {
                Ok(()) => {
                    record.done_count += 1;
                    record.transferred_bytes += item.size;
                }
                Err(error) => {
                    record.failed_count += 1;
                    record.failures.push(FailureEntry {
                        source_key: item.source_key.clone(),
                        target_key: item.target_key.clone(),
                        size: item.size,
                        error: error.to_string(),
                    });
                }
            }
            let snapshot = record.clone();
            drop(record);
            let mut last = last_emit.lock().expect("progress lock");
            if last.elapsed() >= PROGRESS_THROTTLE {
                *last = std::time::Instant::now();
                emit_progress(&events, &snapshot);
            }
        });
    }
    stream::iter(futures).buffer_unordered(settings.object_concurrency).collect::<Vec<_>>().await;

    let mut record = handle.record.lock().expect("run record lock");
    record.finished_at = Some(chrono::Utc::now());
    record.status = if handle.is_cancelled() {
        RunStatus::Cancelled
    } else if record.failed_count > 0 && record.done_count == 0 {
        RunStatus::Failed
    } else {
        RunStatus::Completed
    };
}

async fn transfer_with_retry(
    handle: &RunHandle,
    source: &S3Client,
    target: &S3Client,
    item: &PlanItem,
    settings: &RunSettings,
    put_slots: &Semaphore,
    part_slots: &Semaphore,
    tmp_dir: &std::path::Path,
    index: usize,
) -> Result<(), S3Error> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        if handle.is_cancelled() {
            return Err(S3Error::transport("Cancelled"));
        }
        match transfer_object(handle, source, target, item, settings, put_slots, part_slots, tmp_dir, index).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if attempt >= MAX_ATTEMPTS || !error.retryable() || handle.is_cancelled() {
                    return Err(error);
                }
                let backoff = Duration::from_secs(1 << (attempt - 1));
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

async fn transfer_object(
    handle: &RunHandle,
    source: &S3Client,
    target: &S3Client,
    item: &PlanItem,
    settings: &RunSettings,
    put_slots: &Semaphore,
    part_slots: &Semaphore,
    tmp_dir: &std::path::Path,
    index: usize,
) -> Result<(), S3Error> {
    // Re-read source metadata so the fingerprint reflects the object we
    // actually copy, not a possibly stale listing entry.
    let source_meta = source
        .head_object(&task_bucket_source(handle), &item.source_key)
        .await?
        .ok_or_else(|| S3Error { status: Some(404), code: "NoSuchKey".into(), message: format!("Source object vanished: {}", item.source_key) })?;
    let meta = fingerprint_meta(&source_meta);

    let tmp_path = tmp_dir.join(format!("{index}.part"));
    let (size, content_type) = source.get_object_to_file(&task_bucket_source(handle), &item.source_key, &tmp_path).await?;
    let result = if size <= settings.multipart_threshold {
        let _permit = put_slots.acquire().await.map_err(|_| S3Error::transport("Transfer slots closed"))?;
        let body = tokio::fs::read(&tmp_path).await.map_err(|e| S3Error::transport(e.to_string()))?;
        target.put_object(&task_bucket_target(handle), &item.target_key, body, &content_type, &meta).await
    } else {
        upload_multipart(handle, target, item, settings, part_slots, &tmp_path, size, &content_type, &meta).await
    };
    let _ = tokio::fs::remove_file(&tmp_path).await;
    result
}

async fn upload_multipart(
    handle: &RunHandle,
    target: &S3Client,
    item: &PlanItem,
    settings: &RunSettings,
    part_slots: &Semaphore,
    tmp_path: &std::path::Path,
    size: u64,
    content_type: &str,
    meta: &std::collections::HashMap<String, String>,
) -> Result<(), S3Error> {
    let bucket = task_bucket_target(handle);
    let upload_id = target.create_multipart(&bucket, &item.target_key, content_type, meta).await?;
    let part_count = size.div_ceil(settings.part_size) as u32;

    let upload = async {
        let mut part_futures = Vec::new();
        for part_number in 1..=part_count {
            let target = target.clone();
            let bucket = bucket.clone();
            let key = item.target_key.clone();
            let upload_id = upload_id.clone();
            let path = tmp_path.to_path_buf();
            let part_size = settings.part_size;
            let slots = part_slots;
            let cancelled = handle.cancelled.clone();
            part_futures.push(async move {
                if cancelled.load(Ordering::SeqCst) {
                    return Err(S3Error::transport("Cancelled"));
                }
                let _permit = slots.acquire().await.map_err(|_| S3Error::transport("Part slots closed"))?;
                let offset = (part_number as u64 - 1) * part_size;
                let length = part_size.min(size - offset) as usize;
                let mut file = tokio::fs::File::open(&path).await.map_err(|e| S3Error::transport(e.to_string()))?;
                file.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| S3Error::transport(e.to_string()))?;
                let mut body = vec![0u8; length];
                file.read_exact(&mut body).await.map_err(|e| S3Error::transport(e.to_string()))?;
                let etag = target.upload_part(&bucket, &key, &upload_id, part_number, body).await?;
                Ok((part_number, etag))
            });
        }
        let mut parts: Vec<(u32, String)> = stream::iter(part_futures)
            .buffer_unordered(settings.part_concurrency)
            .collect::<Vec<Result<(u32, String), S3Error>>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        parts.sort_by_key(|(number, _)| *number);
        target.complete_multipart(&bucket, &item.target_key, &upload_id, &parts).await
    };
    let result = upload.await;
    if result.is_err() {
        target.abort_multipart(&bucket, &item.target_key, &upload_id).await;
    }
    result
}

fn task_bucket_source(handle: &RunHandle) -> String {
    handle.record.lock().expect("run record lock").task.source.bucket.clone()
}

fn task_bucket_target(handle: &RunHandle) -> String {
    handle.record.lock().expect("run record lock").task.target.bucket.clone()
}

fn emit_progress(events: &EventSink, record: &RunRecord) {
    events(
        "s3-sync/progress",
        json!({
            "runId": record.run_id,
            "status": record.status,
            "totalCount": record.total_count,
            "doneCount": record.done_count,
            "skipCount": record.skip_count,
            "failedCount": record.failed_count,
            "totalBytes": record.total_bytes,
            "transferredBytes": record.transferred_bytes,
            "failures": record.failures,
        }),
    );
}