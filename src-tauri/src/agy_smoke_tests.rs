//! Explicit opt-in native smoke test. Never reads the real database itself.
use crate::{agy_config, app_config::AppType, database::Database, provider::Provider};
use serde_json::{json, Value};
use std::{fs, path::Path, sync::Arc};

struct RestoreHome(Option<std::ffi::OsString>);
impl Drop for RestoreHome {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        let _ = crate::settings::reload_settings();
    }
}

async fn native(bin: &Path, home: &Path, real: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .current_dir(home.join("workspace"))
        .env("HOME", home)
        .env("AGY_REAL_BIN", real)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_GEMINI_BASE_URL")
        .kill_on_drop(true);
    tokio::time::timeout(std::time::Duration::from_secs(100), cmd.output())
        .await
        .expect("native agy timed out")
        .expect("launch native agy")
}

#[tokio::test]
#[ignore = "requires CCS_AGY_SMOKE_CONFIG, CCS_AGY_SMOKE_BIN and explicit paid API authorization"]
#[serial_test::serial]
async fn agy_live_provider_skill_session_and_tokens() {
    let config = std::env::var("CCS_AGY_SMOKE_CONFIG").expect("explicit fixture file required");
    let real = std::env::var("CCS_AGY_SMOKE_BIN").expect("explicit native binary required");
    let settings: Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let _restore = RestoreHome(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", home);
    crate::settings::reload_settings().unwrap();
    fs::create_dir_all(home.join("workspace")).unwrap();
    let db = Arc::new(Database::memory().unwrap());
    let row = Provider::with_id("smoke".into(), "Smoke".into(), settings.clone(), None);
    db.save_provider("agy", &row).unwrap();
    crate::services::provider::agy_direct::switch_to(&db, &row).unwrap();
    assert_eq!(
        crate::mode::current::provider_for(
            &db,
            &AppType::Agy,
            crate::mode::current::Purpose::Direct
        )
        .unwrap()
        .as_deref(),
        Some("smoke")
    );
    let bin = agy_config::agy_dir().join("bin/agy");

    let key = settings["env"]["GEMINI_API_KEY"].as_str().unwrap();
    let base = settings["env"]["GOOGLE_GEMINI_BASE_URL"]
        .as_str()
        .filter(|v| !v.is_empty())
        .unwrap_or("https://generativelanguage.googleapis.com");
    let models = crate::services::model_fetch::fetch_models(
        base,
        key,
        false,
        None,
        None,
        Some("google-generative-ai"),
        None,
    )
    .await
    .expect("CCS model discovery");
    let live = agy_config::read_live_settings().unwrap();
    let model = live["config"]["model"]["name"].as_str().unwrap();
    assert!(
        models.iter().any(|m| m.id == model),
        "configured model absent from API catalog"
    );
    println!("SMOKE model_catalog={} selected={model}", models.len());

    // Install through the existing import + toggle path, then let the real CLI load it.
    let skill_dir = agy_config::customizations_dir().join("skills/ccs-smoke");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(skill_dir.join("SKILL.md"), "---\nname: ccs-smoke\ndescription: An explicitly requested smoke test.\n---\nWhen invoked, respond with exactly CCS_AGY_SKILL_OK. Do not call tools or access files.\n").unwrap();
    let imported = crate::services::SkillService::import_from_apps(
        &db,
        vec![crate::services::skill::ImportSkillSelection {
            directory: "ccs-smoke".into(),
            apps: crate::app_config::SkillApps {
                agy: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert_eq!(imported.len(), 1);
    crate::services::SkillService::toggle_app(&db, &imported[0].id, &AppType::Agy, false).unwrap();
    assert!(!skill_dir.exists());
    crate::services::SkillService::toggle_app(&db, &imported[0].id, &AppType::Agy, true).unwrap();
    assert!(skill_dir.join("SKILL.md").is_file());
    println!("SMOKE skill_import_disable_enable=ok");

    let server = home.join("smoke_mcp.py");
    let events = home.join("mcp-events.jsonl");
    fs::write(&server, r#"import json,sys
for line in sys.stdin:
    request=json.loads(line)
    method=request.get('method','')
    with open(sys.argv[1],'a') as log:
        log.write(json.dumps({'method':method})+'\n')
    if 'id' not in request: continue
    if method=='initialize':
        result={'protocolVersion':request['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'ccs-smoke','version':'1'}}
    elif method=='tools/list':
        result={'tools':[{'name':'echo','description':'Return a fixed smoke test marker','inputSchema':{'type':'object','properties':{}},'annotations':{'readOnlyHint':True}}]}
    elif method=='tools/call':
        result={'content':[{'type':'text','text':'CCS_MCP_OK'}]}
    elif method in ('resources/list','prompts/list'):
        result={method.split('/')[0]:[]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)
"#).unwrap();
    let spec = json!({"command":"python3","args":[server, events]});
    crate::mcp::agy::sync("ccs-smoke", Some(&spec)).unwrap();
    let list = native(&bin, home, &real, &["mcp", "list"]).await;
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains("ccs-smoke"));
    crate::mcp::agy::sync("ccs-smoke", None).unwrap();
    println!("SMOKE native_mcp_recognized_and_disabled=ok");
    crate::mcp::agy::sync("ccs-smoke", Some(&spec)).unwrap();

    // Headless agy denies Skill reads unless explicitly allowed. Limit the test grant
    // to this fixture (including its SSOT target), never to the real home directory.
    let skill_file = skill_dir.join("SKILL.md");
    let mut native_settings: Value =
        serde_json::from_slice(&fs::read(agy_config::settings_path()).unwrap()).unwrap();
    native_settings["permissions"] = json!({"allow":[
        format!("read_file({})", skill_file.display()),
        format!("read_file({})", skill_file.canonicalize().unwrap().display())
    ]});
    fs::write(
        agy_config::settings_path(),
        serde_json::to_vec(&native_settings).unwrap(),
    )
    .unwrap();

    let output = native(
        &bin,
        home,
        &real,
        &[
            "--print",
            "/ccs-smoke",
            "--output-format",
            "json",
            "--print-timeout",
            "75s",
        ],
    )
    .await;
    // No raw stdout/stderr in failures: a third-party CLI may echo credentials.
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("CCS_AGY_SKILL_OK") {
        println!(
            "SMOKE diagnostic={}",
            stdout
                .replace(key, "[redacted]")
                .chars()
                .take(3000)
                .collect::<String>()
        );
        println!(
            "SMOKE stderr={}",
            String::from_utf8_lossy(&output.stderr)
                .replace(key, "[redacted]")
                .chars()
                .take(1500)
                .collect::<String>()
        );
    }
    assert!(
        output.status.success(),
        "native request failed (output suppressed)"
    );
    assert!(
        stdout.contains("CCS_AGY_SKILL_OK"),
        "skill response marker missing (output suppressed)"
    );
    println!("SMOKE native_generation_and_skill=CCS_AGY_SKILL_OK");
    let response: Value = serde_json::from_str(&stdout).expect("native JSON response");
    println!("SMOKE native_usage={}", response["usage"]);
    let mcp_events = fs::read_to_string(&events).expect("native MCP was started");
    assert!(mcp_events.contains("initialize"));
    assert!(mcp_events.contains("tools/list"));
    println!("SMOKE native_mcp_handshake_and_tool_discovery=ok");

    let sessions = crate::session_manager::providers::agy::scan_sessions();
    assert!(!sessions.is_empty());
    let mut preview_found = false;
    for session in &sessions {
        if let Some(path) = &session.source_path {
            let messages =
                crate::session_manager::providers::agy::load_messages(Path::new(path)).unwrap();
            preview_found |= serde_json::to_string(&messages)
                .unwrap()
                .contains("CCS_AGY_SKILL_OK");
        }
    }
    assert!(
        preview_found,
        "native generated reply absent from CCS preview"
    );
    let first = crate::services::session_usage_agy::sync_agy_usage(&db).unwrap();
    let second = crate::services::session_usage_agy::sync_agy_usage(&db).unwrap();
    assert!(first.imported > 0);
    assert_eq!(second.imported, 0);
    println!(
        "SMOKE session_preview=ok imported={} repeated_import=0",
        first.imported
    );
}
