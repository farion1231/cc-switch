use super::super::test_support::TestAgentDir;
use super::*;
use serde_json::json;

fn settings_patch(state: &ModelManagementState, changes: Value) -> SettingsPatch {
    serde_json::from_value(json!({"expectedPath":state.settings_path, "expectedRevision":state.settings_revision, "changes":changes})).unwrap()
}
fn override_patch(state: &ModelManagementState, changes: Value) -> ModelOverridePatch {
    serde_json::from_value(json!({"expectedPath":state.models_path, "expectedRevision":state.models_revision, "provider":"openai-codex", "modelId":"vendor/model:version", "changes":changes})).unwrap()
}

#[test]
#[serial_test::serial]
fn management_settings_merge_disjoint_edits_but_conflict_same_entry_and_scope() {
    let _agent = TestAgentDir::new();
    let path = super::super::get_omp_settings_path().unwrap();
    let original = "# global\nmodelRoles:\n  default: [CPA/model:xhigh, '@task']\n  custom: '@smol'\ncycleOrder: [custom, additional]\nmodelTags:\n  tagged: {name: Tagged}\ntask:\n  limit: 4\n  agentModelOverrides:\n    scout: ['@task', CPA/model:low]\nscript: |\n  # opaque\n  echo &data\n  \tinside script\n";
    std::fs::write(&path, original).unwrap();
    let state = get_state().unwrap();
    assert_eq!(
        state.model_roles["default"],
        json!(["CPA/model:xhigh", "@task"])
    );
    assert_eq!(
        state.agent_model_overrides["scout"],
        json!(["@task", "CPA/model:low"])
    );
    assert!(["widget", "additional", "tagged"]
        .iter()
        .all(|name| state.role_names.iter().any(|role| role == name)));
    std::fs::write(&path, original.replace("limit: 4", "limit: 5")).unwrap();
    patch_settings(settings_patch(&state, json!([{"section":"modelRoles","key":"default","expected":["CPA/model:xhigh","@task"],"value":["@custom","CPA/new:high"]}]))).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("limit: 5"));
    assert!(after.contains("script: |\n  # opaque\n  echo &data\n  \tinside script\n"));
    assert!(!super::super::get_omp_models_path().unwrap().exists());
    assert!(patch_settings(settings_patch(&state, json!([{"section":"modelRoles","key":"default","expected":["CPA/model:xhigh","@task"],"value":"other/model"}]))).is_err());
    let mut scoped = settings_patch(
        &state,
        json!([{"section":"modelRoles","key":"custom","expected":"@smol","value":"@task"}]),
    );
    scoped.expected_path.push_str(".other");
    assert!(patch_settings(scoped).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
}

#[test]
#[serial_test::serial]
fn management_overrides_are_leaf_only_allowlisted_and_credential_neutral() {
    let _agent = TestAgentDir::new();
    let path = super::super::get_omp_models_path().unwrap();
    let original = "# models\nproviders:\n  openai-codex:\n    modelOverrides:\n      'vendor/model:version':\n        maxTokens: 1000\n        headers: {Authorization: '!private helper'} # preserve\n        unknown: [keep, exactly]\n  other:\n    apiKey: '!untouched'\n";
    std::fs::write(&path, original).unwrap();
    let state = get_state().unwrap();
    assert_eq!(state.model_overrides[0].parameters.len(), 1);
    patch_override(override_patch(
        &state,
        json!([{"field":"maxTokens","expected":1000,"value":4096}]),
    ))
    .unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after,
        original.replace("maxTokens: 1000", "maxTokens: 4096")
    );
    assert!(!super::super::get_omp_settings_path().unwrap().exists());
    for field in ["headers", "apiKey", "thinking", "cost", "baseUrl"] {
        assert!(patch_override(override_patch(
            &state,
            json!([{"field":field,"expected":null,"value":"rejected"}])
        ))
        .is_err());
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), after);
    assert!(!serde_json::to_string(&get_state().unwrap())
        .unwrap()
        .contains("private helper"));
}

#[test]
#[serial_test::serial]
fn management_override_reset_removes_only_empty_metadata_containers() {
    let _agent = TestAgentDir::new();
    let state = get_state().unwrap();
    patch_override(override_patch(
        &state,
        json!([{"field":"maxTokens","expected":null,"value":4096}]),
    ))
    .unwrap();
    let state = get_state().unwrap();
    patch_override(override_patch(
        &state,
        json!([{"field":"maxTokens","expected":4096,"value":null}]),
    ))
    .unwrap();
    assert!(super::super::get_providers().unwrap().is_empty());
}

