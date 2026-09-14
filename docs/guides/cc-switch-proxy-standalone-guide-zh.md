# CC-Switch Proxy Standalone CLI —— 使用与维护指南

> 适用于 cc-switch fork 的 `feat/cc-switch-proxy-cli` 分支。本文档涵盖：构建、启动、管理 API、接入 Codex 与其他 Rust 程序、国产模型配置、以及**如何合并上游代码**。

## 一、这是什么

`cc-switch-proxy` 是从 cc-switch（Tauri 桌面应用）提取的 **headless 命令行代理**，不依赖 Tauri/GUI 即可独立运行。它做两件事：

1. **协议转换代理**：监听本地端口，把 Codex CLI 的 OpenAI **Responses API** 请求自动转换成上游模型的 **Chat Completions** 格式转发（支持 deepseek / glm / kimi / qwen / minimax / ark 等国产模型）。
2. **管理 API**：提供 HTTP 接口（`/admin/*`）动态增删改查 provider，数据存独立 sqlite DB。

**适用场景**：服务器 / 容器 / CI 等无 GUI 环境；其他 Rust 程序通过 HTTP 动态注册/切换模型；不想装整个 GUI。

**设计文档**：`docs/superpowers/specs/2026-07-07-cc-switch-proxy-standalone-cli-design.md`
**实现计划**：`docs/superpowers/plans/2026-07-07-cc-switch-proxy-cli.md`

---

## 二、构建

仓库根目录：

```bash
# release（推荐，体积小）
cargo build --release --manifest-path src-tauri/Cargo.toml --bin cc-switch-proxy
# debug
cargo build --manifest-path src-tauri/Cargo.toml --bin cc-switch-proxy
```

产物：`src-tauri/target/{release|debug}/cc-switch-proxy[.exe]`

> 首次编译较慢（链接整个 lib，含 tauri 等重依赖），release 约 3 分钟。后续增量编译快。

---

## 三、启动

```bash
./cc-switch-proxy [选项]
```

| 选项 | 默认 | 说明 |
|---|---|---|
| `--db <path>` | `~/.config/cc-switch/cli-proxy.db` | sqlite DB 路径（**独立**，与 GUI 的 `cc-switch.db` 不共享）|
| `--address <ip>` | `127.0.0.1` | 监听地址（**强制回环**，非 `127.0.0.1`/`localhost`/`::1` 拒绝启动）|
| `--port <num>` | `15721` | 监听端口（`0` = OS 分配）|
| `--help` | | 帮助 |

启动后输出监听地址 + 管理 API 提示。`Ctrl-C` / `SIGTERM` 优雅停止。

**退出码**：`0` 正常 / `2` 参数错或非回环地址 / `3` DB 失败 / `4` 端口绑定失败。

---

## 四、管理 API

所有路由在 `/admin/*`，**无鉴权**（仅绑回环，局域网访问不到，安全）。

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/admin/status` | 代理运行状态 |
| GET | `/admin/providers` | 列出所有 codex provider（**脱敏**，无 api_key）|
| POST | `/admin/providers` | 创建 provider（DTO 见下）|
| DELETE | `/admin/providers/:id` | 删除 |
| POST | `/admin/providers/:id/enable` | 设为当前启用 |

**创建 DTO**：

```json
{
  "name": "DeepSeek",
  "base_url": "https://api.deepseek.com",
  "api_key": "sk-xxx",
  "model": "deepseek-chat",
  "reasoning_effort": "high",
  "api_format": "openai_chat",
  "enable": true
}
```

**curl 示例**：

```bash
# 创建并启用 DeepSeek
curl -X POST http://127.0.0.1:15721/admin/providers \
  -H 'Content-Type: application/json' \
  -d '{"name":"DeepSeek","base_url":"https://api.deepseek.com","api_key":"sk-xxx","model":"deepseek-chat","api_format":"openai_chat","enable":true}'

# 列表（返回 id / name / base_url / model / api_format / is_current，不含 api_key）
curl http://127.0.0.1:15721/admin/providers

# 切换当前 provider
curl -X POST http://127.0.0.1:15721/admin/providers/<id>/enable

# 删除
curl -X DELETE http://127.0.0.1:15721/admin/providers/<id>

# 状态
curl http://127.0.0.1:15721/admin/status
```

**`api_format`**（决定是否做协议转换）：
- `openai_chat`：上游是 Chat Completions（deepseek/kimi/glm/qwen/minimax/ark 等）→ **需要转换**，代理把 Codex 的 Responses 请求改写成 Chat 发上游，再把 Chat 响应转回 Responses
- `openai_responses`：上游是 Responses API（官方 OpenAI、或 PackyCode 等聚合商的 Responses 端点）→ **不转换**，透传

**需转换（deepseek 类）示例**：

```bash
curl -X POST http://127.0.0.1:15721/admin/providers \
  -H 'Content-Type: application/json' \
  -d '{"name":"DeepSeek","base_url":"https://api.deepseek.com","api_key":"sk-xxx","model":"deepseek-chat","api_format":"openai_chat","enable":true}'
