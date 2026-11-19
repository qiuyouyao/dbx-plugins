//! Diff planning: list both sides, judge each source object against the
//! target, and produce the execution plan used by preview and run alike.

use std::collections::HashMap;

use futures_util::stream::{self, StreamExt};

use crate::model::{fingerprint_matches, ConflictPolicy, ObjectMeta, Plan, PlanItem, TaskSpec, TransferReason};
use crate::s3::{S3Client, S3Error};

const LIST_PAGE_SIZE: u32 = 1000;
const HEAD_CONCURRENCY: usize = 16;

pub async fn list_all(client: &S3Client, bucket: &str, prefix: &str) -> Result<Vec<ObjectMeta>, S3Error> {
    let mut objects = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let page = client.list_objects(bucket, prefix, token.as_deref(), LIST_PAGE_SIZE).await?;
        objects.extend(page.objects);
        match page.next_token {
            Some(next) => token = Some(next),
            None => break,
        }
    }
    Ok(objects)
}

/// Build the plan. `cancel` is polled between pages so a huge bucket listing
/// can still be aborted from the UI.
pub async fn build_plan(
    source: &S3Client,
    target: &S3Client,
    task: &TaskSpec,
    mut cancel: impl FnMut() -> bool,
) -> Result<Plan, S3Error> {
    let source_objects = list_all(source, &task.source.bucket, &task.source.prefix).await?;
    if cancel() {
        return Err(S3Error::transport("Cancelled"));
    }
    let target_objects = list_all(target, &task.target.bucket, &task.target.prefix).await?;
    let target_index: HashMap<String, ObjectMeta> =
        target_objects.into_iter().map(|object| (object.key.clone(), object)).collect();

    let mut plan = Plan::default();
    // Candidates whose size matches need a HEAD to read the fingerprint.
    let mut need_head: Vec<(ObjectMeta, String)> = Vec::new();

    for source_object in source_objects {
        if cancel() {
            return Err(S3Error::transport("Cancelled"));
        }
        let target_key = task.target_key(&source_object.key);
        match target_index.get(&target_key) {
            None => plan.items.push(PlanItem {
                source_key: source_object.key,
                target_key,
                size: source_object.size,
                reason: TransferReason::New,
            }),
            Some(target_object) => {
                if task.conflict_policy == ConflictPolicy::SkipExisting {
                    plan.skip_count += 1;
                    continue;
                }
                if target_object.size != source_object.size {
                    plan.items.push(PlanItem {
                        source_key: source_object.key,
                        target_key,
                        size: source_object.size,
                        reason: TransferReason::Changed,
                    });
                } else {
                    need_head.push((source_object, target_key));
                }
            }
        }
    }

    let head_results: Vec<Result<(ObjectMeta, String, Option<ObjectMeta>), S3Error>> = stream::iter(need_head)
        .map(|(source_object, target_key)| async {
            let head = target.head_object(&task.target.bucket, &target_key).await?;
            Ok((source_object, target_key, head))
        })
        .buffer_unordered(HEAD_CONCURRENCY)
        .collect()
        .await;

    for result in head_results {
        let (source_object, target_key, head) = result?;
        match head {
            Some(target_object) if fingerprint_matches(&source_object, &target_object) => {
                plan.skip_count += 1;
            }
            Some(_) => plan.items.push(PlanItem {
                source_key: source_object.key,
                target_key,
                size: source_object.size,
                reason: TransferReason::Unverified,
            }),
            // Deleted between list and head: treat as new.
            None => plan.items.push(PlanItem {
                source_key: source_object.key,
                target_key,
                size: source_object.size,
                reason: TransferReason::New,
            }),
        }
    }

    plan.items.sort_by(|a, b| a.source_key.cmp(&b.source_key));
    plan.total_bytes = plan.items.iter().map(|item| item.size).sum();
    Ok(plan)
}