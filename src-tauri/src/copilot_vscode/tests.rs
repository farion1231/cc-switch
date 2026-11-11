use super::*;
use std::cell::Cell;

fn registry() -> Value {
    json!({
        "providers":[{"name":"relay","baseUrl":"https://example.test/api","apiKey":"fixture-key","headers":{"X-Project":"test"}}],
        "models":[{"provider":"relay","id":"friendly","wireModel":"wire-id","modelId":"behavior-only","name":"Friendly",
            "maxContextWindowTokens":128000,"maxOutputTokens":8192,"maxPromptTokens":100000,
            "capabilities":{"supports":{"toolCalls":true,"tool_calls":false,"vision":true,"streaming":false}}}]
    })
}

fn settings(paths: &[&Path]) -> SyncSettings {
    SyncSettings {
        enabled: true,
        targets: paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    }
}

fn read(path: &Path) -> Value {
    json5::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn projects_wire_model_capabilities_limits_and_custom_api_prefix() {
    let groups = project(&registry()).unwrap();
    let group = &groups[0];
    let model = &group["models"][0];
    assert_eq!(
        model["requestHeaders"]["Authorization"],
        "Bearer fixture-key"
    );
    assert_eq!(model["id"], "wire-id");
    assert_eq!(model["name"], "Friendly");
    assert_eq!(model["url"], "https://example.test/api/chat/completions");
    assert_eq!(model["toolCalling"], false);
    assert_eq!(model["vision"], true);
    assert_eq!(model["streaming"], false);
    assert_eq!(model["contextWindow"], 128000);
    assert_eq!(model["maxInputTokens"], 100000);
    assert_eq!(model["maxOutputTokens"], 8192);
    assert_eq!(model["requestHeaders"]["X-Project"], "test");
    assert!(group["ccSwitchCopilotCli"].is_string());
}

#[test]
fn model_id_is_not_the_wire_id_and_unused_providers_are_not_exported() {
    let mut document = registry();
    document["models"][0]
        .as_object_mut()
        .unwrap()
        .remove("wireModel");
    document["providers"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"empty","baseUrl":"https://empty.test"}));
    let groups = project(&document).unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0]["models"][0]["id"], "friendly");
}

#[test]
fn fills_required_vscode_limits_when_the_cli_omits_them() {
    let mut doc = registry();
    doc["models"] = json!([{"id":"minimal","provider":"relay"}]);
    let groups = project(&doc).unwrap();
    let model = &groups[0]["models"][0];
    for field in [
        "id",
        "name",
        "url",
        "toolCalling",
        "vision",
        "maxOutputTokens",
    ] {
        assert!(!model[field].is_null(), "{field}");
    }
    assert_eq!(model["maxInputTokens"], 128000);
    assert_eq!(model["maxOutputTokens"], 8192);
    doc["models"][0]["maxContextWindowTokens"] = json!(8000);
    let groups = project(&doc).unwrap();
    assert_eq!(groups[0]["models"][0]["maxOutputTokens"], 2000);
    assert_eq!(groups[0]["models"][0]["contextWindow"], 8000);
    assert!(groups[0]["models"][0].get("maxInputTokens").is_none());
}

#[test]
fn supports_responses_messages_and_azure_v1_with_explicit_auth() {
    for (provider, api, endpoint, auth) in [
        (
            json!({"name":"relay","baseUrl":"https://relay.test/v1","wireApi":"responses","apiKey":"key"}),
            "responses",
            "https://relay.test/v1/responses",
            "Authorization",
        ),
        (
            json!({"name":"relay","baseUrl":"https://relay.test","type":"anthropic","apiKey":"key"}),
            "messages",
            "https://relay.test/v1/messages",
            "x-api-key",
        ),
        (
            json!({"name":"relay","baseUrl":"https://gateway.test","type":"azure","apiKey":"key"}),
            "chat-completions",
            "https://gateway.test/openai/v1/chat/completions",
            "api-key",
        ),
        (
            json!({"name":"relay","baseUrl":"https://relay.test/v1","type":"anthropic","apiKey":"key","bearerToken":"token"}),
            "messages",
            "https://relay.test/v1/messages",
            "Authorization",
        ),
    ] {
        let mut doc = registry();
        doc["providers"][0] = provider;
        let groups = project(&doc).unwrap();
        assert_eq!(groups[0]["models"][0]["apiType"], api);
        assert_eq!(groups[0]["models"][0]["url"], endpoint);
        assert!(groups[0]["models"][0]["requestHeaders"][auth].is_string());
        if doc["providers"][0].get("bearerToken").is_some() {
            assert_eq!(
                groups[0]["models"][0]["requestHeaders"]["Authorization"],
                "Bearer token"
            );
        }
    }
}

