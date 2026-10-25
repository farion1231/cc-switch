# Huawei CodeAgent 3.0 完整方案

> **2026-10-24 更新说明：** 本文的 8888 连通性代理与 TLS 证书方案已被移除。当前实现及排查结论见 [Huawei CodeAgent 与 Claude Code 排查记录](guides/huawei-codeagent-troubleshooting.md)；构建见 [Windows Tauri 构建](guides/windows-build.md)。

> 本文档保留历史设计（hook 隔离、CodeAgent 3.0 provider、token 自动刷新）和 TTY 兼容模式方案，供追溯设计决策。

## 一、架构总览

```
cc-switch (Tauri App)
├── 核心代理 (proxy/)
│   ├── server.rs          — Axum HTTP 路由
│   ├── forwarder.rs       — 请求转发 + Hook 调用
│   ├── hooks.rs           — ProxyHook trait + HookRegistry
│   └── providers/         — 协议转换 (Anthropic/OpenAI/Gemini)
│
├── Huawei 扩展 (huawei/)
│   ├── mod.rs             — 初始化 + Hook 注册
│   ├── proxy_hooks.rs     — HuaweiCodeAgentProxyHook 实现
│   ├── proxy_processor.rs — 指纹替换 + 动态 token
│   ├── auth.rs            — W3 认证 + token 管理
│   ├── connectivity_proxy.rs — [新增] 独立连通性代理
│   ├── tls_cert.rs        — [新增] TLS 证书管理
│   └── claude_config.rs   — [新增] ~/.claude.json 管理
│
└── 服务层 (services/)
    └── proxy.rs           — 接管/恢复 + Hook 生命周期调用
```

**核心原则**：Huawei 代码收敛到 `huawei/` 目录；核心文件只暴露通用扩展点；运行时注册 + 编译时可选。

## 二、ProxyHook 架构

### 2.1 Trait 定义

```rust
pub trait ProxyHook: Send + Sync {
    // 请求处理
    fn matches_provider(&self, _meta: &ProviderMeta) -> bool { false }
    fn is_managed_auth(&self) -> bool { false }
    fn skip_auth_token(&self) -> bool { false }
    fn force_api_format(&self, _meta: &ProviderMeta) -> Option<String> { None }
    fn on_request_body(&self, _body: &mut Value, _meta: &ProviderMeta) {}
    fn on_auth_headers(&self, _meta: &ProviderMeta, _app_handle: Option<&AppHandle>, _body: Option<&Value>) -> Pin<Box<dyn Future<Output = Vec<(String, String)>> + Send>> { Box::pin(async { vec![] }) }
    fn filter_outbound_headers(&self, _meta: &ProviderMeta, _headers: &mut HeaderMap) {}
    fn on_build_url(&self, _meta: &ProviderMeta, _url: &mut String) {}
    fn skip_tls_verify(&self, _meta: &ProviderMeta) -> bool { false }
    fn on_response_body(&self, _meta: &ProviderMeta, _body: &[u8]) -> Option<Vec<u8>> { None }
    fn transform_upstream_error(&self, _meta: &ProviderMeta, _body: &Value) -> Option<ProxyError> { None }

    // [新增] 生命周期
    fn on_takeover(&self, _meta: &ProviderMeta) -> Result<(), String> { Ok(()) }
    fn on_restore(&self, _meta: &ProviderMeta) -> Result<(), String> { Ok(()) }
}
```

### 2.2 HookRegistry

全局单例，`LazyLock` + `RwLock<Vec<Box<dyn ProxyHook>>>`。注册在 `huawei::init()` 中完成。

### 2.3 调用点

