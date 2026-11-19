<script>
  import { app, showError } from "./state.svelte.js";
  import { rpc, formatBytes, formatTime, taskSummary } from "./api.js";
  import { makeI18n } from "./i18n.js";

  let { onback, onretry } = $props();
  const t = makeI18n(app.locale);

  async function openDetail(runId) {
    try {
      app.historyDetail = await rpc("history/get", { runId });
    } catch (error) {
      showError(error);
    }
  }

  async function retry(runId) {
    app.busy = true;
    try {
      const result = await rpc("task/retry", { runId });
      const detail = app.historyDetail;
      app.run = { runId: result.runId, status: "running", totalCount: detail?.failedCount ?? 0, doneCount: 0, skipCount: 0, failedCount: 0, totalBytes: 0, transferredBytes: 0, failures: [] };
      onretry?.();
    } catch (error) {
      showError(error);
    } finally {
      app.busy = false;
    }
  }
</script>

<div class="card">
  <div class="head">
    <h2>{t("history")}</h2>
    <button onclick={() => onback?.()}>{t("back")}</button>
  </div>

  {#if app.historyDetail}
    {@const detail = app.historyDetail}
    <div class="detail">
      <div class="head">
        <h3>{taskSummary(detail.task)}</h3>
        <button class="ghost" onclick={() => (app.historyDetail = null)}>{t("close")}</button>
      </div>
      <p>
        <span class="badge {detail.status}">{t(detail.status)}</span>
        · {t("startedAt")}: {formatTime(detail.startedAt)} · {t("finishedAt")}: {formatTime(detail.finishedAt)}
        {#if detail.retryOf}· {t("retryOf")}: <span class="mono">{detail.retryOf}</span>{/if}
      </p>
      <p class="muted">
        {detail.doneCount} / {detail.totalCount} {t("objects")} · {formatBytes(detail.transferredBytes)} · {t("failures")}: {detail.failedCount}
      </p>
      {#if detail.failures.length > 0}
        <div class="failures">
          <table>
            <thead>
              <tr><th>{t("failureKey")}</th><th>{t("failureError")}</th></tr>
            </thead>
            <tbody>
              {#each detail.failures as failure (failure.sourceKey)}
                <tr>
                  <td class="mono">{failure.sourceKey}</td>
                  <td class="mono">{failure.error}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <button class="primary" onclick={() => retry(detail.runId)} disabled={app.busy}>{t("retryFailed")}</button>
      {/if}
    </div>
  {/if}

  {#if app.history.length === 0}
    <p class="muted">{t("historyEmpty")}</p>
  {:else}
    <table>
      <thead>
        <tr><th>{t("startedAt")}</th><th>{t("tasks")}</th><th>{t("status")}</th><th>{t("objects")}</th><th>{t("failures")}</th><th></th></tr>
      </thead>
      <tbody>
        {#each app.history as record (record.runId)}
          <tr>
            <td>{formatTime(record.startedAt)}</td>
            <td class="mono">{record.task.name || taskSummary(record.task)}</td>
            <td><span class="badge {record.status}">{t(record.status)}</span></td>
            <td>{record.doneCount} / {record.totalCount}</td>
            <td>{record.failedCount}</td>
            <td><button class="ghost" onclick={() => openDetail(record.runId)}>{t("details")}</button></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>

<style>
  h2 {
    margin: 0;
    font-size: 16px;
  }
  h3 {
    margin: 0;
    font-size: 14px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
    gap: 8px;
  }
  .detail {
    border: 1px solid var(--color-border, #e4e4e7);
    border-radius: var(--radius-md, 8px);
    padding: 12px;
    margin-bottom: 14px;
  }
  .failures {
    max-height: 220px;
    overflow: auto;
    border: 1px solid var(--color-border, #e4e4e7);
    border-radius: var(--radius-md, 8px);
    margin-bottom: 10px;
  }
</style>