//! 多目标写入的集成检查：在临时 HOME 里，用临时目录模拟 WSL 目标（真实 WSL 只在
//! Windows 上有），走真实的切换、保存和同步链路。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use serial_test::serial;

use super::model::{AppTargetsConfig, TargetId, WindowsTarget, WslTarget};
use super::service::{self, Resolution};
use super::status::{self, TargetStatus};
use super::sync::{self, SyncKind};
use crate::app_config::{AppType, McpApps, McpServer};
use crate::database::Database;
use crate::provider::Provider;
use crate::services::ProviderService;
use crate::store::AppState;

fn with_home<T>(test: impl FnOnce(&AppState, &Path) -> T) -> T {
    let temp = tempfile::tempdir().expect("tempdir");
    let old_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
    let old_home = std::env::var_os("HOME");
    std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
    std::env::set_var("HOME", temp.path());
    crate::settings::reload_settings().expect("reload settings");

    let db = Arc::new(Database::memory().expect("in-memory database"));
    let state = AppState::new(db);
    let result = test(&state, temp.path());

    match old_test_home {
        Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
        None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
    }
    match old_home {
        Some(value) => std::env::set_var("HOME", value),
        None => std::env::remove_var("HOME"),
    }
    let _ = crate::settings::reload_settings();
    result
}

fn claude(id: &str, base: &str, key: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({ "env": { "ANTHROPIC_BASE_URL": base, "ANTHROPIC_AUTH_TOKEN": key } }),
        None,
    )
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read")).expect("json")
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Windows 主环境（默认目录）+ 一个模拟的 WSL 跟随目标。
fn windows_plus_wsl(distro: &str, dir: &Path) -> AppTargetsConfig {
    AppTargetsConfig {
        primary: "windows".into(),
        windows: WindowsTarget {
            enabled: true,
            route: false,
            dir: None,
        },
        wsl: vec![WslTarget {
            distro: distro.into(),
            enabled: true,
            route: false,
            dir: Some(dir.to_string_lossy().into_owned()),
        }],
        extra: Default::default(),
    }
}

fn set_targets(app: &AppType, config: AppTargetsConfig) {
    crate::settings::set_app_targets(app, config, None).expect("save targets");
}

fn seed_claude(state: &AppState) {
    for provider in [
        claude("a", "https://a.example/api", "sk-a"),
        claude("b", "https://b.example/api", "sk-b"),
    ] {
        state.db.save_provider("claude", &provider).unwrap();
    }
    ProviderService::switch(state, AppType::Claude, "a").expect("initial switch");
}

fn activate(state: &AppState, app: &str, target: &str, id: &str) -> super::sync::TargetReport {
    service::activate(state, app, target, id).expect("activate")
}

