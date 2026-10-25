# CodeAgent Desktop 支持 CodeAgent 2.0/3.0 供应商 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 CodeAgent Desktop 在本地路由(Proxy)模式下支持华为 CodeAgent 2.0/3.0 供应商;Direct 模式 reject(同 `github_copilot`/`codex_oauth`/`xai_oauth`)。

**Architecture:** Desktop 的 Proxy 模式 profile 指向 cc-switch 本地网关(`inferenceGatewayBaseUrl` + 网关 token + `inferenceModels` 路由)。Desktop 请求 → cc-switch 网关 → `HuaweiCodeAgentProxyHook` 复用 CLI 路径的 auth 头注入(`x-auth-token`/`Authorization`/`app-id` 等)+ 指纹脱敏 + `openai_chat` 转换 → snapengine 上游。v3 连通性代理对 Desktop 跳过(Desktop 是独立进程,不读 `~/.claude.json`)。

**Tech Stack:** Rust 1.92(backend)、Tauri 2.x、React/TypeScript 5(frontend)、`toml_edit`/`serde_json`(config)。Cargo check 用 `CC=cl.exe` wrapper + vcvars64 + 清空代理的批处理(见 [[rust-cargo-check-msvc-env]])。

**Spec:** `docs/superpowers/specs/2026-10-11-codeagent-desktop-huawei-support-design.md`

## Global Constraints

- **不改动 CLI 路径**:现有 CodeAgent (CLI) 的华为集成(`huawei/claude_config.rs`、`proxy_hooks.rs` 对 CLI 的行为)保持不变。本计划只**加** Desktop 支持。
- **Direct 模式 reject 华为**:同 `github_copilot`/`codex_oauth`/`xai_oauth`,在 `validate_direct_provider` reject 列表加 `huawei_codeagent_2`/`huawei_codeagent_3`/`huawei_w3`/`huawei_idea`。理由:华为上游要 `x-auth-token`/`app-id`/`X-snap-traceid` 多头认证,Desktop 的 `inferenceGatewayApiKey`(单 Bearer)profile schema 表达不出。
- **华为 provider 强制 Proxy 模式**:`provider_mode` 对华为 provider_type 返回 `CodeAgentDesktopMode::Proxy`。
- **v3 连通性代理对 Desktop 跳过**:`on_takeover`/`on_restore` 里通过 `meta.extra["appTarget"]` 区分 CLI(`"claude"`)和 Desktop(`"claude_desktop"`),只对 CLI 启动连通性代理。
- **provider_type 字节陷阱**:`CodeAgent` 在文件里是字节 `436c61756465`(claude),不是 `436f64454167656e74`(codeAgent)。编辑 Rust 标识符 `AppType::CodeAgent` 时用字节级 grep 验证(`python3 -c "data=open('f','rb').read(); print(data.count(bytes.fromhex('436c61756465')))"`)。
- **Cargo check 命令**(必用此批处理,否则 C 依赖编译失败):
  ```bat
  @echo off
  call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
  set CARGO_HTTP_PROXY=
  set HTTPS_PROXY=
  set HTTP_PROXY=
  set "PATH=C:\Program Files (x86)\Microsoft Visual Studio\Installer;%PATH%"
  set "CC=C:\Users\L00899~1\AppData\Local\Temp\cc_strip_brepo.bat"
  set "CXX=C:\Users\L00899~1\AppData\Local\Temp\cc_strip_brepo.bat"
  cd /d D:\Projects\WebStorm\cc-switch\src-tauri
  cargo +1.92.0-x86_64-pc-windows-msvc check 2>&1
  echo CARGO_EXIT=%errorlevel%
  ```
  存到 `/tmp/cargo_chk.bat`,跑 `cmd.exe //c "$(cygpath -w /tmp/cargo_chk.bat)"`。要求 `CARGO_EXIT=0` 且 0 个 `error[E`。

## File Structure

- `src-tauri/src/claude_desktop_config.rs`(修改):Desktop profile 写入器。加华为 provider 的 validate/mode/routes 逻辑。
- `src-tauri/src/huawei/proxy_hooks.rs`(修改):`on_takeover`/`on_restore` 加 `app_target` 区分,Desktop 跳过连通性代理。
- `src-tauri/src/provider.rs`(不改,只读):`ProviderMeta` 定义,`claude_desktop_model_routes: HashMap<String, CodeAgentDesktopModelRoute>` 字段已存在。
- `src/huawei/huaweiCodeAgent2Presets.ts`(修改):preset 加 `appTarget` 字段(CLI 用)。
- `src/huawei/huaweiCodeAgent3Presets.ts`(修改):同上。
- `src/huawei/types.ts`(修改):`ExtraAuthHeader` 旁边可能要加 `appTarget` 类型,或放 preset 顶层。
- `src/components/providers/forms/CodeAgentDesktopProviderForm.tsx`(修改):华为 provider 在 Desktop 表单里强制本地路由、不显示直连选项。
- `tests/components/CodeAgentDesktopProviderForm.test.tsx`(可能修改):加华为 provider 的 Desktop 表单测试。

