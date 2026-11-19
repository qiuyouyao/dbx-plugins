<script>
  import { app, showError } from "./state.svelte.js";
  import { rpc, formatBytes, settingsToWire } from "./api.js";
  import { makeI18n } from "./i18n.js";

  let { onback } = $props();
  const t = makeI18n(app.locale);
  const run = $derived(app.run);
  const finished = $derived(run && run.status !== "running");
  const processed = $derived(run ? run.doneCount + run.failedCount : 0);
  const percent = $derived(run && run.totalCount > 0 ? Math.round((processed / run.totalCount) * 100) : 0);

  async function cancelRun() {
    try {
      await rpc("task/cancel", { runId: run.runId });
    } catch (error) {
      showError(error);
    }
  }

  async function retryFailed() {
    app.busy = true;
    try {
      const result = await rpc("task/retry", { runId: run.runId, settings: settingsToWire(app.settings) });
      app.run = { runId: result.runId, status: "running", totalCount: run.failedCount, doneCount: 0, skipCount: 0, failedCount: 0, totalBytes: 0, transferredBytes: 0, failures: [] };
    } catch (error) {
      showError(error);
    } finally {
      app.busy = false;
    }
  }
</script>

{#if run}
  <div class="card">
    <div class="head">
      <h2>{t("progress")}</h2>
      <span class="badge {run.status}">{t(run.status)}</span>
    </div>

    <div class="progress-track"><div class="progress-fill" style="width:{percent}%"></div></div>
    <p class="muted">
      {processed} / {run.totalCount} {t("objects")} · {t("transferred")}: {formatBytes(run.transferredBytes)} / {formatBytes(run.totalBytes)}
      {#if run.skipCount > 0}· {t("skipCount")}: {run.skipCount}{/if}
    </p>

    {#if !finished}
      <button class="destructive" onclick={cancelRun}>{t("cancelRun")}</button>
    {:else}
      <p>{t("runFinished")}</p>
      <h3>{t("failures")} ({run.failedCount})</h3>
      {#if run.failures.length === 0}
        <p class="muted">{t("noFailures")}</p>
      {:else}
        <div class="failures">
          <table>
            <thead>
              <tr><th>{t("failureKey")}</th><th>{t("failureError")}</th></tr>
            </thead>
            <tbody>
              {#each run.failures as failure (failure.sourceKey)}
                <tr>
                  <td class="mono">{failure.sourceKey}</td>
                  <td class="mono">{failure.error}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
        <button class="primary" onclick={retryFailed} disabled={app.busy}>{t("retryFailed")}</button>
      {/if}
      <div class="actions">
        <button onclick={() => onback?.()}>{t("back")}</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  h2 {
    margin: 0;
    font-size: 16px;
  }
  h3 {
    font-size: 13px;
    margin: 16px 0 6px;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }
  .failures {
    max-height: 260px;
    overflow: auto;
    border: 1px solid var(--color-border, #e4e4e7);
    border-radius: var(--radius-md, 8px);
    margin-bottom: 12px;
  }
  .actions {
    margin-top: 12px;
  }
</style>