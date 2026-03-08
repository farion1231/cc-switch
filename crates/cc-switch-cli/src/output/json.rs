//! JSON output format

pub fn print_providers(
    providers: &indexmap::IndexMap<String, cc_switch_core::Provider>,
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(providers)?);
    Ok(())
}

pub fn print_provider_detail(provider: &cc_switch_core::Provider) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(provider)?);
    Ok(())
}

pub fn print_universal_providers(
    providers: &std::collections::HashMap<String, cc_switch_core::UniversalProvider>,
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(providers)?);
    Ok(())
}

pub fn print_mcp_servers(
    servers: &indexmap::IndexMap<String, cc_switch_core::McpServer>,
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(servers)?);
    Ok(())
}

pub fn print_mcp_server_detail(server: &cc_switch_core::McpServer) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(server)?);
    Ok(())
}

pub fn print_prompts(
    prompts: &indexmap::IndexMap<String, cc_switch_core::Prompt>,
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(prompts)?);
    Ok(())
}

pub fn print_prompt_detail(prompt: &cc_switch_core::Prompt) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(prompt)?);
    Ok(())
}

pub fn print_skills(skills: &[cc_switch_core::InstalledSkill]) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(skills)?);
    Ok(())
}

pub fn print_proxy_status(status: &cc_switch_core::ProxyStatus) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(status)?);
    Ok(())
}

pub fn print_proxy_config(config: &cc_switch_core::ProxyConfig) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(config)?);
    Ok(())
}

pub fn print_takeover_status(status: &cc_switch_core::ProxyTakeoverStatus) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(status)?);
    Ok(())
}

pub fn print_failover_queue(queue: &[cc_switch_core::FailoverQueueItem]) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(queue)?);
    Ok(())
}

pub fn print_provider_health(health: &cc_switch_core::ProviderHealth) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(health)?);
    Ok(())
}

pub fn print_circuit_breaker_config(
    config: &cc_switch_core::CircuitBreakerConfig,
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(config)?);
    Ok(())
}

pub fn print_settings(settings: &cc_switch_core::AppSettings) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(settings)?);
    Ok(())
}

pub fn print_usage_summary(summary: &cc_switch_core::UsageSummary) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(summary)?);
    Ok(())
}

pub fn print_usage_logs(logs: &[cc_switch_core::RequestLog]) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(logs)?);
    Ok(())
}

pub fn print_custom_endpoints(
    endpoints: &[cc_switch_core::settings::CustomEndpoint],
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(endpoints)?);
    Ok(())
}

pub fn print_endpoint_latencies(
    latencies: &[cc_switch_core::EndpointLatency],
) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(latencies)?);
    Ok(())
}

pub fn print_common_config_snippet(app: &str, snippet: Option<&str>) -> anyhow::Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "app": app,
            "snippet": snippet,
        }))?
    );
    Ok(())
}
