# Windows 开发环境（本机实测记录）

以下记录已验证可用的工具组合；路径仅使用环境变量或占位符，不绑定个人目录。

## 工具链布局

| 组件 | 位置 | 说明 |
| --- | --- | --- |
| RUSTUP_HOME | 自定义稳定目录 | 由用户环境变量配置 |
| CARGO_HOME | 自定义稳定目录 | 由用户环境变量配置 |
| Rust 工具链 | `stable-x86_64-pc-windows-gnu` | 无 MSVC/VS Build Tools 时可使用 GNU 目标 |
| MinGW-w64 | 自定义目录 | 提供 dlltool 所需的 as/ld 及其 DLL 依赖 |
| crates 镜像 | `$CARGO_HOME/config.toml` | 按所在网络环境配置 sparse 镜像 |
| dbx-plugin CLI | npm 全局 `@dbx-app/plugin-cli` | SDK 随包分发，不在 crates.io |

PATH 需包含 `$CARGO_HOME/bin`、MinGW-w64 的 `bin`，以及 Rust GNU 工具链
`lib\rustlib\x86_64-pc-windows-gnu\bin` 目录。

## 两个已修复的上游问题（本机 workaround）

### 1. CLI 的 `\\?\` verbatim 路径 vs GNU 工具链

`dbx-plugin package/dev` 用规范化（`\\?\` 前缀）路径调 cargo，GNU 的
ld/dlltool 不认这种前缀。解决方式是使用 cargo 转发 shim，剥掉参数与环境变量
中的 `\\?\` 前缀（含 `--config` 里 TOML 转义的双反斜杠形式）。

- shim 源码：独立目录，例如 `$TOOLS_HOME/cargo-shim`。
- 安装方式：用编译后的 shim 覆盖 `$CARGO_HOME/bin/cargo.exe`；原始 rustup 代理
  另存为其他文件名，因为 rustup 代理按自身文件名识别身份。
- **rustup update 可能覆盖 shim**，更新后需重新复制 shim 到
  `$CARGO_HOME/bin/cargo.exe`。

### 2. dev 宿主的 UNC cwd

dev 宿主以 `\\?\` 路径为 cwd 启动 `ui_build`，cmd.exe 拒绝 UNC。
`dbx-plugin.toml` 的 `[dev]` 已用 pwsh 包一层：先取 `ProviderPath` 剥前缀再
`Set-Location`。这是仓库内修复，跨机器有效（但依赖 pwsh 存在）。

## SDK 补丁（直接跑 cargo 时需要）

`dbx-plugin-sdk` 不在 crates.io。CLI 构建时自动注入补丁；手动
`cargo test/check` 时，在 `$CARGO_HOME/config.toml` 配置本机 CLI 安装位置：

```toml
[patch.crates-io]
dbx-plugin-sdk = { path = "<npm-global>/node_modules/@dbx-app/plugin-cli/sdk-root/plugins/sdk/rust/dbx-plugin-sdk" }
```

## 集成测试环境（MinIO 双端）

使用公开 MinIO 镜像即可；镜像拉取失败时按所在网络环境选择可访问的公共镜像源。

```pwsh
docker run -d --name s3sync-src -p 9101:9000 -e MINIO_ROOT_USER=minioadmin -e MINIO_ROOT_PASSWORD=minioadmin minio/minio:latest server /data
docker run -d --name s3sync-dst -p 9102:9000 -e MINIO_ROOT_USER=minioadmin -e MINIO_ROOT_PASSWORD=minioadmin minio/minio:latest server /data
$env:S3_SYNC_TEST_SOURCE_ENDPOINT='http://127.0.0.1:9101'
$env:S3_SYNC_TEST_TARGET_ENDPOINT='http://127.0.0.1:9102'
$env:S3_SYNC_TEST_ACCESS_KEY='minioadmin'
$env:S3_SYNC_TEST_SECRET_KEY='minioadmin'
cargo test -- --test-threads=1
```

未设环境变量时集成测试自动跳过。
