<script>
  import { app, showError, loadPersisted, loadConnections, removeTask } from "./lib/state.svelte.js";
  import { rpc, taskSummary } from "./lib/api.js";
  import { makeI18n } from "./lib/i18n.js";
  import TaskEditor from "./lib/TaskEditor.svelte";
  import PreviewPanel from "./lib/PreviewPanel.svelte";
  import RunPanel from "./lib/RunPanel.svelte";
  import HistoryPanel from "./lib/HistoryPanel.svelte";
  import SettingsPanel from "./lib/SettingsPanel.svelte";

  const t = $derived(makeI18n(app.locale));
  let deletingId = $state(null);
  let offEvent = null;
  let offContext = null;

  $effect(() => {
    let disposed = false;
    (async () => {
      await window.dbxPlugin.ready;
      if (disposed) return;
      app.locale = window.dbxPlugin.locale || "en";
      await loadPersisted();
      try {
        await loadConnections();
      } catch (error) {
        showError(error);
      }
      // A workbench opened from a connection carries its id: prefill source.
      const context = window.dbxPlugin.context || {};
      if (context.connectionId && app.tasks.length === 0) {
        app.editingTask = { source: { connectionId: context.connectionId } };
        app.view = "editor";
      }
      offEvent = window.dbxPlugin.onEvent((event) => {
        if (event?.method !== "s3-sync/progress") return;
        const params = event.params || {};
        if (app.run && params.runId === app.run.runId) {
          app.run = { ...params };
          if (params.status !== "running") refreshHistory();
        }
      });
      offContext = window.dbxPlugin.onContext(() => {});
      app.ready = true;
    })();
    const onEnv = (event) => {
      const locale = event.detail?.locale;
      if (locale) app.locale = locale;
    };
    window.addEventListener("dbx-plugin-env", onEnv);
    return () => {
      disposed = true;
      offEvent?.();
      offContext?.();
      window.removeEventListener("dbx-plugin-env", onEnv);
    };
  });

  async function refreshHistory() {
    try {
      const result = await rpc("history/list");
      app.history = result.runs || [];
    } catch {
      // history is best-effort; the run panel already shows live state
    }
  }

  async function previewTask(task) {
    app.busy = true;
    app.error = null;
    try {
      const result = await rpc("task/preview", { task }, 300000);
      app.preview = { planId: result.planId, summary: result.summary, task };
      app.view = "preview";
    } catch (error) {
      showError(error);
    } finally {
      app.busy = false;
    }
  }

  async function openHistory() {
    app.historyDetail = null;
    await refreshHistory();
    app.view = "history";
  }

  async function confirmDelete(id) {
    try {
      await removeTask(id);
    } catch (error) {
      showError(error);
    }
    deletingId = null;
  }
</script>

<main>
  <header>
    <h1>{t("appTitle")}</h1>
    <nav>
      <button class:ghost={app.view !== "tasks"} onclick={() => (app.view = "tasks")}>{t("tasks")}</button>
      <button class:ghost={app.view !== "history"} onclick={openHistory}>{t("history")}</button>
      <button class:ghost={app.view !== "settings"} onclick={() => (app.view = "settings")}>{t("settings")}</button>
    </nav>
  </header>

  {#if app.error}
    <div class="error-banner">
      <span>{app.error}</span>
      <button class="ghost" onclick={() => (app.error = null)}>{t("dismiss")}</button>
    </div>
  {/if}

  {#if !app.ready}
    <p class="muted">{t("loading")}</p>
  {:else if app.view === "editor"}
    <TaskEditor task={app.editingTask} ondone={() => (app.view = "tasks")} />
  {:else if app.view === "preview"}
    <PreviewPanel onback={() => (app.view = "tasks")} onstarted={() => (app.view = "run")} />
  {:else if app.view === "run"}
    <RunPanel onback={() => (app.view = "tasks")} />
  {:else if app.view === "history"}
    <HistoryPanel onback={() => (app.view = "tasks")} onretry={() => (app.view = "run")} />
  {:else if app.view === "settings"}
    <SettingsPanel onback={() => (app.view = "tasks")} />
  {:else}
    <div class="toolbar">
      <button class="primary" onclick={() => { app.editingTask = null; app.view = "editor"; loadConnections(); }}>{t("newTask")}</button>
    </div>
    {#if app.tasks.length === 0}
      <p class="muted">{t("noTasks")}</p>
    {/if}
    <div class="task-list">
      {#each app.tasks as task (task.id)}
        <div class="card task">
          <div class="task-head">
            <strong>{task.name}</strong>
            <span class="badge">{task.conflictPolicy === "skipExisting" ? t("skipExisting") : t("sourceAuthority")}</span>
          </div>
          <p class="mono muted">{taskSummary(task)}</p>
          {#if deletingId === task.id}
            <p>{t("deleteConfirm")}</p>
            <div class="task-actions">
              <button class="destructive" onclick={() => confirmDelete(task.id)}>{t("delete")}</button>
              <button onclick={() => (deletingId = null)}>{t("cancel")}</button>
            </div>
          {:else}
            <div class="task-actions">
              <button class="primary" onclick={() => previewTask(task)} disabled={app.busy}>{t("preview")}</button>
              <button onclick={() => { app.editingTask = task; app.view = "editor"; loadConnections(); }}>{t("edit")}</button>
              <button class="ghost" onclick={() => (deletingId = task.id)}>{t("delete")}</button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</main>

<style>
  main {
    max-width: 980px;
    margin: 0 auto;
    padding: 20px clamp(12px, 3vw, 32px) 48px;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
    flex-wrap: wrap;
  }
  h1 {
    font-size: 18px;
    margin: 0;
  }
  nav {
    display: flex;
    gap: 6px;
  }
  .toolbar {
    margin-bottom: 12px;
  }
  .task-list {
    display: grid;
    gap: 10px;
  }
  .task-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .task p {
    margin: 6px 0 10px;
  }
  .task-actions {
    display: flex;
    gap: 8px;
  }
</style>