#[test]
#[serial]
fn each_environment_keeps_its_own_provider_and_unmanaged_fields() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        write(
            &wsl_dir.join("settings.json"),
            r#"{ "permissions": { "allow": ["Bash(ls)"] }, "env": { "FOO": "1" } }"#,
        );
        set_targets(&AppType::Claude, windows_plus_wsl("Ubuntu", &wsl_dir));

        // WSL 用 b，Windows 仍是 a。
        let report = activate(state, "claude", "wsl:Ubuntu", "b");
        assert_eq!(report.target, "wsl:Ubuntu");
        assert_eq!(report.record.status, TargetStatus::Ok);
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(
            follower["env"]["ANTHROPIC_BASE_URL"],
            "https://b.example/api"
        );
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");
        assert_eq!(follower["env"]["FOO"], "1", "unmanaged env kept");
        assert_eq!(follower["permissions"]["allow"][0], "Bash(ls)");
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert_eq!(primary["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

        // 主环境切换只改 Windows，WSL 继续用 b。
        let result = ProviderService::switch(state, AppType::Claude, "b").expect("switch");
        assert!(result.targets.is_empty());
        ProviderService::switch(state, AppType::Claude, "a").expect("switch back");
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert_eq!(primary["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

        let record = status::get("claude", "wsl:Ubuntu").expect("record");
        assert_eq!(record.provider_id.as_deref(), Some("b"));
        assert_eq!(sync::followers_using(&AppType::Claude, "b").len(), 1);

        // 主环境入口也能用同一个接口。
        let report = activate(state, "claude", "windows", "b");
        assert_eq!(report.record.status, TargetStatus::Ok);
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert_eq!(primary["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");

        // 跟随环境的写前意图和首写备份在它自己的状态目录里，主环境的状态不受影响。
        let target_store = home
            .join(".cc-switch")
            .join("targets")
            .join("claude")
            .join("wsl-ubuntu");
        assert!(target_store
            .join("backups")
            .join("live-first-write")
            .exists());
        assert!(crate::mode::state::pending(
            &crate::live::engine::DeviceStore::for_device(),
            "claude"
        )
        .unwrap()
        .is_none());
    });
}

#[test]
#[serial]
fn failed_activation_keeps_the_previous_provider_and_retry_reapplies_it() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        set_targets(&AppType::Claude, windows_plus_wsl("Ubuntu", &wsl_dir));
        activate(state, "claude", "wsl:Ubuntu", "a");

        write(&wsl_dir.join("settings.json"), "{ broken");
        let report = activate(state, "claude", "wsl:Ubuntu", "b");
        assert_eq!(report.record.status, TargetStatus::Error);
        assert_eq!(report.record.reason.as_deref(), Some("write_failed"));
        assert_eq!(
            fs::read_to_string(wsl_dir.join("settings.json")).unwrap(),
            "{ broken",
            "broken file left as is"
        );
        let record = status::get("claude", "wsl:Ubuntu").unwrap();
        assert_eq!(record.provider_id.as_deref(), Some("a"), "not switched");

        // 修好之后重试：写回这个环境正在用的那家。
        write(&wsl_dir.join("settings.json"), "{}");
        let retried =
            service::sync_now(state, "claude", Some(vec!["wsl:Ubuntu".into()])).expect("retry");
        assert!(retried.primary.is_none());
        assert_eq!(retried.targets[0].record.status, TargetStatus::Ok);
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

        // 编辑正在用的那家：只重写用它的环境。
        let mut edited = claude("a", "https://a2.example/api", "sk-a2");
        edited.name = "A".into();
        state.db.save_provider("claude", &edited).unwrap();
        sync::after_provider_edit(state, &AppType::Claude, Some("a"));
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a2");
    });
}

#[test]
#[serial]
fn managed_login_cards_cannot_be_enabled_in_wsl() {
    with_home(|state, home| {
        seed_claude(state);
        let mut official = Provider::with_id(
            "official".into(),
            "Claude Official".into(),
            json!({ "env": {} }),
            None,
        );
        official.category = Some("official".into());
        official.meta = Some(crate::provider::ProviderMeta {
            auth_binding: Some(crate::provider::AuthBinding {
                source: crate::provider::AuthBindingSource::ManagedAccount,
                auth_provider: Some("claude_official".into()),
                account_id: Some("acct-1".into()),
            }),
            ..Default::default()
        });
        assert!(sync::is_managed_login(&AppType::Claude, &official));
        state.db.save_provider("claude", &official).unwrap();

        let wsl_dir = home.join("wsl").join(".claude");
        set_targets(&AppType::Claude, windows_plus_wsl("Ubuntu", &wsl_dir));
        activate(state, "claude", "wsl:Ubuntu", "a");
        let before = fs::read_to_string(wsl_dir.join("settings.json")).unwrap();

        let err = service::activate(state, "claude", "wsl:Ubuntu", "official")
            .expect_err("managed login is Windows-only");
        assert!(err.to_string().contains("Windows"), "{err}");
        assert_eq!(
            fs::read_to_string(wsl_dir.join("settings.json")).unwrap(),
            before
        );
        assert_eq!(
            status::get("claude", "wsl:Ubuntu")
                .unwrap()
                .provider_id
                .as_deref(),
            Some("a")
        );
    });
}

