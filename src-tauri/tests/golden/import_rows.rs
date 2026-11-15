//! ① 深链导入、首启导入写出的 DB 原始行。
//!
//! 重构不动数据库（不升 schema、不回填、不剥离存量行），降级后旧版直接读这些行，
//! 所以新增供应商时写进 DB 的字节要保持原样。

use cc_switch_lib::{
    import_default_config_test_hook, import_provider_from_deeplink, parse_deeplink_url, AppType,
};

use crate::support::{create_test_state, reset_test_fs, test_mutex};
use crate::util::{assert_golden, dump_provider_rows, write_home_file};

/// 深链生成的 id 是「名称-毫秒时间戳」，快照里换成固定写法。
fn stable_deeplink_id(id: &str) -> String {
    match id.rsplit_once('-') {
        Some((name, ts)) if ts.chars().all(|c| c.is_ascii_digit()) => format!("{name}-<ts>"),
        _ => id.to_string(),
    }
}

fn import_deeplinks(app: AppType, urls: &[&str], snapshot: &str) {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let state = create_test_state().expect("create test state");
    for url in urls {
        let request = parse_deeplink_url(url).expect("parse deeplink url");
        import_provider_from_deeplink(&state, request).expect("import provider from deeplink");
    }
    assert_golden(snapshot, &dump_provider_rows(&app, stable_deeplink_id, &[]));
}

#[test]
fn deeplink_claude_rows() {
    import_deeplinks(
        AppType::Claude,
        &[
            "ccswitch://v1/import?resource=provider&app=claude&name=Relay%20A&homepage=https%3A%2F%2Frelay-a.example&endpoint=https%3A%2F%2Fapi.relay-a.example%2Fanthropic,https%3A%2F%2Fbackup.relay-a.example&apiKey=sk-relay-a&model=claude-sonnet-4-5&haikuModel=claude-haiku-4-5&sonnetModel=claude-sonnet-4-5&opusModel=claude-opus-4-1&icon=anthropic&notes=imported%20from%20link",
            "ccswitch://v1/import?resource=provider&app=claude&name=Relay%20B&homepage=https%3A%2F%2Frelay-b.example&endpoint=https%3A%2F%2Fapi.relay-b.example&apiKey=sk-relay-b",
        ],
        "rows/deeplink-claude.txt",
    );
}

#[test]
fn deeplink_codex_rows() {
    import_deeplinks(
        AppType::Codex,
        &[
            "ccswitch://v1/import?resource=provider&app=codex&name=Relay%20Codex&homepage=https%3A%2F%2Frelay.example&endpoint=https%3A%2F%2Fapi.relay.example%2Fv1&apiKey=sk-relay-codex&model=gpt-5-codex&icon=openai",
        ],
        "rows/deeplink-codex.txt",
    );
}

/// 带 `config.toml` 的 Codex 链接：按白名单收下供应商自己的键，选路统一写成 custom。
#[test]
fn deeplink_codex_config_rows() {
    use base64::prelude::*;

    let config = serde_json::json!({
        "auth": { "OPENAI_API_KEY": "sk-relay-codex" },
        "config": concat!(
            "model_provider = \"relay\"\n",
            "model = \"gpt-5.5\"\n",
            "model_reasoning_effort = \"medium\"\n",
            "web_search = \"live\"\n",
            "approval_policy = \"never\"\n\n",
            "[features]\nstandalone_web_search = true\n\n",
            "[model_providers.relay]\n",
            "name = \"Relay Upstream\"\n",
            "base_url = \"https://api.relay.example/v1\"\n",
            "wire_api = \"responses\"\n",
            "requires_openai_auth = false\n",
            "http_headers = { \"x-relay-client\" = \"cc-switch\" }\n",
            "supports_websockets = true\n",
            "model_catalog_url = \"https://api.relay.example/v1/models?format=codex\"\n",
            "experimental_bearer_token = \"sk-relay-codex\"\n",
        )
    });
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=codex&name=Relay%20Codex%20Config&configFormat=json&config={}",
        BASE64_URL_SAFE_NO_PAD.encode(config.to_string())
    );
    import_deeplinks(AppType::Codex, &[&url], "rows/deeplink-codex-config.txt");
}

#[test]
fn deeplink_gemini_rows() {
    import_deeplinks(
        AppType::Gemini,
        &[
            "ccswitch://v1/import?resource=provider&app=gemini&name=Relay%20Gemini&homepage=https%3A%2F%2Frelay.example&endpoint=https%3A%2F%2Fapi.relay.example&apiKey=g-relay&model=gemini-2.5-pro",
        ],
        "rows/deeplink-gemini.txt",
    );
}

#[test]
fn deeplink_grokbuild_rows() {
    import_deeplinks(
        AppType::GrokBuild,
        &[
            "ccswitch://v1/import?resource=provider&app=grokbuild&name=Relay%20Grok&homepage=https%3A%2F%2Frelay.example&endpoint=https%3A%2F%2Fapi.relay.example%2Fv1&apiKey=xai-relay&model=grok-4.5",
        ],
        "rows/deeplink-grokbuild.txt",
    );
}