#[test]
fn preserves_custom_auth_and_endpoint_queries() {
    let mut doc = registry();
    doc["providers"][0]["baseUrl"] =
        json!("https://relay.test/custom/chat/completions?tenant=test");
    doc["providers"][0]["headers"] = json!({"authorization":"custom-token"});
    let groups = project(&doc).unwrap();
    assert_eq!(
        groups[0]["models"][0]["url"],
        "https://relay.test/custom/chat/completions?tenant=test"
    );
    assert_eq!(
        groups[0]["models"][0]["requestHeaders"],
        json!({"authorization":"custom-token"})
    );
}

#[test]
fn gateway_headers_do_not_replace_protocol_authentication() {
    for (kind, auth_header, gateway_header, auth_value) in [
        ("openai", "Authorization", "x-api-key", "Bearer fixture-key"),
        ("azure", "api-key", "Authorization", "fixture-key"),
        ("anthropic", "x-api-key", "api-key", "fixture-key"),
    ] {
        let mut document = registry();
        document["providers"][0]["type"] = json!(kind);
        document["providers"][0]["headers"] = json!({(gateway_header): "gateway-key"});
        let groups = project(&document).unwrap();
        let headers = &groups[0]["models"][0]["requestHeaders"];
        assert_eq!(headers[auth_header], auth_value);
        assert_eq!(headers[gateway_header], "gateway-key");
    }
}

#[test]
fn anthropic_preserves_api_key_and_bearer_token_together() {
    let mut document = registry();
    document["providers"][0]["type"] = json!("anthropic");
    document["providers"][0]["bearerToken"] = json!("fixture-bearer");
    let groups = project(&document).unwrap();
    let headers = &groups[0]["models"][0]["requestHeaders"];
    assert_eq!(headers["x-api-key"], "fixture-key");
    assert_eq!(headers["Authorization"], "Bearer fixture-bearer");
}

#[test]
fn custom_anthropic_auth_overrides_only_matching_headers_case_insensitively() {
    for (custom, expected) in [
        (
            json!({"X-Api-Key": "custom-key"}),
            json!({"X-Api-Key": "custom-key", "Authorization": "Bearer fixture-bearer"}),
        ),
        (
            json!({"authorization": "custom-token"}),
            json!({"x-api-key": "fixture-key", "authorization": "custom-token"}),
        ),
        (
            json!({"X-API-Key": "custom-key", "authorization": "custom-token"}),
            json!({"X-API-Key": "custom-key", "authorization": "custom-token"}),
        ),
    ] {
        let mut document = registry();
        document["providers"][0]["type"] = json!("anthropic");
        document["providers"][0]["bearerToken"] = json!("fixture-bearer");
        document["providers"][0]["headers"] = custom;
        let groups = project(&document).unwrap();
        assert_eq!(groups[0]["models"][0]["requestHeaders"], expected);
    }
}

#[test]
fn rejects_settings_that_cannot_be_translated_without_changing_meaning() {
    for (pointer, value) in [
        ("/providers/0/transport", json!("websockets")),
        ("/providers/0/hasBearerTokenProvider", json!(true)),
        ("/providers/0/azure", json!({"apiVersion":"2024-10-21"})),
        (
            "/models/0/systemMessage",
            json!({"mode":"append","content":"fixture"}),
        ),
    ] {
        let mut doc = registry();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        doc.pointer_mut(parent).unwrap()[key] = value;
        assert!(project(&doc).is_err(), "{pointer}");
    }
    let mut doc = registry();
    let mut duplicate = doc["models"][0].clone();
    duplicate["id"] = json!("another-alias");
    doc["models"].as_array_mut().unwrap().push(duplicate);
    assert!(project(&doc).is_err());
}

