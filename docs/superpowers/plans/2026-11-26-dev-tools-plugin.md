# Dev Tools 小工具插件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `plugins/dev-tools` 纯前端工作台插件，首版提供 JSON 格式化与密码生成器，并留出后续小工具的扩展点。

**Architecture:** 使用官方 CLI 的 `svelte` 模板生成纯前端（universal）插件；所有功能在沙箱 UI 内用纯 JS 实现，不引入 Rust sidecar、不申请存储/连接权限；核心逻辑拆成可单测的纯函数模块，UI 通过工具注册表挂载面板，新增工具只需加一个组件和一条注册项。

**Tech Stack:** DBX Plugin CLI 0.1.10、Svelte 5 runes、Vite 7、Node `node:test`、GitHub Actions reusable workflow。

---

### Task 1: 生成插件骨架并收紧 manifest

**Files:**
- Create（CLI 生成）: `plugins/dev-tools/`（manifest.json、package.json、dbx-plugin.toml、vite.config.js、index.html、src/main.js、assets/plugin.svg、README.md）
- Modify: `plugins/dev-tools/manifest.json`
- Modify: `plugins/dev-tools/vite.config.js`

- [x] **Step 1: 用官方 CLI 生成骨架**

```powershell
dbx-plugin create plugins/dev-tools --template svelte --id io.github.qiuyouyao.dev-tools --name "Dev Tools" --publisher qiuyouyao --description "Small offline developer utilities in one workbench: JSON formatting, password generation, and more." --version 0.1.0 -y
```

Expected: `Success: Created Svelte + Vite (universal) plugin at plugins/dev-tools`，无 `backend/` 目录。

- [x] **Step 2: 替换 manifest 为最小权限 + 工作台/命令/菜单贡献点**

完整内容：

```json
{
  "$schema": "https://raw.githubusercontent.com/t8y2/dbx/main/plugins/manifest.schema.json",
  "manifest_version": 1,
  "id": "io.github.qiuyouyao.dev-tools",
  "name": "Dev Tools",
  "version": "0.1.0",
  "publisher": "qiuyouyao",
  "description": "Small offline developer utilities in one workbench: JSON formatting, password generation, and more.",
  "icon": "assets/plugin.svg",
  "permissions": ["host.workbench"],
  "engines": { "dbx": ">=0.5.68", "host_api": "1" },
  "entrypoints": { "ui": { "root": "ui", "entry": "ui/index.html" } },
  "contributions": [
    {
      "type": "workbench",
      "id": "io.github.qiuyouyao.dev-tools.workbench",
      "label": "Dev Tools",
      "description": "Offline JSON formatting, password generation and other small developer utilities.",
      "icon": "assets/plugin.svg"
    },
    {
      "type": "command",
      "id": "io.github.qiuyouyao.dev-tools.open",
      "label": "Open Dev Tools",
      "description": "Open the Dev Tools workbench.",
      "icon": "assets/plugin.svg",
      "action": {
        "type": "open-workbench",
        "workbench": "io.github.qiuyouyao.dev-tools.workbench",
        "presentation": "tab"
      }
    },
    {
      "type": "menus",
      "id": "io.github.qiuyouyao.dev-tools.menus",
      "items": [
        {
          "location": "appToolbar",
          "command": "io.github.qiuyouyao.dev-tools.open",
          "group": "primary",
          "order": 20,
          "default_visible": true
        },
        {
          "location": "commandPalette",
          "command": "io.github.qiuyouyao.dev-tools.open",
          "group": "primary",
          "order": 20
        }
      ]
    }
  ],
  "localizations": {
    "zh-CN": {
      "name": "开发工具箱",
      "description": "离线小工具工作台：JSON 格式化、密码生成器，以及后续更多小功能。",
      "contributions": {
        "io.github.qiuyouyao.dev-tools.workbench": {
          "label": "开发工具箱",
          "description": "离线 JSON 格式化、密码生成等开发者小工具。"
        },
        "io.github.qiuyouyao.dev-tools.open": {
          "label": "打开开发工具箱",
          "description": "打开开发工具箱工作台。"
        }
      }
    }
  }
}
```

- [x] **Step 3: 修复沙箱资源相对路径**

