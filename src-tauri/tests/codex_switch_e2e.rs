//! Codex LLM 供应商切换端到端用例（按 2026-09-28 真实 ~/.codex 形态复刻）。
//!
//! 覆盖用户实际踩到的三个坑：
//!  1. 切到第三方网关后顶层 `model_provider` 必须指向该端点的 TOML id；
//!  2. 切走时，非激活端点的 `[model_providers.*]` 表必须以 inert 形式保留，
//!     否则老 session 恢复时报 "Model provider `custom` not found"；
//!  3. 切回 OpenAI official 时，auth.json 的 ChatGPT 登录态必须原封不动，
//!     且顶层不得残留 `openai_base_url`（会让 ChatGPT token 打到第三方端点 401）。
//!
//! 另含一个 `#[ignore]` 的缺陷复现用例，见该用例的文档注释。

use serde_json::json;

use cc_switch_lib::{
    read_json_file, write_codex_live_atomic, AppType, MultiAppConfig, Provider, ProviderService,
};

#[path = "support.rs"]
mod support;
use support::{
    create_test_state_with_config, enable_codex_official_auth_preservation, ensure_test_home,
    reset_test_fs, test_mutex,
};

/// 与真实 `auth.json` 同形：ChatGPT 登录态，无 API key。
fn chatgpt_auth() -> serde_json::Value {
    json!({
        "auth_mode": "chatgpt",
        "OPENAI_API_KEY": null,
        "tokens": {
            "id_token": "ID-TOKEN",
            "access_token": "ACCESS-TOKEN",
            "refresh_token": "REFRESH-TOKEN",
            "account_id": "acct-real-1"
        },
        "last_refresh": "2026-09-28T04:19:44.604403Z"
    })
}

/// 官方供应商存档：不带 `model_provider`、不带任何 `[model_providers.*]` 表。
/// 这是用户「第二次补登」之前 DB 里的形态——也正是触发丢表 bug 的输入。
const OFFICIAL_CONFIG: &str = r#"model = "gpt-6-luna"
model_reasoning_effort = "high"
sandbox_mode = "danger-full-access"
"#;

/// 开轩网关存档：顶层指向 legacy `custom` 表，表内 name 为 `kxpms_gateway`。
const KXPMS_CONFIG: &str = r#"model_provider = "custom"
model = "gpt-6-sol"
model_reasoning_effort = "xhigh"
model_catalog_json = "cc-switch-model-catalog.json"

[model_providers.custom]
name = "kxpms_gateway"
base_url = "https://llm.kxpms.cn/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "sk-test-kxpms"
"#;

/// 本机网关存档：同样是 legacy `custom` id，name 为 `local_gateway`。
const LOCAL_CONFIG: &str = r#"model_provider = "custom"
model = "kx-glm-5.3-flash"
model_reasoning_effort = "high"
disable_response_storage = true

[model_providers.custom]
name = "local_gateway"
base_url = "http://localhost:8782/v1"
wire_api = "responses"
requires_openai_auth = false
"#;

/// 用户当前 live 形态：官方激活，但手工补回了 `[model_providers.custom]`。
const SEED_LIVE_CONFIG: &str = r#"model = "gpt-6-luna"
model_reasoning_effort = "high"

[model_providers.custom]
name = "kxpms_gateway"
base_url = "https://llm.kxpms.cn/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "sk-live-kxpms"
"#;

fn read_live_toml() -> toml::Value {
    let text = std::fs::read_to_string(cc_switch_lib::get_codex_config_path())
        .expect("live codex config.toml 应存在");
    toml::from_str(&text).unwrap_or_else(|e| panic!("live config.toml 不是合法 TOML: {e}\n{text}"))
}

fn read_live_text() -> String {
    std::fs::read_to_string(cc_switch_lib::get_codex_config_path()).expect("read live config.toml")
}

fn provider_table<'a>(live: &'a toml::Value, id: &str) -> Option<&'a toml::Value> {
    live.get("model_providers").and_then(|mp| mp.get(id))
}