```

**不需转换（Responses 兼容上游）示例**：

```bash
curl -X POST http://127.0.0.1:15721/admin/providers \
  -H 'Content-Type: application/json' \
  -d '{"name":"OpenAI-Resp","base_url":"https://api.openai.com/v1","api_key":"sk-xxx","model":"gpt-4o","api_format":"openai_responses","enable":true}'
```

`GET /admin/providers` 的响应里，每个 provider 都带 `needs_transform`（true=需转换，false=透传）和 `api_format` 两个字段，一眼区分：

```json
{"providers":[
  {"id":"...","name":"DeepSeek","base_url":"https://api.deepseek.com/v1","model":"deepseek-chat","api_format":"openai_chat","needs_transform":true,"is_current":true},
  {"id":"...","name":"OpenAI-Resp","base_url":"https://api.openai.com/v1","model":"gpt-4o","api_format":"openai_responses","needs_transform":false,"is_current":false}
]}
```

> 管理 API 改动后**立即生效**（代理每次请求实时从 DB 读 provider），无需重启。

---

## 五、连接 Codex CLI

让 Codex 走本地代理。两种方式（效果一样）：

### 方式 A：自动接管（推荐，一键）

通过 admin API 自动改写 codex 配置指向代理（原配置备份到 DB，可一键还原）：

```bash
# 启用接管：改写 ~/.codex/config.toml 的 base_url 指向代理 + wire_api=responses
curl -X POST http://127.0.0.1:15721/admin/routing/codex/enable
# 返回 {"ok":true,"proxy_url":"http://127.0.0.1:15721/v1",...}

# 查看接管状态
curl http://127.0.0.1:15721/admin/routing/codex/status
# 返回 {"codex_takeover_active":true}

# 停止接管：从备份还原原始 config.toml
curl -X POST http://127.0.0.1:15721/admin/routing/codex/disable
```

- 只改 `base_url` + `wire_api`，**不动 `auth.json`**——转发时代理用 DB 里 provider 的真实 key 注入，codex 的 auth 不影响。
- 前提：`~/.codex/config.toml` 已存在（先运行一次 `codex` 初始化）。

### 方式 B：手动改 config

编辑 `~/.codex/config.toml`：

```toml
[model_providers.custom]
name = "cc-switch-proxy"
base_url = "http://127.0.0.1:15721/v1"
wire_api = "responses"
```

配好后正常用 `codex`。它发 Responses 请求到代理，代理按当前 provider 转换转发。

> provider 的真实 api_key 由代理在转发时注入（存在 DB），不需要写进 codex config。

---

## 六、用其他 Rust 程序接入（动态管理）

这是「供给其他 Rust 使用」的核心——用 `reqwest` 调管理 API：

```rust
let client = reqwest::Client::new();

// 动态注册一个 provider 并启用
client.post("http://127.0.0.1:15721/admin/providers")
    .json(&serde_json::json!({
        "name": "DeepSeek",
        "base_url": "https://api.deepseek.com",
        "api_key": "sk-...",
        "model": "deepseek-chat",
        "api_format": "openai_chat",
        "enable": true
    }))
    .send().await?;

// 运行时切到另一个 provider
client.post("http://127.0.0.1:15721/admin/providers/<other-id>/enable")
    .send().await?;
```

这样你的程序可以动态切模型/加 provider，无需重启代理。

---

## 七、国产模型配置示例

通过管理 API 创建时，`base_url` 填各厂商 OpenAI 兼容端点，`api_format=openai_chat`：

| 厂商 | base_url | model 示例 |
|---|---|---|
| DeepSeek | `https://api.deepseek.com` | `deepseek-chat` |
| GLM（智谱）| `https://open.bigmodel.cn/api/paas/v4` | `glm-4-plus` |
| Kimi（月之暗面）| `https://api.moonshot.cn/v1` | `moonshot-v1-128k` |
| 通义千问（DashScope）| `https://dashscope.aliyuncs.com/compatible-mode/v1` | `qwen-plus` |
| MiniMax | `https://api.minimax.chat/v1` | `abab6.5-chat` |
| 火山方舟（Ark）| `https://ark.cn-beijing.volces.com/api/v3` | `<endpoint-id>` |

> base_url / model 以各厂商最新文档为准，或参考 cc-switch GUI 的内置预设（`src/config/codexProviderPresets.ts`）。reasoning 参数（thinking/effort）兼容已内置，按厂商 base_url/model 自动适配。

---

## 八、架构与改动范围（维护参考）

**对 cc-switch 现有源码的改动**（仅 3 处纯加法，其余新增文件）：

1. `src-tauri/src/lib.rs`：+1 行 `pub mod standalone;`
2. `src-tauri/src/proxy/server.rs`：`ProxyServer` 加 `extra_routes` 字段 + `with_extra_routes()` + `build_router` 末尾 merge
3. `src-tauri/src/database/mod.rs`：`init()` 重构为 `open_at_inner()`，新增 `open_at()`（路径可配）