在 `vite.config.js` 的 `defineConfig` 中加 `base: "./",`（与 `plugins/s3-sync/vite.config.js` 一致；宿主注入 per-plugin base，绝对路径 `/assets/...` 会 404）。同时把 `dbx-plugin.toml` 的 `[dev]` 换成与 s3-sync 相同的 pwsh 包装命令，规避 Windows dev 宿主的 `\\?\` cwd 问题。

---

### Task 2: 纯函数核心模块与单元测试（TDD）

**Files:**
- Create: `plugins/dev-tools/src/lib/jsonTools.js`
- Create: `plugins/dev-tools/src/lib/passwordTools.js`
- Create: `plugins/dev-tools/src/lib/tools.test.js`
- Modify: `plugins/dev-tools/package.json`

- [x] **Step 1: 写失败测试**

`src/lib/tools.test.js` 完整内容（测试脚本使用显式文件路径 `node --test src/lib/tools.test.js`，避免 Node 把目录当模块、也兼容 Windows cmd 不展开 glob）：

```js
import test from "node:test";
import assert from "node:assert/strict";
import { formatJson, minifyJson, JsonToolError } from "./jsonTools.js";
import { selectedSets, generatePassword, estimateEntropy, PasswordToolError } from "./passwordTools.js";

function seeded(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state * 1664525 + 1013904223) >>> 0;
    return state;
  };
}

test("formatJson pretty prints with two spaces", () => {
  assert.equal(formatJson('{"a":1,"b":[2,3]}'), '{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}');
});

test("formatJson honors a four-space indent", () => {
  assert.equal(formatJson('{"a":1}', 4), '{\n    "a": 1\n}');
});

test("formatJson rejects empty input", () => {
  assert.throws(() => formatJson("   "), (error) => error instanceof JsonToolError && error.code === "empty");
});

test("formatJson rejects invalid input", () => {
  assert.throws(() => formatJson("{oops}"), (error) => error instanceof JsonToolError && error.code === "invalid");
});

test("minifyJson removes insignificant whitespace", () => {
  assert.equal(minifyJson('{ "a" : 1 }'), '{"a":1}');
});

test("selectedSets honors flags and the ambiguous-character filter", () => {
  assert.deepEqual(selectedSets({ lower: true, upper: false, digits: false, symbols: false }), ["abcdefghijklmnopqrstuvwxyz"]);
  const digits = selectedSets({ lower: false, upper: false, digits: true, symbols: false, excludeAmbiguous: true });
  assert.equal(digits.length, 1);
  assert.ok(!digits[0].includes("0"));
});

test("generatePassword is deterministic with an injected RNG", () => {
  const options = { length: 4, lower: true, upper: false, digits: false, symbols: false };
  assert.equal(generatePassword(options, () => 0), "aaaa");
});

test("generatePassword respects length and selected character sets", () => {
  const options = { length: 32, lower: true, upper: true, digits: true, symbols: false };
  const password = generatePassword(options, seeded(7));
  const allowed = new Set(selectedSets(options).join(""));
  assert.equal(password.length, 32);
  assert.ok([...password].every((char) => allowed.has(char)));
});

test("generatePassword rejects an empty charset", () => {
  const options = { length: 12, lower: false, upper: false, digits: false, symbols: false };
  assert.throws(() => generatePassword(options), (error) => error instanceof PasswordToolError && error.code === "noCharset");
});

test("estimateEntropy grows with length", () => {
  const options = { length: 8, lower: true, upper: true, digits: true, symbols: false };
  assert.ok(estimateEntropy({ ...options, length: 16 }) > estimateEntropy(options));
});
```

在 `package.json` 加 `"test": "node --test src/lib/tools.test.js"`。

- [x] **Step 2: 运行测试确认失败**

```powershell
cd plugins/dev-tools; npm test
```

Expected: 模块找不到而失败。

- [x] **Step 3: 实现 JSON 核心**

`src/lib/jsonTools.js`：

```js
export class JsonToolError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "JsonToolError";
    this.code = code;
  }
}

export function parseJson(input) {
  const text = typeof input === "string" ? input.trim() : "";
  if (!text) throw new JsonToolError("empty", "Input is empty");
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new JsonToolError("invalid", error.message);
  }
}

export function formatJson(input, spaces = 2) {
  const indent = Math.max(0, Math.min(8, Number(spaces) || 2));
  return JSON.stringify(parseJson(input), null, indent);
}

