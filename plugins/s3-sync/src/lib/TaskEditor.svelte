<script>
  import { untrack } from "svelte";
  import { app, showError, upsertTask, loadConnections } from "./state.svelte.js";
  import { rpc } from "./api.js";
  import { makeI18n } from "./i18n.js";

  let { task = null, ondone } = $props();
  const t = makeI18n(app.locale);

  // Form fields start from the task snapshot; later prop changes intentionally
  // do not reset what the user is typing.
  let name = $state(untrack(() => task?.name ?? ""));
  let source = $state(untrack(() => ({ connectionId: task?.source?.connectionId ?? "", bucket: task?.source?.bucket ?? "", prefix: task?.source?.prefix ?? "" })));
  let target = $state(untrack(() => ({ connectionId: task?.target?.connectionId ?? "", bucket: task?.target?.bucket ?? "", prefix: task?.target?.prefix ?? "" })));
  let conflictPolicy = $state(untrack(() => task?.conflictPolicy ?? "sourceAuthority"));
  let sourceBuckets = $state([]);
  let targetBuckets = $state([]);
  let bucketsError = $state({ source: "", target: "" });
  let validation = $state("");
  let bucketRequestId = { source: 0, target: 0 };
  let testing = $state({ source: false, target: false });
  let testResult = $state({ source: null, target: null });

  async function loadBuckets(side, connectionId) {
    if (!connectionId) return;
    const requestId = ++bucketRequestId[side];
    bucketsError[side] = "";
    if (side === "source") sourceBuckets = [];
    else targetBuckets = [];
    try {
      const result = await rpc("s3/listBuckets", { connectionId });
      if (requestId !== bucketRequestId[side]) return;
      if (side === "source") sourceBuckets = result.buckets;
      else targetBuckets = result.buckets;
    } catch (error) {
      if (requestId !== bucketRequestId[side]) return;
      bucketsError[side] = t("loadBucketsFailed");
      if (side === "source") sourceBuckets = [];
      else targetBuckets = [];
    }
  }

  $effect(() => {
    const connectionId = source.connectionId;
    if (connectionId) loadBuckets("source", connectionId);
  });
  $effect(() => {
    const connectionId = target.connectionId;
    if (connectionId) loadBuckets("target", connectionId);
  });

  function onConnectionChange(side, connectionId) {
    testResult[side] = null;
    if (side === "source") {
      source.connectionId = connectionId;
      source.bucket = "";
    } else {
      target.connectionId = connectionId;
      target.bucket = "";
    }
  }

  // The UI never holds credentials, so a live bucket listing against the
  // sidecar session is the connectivity probe (same round trip as test).
  // Unopened connections have no session: ask the host to open one first.
  async function testConnection(side) {
    const connectionId = side === "source" ? source.connectionId : target.connectionId;
    if (!connectionId || testing[side]) return;
    testing[side] = true;
    testResult[side] = null;
    try {
      const conn = app.connections.find((c) => c.id === connectionId);
      if (conn && !conn.connected && typeof window.dbxPlugin.reopenConnection === "function") {
        try {
          await window.dbxPlugin.reopenConnection(connectionId);
          await loadConnections();
        } catch {
          // host refused or API unavailable; the probe below reports it
        }
      }
      const result = await rpc("s3/listBuckets", { connectionId });
      testResult[side] = { ok: true, text: t("testOk").replace("{count}", result.buckets.length) };
      loadBuckets(side, connectionId);
    } catch (error) {
      const notOpen = error?.code === -32010 || /Open the saved S3 connection/i.test(error?.message ?? "");
      testResult[side] = { ok: false, text: notOpen ? t("testNotOpen") : t("testFailed") + (error?.message ?? String(error)) };
    } finally {
      testing[side] = false;
    }
  }

  const sameEndpoint = $derived(
    source.connectionId && source.connectionId === target.connectionId && source.bucket && source.bucket === target.bucket && source.prefix === target.prefix,
  );

  async function save() {
    validation = "";
    if (!name.trim()) return (validation = t("nameRequired"));
    if (!source.connectionId || !source.bucket || !target.connectionId || !target.bucket) return (validation = t("sidesRequired"));
    const saved = {
      id: task?.id ?? crypto.randomUUID(),
      name: name.trim(),
      source: { ...source },
      target: { ...target },
      conflictPolicy,
    };
    try {
      await upsertTask(saved);
      ondone?.();
    } catch (error) {
      showError(error);
    }
  }

  function connectionLabel(connection) {
    const open = connection.connected || connection.isConnected;
    return `${open ? "● " : "○ "}${connection.name}`;
  }
</script>

