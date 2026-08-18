# Claude `count_tokens` 透传 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 cc-switch 路由模式透传 `/v1/messages/count_tokens`,消除 Claude Desktop 因该端点 404 而退化为 `max_tokens=1` 探测请求的异常调用。

**Architecture:** 在 axum router 注册三条 `count_tokens` 路由(`/v1/messages/count_tokens`、`/claude/v1/messages/count_tokens`、`/claude-desktop/v1/messages/count_tokens`),复用现有 `handle_messages_for_app` 的请求编排(RequestContext → forwarder → process_response),但**始终透传**:跳过 openai/gemini 格式转换、跳过 media 剥离、跳过 thinking 改写。在 `RequestForwarder` 内部用 `is_count_tokens_endpoint` 判定,把 `needs_transform` 强制为 `false`、绕过 `apply_media_prevention`,并在 full-url 配成 `.../v1/messages` 时把出站 URL 改写为 `.../v1/messages/count_tokens`。

**Tech Stack:** Rust + axum + tokio + serde_json(现有栈,无新依赖)

## Global Constraints

- 不引入新 crate 依赖;只复用 `forwarder.rs`/`handlers.rs`/`server.rs` 现有符号。
- `count_tokens` 永远透传:不做 `Claude ↔ OpenAI/Gemini` 格式转换,不做 media 剥离,不做 thinking 改写。
- `Claude Desktop` gateway 路由必须经 `validate_claude_desktop_gateway_auth` 鉴权(与 `/claude-desktop/v1/messages` 一致)。
- `forwarder.rs` 内 `is_count_tokens_endpoint` / `rewrite_full_messages_url_to_count_tokens` 为纯函数,必须带单元测试(TDD)。
- 代码注释与命名沿用文件现有中文注释风格(参考 `handle_messages` / `apply_media_prevention` 注释)。
- 编译目标:`cd src-tauri && cargo check` 必须通过;新增单元测试必须通过 `cargo test --lib`。

---

## File Structure

| 文件 | 责任 | 改动类型 |
|---|---|---|
| `src-tauri/src/proxy/forwarder.rs` | `is_count_tokens_endpoint` 判定 + full-url 改写 + 透传逻辑(跳过 transform / media prevention) + 2 个纯函数单元测试 | Modify |
| `src-tauri/src/proxy/handlers.rs` | `handle_count_tokens` / `handle_claude_desktop_count_tokens` / `handle_count_tokens_for_app` 三个 axum handler | Modify |
| `src-tauri/src/proxy/server.rs` | 注册 3 条 `count_tokens` 路由 | Modify |

**接口契约(跨任务):**
- `forwarder.rs` 产出纯函数:
  - `fn is_count_tokens_endpoint(endpoint: &str) -> bool`
  - `fn rewrite_full_messages_url_to_count_tokens(base_url: &str, extra_query: Option<&str>) -> String`
- `handlers.rs` 产出 axum handler:
  - `pub async fn handle_count_tokens(State(state), request) -> Result<Response, ProxyError>`
  - `pub async fn handle_claude_desktop_count_tokens(State(state), request) -> Result<Response, ProxyError>`
- `server.rs` 在 `build_router` 中 `.route(...)` 引用上述两个 handler。

---

### Task 1: forwarder.rs 纯函数 + 单元测试(TDD)

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`(在 `is_messages_endpoint` 附近 ~2726 行新增两个 fn;在文件末尾 `#[cfg(test)] mod tests` 内新增两个 `#[test]`)
- Test: `src-tauri/src/proxy/forwarder.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: 现有 `split_endpoint_and_query`(forwarder.rs:2704)、`append_query_to_full_url`(forwarder.rs:3062)
- Produces:
  - `fn is_count_tokens_endpoint(endpoint: &str) -> bool`
  - `fn rewrite_full_messages_url_to_count_tokens(base_url: &str, extra_query: Option<&str>) -> String`

- [ ] **Step 1: 写失败测试**

在 `forwarder.rs` 末尾的 `#[cfg(test)] mod tests` 块内(紧邻已有的 `rewrite_codex_responses_endpoint_to_chat_preserves_query` 测试附近)新增:

```rust
    #[test]
    fn is_count_tokens_endpoint_matches_known_paths() {
        assert!(is_count_tokens_endpoint("/v1/messages/count_tokens"));
        assert!(is_count_tokens_endpoint(
            "/claude-desktop/v1/messages/count_tokens?x=1"
        ));
        assert!(is_count_tokens_endpoint("/claude/v1/messages/count_tokens"));
        // 任意前缀的 .../messages/count_tokens 也算命中(覆盖中转网关)
        assert!(is_count_tokens_endpoint(
            "https://relay.example/api/v1/messages/count_tokens"
        ));
        assert!(!is_count_tokens_endpoint("/v1/messages"));
        assert!(!is_count_tokens_endpoint("/v1/messages/count"));
    }

    #[test]
    fn rewrite_full_messages_url_to_count_tokens_preserves_query() {
        assert_eq!(
            rewrite_full_messages_url_to_count_tokens(
                "https://relay.example/api/v1/messages",
                None
            ),
            "https://relay.example/api/v1/messages/count_tokens"
        );
        assert_eq!(
            rewrite_full_messages_url_to_count_tokens(
                "https://relay.example/api/v1/messages?beta=true",
                Some("x=1")
            ),
            "https://relay.example/api/v1/messages/count_tokens?beta=true&x=1"
        );
        assert_eq!(
            rewrite_full_messages_url_to_count_tokens(
                "https://relay.example/api/v1/messages/",
                None
            ),
            "https://relay.example/api/v1/messages/count_tokens"
        );
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run:
```bash
cd src-tauri && cargo test --lib proxy::forwarder::tests::is_count_tokens_endpoint_matches_known_paths proxy::forwarder::tests::rewrite_full_messages_url_to_count_tokens_preserves_query
```
Expected: 编译错误 `cannot find function is_count_tokens_endpoint` / `rewrite_full_messages_url_to_count_tokens`。

- [ ] **Step 3: 实现两个纯函数**

在 `forwarder.rs` 中 `is_messages_endpoint` 函数紧邻处(约 2726 行,该函数形如 `matches!(path, "/v1/messages" | "/claude/v1/messages")`)新增:

```rust
/// Anthropic Token Count API 路径(含本地 gateway 前缀形态)。
///
/// 用于在转发器内部判定 count_tokens 请求:这类请求必须始终透传,
/// 不参与 messages 的 openai/gemini 格式转换,也不做 media 剥离。
fn is_count_tokens_endpoint(endpoint: &str) -> bool {
    let (path, _) = split_endpoint_and_query(endpoint);
    matches!(
        path,
        "/v1/messages/count_tokens"
            | "/claude/v1/messages/count_tokens"
            | "/claude-desktop/v1/messages/count_tokens"
    ) || path.ends_with("/messages/count_tokens")
}