export function minifyJson(input) {
  return JSON.stringify(parseJson(input));
}
```

- [x] **Step 4: 实现密码核心**

`src/lib/passwordTools.js`：

```js
const SETS = {
  lower: "abcdefghijklmnopqrstuvwxyz",
  upper: "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
  digits: "0123456789",
  symbols: "!@#$%^&*()-_=+[]{};:,.?/",
};
const AMBIGUOUS = new Set(["O", "0", "I", "l", "1", "|", "`", "\"", "'"]);

export class PasswordToolError extends Error {
  constructor(code, message) {
    super(message);
    this.name = "PasswordToolError";
    this.code = code;
  }
}

function clean(chars, excludeAmbiguous) {
  return excludeAmbiguous ? [...chars].filter((char) => !AMBIGUOUS.has(char)).join("") : chars;
}

export function selectedSets(options = {}) {
  const flags = {
    lower: options.lower !== false,
    upper: options.upper !== false,
    digits: options.digits !== false,
    symbols: !!options.symbols,
  };
  const exclude = !!options.excludeAmbiguous;
  return Object.keys(SETS)
    .filter((key) => flags[key])
    .map((key) => clean(SETS[key], exclude))
    .filter((chars) => chars.length > 0);
}

export function buildAlphabet(options = {}) {
  return [...new Set(selectedSets(options).join(""))].join("");
}

function defaultRandomUint32() {
  if (!globalThis.crypto?.getRandomValues) throw new PasswordToolError("noCrypto", "Web Crypto unavailable");
  return globalThis.crypto.getRandomValues(new Uint32Array(1))[0];
}

function unbiasedIndex(max, random) {
  const limit = Math.floor(0x100000000 / max) * max;
  let value = 0;
  do {
    value = random() >>> 0;
  } while (value >= limit);
  return value % max;
}

function pick(chars, random) {
  return chars[unbiasedIndex(chars.length, random)];
}

export function generatePassword(options = {}, random = defaultRandomUint32) {
  const sets = selectedSets(options);
  if (sets.length === 0) throw new PasswordToolError("noCharset", "No character type selected");
  const length = Math.max(4, Math.min(128, Number(options.length) || 20));
  if (length < sets.length) throw new PasswordToolError("lengthTooSmall", "Length is too small for the selected types");
  const alphabet = sets.join("");
  const chars = sets.map((set) => pick(set, random));
  while (chars.length < length) chars.push(pick(alphabet, random));
  for (let i = chars.length - 1; i > 0; i -= 1) {
    const j = unbiasedIndex(i + 1, random);
    [chars[i], chars[j]] = [chars[j], chars[i]];
  }
  return chars.join("");
}

export function estimateEntropy(options = {}) {
  const alphabet = buildAlphabet(options);
  const length = Math.max(4, Math.min(128, Number(options.length) || 20));
  return alphabet ? Math.round(length * Math.log2(alphabet.length)) : 0;
}
```

- [x] **Step 5: 运行测试确认通过**

```powershell
cd plugins/dev-tools; npm test
```

Expected: 10 个测试全部 PASS。

---

### Task 3: 工作台 UI 与工具注册表

**Files:**
- Create: `plugins/dev-tools/src/lib/i18n.js`
- Create: `plugins/dev-tools/src/lib/tools.js`
- Create: `plugins/dev-tools/src/lib/JsonFormatter.svelte`
- Create: `plugins/dev-tools/src/lib/PasswordGenerator.svelte`
- Create: `plugins/dev-tools/src/app.css`
- Modify: `plugins/dev-tools/src/App.svelte`
- Modify: `plugins/dev-tools/src/main.js`
- Modify: `plugins/dev-tools/index.html`

- [x] **Step 1: 写 i18n 字典**

`src/lib/i18n.js`：

```js
const dictionaries = {
  en: {
    appTitle: "Dev Tools",
    loading: "Loading…",
    toolJson: "JSON Formatter",
    toolJsonDesc: "Pretty-print, minify and validate JSON.",
    toolPassword: "Password Generator",
    toolPasswordDesc: "Random passwords with selectable character types.",
    input: "Input",
    output: "Output",
    indent: "Indent",
    format: "Format",
    minify: "Minify",
    copy: "Copy",
    copied: "Copied",
    clear: "Clear",
    jsonEmpty: "Paste some JSON first.",
    jsonInvalid: "Invalid JSON. Check the input and try again.",
    length: "Length",
    lowercase: "Lowercase (a-z)",
    uppercase: "Uppercase (A-Z)",
    digits: "Digits (0-9)",
    symbols: "Symbols",
    excludeAmbiguous: "Exclude ambiguous characters (O/0/I/l/1)",
    regenerate: "Generate",
    strength: "Strength",
    weak: "Weak",
    medium: "Medium",
    strong: "Strong",
    noCharset: "Select at least one character type.",
    genFailed: "Cannot generate a password here.",
    copyFailed: "Clipboard is unavailable in this context.",
  },
  zh: {
    appTitle: "开发工具箱",
    loading: "加载中…",
    toolJson: "JSON 格式化",
    toolJsonDesc: "美化、压缩并校验 JSON。",
    toolPassword: "密码生成器",
    toolPasswordDesc: "按字符类型生成随机密码。",
    input: "输入",
    output: "输出",
    indent: "缩进",
    format: "格式化",
    minify: "压缩",
    copy: "复制",
    copied: "已复制",
    clear: "清空",
    jsonEmpty: "请先粘贴 JSON 内容。",
    jsonInvalid: "JSON 无效，请检查输入后重试。",
    length: "长度",
    lowercase: "小写字母（a-z）",
    uppercase: "大写字母（A-Z）",
    digits: "数字（0-9）",
    symbols: "符号",
    excludeAmbiguous: "排除易混淆字符（O/0/I/l/1）",
    regenerate: "生成",
    strength: "强度",
    weak: "弱",
    medium: "中",
    strong: "强",
    noCharset: "请至少选择一种字符类型。",
    genFailed: "当前环境无法生成密码。",
    copyFailed: "当前环境无法访问剪贴板。",
  },
};

