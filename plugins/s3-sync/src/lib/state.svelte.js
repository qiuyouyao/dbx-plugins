// Shared reactive state (Svelte 5 runes). Task definitions and settings are
// persisted through host.storage; run history lives in the sidecar.
import { defaultSettings, plain, rpc } from "./api.js";

export const app = $state({
  ready: false,
  locale: "en",
  view: "tasks", // tasks | editor | preview | run | history | settings
  tasks: [],
  settings: { ...defaultSettings },
  connections: [],
  editingTask: null,
  preview: null, // { planId, summary, task }
  run: null, // live RunRecord snapshot
  history: [],
  historyDetail: null,
  error: null,
  busy: false,
});

export function showError(error) {
  app.error = error?.message ? error.message : String(error);
}

// The real DBX host advertises capabilities; the dev host injects the APIs
// without a capabilities object. Detect the actual surface, not just the flag.
function hasStorage() {
  const caps = window.dbxPlugin.capabilities;
  return caps ? !!caps.storage : !!window.dbxPlugin.storage;
}

function hasConnectionsApi() {
  const caps = window.dbxPlugin.capabilities;
  return caps ? !!caps.connectionsApi : typeof window.dbxPlugin.getConnections === "function";
}

export async function loadPersisted() {
  if (!hasStorage()) return;
  const storage = window.dbxPlugin.storage;
  const tasks = await storage.get("tasks");
  if (Array.isArray(tasks)) app.tasks = tasks;
  const settings = await storage.get("settings");
  if (settings && typeof settings === "object") app.settings = { ...defaultSettings, ...settings };
}

export async function persistTasks() {
  if (!hasStorage()) return;
  await window.dbxPlugin.storage.set("tasks", plain(app.tasks));
}

export async function persistSettings() {
  if (!hasStorage()) return;
  await window.dbxPlugin.storage.set("settings", plain(app.settings));
}

export async function loadConnections() {
  // The sidecar's open sessions are authoritative on every host (the dev
  // host mocks host.getConnections with unrelated sample data).
  let sessions = [];
  try {
    const result = await rpc("s3/listSessions");
    sessions = result.sessions || [];
  } catch {
    // sidecar not ready yet; fall through to the host list
  }
  const merged = new Map(sessions.map((s) => [s.connectionId, { id: s.connectionId, name: s.name, connected: true }]));
  if (hasConnectionsApi()) {
    try {
      const all = (await window.dbxPlugin.getConnections()) || [];
      for (const c of all) {
        if (c.dbType !== "s3-sync" && c.providerId !== "io.github.qiuyouyao.s3-sync.connection") continue;
        if (!merged.has(c.id)) merged.set(c.id, { id: c.id, name: c.name, connected: !!(c.connected || c.isConnected) });
      }
    } catch {
      // host list unavailable; sessions alone still populate the dropdowns
    }
  }
  app.connections = [...merged.values()];
}

export function upsertTask(task) {
  const clean = plain(task);
  const index = app.tasks.findIndex((t) => t.id === clean.id);
  if (index >= 0) app.tasks[index] = clean;
  else app.tasks.push(clean);
  return persistTasks();
}

export function removeTask(id) {
  app.tasks = app.tasks.filter((t) => t.id !== id);
  return persistTasks();
}