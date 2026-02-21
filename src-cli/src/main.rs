mod output;
mod tui;

use anyhow::{Context, Result};
use cc_switch_lib::{AppState, AppType, Database, ProviderService};
use clap::{Parser, Subcommand};
use output::Format;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "ccswitch", version, about = "Quick provider switcher for cc-switch")]
struct Cli {
    #[arg(long, default_value = "table", global = true)]
    format: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// List all providers
    #[command(alias = "ls")]
    List,
    /// Switch provider: ccswitch use <app> <num>
    Use {
        /// App name (claude, codex, gemini, opencode, openclaw)
        app: String,
        /// Provider number from list
        num: usize,
    },
    /// Show version
    Version,
}

fn init_state() -> Result<AppState> {
    let db = Database::init().context("Failed to init database")?;
    Ok(AppState::new(Arc::new(db)))
}

fn parse_app(s: &str) -> Result<AppType> {
    s.parse::<AppType>()
        .map_err(|e| anyhow::anyhow!("{}", e))
}

fn extract_str(cfg: &serde_json::Value, paths: &[&[&str]]) -> String {
    for path in paths {
        let mut v = cfg;
        let mut found = true;
        for key in *path {
            match v.get(key) {
                Some(next) => v = next,
                None => { found = false; break; }
            }
        }
        if found {
            if let Some(s) = v.as_str() {
                if !s.is_empty() { return s.to_string(); }
            }
        }
    }
    String::new()
}

fn mask_key(key: &str) -> String {
    if key.len() <= 10 { return "*".repeat(key.len()); }
    format!("{}****{}", &key[..6], &key[key.len()-4..])
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let state = init_state()?;

    match cli.command {
        // No subcommand → launch TUI
        None => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(tui::run(state))?;
        }
        Some(cmd) => run_cli(cmd, &cli.format, &state)?,
    }
    Ok(())
}

fn run_cli(cmd: Commands, format: &str, state: &AppState) -> Result<()> {
    let fmt = Format::from_str(format);
    match cmd {
        Commands::Version => {
            println!("ccswitch {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::List => {
            for app_type in AppType::all() {
                let providers = ProviderService::list(state, app_type.clone()).unwrap_or_default();
                if providers.is_empty() { continue; }
                let current_id = ProviderService::current(state, app_type.clone()).unwrap_or_default();
                println!("\n[{}]", app_type.as_str());
                let rows: Vec<Vec<String>> = providers
                    .iter()
                    .enumerate()
                    .map(|(i, (id, p))| {
                        let cfg = &p.settings_config;
                        let marker = if id == &current_id { "→" } else { "" };
                        let url = extract_str(cfg, &[
                            &["env", "ANTHROPIC_BASE_URL"],
                            &["env", "GOOGLE_GEMINI_BASE_URL"],
                            &["baseUrl"], &["base_url"], &["baseURL"],
                            &["apiEndpoint"],
                        ]);
                        let key_raw = extract_str(cfg, &[
                            &["env", "ANTHROPIC_AUTH_TOKEN"],
                            &["env", "ANTHROPIC_API_KEY"],
                            &["env", "OPENROUTER_API_KEY"],
                            &["env", "OPENAI_API_KEY"],
                            &["env", "GEMINI_API_KEY"],
                            &["auth", "OPENAI_API_KEY"],
                            &["apiKey"], &["api_key"],
                        ]);
                        let key = if key_raw.is_empty() { String::new() } else { mask_key(&key_raw) };
                        vec![marker.into(), (i + 1).to_string(), p.name.clone(), url, key]
                    })
                    .collect();
                output::print_table(&["", "#", "Name", "URL", "Key"], rows, fmt);
            }
        }
        Commands::Use { app, num } => {
            let app_type = parse_app(&app)?;
            let providers = ProviderService::list(state, app_type.clone())?;
            let id = providers.keys().nth(num - 1)
                .ok_or_else(|| anyhow::anyhow!("Invalid number {num}, max is {}", providers.len()))?;
            ProviderService::switch(state, app_type, id)?;
            let name = providers.get(id).map(|p| p.name.as_str()).unwrap_or(id);
            println!("Switched {app} → {name}");
        }
    }
    Ok(())
}
