use serde_json::json;
use serial_test::serial;
use std::ffi::OsString;
use std::fs;
use tempfile::TempDir;

struct TempHome {
    #[allow(dead_code)]
    dir: TempDir,
    original_home: Option<OsString>,
    original_userprofile: Option<OsString>,
    original_test_home: Option<OsString>,
    original_pi_dir: Option<OsString>,
}

impl TempHome {
    fn new() -> Self {
        let dir = TempDir::new().expect("failed to create temp home");
        let original_home = std::env::var_os("HOME");
        let original_userprofile = std::env::var_os("USERPROFILE");
        let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
        let original_pi_dir = std::env::var_os("PI_CODING_AGENT_DIR");

        std::env::set_var("HOME", dir.path());
        std::env::set_var("USERPROFILE", dir.path());
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        std::env::remove_var("PI_CODING_AGENT_DIR");

        Self {
            dir,
            original_home,
            original_userprofile,
            original_test_home,
            original_pi_dir,
        }
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        match &self.original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match &self.original_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match &self.original_test_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match &self.original_pi_dir {
            Some(value) => std::env::set_var("PI_CODING_AGENT_DIR", value),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }
}

#[test]
#[serial]
fn writes_provider_and_defaults_without_clobbering_existing_pi_config() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(
        pi_dir.join("models.json"),
        r#"{
  "providers": {
    "existing": {
      "baseURL": "https://existing.example/v1",
      "apiKey": "keep",
      "models": [{ "id": "existing-model" }]
    }
  },
  "metadata": {
    "keep": true
  }
}"#,
    )
    .expect("seed models.json");
    fs::write(
        pi_dir.join("settings.json"),
        r#"{
  "defaultProvider": "existing",
  "defaultModel": "existing-model",
  "theme": "dark"
}"#,
    )
    .expect("seed settings.json");

    let provider = cc_switch_lib::Provider::with_id(
        "packy".to_string(),
        "Packy".to_string(),
        json!({
            "baseUrl": "https://api.packy.example/v1",
            "apiKey": "sk-packy",
            "api": "openai-completions",
            "models": [
                {
                    "id": "gpt-5.5",
                    "name": "GPT 5.5",
                    "contextWindow": 400000
                },
                { "id": "gpt-5.5-mini", "name": "GPT 5.5 Mini" }
            ],
            "defaultModel": "gpt-5.5-mini"
        }),
        None,
    );

    cc_switch_lib::pi_config::write_pi_live_provider(&provider).expect("write pi live");

    let models: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("models.json")).unwrap()).unwrap();
    assert_eq!(
        models.pointer("/providers/existing/apiKey"),
        Some(&json!("keep")),
        "existing provider should be preserved"
    );
    assert_eq!(models.pointer("/metadata/keep"), Some(&json!(true)));
    assert_eq!(
        models.pointer("/providers/packy/baseUrl"),
        Some(&json!("https://api.packy.example/v1"))
    );
    assert!(
        models.pointer("/providers/packy/baseURL").is_none(),
        "new Pi providers should use the canonical baseUrl key"
    );
    assert_eq!(
        models.pointer("/providers/packy/models/1/id"),
        Some(&json!("gpt-5.5-mini")),
        "Pi provider model ids should be preserved"
    );
    assert_eq!(
        models.pointer("/providers/packy/models/0/name"),
        Some(&json!("GPT 5.5")),
        "Pi provider model display names should be preserved"
    );
    assert_eq!(
        models.pointer("/providers/packy/models/0/contextWindow"),
        Some(&json!(400000)),
        "Pi provider model metadata should be preserved"
    );
    assert!(
        models.pointer("/providers/packy/defaultModel").is_none(),
        "defaultModel is a settings.json concern and should not be written into provider config"
    );
    assert!(
        models.pointer("/providers/packy/apiKey").is_none(),
        "managed API keys must not be written inline"
    );

    let auth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(
        auth.pointer("/packy"),
        Some(&json!({ "type": "api_key", "key": "sk-packy" }))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(pi_dir.join("auth.json"))
                .expect("auth metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600,
            "new auth.json must be private"
        );
    }

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        settings.pointer("/defaultProvider"),
        Some(&json!("packy")),
        "switch should update Pi default provider"
    );
    assert_eq!(
        settings.pointer("/defaultModel"),
        Some(&json!("gpt-5.5-mini")),
        "switch should use explicit defaultModel"
    );
    assert_eq!(
        settings.pointer("/theme"),
        Some(&json!("dark")),
        "unrelated settings should be preserved"
    );

    let backup_root = home
        .dir
        .path()
        .join(".cc-switch")
        .join("backups")
        .join("pi");
    let backup_dirs = fs::read_dir(&backup_root)
        .expect("read Pi backups")
        .map(|entry| entry.expect("read Pi backup entry").path())
        .collect::<Vec<_>>();
    assert_eq!(backup_dirs.len(), 1, "one activation creates one backup");
    let backup_models: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(backup_dirs[0].join("models.json")).expect("read backed-up models"),
    )
    .expect("parse backed-up models");
    let backup_settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(backup_dirs[0].join("settings.json")).expect("read backed-up settings"),
    )
    .expect("parse backed-up settings");
    assert_eq!(
        backup_models.pointer("/providers/existing/apiKey"),
        Some(&json!("keep"))
    );
    assert_eq!(
        backup_settings.pointer("/defaultProvider"),
        Some(&json!("existing"))
    );

    cc_switch_lib::pi_config::write_pi_live_provider(&provider).expect("repeat no-op write");
    assert_eq!(
        fs::read_dir(&backup_root)
            .expect("re-read Pi backups")
            .count(),
        1,
        "a no-op activation must not create another backup"
    );
}

