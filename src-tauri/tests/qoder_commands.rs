use cc_switch_lib::{
    AppType, McpServer, McpService, Prompt, PromptService, Provider, ProviderService,
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, reset_test_fs, test_mutex};

struct QoderHomes {
    directory: tempfile::TempDir,
    previous: [Option<OsString>; 2],
}
impl QoderHomes {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let variables = ["QODER_CONFIG_DIR", "QODERCN_CONFIG_DIR"];
        let previous = variables.map(std::env::var_os);
        for (variable, folder) in variables.into_iter().zip(["global", "cn"]) {
            let path = directory.path().join(folder);
            fs::create_dir_all(&path).unwrap();
            std::env::set_var(variable, path);
        }
        Self {
            directory,
            previous,
        }
    }
    fn root(&self, folder: &str) -> PathBuf {
        self.directory.path().join(folder)
    }
    fn settings(&self, folder: &str) -> PathBuf {
        self.root(folder).join("settings.json")
    }
}
impl Drop for QoderHomes {
    fn drop(&mut self) {
        for (variable, previous) in ["QODER_CONFIG_DIR", "QODERCN_CONFIG_DIR"]
            .into_iter()
            .zip(&self.previous)
        {
            if let Some(previous) = previous {
                std::env::set_var(variable, previous);
            } else {
                std::env::remove_var(variable);
            }
        }
    }
}
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn server() -> McpServer {
    serde_json::from_value(json!({"id":"managed","name":"Managed","server":{"command":"node","args":["server.js"]},"apps":{"qoder":true,"qoder-cn":true}})).unwrap()
}

#[test]
fn qoder_mcp_lifecycle_keeps_editions_and_native_settings_separate() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let state = create_test_state().unwrap();
    let native = json!({"model":{"name":"auto"},"mcpServers":{"native":{"type":"http","url":"https://example.test/mcp","disabled":true,"trust":false}}});
    for folder in ["global", "cn"] {
        fs::write(homes.settings(folder), native.to_string()).unwrap();
    }
    McpService::upsert_server(&state, server()).unwrap();
    for folder in ["global", "cn"] {
        let settings = read(&homes.settings(folder));
        assert_eq!(settings["model"], native["model"]);
        assert_eq!(
            settings["mcpServers"]["native"],
            native["mcpServers"]["native"]
        );
        assert_eq!(settings["mcpServers"]["managed"]["command"], "node");
    }
    McpService::toggle_app(&state, "managed", AppType::QoderCn, false).unwrap();
    assert!(read(&homes.settings("cn"))["mcpServers"]
        .get("managed")
        .is_none());
    assert!(read(&homes.settings("global"))["mcpServers"]
        .get("managed")
        .is_some());
    McpService::toggle_app(&state, "managed", AppType::QoderCn, true).unwrap();
    McpService::delete_server(&state, "managed").unwrap();
    for folder in ["global", "cn"] {
        assert_eq!(
            read(&homes.settings(folder))["mcpServers"],
            native["mcpServers"]
        );
    }
    assert!(state.db.get_all_mcp_servers().unwrap().is_empty());
}

#[test]
fn qoder_mcp_multi_edition_failure_rolls_back_and_native_models_stay_in_cli() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let state = create_test_state().unwrap();
    let original = "{\"model\":{\"name\":\"auto\"}}";
    fs::write(homes.settings("global"), original).unwrap();
    fs::write(homes.settings("cn"), "broken JSON").unwrap();
    assert!(McpService::upsert_server(&state, server()).is_err());
    assert_eq!(
        fs::read_to_string(homes.settings("global")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(homes.settings("cn")).unwrap(),
        "broken JSON"
    );
    assert!(state.db.get_all_mcp_servers().unwrap().is_empty());
    for app in [AppType::Qoder, AppType::QoderCn] {
        assert!(ProviderService::list(&state, app.clone())
            .unwrap()
            .is_empty());
        assert!(!ProviderService::should_import_default_config_on_startup(&state, &app).unwrap());
        assert!(!ProviderService::import_default_config(&state, app.clone()).unwrap());
        let provider = Provider::with_id(
            "custom".into(),
            "Custom".into(),
            json!({"apiKey":"test"}),
            None,
        );
        assert!(ProviderService::add(&state, app, provider, true).is_err());
    }
}

