//! Constants the frontend keeps its own copy of. `tests/config/backendConstants.json`
//! is generated here; `tests/config/backendConstants.test.ts` checks the frontend
//! copies against it, so changing either side alone fails a test. After an
//! intended change, regenerate with
//! `CC_SWITCH_UPDATE_GOLDEN=1 cargo test backend_constants_match_shared_fixture`
//! and update the frontend copy.
use crate::app_config::AppType;
use serde_json::{json, Map, Value};

/// Base URLs whose coding-plan detection both sides must agree on.
const CODING_PLAN_URLS: &[&str] = &[
    "https://api.kimi.com/coding/",
    "HTTPS://API.KIMI.COM/CODING/v1",
    "https://open.bigmodel.cn/api/anthropic",
    "https://api.z.ai/api/anthropic",
    "https://api.minimaxi.com/anthropic",
    "https://api.minimax.io/anthropic",
    "https://api.minimax.cn/v1",
    "https://api.minimax.cn.example.com/v1",
    "https://zenmux.ai/api/anthropic",
    "https://ark.cn-beijing.volces.com/api/plan/v3",
    "https://ark.cn-beijing.volces.com/api/coding",
    "https://ark.cn-beijing.volces.com/api/v3",
    "https://opencode.ai/zen/go/v1",
    "https://opencode.ai/inference/go/anthropic",
    "https://opencode.ai/zen/v1",
    "https://api.commandcode.ai/provider",
    "https://api.commandcode.ai/provider/v1",
    // /providers continues past the /provider path boundary, so detection
    // must return null (same rule as the frontend regex).
    "https://api.commandcode.ai/providers",
    "https://ollama.com",
    "https://ollama.com/v1",
    // Local Ollama has no plan quota; a look-alike host or ollama.com in a
    // path must not be mistaken for Ollama Cloud.
    "http://localhost:11434",
    "https://ollama.com.example.com/v1",
    "https://proxy.example.com/ollama.com/v1",
    "https://api.openai.com/v1",
];

/// Header values whose validity both sides must agree on: trimmed-empty means
/// unset, and only control characters other than tab are rejected.
const USER_AGENT_SAMPLES: &[&str] = &[
    "",
    "   ",
    "Mozilla/5.0 (X11; Linux x86_64)",
    " padded/1.0 ",
    "tab\tinside",
    "café/1.0",
    "客户端/2.0",
    "line\nbreak",
    "carriage\rreturn",
    "nul\u{0}byte",
    "escape\u{1b}[0m",
    "delete\u{7f}",
];

/// Cards whose "must route through the local proxy" answer both sides must
/// agree on, checked in every tray app.
fn routing_cards() -> Vec<(&'static str, Value)> {
    let codex_config = |wire_api: &str| {
        json!({
            "auth": {"OPENAI_API_KEY": "sk"},
            "config": format!(
                "model_provider = \"x\"\n[model_providers.x]\nbase_url = \"https://x.example/v1\"\nwire_api = \"{wire_api}\"\n"
            )
        })
    };
    let card = |meta: Value, settings: Value| json!({"id": "p1", "name": "P1", "settingsConfig": settings, "meta": meta});
    let mut official_copilot = card(json!({"providerType": "github_copilot"}), json!({}));
    official_copilot["category"] = json!("official");
    vec![
        ("plain", card(json!({}), json!({}))),
        (
            "copilot",
            card(json!({"providerType": "github_copilot"}), json!({})),
        ),
        (
            "openai_chat",
            card(json!({"apiFormat": "openai_chat"}), json!({})),
        ),
        (
            "anthropic",
            card(json!({"apiFormat": "anthropic"}), json!({})),
        ),
        ("full_url", card(json!({"isFullUrl": true}), json!({}))),
        ("codex_chat_wire", card(json!({}), codex_config("chat"))),
        (
            "codex_responses_wire",
            card(json!({}), codex_config("responses")),
        ),
        (
            "desktop_proxy_mode",
            card(json!({"claudeDesktopMode": "proxy"}), json!({})),
        ),
        ("official_copilot", official_copilot),
    ]
}

#[test]
fn backend_constants_match_shared_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/config/backendConstants.json");
    let coding_plan: Map<String, Value> = CODING_PLAN_URLS
        .iter()
        .map(|url| {
            let id = crate::services::coding_plan::detect_provider_id(url);
            (url.to_string(), json!(id))
        })
        .collect();
    let profile_scope: Map<String, Value> = AppType::all()
        .map(|app| {
            let scope = crate::services::profile::ProfileScope::for_app(&app);
            (app.as_str().to_string(), json!(scope.map(|s| s.as_str())))
        })
        .collect();
    let user_agent: Map<String, Value> = USER_AGENT_SAMPLES
        .iter()
        .map(|ua| {
            let valid = crate::provider::parse_custom_user_agent(Some(ua)).is_ok();
            (ua.to_string(), json!(valid))
        })
        .collect();
    let routing: Vec<Value> = routing_cards()
        .into_iter()
        .map(|(name, card)| {
            let provider: crate::provider::Provider = serde_json::from_value(card.clone()).unwrap();
            let by_app: Map<String, Value> = crate::tray::TRAY_APPS
                .iter()
                .map(|app| {
                    let needs = crate::tray::provider_needs_routing(app, &provider);
                    (app.as_str().to_string(), json!(needs))
                })
                .collect();
            json!({"name": name, "provider": card, "needsRouting": by_app})
        })
        .collect();
    let constants = json!({
        "copilotReasoningEfforts": crate::copilot_config::REASONING_EFFORTS,
        "codexReservedModelProviderIds": crate::codex_config::CODEX_RESERVED_MODEL_PROVIDER_IDS,
        "codexWireApis": {
            "chat": crate::codex_config::CHAT_WIRE_APIS,
            "anthropic": crate::codex_config::ANTHROPIC_WIRE_APIS,
        },
        "hermesProviderSource": {
            "field": crate::hermes_config::PROVIDER_SOURCE_FIELD,
            "customList": crate::hermes_config::PROVIDER_SOURCE_CUSTOM_LIST,
            "dict": crate::hermes_config::PROVIDER_SOURCE_DICT,
        },
        "codingPlanProviderByUrl": coding_plan,
        "quotaWarnBelowPercent": crate::tray::WARN_BELOW_PERCENT,
        "exchangeRateCurrencies": crate::services::exchange_rates::TARGET_CURRENCIES,
        "customIcon": {
            "prefix": crate::services::provider::icon::CUSTOM_ICON_PREFIX,
            "maxLength": crate::services::provider::icon::MAX_CUSTOM_ICON_LENGTH,
            "maxDimension": crate::services::provider::icon::MAX_CUSTOM_ICON_DIMENSION,
        },
        "profileScopeByApp": profile_scope,
        "userAgentValid": user_agent,
        "providerNeedsRouting": routing,
    });
    let expected = serde_json::to_string_pretty(&constants).unwrap() + "\n";
    if std::env::var_os("CC_SWITCH_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual, expected,
        "tests/config/backendConstants.json is out of date; after an intended change, regenerate it with CC_SWITCH_UPDATE_GOLDEN=1 and update the frontend copy"
    );
}