**新增文件**：
- `src-tauri/src/standalone/mod.rs`（run / CLI / 信号 / 地址归一化）
- `src-tauri/src/standalone/admin.rs`（admin API）
- `src-tauri/src/bin/cc_switch_proxy.rs`（main）

**Cargo.toml**：tokio +`signal` feature、+`env_logger`、+`[[bin]]`。

---

## 九、合并上游代码（关键维护流程）

你的 fork（`mengjunwei/cc-switch`）需要定期同步上游（`farion1231/cc-switch`）的更新。因为改动是「3 处纯加法 + 新增文件」，合并冲突极小。

### 一次性配置上游 remote

```bash
git remote add upstream https://github.com/farion1231/cc-switch.git
# 或 SSH：git remote add upstream git@github.com:farion1231/cc-switch.git
git remote -v   # 确认 upstream 已加
```

### 同步流程（推荐 rebase）

```bash
# 1. 拉上游最新
git fetch upstream

# 2. 切到 feature 分支
git checkout feat/cc-switch-proxy-cli

# 3. rebase 到上游 main
git rebase upstream/main

# 4. 解决冲突（见下），逐个文件 add 后 continue
git add <冲突文件>
git rebase --continue

# 5. 强推（rebase 改写了历史）
git push --force-with-lease origin feat/cc-switch-proxy-cli
```

> 也可以用 `git merge upstream/main`（保留合并 commit，不强推）。rebase 历史更干净，merge 更安全。二选一。

### 冲突解决（仅可能发生在 3 个文件，都是「加法」）

**1. `src-tauri/src/lib.rs`** —— 上游可能增删模块声明。保留我们的 `pub mod standalone;`（放模块列表任意合适位置，通常和 `mod proxy;` 邻近）。冲突时同时保留上游的新模块 + 我们的 `pub mod standalone;`。

**2. `src-tauri/src/proxy/server.rs`** —— 保留三处：
- `ProxyServer` struct 的 `extra_routes: Option<Router<ProxyState>>` 字段
- `with_extra_routes()` 方法
- `build_router`：开头是 `let router = Router::new()`（不能变回裸 `Router::new()`），末尾保留 merge 分支：
  ```rust
              .layer(DefaultBodyLimit::max(200 * 1024 * 1024));
          let router = match &self.extra_routes {
              Some(extra) => router.merge(extra.clone()),
              None => router,
          };
          router.with_state(self.state.clone())
  ```
  如果上游改了路由列表（增删 `.route(...)`），接受上游的路由改动即可，merge 分支不受影响。

**3. `src-tauri/src/database/mod.rs`**（**最需要注意**）—— 我们的 `init()` 主体移到了 `open_at_inner(db_path, register_hook)`。如果**上游改了 `init()`**（比如加了新表迁移、新 PRAGMA），需要把这些改动**同步到 `open_at_inner` 函数体**（因为 `init` 现在只是一行调用）：
- 冲突时：接受上游对 init 逻辑的改动，手动把同样的改动应用到 `open_at_inner` 里。
- `init()` 与 `open_at()` 的签名/调用保持不变。
- 如果上游改动很大或很频繁，可考虑让 `open_at` 独立（不共享 `open_at_inner`，自己复制连接逻辑），牺牲一点 DRY 换取零触碰 `init`。

> 新增文件（`standalone/`、`bin/`）不会和上游冲突（上游没有这些文件）。

### 同步后验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib standalone::
cargo build --manifest-path src-tauri/Cargo.toml --bin cc-switch-proxy
```

---

## 十、开发与测试

```bash
# 单元测试（open_at 建表、admin DTO 映射、地址归一化、model 提取）
cargo test --manifest-path src-tauri/Cargo.toml --lib standalone::

# 端到端冒烟
./src-tauri/target/debug/cc-switch-proxy --db /tmp/test.db --port 15921 &
sleep 2
curl http://127.0.0.1:15921/admin/status
# 用完 kill %1
```

---

## 十一、常见问题

**Q: 能和 cc-switch GUI 同时开吗？**
A: 能。CLI 用独立 DB（默认 `cli-proxy.db`），GUI 用 `cc-switch.db`，互不干扰。但两者默认都监听 15721，CLI 用 `--port` 改一个即可。

**Q: admin API 无鉴权安全吗？**
A: 进程强制绑回环（`--address` 非 `127.0.0.1`/`localhost`/`::1` 拒绝启动），局域网/公网访问不到。如需远程管理，前置反向代理 + 鉴权。

**Q: 支持哪些模型？**
A: 任何 OpenAI Chat Completions 或 Responses 兼容上游。国产模型用 `api_format=openai_chat`。OAuth 类（Copilot / CodexOAuth）不支持（CLI 不带 Tauri 凭证托管）。

**Q: 怎么改代理配置（超时/重试/熔断）？**
A: 当前 CLI 用默认 `ProxyConfig`。如需可配，扩展 `standalone::run` 读配置文件，或未来通过 `/admin/config` 端点（未实现）。

**Q: 编译太慢/体积太大？**
A: 因为 bin 链接了整个 lib（含 tauri）。未来可用 cargo feature 把 tauri 部分门控掉瘦身（未实现，见设计文档 §13）。