#[test]
fn qoder_prompts_follow_each_editions_config_directory() {
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let state = create_test_state().unwrap();
    for (app, folder, content) in [
        (AppType::Qoder, "global", "Global instructions"),
        (AppType::QoderCn, "cn", "China instructions"),
    ] {
        let prompt: Prompt = serde_json::from_value(
            json!({"id":"global","name":"Global","content":content,"enabled":false}),
        )
        .unwrap();
        PromptService::upsert_prompt(&state, app.clone(), "global", prompt).unwrap();
        PromptService::enable_prompt(&state, app, "global").unwrap();
        assert_eq!(
            fs::read_to_string(homes.root(folder).join("AGENTS.md")).unwrap(),
            content
        );
    }
    assert_eq!(
        fs::read_to_string(homes.root("global").join("AGENTS.md")).unwrap(),
        "Global instructions"
    );
}

#[test]
fn qoder_usage_metadata_is_separate_from_native_credentials_and_editions() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let state = create_test_state().unwrap();
    let native = json!({"providers":{"native":{"baseUrl":"https://example.test/v1","apiKey":"native-key","model":"model"}},"model":{"name":"native/model"}});
    for folder in ["global", "cn"] {
        fs::write(homes.settings(folder), native.to_string()).unwrap();
    }
    let before = fs::read(homes.settings("cn")).unwrap();
    let script = serde_json::from_value(json!({"enabled":true,"language":"javascript","code":"({request:{url:'{{baseUrl}}/usage',method:'GET',headers:{Authorization:'Bearer {{apiKey}}'}},extractor:function(r){return r}})","autoQueryInterval":15})).unwrap();
    let models = cc_switch_lib::qoder_models::save_usage_script(
        &state.db,
        &AppType::QoderCn,
        "native",
        script,
    )
    .unwrap();
    assert!(
        models.metadata["native"]
            .usage_script
            .as_ref()
            .unwrap()
            .enabled
    );
    assert_eq!(fs::read(homes.settings("cn")).unwrap(), before);
    let stored = state
        .db
        .get_provider_by_id("native", "qoder-cn")
        .unwrap()
        .unwrap();
    assert_eq!(stored.settings_config, json!({}));
    assert!(state
        .db
        .get_provider_by_id("native", "qoder")
        .unwrap()
        .is_none());
    assert_eq!(read(&homes.settings("global")), native);
    // Query against a local fixture after credentials change outside CC Switch.
    let listener = TcpListener::bind("localhost:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let start = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut buffer = [0u8; 8192];
                    let count = stream.read(&mut buffer).unwrap();
                    let request = String::from_utf8_lossy(&buffer[..count]).to_ascii_lowercase();
                    assert!(request.starts_with("get /usage "), "{request}");
                    assert!(request.contains("authorization: bearer changed-key"));
                    let body = r#"{"remaining":42,"unit":"USD"}"#;
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                    break;
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && start.elapsed() < Duration::from_secs(10) =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("Usage fixture was not reached: {error}"),
            }
        }
    });
    let mut changed = native.clone();
    changed["providers"]["native"]["apiKey"] = json!("changed-key");
    changed["providers"]["native"]["baseUrl"] =
        json!(format!("http://localhost:{}", address.port()));
    fs::write(homes.settings("cn"), changed.to_string()).unwrap();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(ProviderService::query_usage(
            &state,
            AppType::QoderCn,
            "native",
        ))
        .unwrap();
    assert!(result.success, "{:?}", result.error);
    assert_eq!(result.data.unwrap()[0].remaining, Some(42.0));
    handle.join().unwrap();
}