#[test]
fn adds_updates_and_removes_only_linked_groups_and_preserves_jsonc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chatLanguageModels.json");
    let original = "// user note\r\n[\r\n  {\"name\":\"Manual\",\"vendor\":\"customendpoint\",\"future\":true}, // keep me\r\n]\r\n";
    fs::write(&path, original).unwrap();
    let options = settings(&[&path]);
    apply(&SyncSettings::default(), &options, &registry(), || Ok(())).unwrap();
    let first = fs::read(&path).unwrap();
    assert_eq!(read(&path)[0]["future"], true);
    assert_eq!(read(&path)[1]["models"][0]["id"], "wire-id");
    assert!(String::from_utf8(first.clone())
        .unwrap()
        .contains("// keep me"));
    assert_eq!(
        fs::read(path.with_extension("json.cc-switch-copilot-cli.bak")).unwrap(),
        original.as_bytes()
    );
    apply(&options, &options, &registry(), || Ok(())).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        first,
        "idempotent sync must not rewrite"
    );
    let mut updated = registry();
    updated["models"][0]["wireModel"] = json!("new-model");
    apply(&options, &options, &updated, || Ok(())).unwrap();
    assert_eq!(read(&path)[1]["models"][0]["id"], "new-model");
    apply(&options, &SyncSettings::default(), &json!({}), || Ok(())).unwrap();
    assert_eq!(
        read(&path),
        json!([{"name":"Manual","vendor":"customendpoint","future":true}])
    );
    assert!(fs::read_to_string(path).unwrap().contains("// user note"));
}

#[test]
fn target_change_cleans_old_profile_and_populates_new_profile() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old");
    let new = dir.path().join("new");
    fs::create_dir(&old).unwrap();
    fs::create_dir(&new).unwrap();
    let old = old.join("chatLanguageModels.json");
    let new = new.join("chatLanguageModels.json");
    let previous = settings(&[&old]);
    let next = settings(&[&new]);
    apply(&SyncSettings::default(), &previous, &registry(), || Ok(())).unwrap();
    apply(&previous, &next, &registry(), || Ok(())).unwrap();
    assert_eq!(read(&old), json!([]));
    assert_eq!(read(&new).as_array().unwrap().len(), 1);
}

#[test]
fn retargets_after_a_previous_profile_directory_is_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let old_dir = dir.path().join("removed");
    let new_dir = dir.path().join("current");
    fs::create_dir(&old_dir).unwrap();
    fs::create_dir(&new_dir).unwrap();
    let old = old_dir.join("chatLanguageModels.json");
    let new = new_dir.join("chatLanguageModels.json");
    let previous = settings(&[&old]);
    apply(&SyncSettings::default(), &previous, &registry(), || Ok(())).unwrap();
    fs::remove_file(&old).unwrap();
    fs::remove_dir(&old_dir).unwrap();
    apply(&previous, &settings(&[&new]), &registry(), || Ok(())).unwrap();
    assert!(!old_dir.exists());
    assert_eq!(read(&new)[0]["models"][0]["id"], "wire-id");
}

#[test]
fn validates_all_targets_before_writing_any() {
    let dir = tempfile::tempdir().unwrap();
    let good_dir = dir.path().join("a");
    let bad_dir = dir.path().join("b");
    fs::create_dir(&good_dir).unwrap();
    fs::create_dir(&bad_dir).unwrap();
    let good = good_dir.join("chatLanguageModels.json");
    let bad = bad_dir.join("chatLanguageModels.json");
    fs::write(&good, "[]").unwrap();
    fs::write(&bad, "{ broken").unwrap();
    let committed = Cell::new(false);
    assert!(apply(
        &SyncSettings::default(),
        &settings(&[&good, &bad]),
        &registry(),
        || {
            committed.set(true);
            Ok(())
        }
    )
    .is_err());
    assert!(!committed.get());
    assert_eq!(fs::read(&good).unwrap(), b"[]");
    assert_eq!(
        fs::read_dir(good_dir).unwrap().count(),
        1,
        "staged files are discarded"
    );
}

