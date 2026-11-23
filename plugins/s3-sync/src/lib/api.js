// Thin RPC wrapper plus display helpers shared by all panels.

// Svelte 5 $state values are reactive Proxies that structured-clone (the
// host bridge's postMessage) rejects. Snapshot to plain JSON before crossing
// the sandbox boundary.
export function plain(value) {
  return JSON.parse(JSON.stringify(value ?? null));
}

export function rpc(method, params = {}, timeoutMs = 120000) {
  return window.dbxPlugin.invoke(method, plain(params), { timeoutMs });
}

export function formatBytes(bytes) {
  const value = Number(bytes) || 0;
  if (value < 1024) return `${value} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let scaled = value;
  let unit = -1;
  do {
    scaled /= 1024;
    unit += 1;
  } while (scaled >= 1024 && unit < units.length - 1);
  return `${scaled.toFixed(scaled >= 100 ? 0 : 1)} ${units[unit]}`;
}

export function formatTime(iso, locale) {
  if (!iso) return "—";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString(locale || undefined);
}

export function taskSummary(task) {
  const side = (s) => `${s.bucket}${s.prefix ? `/${s.prefix.replace(/\/$/, "")}` : ""}`;
  return `${side(task.source)} → ${side(task.target)}`;
}

export const MIB = 1024 * 1024;

export function settingsToWire(settings) {
  return {
    objectConcurrency: Number(settings.objectConcurrency) || 8,
    multipartThreshold: (Number(settings.multipartThresholdMiB) || 64) * MIB,
    partSize: (Number(settings.partSizeMiB) || 16) * MIB,
    partConcurrency: Number(settings.partConcurrency) || 4,
  };
}

export const defaultSettings = {
  objectConcurrency: 8,
  multipartThresholdMiB: 64,
  partSizeMiB: 16,
  partConcurrency: 4,
};