/// 把 full-url 形态的 `.../v1/messages` 改写为 `.../v1/messages/count_tokens`。
///
/// 保留原 URL 上的 query / fragment,并合并额外透传 query(`append_query_to_full_url`
/// 负责 `?`/`&` 拼接)。用于 base_url 直接配成完整 messages 端点时,
/// 把计数请求打到正确的 count_tokens 端点而非 messages 端点。
fn rewrite_full_messages_url_to_count_tokens(base_url: &str, extra_query: Option<&str>) -> String {
    let trimmed = base_url.trim();
    let (path_part, suffix) = match trimmed.find(['?', '#']) {
        Some(idx) => (&trimmed[..idx], &trimmed[idx..]),
        None => (trimmed, ""),
    };
    let path = path_part.trim_end_matches('/');
    let rewritten = format!("{path}/count_tokens{suffix}");
    append_query_to_full_url(&rewritten, extra_query)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run:
```bash
cd src-tauri && cargo test --lib proxy::forwarder::tests::is_count_tokens_endpoint_matches_known_paths proxy::forwarder::tests::rewrite_full_messages_url_to_count_tokens_preserves_query
```
Expected: 2 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/forwarder.rs
git commit -m "feat(proxy): add count_tokens endpoint helpers + tests"
```

---

### Task 2: forwarder.rs 透传逻辑(跳过 transform / media prevention / URL 改写)

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`
  - 在 ~1167 行(`normalize_thinking_type` 之前)插入 `is_count_tokens` 判定
  - 在 ~1331 行(`apply_media_prevention` 调用处)用 `is_count_tokens` 守卫
  - 在 ~1334 行(`needs_transform` 处)对 count_tokens 强制 `false`
  - 在 ~1379 行(`let url = ...` 处)新增 count_tokens 的 full-url 改写分支

**Interfaces:**
- Consumes: Task 1 产出的 `is_count_tokens_endpoint` / `rewrite_full_messages_url_to_count_tokens`;现有 `base_url_is_full_endpoint`(2786)、`is_full_url`(forwarder.rs:1383 处使用的变量)、`codex_anthropic_base_is_full_endpoint`(1376)
- Produces: forwarder 对 count_tokens 请求的行为变更(无新公开符号)

- [ ] **Step 1: 写失败测试(行为级,确认 count_tokens 不被 media 剥离)**

由于 `forward_with_retry` 是集成级函数、需要完整 `ProxyState`/provider 装配,行为测试成本高且超出本任务单测边界,本步骤改为**编译契约测试**:确认新逻辑的判定变量存在且语义正确(纯函数部分已在 Task 1 覆盖)。此处仅新增一个回归测试,确保 `is_count_tokens_endpoint("/")` 之类负例不误命中 full-url 改写路径:

在 `#[cfg(test)] mod tests` 内新增:

```rust
    #[test]
    fn is_count_tokens_endpoint_rejects_plain_messages() {
        // 关键回归:普通 messages 端点绝不能被识别成 count_tokens,
        // 否则 needs_transform 会被错误强制为 false,破坏格式转换。
        assert!(!is_count_tokens_endpoint("/v1/messages"));
        assert!(!is_count_tokens_endpoint("/claude/v1/messages"));
        assert!(!is_count_tokens_endpoint("/claude-desktop/v1/messages"));
        assert!(!is_count_tokens_endpoint("/chat/completions"));
    }
```

- [ ] **Step 2: 跑测试确认通过(此测试不依赖未写的逻辑,先确认基线绿)**

Run:
```bash
cd src-tauri && cargo test --lib proxy::forwarder::tests::is_count_tokens_endpoint_rejects_plain_messages
```
Expected: 1 passed(Task 1 已提供函数实现)。

- [ ] **Step 3: 插入 `is_count_tokens` 判定变量**

定位到 ~1167 行,当前代码为:

```rust
        // 与 CCH 对齐：请求前不做 thinking 主动改写（仅保留兼容入口）
        let mut mapped_body = normalize_thinking_type(mapped_body);
```

在其**之前**插入:

```rust
        // Token Count 必须始终按 Anthropic `/v1/messages/count_tokens` 透传：
        // 不能走 openai/gemini 格式转换，也不能被 media prevention 改写 body。
        let is_count_tokens = is_count_tokens_endpoint(endpoint);

```

- [ ] **Step 4: 用 `is_count_tokens` 守卫 `apply_media_prevention`**

定位到 ~1324-1333 行,当前代码为:

```rust
        if adapter.name() == "Claude" {
            if let Some(api_format) = resolved_claude_api_format.as_deref() {
                super::providers::normalize_anthropic_messages_for_provider(
                    &mut mapped_body,
                    provider,
                    api_format,
                );
                self.apply_media_prevention(&mut mapped_body, provider);
            }
        }
        let needs_transform = match resolved_claude_api_format.as_deref() {
            Some(api_format) => super::providers::claude_api_format_needs_transform(api_format),
            None => adapter.needs_transform(provider),
        };
```

替换为:

```rust
        if adapter.name() == "Claude" {
            if let Some(api_format) = resolved_claude_api_format.as_deref() {
                super::providers::normalize_anthropic_messages_for_provider(
                    &mut mapped_body,
                    provider,
                    api_format,
                );
                // count_tokens 的 body 结构本身就是计数输入，不能做 media 剥离，
                // 否则 Desktop 看到的上下文占用会系统性偏低。
                if !is_count_tokens {
                    self.apply_media_prevention(&mut mapped_body, provider);
                }
            }
        }
        // count_tokens 永远透传：忽略 apiFormat 的 openai/gemini 转换开关。
        let needs_transform = if is_count_tokens {
            false
        } else {
            match resolved_claude_api_format.as_deref() {
                Some(api_format) => {
                    super::providers::claude_api_format_needs_transform(api_format)
                }
                None => adapter.needs_transform(provider),
            }
        };
```

- [ ] **Step 5: 新增 count_tokens 的 full-url 改写分支**

定位到 ~1379 行,当前代码为:

```rust
        let url = if matches!(resolved_claude_api_format.as_deref(), Some("gemini_native")) {
            super::gemini_url::resolve_gemini_native_url(
                &base_url,
                &effective_endpoint,
                is_full_url,
            )
        } else if is_full_url
            || codex_chat_base_is_full_endpoint
            || codex_anthropic_base_is_full_endpoint
        {
            append_query_to_full_url(&base_url, passthrough_query.as_deref())
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };
```

替换为(在 `gemini_native` 分支上加 `&& !is_count_tokens` 守卫,并在最前面插入 count_tokens full-url 改写分支):

```rust
        let url = if is_count_tokens
            && (is_full_url || codex_anthropic_base_is_full_endpoint)
            && base_url_is_full_endpoint(&base_url, "/v1/messages")
        {
            // full URL 配成了 `.../v1/messages` 时，count_tokens 要改写到
            // `.../v1/messages/count_tokens`，避免把计数请求打到 messages 端点。
            rewrite_full_messages_url_to_count_tokens(&base_url, passthrough_query.as_deref())
        } else if matches!(resolved_claude_api_format.as_deref(), Some("gemini_native"))
            && !is_count_tokens
        {
            super::gemini_url::resolve_gemini_native_url(
                &base_url,
                &effective_endpoint,
                is_full_url,
            )
        } else if is_full_url
            || codex_chat_base_is_full_endpoint
            || codex_anthropic_base_is_full_endpoint
        {
            append_query_to_full_url(&base_url, passthrough_query.as_deref())
        } else {
            adapter.build_url(&base_url, &effective_endpoint)
        };
```

> **注意:** `is_full_url` 是 `forward_with_retry_inner` 作用域内已有的变量(在 ~1383 行 `resolve_gemini_native_url` 调用中已被使用),无需新增声明。`endpoint` 是 `forward_with_retry` 的参数,`passthrough_query` 在 ~1350 行 `(effective_endpoint, passthrough_query)` 解构处已定义,本任务对其无改动。

- [ ] **Step 6: 编译确认**

Run:
```bash
cd src-tauri && cargo check
```
Expected: 编译通过(0 error)。若报 `cannot find value is_full_url`,说明锚点版本漂移,在 `let url =` 之前搜索 `is_full_url` 的实际声明处确认作用域可见。

- [ ] **Step 7: 跑全部 forwarder 测试**

Run:
```bash
cd src-tauri && cargo test --lib proxy::forwarder::tests
```
Expected: 全部 passed(含 Task 1 的 2 个 + Step 1 的 1 个回归)。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/proxy/forwarder.rs
git commit -m "feat(proxy): passthrough count_tokens without transform/media-prevention"
```

---

### Task 3: handlers.rs 新增 count_tokens handler

**Files:**
- Modify: `src-tauri/src/proxy/handlers.rs`(在 `handle_messages` / `handle_claude_desktop_messages` 之后 ~138 行插入三个 fn)

**Interfaces:**
- Consumes(全部为文件内现有符号,无需 import,已在文件顶部 `use super::{...}` 覆盖):
  - `RequestContext::new(handler_context.rs:88)`、`ctx.create_forwarder(state)(:201)`、`ctx.get_providers()(:250)`
  - `forwarder.forward_with_retry(...)` 签名:`(app_type: &AppType, method: http::Method, endpoint: &str, body: Value, headers: HeaderMap, extensions: Extensions, providers: Vec<Provider>) -> Result<ForwardResult, ForwardError>`
  - `ForwardResult` 字段:`response: ProxyResponse`、`provider: Provider`、`outbound_model: Option<String>`、`connection_guard: Option<ActiveConnectionGuard>`(crate-private,同模块可见)
  - `ForwardError { error: ProxyError, provider: Option<Provider> }`
  - `validate_claude_desktop_gateway_auth(&state, &headers)(handlers.rs:256)`
  - `log_forward_error(state, ctx, is_stream, &err.error)(handlers.rs:2303)`
  - `process_response(response, ctx, state, &CLAUDE_PARSER_CONFIG, connection_guard)(response_processor.rs:321)`
  - `crate::app_config::AppType::{Claude, ClaudeDesktop}`(已 use 在 :45)
- Produces:
  - `pub async fn handle_count_tokens(State(state), request) -> Result<Response, ProxyError>`
  - `pub async fn handle_claude_desktop_count_tokens(State(state), request) -> Result<Response, ProxyError>`

- [ ] **Step 1: 写失败测试(handler 存在性编译契约)**

handler 是 axum 端点、需完整 router 装配才能端到端测试,单测成本超出本任务边界。本步骤改为**编译契约**:确认 handler 函数可被 `server.rs` 引用(Task 4 会引用)。无独立单测;依赖 Task 4 的 `cargo check` 作为通过门。此 Step 仅记录约束,不写测试代码。

- [ ] **Step 2: 实现三个 handler**

在 `handle_claude_desktop_messages` 函数结束(约 138 行 `}`)之后、`handle_claude_desktop_models`(140 行)之前插入:

```rust
/// 处理 `/v1/messages/count_tokens`(Claude API)
///
/// 始终透传到上游：只做供应商选择 / 模型映射 / 鉴权，不做 openai/gemini 格式转换。
/// 上游若不支持该接口，会把上游错误原样返回给客户端。
pub async fn handle_count_tokens(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_count_tokens_for_app(state, request, AppType::Claude, "Claude", "claude", None).await
}