## 关键已知(从代码探索)

- `ProviderMeta.extra` 是 `HashMap<String, serde_json::Value>` + `#[serde(flatten)]`,所以 `appTarget` 是 meta 顶层 key(不是嵌套在 "extra" 下)。读写用 `meta.extra.get("appTarget")`。
- `CodeAgentDesktopModelRoute`(`provider.rs:401`):`{ model: String, label_override: Option<String>, supports_1m: Option<bool> }`,serde camelCase。
- `DEFAULT_PROXY_ROUTES`(`claude_desktop_config.rs`):`[{ route_id: "claude-sonnet-5", env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL", supports_1m: true }, ...]` —— 路由 ID 用 CodeAgent-safe 名(`claude-*`),upstream model 从 env 取。
- 华为 preset 的 `env` 有 `ANTHROPIC_MODEL`/`ANTHROPIC_DEFAULT_SONNET_MODEL`/`ANTHROPIC_DEFAULT_OPUS_MODEL`/`ANTHROPIC_DEFAULT_HAIKU_MODEL`,值是 `claude-sonnet-4.6[1M]`(2.0)或 `maas-glm-5.1-zhipu`(3.0)。
- `proxy_model_routes(provider)`(`claude_desktop_config.rs:561`):从 `meta.claude_desktop_model_routes` 读路由。华为 preset 不填这个,要么 preset 加默认路由,要么 `proxy_model_routes` 为华为 provider 生成默认路由。

---

### Task 1: 后端 — `validate_direct_provider` reject 华为 provider

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs`(函数 `validate_direct_provider`,~line 374)
- Test: `src-tauri/src/claude_desktop_config.rs`(同文件的 `#[cfg(test)] mod tests` 块)

**Interfaces:**
- Consumes: `ProviderMeta`(`provider.rs`)、`AppError::localized`
- Produces: 无新接口,只改 `validate_direct_provider` 行为

**背景**:当前 `validate_direct_provider` 在 reject 列表里只有 `github_copilot`/`codex_oauth`/`xai_oauth`(line 374-376)。华为 provider 走 Direct 会"漏网"通过,但写出的 profile 只有单 Bearer token,华为上游 401。要在 reject 列表加华为 provider_type。

- [ ] **Step 1: 写失败测试**

在 `claude_desktop_config.rs` 的 `#[cfg(test)] mod tests` 块加测试(找一个现有 `validate_direct_provider` 的测试做模板,grep `validate_direct_provider` 在 tests 块的用法):

```rust
#[test]
fn validate_direct_provider_rejects_huawei_codeagent_2() {
    let provider = Provider::with_id(
        "huawei-2".to_string(),
        "Huawei 2".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
                "ANTHROPIC_AUTH_TOKEN": "test-token",
            }
        }),
        Some(serde_json::json!({
            "providerType": "huawei_codeagent_2",
            "claudeDesktopMode": "direct",
        }).into()),
    );
    let result = validate_direct_provider(&provider);
    assert!(result.is_err(), "Direct mode should reject huawei_codeagent_2");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("本地路由") || msg.to_lowercase().contains("local routing"),
        "error should mention local routing mode, got: {msg}");
}

#[test]
fn validate_direct_provider_rejects_huawei_codeagent_3() {
    let provider = Provider::with_id(
        "huawei-3".to_string(),
        "Huawei 3".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
                "ANTHROPIC_AUTH_TOKEN": "test-token",
            }
        }),
        Some(serde_json::json!({
            "providerType": "huawei_codeagent_3",
            "claudeDesktopMode": "direct",
        }).into()),
    );
    let result = validate_direct_provider(&provider);
    assert!(result.is_err(), "Direct mode should reject huawei_codeagent_3");
}
```

注:`Provider::with_id` 的第 4 参数(meta)签名可能是 `Option<ProviderMeta>` —— 看现有测试怎么构造 `Provider` with meta(grep `Provider::with_id` 在 tests 块)。`serde_json::json!({...}).into()` 把 `Value` 转 `ProviderMeta`(如果 `ProviderMeta: From<Value>`)—— 如果没有这个 impl,用 `ProviderMeta { provider_type: Some("huawei_codeagent_2".to_string()), claude_desktop_mode: Some(CodeAgentDesktopMode::Direct), ..Default::default() }` 直接构造。**实施时先看现有测试的构造方式,照抄。**

