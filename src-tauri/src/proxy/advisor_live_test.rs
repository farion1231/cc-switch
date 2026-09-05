//! Opt-in, ignored live proof. Never compiled into release applications.
use super::ProxyError;
use serde_json::{json, Value};

pub(super) fn reserve_request(url: &str, model: &str) -> Result<(), ProxyError> {
    let Ok(path) = std::env::var("CC_SWITCH_ADVISOR_LIVE_LEDGER") else {
        return Ok(());
    };
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK
        .lock()
        .map_err(|_| ProxyError::Internal("Live budget lock failed".into()))?;
    let reserve = || -> Result<(), Box<dyn std::error::Error>> {
        let url = url::Url::parse(url)?;
        if url.scheme() != "https"
            || url.host_str() != Some("chatgpt.com")
            || url.path() != "/backend-api/codex/responses"
        {
            return Err("Live proof permits only the official Codex model endpoint".into());
        }
        let mut ledger: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let used = ledger["used"].as_u64().ok_or("Missing live budget usage")?;
        let limit = ledger["limit"]
            .as_u64()
            .ok_or("Missing live budget limit")?
            .min(12);
        if used >= limit {
            return Err("Authorized live model request budget exhausted".into());
        }
        ledger["used"] = json!(used + 1);
        ledger["requests"]
            .as_array_mut()
            .ok_or("Missing live ledger")?
            .push(json!({"number":used+1,"model":model}));
        std::fs::write(path, serde_json::to_vec_pretty(&ledger)?)?;
        Ok(())
    };
    reserve().map_err(|_| {
        ProxyError::InvalidRequest(
            "Live request refused by the persistent budget/endpoint guard".into(),
        )
    })
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "Requires explicit credential authorization and a persistent request budget; run alone"]
async fn advisor_live_claude_code_consults_astra_and_continues() {
    use super::{
        providers::codex_oauth_auth::CodexOAuthManager, server::ProxyServer, types::ProxyConfig,
    };
    use crate::{
        commands::CodexOAuthState,
        database::Database,
        provider::{AuthBinding, AuthBindingSource, Provider, ProviderMeta},
    };
    use std::sync::Arc;

    let auth_path =
        std::env::var("CC_SWITCH_ADVISOR_LIVE_AUTH_FILE").expect("Explicit auth file required");
    std::env::var("CC_SWITCH_ADVISOR_LIVE_LEDGER").expect("Persistent budget required");
    let root = std::path::PathBuf::from(
        std::env::var("CC_SWITCH_TEST_HOME").expect("Isolated test home required"),
    );
    let auth: Value =
        serde_json::from_slice(&std::fs::read(auth_path).expect("Read authorized login"))
            .expect("Parse login");
    let tokens = &auth["tokens"];
    let manager = Arc::new(CodexOAuthManager::new(root.clone()));
    manager
        .add_test_account_with_workspace_and_access_token(
            "advisor-live-test",
            tokens["account_id"].as_str().expect("Workspace required"),
            tokens["access_token"]
                .as_str()
                .expect("Access token required"),
            tokens["id_token"].as_str(),
        )
        .await
        .expect("Seed isolated in-memory access token");
    // The helper persists an id_token but no real refresh token. Remove that file
    // immediately; the real access token remains only in the manager's memory.
    std::fs::remove_file(root.join("codex_oauth_auth.json"))
        .expect("Remove temporary account file");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.ccswitch.advisor-live-proof".into();
    let app = tauri::Builder::default()
        .any_thread()
        .manage(CodexOAuthState(manager))
        .build(context)
        .expect("Create isolated app handle");
    let db = Arc::new(Database::memory().unwrap());
    let mut provider = Provider::with_id(
        "advisor-live-test".into(),
        "Advisor proof".into(),
        json!({"env":{
        "ANTHROPIC_AUTH_TOKEN":"managed-oauth", "ANTHROPIC_MODEL":"gpt-5.6-sol", "CC_SWITCH_ADVISOR_MODEL":"gpt-6-astra"}}),
        None,
    );
    provider.meta = Some(ProviderMeta {
        provider_type: Some("codex_oauth".into()),
        auth_binding: Some(AuthBinding {
            source: AuthBindingSource::ManagedAccount,
            auth_provider: Some("codex_oauth".into()),
            account_id: Some("advisor-live-test".into()),
        }),
        ..Default::default()
    });
    db.save_provider("claude", &provider).unwrap();
    db.set_current_provider("claude", &provider.id).unwrap();
    let server = ProxyServer::new(
        ProxyConfig {
            listen_port: 0,
            ..Default::default()
        },
        db,
        Some(app.handle().clone()),
    );
    let info = server
        .start()
        .await
        .expect("Start production CC Switch proxy");
    let client_dir = root.join("client");
    std::fs::create_dir_all(&client_dir).unwrap();
    let mut command = std::process::Command::new(
        std::env::var("CC_SWITCH_ADVISOR_CLAUDE_BIN").expect("Claude executable required"),
    );
    for (key, _) in std::env::vars().filter(|(key, _)| {
        key.starts_with("ANTHROPIC_")
            || key.starts_with("CLAUDE_CODE_")
            || key == "CLAUDE_CONFIG_DIR"
    }) {
        command.env_remove(key);
    }
    command.current_dir(&client_dir)
        .env("CLAUDE_CONFIG_DIR", client_dir.join("profile"))
        .env("ANTHROPIC_BASE_URL", format!("http://127.0.0.1:{}", info.port))
        .env("ANTHROPIC_API_KEY", "local-proxy-placeholder")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .args(["-p", "Consult advisor exactly once to check 7 times 8, then reply ADVISOR_OK: 56 only after reading its advice. Do not use other tools.",
            "--model", "claude-sonnet-4-6", "--advisor", "opus", "--output-format", "stream-json", "--verbose",
            "--no-session-persistence", "--strict-mcp-config", "--tools", "", "--max-turns", "2",
            "--system-prompt", "You are testing advisor consultation. Use the advisor tool when requested, then finish concisely."])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    fn drain(mut pipe: impl std::io::Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.read_to_end(&mut bytes).unwrap();
            bytes
        })
    }
    let mut child = command.spawn().expect("Run real Claude Code");
    let stdout_reader = drain(child.stdout.take().unwrap());
    let stderr_reader = drain(child.stderr.take().unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    };
    let output = std::process::Output {
        status,
        stdout: stdout_reader.join().unwrap(),
        stderr: stderr_reader.join().unwrap(),
    };
    server.stop().await.unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.pointer("/message/content").and_then(Value::as_array))
        .flatten()
        .collect();
    let consultation = blocks
        .iter()
        .any(|block| block["type"] == "server_tool_use" && block["name"] == "advisor");
    let advice = blocks.iter().any(|block| {
        block["type"] == "advisor_tool_result" && block["content"]["type"] == "advisor_result"
    });
    let result = events
        .iter()
        .find(|event| event["type"] == "result")
        .cloned()
        .unwrap_or(json!({}));
    let astra = result
        .pointer("/usage/iterations")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item["type"] == "advisor_message"
                    && item["model"] == "gpt-6-astra"
                    && item["output_tokens"].as_u64().unwrap_or(0) > 0
            })
        });
    let success = output.status.success()
        && consultation
        && advice
        && astra
        && result["is_error"] == false
        && result["result"]
            .as_str()
            .is_some_and(|text| text.contains("ADVISOR_OK") && text.contains("56"));
    let report = json!({"success":success,"exit_code":output.status.code(),"server_consultation":consultation,"advisor_result":advice,"astra_usage":astra,
        "result":result.get("result"),"usage":result.get("usage"),"error":result.get("error"),"stderr_bytes":output.stderr.len(),
        "live_requests":serde_json::from_slice::<Value>(&std::fs::read(std::env::var("CC_SWITCH_ADVISOR_LIVE_LEDGER").unwrap()).unwrap()).unwrap()});
    std::fs::write(
        std::env::var("CC_SWITCH_ADVISOR_LIVE_REPORT").expect("Report path required"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
    assert!(
        success,
        "Real Claude Code advisor proof did not pass; inspect the sanitized report"
    );
}
