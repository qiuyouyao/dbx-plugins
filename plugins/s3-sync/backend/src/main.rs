//! S3 Sync sidecar entry: connection lifecycle, bucket/object listing,
//! preview planning, run execution and history. Long runs are spawned on a
//! shared tokio runtime; RPC handlers only block for short operations.

mod engine;
mod model;
mod plan;
mod s3;
mod store;

#[cfg(test)]
mod integration_tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dbx_plugin_sdk::{PluginEmitter, PluginError, PluginHandler, PluginMetadata, PluginServer, RequestContext};
use serde_json::{json, Value};

use engine::RunHandle;
use model::{Plan, PlanItem, RunSettings, TaskSpec, TransferReason};
use s3::{S3Client, S3Config, S3Error};

const PLAN_TTL: Duration = Duration::from_secs(30 * 60);
const PLAN_CACHE_CAP: usize = 16;
const PREVIEW_SAMPLE: usize = 200;

struct CachedPlan {
    plan: Plan,
    task: TaskSpec,
    created: Instant,
}

struct Session {
    client: S3Client,
    name: String,
}

struct Plugin {
    runtime: Arc<tokio::runtime::Runtime>,
    sessions: Mutex<HashMap<String, Session>>,
    plans: Mutex<HashMap<String, CachedPlan>>,
    runs: Mutex<HashMap<String, RunHandle>>,
}

impl Plugin {
    fn new() -> Result<Self, PluginError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .map_err(|e| PluginError::new(-32000, format!("Cannot start async runtime: {e}")))?;
        Ok(Self {
            runtime: Arc::new(runtime),
            sessions: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
        })
    }

    fn session(&self, connection_id: &str) -> Result<S3Client, PluginError> {
        self.sessions
            .lock()
            .map_err(|_| PluginError::new(-32000, "Session registry is poisoned"))?
            .get(connection_id)
            .map(|session| session.client.clone())
            .ok_or_else(|| PluginError::new(-32010, "Open the saved S3 connection in DBX first"))
    }

    fn block<F, T>(&self, future: F) -> Result<T, PluginError>
    where
        F: std::future::Future<Output = Result<T, S3Error>>,
    {
        self.runtime.block_on(future).map_err(s3_to_plugin_error)
    }

    fn cached_plan(&self, plan_id: &str) -> Option<CachedPlan> {
        let mut plans = self.plans.lock().ok()?;
        plans.retain(|_, cached| cached.created.elapsed() < PLAN_TTL);
        plans.remove(plan_id)
    }

    fn store_plan(&self, plan: Plan, task: TaskSpec) -> String {
        let plan_id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut plans) = self.plans.lock() {
            plans.retain(|_, cached| cached.created.elapsed() < PLAN_TTL);
            while plans.len() >= PLAN_CACHE_CAP {
                if let Some(oldest) = plans.iter().min_by_key(|(_, cached)| cached.created).map(|(id, _)| id.clone()) {
                    plans.remove(&oldest);
                }
            }
            plans.insert(plan_id.clone(), CachedPlan { plan, task, created: Instant::now() });
        }
        plan_id
    }

    fn register_run(&self, handle: RunHandle) {
        if let Ok(mut runs) = self.runs.lock() {
            // Keep finished handles for status queries; history is on disk.
            runs.retain(|_, run| run.snapshot().finished_at.is_none());
            runs.insert(handle.snapshot().run_id, handle);
        }
    }

    fn run_handle(&self, run_id: &str) -> Option<RunHandle> {
        self.runs.lock().ok()?.get(run_id).cloned()
    }
}

