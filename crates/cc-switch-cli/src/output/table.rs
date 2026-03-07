//! Table output format

use colored::Colorize;
use tabled::{
    settings::{object::Rows, Alignment, Modify, Style},
    Table, Tabled,
};

pub fn print_providers(
    providers: &indexmap::IndexMap<String, cc_switch_core::Provider>,
) -> anyhow::Result<()> {
    if providers.is_empty() {
        println!("No providers found.");
        return Ok(());
    }

    let rows: Vec<ProviderRow> = providers
        .iter()
        .map(|(id, p)| ProviderRow {
            id: id.clone(),
            name: p.name.clone(),
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct ProviderRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

pub fn print_provider_detail(provider: &cc_switch_core::Provider) -> anyhow::Result<()> {
    println!("Name: {}", provider.name.cyan());
    println!("ID: {}", provider.id);
    if let Some(notes) = &provider.notes {
        println!("Notes: {}", notes);
    }
    Ok(())
}

pub fn print_universal_providers(
    providers: &std::collections::HashMap<String, cc_switch_core::UniversalProvider>,
) -> anyhow::Result<()> {
    if providers.is_empty() {
        println!("No universal providers found.");
        return Ok(());
    }

    let rows: Vec<UniversalProviderRow> = providers
        .iter()
        .map(|(id, p)| UniversalProviderRow {
            id: id.clone(),
            name: p.name.clone(),
            claude: if p.apps.claude {
                "✓".to_string()
            } else {
                "".to_string()
            },
            codex: if p.apps.codex {
                "✓".to_string()
            } else {
                "".to_string()
            },
            gemini: if p.apps.gemini {
                "✓".to_string()
            } else {
                "".to_string()
            },
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct UniversalProviderRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Claude")]
    claude: String,
    #[tabled(rename = "Codex")]
    codex: String,
    #[tabled(rename = "Gemini")]
    gemini: String,
}

pub fn print_mcp_servers(
    servers: &indexmap::IndexMap<String, cc_switch_core::McpServer>,
) -> anyhow::Result<()> {
    if servers.is_empty() {
        println!("No MCP servers found.");
        return Ok(());
    }

    let rows: Vec<McpServerRow> = servers
        .iter()
        .map(|(id, s)| McpServerRow {
            id: id.clone(),
            name: s.name.clone(),
            claude: if s.apps.claude {
                "✓".green().to_string()
            } else {
                "".to_string()
            },
            codex: if s.apps.codex {
                "✓".green().to_string()
            } else {
                "".to_string()
            },
            gemini: if s.apps.gemini {
                "✓".green().to_string()
            } else {
                "".to_string()
            },
            opencode: if s.apps.opencode {
                "✓".green().to_string()
            } else {
                "".to_string()
            },
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct McpServerRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Claude")]
    claude: String,
    #[tabled(rename = "Codex")]
    codex: String,
    #[tabled(rename = "Gemini")]
    gemini: String,
    #[tabled(rename = "OpenCode")]
    opencode: String,
}

pub fn print_mcp_server_detail(server: &cc_switch_core::McpServer) -> anyhow::Result<()> {
    println!("ID: {}", server.id.cyan());
    println!("Name: {}", server.name);
    println!("Config: {}", serde_json::to_string_pretty(&server.server)?);
    println!("Apps:");
    if server.apps.claude {
        println!("  {} Claude", "✓".green());
    }
    if server.apps.codex {
        println!("  {} Codex", "✓".green());
    }
    if server.apps.gemini {
        println!("  {} Gemini", "✓".green());
    }
    if server.apps.opencode {
        println!("  {} OpenCode", "✓".green());
    }
    Ok(())
}

pub fn print_prompts(
    prompts: &indexmap::IndexMap<String, cc_switch_core::Prompt>,
) -> anyhow::Result<()> {
    if prompts.is_empty() {
        println!("No prompts found.");
        return Ok(());
    }

    let rows: Vec<PromptRow> = prompts
        .iter()
        .map(|(id, p)| PromptRow {
            id: id.clone(),
            name: p.name.clone(),
            enabled: if p.enabled {
                "✓".green().to_string()
            } else {
                "".to_string()
            },
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct PromptRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Enabled")]
    enabled: String,
}

pub fn print_prompt_detail(prompt: &cc_switch_core::Prompt) -> anyhow::Result<()> {
    println!("Name: {}", prompt.name.cyan());
    println!(
        "Description: {}",
        prompt.description.as_deref().unwrap_or("-")
    );
    println!(
        "Enabled: {}",
        if prompt.enabled {
            "Yes".green()
        } else {
            "No".yellow()
        }
    );
    println!("Content:\n{}", prompt.content);
    Ok(())
}

pub fn print_skills(skills: &[cc_switch_core::InstalledSkill]) -> anyhow::Result<()> {
    if skills.is_empty() {
        println!("No skills installed.");
        return Ok(());
    }

    let rows: Vec<SkillRow> = skills
        .iter()
        .map(|s| SkillRow {
            id: s.id.clone(),
            name: s.name.clone(),
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct SkillRow {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name")]
    name: String,
}

pub fn print_proxy_status(status: &cc_switch_core::ProxyStatus) -> anyhow::Result<()> {
    println!(
        "Running: {}",
        if status.running {
            "Yes".green()
        } else {
            "No".yellow()
        }
    );
    if status.running {
        println!("Address: {}:{}", status.address, status.port);
    }
    if let Some(provider_name) = &status.current_provider {
        println!("Current provider: {}", provider_name);
    }
    if !status.active_targets.is_empty() {
        let targets = status
            .active_targets
            .iter()
            .map(|target| {
                format!(
                    "{} -> {} ({})",
                    target.app_type, target.provider_name, target.provider_id
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("Active targets: {}", targets);
    }
    Ok(())
}

pub fn print_proxy_config(config: &cc_switch_core::ProxyConfig) -> anyhow::Result<()> {
    println!("Port: {}", config.listen_port);
    println!("Host: {}", config.listen_address);
    println!(
        "Log enabled: {}",
        if config.enable_logging { "Yes" } else { "No" }
    );
    Ok(())
}

pub fn print_takeover_status(status: &cc_switch_core::ProxyTakeoverStatus) -> anyhow::Result<()> {
    let rows = [
        ("claude", status.claude),
        ("codex", status.codex),
        ("gemini", status.gemini),
        ("opencode", status.opencode),
        ("openclaw", status.openclaw),
    ]
    .into_iter()
    .map(|(app, enabled)| TakeoverRow {
        app: app.to_string(),
        enabled: if enabled {
            "Yes".green().to_string()
        } else {
            "No".to_string()
        },
    })
    .collect::<Vec<_>>();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct TakeoverRow {
    #[tabled(rename = "App")]
    app: String,
    #[tabled(rename = "Takeover")]
    enabled: String,
}

pub fn print_failover_queue(queue: &[cc_switch_core::FailoverQueueItem]) -> anyhow::Result<()> {
    if queue.is_empty() {
        println!("Failover queue is empty.");
        return Ok(());
    }

    let rows: Vec<FailoverRow> = queue
        .iter()
        .map(|item| FailoverRow {
            priority: item.priority.to_string(),
            provider_id: item.provider_id.clone(),
            provider_name: item.provider_name.clone(),
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct FailoverRow {
    #[tabled(rename = "Priority")]
    priority: String,
    #[tabled(rename = "Provider ID")]
    provider_id: String,
    #[tabled(rename = "Provider Name")]
    provider_name: String,
}

pub fn print_provider_health(health: &cc_switch_core::ProviderHealth) -> anyhow::Result<()> {
    println!("Provider: {}", health.provider_id);
    println!("App: {}", health.app_type);
    println!(
        "Healthy: {}",
        if health.is_healthy {
            "Yes".green().to_string()
        } else {
            "No".red().to_string()
        }
    );
    println!("Consecutive failures: {}", health.consecutive_failures);
    if let Some(time) = &health.last_failure_at {
        println!("Last failure: {}", time);
    }
    if let Some(time) = &health.last_success_at {
        println!("Last success: {}", time);
    }
    if let Some(error) = &health.last_error {
        println!("Last error: {}", error);
    }
    Ok(())
}

pub fn print_circuit_breaker_config(
    config: &cc_switch_core::CircuitBreakerConfig,
) -> anyhow::Result<()> {
    println!("Failure threshold: {}", config.failure_threshold);
    println!("Success threshold: {}", config.success_threshold);
    println!("Recovery timeout: {}s", config.timeout_seconds);
    println!("Error rate threshold: {:.2}", config.error_rate_threshold);
    println!("Minimum requests: {}", config.min_requests);
    Ok(())
}

pub fn print_settings(settings: &cc_switch_core::AppSettings) -> anyhow::Result<()> {
    if let Some(lang) = &settings.language {
        println!("Language: {}", lang);
    }
    if let Some(claude_dir) = &settings.claude_config_dir {
        println!("Claude config dir: {}", claude_dir);
    }
    if let Some(codex_dir) = &settings.codex_config_dir {
        println!("Codex config dir: {}", codex_dir);
    }
    if let Some(gemini_dir) = &settings.gemini_config_dir {
        println!("Gemini config dir: {}", gemini_dir);
    }
    if let Some(opencode_dir) = &settings.opencode_config_dir {
        println!("OpenCode config dir: {}", opencode_dir);
    }
    Ok(())
}

pub fn print_usage_summary(summary: &cc_switch_core::UsageSummary) -> anyhow::Result<()> {
    println!("Total requests: {}", summary.total_requests);
    println!("Total tokens: {}", summary.total_tokens);
    println!("Total cost: ${:.4}", summary.total_cost);
    Ok(())
}

pub fn print_usage_logs(logs: &[cc_switch_core::RequestLog]) -> anyhow::Result<()> {
    if logs.is_empty() {
        println!("No request logs found.");
        return Ok(());
    }

    let rows: Vec<LogRow> = logs
        .iter()
        .map(|log| LogRow {
            timestamp: log.timestamp.clone(),
            model: log.model.clone(),
            tokens: log.total_tokens.to_string(),
            cost: format!("${:.4}", log.cost),
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()))
        .to_string();
    println!("{table}");
    Ok(())
}

#[derive(Tabled)]
struct LogRow {
    #[tabled(rename = "Timestamp")]
    timestamp: String,
    #[tabled(rename = "Model")]
    model: String,
    #[tabled(rename = "Tokens")]
    tokens: String,
    #[tabled(rename = "Cost")]
    cost: String,
}
