//! Constants the frontend keeps its own copy of. `tests/config/backendConstants.json`
//! is generated here; `tests/config/backendConstants.test.ts` checks the frontend
//! copies against it, so changing either side alone fails a test. After an
//! intended change, regenerate with
//! `CC_SWITCH_UPDATE_GOLDEN=1 cargo test backend_constants_match_shared_fixture`
//! and update the frontend copy.
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
    "https://api.openai.com/v1",
];

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
    let constants = json!({
        "copilotReasoningEfforts": crate::copilot_config::REASONING_EFFORTS,
        "codexReservedModelProviderIds": crate::codex_config::CODEX_RESERVED_MODEL_PROVIDER_IDS,
        "hermesProviderSource": {
            "field": crate::hermes_config::PROVIDER_SOURCE_FIELD,
            "customList": crate::hermes_config::PROVIDER_SOURCE_CUSTOM_LIST,
            "dict": crate::hermes_config::PROVIDER_SOURCE_DICT,
        },
        "codingPlanProviderByUrl": coding_plan,
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
