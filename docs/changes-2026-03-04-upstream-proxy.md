# Upstream Proxy（写回地址）+ Rust 编译修复

日期：2026-03-04

## 背景 / 目标
当前本地代理的「监听端口」与写回到各客户端（Claude/Codex/Gemini Live 配置）的代理地址是强绑定的。

本次新增 **GlobalProxyConfig.upstreamUrl**：
- 写回 Live 配置时使用该 upstreamUrl（仅允许 `http(s)://host:port` 的 origin）
- 代理服务实际启动/监听仍使用本地 `listen_address/listen_port`

该能力用于给用户提供一个“中间层端口”，便于插入调试/转发代理。

## 功能变更
### 1) 数据库与后端接口
- 在 `proxy_config` 表新增字段 `upstream_url`，并将数据库版本从 v5 升级到 v6，提供迁移逻辑（ALTER TABLE / add_column_if_missing）。
- 新增/扩展 Tauri Command：`get_global_proxy_config` / `update_global_proxy_config` 读取与写入 `upstream_url`。
- 在 `update_global_proxy_config` 增加 upstreamUrl 校验与规范化：
  - 仅允许 http/https
  - 必须包含 host 与 port
  - 不允许 path/query/fragment
  - 入库时规范化为 origin（不带末尾 `/`）

相关文件：
- src-tauri/src/database/schema.rs
- src-tauri/src/database/mod.rs
- src-tauri/src/database/dao/proxy.rs
- src-tauri/src/commands/proxy.rs
- src-tauri/src/proxy/types.rs

### 2) 写回 Live 配置逻辑
- 构造写回 URL 时（build_proxy_urls）：
  - 若设置了 `upstream_url`，则写回使用它
  - 否则回退到 `http://{listen_address}:{listen_port}`（并对 0.0.0.0 / IPv6 做回环地址处理）

相关文件：
- src-tauri/src/services/proxy.rs

### 3) 前端配置 UI
- Proxy 面板新增 Upstream URL 输入框
- 保存前做同等规则的 URL 校验
- i18n（en/ja/zh）新增对应文案
- TS 类型 `GlobalProxyConfig` 新增 `upstreamUrl?: string | null`

相关文件：
- src/components/proxy/ProxyPanel.tsx
- src/types/proxy.ts
- src/i18n/locales/en.json
- src/i18n/locales/ja.json
- src/i18n/locales/zh.json

## Rust 编译修复（你本次遇到的 E0308）
问题：`ProxyServer::new` 需要 `ProxyConfig`，但 `ProxyService::start()` 误传了 `GlobalProxyConfig`。

修复：
- `ProxyService::start()` 改为使用 `db.get_proxy_config()`（旧版 ProxyConfig，包含运行时配置）
- `GlobalProxyConfig` 继续用于 UI 全局字段与写回 upstream_url

## 补充：macOS / Unix 构建修复（E0599: write_all not found）
问题：在 `#[cfg(unix)]` 分支中调用 `file.write_all(...)`，需要 `std::io::Write` trait 在作用域内；此前移除全局 `use std::io::Write;` 后，macOS aarch64 构建会报错。

修复：
- 在 `#[cfg(unix)]` 代码块内局部 `use std::io::Write;`，仅对 Unix 生效，不影响 Windows。

相关文件：
- src-tauri/src/settings.rs

## 测试 / 构建验证
- 已在 Windows 环境执行：
  - `cargo build --manifest-path src-tauri/Cargo.toml`
  - 结果：编译通过

## 注意事项
- upstreamUrl 只影响“写回客户端配置”的地址；不改变本地代理真实监听端口。
- upstreamUrl 必须是 origin（`http(s)://host:port`），不允许包含 path/query/fragment。