#[test]
#[serial]
fn codex_follower_keeps_its_own_login_when_switching_to_a_key() {
    with_home(|state, home| {
        let codex = |id: &str, base: &str, key: &str| {
            Provider::with_id(
                id.into(),
                id.into(),
                json!({
                    "auth": { "OPENAI_API_KEY": key },
                    "config": format!(
                        "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"{base}\"\nwire_api = \"responses\"\n"
                    )
                }),
                None,
            )
        };
        state
            .db
            .save_provider("codex", &codex("a", "https://a.example/v1", "sk-a"))
            .unwrap();
        state
            .db
            .save_provider("codex", &codex("b", "https://b.example/v1", "sk-b"))
            .unwrap();
        ProviderService::switch(state, AppType::Codex, "a").expect("initial");

        let wsl_dir = home.join("wsl").join(".codex");
        let login = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": { "id_token": "id", "access_token": "at", "refresh_token": "rt", "account_id": "acct" },
            "last_refresh": "2026-11-01T00:00:00Z"
        });
        write(&wsl_dir.join("auth.json"), &login.to_string());
        write(
            &wsl_dir.join("config.toml"),
            "model = \"gpt-5\"\n[profiles.mine]\nmodel = \"o3\"\n",
        );
        set_targets(&AppType::Codex, windows_plus_wsl("Ubuntu", &wsl_dir));

        let report = activate(state, "codex", "wsl:Ubuntu", "b");
        assert_eq!(report.record.status, TargetStatus::Ok, "{report:?}");
        let config = fs::read_to_string(wsl_dir.join("config.toml")).unwrap();
        assert!(config.contains("https://b.example/v1"), "{config}");
        assert!(
            config.contains("[profiles.mine]"),
            "unmanaged table kept: {config}"
        );
        let auth = read_json(&wsl_dir.join("auth.json"));
        assert_eq!(
            auth["tokens"]["refresh_token"], "rt",
            "WSL login kept: {auth}"
        );
    });
}

#[test]
#[serial]
fn gemini_and_opencode_followers_are_written() {
    with_home(|state, home| {
        let gemini = |id: &str, key: &str| {
            Provider::with_id(
                id.into(),
                id.into(),
                json!({ "env": { "GEMINI_API_KEY": key, "GOOGLE_GEMINI_BASE_URL": "https://g.example", "GEMINI_MODEL": "gemini-3" }, "config": {} }),
                None,
            )
        };
        state
            .db
            .save_provider("gemini", &gemini("a", "ga"))
            .unwrap();
        state
            .db
            .save_provider("gemini", &gemini("b", "gb"))
            .unwrap();
        ProviderService::switch(state, AppType::Gemini, "a").expect("initial");
        let gemini_dir = home.join("wsl").join(".gemini");
        set_targets(&AppType::Gemini, windows_plus_wsl("Ubuntu", &gemini_dir));
        let report = activate(state, "gemini", "wsl:Ubuntu", "b");
        assert_eq!(report.record.status, TargetStatus::Ok);
        let env = fs::read_to_string(gemini_dir.join(".env")).unwrap();
        assert!(env.contains("GEMINI_API_KEY=gb"), "{env}");

        let opencode_dir = home.join("wsl").join(".config").join("opencode");
        set_targets(
            &AppType::OpenCode,
            windows_plus_wsl("Ubuntu", &opencode_dir),
        );
        let provider = Provider::with_id(
            "relay".into(),
            "Relay".into(),
            json!({
                "npm": "@ai-sdk/openai-compatible",
                "options": { "baseURL": "https://relay.example/v1", "apiKey": "sk-r" },
                "models": { "m1": { "name": "M1" } }
            }),
            None,
        );
        ProviderService::add(state, AppType::OpenCode, provider, true).expect("add");
        sync::after_provider_edit(state, &AppType::OpenCode, Some("relay"));
        let config = read_json(&opencode_dir.join("opencode.json"));
        assert_eq!(
            config["provider"]["relay"]["options"]["baseURL"],
            "https://relay.example/v1"
        );
        assert_eq!(
            status::get("opencode", "wsl:Ubuntu").unwrap().status,
            TargetStatus::Ok
        );
    });
}

#[test]
#[serial]
fn wsl_followers_skip_windows_only_mcp_commands() {
    with_home(|state, home| {
        seed_claude(state);
        let apps = McpApps {
            claude: true,
            ..Default::default()
        };
        for (id, spec) in [
            (
                "portable",
                json!({ "command": "npx", "args": ["-y", "@scope/server"] }),
            ),
            ("win-only", json!({ "command": "C:\\tools\\mcp.exe" })),
        ] {
            state
                .db
                .save_mcp_server(&McpServer {
                    id: id.into(),
                    name: id.into(),
                    server: spec,
                    apps: apps.clone(),
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                })
                .unwrap();
        }
        let wsl_dir = home.join("wsl").join(".claude");
        fs::create_dir_all(&wsl_dir).unwrap();
        set_targets(&AppType::Claude, windows_plus_wsl("Ubuntu", &wsl_dir));
        let reports = sync::sync_followers(state, &AppType::Claude, None, SyncKind::Full);
        // 这个环境还没选供应商：只同步共享的 MCP，标「待选择」。
        assert_eq!(
            reports[0].record.status,
            TargetStatus::Deferred,
            "{reports:?}"
        );
        assert_eq!(reports[0].record.reason.as_deref(), Some("no_provider"));
        assert!(reports[0]
            .record
            .warnings
            .contains(&"mcp_skipped_windows_only:win-only".to_string()));
        let mcp = read_json(&wsl_dir.join(".claude.json"));
        assert!(mcp["mcpServers"].get("portable").is_some(), "{mcp}");
        assert!(mcp["mcpServers"].get("win-only").is_none(), "{mcp}");
    });
}