#[test]
#[serial_test::serial]
fn management_catalog_uses_payload_provider_newest_snapshot_and_explicit_custom_fields() {
    let _agent = TestAgentDir::new();
    let path = super::super::get_omp_data_dir().unwrap().join("models.db");
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE model_cache (provider_id TEXT, version INTEGER, updated_at INTEGER, models TEXT);").unwrap();
    for (namespace, updated, model) in [
        (
            "openai-codex:old",
            1,
            json!({"provider":"openai-codex","id":"same","name":"Old","contextWindow":900,"maxTokens":100,"input":["image"],"reasoning":false}),
        ),
        (
            "openai-codex:new",
            2,
            json!({"provider":"openai-codex","id":"same","name":"New","contextWindow":500000,"maxTokens":128000,"input":["text","image"],"reasoning":true,"thinking":{"mode":"effort","efforts":["low","high","xhigh"],"effortMap":{"secret":"do not return"}},"headers":{"Authorization":"secret"}}),
        ),
        (
            "other:scope",
            3,
            json!({"provider":"other","id":"same","name":"Other","contextWindow":4096,"maxTokens":512,"input":["text"],"reasoning":false,"kind":"image"}),
        ),
    ] {
        db.execute(
            "INSERT INTO model_cache VALUES (?1,14,?2,?3)",
            rusqlite::params![namespace, updated, json!([model]).to_string()],
        )
        .unwrap();
    }
    std::fs::write(super::super::get_omp_models_path().unwrap(), "providers:\n  openai-codex:\n    models:\n      - id: same\n        name: Configured\n    modelOverrides:\n      same:\n        maxTokens: 8192\n").unwrap();
    let state = get_state().unwrap();
    assert_eq!(state.catalog_status, "ready");
    assert_eq!(state.models.len(), 2);
    let codex = state
        .models
        .iter()
        .find(|model| model.provider == "openai-codex")
        .unwrap();
    assert_eq!(codex.name, "Configured");
    assert_eq!(codex.context_window, Some(500000));
    assert_eq!(codex.max_tokens, Some(8192));
    assert_eq!(codex.input, vec!["text", "image"]);
    assert!(codex.reasoning);
    let response = serde_json::to_string(&state).unwrap();
    assert!(!response.contains("secret"));
    assert!(!response.contains("openai-codex:new"));
}

#[test]
#[serial_test::serial]
fn management_catalog_distinguishes_missing_empty_unsupported_and_corrupt() {
    let _agent = TestAgentDir::new();
    assert_eq!(get_state().unwrap().catalog_status, "unavailable");
    let path = super::super::get_omp_data_dir().unwrap().join("models.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE model_cache (provider_id TEXT, version INTEGER, updated_at INTEGER, models TEXT);").unwrap();
    assert_eq!(get_state().unwrap().catalog_status, "ready");
    db.execute("INSERT INTO model_cache VALUES ('old',10,0,'[]')", [])
        .unwrap();
    assert_eq!(get_state().unwrap().catalog_status, "unsupported");
    db.execute("DELETE FROM model_cache", []).unwrap();
    db.execute(
        "INSERT INTO model_cache VALUES ('broken',14,0,'not json')",
        [],
    )
    .unwrap();
    assert_eq!(get_state().unwrap().catalog_status, "error");
}

#[test]
#[serial_test::serial]
fn management_rejects_malformed_references_and_legacy_creation() {
    let _agent = TestAgentDir::new();
    let path = super::super::get_omp_settings_path().unwrap();
    std::fs::write(&path, "modelRoles:\n  valid: a/b\n  malformed: 42\n").unwrap();
    assert!(get_state().is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::write(
        path.parent().unwrap().join("settings.json"),
        "{\"model\":\"keep\"}",
    )
    .unwrap();
    let state = get_state().unwrap();
    assert!(patch_settings(settings_patch(
        &state,
        json!([{"section":"modelRoles","key":"default","expected":null,"value":"a/b"}])
    ))
    .is_err());
    assert!(!path.exists());
}

#[test]
#[serial_test::serial]
fn management_validation_failure_never_writes_either_file() {
    let _agent = TestAgentDir::new();
    let state = get_state().unwrap();
    let settings = super::super::get_omp_settings_path().unwrap();
    let models = super::super::get_omp_models_path().unwrap();
    std::fs::write(&models, "providers: [not-a-map]\n").unwrap();
    assert!(patch_settings(settings_patch(
        &state,
        json!([{"section":"modelRoles","key":"default","expected":null,"value":"a/b"}])
    ))
    .is_err());
    assert!(!settings.exists());
    std::fs::write(&models, "providers: {}\n").unwrap();
    let state = get_state().unwrap();
    std::fs::write(&settings, "modelRoles:\n  invalid: 42\n").unwrap();
    let before = std::fs::read(&models).unwrap();
    assert!(patch_override(override_patch(
        &state,
        json!([{"field":"maxTokens","expected":null,"value":4096}])
    ))
    .is_err());
    assert_eq!(std::fs::read(&models).unwrap(), before);
}

#[test]
fn management_empty_mapping_reset_splices_each_container_safely() {
    let path = ["providers", "p", "modelOverrides", "m", "maxTokens"];
    let text = edit_leaf("", &path, Some(&json!(4096))).unwrap();
    let text = edit_leaf(&text, &path, None).expect("remove field");
    let text = edit_leaf(&text, &path[..4], None).expect("remove empty model");
    let text = edit_leaf(&text, &path[..3], None).expect("remove empty modelOverrides");
    edit_leaf(&text, &path[..2], None).expect("remove empty provider");
}
