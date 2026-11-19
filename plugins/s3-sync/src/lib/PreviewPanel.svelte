<script>
  import { app, showError } from "./state.svelte.js";
  import { rpc, formatBytes, settingsToWire } from "./api.js";
  import { makeI18n } from "./i18n.js";

  let { onback, onstarted } = $props();
  const t = makeI18n(app.locale);
  const summary = $derived(app.preview?.summary);

  async function start() {
    app.busy = true;
    try {
      const result = await rpc("task/run", { planId: app.preview.planId, settings: settingsToWire(app.settings) });
      app.run = { runId: result.runId, status: "running", totalCount: summary.transferCount, doneCount: 0, skipCount: summary.skipCount, failedCount: 0, totalBytes: summary.totalBytes, transferredBytes: 0, failures: [] };
      onstarted?.();
    } catch (error) {
      showError(error);
    } finally {
      app.busy = false;
    }
  }
</script>

<div class="card">
  <h2>{t("previewTitle")} — {app.preview?.task?.name ?? ""}</h2>
  {#if summary}
    <div class="stats">
      <div><strong>{summary.transferCount}</strong><span class="muted">{t("transferCount")}</span></div>
      <div><strong>{summary.skipCount}</strong><span class="muted">{t("skipCount")}</span></div>
      <div><strong>{formatBytes(summary.totalBytes)}</strong><span class="muted">{t("totalBytes")}</span></div>
    </div>
    <p class="muted">
      {t("newCount")}: {summary.newCount} · {t("changedCount")}: {summary.changedCount} · {t("unverifiedCount")}: {summary.unverifiedCount}
    </p>

    {#if summary.transferCount === 0}
      <p>{t("previewEmpty")}</p>
    {:else}
      <h3>{t("sampleKeys")}</h3>
      <div class="sample">
        <table>
          <thead>
            <tr><th>{t("key")}</th><th>{t("size")}</th><th>{t("reason")}</th></tr>
          </thead>
          <tbody>
            {#each summary.sample as item (item.sourceKey)}
              <tr>
                <td class="mono">{item.sourceKey}</td>
                <td>{formatBytes(item.size)}</td>
                <td><span class="badge">{item.reason}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}

  <div class="actions">
    <button class="primary" onclick={start} disabled={app.busy || !summary || summary.transferCount === 0}>{t("startMigration")}</button>
    <button onclick={() => onback?.()}>{t("back")}</button>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 12px;
    font-size: 16px;
  }
  h3 {
    font-size: 13px;
    margin: 16px 0 6px;
  }
  .stats {
    display: flex;
    gap: 28px;
    margin-bottom: 6px;
  }
  .stats div {
    display: flex;
    flex-direction: column;
  }
  .stats strong {
    font-size: 20px;
  }
  .sample {
    max-height: 300px;
    overflow: auto;
    border: 1px solid var(--color-border, #e4e4e7);
    border-radius: var(--radius-md, 8px);
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
</style>