#[test]
#[serial]
fn conflicts_are_reported_and_import_deduplicates() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        write(
            &wsl_dir.join("settings.json"),
            r#"{ "env": { "ANTHROPIC_BASE_URL": "https://other.example/v1", "ANTHROPIC_AUTH_TOKEN": "sk-other" } }"#,
        );
        let spec = super::model::TargetSpec {
            id: TargetId::Wsl("Ubuntu".into()),
            enabled: true,
            route: false,
            dir: Some(wsl_dir.to_string_lossy().into_owned()),
        };
        let conflict = sync::inspect_target(state, &AppType::Claude, &spec, Some(wsl_dir.clone()))
            .unwrap()
            .expect("conflict");
        assert_eq!(conflict.existing_host.as_deref(), Some("other.example"));
        assert_eq!(conflict.existing_provider_id, None);
        assert_eq!(conflict.incoming_provider_id.as_deref(), Some("a"));
        let serialized = serde_json::to_string(&conflict).unwrap();
        assert!(
            !serialized.contains("sk-other"),
            "no credentials: {serialized}"
        );

        let first = sync::import_target(state, &AppType::Claude, &spec, Some(wsl_dir.clone()))
            .unwrap()
            .expect("imported");
        let second = sync::import_target(state, &AppType::Claude, &spec, Some(wsl_dir.clone()))
            .unwrap()
            .expect("matched");
        assert_eq!(first, second, "import deduplicates");
        let conflict = sync::inspect_target(state, &AppType::Claude, &spec, Some(wsl_dir.clone()))
            .unwrap()
            .expect("still a conflict");
        assert_eq!(
            conflict.existing_provider_id.as_deref(),
            Some(first.as_str())
        );

        // 目标里就是当前这家：没有冲突。
        write(
            &wsl_dir.join("settings.json"),
            r#"{ "env": { "ANTHROPIC_BASE_URL": "https://a.example/api", "ANTHROPIC_AUTH_TOKEN": "sk-a" } }"#,
        );
        assert!(
            sync::inspect_target(state, &AppType::Claude, &spec, Some(wsl_dir))
                .unwrap()
                .is_none()
        );
    });
}