/// 带 `config.toml` 的 Grok Build 链接：只收 `models.default` 指向的那张表，地址和 Key
/// 用链接参数。
#[test]
fn deeplink_grokbuild_config_rows() {
    use base64::prelude::*;

    let config = serde_json::json!({
        "config": concat!(
            "[models]\ndefault = \"grok-4.6\"\n\n",
            "[model.\"grok-4.6\"]\nmodel = \"grok-4.6\"\nname = \"Grok 4.6 (Relay)\"\n",
            "description = \"Fast coding model\"\n",
            "base_url = \"https://ignored.example/v1\"\napi_key = \"sk-ignored\"\n",
            "api_backend = \"chat_completions\"\ncontext_window = 256000\n\n",
            "[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nname = \"Grok 4.5\"\n",
        )
    });
    let url = format!(
        "ccswitch://v1/import?resource=provider&app=grokbuild&name=Relay%20Grok%20Config&endpoint=https%3A%2F%2Fapi.relay.example%2Fv1&apiKey=xai-relay&configFormat=json&config={}",
        BASE64_URL_SAFE_NO_PAD.encode(config.to_string())
    );
    import_deeplinks(
        AppType::GrokBuild,
        &[&url],
        "rows/deeplink-grokbuild-config.txt",
    );
}

/// 首启：先把 live 导入成 `default`，再 seed 官方预设（顺序同 `lib.rs` 的启动流程）。
/// live 里供应商以外的部分进了通用配置片段，片段也一起锁。关键字段（模型名、推理档位
/// 等）不进片段、留在 `default` 行里：片段已冻结，收进去就再也写不回 live。
///
/// `settings_sort` 见 [`crate::util::sort_objects`]：Gemini 的 `.env` 解析顺序不稳定。
fn first_run_import(app: AppType, snapshot: &str, settings_sort: &[&str]) {
    let state = create_test_state().expect("create test state");
    import_default_config_test_hook(&state, app.clone()).expect("import live config");
    state
        .db
        .init_default_official_providers()
        .expect("seed official providers");
    let snippet = state
        .db
        .get_config_snippet(app.as_str())
        .expect("read common config snippet");
    let dump = format!(
        "{}\ncommon config snippet: {}\n",
        dump_provider_rows(&app, str::to_string, settings_sort),
        snippet.as_deref().unwrap_or("NULL"),
    );
    assert_golden(snapshot, &dump);
}

#[test]
fn first_run_claude_rows() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        ".claude/settings.json",
        r#"{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-live",
    "ANTHROPIC_BASE_URL": "https://relay.example",
    "ANTHROPIC_MODEL": "claude-sonnet-4-5",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
  },
  "permissions": {
    "allow": ["Bash(git status)"]
  },
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "command", "command": "say done" }] }]
  },
  "enabledPlugins": {
    "example@marketplace": true
  },
  "model": "opus",
  "statusLine": { "type": "command", "command": "~/.claude/statusline.sh" }
}"#,
    );
    first_run_import(AppType::Claude, "rows/first-run-claude.txt", &[]);
}

#[test]
fn first_run_codex_rows() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(".codex/auth.json", r#"{"OPENAI_API_KEY":"sk-live"}"#);
    write_home_file(
        ".codex/config.toml",
        r#"model_provider = "relay"
model = "gpt-5-codex"
model_reasoning_effort = "high"

[model_providers.relay]
name = "relay"
base_url = "https://relay.example/v1"
wire_api = "responses"

[projects."/Users/me/repo"]
trust_level = "trusted"

[mcp_servers.external]
command = "external"
"#,
    );
    first_run_import(AppType::Codex, "rows/first-run-codex.txt", &[]);
}

#[test]
fn first_run_gemini_rows() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        ".gemini/.env",
        "GEMINI_API_KEY=g-live\nGOOGLE_GEMINI_BASE_URL=https://relay.example\nGEMINI_MODEL=gemini-2.5-pro\n",
    );
    write_home_file(
        ".gemini/settings.json",
        r#"{
  "security": { "auth": { "selectedType": "gemini-api-key" } },
  "ui": { "theme": "GitHub" }
}"#,
    );
    first_run_import(AppType::Gemini, "rows/first-run-gemini.txt", &["/env"]);
}

#[test]
fn first_run_grokbuild_rows() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    write_home_file(
        ".grok/config.toml",
        r#"[models]
default = "grok-4.5"

[model."grok-4.5"]
model = "grok-4.5"
base_url = "https://relay.example/v1"
name = "Relay"
api_key = "xai-live"
api_backend = "responses"
context_window = 500000
"#,
    );
    first_run_import(AppType::GrokBuild, "rows/first-run-grokbuild.txt", &[]);
}
