# CodeAgent Desktop 支持 CodeAgent 2.0/3.0 供应商 — 设计

**日期**:2026-10-11
**分支**:`person_l00899467_develop`(main 合并后)
**目标**:让 CodeAgent Desktop 在本地路由(Proxy)模式下支持华为 CodeAgent 2.0/3.0 供应商;Direct 模式拒绝(同 `github_copilot` 等)。

## 背景

当前华为 CodeAgent 2.0/3.0 供应商只在 CodeAgent CLI 上支持。CLI 路径:
- 写 `~//codeagent/settings.json`(`claude_direct::reapply` 按字段补丁)
- CLI 请求经 cc-switch 本地代理 → `HuaweiCodeAgentProxyHook` 注入 auth 头(`x-auth-token`/`Authorization`/`app-id`/`X-snap-traceid`/...)+ 指纹脱敏 + `openai_chat` 转换 → snapengine 上游
- v3 额外启动连通性代理(`127.0.0.1:8888`)+ 写 `~/.claude.json` 的 `HTTPS_PROXY`(CLI 读这个走代理)

Desktop 当前完全不认华为 provider —— `claude_desktop_config.rs` 没有 `huawei_codeagent_*` 任何处理。

## 架构事实(代码探索得出)

1. **Desktop 两种模式都走 `inferenceProvider: "gateway"` 形式**,profile schema 只有 `inferenceGatewayBaseUrl` + `inferenceGatewayApiKey`(Bearer)+ `inferenceGatewayAuthScheme: "bearer"` + `inferenceModels`。**表达不出华为需要的多头认证**(`x-auth-token`/`app-id`/`X-snap-traceid` 等)。

2. **华为 auth/指纹/转换只在 `HuaweiCodeAgentProxyHook` 里**(proxy 路径触发)。Desktop Direct 不经 proxy → hook 不触发 → auth 头无法注入 → 华为上游 401。

3. **Direct 模式对华不可行**:Desktop profile 的单 Bearer token 满足不了华为上游契约,且无法重写 hook 在 Desktop 进程里(Desktop 不是 proxy,拿不到 `HuaweiCodeAgentAuthManager` 的 token 注入到请求头里 —— 它只能写 profile 文件,然后 Desktop 进程自己发请求)。

4. **Proxy 模式可行**:Desktop profile 的 `inferenceGatewayBaseUrl` 指 cc-switch 网关,`inferenceGatewayApiKey` 是 cc-switch 网关 token;Desktop 请求 → cc-switch 网关 → `HuaweiCodeAgentProxyHook` 触发 → 华为上游。这和 `github_copilot` 在 Desktop 的 Proxy 模式同机制。

5. **v3 连通性代理对 Desktop 无意义**:Desktop 是独立进程,不读 `~/.claude.json` 的 `HTTPS_PROXY`。v3 的连通性代理只对 CLI 有用(CLI 读 `~/.claude.json`)。

## 设计

### 范围

- **支持**:Desktop 的本地路由(Proxy)模式支持华为 CodeAgent 2.0/3.0 供应商
- **不支持**:Desktop 的直连(Direct)模式对华为 reject(同 `github_copilot`/`codex_oauth`/`xai_oauth`)

### 改动点

#### 1. `src-tauri/src/claude_desktop_config.rs`

**`validate_direct_provider`**(line ~374):在 reject 列表加华为 provider_type
```rust
if matches!(
    meta.provider_type.as_deref(),
    Some("github_copilot") | Some("codex_oauth") | Some("xai_oauth")
        | Some("huawei_codeagent_2") | Some("huawei_codeagent_3") | Some("huawei_w3") | Some("huawei_idea")
) {
    return Err(AppError::localized(
        "claude_desktop.provider.type_unsupported",
        "华为 CodeAgent 在 CodeAgent Desktop 直连模式不支持（上游需要多头认证），请使用本地路由模式",
        "Huawei CodeAgent does not support CodeAgent Desktop direct mode (upstream requires multi-header auth); use local routing mode",
    ));
}
```

**`validate_proxy_provider`**(line ~398):`api_format` 允许 `anthropic`(华为 preset 是 `anthropic`)。当前已允许 `"" | "anthropic" | "openai_chat" | "openai_responses" | "gemini_native"` —— 华为的 `anthropic` 已在列,无需改。但确认华为 provider_type 不在 reject 列表里(目前没 reject)。

**`provider_mode`**(line 229):华为 provider 强制 `Proxy`:
```rust
pub fn provider_mode(provider: &Provider) -> CodeAgentDesktopMode {
    if let Some(meta) = provider.meta.as_ref() {
        if matches!(meta.provider_type.as_deref(), Some("huawei_codeagent_2") | Some("huawei_codeagent_3") | Some("huawei_w3") | Some("huawei_idea")) {
            return CodeAgentDesktopMode::Proxy;
        }
        // ... 现有逻辑
    }
    CodeAgentDesktopMode::Direct
}
```