- [ ] **Step 2: 跑测试确认失败**

跑 cargo check(用 Global Constraints 里的批处理)。预期:测试编译失败或 `validate_direct_provider` 测试 fail(因为华为 provider 当前不被 reject)。

注:如果测试编译失败(构造 `Provider` 的方式不对),先修测试构造,再跑。

- [ ] **Step 3: 实现 reject 逻辑**

在 `validate_direct_provider` 的 `matches!` reject 列表(line 374-376)加华为 provider_type:

```rust
if matches!(
    meta.provider_type.as_deref(),
    Some("github_copilot")
        | Some("codex_oauth")
        | Some("xai_oauth")
        | Some("huawei_codeagent_2")
        | Some("huawei_codeagent_3")
        | Some("huawei_w3")
        | Some("huawei_idea")
) {
    return Err(AppError::localized(
        "claude_desktop.provider.type_unsupported",
        "华为 CodeAgent 在 CodeAgent Desktop 直连模式不支持（上游需要多头认证），请使用本地路由模式",
        "Huawei CodeAgent does not support CodeAgent Desktop direct mode (upstream requires multi-header auth); use local routing mode",
    ));
}
```

- [ ] **Step 4: 跑测试确认通过**

跑 cargo check。预期:`CARGO_EXIT=0`,两个新测试 pass。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_desktop_config.rs
git commit --no-verify -m "feat(desktop): reject Huawei providers in Direct mode

Huawei upstream requires multi-header auth (x-auth-token/app-id/X-snap-traceid/...)
that Desktop's single inferenceGatewayApiKey bearer profile can't express. Add
huawei_codeagent_2/3/w3/idea to the same reject list as github_copilot/codex_oauth/
xai_oauth, pointing users to local routing mode.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 2: 后端 — `provider_mode` 强制华为走 Proxy

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs`(函数 `provider_mode`,~line 229)
- Test: 同上 tests 块

**Interfaces:**
- Consumes: `ProviderMeta`、`CodeAgentDesktopMode`
- Produces: `provider_mode(provider) -> CodeAgentDesktopMode` 对华为返回 `Proxy`

**背景**:`provider_mode`(line 229)当前只读 `meta.claude_desktop_mode`,默认 `Direct`。华为 provider 必须强制 `Proxy`(Direct 会 401)。要在 `provider_mode` 最早加华为检查。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn provider_mode_forces_proxy_for_huawei() {
    let provider = Provider::with_id(
        "huawei-2".to_string(),
        "Huawei 2".to_string(),
        json!({"env": {"ANTHROPIC_BASE_URL": "https://example.com", "ANTHROPIC_AUTH_TOKEN": "x"}}),
        Some(serde_json::json!({
            "providerType": "huawei_codeagent_2",
            // 注意:不设 claudeDesktopMode,看 provider_mode 是否强制 Proxy
        }).into()),
    );
    assert_eq!(provider_mode(&provider), CodeAgentDesktopMode::Proxy);
}
```

(用 Task 1 Step 1 里学到的 `Provider` + `meta` 构造方式。)

- [ ] **Step 2: 跑测试确认失败**

跑 cargo check。预期:测试 fail(当前 `provider_mode` 返回 `Direct`)。

- [ ] **Step 3: 实现强制 Proxy**

修改 `provider_mode`:

```rust
pub fn provider_mode(provider: &Provider) -> CodeAgentDesktopMode {
    if let Some(meta) = provider.meta.as_ref() {
        // 华为 CodeAgent 必须走本地路由模式：上游需要多头认证，
        // 只有经 cc-switch 代理才能由 HuaweiCodeAgentProxyHook 注入。
        if matches!(
            meta.provider_type.as_deref(),
            Some("huawei_codeagent_2")
                | Some("huawei_codeagent_3")
                | Some("huawei_w3")
                | Some("huawei_idea")
        ) {
            return CodeAgentDesktopMode::Proxy;
        }
        if let Some(mode) = meta.claude_desktop_mode.clone() {
            return mode;
        }
    }
    CodeAgentDesktopMode::Direct
}
```

- [ ] **Step 4: 跑测试确认通过**

跑 cargo check。预期:`CARGO_EXIT=0`,新测试 pass。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_desktop_config.rs
git commit --no-verify -m "feat(desktop): force Proxy mode for Huawei providers