/// 处理 Claude Desktop gateway 的 `/v1/messages/count_tokens`
pub async fn handle_claude_desktop_count_tokens(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, request.headers())?;
    handle_count_tokens_for_app(
        state,
        request,
        AppType::ClaudeDesktop,
        "Claude Desktop",
        "claude-desktop",
        Some("/claude-desktop"),
    )
    .await
}

async fn handle_count_tokens_for_app(
    state: ProxyState,
    request: axum::extract::Request,
    app_type: AppType,
    tag: &'static str,
    app_type_str: &'static str,
    strip_prefix: Option<&'static str>,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, app_type.clone(), tag, app_type_str).await?;

    let raw_endpoint = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str())
        .unwrap_or(uri.path());
    let endpoint = strip_prefix
        .and_then(|prefix| raw_endpoint.strip_prefix(prefix))
        .unwrap_or(raw_endpoint);

    // count_tokens 是非流式探测接口，不参与 messages 的格式转换分支。
    let forwarder = ctx.create_forwarder(&state);
    let mut result = match forwarder
        .forward_with_retry(
            &app_type,
            method,
            endpoint,
            body,
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, false, &err.error);
            return Err(err.error);
        }
    };

    let connection_guard = result.connection_guard.take();
    ctx.outbound_model = result.outbound_model.take();
    ctx.provider = result.provider;

    // 始终透传响应（不做 Claude openai/gemini transform）
    process_response(
        result.response,
        &ctx,
        &state,
        &CLAUDE_PARSER_CONFIG,
        connection_guard,
    )
    .await
}