**`proxy_model_routes`**(line 561):为华为 provider 生成模型路由。从 `provider.settings_config.env.ANTHROPIC_MODEL` / `ANTHROPIC_DEFAULT_SONNET_MODEL` 等取模型名,生成 `ResolvedModelRoute`(route_id = 模型名,label_override 可空)。复用现有 `direct_inference_model_specs` 的模型提取逻辑(都是从 env 取模型)。

#### 2. `src-tauri/src/huawei/proxy_hooks.rs`

**`on_takeover` / `on_restore`**(line 174, 207):连通性代理逻辑加 app 区分。当前:
```rust
fn on_takeover(&self, meta: &ProviderMeta) -> Result<(), String> {
    if meta.provider_type.as_deref() != Some("huawei_codeagent_3") {
        return Ok(());
    }
    // 启动连通性代理 + 写 ~/.claude.json PROXY_MANAGED
}
```

问题:`provider_type` 都是 `huawei_codeagent_3`,无法只凭它区分 CLI/Desktop。**需在 meta 加标记**(如 `meta.extra.app_target = "claude" | "claude_desktop"`)或从 takeover 调用上下文拿 `AppType`。看 `ProxyHook::on_takeover` 签名 —— 当前只传 `meta`,要改签名传 `app_type: &AppType` 或在 meta 里加字段。

**推荐**:在 `ProviderMeta.extra` 加 `app_target` 字段(preset 里设),`on_takeover` 检查 `app_target == "claude"` 才启动连通性代理。这样不改 `ProxyHook` trait 签名(影响小)。

**`matches_provider`**:已匹配华为 provider_type,**需验证 Desktop 请求到 proxy 时,proxy 能拿到 provider 的 meta**。看现有 `github_copilot` 在 Desktop Proxy 模式怎么 work —— 它也靠 proxy 注入 OAuth,机制应该一致。

#### 3. 前端

**`src/huawei/huaweiCodeAgent2Presets.ts` / `3Presets.ts`**:preset 加 `app_target: "claude"` 字段(标明 CLI 用)。或加 `supportsApps: ["claude"]` —— 看 preset schema 有没有这字段。

**`src/components/providers/forms/ProviderForm.tsx`**:华为 provider 选 Desktop app 时:
- 强制选本地路由模式(隐藏/禁用直连)
- 表单字段适配 Desktop 的 `inferenceModels` 路由

#### 4. 测试

`claude_desktop_config.rs` 的 test 模块加:
- `validate_direct_provider` reject 华为 provider
- `validate_proxy_provider` 放行华为 provider
- `proxy_model_routes` 为华为 provider 生成正确路由
- `provider_mode` 强制华为为 Proxy

### 验证标准

1. Desktop app 里能创建/编辑华为 CodeAgent 2.0/3.0 provider,选本地路由
2. 切换到华为 provider 后,Desktop profile 写入正确(`inferenceGatewayBaseUrl` 指 cc-switch 网关,`inferenceModels` 是华为模型路由)
3. Desktop 发请求 → cc-switch 代理 → `HuaweiCodeAgentProxyHook` 注入 auth 头 + 指纹脱敏 + 转换 → 华为上游 200
4. v3 在 Desktop 场景**不**启动连通性代理
5. 选 Direct 模式选华为时报错提示用本地路由

### 风险/待探索(实施时确认)

- **proxy 对 Desktop 请求注入华为 auth 的机制**:需看现有 `github_copilot` 在 Desktop Proxy 模式怎么把 provider meta 传到 proxy hook。如果机制存在,华为可复用;如果不存在,要先建这个机制(影响范围大,需单独评估)。
- **`on_takeover` 区分 CLI/Desktop**:`provider_type` 相同,需 meta 加 `app_target` 字段。preset 和 ProviderForm 都要加。
- **`HuaweiCodeAgentProxyHook::force_api_format` 强制 `openai_chat`**:华为 preset 是 `anthropic`,但 hook 强制 `openai_chat`。Desktop profile 没有显式 api_format 字段(走 `inferenceGatewayAuthScheme: bearer`)。需确认 Desktop 请求格式 + proxy 转换链路能正确把 anthropic-format 请求转成华为上游要的 openai_chat —— 这是 CLI 路径已验证的,Desktop 路径要复测。

### 不做(YAGNI)

- 不为 Direct 模式重写 hook 逻辑(上游契约不允许,且 Desktop profile schema 表达不出)
- 不在 Desktop 进程内嵌 auth 注入(Desktop 不是 proxy,不该承担这个)
- 不动 CLI 的现有华为集成路径(只加 Desktop)