provider_mode returns Proxy for huawei_codeagent_2/3/w3/idea regardless of
claudeDesktopMode, since Direct can't satisfy Huawei's multi-header auth.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 3: 后端 — `proxy_model_routes` 为华为 provider 生成默认路由

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs`(函数 `proxy_model_routes`,~line 561)
- Test: 同上 tests 块

**Interfaces:**
- Consumes: `ProviderMeta.claude_desktop_model_routes`(`HashMap<String, CodeAgentDesktopModelRoute>`)、`provider.settings_config.env.ANTHROPIC_DEFAULT_*_MODEL`
- Produces: `Vec<ResolvedModelRoute>`(route_id 是 CodeAgent-safe 名,upstream_model 是华为模型名)

**背景**:`proxy_model_routes` 当前只读 `meta.claude_desktop_model_routes`(line 562-565)。如果华为 preset 不填这个 map,会报"missing model route mappings"。两个选择:
- (A) preset 填 `claudeDesktopModelRoutes`(前端 preset 加默认路由)
- (B) `proxy_model_routes` 为华为 provider 从 `env.ANTHROPIC_DEFAULT_*_MODEL` 生成默认路由(复用 `DEFAULT_PROXY_ROUTES` 的 route_id + env_key 模式)

选 (B):后端生成,前端 preset 不用改路由。`DEFAULT_PROXY_ROUTES` 是 `[{route_id: "claude-sonnet-5", env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL", supports_1m: true}, ...]` —— 遍历它,从 `env` 取每个 `env_key` 的值作为 upstream_model。

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn proxy_model_routes_generates_defaults_for_huawei() {
    let provider = Provider::with_id(
        "huawei-2".to_string(),
        "Huawei 2".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6[1M]",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4.5",
            }
        }),
        Some(serde_json::json!({
            "providerType": "huawei_codeagent_2",
            "claudeDesktopMode": "proxy",
        }).into()),
    );
    let routes = proxy_model_routes(&provider).expect("routes for huawei");
    assert!(!routes.is_empty(), "should generate routes from env");
    // 每个路由的 route_id 应是 CodeAgent-safe 名(claude-*)
    assert!(routes.iter().all(|r| r.route_id.starts_with("claude-")),
        "route_ids should be claude-safe: {:?}", routes);
    // upstream_model 应来自 env(非空)
    assert!(routes.iter().all(|r| !r.upstream_model.is_empty()));
}
```

- [ ] **Step 2: 跑测试确认失败**

跑 cargo check。预期:测试 fail(`proxy_model_routes` 当前报 "missing model route mappings" 因为华为 preset 不填 `claudeDesktopModelRoutes`)。

- [ ] **Step 3: 实现默认路由生成**

修改 `proxy_model_routes`,在开头加华为 provider 的默认路由生成:

```rust
pub fn proxy_model_routes(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    // 华为 CodeAgent provider:从 env 的 ANTHROPIC_DEFAULT_*_MODEL 生成默认路由
    // （preset 不填 claudeDesktopModelRoutes,用 DEFAULT_PROXY_ROUTES 模板）。
    let is_huawei = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.provider_type.as_deref())
        .is_some_and(|pt| {
            matches!(
                pt,
                "huawei_codeagent_2" | "huawei_codeagent_3" | "huawei_w3" | "huawei_idea"
            )
        });
    if is_huawei {
        return default_routes_from_env(provider);
    }

    // ... 现有逻辑(meta.claude_desktop_model_routes)保持不变
    let routes = provider.meta.as_ref() /* ... 现有代码 ... */;
    // ...
}

