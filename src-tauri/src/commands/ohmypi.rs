use crate::ohmypi_config::{
    agent_discovery_provider_display_name, read_ohmypi_disabled_providers,
    set_ohmypi_disabled_providers_union, AGENT_DISCOVERY_PROVIDER_IDS,
};
use crate::provider::UsageScript;
use crate::services::ohmypi_state::{OhMyPiCurrentState, OhMyPiStateService};
use crate::services::ProviderService;
use crate::store::AppState;
use serde::Serialize;
use tauri::State;

/// Snapshot of omp's agent auto-discovery state used by the enable-ohmypi guard.
///
/// `needsConfirmation` is true when `disabledProviders` does not yet contain
/// every id in `AGENT_DISCOVERY_PROVIDER_IDS`; the frontend then prompts the
/// user before enabling management.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OhMyPiAgentDiscoveryState {
    pub needs_confirmation: bool,
    /// Full discovery-provider id list (always `AGENT_DISCOVERY_PROVIDER_IDS`).
    pub required_provider_ids: Vec<String>,
    /// Discovery ids not yet present in `disabledProviders` (subset).
    pub missing_provider_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OhMyPiAgentDiscoveryProvider {
    pub id: String,
    pub display_name: String,
}

#[tauri::command]
pub(crate) fn get_ohmypi_agent_discovery_state() -> Result<OhMyPiAgentDiscoveryState, String> {
    let disabled = read_ohmypi_disabled_providers().map_err(|error| error.to_string())?;
    let disabled_set: std::collections::HashSet<&str> =
        disabled.iter().map(String::as_str).collect();
    let missing: Vec<String> = AGENT_DISCOVERY_PROVIDER_IDS
        .iter()
        .copied()
        .filter(|id| !disabled_set.contains(*id))
        .map(String::from)
        .collect();
    Ok(OhMyPiAgentDiscoveryState {
        needs_confirmation: !missing.is_empty(),
        required_provider_ids: AGENT_DISCOVERY_PROVIDER_IDS
            .iter()
            .copied()
            .map(String::from)
            .collect(),
        missing_provider_ids: missing,
    })
}

/// for the confirm dialog.
#[tauri::command]
pub(crate) fn get_ohmypi_agent_discovery_providers(
) -> Result<Vec<OhMyPiAgentDiscoveryProvider>, String> {
    Ok(AGENT_DISCOVERY_PROVIDER_IDS
        .iter()
        .copied()
        .map(|id| OhMyPiAgentDiscoveryProvider {
            id: id.to_string(),
            display_name: agent_discovery_provider_display_name(id).to_string(),
        })
        .collect())
}

/// Write: union all discovery-provider ids into omp `disabledProviders`
#[tauri::command]
pub(crate) fn disable_ohmypi_agent_auto_discovery() -> Result<Vec<String>, String> {
    set_ohmypi_disabled_providers_union(&AGENT_DISCOVERY_PROVIDER_IDS)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_ohmypi_current_state(
    state: State<'_, AppState>,
) -> Result<OhMyPiCurrentState, String> {
    OhMyPiStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn update_ohmypi_provider_usage_script(
    state: State<'_, AppState>,
    id: String,
    #[allow(non_snake_case)] usageScript: UsageScript,
) -> Result<bool, String> {
    ProviderService::update_ohmypi_usage_script(state.inner(), &id, usageScript)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::ohmypi_config::test_support::TestAgentDir;
    use crate::ohmypi_config::{get_ohmypi_agent_dir, AGENT_DISCOVERY_PROVIDER_IDS};
    use serial_test::serial;
    use std::fs;

    fn write_config(content: &str) {
        let agent = get_ohmypi_agent_dir().expect("agent dir");
        fs::create_dir_all(&agent).expect("create agent dir");
        fs::write(agent.join("config.yml"), content).expect("write config");
    }

    #[test]
    #[serial]
    fn discovery_state_empty_disabled_needs_confirmation() {
        let _agent = TestAgentDir::new();
        write_config("modelRoles:\n  default: openai/gpt-4o\n");
        let state = super::get_ohmypi_agent_discovery_state().expect("discovery state");
        assert!(state.needs_confirmation);
        assert_eq!(state.required_provider_ids.len(), 12);
        assert_eq!(state.missing_provider_ids.len(), 12);
    }

    #[test]
    #[serial]
    fn discovery_state_all_disabled_no_confirmation() {
        let _agent = TestAgentDir::new();
        let ids_yaml = AGENT_DISCOVERY_PROVIDER_IDS
            .iter()
            .map(|id| format!("  - {id}"))
            .collect::<Vec<_>>()
            .join("\n");
        write_config(&format!(
            "modelRoles:\n  default: openai/gpt-4o\ndisabledProviders:\n{ids_yaml}\n"
        ));
        let state = super::get_ohmypi_agent_discovery_state().expect("discovery state");
        assert!(!state.needs_confirmation);
        assert!(state.missing_provider_ids.is_empty());
    }

    #[test]
    #[serial]
    fn disable_auto_discovery_writes_union_and_preserves_keys() {
        let _agent = TestAgentDir::new();
        write_config(
            "modelRoles:\n  default: openai/gpt-4o\ndisabledProviders:\n  - claude\n  - custom-src\n",
        );
        let updated = super::disable_ohmypi_agent_auto_discovery().expect("disable");
        // all 12 discovery ids present
        for id in AGENT_DISCOVERY_PROVIDER_IDS {
            assert!(updated.iter().any(|s| s == id), "missing {id}");
        }
        // existing entries preserved
        assert!(updated.iter().any(|s| s == "custom-src"));
        // other key preserved on disk
        let agent = get_ohmypi_agent_dir().expect("agent dir");
        let source = fs::read_to_string(agent.join("config.yml")).expect("read config");
        assert!(source.contains("default: openai/gpt-4o"));
    }

    #[test]
    #[serial]
    fn discovery_providers_list_complete() {
        let list = super::get_ohmypi_agent_discovery_providers().expect("providers list");
        assert_eq!(list.len(), 12);
        assert!(list.iter().all(|p| !p.display_name.is_empty()));
        assert_eq!(list[0].id, "claude");
        assert_eq!(list[0].display_name, "Claude Code");
    }
}
