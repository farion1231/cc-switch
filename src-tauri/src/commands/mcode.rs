use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::misc::run_detected_tool_command_with_timeout;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum McodeAction {
    Status,
    SaveApiKey,
    UseApiKey,
    UseTokenPlan,
    Test,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McodeSource {
    TokenPlan,
    MinimaxApiKey,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McodeStatus {
    source: McodeSource,
    has_api_key: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSnapshot {
    minimax_model_source: McodeSource,
    providers: Vec<ProviderStatus>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderStatus {
    provider_id: String,
    has_api_key: bool,
}

fn run_provider(args: &[&str], api_key: Option<&str>) -> Result<Vec<u8>, String> {
    let home = dirs::home_dir().ok_or("Cannot locate the home directory")?;
    let env = api_key
        .map(|key| vec![("MCODE_PROVIDER_API_KEY", key.to_owned())])
        .unwrap_or_default();
    let output = run_detected_tool_command_with_timeout(
        "mcode",
        args,
        Some(Duration::from_secs(60)),
        &env,
        &home,
    )?;
    if !output.status.success() {
        // CLI diagnostics may contain credentials. Do not forward them to the UI or logs.
        return Err(
            "MCode command failed. Check your installation and credentials in mcode.".into(),
        );
    }
    Ok(output.stdout)
}

fn manage(action: McodeAction, api_key: Option<String>) -> Result<McodeStatus, String> {
    match action {
        McodeAction::Status => {}
        McodeAction::SaveApiKey => {
            let key = api_key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .ok_or("Enter a MiniMax API key")?;
            run_provider(&["provider", "set-minimax-key"], Some(key))?;
        }
        McodeAction::UseApiKey => {
            run_provider(&["provider", "use", "api-key"], None)?;
        }
        McodeAction::UseTokenPlan => {
            run_provider(&["provider", "use", "token-plan"], None)?;
        }
        McodeAction::Test => {
            let output = run_provider(&["provider", "test", "minimax_api", "--json"], None)?;
            let result: serde_json::Value = serde_json::from_slice(&output)
                .map_err(|_| "MCode returned an invalid test result")?;
            if result.get("success").and_then(|value| value.as_bool()) != Some(true) {
                return Err(
                    "MiniMax API key test failed. Check your key and network in mcode.".into(),
                );
            }
        }
    }
    let output = run_provider(&["provider", "list", "--json"], None)?;
    let snapshot: ProviderSnapshot =
        serde_json::from_slice(&output).map_err(|_| "MCode returned an invalid provider list")?;
    let provider = snapshot
        .providers
        .iter()
        .find(|provider| provider.provider_id == "minimax_api")
        .ok_or("MCode does not expose a MiniMax API key provider. Update MCode.")?;
    Ok(McodeStatus {
        source: snapshot.minimax_model_source,
        has_api_key: provider.has_api_key,
    })
}

/// MCode owns its credential storage; CC Switch only invokes its native provider commands.
#[tauri::command]
pub async fn manage_mcode(
    action: McodeAction,
    api_key: Option<String>,
) -> Result<McodeStatus, String> {
    tauri::async_runtime::spawn_blocking(move || manage(action, api_key))
        .await
        .map_err(|_| "MCode command could not complete".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_key_without_starting_mcode() {
        assert!(manage(McodeAction::SaveApiKey, Some("  ".into())).is_err());
    }

    /// Run with an isolated MINIMAX_DATA_DIR, mcode on PATH and MCODE_TEST_API_KEY.
    #[test]
    #[ignore = "requires installed MCode and a real API key"]
    fn real_mcode_credentials() {
        let data_dir = std::env::var("MINIMAX_DATA_DIR").expect("Set an isolated MINIMAX_DATA_DIR");
        let key = std::env::var("MCODE_TEST_API_KEY").expect("Set MCODE_TEST_API_KEY");
        let config_path = std::path::Path::new(&data_dir).join("config.yaml");
        let before: serde_yaml::Value = serde_yaml::from_str(
            &std::fs::read_to_string(&config_path).expect("Prepare isolated MCode configuration"),
        )
        .expect("Valid YAML");

        let saved = manage(McodeAction::SaveApiKey, Some(key.clone())).expect("Save key");
        assert!(saved.has_api_key);
        assert!(matches!(saved.source, McodeSource::MinimaxApiKey));
        assert!(!serde_json::to_string(&saved)
            .expect("Status JSON")
            .contains(&key));
        let plan = manage(McodeAction::UseTokenPlan, None).expect("Select Token Plan");
        assert!(matches!(plan.source, McodeSource::TokenPlan));
        let api = manage(McodeAction::UseApiKey, None).expect("Select API Key");
        assert!(matches!(api.source, McodeSource::MinimaxApiKey));
        manage(McodeAction::Test, None).expect("Real MiniMax request");
        manage(
            McodeAction::SaveApiKey,
            Some("invalid-mcode-test-key".into()),
        )
        .expect("Save invalid test key");
        let failed_test = manage(McodeAction::Test, None);
        manage(McodeAction::SaveApiKey, Some(key)).expect("Restore real key");
        assert!(
            failed_test.is_err(),
            "Invalid credentials must fail the test"
        );
        assert!(
            manage(McodeAction::Status, None)
                .expect("Refresh")
                .has_api_key
        );

        let after: serde_yaml::Value = serde_yaml::from_str(
            &std::fs::read_to_string(config_path).expect("Read MCode configuration"),
        )
        .expect("Valid YAML");
        for (field, value) in before.as_mapping().expect("Configuration mapping") {
            if field.as_str() != Some("minimax_api") && field.as_str() != Some("minimaxModelSource")
            {
                assert!(
                    after.get(field) == Some(value),
                    "Unrelated MCode setting changed"
                );
            }
        }
    }
}
