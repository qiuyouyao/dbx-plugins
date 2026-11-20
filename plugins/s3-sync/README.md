# S3 Sync (DBX plugin)

Migrate and incrementally sync objects between S3-compatible storage services
(MinIO, RustFS, AWS S3). Design decisions live in the repo root `GLOSSARY.md`
and `docs/adr/`.

## Layout

- `manifest.json` — connection provider + workbench contributions, permissions, i18n.
- `src/` — Svelte 5 workbench UI, built by Vite into `ui/`.
- `backend/` — Rust sidecar: S3 client (SigV4), diff planning, run engine, history.

## Develop

```pwsh
npm ci
npm run build            # or build:watch
dbx-plugin dev --port 5190
```

The Rust sidecar is built by the CLI (`dbx-plugin dev` / `dbx-plugin package`).
The `dbx-plugin-sdk` crate ships inside `@dbx-app/plugin-cli`; for direct
`cargo` use, add a machine-level patch in `$CARGO_HOME/config.toml`:

```toml
[patch.crates-io]
dbx-plugin-sdk = { path = "<npm global>/node_modules/@dbx-app/plugin-cli/sdk-root/plugins/sdk/rust/dbx-plugin-sdk" }
```

## Test

Unit tests (fingerprint judgement, key mapping, XML parsing, retry classification):

```pwsh
cargo test --manifest-path backend/Cargo.toml
```

Integration tests need two MinIO instances:

```pwsh
docker run -d --name s3sync-src -p 9001:9000 -e MINIO_ROOT_USER=minioadmin -e MINIO_ROOT_PASSWORD=minioadmin minio/minio server /data
docker run -d --name s3sync-dst -p 9002:9000 -e MINIO_ROOT_USER=minioadmin -e MINIO_ROOT_PASSWORD=minioadmin minio/minio server /data
$env:S3_SYNC_TEST_SOURCE_ENDPOINT='http://127.0.0.1:9001'
$env:S3_SYNC_TEST_TARGET_ENDPOINT='http://127.0.0.1:9002'
cargo test --manifest-path backend/Cargo.toml integration -- --nocapture --test-threads=1
```

Without the env vars the integration test skips itself.

## Package

```pwsh
npm ci
npm run build
dbx-plugin package .
```

Unsigned candidate `.dbxp` lands in `dist/`. Installing it locally requires
“allow unsigned development packages” in the DBX plugin center.

The CI workflow lives at the repository root. It builds every DBX target and
uploads the unsigned `.dbxp` files plus `release-candidates.json` to the GitHub
Release matching the tag.

## DBX Store submission

1. Publish a source tag `v0.1.0` and a GitHub Release for it.
2. Wait for `.github/workflows/plugin-release.yml` to upload all candidates.
3. Download `release-candidates.json` from that Release, then generate the store candidate:

```pwsh
npm run store:candidate -- ./release-candidates.json
```

4. Open a PR against `t8y2/dbx-store` `main` containing:
   - `publishers/qiuyouyao.json` from `store/publishers/qiuyouyao.json`
   - `candidates/io.github.qiuyouyao.s3-sync.json` from `dist/store-candidate.json`

Do not commit `dist/`, `.dbxp` files, signing keys, or tokens. DBX Store signs
approved candidates; plugin authors must not run `dbx-plugin keygen` for the
official store.