<div class="card">
  <h2>{task ? t("editTask") : t("newTask")}</h2>
  {#if app.connections.length === 0}
    <p class="muted">{t("noConnections")}</p>
  {:else}
    <p class="muted">{t("openConnectionHint")}</p>
  {/if}

  <label for="task-name">{t("taskName")}</label>
  <input id="task-name" bind:value={name} placeholder={t("taskNamePlaceholder")} />

  <div class="sides">
    <fieldset>
      <legend>{t("source")}</legend>
      <label for="src-conn">{t("connection")}</label>
      <div class="conn-row">
        <select id="src-conn" value={source.connectionId} onchange={(e) => onConnectionChange("source", e.currentTarget.value)}>
          <option value="">{t("selectConnection")}</option>
          {#each app.connections as connection (connection.id)}
            <option value={connection.id}>{connectionLabel(connection)}</option>
          {/each}
        </select>
        <button onclick={() => testConnection("source")} disabled={!source.connectionId || testing.source}>{testing.source ? t("testing") : t("testConnection")}</button>
      </div>
      {#if testResult.source}<p class:warn={!testResult.source.ok} class:muted={testResult.source.ok}>{testResult.source.text}</p>{/if}
      <label for="src-bucket">{t("bucket")}</label>
      <select id="src-bucket" bind:value={source.bucket} disabled={sourceBuckets.length === 0}>
        <option value="">{t("selectBucket")}</option>
        {#each sourceBuckets as bucket (bucket)}
          <option value={bucket}>{bucket}</option>
        {/each}
      </select>
      {#if bucketsError.source}<p class="muted">{bucketsError.source}</p>{/if}
      <label for="src-prefix">{t("prefix")}</label>
      <input id="src-prefix" bind:value={source.prefix} placeholder={t("prefixPlaceholder")} />
    </fieldset>

    <fieldset>
      <legend>{t("target")}</legend>
      <label for="dst-conn">{t("connection")}</label>
      <div class="conn-row">
        <select id="dst-conn" value={target.connectionId} onchange={(e) => onConnectionChange("target", e.currentTarget.value)}>
          <option value="">{t("selectConnection")}</option>
          {#each app.connections as connection (connection.id)}
            <option value={connection.id}>{connectionLabel(connection)}</option>
          {/each}
        </select>
        <button onclick={() => testConnection("target")} disabled={!target.connectionId || testing.target}>{testing.target ? t("testing") : t("testConnection")}</button>
      </div>
      {#if testResult.target}<p class:warn={!testResult.target.ok} class:muted={testResult.target.ok}>{testResult.target.text}</p>{/if}
      <label for="dst-bucket">{t("bucket")}</label>
      <select id="dst-bucket" bind:value={target.bucket} disabled={targetBuckets.length === 0}>
        <option value="">{t("selectBucket")}</option>
        {#each targetBuckets as bucket (bucket)}
          <option value={bucket}>{bucket}</option>
        {/each}
      </select>
      {#if bucketsError.target}<p class="muted">{bucketsError.target}</p>{/if}
      <label for="dst-prefix">{t("prefix")}</label>
      <input id="dst-prefix" bind:value={target.prefix} placeholder={t("prefixPlaceholder")} />
    </fieldset>
  </div>

  <label for="conflict-policy">{t("conflictPolicy")}</label>
  <select id="conflict-policy" bind:value={conflictPolicy}>
    <option value="sourceAuthority">{t("sourceAuthority")}</option>
    <option value="skipExisting">{t("skipExisting")}</option>
  </select>

  {#if sameEndpoint}<p class="warn">{t("sameEndpointWarning")}</p>{/if}
  {#if validation}<p class="warn">{validation}</p>{/if}

  <div class="actions">
    <button class="primary" onclick={save}>{t("save")}</button>
    <button onclick={() => ondone?.()}>{t("cancel")}</button>
    <button class="ghost" onclick={() => loadConnections()}>{t("refresh")}</button>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .sides {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 14px;
  }
  .conn-row {
    display: flex;
    gap: 6px;
    align-items: stretch;
  }
  .conn-row select {
    flex: 1;
    min-width: 0;
  }
  .conn-row button {
    flex-shrink: 0;
    white-space: nowrap;
  }
  fieldset {
    border: 1px solid var(--color-border, #e4e4e7);
    border-radius: var(--radius-md, 8px);
    padding: 10px 12px;
    margin: 12px 0 0;
  }
  legend {
    font-weight: 700;
    padding: 0 6px;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
  .warn {
    color: var(--color-destructive, #dc2626);
  }
  @media (max-width: 640px) {
    .sides {
      grid-template-columns: 1fr;
    }
  }
</style>