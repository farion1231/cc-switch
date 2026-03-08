mod config_smoke;
mod import_export_deeplink;
mod mcp_sync;
mod prompt_live_sync;
mod provider_endpoints;
mod provider_live_switch;
mod proxy_failover_runtime;
mod proxy_takeover_restore;
mod skill_local_lifecycle;
mod usage_via_real_proxy_traffic;
mod util;

use crate::runner::Scenario;

pub fn all() -> Vec<Scenario> {
    vec![
        config_smoke::scenario(),
        provider_live_switch::scenario(),
        provider_endpoints::scenario(),
        prompt_live_sync::scenario(),
        mcp_sync::scenario(),
        import_export_deeplink::scenario(),
        proxy_takeover_restore::scenario(),
        proxy_failover_runtime::scenario(),
        usage_via_real_proxy_traffic::scenario(),
        skill_local_lifecycle::scenario(),
    ]
}