fn top_level_str(live: &toml::Value, key: &str) -> Option<String> {
    live.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn seed_three_provider_config() -> MultiAppConfig {
    let mut initial = MultiAppConfig::default();
    let manager = initial
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager");
    manager.current = "codex-official".to_string();

    let mut official = Provider::with_id(
        "codex-official".to_string(),
        "OpenAI Official".to_string(),
        json!({ "auth": chatgpt_auth(), "config": OFFICIAL_CONFIG }),
        None,
    );
    official.category = Some("official".to_string());
    manager
        .providers
        .insert("codex-official".to_string(), official);

    let mut kxpms = Provider::with_id(
        "kxpms-gateway".to_string(),
        "开轩 LLM 网关".to_string(),
        json!({ "auth": chatgpt_auth(), "config": KXPMS_CONFIG }),
        None,
    );
    kxpms.category = Some("custom".to_string());
    manager.providers.insert("kxpms-gateway".to_string(), kxpms);

    let mut local = Provider::with_id(
        "local-gateway-8782".to_string(),
        "本地 LLM 网关 (8782)".to_string(),
        json!({
            "auth": { "OPENAI_API_KEY": "sk-test-local" },
            "config": LOCAL_CONFIG
        }),
        None,
    );
    local.category = Some("custom".to_string());
    manager
        .providers
        .insert("local-gateway-8782".to_string(), local);

    initial
}

fn assert_chatgpt_login_intact(stage: &str) {
    let auth: serde_json::Value =
        read_json_file(&cc_switch_lib::get_codex_auth_path()).expect("read auth.json");
    assert_eq!(
        auth.get("auth_mode").and_then(|v| v.as_str()),
        Some("chatgpt"),
        "[{stage}] ChatGPT 登录态必须保留"
    );
    assert_eq!(
        auth.get("tokens")
            .and_then(|t| t.get("refresh_token"))
            .and_then(|v| v.as_str()),
        Some("REFRESH-TOKEN"),
        "[{stage}] refresh_token 必须原样保留"
    );
    assert_eq!(
        auth.get("tokens")
            .and_then(|t| t.get("id_token"))
            .and_then(|v| v.as_str()),
        Some("ID-TOKEN"),
        "[{stage}] id_token 必须原样保留"
    );
}

#[test]
fn codex_switch_roundtrip_across_official_and_two_gateways() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    // 播种真实形态的 live：ChatGPT 登录 + 手工补回 custom 表。
    write_codex_live_atomic(&chatgpt_auth(), Some(SEED_LIVE_CONFIG))
        .expect("seed codex live state");

    let initial = seed_three_provider_config();
    let state = create_test_state_with_config(&initial).expect("create test state");

    // ---- 第 1 跳：official → 开轩网关 ----
    ProviderService::switch(&state, AppType::Codex, "kxpms-gateway")
        .expect("switch to kxpms gateway");

    let live = read_live_toml();
    assert_eq!(
        top_level_str(&live, "model_provider").as_deref(),
        Some("kxpms"),
        "legacy custom 表应按 name 迁移成稳定 id `kxpms` 并成为激活端点"
    );
    assert_eq!(
        provider_table(&live, "kxpms")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("https://llm.kxpms.cn/v1"),
        "激活端点 base_url 应指向开轩网关"
    );
    assert_eq!(
        provider_table(&live, "local8782")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("http://localhost:8782/v1"),
        "非激活端点的表必须以 inert 形式保留，老 session 才能恢复"
    );
    assert_chatgpt_login_intact("切到 kxpms 后");

    // ---- 第 2 跳：开轩网关 → 本机网关 ----
    ProviderService::switch(&state, AppType::Codex, "local-gateway-8782")
        .expect("switch to local gateway");

    let live = read_live_toml();
    assert_eq!(
        top_level_str(&live, "model_provider").as_deref(),
        Some("local8782"),
        "顶层 model_provider 应切到本机端点 id"
    );
    assert_eq!(
        provider_table(&live, "local8782")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("http://localhost:8782/v1"),
        "本机端点应成为激活表"
    );
    assert_eq!(
        provider_table(&live, "kxpms")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("https://llm.kxpms.cn/v1"),
        "开轩端点应降级为 inert 表但内容不变"
    );
    assert!(
        !read_live_text().contains("openai_base_url"),
        "切到第三方端点后顶层不得出现 openai_base_url"
    );

    // ---- 第 3 跳：本机网关 → OpenAI official ----
    ProviderService::switch(&state, AppType::Codex, "codex-official")
        .expect("switch back to official");

    let live = read_live_toml();
    assert_eq!(
        provider_table(&live, "kxpms")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("https://llm.kxpms.cn/v1"),
        "切回官方后 kxpms 表必须以 inert 形式保留 —— 这正是 82a731d9 修的回归"
    );
    assert_eq!(
        provider_table(&live, "local8782")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        Some("http://localhost:8782/v1"),
        "切回官方后本机端点表同样必须保留"
    );
    assert!(
        !read_live_text().contains("openai_base_url"),
        "红线：ChatGPT 登录态下顶层残留 openai_base_url 会把 token 劫持到第三方端点并 401"
    );
    assert_chatgpt_login_intact("切回官方后");

    let current_id = state
        .db
        .get_current_provider(AppType::Codex.as_str())
        .expect("read current provider");
    assert_eq!(
        current_id.as_deref(),
        Some("codex-official"),
        "current provider 应指回官方"
    );
}

