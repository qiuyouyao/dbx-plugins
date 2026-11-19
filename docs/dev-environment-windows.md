# Windows 开发环境（本机实测记录）

本机 C 盘空间紧张（<1 GB），所有工具链装在 D 盘。以下为已验证可用的完整配置。

## 工具链布局

| 组件 | 位置 | 说明 |
| --- | --- | --- |
| RUSTUP_HOME | `D:\dev\rustup` | 用户环境变量已持久化 |
| CARGO_HOME | `D:\dev\cargo` | 用户环境变量已持久化 |
| Rust 工具链 | `stable-x86_64-pc-windows-gnu` | 无 MSVC/VS Build Tools，GNU 目标 + 自带 lld |
| MinGW-w64 | `D:\dev\mingw64` | winlibs GCC 16.2（无 LLVM 版），提供 dlltool 所需的 as/ld 及其 DLL 依赖 |
| crates 镜像 | `D:\dev\cargo\config.toml` | rsproxy.cn sparse；`RUSTUP_DIST_SERVER=https://rsproxy.cn` |
| dbx-plugin CLI | npm 全局 `@dbx-app/plugin-cli` | SDK 随包分发，不在 crates.io |

PATH（用户级）需包含：`D:\dev\cargo\bin`、`D:\dev\mingw64\bin`、
`D:\dev\rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin\self-contained`。

## 两个已修复的上游问题（本机 workaround）

### 1. CLI 的 `\\?\` verbatim 路径 vs GNU 工具链

`dbx-plugin package/dev` 用规范化（`\\?\D:\...`）路径调 cargo，GNU 的
ld/dlltool 不认这种前缀，链接必挂。解决：cargo 转发 shim。

- shim 源码：`D:\dev\cargo-shim`（剥掉参数与环境变量中的 `\\?\` 前缀，
  含 `--config` 里 TOML 转义的双反斜杠形式）。
- 安装方式：`D:\dev\cargo\bin\cargo.exe` 是 shim；原始 rustup 代理保留在
  `D:\dev\cargo-real\cargo.exe`（rustup 代理按文件名识别身份，不能改名）。
- **rustup update 可能覆盖 shim**，更新后需重新复制
  `D:\dev\cargo-shim\target\release\cargo-shim.exe` → `D:\dev\cargo\bin\cargo.exe`。

### 2. dev 宿主的 UNC cwd

dev 宿主以 `\\?\` 路径为 cwd 启动 `ui_build`，cmd.exe 拒绝 UNC。
`dbx-plugin.toml` 的 `[dev]` 已用 pwsh 包一层：先取 `ProviderPath` 剥前缀再
`Set-Location`。这是仓库内修复，跨机器有效（但依赖 pwsh 存在）。

## SDK 补丁（直接跑 cargo 时需要）

`dbx-plugin-sdk` 不在 crates.io。CLI 构建时自动注入补丁；手动
`cargo test/check` 依赖机器级配置（已写入 `D:\dev\cargo\config.toml`）：

```toml
[patch.crates-io]
dbx-plugin-sdk = { path = "C:/Users/qiuyouyao/AppData/Roaming/npm/node_modules/@dbx-app/plugin-cli/sdk-root/plugins/sdk/rust/dbx-plugin-sdk" }
```

## 集成测试环境（RustFS 双端）

Docker Hub 被墙，本机用私有仓库已有镜像
`172.16.180.48:8666/common_images/rustfs/rustfs:latest`：

```pwsh
docker run -d --name s3sync-src -p 9101:9000 172.16.180.48:8666/common_images/rustfs/rustfs:latest
docker run -d --name s3sync-dst -p 9102:9000 172.16.180.48:8666/common_images/rustfs/rustfs:latest
$env:S3_SYNC_TEST_SOURCE_ENDPOINT='http://127.0.0.1:9101'
$env:S3_SYNC_TEST_TARGET_ENDPOINT='http://127.0.0.1:9102'
$env:S3_SYNC_TEST_ACCESS_KEY='rustfsadmin'
$env:S3_SYNC_TEST_SECRET_KEY='rustfsadmin'
cargo test -- --test-threads=1
```

未设环境变量时集成测试自动跳过。MinIO 镜像拉取失败时的替代即此方案。

## 网络备注

- `win.rustup.rs`、Docker Hub、`raw.githubusercontent.com` 不可达。
- GitHub Release 下载走 `https://gh-proxy.com/<原始URL>` 镜像。
- rustup 组件与 crates 走 rsproxy.cn；`static.rust-lang.org` 直连可用。