#[test]
fn rolls_back_existing_and_new_files_when_commit_fails() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a");
    let b = dir.path().join("b");
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let a = a.join("chatLanguageModels.json");
    let b = b.join("chatLanguageModels.json");
    fs::write(&a, "// original\n[]").unwrap();
    let result: Result<(), AppError> = apply(
        &SyncSettings::default(),
        &settings(&[&a, &b]),
        &registry(),
        || Err(AppError::Message("DB failed".into())),
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&a).unwrap(), b"// original\n[]");
    assert!(!b.exists());
}

#[test]
fn rollback_does_not_overwrite_an_external_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chatLanguageModels.json");
    fs::write(&path, "[]").unwrap();
    let result: Result<(), AppError> = apply(
        &SyncSettings::default(),
        &settings(&[&path]),
        &registry(),
        || {
            fs::write(&path, "[{\"name\":\"external\"}]").unwrap();
            Err(AppError::Message("commit failed".into()))
        },
    );
    assert!(result.is_err());
    assert_eq!(read(&path), json!([{"name":"external"}]));
}

#[test]
fn inactive_sync_never_reads_or_creates_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing").join("chatLanguageModels.json");
    let options = SyncSettings {
        enabled: false,
        targets: vec![path.to_string_lossy().to_string()],
    };
    apply(
        &SyncSettings::default(),
        &options,
        &json!("invalid unused CLI registry"),
        || Ok(()),
    )
    .unwrap();
    assert!(!path.parent().unwrap().exists());
    assert!(apply(
        &SyncSettings::default(),
        &SyncSettings {
            enabled: true,
            targets: vec![]
        },
        &registry(),
        || Ok(())
    )
    .is_err());
}

#[test]
fn rejects_wrong_filenames_and_foreign_name_collisions() {
    assert!(target_path("relative/chatLanguageModels.json").is_err());
    let dir = tempfile::tempdir().unwrap();
    assert!(target_path(&dir.path().join("settings.json").to_string_lossy()).is_err());
    let source =
        r#"[{"name":"Copilot CLI · relay","vendor":"customendpoint","apiKey":"user-key"}]"#;
    assert!(merged_source(Some(source.as_bytes()), &project(&registry()).unwrap()).is_err());
}

#[test]
fn retains_comments_inside_foreign_groups_after_a_removed_linked_group() {
    let source = r#"[
      {"name":"old","vendor":"customendpoint","ccSwitchCopilotCli":"relay"},
      {"name":"manual", /* user's credential */ "apiKey":"external-key","vendor":"customendpoint"},
    ]"#;
    let output = merged_source(Some(source.as_bytes()), &[])
        .unwrap()
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(r#"{"name":"manual", /* user's credential */ "apiKey":"external-key","vendor":"customendpoint"}"#));
}