impl PluginHandler for Plugin {
    fn handle(
        &self,
        _context: RequestContext,
        method: &str,
        params: Value,
        emitter: &PluginEmitter,
    ) -> Result<Value, PluginError> {
        match method {
            "connection/test" => {
                let connection = params.get("connection").cloned().unwrap_or_default();
                let client = S3Client::new(s3_config(&connection)?).map_err(s3_to_plugin_error)?;
                let buckets = self.block(client.list_buckets())?;
                Ok(json!({ "success": true, "message": format!("Connected. {} bucket(s) visible.", buckets.len()) }))
            }
            "connection/connect" => {
                let connection = params.get("connection").ok_or_else(|| PluginError::new(-32602, "Missing connection"))?;
                let connection_id = connection_id(connection)?;
                let name = connection.get("name").and_then(Value::as_str).unwrap_or("S3 connection").to_string();
                let client = S3Client::new(s3_config(connection)?).map_err(s3_to_plugin_error)?;
                self.block(client.list_buckets())?;
                self.sessions
                    .lock()
                    .map_err(|_| PluginError::new(-32000, "Session registry is poisoned"))?
                    .insert(connection_id.to_string(), Session { client, name });
                Ok(json!({ "success": true }))
            }
            "connection/disconnect" => {
                let connection = params.get("connection").cloned().unwrap_or_default();
                let connection_id = connection_id(&connection)?;
                self.sessions
                    .lock()
                    .map_err(|_| PluginError::new(-32000, "Session registry is poisoned"))?
                    .remove(connection_id);
                Ok(json!({ "success": true }))
            }
            "s3/listSessions" => {
                let sessions = self.sessions
                    .lock()
                    .map_err(|_| PluginError::new(-32000, "Session registry is poisoned"))?
                    .iter()
                    .map(|(id, session)| json!({ "connectionId": id, "name": session.name }))
                    .collect::<Vec<_>>();
                Ok(json!({ "sessions": sessions }))
            }
            "s3/listBuckets" => {
                let client = self.session(require_str(&params, "connectionId")?)?;
                let buckets = self.block(client.list_buckets())?;
                Ok(json!({ "buckets": buckets }))
            }
            "s3/listObjects" => {
                let client = self.session(require_str(&params, "connectionId")?)?;
                let bucket = require_str(&params, "bucket")?;
                let prefix = params.get("prefix").and_then(Value::as_str).unwrap_or("");
                let cursor = params.get("cursor").and_then(Value::as_str);
                let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(200).clamp(1, 1000) as u32;
                let listing = self.block(client.list_objects(bucket, prefix, cursor, limit))?;
                Ok(json!({
                    "entries": listing.objects.iter().map(|object| json!({
                        "key": object.key,
                        "size": object.size,
                        "etag": object.etag,
                        "lastModified": object.last_modified.to_rfc3339(),
                    })).collect::<Vec<_>>(),
                    "nextCursor": listing.next_token,
                }))
            }
            "task/preview" => {
                let task: TaskSpec = serde_json::from_value(params.get("task").cloned().unwrap_or_default())
                    .map_err(|e| PluginError::new(-32602, format!("Invalid task: {e}")))?;
                let source = self.session(&task.source.connection_id)?;
                let target = self.session(&task.target.connection_id)?;
                let plan = self.block(plan::build_plan(&source, &target, &task, || false))?;
                let summary = plan_summary(&plan);
                let plan_id = self.store_plan(plan, task);
                Ok(json!({ "planId": plan_id, "summary": summary }))
            }
            "task/run" => {
                let settings = run_settings(&params);
                let (plan, task) = match params.get("planId").and_then(Value::as_str) {
                    Some(plan_id) => {
                        let cached = self.cached_plan(plan_id)
                            .ok_or_else(|| PluginError::new(-32011, "Preview expired; run preview again"))?;
                        (cached.plan, cached.task)
                    }
                    None => {
                        let task: TaskSpec = serde_json::from_value(params.get("task").cloned().unwrap_or_default())
                            .map_err(|e| PluginError::new(-32602, format!("Invalid task: {e}")))?;
                        let source = self.session(&task.source.connection_id)?;
                        let target = self.session(&task.target.connection_id)?;
                        let plan = self.block(plan::build_plan(&source, &target, &task, || false))?;
                        (plan, task)
                    }
                };
                let source = self.session(&task.source.connection_id)?;
                let target = self.session(&task.target.connection_id)?;
                let handle = engine::start_run(
                    &self.runtime.handle(),
                    source,
                    target,
                    task,
                    plan,
                    settings,
                    None,
                    event_sink(emitter),
                );
                let run_id = handle.snapshot().run_id;
                self.register_run(handle);
                Ok(json!({ "runId": run_id }))
            }
            "task/cancel" => {
                let run_id = require_str(&params, "runId")?;
                match self.run_handle(run_id) {
                    Some(handle) => {
                        handle.cancel();
                        Ok(json!({ "success": true }))
                    }
                    None => Err(PluginError::new(-32012, "Run is not active")),
                }
            }
            "task/status" => {
                let run_id = require_str(&params, "runId")?;
                if let Some(handle) = self.run_handle(run_id) {
                    return Ok(serde_json::to_value(handle.snapshot()).expect("record serializes"));
                }
                match self.runtime.block_on(store::load_run(run_id)) {
                    Some(record) => Ok(serde_json::to_value(record).expect("record serializes")),
                    None => Err(PluginError::new(-32012, "Unknown run")),
                }
            }
            "task/retry" => {
                let run_id = require_str(&params, "runId")?;
                let record = match self.run_handle(run_id) {
                    Some(handle) => handle.snapshot(),
                    None => self.runtime.block_on(store::load_run(run_id))
                        .ok_or_else(|| PluginError::new(-32012, "Unknown run"))?,
                };
                if record.failures.is_empty() {
                    return Err(PluginError::new(-32013, "Nothing to retry"));
                }
                let items: Vec<PlanItem> = record.failures.iter().map(|failure| PlanItem {
                    source_key: failure.source_key.clone(),
                    target_key: failure.target_key.clone(),
                    size: failure.size,
                    reason: TransferReason::Changed,
                }).collect();
                let total_bytes = items.iter().map(|item| item.size).sum();
                let plan = Plan { items, skip_count: 0, total_bytes };
                let settings = if params.get("settings").is_some() { run_settings(&params) } else { record.settings.clone() };
                let source = self.session(&record.task.source.connection_id)?;
                let target = self.session(&record.task.target.connection_id)?;
                let handle = engine::start_run(
                    &self.runtime.handle(),
                    source,
                    target,
                    record.task.clone(),
                    plan,
                    settings,
                    Some(run_id.to_string()),
                    event_sink(emitter),
                );
                let new_run_id = handle.snapshot().run_id;
                self.register_run(handle);
                Ok(json!({ "runId": new_run_id }))
            }
            "history/list" => {
                let records = self.runtime.block_on(store::list_runs());
                Ok(json!({ "runs": records }))
            }
            "history/get" => {
                let run_id = require_str(&params, "runId")?;
                match self.runtime.block_on(store::load_run(run_id)) {
                    Some(record) => Ok(serde_json::to_value(record).expect("record serializes")),
                    None => Err(PluginError::new(-32012, "Unknown run")),
                }
            }
            _ => Err(PluginError::method_not_found(method)),
        }
    }
}

