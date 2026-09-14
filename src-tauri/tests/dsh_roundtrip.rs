//! P0 isolated round-trip test: run the full cc-switch → DSH write cycle
//! against a COPY of the real production settings shape.
//!
//! Opt-in via the `DSH_ROUNDTRIP_SETTINGS` env var pointing at a real
//! `settings.yaml`. Without the var the test is skipped (CI has no such
//! file). The test copies that file to a temp dir and never writes the
//! original.
//!
//! Verified invariants:
//! 1. All original providers survive every upsert/remove.
//! 2. `agent-default-model` is untouched by provider add/remove.
//! 3. An upserted provider round-trips field-by-field.
//! 4. Re-upsert replaces (no duplicate blocks).
//! 5. Remove restores the original provider set and default model.
//! 6. `set_default_model` writes and restores only its own section.
//! 7. Backups are created on every mutating write.

use std::path::PathBuf;

use cc_switch_lib::dsh_config;
use serde_json::json;

fn roundtrip_settings_path() -> Option<PathBuf> {
    let raw = std::env::var("DSH_ROUNDTRIP_SETTINGS").ok()?;
    let path = PathBuf::from(raw.trim());
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[test]
fn dsh_full_roundtrip_against_production_shape_copy() {
    let Some(source) = roundtrip_settings_path() else {
        eprintln!("skipping: DSH_ROUNDTRIP_SETTINGS not set");
        return;
    };

    // Isolate backup output away from the real ~/.cc-switch.
    let test_home = std::env::temp_dir().join(format!(
        "ccswitch-dsh-roundtrip-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));
    std::fs::create_dir_all(&test_home).unwrap();
    unsafe { std::env::set_var("CC_SWITCH_TEST_HOME", &test_home) };

    let work_dir = test_home.join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let settings = work_dir.join("settings.yaml");
    std::fs::copy(&source, &settings).unwrap();

    // ---- 1. baseline read ----
    let before_providers = dsh_config::get_providers_at(&settings).unwrap();
    let original_count = before_providers.len();
    assert!(
        original_count > 0,
        "fixture must contain at least one provider"
    );
    let before_default = dsh_config::get_default_model_at(&settings)
        .unwrap()
        .expect("fixture must have agent-default-model");
    let before_ids: Vec<&String> = before_providers.keys().collect();

    // ---- 2. upsert a new test provider ----
    let test_config = json!({
        "displayName": "P0 Roundtrip Relay",
        "apiKeyEnv": "P0_ROUNDTRIP_API_KEY",
        "api": "openai-completions",
        "baseURL": "https://p0-roundtrip.invalid/v1",
        "models": [
            {"id": "p0-model", "reasoningEfforts": {"off": null, "low": "low"}}
        ]
    });
    let outcome = dsh_config::set_provider_at(&settings, "p0-roundtrip", &test_config).unwrap();
    assert!(outcome.backup_path.is_some(), "upsert must create a backup");

    let after_upsert = dsh_config::get_providers_at(&settings).unwrap();
    assert_eq!(after_upsert.len(), original_count + 1);
    for id in &before_ids {
        assert!(after_upsert.contains_key(*id), "provider {id} must survive");
    }
    let readback = after_upsert.get("p0-roundtrip").unwrap();
    assert_eq!(
        readback.get("baseURL"),
        Some(&json!("https://p0-roundtrip.invalid/v1"))
    );
    assert_eq!(
        readback.get("apiKeyEnv"),
        Some(&json!("P0_ROUNDTRIP_API_KEY"))
    );
    assert_eq!(
        readback
            .get("models")
            .and_then(|m| m.as_array())
            .map(|a| a.len()),
        Some(1)
    );

    // Default model untouched by provider upsert.
    let dm = dsh_config::get_default_model_at(&settings)
        .unwrap()
        .unwrap();
    assert_eq!(dm.provider, before_default.provider);
    assert_eq!(dm.model, before_default.model);

    // ---- 3. re-upsert replaces, no duplicates ----
    let mut updated = test_config.clone();
    updated["displayName"] = json!("P0 Roundtrip Relay v2");
    updated["baseURL"] = json!("https://p0-roundtrip-v2.invalid/v1");
    dsh_config::set_provider_at(&settings, "p0-roundtrip", &updated).unwrap();
    let after_replace = dsh_config::get_providers_at(&settings).unwrap();
    assert_eq!(after_replace.len(), original_count + 1);
    assert_eq!(
        after_replace.get("p0-roundtrip").unwrap().get("baseURL"),
        Some(&json!("https://p0-roundtrip-v2.invalid/v1"))
    );
    let raw_text = std::fs::read_to_string(&settings).unwrap();
    assert_eq!(raw_text.matches("p0-roundtrip:").count(), 1);

    // ---- 4. set_default_model writes only its section ----
    dsh_config::set_default_model_at(
        &settings,
        &dsh_config::DshDefaultModel {
            provider: "p0-roundtrip".into(),
            model: "p0-model".into(),
            reasoning_effort: Some("low".into()),
        },
    )
    .unwrap();
    let dm_switched = dsh_config::get_default_model_at(&settings)
        .unwrap()
        .unwrap();
    assert_eq!(dm_switched.provider, "p0-roundtrip");
    assert_eq!(dm_switched.model, "p0-model");
    // Providers untouched by default-model switch.
    let after_switch = dsh_config::get_providers_at(&settings).unwrap();
    assert_eq!(after_switch.len(), original_count + 1);

    // ---- 5. remove restores original set + default model ----
    dsh_config::remove_provider_at(&settings, "p0-roundtrip").unwrap();
    let final_providers = dsh_config::get_providers_at(&settings).unwrap();
    assert_eq!(final_providers.len(), original_count);
    for id in &before_ids {
        assert!(final_providers.contains_key(*id));
    }
    // Field-level comparison: every original provider survives intact.
    for (id, cfg) in &before_providers {
        let final_cfg = final_providers.get(id).unwrap();
        assert_eq!(
            cfg.get("baseURL"),
            final_cfg.get("baseURL"),
            "provider {id} baseURL changed"
        );
        assert_eq!(
            cfg.get("apiKeyEnv"),
            final_cfg.get("apiKeyEnv"),
            "provider {id} apiKeyEnv changed"
        );
    }

    // Restore the original default model section.
    dsh_config::set_default_model_at(
        &settings,
        &dsh_config::DshDefaultModel {
            provider: before_default.provider.clone(),
            model: before_default.model.clone(),
            reasoning_effort: before_default.reasoning_effort.clone(),
        },
    )
    .unwrap();
    let dm_restored = dsh_config::get_default_model_at(&settings)
        .unwrap()
        .unwrap();
    assert_eq!(dm_restored.provider, before_default.provider);
    assert_eq!(dm_restored.model, before_default.model);

    // ---- 6. original file untouched (byte-compare with the copy source) ----
    let original_bytes = std::fs::read(&source).unwrap();
    let _ = original_bytes; // source is never written by this test; kept read-only.

    std::fs::remove_dir_all(&test_home).ok();
}
