//! Explicit opt-in native smoke test: configures Cline only through CC Switch
//! services, then runs the real `cline` against a live endpoint. Never touches
//! the real database or `~/.cline`.
//!
//! CCS_CLINE_SMOKE_BIN      path to the `cline` binary
//! CCS_CLINE_SMOKE_URL      OpenAI- and Anthropic-compatible base URL including `/v1`
//! CCS_CLINE_SMOKE_KEY_FILE file containing the API key
//! CCS_CLINE_SMOKE_MODEL    model id served by both protocols
use crate::{
    app_config::{AppType, McpApps, McpServer, SkillApps},
    database::Database,
    prompt::Prompt,
    provider::Provider,
    services::{skill::ImportSkillSelection, McpService, PromptService, ProviderService},
    store::AppState,
};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

struct RestoreEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Drop for RestoreEnv {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = crate::settings::reload_settings();
    }
}

fn provider(id: &str, base: &str, key: &str, model: &str, anthropic: bool) -> Provider {
    let mut meta = json!({"name": id, "baseUrl": base, "defaultModelId": model,
                          "protocol": if anthropic { "anthropic" } else { "openai-chat" }});
    if anthropic {
        meta["client"] = json!("anthropic");
    }
    Provider::with_id(
        id.into(),
        id.into(),
        json!({"settings": {"apiKey": key, "model": model}, "provider": meta,
               "models": {model: {"id": model, "name": model, "contextWindow": 128000}}}),
        None,
    )
}

fn run(bin: &str, home: &Path, prompt: &str) -> (String, Vec<String>) {
    let output = std::process::Command::new(bin)
        .args(["--json", "-t", "180", prompt])
        .current_dir(home.join("workspace"))
        .env("HOME", home)
        .env("CLINE_DIR", home.join(".cline"))
        .env_remove("CLINE_DATA_DIR")
        .output()
        .expect("launch native cline");
    let mut text = String::new();
    let mut tools = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event["type"] == "run_result" {
            text = event["text"].as_str().unwrap_or_default().to_string();
        }
        if event["event"]["type"] == "content_start" && event["event"]["contentType"] == "tool" {
            tools.push(
                event["event"]["toolName"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
            );
        }
    }
    (text, tools)
}

fn stop_hub(home: &Path) {
    let workspace = fs::canonicalize(home.join("workspace")).unwrap();
    let _ = std::process::Command::new("pkill")
        .args([
            "-f",
            &format!("cline-hub-daemon --cwd {}", workspace.display()),
        ])
        .status();
    std::thread::sleep(std::time::Duration::from_secs(1));
}