#[test]
#[serial_test::serial]
fn cli_lifecycle_and_settings_share_sync_and_rollback() {
    use crate::copilot_config::{self, test_support::CopilotHomeGuard};
    struct HomeGuard(Option<std::ffi::OsString>);
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            crate::settings::reload_settings().unwrap();
        }
    }
    let root = tempfile::tempdir().unwrap();
    let _home = HomeGuard(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", root.path());
    let _cli = CopilotHomeGuard::new(&root.path().join(".copilot"));
    crate::settings::reload_settings().unwrap();
    assert!(crate::config::get_app_config_dir().starts_with(root.path()));
    let target = root.path().join("chatLanguageModels.json");
    fs::write(&target, r#"[{"name":"external","vendor":"manual"}]"#).unwrap();
    let options = settings(&[&target]);
    let mut local = crate::settings::get_settings();
    local.copilot_vscode = options.clone();
    let document = registry();
    let mut card = json!({"provider":document["providers"][0],"models":document["models"]});
    let commits = Cell::new(0);
    copilot_config::add_and_commit("relay", &card, || {
        commits.set(commits.get() + 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(commits.get(), 1);
    assert_eq!(read(&target).as_array().unwrap().len(), 1);
    copilot_config::save_vscode_settings(&SyncSettings::default(), &options, || {
        crate::settings::update_settings(local)
    })
    .unwrap();
    assert_eq!(read(&target)[1]["models"][0]["id"], "wire-id");
    let old_cli = fs::read(copilot_config::providers_path()).unwrap();
    let old_vscode = fs::read(&target).unwrap();
    card["models"][0]["wireModel"] = json!("updated");
    let failed: Result<(), AppError> =
        copilot_config::save_and_commit("relay", Some(&card), false, || {
            Err(AppError::Message("DB unavailable".into()))
        });
    assert!(failed.is_err());
    assert_eq!(fs::read(copilot_config::providers_path()).unwrap(), old_cli);
    assert_eq!(fs::read(&target).unwrap(), old_vscode);
    copilot_config::save_and_commit("relay", Some(&card), false, || Ok(())).unwrap();
    assert_eq!(read(&target)[1]["models"][0]["id"], "updated");
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Allow reads so status inspection succeeds, but deny replacement by
        // holding a handle without FILE_SHARE_DELETE.
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&target)
            .unwrap();
        card["models"][0]["wireModel"] = json!("write-locked-target");
        copilot_config::save_and_commit("relay", Some(&card), false, || Ok(())).unwrap();
        let status = copilot_config::vscode_sync_status().unwrap();
        assert!(status.pending);
        assert!(
            status.error.is_some(),
            "the actual write error must remain visible"
        );
        assert_eq!(read(&target)[1]["models"][0]["id"], "updated");
        drop(lock);
        copilot_config::sync_vscode().unwrap();
        assert_eq!(read(&target)[1]["models"][0]["id"], "write-locked-target");
        assert!(!copilot_config::vscode_sync_status().unwrap().pending);
    }
    fs::write(&target, r#"{"apiKey":"must-stay-private", invalid }"#).unwrap();
    let old_cli = fs::read(copilot_config::providers_path()).unwrap();
    card["models"][0]["wireModel"] = json!("blocked");
    copilot_config::save_and_commit("relay", Some(&card), false, || {
        commits.set(99);
        Ok(())
    })
    .unwrap();
    assert_eq!(commits.get(), 99);
    assert_ne!(fs::read(copilot_config::providers_path()).unwrap(), old_cli);
    let status = copilot_config::vscode_sync_status().unwrap();
    assert!(status.pending);
    assert!(status
        .error
        .as_deref()
        .unwrap()
        .contains("chatLanguageModels.json"));
    assert!(!status
        .error
        .as_deref()
        .unwrap()
        .contains("must-stay-private"));
    fs::write(&target, old_vscode).unwrap();
    copilot_config::sync_vscode().unwrap();
    assert_eq!(read(&target)[1]["models"][0]["id"], "blocked");
    assert!(!copilot_config::vscode_sync_status().unwrap().pending);
    copilot_config::save_and_commit("relay", None, false, || Ok(())).unwrap();
    assert_eq!(
        read(&target),
        json!([{"name":"external","vendor":"manual"}])
    );
    copilot_config::add_and_commit("relay", &card, || Ok(())).unwrap();
    fs::write(copilot_config::providers_path(), "{ invalid external edit").unwrap();
    let mut local = crate::settings::get_settings();
    local.copilot_vscode = SyncSettings::default();
    copilot_config::save_vscode_settings(&options, &SyncSettings::default(), || {
        crate::settings::update_settings(local)
    })
    .unwrap();
    assert_eq!(
        read(&target),
        json!([{"name":"external","vendor":"manual"}])
    );
    assert!(!crate::settings::get_settings().copilot_vscode.enabled);
}

#[test]
fn inspecting_pending_sync_never_creates_files() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("chatLanguageModels.json");
    let options = settings(&[&target]);
    assert!(inspect(&options, &registry()).unwrap().pending);
    assert!(!target.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    apply(&options, &options, &registry(), || Ok(())).unwrap();
    assert!(!inspect(&options, &registry()).unwrap().pending);
}

#[test]
fn oversized_generated_config_is_rejected_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("chatLanguageModels.json");
    fs::write(&target, "[]").unwrap();
    let mut document = registry();
    document["models"][0]["name"] = json!("A".repeat(MAX_CONFIG_BYTES as usize));
    let committed = Cell::new(false);
    assert!(apply(
        &SyncSettings::default(),
        &settings(&[&target]),
        &document,
        || {
            committed.set(true);
            Ok(())
        }
    )
    .is_err());
    assert!(!committed.get());
    assert_eq!(fs::read(&target).unwrap(), b"[]");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
#[ignore = "exports a native fixture for the installed VS Code end-to-end test"]
fn export_vscode_smoke_fixture() {
    let origin = std::env::var("CC_SWITCH_VSCODE_SMOKE_ORIGIN").expect("local smoke server origin");
    let url = url::Url::parse(&origin).unwrap();
    assert_eq!(url.scheme(), "http");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    let root = tempfile::tempdir().unwrap();
    let user_data = root.path().join("vscode");
    let target = user_data.join("User/chatLanguageModels.json");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, serde_json::to_vec(&json!([{
        "name":"Existing group","vendor":"customendpoint","apiKey":"fixture-existing",
        "models":[{"id":"ccswitch-existing","name":"Existing model",
            "url":format!("{origin}/existing/v1/chat/completions"),"toolCalling":true,"vision":false,
            "maxInputTokens":128000,"maxOutputTokens":8192}]
    }])).unwrap()).unwrap();
    let cases = json!([
        {"name":"CCS Chat","provider":"smoke-chat","wire":"ccs-chat-wire","apiType":"chat-completions",
         "path":"/chat/v1/chat/completions","authHeader":"authorization","authValue":"Bearer fixture-chat",
         "additionalHeaders":{"x-api-key":"fixture-gateway"},"expected":"CCS_CHAT_OK"},
        {"name":"CCS Responses","provider":"smoke-responses","wire":"ccs-responses-wire","apiType":"responses",
         "path":"/responses/v1/responses","authHeader":"authorization","authValue":"Bearer fixture-responses","expected":"CCS_RESPONSES_OK"},
        {"name":"CCS Messages","provider":"smoke-messages","wire":"ccs-messages-wire","apiType":"messages",
         "path":"/anthropic/v1/messages","authHeader":"x-api-key","authValue":"fixture-messages",
         "additionalHeaders":{"authorization":"Bearer fixture-messages-bearer"},"expected":"CCS_MESSAGES_OK"},
        {"name":"CCS Azure","provider":"smoke-azure","wire":"ccs-azure-wire","apiType":"chat-completions",
         "path":"/azure/openai/v1/chat/completions","authHeader":"api-key","authValue":"fixture-azure","expected":"CCS_AZURE_OK"}
    ]);
    let mut document = json!({"providers":[
        {"name":"smoke-chat","type":"openai","baseUrl":format!("{origin}/chat/v1"),
         "apiKey":"fixture-chat","headers":{"x-api-key":"fixture-gateway"}},
        {"name":"smoke-responses","type":"openai","wireApi":"responses","baseUrl":format!("{origin}/responses/v1"),
         "apiKey":"unused-key","bearerToken":"fixture-responses"},
        {"name":"smoke-messages","type":"anthropic","baseUrl":format!("{origin}/anthropic"),
         "apiKey":"fixture-messages","bearerToken":"fixture-messages-bearer"},
        {"name":"smoke-azure","type":"azure","baseUrl":format!("{origin}/azure"),"apiKey":"fixture-azure"}
    ],"models":[]});
    for case in cases.as_array().unwrap() {
        document["models"].as_array_mut().unwrap().push(json!({
            "provider":case["provider"],"id":format!("{}-alias",case["wire"].as_str().unwrap()),
            "wireModel":case["wire"],"modelId":"metadata-only-id","name":case["name"],
            "maxPromptTokens":128000,"maxOutputTokens":8192
        }));
    }
    let source = root.path().join("providers.json");
    fs::write(&source, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let native = crate::copilot_config::read_providers(&source).unwrap();
    let options = settings(&[&target]);
    apply(&SyncSettings::default(), &options, &native, || Ok(())).unwrap();
    assert!(!inspect(&options, &native).unwrap().pending);
    let root = root.keep();
    println!(
        "CC_SWITCH_VSCODE_FIXTURE={}",
        json!({
            "root":root,"userDataDir":user_data,"configuration":target,"cases":cases
        })
    );
}