```

> **类型核对:** `RequestContext` 字段 `provider`(可写,:41)、`outbound_model: Option<String>`(:55) 均 pub,可在外部赋值。`result.connection_guard` 为 `pub(crate)`,handlers 同 crate 可 `.take()`。`process_response` 签名见 response_processor.rs:321。`axum::extract::Request` / `axum::response::Response` / `http_body_util::BodyExt`(用于 `.collect()`)/ `bytes::Bytes` 均已在 handlers.rs 顶部 import(:47-50)。

- [ ] **Step 3: 编译确认**

Run:
```bash
cd src-tauri && cargo check
```
Expected: 编译通过。若报 `connection_guard is private`,确认 `forwarder.rs:69` 的 `pub(crate) connection_guard` 与 handlers 同 crate(均在 `crate::proxy`),可见性成立。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/proxy/handlers.rs
git commit -m "feat(proxy): add count_tokens handlers for Claude + Claude Desktop"
```

---

### Task 4: server.rs 注册路由 + 端到端编译验证

**Files:**
- Modify: `src-tauri/src/proxy/server.rs`(`build_router` 内 ~297-307 行)

**Interfaces:**
- Consumes: Task 3 产出的 `handlers::handle_count_tokens` / `handlers::handle_claude_desktop_count_tokens`
- Produces: 三条新路由对外可达

- [ ] **Step 1: 注册 `/v1/messages/count_tokens` 与 `/claude/v1/messages/count_tokens`**

定位到 server.rs ~297-298 行:

```rust
            .route("/v1/messages", post(handlers::handle_messages))
            .route("/claude/v1/messages", post(handlers::handle_messages))
```

在其后插入:

```rust
            // Token Count API：始终透传到上游（仅做模型映射 / 鉴权，不做格式转换）
            .route(
                "/v1/messages/count_tokens",
                post(handlers::handle_count_tokens),
            )
            .route(
                "/claude/v1/messages/count_tokens",
                post(handlers::handle_count_tokens),
            )
```

