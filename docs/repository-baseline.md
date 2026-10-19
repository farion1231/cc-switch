# 仓库基线与清理记录

日期：2026-10-19。范围：按照项目总结整理仓库并规划开发，不实现安全内核。

## 基线

| 项目 | 本地证据 |
| --- | --- |
| Git HEAD | `adc223af322adaeca878e82bf78323865b0db768` |
| 提交标题 | `fix(claude): use catalog ids for 5.5 upstreams in routing takeover (#8450)` |
| 提交时间 | 2026-10-19 11:48:25 +0800 |
| 包版本 | package.json / src-tauri/Cargo.toml：4.0.15 |
| 工具链 | `.node-version`：22.12.0；packageManager：pnpm@10.12.3；rust-toolchain.toml：1.95 |
| 开始时工作树 | 仅项目总结为用户已暂存新增文件；正文和暂存状态保持原样 |
| 许可证 | 根目录 LICENSE 保留原文 |

上述为本地证据，未通过远端核验 commit 的归属或最新性。未新建提交、推送或改变仓库 remote。

## 已执行的清理

共移除 **324 个已跟踪文件，24,529,796 字节（约 23.39 MiB）**。这是工作树文件体积，不是 Git 仓库或最终安装包的节省量。每个文件删除前与 HEAD 内容比对一致，完整路径、字节数、SHA-256 和原因在 [cleanup-manifest.tsv](cleanup-manifest.tsv)。

| 清理对象 | 原因 |
| --- | --- |
| 根目录旧多语言 README、CHANGELOG、SUPPORT、CODE_OF_CONDUCT | 旧产品发布、社区联系方式和宣传不适用于当前项目 |
| `assets/` | 上游合作商横幅、宣传截图、历史发布配图；运行时素材在 src 内，已保留 |
| `docs/release-notes/`、`docs/user-manual/` | 旧产品历史与安装操作文档，不应作为 XSEC 支持承诺；可从基线查阅 |
| `deplink.html` | 上游网站 deeplink 生成页，非 Vite 构建入口；应用 deeplink 解析代码保留 |
| `flatpak/` | 下载旧产品包的分发配置；未来在新产品发布任务中重新配置 |
| 上游 release、sync-r2 工作流及其 scripts、签名 action | 依赖上游发布身份、签名与镜像渠道；通过 Git 基线保留参考 |
| Claude、stale、labeler 工作流、旧 ISSUE_TEMPLATE、CODEOWNERS、FUNDING、labeler 配置 | 上游机器人、社区和所有者配置不代表本项目治理约定 |

README、CONTRIBUTING、SECURITY 已替换为项目当前状态与开发入口。清理清单只统计删除，不含上述替换及新增文档。

保留的 22 篇指南中，68 处指向已删除手册/版本说明的链接改为固定到上述 commit 的上游源码链接，避免本地断链；未验证远端链接可达性，本地 Git 基线仍是恢复依据。`src/whats-new/README.md` 同步说明历史说明的查阅位置，未修改其运行时 JSON 数据。

## 保留的工程与参考资料

- `src/`、`src-tauri/`、`tests/`：保留运行时代码、测试、图标、应用内 whats-new 数据与数据库迁移。功能不在 MVP 范围不等于可以安全删除。
- 包管理文件、锁文件、构建配置、`ci.yml`、`wsl2-nightly.yml`、Dependabot 和通用 PR 模板：维持工程验证基础。
- `docs/guides/`、`docs/images/`、Pi 契约与需求文档、`docs/design-system.html`、`session-manager.md`：保留路由、界面和兼容性依据，属于上游参考，不代表 XSEC 已验收功能。
- `.git/`、`.idea/`、已安装依赖和本地构建缓存：不属于本次删除范围。
- 用户原始项目总结：未经修改，继续作为需求基线。

运行时仍有 CC Switch 品牌、更新地址、官网链接和数据目录；这部分与代码及测试耦合，纳入 P0 一起替换。清理阶段未宣称品牌迁移完成。

## 关键代码地图

| 能力 | 当前入口 | 开发处理 |
| --- | --- | --- |
| 桌面和 IPC | `src/App.tsx`、`src-tauri/src/lib.rs`、`src-tauri/src/commands/` | 复用；新增受限安全状态与策略命令 |
| Provider / Agent 配置 | `src/config/`、`src-tauri/src/services/provider/`、`src-tauri/src/mode/`、`src-tauri/src/live/` | 复用；严格模式不得经模式切换或退出恢复为静默直连 |
| 代理路由 | `src-tauri/src/proxy/server.rs`、`handlers.rs`、`handler_context.rs` | 请求入口预检查与内容分类 |
| 上游发送和重试 | `src-tauri/src/proxy/forwarder.rs` | 脱敏状态贯穿转换、重试、故障切换；最终出站检查 |
| 两类发送传输 | `src-tauri/src/proxy/http_client.rs`、`hyper_client.rs` | 同时覆盖 reqwest 与 hyper；不能只修改其中一个 |
| 普通与流式响应 | `src-tauri/src/proxy/response_processor.rs`、`sse.rs`、`providers/streaming*.rs` | 协议语义层恢复，覆盖专用转换分支 |
| 会话标识 | `src-tauri/src/proxy/session.rs` | 当前会话 ID 可由客户端提供，不能直接作为恢复授权依据 |
| 本地数据库与日志 | `src-tauri/src/database/`、`proxy/usage/logger.rs`、`panic_hook.rs` | 保留配置存储；敏感映射独立加密，日志单独审查 |
| 运行时身份与更新 | `src-tauri/tauri.conf.json`、`src-tauri/src/config.rs`、`src/lib/updater.ts` | P0 隔离应用标识、数据和更新渠道 |

## 可恢复性与验证

所有移除文件都在上述基线提交中，完整路径见清单。需要查阅某个旧文件时可使用：

```sh
git show adc223af322adaeca878e82bf78323865b0db768:README_ZH.md
```

仅在需要恢复特定文件时，确认不会覆盖后续修改，再使用 `git restore --source=<基线提交> --worktree -- <清单中的单个路径>`。不要整体恢复覆盖新的项目文档。

本轮验证结果记录在 [测试策略](testing.md#本轮整理验证)。
