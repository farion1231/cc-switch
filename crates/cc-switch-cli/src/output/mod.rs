//! Output formatting module

mod json;
mod table;
mod yaml;

use crate::cli::OutputFormat;
use serde::Serialize;

pub struct Printer {
    format: OutputFormat,
    quiet: bool,
    verbose: bool,
}

impl Printer {
    pub fn new(format: OutputFormat, quiet: bool, verbose: bool) -> Self {
        Self {
            format,
            quiet,
            verbose,
        }
    }

    pub fn success(&self, message: impl AsRef<str>) {
        if !self.quiet {
            println!("{}", message.as_ref());
        }
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        if !self.quiet {
            eprintln!("{}", message.as_ref());
        }
    }

    pub fn print_text(&self, text: impl AsRef<str>) -> anyhow::Result<()> {
        if !self.quiet {
            println!("{}", text.as_ref());
        }
        Ok(())
    }

    pub fn verbose(&self, message: impl AsRef<str>) {
        if self.verbose && !self.quiet {
            eprintln!("{}", message.as_ref());
        }
    }

    fn is_quiet(&self) -> bool {
        self.quiet
    }

    pub fn print_providers(
        &self,
        providers: &indexmap::IndexMap<String, cc_switch_core::Provider>,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_providers(providers),
            OutputFormat::Json => json::print_providers(providers),
            OutputFormat::Yaml => yaml::print_providers(providers),
        }
    }

    pub fn print_provider_detail(&self, provider: &cc_switch_core::Provider) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_provider_detail(provider),
            OutputFormat::Json => json::print_provider_detail(provider),
            OutputFormat::Yaml => yaml::print_provider_detail(provider),
        }
    }

    pub fn print_universal_providers(
        &self,
        providers: &std::collections::HashMap<String, cc_switch_core::UniversalProvider>,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_universal_providers(providers),
            OutputFormat::Json => json::print_universal_providers(providers),
            OutputFormat::Yaml => yaml::print_universal_providers(providers),
        }
    }

    pub fn print_mcp_servers(
        &self,
        servers: &indexmap::IndexMap<String, cc_switch_core::McpServer>,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_mcp_servers(servers),
            OutputFormat::Json => json::print_mcp_servers(servers),
            OutputFormat::Yaml => yaml::print_mcp_servers(servers),
        }
    }

    pub fn print_mcp_server_detail(
        &self,
        server: &cc_switch_core::McpServer,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_mcp_server_detail(server),
            OutputFormat::Json => json::print_mcp_server_detail(server),
            OutputFormat::Yaml => yaml::print_mcp_server_detail(server),
        }
    }

    pub fn print_prompts(
        &self,
        prompts: &indexmap::IndexMap<String, cc_switch_core::Prompt>,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_prompts(prompts),
            OutputFormat::Json => json::print_prompts(prompts),
            OutputFormat::Yaml => yaml::print_prompts(prompts),
        }
    }

    pub fn print_prompt_detail(&self, prompt: &cc_switch_core::Prompt) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_prompt_detail(prompt),
            OutputFormat::Json => json::print_prompt_detail(prompt),
            OutputFormat::Yaml => yaml::print_prompt_detail(prompt),
        }
    }

    pub fn print_skills(&self, skills: &[cc_switch_core::InstalledSkill]) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_skills(skills),
            OutputFormat::Json => json::print_skills(skills),
            OutputFormat::Yaml => yaml::print_skills(skills),
        }
    }

    pub fn print_proxy_status(&self, status: &cc_switch_core::ProxyStatus) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_proxy_status(status),
            OutputFormat::Json => json::print_proxy_status(status),
            OutputFormat::Yaml => yaml::print_proxy_status(status),
        }
    }

    pub fn print_proxy_config(&self, config: &cc_switch_core::ProxyConfig) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_proxy_config(config),
            OutputFormat::Json => json::print_proxy_config(config),
            OutputFormat::Yaml => yaml::print_proxy_config(config),
        }
    }

    pub fn print_takeover_status(
        &self,
        status: &cc_switch_core::ProxyTakeoverStatus,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_takeover_status(status),
            OutputFormat::Json => json::print_takeover_status(status),
            OutputFormat::Yaml => yaml::print_takeover_status(status),
        }
    }

    pub fn print_failover_queue(
        &self,
        queue: &[cc_switch_core::FailoverQueueItem],
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_failover_queue(queue),
            OutputFormat::Json => json::print_failover_queue(queue),
            OutputFormat::Yaml => yaml::print_failover_queue(queue),
        }
    }

    pub fn print_provider_health(
        &self,
        health: &cc_switch_core::ProviderHealth,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_provider_health(health),
            OutputFormat::Json => json::print_provider_health(health),
            OutputFormat::Yaml => yaml::print_provider_health(health),
        }
    }

    pub fn print_circuit_breaker_config(
        &self,
        config: &cc_switch_core::CircuitBreakerConfig,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_circuit_breaker_config(config),
            OutputFormat::Json => json::print_circuit_breaker_config(config),
            OutputFormat::Yaml => yaml::print_circuit_breaker_config(config),
        }
    }

    pub fn print_settings(&self, settings: &cc_switch_core::AppSettings) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_settings(settings),
            OutputFormat::Json => json::print_settings(settings),
            OutputFormat::Yaml => yaml::print_settings(settings),
        }
    }

    pub fn print_usage_summary(
        &self,
        summary: &cc_switch_core::UsageSummary,
    ) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_usage_summary(summary),
            OutputFormat::Json => json::print_usage_summary(summary),
            OutputFormat::Yaml => yaml::print_usage_summary(summary),
        }
    }

    pub fn print_usage_logs(&self, logs: &[cc_switch_core::RequestLog]) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table => table::print_usage_logs(logs),
            OutputFormat::Json => json::print_usage_logs(logs),
            OutputFormat::Yaml => yaml::print_usage_logs(logs),
        }
    }

    pub fn print_value<T: Serialize>(&self, value: &T) -> anyhow::Result<()> {
        if self.is_quiet() {
            return Ok(());
        }
        match self.format {
            OutputFormat::Table | OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(value)?);
            }
            OutputFormat::Yaml => {
                print!("{}", serde_yaml::to_string(value)?);
            }
        }
        Ok(())
    }
}
