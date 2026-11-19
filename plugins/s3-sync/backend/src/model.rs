//! Domain model: objects, task specs, source fingerprints and diff plans.
//! The fingerprint contract is recorded in docs/adr/0001: metadata keys are a
//! compatibility contract once released — only add keys, keep reading old ones.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const META_SRC_ETAG: &str = "src-etag";
pub const META_SRC_MTIME: &str = "src-mtime";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
    /// Target-side user metadata (without the x-amz-meta- prefix). Listing
    /// responses leave this empty; HEAD fills it.
    #[serde(default)]
    pub user_meta: HashMap<String, String>,
}

/// One side of a migration task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSide {
    pub connection_id: String,
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictPolicy {
    /// Source wins: overwrite targets that differ or cannot be judged.
    SourceAuthority,
    /// Never touch an existing target object.
    SkipExisting,
}

impl Default for ConflictPolicy {
    fn default() -> Self {
        Self::SourceAuthority
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSpec {
    /// Display name carried from the workbench task definition; optional on
    /// the wire so raw RPC callers can omit it.
    #[serde(default)]
    pub name: String,
    pub source: TaskSide,
    pub target: TaskSide,
    #[serde(default)]
    pub conflict_policy: ConflictPolicy,
}

impl TaskSpec {
    /// Map a source key to its target key by swapping the prefix.
    pub fn target_key(&self, source_key: &str) -> String {
        let relative = source_key.strip_prefix(&self.source.prefix).unwrap_or(source_key);
        format!("{}{relative}", self.target.prefix)
    }
}

/// Tunables for one run, supplied by the workbench settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSettings {
    pub object_concurrency: usize,
    pub multipart_threshold: u64,
    pub part_size: u64,
    pub part_concurrency: usize,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self { object_concurrency: 8, multipart_threshold: 64 * 1024 * 1024, part_size: 16 * 1024 * 1024, part_concurrency: 4 }
    }
}

impl RunSettings {
    pub fn sanitized(mut self) -> Self {
        self.object_concurrency = self.object_concurrency.clamp(1, 32);
        self.part_concurrency = self.part_concurrency.clamp(1, 16);
        self.part_size = self.part_size.clamp(5 * 1024 * 1024, 512 * 1024 * 1024);
        self.multipart_threshold = self.multipart_threshold.max(self.part_size);
        self
    }
}

/// Why an object needs transferring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferReason {
    New,
    Changed,
    /// Target exists without a fingerprint: cannot judge, conflict policy applies.
    Unverified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    pub source_key: String,
    pub target_key: String,
    pub size: u64,
    pub reason: TransferReason,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub items: Vec<PlanItem>,
    pub skip_count: u64,
    pub total_bytes: u64,
}

/// The incremental judgement (ADR 0001): identical size plus a matching source
/// fingerprint recorded on the target. Anything else is a conflict.
/// The mtime is stored as Unix seconds: LIST returns millisecond precision
/// while HEAD (RFC 2822) only carries seconds, so a string timestamp would
/// never match across the two sources.
pub fn fingerprint_matches(source: &ObjectMeta, target: &ObjectMeta) -> bool {
    if source.size != target.size {
        return false;
    }
    let Some(etag) = target.user_meta.get(META_SRC_ETAG) else { return false };
    let Some(mtime) = target.user_meta.get(META_SRC_MTIME) else { return false };
    etag == &source.etag && mtime == &source.last_modified.timestamp().to_string()
}

pub fn fingerprint_meta(source: &ObjectMeta) -> HashMap<String, String> {
    let mut meta = HashMap::new();
    meta.insert(META_SRC_ETAG.to_string(), source.etag.clone());
    meta.insert(META_SRC_MTIME.to_string(), source.last_modified.timestamp().to_string());
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(key: &str, size: u64, etag: &str) -> ObjectMeta {
        ObjectMeta {
            key: key.into(),
            size,
            etag: etag.into(),
            last_modified: DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z").unwrap().with_timezone(&Utc),
            user_meta: HashMap::new(),
        }
    }

    fn task() -> TaskSpec {
        TaskSpec {
            name: "test".into(),
            source: TaskSide { connection_id: "a".into(), bucket: "src".into(), prefix: "uploads/".into() },
            target: TaskSide { connection_id: "b".into(), bucket: "dst".into(), prefix: "backup/".into() },
            conflict_policy: ConflictPolicy::default(),
        }
    }

    #[test]
    fn prefix_swap_maps_keys() {
        let task = task();
        assert_eq!(task.target_key("uploads/2025/a.txt"), "backup/2025/a.txt");
        assert_eq!(task.target_key("uploads/"), "backup/");
    }

    #[test]
    fn empty_prefixes_copy_keys_verbatim() {
        let mut task = task();
        task.source.prefix = String::new();
        task.target.prefix = String::new();
        assert_eq!(task.target_key("a/b.txt"), "a/b.txt");
    }

    #[test]
    fn fingerprint_roundtrip_matches() {
        let source = object("k", 10, "abc");
        let mut target = object("k", 10, "target-etag-differs");
        target.user_meta = fingerprint_meta(&source);
        assert!(fingerprint_matches(&source, &target));
    }

    #[test]
    fn fingerprint_survives_list_head_precision_gap() {
        // LIST yields millisecond precision; HEAD (RFC 2822) only seconds.
        let mut listed = object("k", 10, "abc");
        listed.last_modified = DateTime::parse_from_rfc3339("2026-01-02T03:04:05.488Z").unwrap().with_timezone(&Utc);
        let headed = object("k", 10, "abc"); // second precision, same instant
        let mut target = object("k", 10, "whatever");
        target.user_meta = fingerprint_meta(&headed);
        assert!(fingerprint_matches(&listed, &target));
    }

    #[test]
    fn size_mismatch_is_conflict_even_with_fingerprint() {
        let source = object("k", 10, "abc");
        let mut target = object("k", 11, "abc");
        target.user_meta = fingerprint_meta(&source);
        assert!(!fingerprint_matches(&source, &target));
    }

    #[test]
    fn missing_fingerprint_is_unverified() {
        let source = object("k", 10, "abc");
        let target = object("k", 10, "abc");
        assert!(!fingerprint_matches(&source, &target));
    }

    #[test]
    fn changed_source_etag_is_conflict() {
        let source = object("k", 10, "new-etag");
        let mut target = object("k", 10, "old");
        target.user_meta = fingerprint_meta(&object("k", 10, "old-etag"));
        assert!(!fingerprint_matches(&source, &target));
    }

    #[test]
    fn settings_are_clamped() {
        let settings = RunSettings { object_concurrency: 0, part_concurrency: 99, part_size: 1, multipart_threshold: 1 }.sanitized();
        assert_eq!(settings.object_concurrency, 1);
        assert_eq!(settings.part_concurrency, 16);
        assert!(settings.part_size >= 5 * 1024 * 1024);
        assert!(settings.multipart_threshold >= settings.part_size);
    }
}