#[test]
#[serial]
fn read_live_settings_returns_current_provider_fragment() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(
        pi_dir.join("models.json"),
        r#"{
  "providers": {
    "packy": {
      "baseURL": "https://api.packy.example/v1",
      "apiKey": "sk-packy",
      "api": "openai-completions",
      "models": [{ "id": "gpt-5.5" }]
    }
  }
}"#,
    )
    .expect("seed models.json");
    fs::write(
        pi_dir.join("settings.json"),
        r#"{
  "defaultProvider": "packy",
  "defaultModel": "gpt-5.5"
}"#,
    )
    .expect("seed settings.json");

    let settings = cc_switch_lib::pi_config::read_pi_live_settings().expect("read pi live");

    assert_eq!(settings["defaultProvider"], json!("packy"));
    assert_eq!(settings["defaultModel"], json!("gpt-5.5"));
    assert_eq!(
        settings.pointer("/providerConfig/baseUrl"),
        Some(&json!("https://api.packy.example/v1"))
    );
    assert_eq!(
        settings.pointer("/providerConfig/models/0/id"),
        Some(&json!("gpt-5.5")),
        "model ids should retain the CC Switch form shape"
    );
    assert_eq!(
        settings.pointer("/providerConfig/apiKey"),
        Some(&json!("sk-packy")),
        "legacy inline keys remain readable until a managed update migrates them"
    );
}

#[test]
#[serial]
fn managed_update_migrates_a_legacy_inline_key_to_auth_json() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(
        pi_dir.join("models.json"),
        r#"// keep this file comment
{
  "providers": {
    "packy": {
      "baseUrl": "https://api.packy.example/v1",
      "apiKey": "legacy-inline",
      "api": "openai-completions",
      "models": [{ "id": "gpt-5.5" }],
    },
  },
}
"#,
    )
    .expect("seed models");
    fs::write(
        pi_dir.join("settings.json"),
        r#"{"defaultProvider":"packy","defaultModel":"gpt-5.5"}"#,
    )
    .expect("seed settings");

    let live = cc_switch_lib::pi_config::read_pi_live_settings().expect("read legacy provider");
    let provider = cc_switch_lib::Provider::with_id(
        "packy".to_string(),
        "Packy".to_string(),
        live["providerConfig"].clone(),
        None,
    );
    cc_switch_lib::pi_config::upsert_pi_live_provider(&provider, true).expect("migrate legacy key");

    let models_text = fs::read_to_string(pi_dir.join("models.json")).expect("read models");
    assert!(models_text.contains("// keep this file comment"));
    let models = cc_switch_lib::pi_config::read_pi_live_settings().expect("re-read provider");
    assert_eq!(
        models.pointer("/providerConfig/apiKey"),
        Some(&json!("legacy-inline")),
        "the migrated auth key remains available to the form"
    );
    let raw_models = fs::read_to_string(pi_dir.join("models.json")).expect("read raw models");
    assert!(
        !raw_models.contains("\"apiKey\""),
        "the managed provider must no longer contain an inline key"
    );
    let auth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(
        auth.pointer("/packy"),
        Some(&json!({ "type": "api_key", "key": "legacy-inline" }))
    );
}

#[test]
#[serial]
fn pi_directory_override_takes_priority_over_environment_and_default() {
    struct ResetSettings;
    impl Drop for ResetSettings {
        fn drop(&mut self) {
            let _ = cc_switch_lib::update_settings(cc_switch_lib::AppSettings::default());
        }
    }

    let home = TempHome::new();
    let _reset_settings = ResetSettings;
    let env_dir = home.dir.path().join("pi-from-env");
    std::env::set_var("PI_CODING_AGENT_DIR", &env_dir);
    assert_eq!(cc_switch_lib::pi_config::get_pi_dir(), env_dir);

    let override_dir = home.dir.path().join("pi-from-settings");
    cc_switch_lib::update_settings(cc_switch_lib::AppSettings {
        pi_config_dir: Some(override_dir.to_string_lossy().to_string()),
        ..Default::default()
    })
    .expect("set Pi directory override");
    assert_eq!(
        cc_switch_lib::pi_config::get_pi_dir(),
        override_dir,
        "the explicit CC Switch directory must win over PI_CODING_AGENT_DIR"
    );

    cc_switch_lib::update_settings(cc_switch_lib::AppSettings::default())
        .expect("clear Pi directory override");
    std::env::remove_var("PI_CODING_AGENT_DIR");
    assert_eq!(
        cc_switch_lib::pi_config::get_pi_dir(),
        home.dir.path().join(".pi").join("agent")
    );
}

