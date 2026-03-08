mod backup_round_trip;
mod config_smoke;
mod env_conflict_flow;
mod import_export_deeplink;
mod mcp_sync;
mod prompt_live_sync;
mod provider_endpoints;
mod provider_common_config;
mod provider_live_switch;
mod provider_usage_script;
mod provider_stream_check;
mod provider_universal_flow;
mod proxy_advanced_config;
mod proxy_failover_runtime;
mod proxy_takeover_restore;
mod skill_local_lifecycle;
mod skill_repo_and_import;
mod usage_via_real_proxy_traffic;
mod util;
mod workspace_memory_flow;

use crate::runner::Scenario;

pub fn all() -> Vec<Scenario> {
    vec![
        backup_round_trip::scenario(),
        config_smoke::scenario(),
        env_conflict_flow::scenario(),
        provider_live_switch::scenario(),
        provider_endpoints::scenario(),
        provider_common_config::scenario(),
        provider_usage_script::scenario(),
        provider_stream_check::scenario(),
        provider_universal_flow::scenario(),
        proxy_advanced_config::scenario(),
        prompt_live_sync::scenario(),
        mcp_sync::scenario(),
        import_export_deeplink::scenario(),
        proxy_takeover_restore::scenario(),
        proxy_failover_runtime::scenario(),
        usage_via_real_proxy_traffic::scenario(),
        skill_local_lifecycle::scenario(),
        skill_repo_and_import::scenario(),
        workspace_memory_flow::scenario(),
    ]
}