fn path_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
#[serial]
fn save_flow_resolves_conflicts_and_moves_the_primary() {
    with_home(|state, home| {
        seed_claude(state);
        let ubuntu: PathBuf = home.join("ubuntu").join(".claude");
        let debian: PathBuf = home.join("debian").join(".claude");
        let foreign = r#"{ "env": { "ANTHROPIC_BASE_URL": "https://other.example", "ANTHROPIC_AUTH_TOKEN": "sk-o" }, "includeCoAuthoredBy": false }"#;
        write(&ubuntu.join("settings.json"), foreign);
        write(&debian.join("settings.json"), foreign);

        let mut draft = windows_plus_wsl("Ubuntu", &ubuntu);
        draft.wsl.push(WslTarget {
            distro: "Debian".into(),
            enabled: false,
            route: false,
            dir: Some(path_of(&debian)),
        });

        // 1. 有冲突、没给处理方式：什么都不保存。
        let result = service::save(state, "claude", draft.clone(), BTreeMap::new()).unwrap();
        assert!(!result.saved);
        assert!(result.needs_resolution);
        assert_eq!(result.inspect.conflicts.len(), 1);
        assert!(crate::settings::get_settings().app_targets.is_none());

        // 2. 暂不写入：保存目标，标待同步，文件不动。
        let resolutions = BTreeMap::from([("wsl:Ubuntu".to_string(), Resolution::Defer)]);
        let result = service::save(state, "claude", draft.clone(), resolutions).unwrap();
        assert!(result.saved, "{result:?}");
        assert_eq!(result.targets[0].record.status, TargetStatus::Deferred);
        assert_eq!(
            result.targets[0].record.reason.as_deref(),
            Some("user_deferred")
        );
        assert_eq!(
            fs::read_to_string(ubuntu.join("settings.json")).unwrap(),
            foreign
        );

        // 3. 只改未勾选目标的目录：保存草稿，不触发写入。
        let mut only_disabled = draft.clone();
        let moved = home.join("debian2").join(".claude");
        only_disabled.wsl[1].dir = Some(path_of(&moved));
        let result =
            service::save(state, "claude", only_disabled.clone(), BTreeMap::new()).unwrap();
        assert!(result.saved);
        assert!(
            result.primary.is_none() && result.targets.is_empty(),
            "{result:?}"
        );

        // 4. 新勾选 Debian 并备份后同步：只更新托管字段。
        let mut with_debian = only_disabled.clone();
        with_debian.wsl[1].dir = Some(path_of(&debian));
        with_debian.wsl[1].enabled = true;
        let resolutions = BTreeMap::from([("wsl:Debian".to_string(), Resolution::Overwrite)]);
        let result = service::save(state, "claude", with_debian.clone(), resolutions).unwrap();
        assert!(result.saved, "{result:?}");
        assert_eq!(result.targets[0].target, "wsl:Debian");
        assert_eq!(result.targets[0].record.status, TargetStatus::Ok);
        let written = read_json(&debian.join("settings.json"));
        assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
        assert_eq!(written["includeCoAuthoredBy"], false);
        let backups = home
            .join(".cc-switch")
            .join("targets")
            .join("claude")
            .join("wsl-debian")
            .join("backups");
        let backup_dirs: Vec<_> = fs::read_dir(&backups)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name() != "live-first-write")
            .collect();
        assert_eq!(backup_dirs.len(), 1);
        assert_eq!(
            fs::read_to_string(backup_dirs[0].path().join("settings.json")).unwrap(),
            foreign
        );

        // 5. 取消 Windows：主环境自动落到 Ubuntu。它里面原有的配置要先问；选导入就直接
        //    用导入的卡，旧单目录投影指向它。Debian 继续用自己的那家。
        let mut ubuntu_primary = with_debian.clone();
        ubuntu_primary.windows.enabled = false;
        let result =
            service::save(state, "claude", ubuntu_primary.clone(), BTreeMap::new()).unwrap();
        assert!(result.needs_resolution, "{result:?}");
        assert_eq!(result.inspect.primary_target, "wsl:Ubuntu");
        assert_eq!(result.inspect.conflicts[0].target, "wsl:Ubuntu");
        let resolutions = BTreeMap::from([("wsl:Ubuntu".to_string(), Resolution::Import)]);
        let result = service::save(state, "claude", ubuntu_primary, resolutions).unwrap();
        assert!(result.saved, "{result:?}");
        let primary = result.primary.as_ref().unwrap();
        assert_eq!(primary.record.status, TargetStatus::Ok, "{primary:?}");
        let imported = primary.record.provider_id.clone().unwrap();
        assert_eq!(result.imported, vec![imported.clone()]);
        assert_eq!(
            crate::mode::current::provider_for(
                &state.db,
                &AppType::Claude,
                crate::mode::current::Purpose::Direct
            )
            .unwrap()
            .as_deref(),
            Some(imported.as_str())
        );
        assert_eq!(
            crate::settings::get_settings().claude_config_dir.as_deref(),
            Some(path_of(&ubuntu).as_str())
        );
        let written = read_json(&ubuntu.join("settings.json"));
        assert_eq!(
            written["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-o",
            "own config kept"
        );
        let written = read_json(&debian.join("settings.json"));
        assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

        // 6. 取消最后一个目标：校验拒绝，设置不变。
        let mut none = with_debian;
        none.windows.enabled = false;
        none.wsl.iter_mut().for_each(|t| t.enabled = false);
        let result = service::save(state, "claude", none, BTreeMap::new()).unwrap();
        assert!(!result.saved);
        assert!(result.inspect.errors.iter().any(|e| e.code == "noTarget"));
    });
}