#[test]
#[serial]
fn auth_json_api_key_takes_priority_and_oauth_is_never_overwritten() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(
        pi_dir.join("models.json"),
        r#"{
  "providers": {
    "packy": {
      "baseUrl": "https://api.packy.example/v1",
      "apiKey": "legacy-inline",
      "api": "openai-completions",
      "models": [{ "id": "gpt-5.5" }]
    }
  }
}"#,
    )
    .expect("seed models");
    fs::write(
        pi_dir.join("auth.json"),
        r#"{
  "packy": {
    "type": "oauth",
    "accessToken": "keep-oauth"
  }
}"#,
    )
    .expect("seed auth");
    fs::write(
        pi_dir.join("settings.json"),
        r#"{"defaultProvider":"packy","defaultModel":"gpt-5.5"}"#,
    )
    .expect("seed settings");

    let live = cc_switch_lib::pi_config::read_pi_live_settings().expect("read OAuth provider");
    assert_eq!(
        live.pointer("/providerConfig/apiKey"),
        Some(&json!("legacy-inline")),
        "non-api-key auth entries must not be exposed as API keys"
    );
    let mut provider = cc_switch_lib::Provider::with_id(
        "packy".to_string(),
        "Packy".to_string(),
        live["providerConfig"].clone(),
        None,
    );
    provider.settings_config["apiKey"] = json!("new-key");
    let error = cc_switch_lib::pi_config::write_pi_live_provider(&provider)
        .expect_err("OAuth must not be overwritten");
    assert!(error.to_string().contains("will not overwrite"));

    provider.settings_config["apiKey"] = json!("");
    cc_switch_lib::pi_config::write_pi_live_provider(&provider)
        .expect("an empty key preserves OAuth while updating models");
    let auth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("auth.json")).unwrap()).unwrap();
    assert_eq!(
        auth.pointer("/packy/accessToken"),
        Some(&json!("keep-oauth"))
    );
}

#[test]
#[serial]
fn write_provider_clears_stale_default_model_when_no_model_can_be_derived() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(pi_dir.join("models.json"), r#"{"providers":{}}"#).expect("seed models.json");
    fs::write(
        pi_dir.join("settings.json"),
        r#"{
  "defaultProvider": "old",
  "defaultModel": "old-model"
}"#,
    )
    .expect("seed settings.json");

    let provider = cc_switch_lib::Provider::with_id(
        "empty".to_string(),
        "Empty".to_string(),
        json!({
            "baseUrl": "https://api.example.com/v1",
            "apiKey": "sk-empty",
            "api": "openai-completions",
            "models": []
        }),
        None,
    );

    cc_switch_lib::pi_config::write_pi_live_provider(&provider).expect("write pi live");

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(pi_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings.pointer("/defaultProvider"), Some(&json!("empty")));
    assert!(
        settings.pointer("/defaultModel").is_none(),
        "stale defaultModel from the previous provider must not survive"
    );
}

#[test]
#[serial]
fn write_provider_rejects_existing_non_object_pi_files() {
    let home = TempHome::new();
    let pi_dir = home.dir.path().join(".pi").join("agent");
    fs::create_dir_all(&pi_dir).expect("create pi config dir");
    fs::write(pi_dir.join("models.json"), "[]").expect("seed invalid models root");
    fs::write(pi_dir.join("settings.json"), r#"{}"#).expect("seed settings.json");

    let provider = cc_switch_lib::Provider::with_id(
        "packy".to_string(),
        "Packy".to_string(),
        json!({
            "baseUrl": "https://api.packy.example/v1",
            "apiKey": "sk-packy",
            "api": "openai-completions",
            "models": [{ "id": "gpt-5.5" }]
        }),
        None,
    );

    let err = cc_switch_lib::pi_config::write_pi_live_provider(&provider)
        .expect_err("non-object Pi config root should be rejected");
    assert!(
        err.to_string().contains("must be a JSON object"),
        "unexpected error: {err}"
    );

    assert_eq!(
        fs::read_to_string(pi_dir.join("models.json")).unwrap(),
        "[]",
        "invalid-but-existing user file should not be overwritten"
    );
}
