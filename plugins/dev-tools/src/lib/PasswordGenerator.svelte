<script>
  import { makeI18n } from "./i18n.js";
  import { generatePassword, estimateEntropy, PasswordToolError } from "./passwordTools.js";

  let { locale = "en" } = $props();
  const t = $derived(makeI18n(locale));
  let length = $state(20);
  let lower = $state(true);
  let upper = $state(true);
  let digits = $state(true);
  let symbols = $state(false);
  let excludeAmbiguous = $state(true);
  let password = $state("");
  let error = $state("");
  let copied = $state(false);

  const options = $derived({ length, lower, upper, digits, symbols, excludeAmbiguous });
  const bits = $derived(estimateEntropy(options));
  const strength = $derived(bits < 40 ? t("weak") : bits < 70 ? t("medium") : t("strong"));

  function regenerate() {
    try {
      password = generatePassword(options);
      error = "";
      copied = false;
    } catch (failure) {
      password = "";
      copied = false;
      error = failure instanceof PasswordToolError && failure.code === "noCharset" ? t("noCharset") : t("genFailed");
    }
  }

  async function copy() {
    if (!password) return;
    try {
      await navigator.clipboard.writeText(password);
      copied = true;
      error = "";
    } catch {
      error = t("copyFailed");
    }
  }
</script>

<div class="card">
  <div class="tool-head">
    <h2>{t("toolPassword")}</h2>
    <span class="muted">{t("strength")}: {strength} ({bits} bits)</span>
  </div>

  <label for="password-length">{t("length")}: {length}</label>
  <input id="password-length" type="range" min="8" max="64" bind:value={length} />

  <label class="check"><input type="checkbox" bind:checked={lower} />{t("lowercase")}</label>
  <label class="check"><input type="checkbox" bind:checked={upper} />{t("uppercase")}</label>
  <label class="check"><input type="checkbox" bind:checked={digits} />{t("digits")}</label>
  <label class="check"><input type="checkbox" bind:checked={symbols} />{t("symbols")}</label>
  <label class="check"><input type="checkbox" bind:checked={excludeAmbiguous} />{t("excludeAmbiguous")}</label>

  <div class="row">
    <button type="button" class="primary" onclick={regenerate}>{t("regenerate")}</button>
    <button type="button" onclick={copy} disabled={!password}>{copied ? t("copied") : t("copy")}</button>
  </div>

  <pre class="mono password-result">{password || "—"}</pre>
  {#if error}<p class="error">{error}</p>{/if}
</div>
