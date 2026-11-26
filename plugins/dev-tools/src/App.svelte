<script>
  import { tools } from "./lib/tools.js";
  import { makeI18n } from "./lib/i18n.js";

  let locale = $state("en");
  let activeTool = $state(tools[0].id);
  let ready = $state(false);
  const t = $derived(makeI18n(locale));
  const current = $derived(tools.find((tool) => tool.id === activeTool) ?? tools[0]);

  $effect(() => {
    let disposed = false;
    (async () => {
      await window.dbxPlugin?.ready;
      if (disposed) return;
      locale = window.dbxPlugin?.locale || "en";
      ready = true;
    })();
    const onEnv = (event) => {
      const next = event.detail?.locale;
      if (next) locale = next;
    };
    window.addEventListener("dbx-plugin-env", onEnv);
    return () => {
      disposed = true;
      window.removeEventListener("dbx-plugin-env", onEnv);
    };
  });
</script>

<svelte:head><title>{t("appTitle")}</title></svelte:head>

<main>
  <header>
    <h1>{t("appTitle")}</h1>
    <nav>
      {#each tools as tool (tool.id)}
        <button
          type="button"
          class:active={tool.id === activeTool}
          onclick={() => (activeTool = tool.id)}
        >{t(tool.labelKey)}</button>
      {/each}
    </nav>
  </header>

  {#if !ready}
    <p class="muted">{t("loading")}</p>
  {:else}
    {@const Tool = current.component}
    <Tool {locale} />
  {/if}
</main>
