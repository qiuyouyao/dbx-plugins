# Dev Tools (DBX plugin)

A small offline workbench for developer utilities. The first release contains
JSON formatting and password generation; more tools can be added incrementally.
Shared repository conventions live in the root `AGENTS.md`; the implementation
plan lives in `docs/superpowers/plans/2026-11-26-dev-tools-plugin.md`.

## Layout

- `manifest.json` — workbench + command + menus contributions, minimal permissions, i18n.
- `src/` — Svelte 5 workbench UI; Vite builds it into `ui/`.
- `src/lib/tools.js` — tool registry; each entry maps an id to a Svelte panel.
- `src/lib/jsonTools.js`, `src/lib/passwordTools.js` — pure, unit-tested logic.
- No Rust sidecar: all tools run locally in the sandboxed UI, with no network or
  storage access and no credentials involved.

## Tools

- **JSON Formatter** — pretty-print (2 or 4 spaces), minify, validate, copy to clipboard.
  The result is syntax-highlighted (keys, strings, numbers, booleans, null and
  punctuation get distinct colors, with light/dark variants).
- **Password Generator** — 8–64 characters, selectable lowercase/uppercase/digits/symbols,
  optional exclusion of ambiguous characters, entropy estimate, copy to clipboard.
  Randomness comes from Web Crypto `crypto.getRandomValues` with rejection sampling.

## Develop

```pwsh
npm install --ignore-scripts
npm test                 # node:test, runs without DBX or network
npm run build            # or npm run build:watch
dbx-plugin dev --path . --port 5190
```

Commit `package-lock.json` so release CI can use `npm ci`. The UI is built from
`src/` into `ui/`; `dist/` and `ui/` are build outputs and are not committed.

## Add another tool

1. Create `src/lib/<ToolName>.svelte` with a `locale` prop and localized UI text.
2. Put reusable, side-effect-free logic in `src/lib/<toolName>.js` and unit-test it in
   `src/lib/tools.test.js` (Node `node:test` cannot import `.svelte` files).
3. Add one entry to `tools` in `src/lib/tools.js`.
4. Add the new i18n keys to both dictionaries in `src/lib/i18n.js`.

No manifest or build changes are needed for additional workbench-internal tools.

## Test

```pwsh
npm test
npm run build
```

Unit tests cover JSON parsing/formatting, character-set selection, deterministic
password generation with an injected RNG, and entropy growth. Integration testing
with the real DBX host is still required before release.

## Package

```pwsh
npm ci
npm run build
dbx-plugin package .
```

The unsigned universal `.dbxp` lands in `dist/`. Installing it locally requires
“allow unsigned development packages” in the DBX plugin center.

## Release and DBX Store

The root workflow `.github/workflows/plugin-release.yml` publishes this plugin only
for release tags prefixed `devtools-v` (for example `devtools-v0.1.1`); plain `v*`
tags remain reserved for S3 Sync. Keep `manifest.json` version in sync with the tag.

1. Publish a source tag `devtools-v0.1.1` and a GitHub Release for it.
2. Wait for CI to upload `release-candidates.json` and the unsigned `.dbxp`.
3. Generate the store candidate:

```pwsh
npm run store:candidate -- ./release-candidates.json
```

4. Open a PR against `t8y2/dbx-store` `main` containing
   `publishers/qiuyouyao.json` and `dist/store-candidate.json`.

Do not commit `dist/`, `.dbxp` files, signing keys, or tokens. DBX Store signs
approved candidates; plugin authors must not run `dbx-plugin keygen` for the
official store.

Note: the reusable workflow's handling of the `devtools-v` tag prefix could not be
verified offline on the development machine; confirm the first release on GitHub.