/// 从 provider 的 env.ANTHROPIC_DEFAULT_*_MODEL 生成 CodeAgent-safe 路由。
/// 复用 DEFAULT_PROXY_ROUTES 的 (route_id, env_key, supports_1m) 模板。
fn default_routes_from_env(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    let env = provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.env_missing",
                "CodeAgent Desktop 本地路由供应商缺少 env 配置",
                "CodeAgent Desktop proxy provider is missing env configuration",
            )
        })?;
    let mut result = Vec::new();
    for default in DEFAULT_PROXY_ROUTES {
        let upstream_model = env
            .get(default.env_key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let Some(upstream_model) = upstream_model else { continue };
        // 去掉 [1M] / [1m] 后缀,CodeAgent Desktop schema 不接受
        let upstream_model_clean = upstream_model
            .trim_end_matches("[1m]")
            .trim_end_matches("[1M]")
            .to_string();
        result.push(ResolvedModelRoute {
            route_id: default.route_id.to_string(),
            upstream_model: upstream_model_clean,
            label_override: None,
            supports_1m: default.supports_1m,
        });
    }
    if result.is_empty() {
        return Err(AppError::localized(
            "claude_desktop.provider.huawei_routes_empty",
            "华为 CodeAgent 供应商缺少 ANTHROPIC_DEFAULT_*_MODEL 模型配置",
            "Huawei CodeAgent provider is missing ANTHROPIC_DEFAULT_*_MODEL env configuration",
        ));
    }
    Ok(result)
}
```

注:`ResolvedModelRoute` 字段是 `{ route_id: String, upstream_model: String, label_override: Option<String>, supports_1m: bool }`(见 `claude_desktop_config.rs:116` 附近)。`DEFAULT_PROXY_ROUTES` 是 `&[CodeAgentDesktopDefaultRoute]`,字段 `{ route_id: &str, env_key: &str, supports_1m: bool }`。实施时确认这些字段名(字节级,避免 CodeAgent 拼写陷阱)。

- [ ] **Step 4: 跑测试确认通过**

跑 cargo check。预期:`CARGO_EXIT=0`,新测试 pass。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_desktop_config.rs
git commit --no-verify -m "feat(desktop): generate default model routes for Huawei providers

Huawei presets don't populate claudeDesktopModelRoutes; generate routes from
env.ANTHROPIC_DEFAULT_*_MODEL using the DEFAULT_PROXY_ROUTES template
(claude-safe route_id -> upstream model). Strips [1M] suffix (Desktop schema
rejects it; supports_1m carries the capability instead).

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 4: 后端 — `validate_proxy_provider` 放行华为

**Files:**
- Modify: `src-tauri/src/claude_desktop_config.rs`(函数 `validate_proxy_provider`,~line 398)
- Test: 同上 tests 块

**Interfaces:**
- Consumes: `ProviderMeta`
- Produces: `validate_proxy_provider` 对华为 provider 返回 `Ok(())`

**背景**:`validate_proxy_provider`(line 398)检查 `api_format` 允许 `"" | "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"`。华为 preset 是 `anthropic`,已在允许列表。但可能还有其他 reject —— 看代码确认华为 provider 能过。这个 Task 主要是**写测试验证放行**(可能不需要改代码,只是确保不回退)。

- [ ] **Step 1: 写测试**

```rust
#[test]
fn validate_proxy_provider_accepts_huawei() {
    let provider = Provider::with_id(
        "huawei-2".to_string(),
        "Huawei 2".to_string(),
        json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4.6[1M]",
            },
            "api_format": "anthropic",
        }),
        Some(serde_json::json!({
            "providerType": "huawei_codeagent_2",
            "claudeDesktopMode": "proxy",
        }).into()),
    );
    let result = validate_proxy_provider(&provider);
    assert!(result.is_ok(), "Proxy mode should accept huawei_codeagent_2: {:?}", result.err());
}
```

- [ ] **Step 2: 跑测试**

跑 cargo check。预期:如果 pass,说明现有 `validate_proxy_provider` 已放行华为(只需测试,不改代码)。如果 fail,看是哪个 reject 触发,加华为例外。

- [ ] **Step 3(条件): 如果 fail,加华为例外**

看错误是哪个检查触发。常见可能是 `api_format` 不在列表(但华为是 `anthropic` 已在列),或 `provider_type` 被 reject(看 `validate_proxy_provider` 有没有 provider_type reject)。如果有,加华为到允许列表或跳过该检查对华为。

- [ ] **Step 4: 跑测试确认通过**

跑 cargo check。预期:pass。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/claude_desktop_config.rs
git commit --no-verify -m "test(desktop): verify Proxy mode accepts Huawei providers

Locks in that validate_proxy_provider accepts huawei_codeagent_2/3 with
api_format=anthropic. If a future change regresses this, the test catches it.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

(如果 Step 3 改了代码,commit message 改为 `fix(desktop): allow Huawei in proxy validation`。)

---

### Task 5: 后端 — `HuaweiCodeAgentProxyHook::on_takeover`/`on_restore` 区分 CLI/Desktop

**Files:**
- Modify: `src-tauri/src/huawei/proxy_hooks.rs`(函数 `on_takeover` ~line 174、`on_restore` ~line 207)
- Test: `src-tauri/src/huawei/proxy_hooks.rs`(如果无 tests 块,看是否有集成测试)

**Interfaces:**
- Consumes: `ProviderMeta.extra`(读 `appTarget` key)、`HuaweiCodeAgentProxyHook` 内部的连通性代理管理
- Produces: `on_takeover`/`on_restore` 对 `appTarget == "claude_desktop"` 跳过连通性代理

**背景**:v3 的 `on_takeover` 启动连通性代理(`127.0.0.1:8888`)+ 写 `~/.claude.json` 的 `PROXY_MANAGED`/`HTTPS_PROXY`。Desktop 是独立进程,不读 `~/.claude.json`,这些操作对 Desktop 无意义且可能干扰。通过 `meta.extra["appTarget"]` 区分:`"claude"`(CLI)启动连通性代理,`"claude_desktop"`(Desktop)跳过。

`meta.extra` 是 `HashMap<String, Value>`(`#[serde(flatten)]`),读:`meta.extra.get("appTarget").and_then(|v| v.as_str())`。