- [ ] **Step 2: 注册 `/claude-desktop/v1/messages/count_tokens`**

定位到 server.rs ~304-307 行:

```rust
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
```

在其后插入:

```rust
            .route(
                "/claude-desktop/v1/messages/count_tokens",
                post(handlers::handle_claude_desktop_count_tokens),
            )
```

- [ ] **Step 3: 编译 + 全量测试**

Run:
```bash
cd src-tauri && cargo check && cargo test --lib
```
Expected: `cargo check` 0 error;`cargo test --lib` 全 passed(含 Task 1/2 新增的 3 个 forwarder 测试)。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/proxy/server.rs
git commit -m "feat(proxy): register /v1/messages/count_tokens routes"
```

---

### Task 5: 本地构建产物替换 + 烟雾验证

**Files:**
- 无源码改动;仅构建 + 替换本地产物

**Interfaces:**
- Consumes: Task 1-4 的编译产物

- [ ] **Step 1: 构建本地产物**

Run:
```bash
cd src-tauri && cargo build --release
```
Expected: 生成 `src-tauri/target/release/cc-switch.exe`(或对应二进制/动态库,按项目实际产物)。

> **注意:** 若项目通过 `pnpm tauri build` 打包完整安装包,且你只想替换后端,可单跑 `cargo build --release` 取二进制;若需替换的是 Tauri 集成产物,改跑 `pnpm tauri build`。

- [ ] **Step 2: 替换本地 cc-switch 后端**

按你本地 cc-switch 安装方式,把新构建的二进制替换到运行目录(具体路径以你本地为准)。此 Step 由用户执行,不在自动化内。

- [ ] **Step 3: 烟雾验证(对照贴主脱敏日志)**

启动 Claude Desktop + cc-switch 路由模式,观察代理日志:
- 期望出现 `path /v1/messages/count_tokens` 透传请求,status 200,`reason: upstream_complete`。
- 期望不再出现大量 `max_tokens=1` 的 `/v1/messages` 降级探测请求。

- [ ] **Step 4: 收尾提交(若有未提交的 pnpm-workspace 等改动)**

```bash
git status
```
若仍有未提交的 `pnpm-workspace.yaml` 等(贴主 patch 里含 `allowBuilds` 块),按需单独提交;否则跳过。

---

## Self-Review

**1. Spec coverage:**
- 透传 `/v1/messages/count_tokens`(含 `/claude` 前缀)→ Task 4 路由 + Task 3 handler ✅
- 透传 `/claude-desktop/v1/messages/count_tokens`(带 gateway 鉴权)→ Task 4 路由 + Task 3 `handle_claude_desktop_count_tokens` 调 `validate_claude_desktop_gateway_auth` ✅
- 跳过 openai/gemini 格式转换 → Task 2 Step 4 `needs_transform = false` ✅
- 跳过 media 剥离 → Task 2 Step 4 `if !is_count_tokens` 守卫 ✅
- full-url `.../v1/messages` 改写为 `.../v1/messages/count_tokens` → Task 2 Step 5 ✅
- 纯函数单测 → Task 1 + Task 2 Step 1 ✅
- 编译 + 本地替换 → Task 4 Step 3 + Task 5 ✅

**2. Placeholder scan:** 无 "TBD"/"add appropriate error handling"/"similar to Task N"。所有代码块为完整可粘贴代码。Task 3 Step 1 / Task 5 Step 2 明确说明"无单测/用户执行"的理由,非占位。

**3. Type consistency:**
- `is_count_tokens_endpoint(endpoint: &str) -> bool`:Task 1 定义,Task 2 Step 3 使用,签名一致 ✅
- `rewrite_full_messages_url_to_count_tokens(base_url: &str, extra_query: Option<&str>) -> String`:Task 1 定义,Task 2 Step 5 使用,签名一致 ✅
- `handle_count_tokens` / `handle_claude_desktop_count_tokens`:Task 3 定义,Task 4 引用为 `handlers::handle_count_tokens` / `handlers::handle_claude_desktop_count_tokens`,命名一致 ✅
- `forward_with_retry` 参数顺序与 forwarder.rs:346 一致 ✅
- `process_response` 参数顺序与 response_processor.rs:321 一致 ✅
- `ForwardResult` 字段名 `response`/`provider`/`outbound_model`/`connection_guard` 与 forwarder.rs:58-69 一致 ✅
