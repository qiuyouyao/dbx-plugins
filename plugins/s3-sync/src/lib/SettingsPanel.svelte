<script>
  import { app, persistSettings, showError } from "./state.svelte.js";
  import { makeI18n } from "./i18n.js";

  let { onback } = $props();
  const t = $derived(makeI18n(app.locale));

  async function save() {
    try {
      await persistSettings();
      onback?.();
    } catch (error) {
      showError(error);
    }
  }
</script>

<div class="card">
  <h2>{t("settings")}</h2>
  <p class="muted">{t("settingsHint")}</p>
  <div class="grid">
    <div>
      <label for="set-oc">{t("objectConcurrency")}</label>
      <input id="set-oc" type="number" min="1" max="32" bind:value={app.settings.objectConcurrency} />
    </div>
    <div>
      <label for="set-mt">{t("multipartThreshold")}</label>
      <input id="set-mt" type="number" min="5" max="5120" bind:value={app.settings.multipartThresholdMiB} />
    </div>
    <div>
      <label for="set-ps">{t("partSize")}</label>
      <input id="set-ps" type="number" min="5" max="512" bind:value={app.settings.partSizeMiB} />
    </div>
    <div>
      <label for="set-pc">{t("partConcurrency")}</label>
      <input id="set-pc" type="number" min="1" max="16" bind:value={app.settings.partConcurrency} />
    </div>
  </div>
  <div class="actions">
    <button class="primary" onclick={save}>{t("save")}</button>
    <button onclick={() => onback?.()}>{t("back")}</button>
  </div>
</div>

<style>
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 16px;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
  @media (max-width: 640px) {
    .grid {
      grid-template-columns: 1fr;
    }
  }
</style>