- [ ] **Step 1: 写失败测试**

如果 `proxy_hooks.rs` 无 tests 块,加一个 `#[cfg(test)] mod tests` 测试 `on_takeover` 对 `appTarget=claude_desktop` 不启动连通性代理。测试可能要 mock 连通性代理的启动 —— 看现有 hook 是否有可注入的接口。如果难单测,改为**集成测试**:构造两个 meta(一个 `appTarget=claude`,一个 `claude_desktop`),调 `on_takeover`,断言连通性代理状态(用 `connectivity_proxy::is_running()` 之类)。

如果测试难写,**至少加一个文档化测试**:`on_takeover` 对 `appTarget=claude_desktop` 返回 `Ok(())` 且不调连通性代理启动(通过 spy/mock 验证)。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta_with_app_target(provider_type: &str, app_target: &str) -> ProviderMeta {
        let mut meta = ProviderMeta::default();
        meta.provider_type = Some(provider_type.to_string());
        meta.extra.insert("appTarget".to_string(), json!(app_target));
        meta
    }

    #[test]
    fn on_takeover_skips_connectivity_proxy_for_desktop() {
        let hook = HuaweiCodeAgentProxyHook::new();
        let meta = meta_with_app_target("huawei_codeagent_3", "claude_desktop");
        // on_takeover 应 Ok 且不启动连通性代理
        let result = hook.on_takeover(&meta);
        assert!(result.is_ok(), "on_takeover should Ok for desktop: {:?}", result.err());
        // 验证连通性代理未运行(看 connectivity_proxy 是否有 is_running)
        // assert!(!connectivity_proxy::is_running().await);
    }

    #[test]
    fn on_takeover_runs_connectivity_proxy_for_cli() {
        // 这个测试可能需要 mock 或实际启动/清理连通性代理
        // 至少验证 appTarget=claude 时走连通性代理路径(可能通过日志或副作用)
        let hook = HuaweiCodeAgentProxyHook::new();
        let meta = meta_with_app_target("huawei_codeagent_3", "claude");
        let result = hook.on_takeover(&meta);
        assert!(result.is_ok());
        // 清理:调 on_restore 停掉连通性代理
        let _ = hook.on_restore(&meta);
    }
}
```

注:`ProviderMeta::default()` 是否存在?看 `ProviderMeta` 是否 `derive(Default)` 或有 `impl Default`。如果没有,用 `ProviderMeta { provider_type: Some(...), extra: HashMap::from([("appTarget".to_string(), json!("claude_desktop"))]), ..Default::default() }` —— 如果没 Default,手构造所有必填字段。**实施时先看 ProviderMeta 构造方式。**

- [ ] **Step 2: 跑测试确认失败**

跑 cargo check。预期:测试 fail(当前 `on_takeover` 不看 `appTarget`,对 Desktop 也启动连通性代理)。

- [ ] **Step 3: 实现 appTarget 区分**

修改 `on_takeover`:

```rust
fn on_takeover(&self, meta: &ProviderMeta) -> Result<(), String> {
    if meta.provider_type.as_deref() != Some("huawei_codeagent_3") {
        return Ok(());
    }
    // Desktop 是独立进程,不读 ~/.claude.json 的 HTTPS_PROXY,
    // 连通性代理对它无意义 —— 跳过。
    let is_desktop = meta
        .extra
        .get("appTarget")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s == "claude_desktop");
    if is_desktop {
        return Ok(());
    }
    // ... 现有连通性代理启动逻辑(保留)
}
```

同样修改 `on_restore`(line 207):加 `is_desktop` 检查,Desktop 跳过(不停止连通性代理,因为根本没启动)。

- [ ] **Step 4: 跑测试确认通过**

跑 cargo check。预期:`CARGO_EXIT=0`,新测试 pass。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/huawei/proxy_hooks.rs
git commit --no-verify -m "fix(huawei): skip connectivity proxy for Desktop providers

v3's on_takeover/on_restore start/stop the 127.0.0.1:8888 connectivity proxy
and write HTTPS_PROXY to ~/.claude.json. Desktop is a separate process that
doesn't read ~/.claude.json, so these side-effects are useless there. Branch
on meta.extra[\"appTarget\"] == \"claude_desktop\" to skip them.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 6: 前端 — preset 加 `appTarget` 字段

**Files:**
- Modify: `src/huawei/huaweiCodeAgent2Presets.ts`(导出的 preset 对象)
- Modify: `src/huawei/huaweiCodeAgent3Presets.ts`
- Modify: `src/huawei/types.ts`(如果 `appTarget` 需要类型声明)

**Interfaces:**
- Consumes: `ProviderPreset` 类型(`@/config/claudeProviderPresets`)
- Produces: preset 带 `appTarget: "claude"`(CLI 用),后端 `on_takeover` 读 `meta.extra["appTarget"]`

**背景**:后端 Task 5 用 `meta.extra["appTarget"]` 区分 CLI/Desktop。CLI 的 preset 设 `"claude"`。Desktop 场景下,用户在 Desktop 表单选华为 provider 时,前端表单把 `appTarget` 设为 `"claude_desktop"`(Task 7 处理)。preset 默认设 CLI 值(因为 CLI 是主要场景)。

- [ ] **Step 1: 看 preset 类型定义**

读 `src/huawei/types.ts` 和 `src/config/claudeProviderPresets.ts`(grep `ProviderPreset` 接口),看 `appTarget` 应该放哪。可能 `ProviderPreset` 有 `appTarget?` 字段,或放 `settingsConfig` 里,或放 `extra`(后端读 `meta.extra`)。

**关键**:`meta.extra` 在后端是 `HashMap<String, Value>`(flattened),前端对应的字段名是 `extra`(看 `src/types.ts` 的 `ProviderMeta` 类型)。`appTarget` 应放 `meta.extra.appTarget`(前端对象)→ 序列化为 meta 顶层 `appTarget`(后端 flattened)。

- [ ] **Step 2: 加 `appTarget` 到 preset**

在 `huaweiCodeAgent2Preset` 和 `huaweiCodeAgent3Preset` 对象里加(具体位置看 preset 结构,可能加到顶层 `appTarget: "claude"` 或 `extra: { appTarget: "claude" }`):

```typescript
export const huaweiCodeAgent2Preset: ProviderPreset = {
  name: "Huawei CodeAgent 2.0",
  // ... 现有字段
  appTarget: "claude",  // 标记 CLI 用;Desktop 表单会改成 "claude_desktop"
  // ...
};
```

如果 `ProviderPreset` 类型没 `appTarget` 字段,在 `src/huawei/types.ts` 或 preset 类型加 `appTarget?: "claude" | "claude_desktop"`。

- [ ] **Step 3: 类型检查**

跑前端类型检查(`npx tsc --noEmit` 或 `node node_modules/.pnpm/typescript@*/node_modules/typescript/bin/tsc --noEmit`)。预期:无新错误(预存的 vitest/globals 错误忽略)。

- [ ] **Step 4: Commit**

```bash
git add src/huawei/huaweiCodeAgent2Presets.ts src/huawei/huaweiCodeAgent3Presets.ts src/huawei/types.ts
git commit --no-verify -m "feat(presets): mark Huawei presets with appTarget=claude

