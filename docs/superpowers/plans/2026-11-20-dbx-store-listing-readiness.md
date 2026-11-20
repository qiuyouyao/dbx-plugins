# DBX Store 上架整改 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `plugins/s3-sync` 从模板占位状态整改到可发布候选包、可提交 dbx-store PR 的状态。

**Architecture:** 保留当前 monorepo 布局；插件身份统一为 `id = io.github.qiuyouyao.s3-sync`、`publisher = qiuyouyao`；GitHub Actions 工作流迁到仓库根目录并用 `working-directory` 指向插件；凭证绑定改为 Secret Store；提供从 `release-candidates.json` 生成 dbx-store 候选文件的脚本。

**Tech Stack:** DBX Plugin CLI 0.1.10、Svelte 5 + Vite、Rust sidecar、GitHub Actions、dbx-store candidate schema v1。

---

### Task 1: 统一插件身份

**Files:**
- Modify: `plugins/s3-sync/manifest.json`
- Modify: `plugins/s3-sync/package.json`
- Modify: `plugins/s3-sync/src/lib/state.svelte.js`
- Modify: `plugins/s3-sync/backend/src/main.rs`

- [x] **Step 1: 替换 manifest 中的旧 ID 与 publisher**

把 `io.github.example.s3-sync` 全部替换为 `io.github.qiuyouyao.s3-sync`，把 `"publisher": "example"` 改为 `"publisher": "qiuyouyao"`。

- [x] **Step 2: 同步其他身份引用**

把 `package.json` 的 `"name": "io.github.example.s3-sync-ui"`、`state.svelte.js` 的 `providerId` 过滤值、`main.rs` 的 `PluginMetadata::new(...)` 参数同步改为新 ID。

- [x] **Step 3: 验证无残留**

```powershell
rg "io\.github\.example|\"publisher\"\s*:\s*\"example\"" d:/ai/dbx-plugins --glob '!dist/**' --glob '!**/target/**'
```

Expected: 无输出。

---

### Task 2: 凭证与权限收口

**Files:**
- Modify: `plugins/s3-sync/manifest.json`
- Modify: `plugins/s3-sync/backend/src/main.rs`
- Modify: `plugins/s3-sync/src/lib/TaskEditor.svelte`
- Modify: `plugins/s3-sync/backend/Cargo.toml`

- [x] **Step 1: `access_key` 写入 Secret Store**

manifest 中 `access_key` 的 `binding` 从 `config` 改为 `secret`。

- [x] **Step 2: Sidecar 从 secrets 读取两个凭证**

在 `s3_config` 中新增 `secret_field` 闭包，`access_key` 和 `secret_key` 都从 `connection_secrets` 读取，不再从 `external_config` 读取。

- [x] **Step 3: 删除未在 CLI 中发现的 `reopenConnection` 调用**

`TaskEditor.svelte` 的 `testConnection` 直接调用 `rpc("s3/listBuckets", { connectionId })`，由错误提示处理未打开连接的情况。

- [x] **Step 4: 统一许可证**

`backend/Cargo.toml` 的 `license` 从 `Apache-2.0` 改为 `MIT`。

---

### Task 3: 发布工作流迁移

**Files:**
- Create: `.github/workflows/plugin-release.yml`
- Delete: `plugins/s3-sync/.github/workflows/plugin-release.yml`

- [x] **Step 1: 创建仓库根工作流**

工作流在 Release published 时触发，调用官方 reusable workflow；固定到 commit SHA `67f058dd5521619c8012a594fa2e3496dde4c3ca`；传 `working-directory: plugins/s3-sync`；`package-command` 先 `npm ci && npm run build` 再 `dbx-plugin package .`；沿用 5 平台默认矩阵。

- [x] **Step 2: 删除插件子目录工作流**

避免 GitHub Actions 忽略和重复配置。

---

### Task 4: 商店 metadata 与候选生成

**Files:**
- Create: `plugins/s3-sync/.dbx-store.json`
- Create: `plugins/s3-sync/store/publishers/qiuyouyao.json`
- Create: `plugins/s3-sync/scripts/build-store-candidate.mjs`
- Modify: `plugins/s3-sync/package.json`
- Modify: `plugins/s3-sync/README.md`

- [x] **Step 1: 写 `.dbx-store.json`**

包含 name、description、HTTPS icon、tags、permissions、HTTPS source、homepage、MIT license、releaseNotes、zh-CN localizations；不包含 `autoUpdate` 或 `signingKeyId`。

- [x] **Step 2: 写 publisher 记录**

```json
{
  "id": "qiuyouyao",
  "name": "qiuyouyao",
  "status": "active"
}
```

- [x] **Step 3: 写候选生成脚本**

脚本读取 `release-candidates.json` 和 `.dbx-store.json`，校验 HTTPS URL、SHA-256、size、publisher 注册一致性，输出 `dist/store-candidate.json`。

- [x] **Step 4: 添加 npm script 与 README 发布说明**

`package.json` 增加 `store:candidate`；README 增加从 Release 下载候选清单、生成并提交 dbx-store PR 的步骤。

---

### Task 5: 文档与忽略规则加固

**Files:**
- Modify: `.gitignore`
- Modify: `docs/dev-environment-windows.md`

- [x] **Step 1: `.gitignore` 增加通用密钥规则**

增加 `.env`、`.env.*`、`*.key`、`*.pem`、`*.p12`、`*.pfx`。

- [x] **Step 2: 清理个人机器信息和内网地址**

把 `C:/Users/qiuyouyao/...` 改为 CLI SDK 的相对路径表达式；把 `172.16.180.48:8666/...` 改为公开 `minio/minio:latest`；把 `D:\dev\...` 改为环境变量说明。

---

### Task 6: 验证

- [x] **Step 1: 静态检查**

```powershell
git diff --check
node -e "JSON.parse(require('fs').readFileSync('d:/ai/dbx-plugins/plugins/s3-sync/manifest.json','utf8')); JSON.parse(require('fs').readFileSync('d:/ai/dbx-plugins/plugins/s3-sync/package.json','utf8')); JSON.parse(require('fs').readFileSync('d:/ai/dbx-plugins/plugins/s3-sync/.dbx-store.json','utf8')); JSON.parse(require('fs').readFileSync('d:/ai/dbx-plugins/plugins/s3-sync/store/publishers/qiuyouyao.json','utf8')); console.log('json ok')"
```

- [x] **Step 2: UI 构建**

```powershell
cd d:/ai/dbx-plugins/plugins/s3-sync; npm ci; npm run build
```

- [x] **Step 3: 候选生成脚本冒烟测试**

用本地 `windows-x64` 候选信息构造临时 `release-candidates.json`，运行 `npm run store:candidate -- <path>`，确认输出结构与 candidate schema 一致。

- [x] **Step 4: Git 状态复核**

确认没有 `.dbxp`、私钥、`dist/`、`.dbx-dev/` 被跟踪；已有 `.idea` 暂存改动保持原样，不纳入本次上架整改。

本地验证说明：`npm ci` 被当前环境的 safe-delete 策略拦截，已改用
`npm install --ignore-scripts` + `npm run build` 验证构建；CI 工作流仍使用 `npm ci`。

---