fn plan_summary(plan: &Plan) -> Value {
    let count = |reason: TransferReason| plan.items.iter().filter(|item| item.reason == reason).count();
    json!({
        "transferCount": plan.items.len(),
        "skipCount": plan.skip_count,
        "totalBytes": plan.total_bytes,
        "newCount": count(TransferReason::New),
        "changedCount": count(TransferReason::Changed),
        "unverifiedCount": count(TransferReason::Unverified),
        "sample": plan.items.iter().take(PREVIEW_SAMPLE).collect::<Vec<_>>(),
    })
}

fn run_settings(params: &Value) -> RunSettings {
    params.get("settings")
        .cloned()
        .and_then(|value| serde_json::from_value::<RunSettings>(value).ok())
        .unwrap_or_default()
        .sanitized()
}

fn s3_config(connection: &Value) -> Result<S3Config, PluginError> {
    let config = connection.get("external_config").cloned().unwrap_or_default();
    let secrets = connection.get("connection_secrets").cloned().unwrap_or_default();
    let field = |key: &str| config.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let secret_field = |key: &str| secrets.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let endpoint = field("endpoint");
    let access_key = secret_field("access_key");
    let secret_key = secret_field("secret_key");
    if endpoint.is_empty() || access_key.is_empty() || secret_key.is_empty() {
        return Err(PluginError::new(-32602, "Endpoint, access key and secret key are required"));
    }
    let region = {
        let region = field("region");
        if region.is_empty() { "us-east-1".to_string() } else { region }
    };
    Ok(S3Config {
        endpoint,
        region,
        access_key,
        secret_key,
        path_style: field("addressing_style") != "virtual",
        insecure_tls: config.get("insecure_tls").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn connection_id(connection: &Value) -> Result<&str, PluginError> {
    connection.get("id").and_then(Value::as_str)
        .ok_or_else(|| PluginError::new(-32602, "Missing connection id"))
}

fn require_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, PluginError> {
    params.get(key).and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PluginError::new(-32602, format!("Missing {key}")))
}

fn s3_to_plugin_error(error: S3Error) -> PluginError {
    PluginError::new(-32000, error.to_string())
}

fn event_sink(emitter: &PluginEmitter) -> engine::EventSink {
    let emitter = emitter.clone();
    Arc::new(move |method, params| {
        let _ = emitter.event(method, params);
    })
}

fn main() -> std::io::Result<()> {
    let plugin = Plugin::new().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.message))?;
    let metadata = PluginMetadata::new("io.github.qiuyouyao.s3-sync", env!("CARGO_PKG_VERSION"))
        .with_capability("connections")
        .with_capability("events");
    PluginServer::new(metadata, plugin).serve()
}