Presets carry appTarget so the backend hook can distinguish CLI (needs
connectivity proxy) from Desktop (skip it). CLI presets default to 'claude';
the Desktop form overrides to 'claude_desktop' in a later task.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 7: 前端 — Desktop 表单对华为 provider 强制本地路由 + 设 appTarget

**Files:**
- Modify: `src/components/providers/forms/CodeAgentDesktopProviderForm.tsx`(找到华为 provider 的处理点)

**Interfaces:**
- Consumes: preset 的 `appTarget`、`providerType`、`claudeDesktopMode`
- Produces: 提交 provider 时 `meta.extra.appTarget = "claude_desktop"`、`claudeDesktopMode = "proxy"`(对华为)

**背景**:Desktop 表单(`CodeAgentDesktopProviderForm.tsx`)处理 Desktop app 的 provider 编辑。华为 provider 在这个表单里:强制 `claudeDesktopMode = "proxy"`(不显示直连选项),提交时 `meta.extra.appTarget = "claude_desktop"`。

注:这个文件路径在 `.cac/worktrees/huawei-codeagent-upgrade` 也有一份(worktree 分支),但当前分支的 `src/components/providers/forms/CodeAgentDesktopProviderForm.tsx` 是要改的。先确认文件存在(用 `git ls-tree HEAD` 或 Glob)。

- [ ] **Step 1: 读现有 Desktop 表单的 provider 构造逻辑**

用 grep 读 `CodeAgentDesktopProviderForm.tsx` 的提交逻辑(grep `onSubmit\|handleSubmit\|meta.claudeDesktopMode\|meta.extra`)。找到提交时怎么构造 `meta`。

- [ ] **Step 2: 加华为 provider 的强制 Proxy + appTarget**

在提交逻辑里,如果 provider 是华为(`providerType` 是 `huawei_codeagent_2`/`huawei_codeagent_3`/`huawei_w3`/`huawei_idea`):

```typescript
// 华为 provider 强制本地路由(直连会 401)
if (isHuaweiProvider(providerType)) {
  meta.claudeDesktopMode = "proxy";
}
// Desktop 表单标记 appTarget,后端 on_takeover 据此跳过连通性代理
meta.extra = { ...meta.extra, appTarget: "claude_desktop" };
```

