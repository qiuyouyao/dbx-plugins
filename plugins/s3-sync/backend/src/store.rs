//! Run history persistence in the sidecar data directory (DBX_PLUGIN_DATA_DIR).
//! Task definitions live in the workbench's host.storage; only execution
//! records — which can grow large — land here.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::model::{RunSettings, TaskSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureEntry {
    pub source_key: String,
    pub target_key: String,
    pub size: u64,
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: String,
    pub task: TaskSpec,
    pub settings: RunSettings,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub total_count: u64,
    pub done_count: u64,
    pub skip_count: u64,
    pub failed_count: u64,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub failures: Vec<FailureEntry>,
    /// Set when this run only retried the failures of an earlier run.
    #[serde(default)]
    pub retry_of: Option<String>,
}

pub fn data_dir() -> PathBuf {
    std::env::var("DBX_PLUGIN_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".s3-sync-data"))
}

fn history_dir() -> PathBuf {
    data_dir().join("history")
}

pub fn tmp_dir(run_id: &str) -> PathBuf {
    data_dir().join("tmp").join(run_id)
}

pub async fn save_run(record: &RunRecord) -> std::io::Result<()> {
    let dir = history_dir();
    tokio::fs::create_dir_all(&dir).await?;
    let json = serde_json::to_vec_pretty(record).expect("run record serializes");
    tokio::fs::write(dir.join(format!("{}.json", record.run_id)), json).await
}

pub async fn load_run(run_id: &str) -> Option<RunRecord> {
    let bytes = tokio::fs::read(history_dir().join(format!("{run_id}.json"))).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub async fn list_runs() -> Vec<RunRecord> {
    let mut records = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(history_dir()).await else { return records };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(bytes) = tokio::fs::read(entry.path()).await {
            if let Ok(record) = serde_json::from_slice::<RunRecord>(&bytes) {
                records.push(record);
            }
        }
    }
    records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    records
}