/// **回归门**：非代理接管模式下切到 `codex-official`，live config 顶层
/// `model_provider` 不得被 inert 网关表顶掉。
///
/// 这条守的是 2026-09-28 发现的缺陷（HEAD `85c4b641` 上失败）。成因链：
///   1. 切走时会把 live 回填进离场 provider，于是 `codex-official` 的存档
///      `config` 里带上了手工补回的 `[model_providers.custom]`（name=kxpms_gateway），
///      且顶层没有 `model_provider`；
///   2. `migrate_legacy_codex_toml_ids_in_db` 在每次 codex 切换时对 DB 行做 id 迁移，
///      把 `custom` 改名为 `kxpms`；
///   3. 迁移当时会在「顶层 model_provider 缺失」时**无条件**补写
///      `model_provider = "kxpms"`，于是 inert 表被提升成激活端点并**永久写回
///      DB 行**，之后每次切官方新会话都打到开轩网关。
///
/// 违反 `docs/rfcs/0002-codex-inert-provider-tables.md` §2.1：inert 表「仅作
/// session 恢复用途，**不影响新流量路由**」。
#[test]
fn codex_switch_to_official_must_not_activate_inert_gateway() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    write_codex_live_atomic(&chatgpt_auth(), Some(SEED_LIVE_CONFIG))
        .expect("seed codex live state");

    let initial = seed_three_provider_config();
    let state = create_test_state_with_config(&initial).expect("create test state");

    // 先离开官方一次，触发 live → provider 的回填（真实使用路径）。
    ProviderService::switch(&state, AppType::Codex, "kxpms-gateway")
        .expect("switch away from official");
    // 再切回官方。
    ProviderService::switch(&state, AppType::Codex, "codex-official")
        .expect("switch back to official");

    let live = read_live_toml();
    assert_ne!(
        top_level_str(&live, "model_provider").as_deref(),
        Some("kxpms"),
        "选 OpenAI Official 时顶层 model_provider 不得被 inert 网关表顶掉；\n实际 live:\n{}",
        read_live_text()
    );
    assert_eq!(
        top_level_str(&live, "model_provider").as_deref(),
        None,
        "选官方时顶层不应残留任何第三方 provider id（Codex 默认走 openai）；\n实际 live:\n{}",
        read_live_text()
    );
}