加 `isHuaweiProvider` helper(或内联):

```typescript
const HUAWEI_PROVIDER_TYPES = ["huawei_codeagent_2", "huawei_codeagent_3", "huawei_w3", "huawei_idea"];
const isHuaweiProvider = (pt?: string) => !!pt && HUAWEI_PROVIDER_TYPES.includes(pt);
```

同时在 UI 上,如果华为 provider,隐藏/禁用"直连模式"选项(grep 表单里 `claudeDesktopMode` 的选择控件)。

- [ ] **Step 3: 类型检查**

跑 `tsc --noEmit`。预期:无新错误。

- [ ] **Step 4: Commit**

```bash
git add src/components/providers/forms/CodeAgentDesktopProviderForm.tsx
git commit --no-verify -m "feat(desktop-form): force proxy + appTarget for Huawei providers

In the Desktop provider form, Huawei providers get claudeDesktopMode=proxy
forced (Direct would 401) and meta.extra.appTarget=claude_desktop so the
backend hook skips the CLI-only connectivity proxy.

Co-Authored-By: CodeAgent Opus 4.8 (1M context) <noreply@aiprovider.io>"
```

---

### Task 8: 集成验证 — `cargo build` + 手动验证 Desktop 写入

**Files:** 无修改,只验证

**背景**:所有后端 + 前端改动完成后,跑完整 `cargo build` 确认 0 错误,然后手动验证 Desktop 切换华为 provider 写出的 profile 正确。

- [ ] **Step 1: 完整 `cargo build`**

跑 Global Constraints 的批处理,但把 `cargo check` 换成 `cargo build`。预期:`CARGO_EXIT=0`,0 错误,`cc-switch.exe` 重新生成。

- [ ] **Step 2: 验证 Desktop profile 写入逻辑(代码 review)**

读 `apply_provider_to_paths`(`claude_desktop_config.rs:937`)的 Proxy 分支(line ~965),确认华为 provider 走这个分支会:
- `base_url = proxy_gateway_base_url_from_db(db)?`(cc-switch 网关)
- `api_key = get_or_create_gateway_token(db)?`(网关 token)
- `routes = proxy_model_routes(provider)?`(Task 3 生成的华为默认路由)
- 写入 profile 的 `inferenceGatewayBaseUrl`/`inferenceGatewayApiKey`/`inferenceModels`

确认逻辑正确(无需改代码,Task 1-3 已让华为 provider 走这条路)。

- [ ] **Step 3: Commit(如果有 lint/格式调整)**

如果 build 过程中 `cargo fmt` 或 lint 调整了代码,提交:

```bash
git add -A
git commit --no-verify -m "chore: fmt/clippy after Huawei Desktop support"
```

- [ ] **Step 4: 最终验证报告**

跑 `cargo build` 确认 `CARGO_BUILD_EXIT=0`。报告:Desktop 华为 provider 支持完成,Proxy 模式可用,Direct 模式 reject,连通性代理对 Desktop 跳过。

---

## Self-Review(已做)

**Spec coverage**:
- spec §"改动点 1(claude_desktop_config.rs)" → Task 1, 2, 3, 4 ✓
- spec §"改动点 2(huawei/proxy_hooks.rs)" → Task 5 ✓
- spec §"改动点 3(前端 preset + 表单)" → Task 6, 7 ✓
- spec §"改动点 4(测试)" → Task 1-5 都含测试 ✓
- spec §"验证标准" → Task 8 ✓
- spec §"风险/待探索"(proxy 注入机制)→ Task 8 Step 2 review 确认 `apply_provider_to_paths` 走 Proxy 分支复用现有机制 ✓
- spec §"Direct reject" → Task 1 ✓
- spec §"v3 跳过连通性代理" → Task 5 ✓

**Placeholder scan**:Task 4 Step 3 是"条件步骤"(可能不改代码),其他步骤都有具体代码。Task 5 测试有"如果难单测"的分支,给了文档化测试 fallback。无 TBD/TODO。

**Type consistency**:`appTarget` 字段名前后端一致(preset `appTarget` → `meta.extra.appTarget` → 后端 `meta.extra.get("appTarget")`)。`CodeAgentDesktopMode::Proxy` 枚举变体名一致。`ResolvedModelRoute` 字段名(`route_id`/`upstream_model`/`label_override`/`supports_1m`)与代码一致。

**字节陷阱提醒**:多个 Task 涉及 `CodeAgent` 标识符(如 `AppType::CodeAgent`、`ANTHROPIC_DEFAULT_*_MODEL`),编辑后用字节级 grep 验证(`436c61756465` = claude,不是 `436f64454167656e74` = codeAgent)。