#[test]
#[serial]
fn external_changes_are_detected_without_overwriting() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        let config = windows_plus_wsl("Ubuntu", &wsl_dir);
        set_targets(&AppType::Claude, config.clone());
        activate(state, "claude", "wsl:Ubuntu", "b");

        write(
            &wsl_dir.join("settings.json"),
            r#"{ "env": { "ANTHROPIC_BASE_URL": "https://hand-edited.example", "ANTHROPIC_AUTH_TOKEN": "sk-b" } }"#,
        );
        sync::refresh_external(&AppType::Claude, &config, &|_| true);
        let record = status::get("claude", "wsl:Ubuntu").unwrap();
        assert_eq!(record.status, TargetStatus::External);
        assert_eq!(
            read_json(&wsl_dir.join("settings.json"))["env"]["ANTHROPIC_BASE_URL"],
            "https://hand-edited.example",
            "detection does not write"
        );

        // 立即同步：按当前选择写回托管字段。
        let synced = service::sync_now(state, "claude", None).unwrap();
        assert_eq!(synced.targets[0].record.status, TargetStatus::Ok);
        assert_eq!(
            read_json(&wsl_dir.join("settings.json"))["env"]["ANTHROPIC_BASE_URL"],
            "https://b.example/api"
        );
    });
}

#[test]
#[serial]
fn cancelled_targets_are_never_written() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        let mut config = windows_plus_wsl("Ubuntu", &wsl_dir);
        config.wsl[0].enabled = false;
        set_targets(&AppType::Claude, config);
        let synced = service::sync_now(state, "claude", Some(vec!["wsl:Ubuntu".into()])).unwrap();
        assert!(synced.targets.is_empty());
        ProviderService::switch(state, AppType::Claude, "b").expect("switch");
        assert!(!wsl_dir.join("settings.json").exists());
    });
}

#[test]
#[serial]
fn legacy_single_directory_is_unchanged_without_saved_targets() {
    with_home(|state, home| {
        seed_claude(state);
        let legacy = home.join("custom").join(".claude");
        let mut settings = crate::settings::get_settings();
        settings.claude_config_dir = Some(path_of(&legacy));
        crate::settings::update_settings(settings).unwrap();

        let result = ProviderService::switch(state, AppType::Claude, "b").expect("switch");
        assert!(result.targets.is_empty());
        let written = read_json(&legacy.join("settings.json"));
        assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");
        assert!(!sync::has_followers(&AppType::Claude));
        assert!(
            crate::settings::get_settings().app_targets.is_none(),
            "no write on read"
        );
    });
}