/// **回归门（同上，但官方卡没有 category）**：早期 OAuth 版本可能在 category
/// 落库前就把固定卡绑上（`live.rs` 里 `apply_codex_official_auth` 的同款说明：
/// 「Early OAuth builds could bind the fixed card before its category was
/// persisted」）。那批 DB 行的 `category` 是 NULL。
///
/// 只判 `category == "official"` 的实现会把它们误当网关档，补写顶层
/// `model_provider = "kxpms"` —— 正是本文件另一条回归门要拦的缺陷，只是发生在
/// 另一类用户身上。这条用例锁定「判官方必须走 `is_codex_official_provider`」。
#[test]
fn codex_switch_to_category_less_official_card_still_ignores_inert_gateway() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    write_codex_live_atomic(&chatgpt_auth(), Some(SEED_LIVE_CONFIG))
        .expect("seed codex live state");

    let mut initial = seed_three_provider_config();
    // 关键差异：官方卡沿用固定 id（codex-official），但 category 为 None。
    let official = initial
        .get_manager_mut(&AppType::Codex)
        .expect("codex manager")
        .providers
        .get_mut("codex-official")
        .expect("official provider");
    official.category = None;

    let state = create_test_state_with_config(&initial).expect("create test state");

    ProviderService::switch(&state, AppType::Codex, "kxpms-gateway")
        .expect("switch away from official");
    ProviderService::switch(&state, AppType::Codex, "codex-official")
        .expect("switch back to official");

    assert_eq!(
        top_level_str(&read_live_toml(), "model_provider").as_deref(),
        None,
        "category 缺失的官方卡同样不得被 inert 网关表顶掉；\n实际 live:\n{}",
        read_live_text()
    );
}

/// **回归门（RFC 0002 §2.1 验收标准）**：切到官方后，live 里必须仍能按 legacy id
/// `custom` 解析到表。
///
/// 老 session 的 `session_meta.payload.model_provider` 记的是 `custom`，而 session
/// 元数据不可改（RFC 0002 §2.2），Codex 0.158+ 加载时又严格校验「每个 session 的
/// model_provider 必须有同名表」。所以「`Model provider 'custom' not found`」有两个
/// 来源：切到官方时整张表被写掉（82a731d9 修的），以及 legacy id 迁移把 `custom`
/// 改名成 `kxpms` 之后没有留兼容副本 —— 后者会把同一个报错换个原因再犯一遍，
/// 直接推翻 RFC 的验收标准。
#[test]
fn codex_switch_to_official_keeps_legacy_custom_id_resolvable() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    enable_codex_official_auth_preservation();
    let _home = ensure_test_home();

    write_codex_live_atomic(&chatgpt_auth(), Some(SEED_LIVE_CONFIG))
        .expect("seed codex live state");
    let initial = seed_three_provider_config();
    let state = create_test_state_with_config(&initial).expect("create test state");

    ProviderService::switch(&state, AppType::Codex, "kxpms-gateway")
        .expect("switch away from official");
    ProviderService::switch(&state, AppType::Codex, "codex-official")
        .expect("switch back to official");

    let live = read_live_toml();
    let legacy = provider_table(&live, "custom").unwrap_or_else(|| {
        panic!(
            "老 session 的 model_provider=\"custom\" 必须在 live 里有同名表，否则无法 resume；实际：\n{}",
            read_live_text()
        )
    });
    // 别名必须与按端点区分的新表指向同一处，否则老 session 会被静默改道。
    assert_eq!(
        legacy.get("base_url").and_then(|v| v.as_str()),
        provider_table(&live, "kxpms")
            .and_then(|t| t.get("base_url"))
            .and_then(|v| v.as_str()),
        "custom 别名必须与 [model_providers.kxpms] 指向同一端点，不能把老 session 改道"
    );
    assert_eq!(
        top_level_str(&live, "model_provider").as_deref(),
        None,
        "别名必须保持 inert：顶层不得指向 custom"
    );
}