export function makeI18n(locale) {
  const dict = String(locale || "en").toLowerCase().startsWith("zh") ? dictionaries.zh : dictionaries.en;
  return (key) => dict[key] ?? dictionaries.en[key] ?? key;
}
```

- [x] **Step 2: 写工具注册表**

`src/lib/tools.js`：

```js
import JsonFormatter from "./JsonFormatter.svelte";
import PasswordGenerator from "./PasswordGenerator.svelte";

export const tools = [
  { id: "json", labelKey: "toolJson", component: JsonFormatter },
  { id: "password", labelKey: "toolPassword", component: PasswordGenerator },
];
```

- [x] **Step 3: 写工作台外壳**

`src/App.svelte`：

```svelte
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
```

- [x] **Step 4: 写 JSON 面板**

`src/lib/JsonFormatter.svelte`：

```svelte
<script>
  import { makeI18n } from "./i18n.js";
  import { formatJson, minifyJson, JsonToolError } from "./jsonTools.js";

  let { locale = "en" } = $props();
  const t = $derived(makeI18n(locale));
  let input = $state("");
  let output = $state("");
  let error = $state("");
  let indent = $state(2);
  let copied = $state(false);

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

<div class="card">
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

  <label for="json-output">{t("output")}</label>
  <textarea id="json-output" value={output} readonly spellcheck="false"></textarea>
  {#if error}<p class="error">{error}</p>{/if}
</div>
```

- [x] **Step 5: 写密码生成面板**

`src/lib/PasswordGenerator.svelte`：

```svelte
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
```

- [x] **Step 6: 写全局样式与入口**

`src/app.css`：

```css
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; font-family: Inter, ui-sans-serif, system-ui, sans-serif; color: CanvasText; background: Canvas; }
main { max-width: 960px; margin: 0 auto; padding: 24px clamp(14px, 3vw, 32px) 56px; }
header h1 { margin: 0 0 14px; font-size: 22px; }
nav { display: flex; flex-wrap: wrap; gap: 8px; margin-bottom: 18px; }
button { border: 1px solid color-mix(in srgb, CanvasText 20%, transparent); border-radius: 8px; padding: 8px 12px; background: transparent; color: inherit; font: inherit; cursor: pointer; }
button.primary { background: #6d5dfc; border-color: #6d5dfc; color: white; }
button.active { border-color: #6d5dfc; color: #6d5dfc; }
button:disabled { opacity: .5; cursor: default; }
.card { padding: 16px; border: 1px solid color-mix(in srgb, CanvasText 14%, transparent); border-radius: 12px; }
.tool-head { display: flex; justify-content: space-between; align-items: baseline; gap: 12px; flex-wrap: wrap; margin-bottom: 12px; }
.tool-head h2 { margin: 0; font-size: 16px; }
.muted { opacity: .7; font-size: 13px; }
label { display: block; margin: 12px 0 6px; font-size: 13px; font-weight: 600; }
textarea { width: 100%; min-height: 180px; padding: 10px; border-radius: 8px; border: 1px solid color-mix(in srgb, CanvasText 20%, transparent); background: color-mix(in srgb, CanvasText 5%, transparent); color: inherit; font: 13px/1.5 ui-monospace, SFMono-Regular, Consolas, monospace; resize: vertical; }
.row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 12px; }
.check { display: flex; align-items: center; gap: 8px; margin: 8px 0; font-size: 14px; font-weight: 400; }
.check input { width: auto; }
.mono { font-family: ui-monospace, SFMono-Regular, Consolas, monospace; }
.password-result { min-height: 48px; padding: 12px; border-radius: 8px; background: color-mix(in srgb, CanvasText 6%, transparent); word-break: break-all; letter-spacing: .08em; }
.error { margin-top: 10px; color: #d14343; font-size: 13px; }
```

`src/main.js`：

```js
import App from "./App.svelte";
import { mount } from "svelte";
import "./app.css";

mount(App, { target: document.getElementById("app") });
```

`index.html` 加 `<title>Dev Tools</title>`。

- [x] **Step 7: 构建 UI 验证**

```powershell
cd plugins/dev-tools; npm install --ignore-scripts; npm run build
```

Expected: 构建成功并输出 `DBX_UI_BUILD_SUCCESS`，产物在 `ui/`。

---

### Task 4: 发布接入、商店材料与文档

**Files:**
- Modify: `.github/workflows/plugin-release.yml`
- Modify: `plugins/dev-tools/package.json`
- Create: `plugins/dev-tools/.dbx-store.json`
- Create: `plugins/dev-tools/store/publishers/qiuyouyao.json`
- Create: `plugins/dev-tools/scripts/build-store-candidate.mjs`
- Modify: `plugins/dev-tools/README.md`

- [x] **Step 1: 根工作流增加独立发布任务**

保留 `release-s3-sync` job 并加 `if: "!startsWith(github.event.release.tag_name, 'devtools-v')"`；新增 `release-dev-tools` job，条件 `startsWith(github.event.release.tag_name, 'devtools-v')`，调用同一个 reusable workflow，`working-directory: plugins/dev-tools`。纯前端插件沿用 CLI 生成模板的 `rust-toolchain: ""` 与 `build-matrix: '{"include":[{"runner":"ubuntu-24.04","target":"universal"}]}'`。删除 CLI 在 `plugins/dev-tools/.github/` 生成的重复工作流（仓库约定：工作流只在根）。这样 `vX.Y.Z` 仍发布 s3-sync，`devtools-vX.Y.Z` 只发布 Dev Tools。

- [x] **Step 2: 商店材料**

`.dbx-store.json` 使用 HTTPS icon/source/homepage、MIT、`permissions: ["host.workbench"]`、zh-CN 本地化，tag 指向 `devtools-v0.1.0`；`store/publishers/qiuyouyao.json` 与 s3-sync 相同；`scripts/build-store-candidate.mjs` 从 s3-sync 复制（仅依赖插件根目录，无需改逻辑）；`package.json` 增加 `store:candidate`。

- [x] **Step 3: README**

写明布局、开发/测试/打包命令、扩展新工具的步骤（加组件 → 注册到 `tools.js` → 加 i18n key → 加纯函数测试）、发布 tag 约定与商店提交流程。

---

### Task 5: 验证

- [x] `npm test`：10 个单元测试通过。
- [x] `npm run build`：Vite 构建通过。
- [x] `node -e "JSON.parse(require('fs').readFileSync('plugins/dev-tools/manifest.json','utf8'))"`：manifest JSON 合法。
- [x] `git diff --check`：无空白错误。
- [x] 全局搜索 `io.github.qiuyouyao.dev-tools`：manifest、package.json、README、商店材料一致；无残留模板文案。

---

## Self-review

- 首版两个工具均有对应 Task 与测试；扩展点由 `tools.js` 注册表提供。
- 无 `TBD` / `TODO` 占位；所有代码步骤给出完整内容。
- 身份 ID、publisher、权限、i18n key 在后续 Task 中保持一致。
- 未验证项：CI reusable workflow 对 `devtools-v` tag 的版本解析（本机无外网，无法拉取 workflow 定义）；发布前需在 GitHub 上实测一次。