| 调用点 | 位置 | 调用的方法 |
|--------|------|-----------|
| 接管完成 | `services/proxy.rs` `start_with_takeover` | `on_takeover` |
| 恢复开始 | `services/proxy.rs` `stop_with_restore` | `on_restore` |
| 请求体处理 | `proxy/forwarder.rs` | `on_request_body` |
| 认证头注入 | `proxy/forwarder.rs` | `on_auth_headers` |
| 出站头过滤 | `proxy/forwarder.rs` | `filter_outbound_headers` |
| URL 构建 | `proxy/forwarder.rs` | `on_build_url` |
| TLS 验证 | `proxy/forwarder.rs` | `skip_tls_verify` |
| 响应体处理 | `proxy/response_processor.rs` | `on_response_body` |
| 错误转换 | `proxy/forwarder.rs` | `transform_upstream_error` |
| API 格式 | `proxy/providers/claude.rs` | `force_api_format` |
| Provider 匹配 | `provider.rs` / `services/proxy.rs` | `matches_provider` |

## 三、HuaweiCodeAgentProxyHook 实现

### 3.1 Provider 匹配

匹配 `provider_type`：`huawei_codeagent_2`、`huawei_codeagent_3`、`huawei_w3`（别名）、`huawei_idea`（别名）。

### 3.2 请求处理

| 方法 | CodeAgent 2.0 | CodeAgent 3.0 |
|------|---------------|---------------|
| `is_managed_auth` | `true` | `true` |
| `skip_auth_token` | `true` | `true` |
| `force_api_format` | `"openai_chat"` | `"openai_chat"` |
| `on_request_body` | 指纹替换 | 指纹替换 |
| `on_auth_headers` | 8 个认证头 | 8 个认证头（不同） |
| `filter_outbound_headers` | 白名单 15 个 | 白名单 |
| `on_build_url` | stripPathPrefix | stripPathPrefix |
| `skip_tls_verify` | `true` | `true` |
| `on_response_body` | 指纹替换 | 指纹替换 |
| `transform_upstream_error` | `to_anthropic` | `to_anthropic` |

### 3.3 生命周期处理 [新增]

**`on_takeover`**：
1. 判断 `provider_type == huawei_codeagent_3`，否则返回 `Ok(())`
2. 启动独立连通性代理（如果尚未运行）
3. 将 `PROXY_MANAGED` 写入 `~/.claude.json` 的 `customApiKeyResponses.approved`

**`on_restore`**：
1. 判断 `provider_type == huawei_codeagent_3`，否则返回 `Ok(())`
2. 停止独立连通性代理
3. 从 `~/.claude.json` 的 `customApiKeyResponses.approved` 中移除 `PROXY_MANAGED`

## 四、TTY 兼容接管策略

### 4.1 标准 vs TTY 兼容模式

| 配置项 | 标准模式 | TTY 兼容模式 |
|--------|---------|-------------|
| `ANTHROPIC_BASE_URL` | `http://127.0.0.1:15721` | **不设置** |
| `ANTHROPIC_API_KEY` | `PROXY_MANAGED` | `PROXY_MANAGED` |
| `HTTPS_PROXY` | 不设置 | `http://127.0.0.1:8888` |
| `HTTP_PROXY` | 不设置 | `http://127.0.0.1:8888` |
| `NODE_TLS_REJECT_UNAUTHORIZED` | 不设置 | `0` |
| `NO_PROXY` | 不设置 | `127.0.0.1,localhost` |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | 不设置 | `1` |
| 模型别名 | 保留 | 保留 |

### 4.2 触发条件

当 `apply_claude_takeover_fields_for_provider` 检测到 hook 的 `matches_provider` 返回 true 且 `provider_type == huawei_codeagent_3` 时，使用 TTY 兼容策略。

### 4.3 三道门控的解决

| 门控 | 标准模式 | TTY 兼容模式 |
|------|---------|-------------|
| 连通性检查 | ❌ `api.anthropic.com` 不可达 | ✅ 独立代理拦截返回 200 |
| 第一方验证 | ❌ `ANTHROPIC_BASE_URL` 非默认 | ✅ 不设置 BASE_URL，使用默认 |
| API Key 批准 | ❌ 需手动批准 | ✅ `on_takeover` 自动批准 |

## 五、独立连通性代理

### 5.1 架构