#[test]
#[serial]
fn wsl_targets_join_the_shared_route_and_fall_back_to_their_own_direct_provider() {
    with_home(|state, home| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(
            state
                .db
                .update_proxy_config(crate::proxy::types::ProxyConfig {
                    listen_port: 0,
                    ..Default::default()
                }),
        )
        .unwrap();
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        let mut config = windows_plus_wsl("Ubuntu", &wsl_dir);
        config.wsl[0].route = true;
        set_targets(&AppType::Claude, config);
        crate::settings::set_proxy_wsl_client_host("Ubuntu", "172.24.0.1").unwrap();
        activate(state, "claude", "wsl:Ubuntu", "b");

        // 直连模式下勾了接入也不写代理。
        assert!(sync::sync_follower_routes(state, &AppType::Claude, false).is_empty());

        rt.block_on(crate::mode::controller::enter(
            state,
            &AppType::Claude,
            false,
        ))
        .expect("enter");
        let reports = sync::sync_follower_routes(state, &AppType::Claude, false);
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].record.status, TargetStatus::Ok, "{reports:?}");
        assert!(reports[0].record.routed);
        assert!(reports[0]
            .record
            .warnings
            .contains(&"listen_loopback".to_string()));
        let follower = read_json(&wsl_dir.join("settings.json"));
        let base = follower["env"]["ANTHROPIC_BASE_URL"].as_str().unwrap();
        assert!(base.starts_with("http://172.24.0.1:"), "{base}");
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert!(primary["env"]["ANTHROPIC_BASE_URL"]
            .as_str()
            .unwrap()
            .starts_with("http://127.0.0.1:"));
        assert!(sync::follower_routed(
            &AppType::Claude,
            &TargetId::Wsl("Ubuntu".into())
        ));
        // 跟随环境的模式不改应用级的路由开关。
        assert!(state.db.get_proxy_flags_sync("claude").0);

        // 接在路由上时换直连供应商：只记下，客户端仍是代理契约。
        let report = activate(state, "claude", "wsl:Ubuntu", "a");
        assert_eq!(report.record.reason.as_deref(), Some("routed"));
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(
            follower["env"]["ANTHROPIC_BASE_URL"].as_str().unwrap(),
            base
        );

        // 不接入：写回它自己的直连供应商（刚选的 a）。
        let report = service::set_route(state, "claude", "wsl:Ubuntu", false).unwrap();
        assert!(!report.record.routed, "{report:?}");
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
        assert_eq!(
            follower["env"]["ANTHROPIC_BASE_URL"],
            "https://a.example/api"
        );

        // 再接入，然后应用退出路由：每个环境回到自己的直连供应商。
        let report = service::set_route(state, "claude", "wsl:Ubuntu", true).unwrap();
        assert!(report.record.routed, "{report:?}");
        rt.block_on(crate::mode::controller::exit(state, &AppType::Claude))
            .expect("exit");
        sync::sync_follower_routes(state, &AppType::Claude, false);
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert_eq!(primary["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
        assert!(!sync::follower_routed(
            &AppType::Claude,
            &TargetId::Wsl("Ubuntu".into())
        ));

        // 退出 CC Switch 时的分离：接着路由的环境也指回直连。
        rt.block_on(crate::mode::controller::enter(
            state,
            &AppType::Claude,
            false,
        ))
        .expect("enter again");
        sync::sync_follower_routes(state, &AppType::Claude, false);
        assert!(sync::follower_routed(
            &AppType::Claude,
            &TargetId::Wsl("Ubuntu".into())
        ));
        let reports = sync::sync_follower_routes(state, &AppType::Claude, true);
        assert!(!reports[0].record.routed, "{reports:?}");
        let follower = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(follower["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

        rt.block_on(crate::mode::controller::exit(state, &AppType::Claude))
            .expect("exit");
        // 后台跟随同步线程可能还在跑：等它们落定再结束（临时目录随后删除）。
        std::thread::sleep(std::time::Duration::from_millis(300));
    });
}

#[test]
fn client_hosts_are_validated() {
    assert_eq!(
        service::validate_client_host(" 172.24.0.1 ").unwrap(),
        "172.24.0.1"
    );
    assert_eq!(
        service::validate_client_host("[fe80::1]").unwrap(),
        "fe80::1"
    );
    assert_eq!(
        service::validate_client_host("win-host.local").unwrap(),
        "win-host.local"
    );
    assert_eq!(service::validate_client_host("").unwrap(), "");
    for bad in ["0.0.0.0", "::", "http://x", "x:8080", "a/b", "-bad", "a..b"] {
        assert!(service::validate_client_host(bad).is_err(), "{bad}");
    }
    let mut env = super::model::LinuxEnv {
        gateway: Some("172.24.0.1".into()),
        networking_mode: Some("nat".into()),
        ..Default::default()
    };
    assert_eq!(
        sync::detected_client_host(&env).as_deref(),
        Some("172.24.0.1")
    );
    env.networking_mode = Some("mirrored".into());
    assert_eq!(
        sync::detected_client_host(&env).as_deref(),
        Some("127.0.0.1")
    );
}

#[test]
#[serial]
fn codex_wsl_target_joins_the_route_with_its_own_host() {
    with_home(|state, home| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(
            state
                .db
                .update_proxy_config(crate::proxy::types::ProxyConfig {
                    listen_port: 0,
                    ..Default::default()
                }),
        )
        .unwrap();
        let codex = |id: &str, base: &str, key: &str| {
            Provider::with_id(
                id.into(),
                id.into(),
                json!({
                    "auth": { "OPENAI_API_KEY": key },
                    "config": format!(
                        "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"{base}\"\nwire_api = \"responses\"\n"
                    )
                }),
                None,
            )
        };
        state
            .db
            .save_provider("codex", &codex("a", "https://a.example/v1", "sk-a"))
            .unwrap();
        state
            .db
            .save_provider("codex", &codex("b", "https://b.example/v1", "sk-b"))
            .unwrap();
        ProviderService::switch(state, AppType::Codex, "a").expect("initial");
        let wsl_dir = home.join("wsl").join(".codex");
        let mut config = windows_plus_wsl("Ubuntu", &wsl_dir);
        config.wsl[0].route = true;
        set_targets(&AppType::Codex, config);
        crate::settings::set_proxy_wsl_client_host("Ubuntu", "172.24.0.1").unwrap();
        activate(state, "codex", "wsl:Ubuntu", "b");

        rt.block_on(crate::mode::controller::enter(
            state,
            &AppType::Codex,
            false,
        ))
        .expect("enter");
        let reports = sync::sync_follower_routes(state, &AppType::Codex, false);
        assert_eq!(reports[0].record.status, TargetStatus::Ok, "{reports:?}");
        let text = fs::read_to_string(wsl_dir.join("config.toml")).unwrap();
        assert!(text.contains("http://172.24.0.1:"), "{text}");
        assert!(!text.contains("https://b.example/v1"), "{text}");

        rt.block_on(crate::mode::controller::exit(state, &AppType::Codex))
            .expect("exit");
        sync::sync_follower_routes(state, &AppType::Codex, false);
        let text = fs::read_to_string(wsl_dir.join("config.toml")).unwrap();
        assert!(
            text.contains("https://b.example/v1"),
            "own direct provider back: {text}"
        );
        assert!(!text.contains("172.24.0.1"), "{text}");
        std::thread::sleep(std::time::Duration::from_millis(300));
    });
}