#[test]
fn qoder_skills_enable_independently_and_preserve_native_skill_directories() {
    use cc_switch_lib::SkillService;
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let state = create_test_state().unwrap();
    let directory = "qoder-managed-skill";
    let ssot = SkillService::get_ssot_dir().unwrap().join(directory);
    fs::create_dir_all(&ssot).unwrap();
    fs::write(
        ssot.join("SKILL.md"),
        "---\nname: managed\ndescription: fixture\n---\nManaged skill",
    )
    .unwrap();
    let skill = serde_json::from_value(json!({"id":"qoder-skill","name":"Managed","description":"fixture","directory":directory,"apps":{},"installedAt":0})).unwrap();
    state.db.save_skill(&skill).unwrap();
    fs::write(homes.settings("global"), json!({"skills":{"disabled":[directory,"unrelated"]},"skillOverrides":{directory:"off","unrelated":"off"},"model":{"name":"auto"}}).to_string()).unwrap();
    SkillService::toggle_app(&state.db, "qoder-skill", &AppType::Qoder, true).unwrap();
    assert_eq!(
        read(&homes.settings("global"))["skills"]["disabled"],
        json!(["unrelated"])
    );
    assert_eq!(
        read(&homes.settings("global"))["skillOverrides"],
        json!({"unrelated":"off"})
    );
    assert!(homes
        .root("global")
        .join("skills")
        .join(directory)
        .join("SKILL.md")
        .exists());
    assert!(!homes.root("cn").join("skills").join(directory).exists());
    SkillService::toggle_app(&state.db, "qoder-skill", &AppType::QoderCn, true).unwrap();
    SkillService::toggle_app(&state.db, "qoder-skill", &AppType::Qoder, false).unwrap();
    assert!(!homes.root("global").join("skills").join(directory).exists());
    assert!(homes
        .root("cn")
        .join("skills")
        .join(directory)
        .join("SKILL.md")
        .exists());
    // An unrelated native skill is never removed by managed Skill synchronization.
    let native = homes.root("global").join("skills/native-skill");
    fs::create_dir_all(&native).unwrap();
    fs::write(native.join("SKILL.md"), "Native user skill").unwrap();
    assert!(SkillService::resync_all_apps(&state.db)
        .iter()
        .filter(|outcome| outcome.app == "qoder" || outcome.app == "qoder-cn")
        .all(|outcome| outcome.ok));
    assert_eq!(
        fs::read_to_string(native.join("SKILL.md")).unwrap(),
        "Native user skill"
    );
    let stored = state
        .db
        .get_installed_skill("qoder-skill")
        .unwrap()
        .unwrap();
    assert!(!stored.apps.qoder && stored.apps.qoder_cn);
}

#[test]
fn qoder_model_management_configures_each_edition_without_running_cli() {
    use cc_switch_lib::qoder_models;
    let _guard = test_mutex().lock().unwrap();
    reset_test_fs();
    let homes = QoderHomes::new();
    let provider = json!({"displayName":"Custom","baseUrl":"https://example.test/v1","apiKey":"test-key","protocol":"anthropic","authType":"api-key","models":[{"model":"custom-model","contextWindow":200000,"maxOutputTokens":8192}],"model":"custom-model"});
    assert!(qoder_models::get(&AppType::Qoder)
        .unwrap()
        .providers
        .is_empty());
    assert!(
        !homes.settings("global").exists(),
        "reading must not create settings"
    );
    qoder_models::save_provider(&AppType::Qoder, "custom", provider.clone(), None, true).unwrap();
    assert_eq!(
        read(&homes.settings("global"))["privacy"]["usageStatisticsEnabled"],
        false
    );
    assert_eq!(
        qoder_models::get(&AppType::Qoder).unwrap().current_model,
        "custom/custom-model"
    );
    assert!(qoder_models::get(&AppType::QoderCn)
        .unwrap()
        .providers
        .is_empty());
    qoder_models::save_provider(&AppType::QoderCn, "custom", provider.clone(), None, false)
        .unwrap();
    assert_eq!(
        read(&homes.settings("cn"))["privacy"]["usageStatisticsEnabled"],
        false
    );
    for folder in ["global", "cn"] {
        let mut settings = read(&homes.settings(folder));
        settings["privacy"]["usageStatisticsEnabled"] = json!(true);
        settings["privacy"]["otherOption"] = json!("keep");
        fs::write(homes.settings(folder), settings.to_string()).unwrap();
    }
    qoder_models::set_model(&AppType::QoderCn, "custom/custom-model").unwrap();
    qoder_models::delete_provider(&AppType::Qoder, "custom", provider.clone()).unwrap();
    for folder in ["global", "cn"] {
        let settings = read(&homes.settings(folder));
        assert_eq!(settings["privacy"]["usageStatisticsEnabled"], false);
        assert_eq!(settings["privacy"]["otherOption"], "keep");
    }
    assert_eq!(
        qoder_models::get(&AppType::Qoder).unwrap().current_model,
        "auto"
    );
    assert!(qoder_models::get(&AppType::Qoder)
        .unwrap()
        .providers
        .is_empty());
    assert_eq!(
        qoder_models::get(&AppType::QoderCn).unwrap().current_model,
        "custom/custom-model"
    );
    assert_eq!(read(&homes.settings("cn"))["providers"]["custom"], provider);
}