```
Claude Code (TTY)
    │
    ├── HTTPS_PROXY = http://127.0.0.1:8888
    │
    ├── CONNECT api.anthropic.com:443
    │   └── TLS 隧道 (自签名证书)
    │       ├── GET /api/hello → 200 {"ok":true}
    │       ├── GET /api/claude_code/settings → 200 {}
    │       ├── GET /api/claude_code/policy_limits → 200 {}
    │       └── POST /v1/messages → 转发到 127.0.0.1:15721
    │
    ├── CONNECT platform.claude.com:443
    │   └── TLS 隧道
    │       └── GET /v1/oauth/hello → 200 {"ok":true}
    │
    └── CONNECT other-host:443
        └── 转发到上游代理 (公司代理)
```

### 5.2 实现

- Rust 原生异步（tokio + tokio-rustls）
- 作为 cc-switch 进程内异步任务（非子进程）
- 通过 `oneshot` channel 控制停止（复用 `ProxyServer` 生命周期模式）
- 日志输出到 `~/.cc-switch/logs/connectivity-proxy.log`

### 5.3 日志格式

```
[2026-08-09T01:47:34Z] [INFO] Connectivity proxy listening on 127.0.0.1:8888
[2026-08-09T01:47:53Z] [INTERCEPT] api.anthropic.com:443 - creating TLS tunnel
[2026-08-09T01:47:53Z] [INTERCEPT] api.anthropic.com:443 - TLS handshake completed
[2026-08-09T01:47:53Z] [REQ] api.anthropic.com - GET /api/hello
[2026-08-09T01:47:53Z] [INTERCEPT] api.anthropic.com - returning 200 for /api/hello
[2026-08-09T01:49:20Z] [REQ] api.anthropic.com - POST /v1/messages?beta=true
[2026-08-09T01:49:20Z] [FORWARD] api.anthropic.com - POST /v1/messages?beta=true → 127.0.0.1:15721
[2026-08-09T01:49:25Z] [RESP] api.anthropic.com - 200 /v1/messages?beta=true
[2026-08-09T01:49:25Z] [ERROR] api.anthropic.com:443 - TLS error: read ECONNRESET
```

## 六、TLS 证书管理

- 证书路径：`~/.cc-switch/tls-certs/proxy-cert.pem`、`proxy-key.pem`
- SAN：`DNS:api.anthropic.com, DNS:platform.claude.com, DNS:*.claude.com, DNS:*.anthropic.com`
- 使用 `rcgen` crate 生成（纯 Rust，不依赖系统 openssl）
- 首次启动自动生成，有效期 10 年
- 通过 `NODE_TLS_REJECT_UNAUTHORIZED=0` 绕过验证（不安装到系统信任存储）

## 七、~/.claude.json 管理

### 7.1 操作函数

```rust
// huawei/claude_config.rs

/// 将 PROXY_MANAGED 从 rejected 移到 approved
pub fn approve_proxy_managed_key() -> Result<bool, AppError>

/// 从 approved 中移除 PROXY_MANAGED
pub fn remove_proxy_managed_key() -> Result<bool, AppError>
```

### 7.2 增量合并原则

复用 `claude_mcp.rs` 的 `read_json_value`/`write_json_value`/`atomic_write` 模式：
- 读取整个 `~/.claude.json`
- 仅修改 `customApiKeyResponses` 字段
- 原子写回
- 不破坏 `hasCompletedOnboarding`、`mcpServers`、`projects` 等其他字段

## 八、Provider 配置

### 8.1 CodeAgent 3.0 认证头

| 头名称 | 值/来源 | tokenSource |
|--------|---------|-------------|
| `x-auth-token` | W3 认证 token | `huawei_w3` |
| `Authorization` | `Bearer <token>` | `huawei_w3` (valuePrefix: `Bearer `) |
| `X-snap-traceid` | UUID v4 | `uuid` |
| `Agent-Type` | `AgentCenter` | 固定值 |
| `app-id` | `com.huawei.devmind.codebot.apibot` | 固定值 |
| `x-innercc-request-kind` | `chat` | 固定值 |
| `x-error-propagation` | `true` | 固定值 |
| `Model-Id` | 请求体中的 model | `request_model` |

### 8.2 上游地址

`https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2`

### 8.3 前端 Preset