#[test]
#[serial]
fn codex_official_card_uses_the_wsl_environments_own_login() {
    with_home(|state, home| {
        state
            .db
            .ensure_official_seed_by_id(crate::database::CODEX_OFFICIAL_PROVIDER_ID, AppType::Codex)
            .unwrap();
        let official_id = crate::database::CODEX_OFFICIAL_PROVIDER_ID;
        let official = state
            .db
            .get_provider_by_id(official_id, "codex")
            .unwrap()
            .unwrap();
        assert!(!sync::is_managed_login(&AppType::Codex, &official));
        let relay = Provider::with_id(
            "relay".into(),
            "Relay".into(),
            json!({
                "auth": { "OPENAI_API_KEY": "sk-r" },
                "config": "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://r.example/v1\"\nwire_api = \"responses\"\n"
            }),
            None,
        );
        state.db.save_provider("codex", &relay).unwrap();
        ProviderService::switch(state, AppType::Codex, "relay").expect("initial");

        let wsl_dir = home.join("wsl").join(".codex");
        let login = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": { "id_token": "id", "access_token": "at", "refresh_token": "rt", "account_id": "acct" },
            "last_refresh": "2026-11-01T00:00:00Z"
        });
        write(&wsl_dir.join("auth.json"), &login.to_string());
        set_targets(&AppType::Codex, windows_plus_wsl("Ubuntu", &wsl_dir));
        activate(state, "codex", "wsl:Ubuntu", "relay");
        let report = activate(state, "codex", "wsl:Ubuntu", official_id);
        assert_eq!(report.record.status, TargetStatus::Ok, "{report:?}");
        let config = fs::read_to_string(wsl_dir.join("config.toml")).unwrap();
        assert!(!config.contains("r.example"), "{config}");
        let auth = read_json(&wsl_dir.join("auth.json"));
        assert_eq!(auth["tokens"]["refresh_token"], "rt", "{auth}");
        // Windows 主环境不受影响。
        let main = fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert!(main.contains("r.example"), "{main}");
    });
}

#[test]
#[serial]
fn enabling_a_provider_in_a_new_wsl_environment_adds_it_and_backs_up_first() {
    with_home(|state, home| {
        seed_claude(state);
        let wsl_dir = home.join("wsl").join(".claude");
        let foreign = r#"{ "env": { "ANTHROPIC_BASE_URL": "https://own.example", "ANTHROPIC_AUTH_TOKEN": "sk-own" } }"#;
        write(&wsl_dir.join("settings.json"), foreign);
        let mut config = windows_plus_wsl("Ubuntu", &wsl_dir);
        config.wsl[0].enabled = false;
        set_targets(&AppType::Claude, config);
        assert!(!sync::has_followers(&AppType::Claude));

        let report = activate(state, "claude", "wsl:Ubuntu", "b");
        assert_eq!(report.record.status, TargetStatus::Ok, "{report:?}");
        assert!(sync::has_followers(&AppType::Claude));
        let written = read_json(&wsl_dir.join("settings.json"));
        assert_eq!(written["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");
        let backups = home
            .join(".cc-switch")
            .join("targets")
            .join("claude")
            .join("wsl-ubuntu")
            .join("backups");
        let saved: Vec<_> = fs::read_dir(&backups)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name() != "live-first-write")
            .collect();
        assert_eq!(saved.len(), 1);
        assert_eq!(
            fs::read_to_string(saved[0].path().join("settings.json")).unwrap(),
            foreign
        );
        // Windows 不受影响。
        let primary = read_json(&home.join(".claude").join("settings.json"));
        assert_eq!(primary["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");
    });
}
