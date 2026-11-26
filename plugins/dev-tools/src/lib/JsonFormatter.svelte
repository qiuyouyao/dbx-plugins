<script>
  import { makeI18n } from "./i18n.js";
  import { formatJson, minifyJson, tokenizeJson, JsonToolError } from "./jsonTools.js";

  let { locale = "en" } = $props();
  const t = $derived(makeI18n(locale));
  let input = $state("");
  let output = $state("");
  let error = $state("");
  let indent = $state(2);
  let copied = $state(false);
  const tokens = $derived(output ? tokenizeJson(output) : []);

  function run(operation) {
    try {
      output = operation(input);
      error = "";
      copied = false;
    } catch (failure) {
      output = "";
      copied = false;
      error = failure instanceof JsonToolError && failure.code === "empty" ? t("jsonEmpty") : t("jsonInvalid");
    }
  }

  function format() {
    run((text) => formatJson(text, indent));
  }

  function minify() {
    run(minifyJson);
  }

  function clear() {
    input = "";
    output = "";
    error = "";
    copied = false;
  }

  async function copy() {
    if (!output) return;
    try {
      await navigator.clipboard.writeText(output);
      copied = true;
      error = "";
    } catch {
      error = t("copyFailed");
    }
  }
</script>

<div class="card json-card">
  <div class="tool-head">
    <h2>{t("toolJson")}</h2>
    <span class="muted">{t("toolJsonDesc")}</span>
  </div>

  <label for="json-input">{t("input")}</label>
  <textarea id="json-input" bind:value={input} spellcheck="false" placeholder={'{"key": "value"}'}></textarea>

  <div class="row">
    <label class="check" for="json-indent">{t("indent")}</label>
    <select id="json-indent" bind:value={indent}>
      <option value={2}>2</option>
      <option value={4}>4</option>
    </select>
    <button type="button" class="primary" onclick={format}>{t("format")}</button>
    <button type="button" onclick={minify}>{t("minify")}</button>
    <button type="button" onclick={copy} disabled={!output}>{copied ? t("copied") : t("copy")}</button>
    <button type="button" onclick={clear}>{t("clear")}</button>
  </div>

  <div class="output-label">
    <span class="field-label">{t("output")}</span>
    {#if output}<span class="badge">{t("validJson")}</span>{/if}
  </div>
  <pre id="json-output" class="json-output mono" class:empty={!output}><code>{#if output}{#each tokens as token}<span class="tok-{token.type}">{token.text}</span>{/each}{:else}{t("outputHint")}{/if}</code></pre>
  {#if error}<p class="error">{error}</p>{/if}
</div>