```json
{
  "env": {
    "ANTHROPIC_API_KEY": "PROXY_MANAGED",
    "HTTPS_PROXY": "http://127.0.0.1:8888",
    "HTTP_PROXY": "http://127.0.0.1:8888",
    "NODE_TLS_REJECT_UNAUTHORIZED": "0",
    "NO_PROXY": "127.0.0.1,localhost",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "maas-glm-5.1-zhipu",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME": "maas-glm-5.1-zhipu",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-8",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "maas-glm-5.1-zhipu",
    "CLAUDE_CODE_SUBAGENT_MODEL": "maas-glm-5.1-zhipu"
  }
}
```

**注意**：不设置 `ANTHROPIC_BASE_URL`。

## 九、Token 自动刷新

### 9.1 三状态懒刷新

| 状态 | 条件 | 行为 |
|------|------|------|
| Valid | 剩余有效期 > 5 分钟 | 返回缓存 token |
| Buffer | 剩余有效期 ≤ 5 分钟 | 返回缓存 token + 异步刷新 |
| Expired | 已过期 | 同步刷新 |

### 9.2 加密存储

- 算法：AES-256-GCM
- 密钥：`SHA-256(machine_id + salt)`，`machine_id = hostname + username`
- 存储：`encrypted_password` 字段

## 十、测试策略

### 10.1 Seam 1：纯函数测试

**位置**：`services/proxy.rs` inline tests

**测试**：
- TTY 兼容模式生成的 config 不含 `ANTHROPIC_BASE_URL`
- 生成的 config 含 `HTTPS_PROXY`、`NODE_TLS_REJECT_UNAUTHORIZED=0`
- 非 TTY 兼容模式行为不变（回归测试）

### 10.2 Seam 2：~/.claude.json 字段管理

**位置**：`tests/huawei_codeagent.rs`

**测试**：
- `approve_proxy_managed_key()` 在空文件上创建 approved 列表
- `approve_proxy_managed_key()` 将 key 从 rejected 移到 approved
- 不破坏 `hasCompletedOnboarding`、`mcpServers` 等其他字段
- `remove_proxy_managed_key()` 从 approved 中移除

### 10.3 Seam 3：完整生命周期

**位置**：`services/proxy.rs` inline tests

**测试**：
- `start_with_takeover` 后 settings.json 和 .claude.json 正确写入
- `stop_with_restore` 后全部恢复
- 连通性代理端口在接管后监听、恢复后停止

## 十一、文件清单

### 新增文件

| 文件 | 职责 |
|------|------|
| `src-tauri/src/huawei/connectivity_proxy.rs` | 独立连通性代理 |
| `src-tauri/src/huawei/tls_cert.rs` | TLS 证书管理 |
| `src-tauri/src/huawei/claude_config.rs` | ~/.claude.json 管理 |

### 修改文件

| 文件 | 改动 |
|------|------|
| `src-tauri/src/proxy/hooks.rs` | trait 新增 `on_takeover`/`on_restore`（默认空实现） |
| `src-tauri/src/services/proxy.rs` | `start_with_takeover`/`stop_with_restore` 调用生命周期方法 |
| `src-tauri/src/huawei/proxy_hooks.rs` | 实现 `on_takeover`/`on_restore` |
| `src-tauri/src/huawei/mod.rs` | 模块声明 |
| `src-tauri/Cargo.toml` | 新增 `rcgen`、`tokio-rustls` |
| `src/huawei/huaweiCodeAgent3ProviderPresets.ts` | 更新 preset |

### 不修改的核心文件

- `proxy/server.rs` — 路由不变
- `proxy/forwarder.rs` — 转发逻辑不变
- `proxy/handlers.rs` — 处理函数不变
- `provider.rs` — Provider 模型不变
- `app_config.rs` — AppType 不变
- `database/schema.rs` — 表结构不变

## 十二、依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `rcgen` | `0.13` | 自签名证书生成 |
| `tokio-rustls` | `0.26` | TLS 服务端 |
| `tokio` | 已有 | 异步运行时 |
| `axum` | 已有 | HTTP 路由 |
| `serde_json` | 已有 | JSON 读写 |