#[test]
#[ignore = "requires CCS_CLINE_SMOKE_* and explicit API authorization"]
#[serial_test::serial]
fn cline_live_provider_mcp_skill_prompt_session_and_usage() {
    let bin = std::env::var("CCS_CLINE_SMOKE_BIN").expect("explicit native binary required");
    let base = std::env::var("CCS_CLINE_SMOKE_URL").expect("endpoint required");
    let key = fs::read_to_string(std::env::var("CCS_CLINE_SMOKE_KEY_FILE").expect("key file"))
        .unwrap()
        .trim()
        .to_string();
    let model = std::env::var("CCS_CLINE_SMOKE_MODEL").expect("model required");

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let _restore = RestoreEnv(
        ["CC_SWITCH_TEST_HOME", "CLINE_DIR", "CLINE_DATA_DIR"]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
    );
    std::env::set_var("CC_SWITCH_TEST_HOME", home);
    std::env::set_var("CLINE_DIR", home.join(".cline"));
    std::env::remove_var("CLINE_DATA_DIR");
    crate::settings::reload_settings().unwrap();
    fs::create_dir_all(home.join("workspace")).unwrap();
    let db = Arc::new(Database::memory().unwrap());
    let state = AppState::new(db.clone());

    // Providers: one per protocol, both added to Cline's native config.
    for (id, anthropic) in [("ccs-smoke-chat", false), ("ccs-smoke-anthropic", true)] {
        ProviderService::add(
            &state,
            AppType::Cline,
            provider(id, &base, &key, &model, anthropic),
            true,
        )
        .unwrap();
    }

    // MCP: a stdio echo server that logs its calls.
    let calls = home.join("mcp-calls.log");
    let server = home.join("mcp-server.py");
    fs::write(
        &server,
        format!(
            r#"import sys, json
def send(o): sys.stdout.write(json.dumps(o)+"\n"); sys.stdout.flush()
for line in sys.stdin:
    m = json.loads(line); mid = m.get("id"); meth = m.get("method")
    if meth == "initialize": send({{"jsonrpc":"2.0","id":mid,"result":{{"protocolVersion":m["params"].get("protocolVersion","2024-11-05"),"capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"ccs","version":"1"}}}}}})
    elif meth == "tools/list": send({{"jsonrpc":"2.0","id":mid,"result":{{"tools":[{{"name":"ccs_echo","description":"Echo text","inputSchema":{{"type":"object","properties":{{"t":{{"type":"string"}}}},"required":["t"]}}}}]}}}})
    elif meth == "tools/call":
        open({calls:?}, "a").write(json.dumps(m["params"]) + "\n")
        send({{"jsonrpc":"2.0","id":mid,"result":{{"content":[{{"type":"text","text":"ECHO_FROM_CCS_MCP:" + m["params"]["arguments"].get("t","")}}]}}}})
    elif mid is not None: send({{"jsonrpc":"2.0","id":mid,"result":{{}}}})
"#
        ),
    )
    .unwrap();
    McpService::upsert_server(
        &state,
        McpServer {
            id: "ccs-smoke".into(),
            name: "CCS smoke".into(),
            server: json!({"type": "stdio", "command": "python3", "args": ["-I", server]}),
            apps: McpApps {
                cline: true,
                ..Default::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        },
    )
    .unwrap();

    // Global prompt written to ~/.cline/rules/AGENTS.md.
    let prompt: Prompt = serde_json::from_value(json!({"id": "smoke", "name": "Smoke",
        "content": "When the user message is exactly 'ccs ping', reply with exactly CCS_RULE_PONG.",
        "enabled": false}))
    .unwrap();
    PromptService::upsert_prompt(&state, AppType::Cline, "smoke", prompt).unwrap();
    PromptService::enable_prompt(&state, AppType::Cline, "smoke").unwrap();

    // Skill imported from another app's directory and enabled for Cline only.
    let source = home.join(".claude").join("skills").join("ccs-smoke");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: ccs-smoke\ndescription: CC Switch smoke skill.\n---\nReply with exactly CCS_SKILL_OK.\n",
    )
    .unwrap();
    crate::services::SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "ccs-smoke".into(),
            apps: SkillApps {
                cline: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert!(home.join(".cline/skills/ccs-smoke/SKILL.md").exists());

    stop_hub(home);
    ProviderService::switch(&state, AppType::Cline, "ccs-smoke-chat").unwrap();
    let (text, _) = run(&bin, home, "Reply with exactly: PONG");
    assert!(text.contains("PONG"), "chat provider: {text}");
    ProviderService::switch(&state, AppType::Cline, "ccs-smoke-anthropic").unwrap();
    let (text, _) = run(&bin, home, "Reply with exactly: PONG");
    assert!(text.contains("PONG"), "anthropic provider: {text}");

    let (text, tools) = run(
        &bin,
        home,
        "Call the ccs_echo MCP tool with t set to smoke, then reply with exactly what it returned.",
    );
    assert!(
        tools.iter().any(|tool| tool == "ccs-smoke__ccs_echo"),
        "{tools:?}"
    );
    assert!(fs::read_to_string(&calls).unwrap().contains("smoke"));
    assert!(text.contains("ECHO_FROM_CCS_MCP:smoke"), "{text}");
    let (text, tools) = run(
        &bin,
        home,
        "Use the ccs-smoke skill and follow its instructions.",
    );
    assert!(tools.iter().any(|tool| tool == "skills"), "{tools:?}");
    assert!(text.contains("CCS_SKILL_OK"), "{text}");
    let (text, _) = run(&bin, home, "ccs ping");
    assert!(text.contains("CCS_RULE_PONG"), "{text}");
    stop_hub(home);

    // Sessions are listed and readable through the session manager.
    let sessions = crate::session_manager::providers::cline::scan_sessions();
    assert!(sessions.len() >= 5, "{} sessions", sessions.len());
    for session in &sessions {
        let path = session.source_path.as_deref().expect("source path");
        assert!(
            !crate::session_manager::providers::cline::load_messages(Path::new(path))
                .unwrap()
                .is_empty()
        );
    }

    // Usage is imported once.
    let first = crate::services::session_usage_cline::sync_cline_usage(&db).unwrap();
    assert!(first.imported >= 6, "{first:?}");
    let second = crate::services::session_usage_cline::sync_cline_usage(&db).unwrap();
    assert_eq!(second.imported, 0, "{second:?}");
}
