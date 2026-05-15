use std::str::FromStr;

use crate::app_config::AppType;
use crate::database::Database;
use crate::services::{ProviderService, SwitchResult};
use crate::store::AppState;

/// Signal file name used for CLI → GUI IPC.
pub(crate) const CLI_SWITCH_SIGNAL_FILE: &str = ".cli-switch-signal";

/// Payload written by the CLI after a successful switch, read by the GUI to refresh.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliSwitchSignal {
    pub app_type: String,
    pub provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    List {
        app: Option<AppType>,
        json: bool,
    },
    Switch {
        app: AppType,
        provider_id: String,
        json: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliParseOutcome {
    Command(CliCommand),
    NotCli,
    Help,
    Error(CliError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
    pub exit_code: i32,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn parse_cli_args<I>(args: I) -> CliParseOutcome
where
    I: IntoIterator<Item = String>,
{
    let args: Vec<String> = args.into_iter().collect();

    if args.len() <= 1 {
        return CliParseOutcome::NotCli;
    }

    let command = &args[1];
    if command.starts_with("ccswitch://") {
        return CliParseOutcome::NotCli;
    }

    match command.as_str() {
        "--help" | "-h" | "help" => CliParseOutcome::Help,
        "list" => parse_list_args(&args[2..]),
        "switch" => parse_switch_args(&args[2..]),
        unknown if !unknown.starts_with('-') => CliParseOutcome::Error(CliError {
            code: "unknown_command",
            message: format!("Unknown command: {unknown}"),
            exit_code: 2,
            json: args[2..].iter().any(|a| a == "--json"),
        }),
        _ => {
            let help = help_text();
            usage_error(help.trim_end(), args[2..].iter().any(|a| a == "--json"))
        }
    }
}

fn parse_list_args(args: &[String]) -> CliParseOutcome {
    let mut json = false;
    let mut app = None;

    for arg in args {
        if arg == "--json" {
            json = true;
        } else if arg.starts_with('-') || app.is_some() {
            return usage_error("Usage: cc-switch list [app] [--json]", json);
        } else {
            match AppType::from_str(arg) {
                Ok(parsed) => app = Some(parsed),
                Err(err) => return unsupported_app_error(err.to_string(), json),
            }
        }
    }

    CliParseOutcome::Command(CliCommand::List { app, json })
}

fn parse_switch_args(args: &[String]) -> CliParseOutcome {
    let mut json = false;
    let mut positional = Vec::new();

    for arg in args {
        if arg == "--json" {
            json = true;
        } else if arg.starts_with('-') {
            return usage_error("Usage: cc-switch switch <app> <provider-id> [--json]", json);
        } else {
            positional.push(arg);
        }
    }

    if positional.len() != 2 {
        return usage_error("Usage: cc-switch switch <app> <provider-id> [--json]", json);
    }

    let app = match AppType::from_str(positional[0]) {
        Ok(parsed) => parsed,
        Err(err) => return unsupported_app_error(err.to_string(), json),
    };

    CliParseOutcome::Command(CliCommand::Switch {
        app,
        provider_id: positional[1].clone(),
        json,
    })
}

fn unsupported_app_error(message: String, json: bool) -> CliParseOutcome {
    CliParseOutcome::Error(CliError {
        code: "unsupported_app",
        message,
        exit_code: 2,
        json,
    })
}

fn usage_error(message: &str, json: bool) -> CliParseOutcome {
    CliParseOutcome::Error(CliError {
        code: "usage",
        message: message.to_string(),
        exit_code: 2,
        json,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliProviderRecord {
    pub app: String,
    pub id: String,
    pub name: String,
    pub current: bool,
}

pub fn collect_provider_records(
    db: &Database,
    app: Option<AppType>,
) -> Result<Vec<CliProviderRecord>, crate::error::AppError> {
    let apps: Vec<AppType> = match app {
        Some(app) => vec![app],
        None => AppType::all().collect(),
    };

    let mut records = Vec::new();
    for app in apps {
        let providers = db.get_all_providers(app.as_str())?;
        let local_current_provider_id = crate::settings::get_current_provider(&app);
        let current_provider_id = match local_current_provider_id {
            Some(local_id) if providers.contains_key(&local_id) => Some(local_id),
            _ => db.get_current_provider(app.as_str())?,
        };
        records.extend(
            providers
                .into_iter()
                .map(|(id, provider)| CliProviderRecord {
                    app: app.as_str().to_string(),
                    current: current_provider_id.as_deref() == Some(id.as_str()),
                    id,
                    name: provider.name,
                }),
        );
    }

    Ok(records)
}

pub fn format_list_text(records: &[CliProviderRecord]) -> String {
    let mut output = String::new();
    let mut current_app: Option<&str> = None;

    for record in records {
        if current_app != Some(record.app.as_str()) {
            if current_app.is_some() {
                output.push('\n');
            }
            current_app = Some(record.app.as_str());
            output.push_str(&format!("{}\n", sanitize_text_output(&record.app)));
        }

        let marker = if record.current { '*' } else { ' ' };
        output.push_str(&format!(
            "{marker} {}  {}\n",
            sanitize_text_output(&record.id),
            sanitize_text_output(&record.name)
        ));
    }

    output
}

pub fn format_list_json(records: &[CliProviderRecord]) -> Result<String, crate::error::AppError> {
    serde_json::to_string(records)
        .map_err(|source| crate::error::AppError::JsonSerialize { source })
}

pub fn format_switch_text(app: &AppType, provider_id: &str) -> String {
    format!(
        "Switched {} to {}\n",
        sanitize_text_output(app.as_str()),
        sanitize_text_output(provider_id)
    )
}

pub fn format_switch_json(
    app: &AppType,
    provider_id: &str,
    warnings: &[String],
) -> Result<String, crate::error::AppError> {
    serde_json::to_string(&serde_json::json!({
        "ok": true,
        "app": app.as_str(),
        "providerId": provider_id,
        "warnings": warnings,
    }))
    .map_err(|source| crate::error::AppError::JsonSerialize { source })
}

pub fn execute_switch(
    state: &AppState,
    app: AppType,
    provider_id: &str,
) -> Result<SwitchResult, CliError> {
    let providers = state
        .db
        .get_all_providers(app.as_str())
        .map_err(|err| CliError {
            code: "switch_failed",
            message: err.to_string(),
            exit_code: 1,
            json: false,
        })?;

    if !providers.contains_key(provider_id) {
        return Err(CliError {
            code: "provider_not_found",
            message: format!("Provider not found: {provider_id} for {}", app.as_str()),
            exit_code: 1,
            json: false,
        });
    }

    ProviderService::switch(state, app, provider_id).map_err(|err| CliError {
        code: "switch_failed",
        message: err.to_string(),
        exit_code: 1,
        json: false,
    })
}

pub fn run_cli_args_with_state<I>(state: &AppState, args: I) -> Option<CliOutput>
where
    I: IntoIterator<Item = String>,
{
    match parse_cli_args(args) {
        CliParseOutcome::Command(command) => Some(run_cli_command(state, command)),
        CliParseOutcome::Help => Some(CliOutput {
            stdout: help_text(),
            stderr: String::new(),
            exit_code: 0,
        }),
        CliParseOutcome::Error(error) => Some(error_output(&error, error.json)),
        CliParseOutcome::NotCli => None,
    }
}

pub fn run_if_cli_args<I>(args: I) -> Option<i32>
where
    I: IntoIterator<Item = String>,
{
    let output = match parse_cli_args(args) {
        CliParseOutcome::Command(command) => {
            let json = match &command {
                CliCommand::List { json, .. } => *json,
                CliCommand::Switch { json, .. } => *json,
            };
            let db = match Database::init() {
                Ok(db) => std::sync::Arc::new(db),
                Err(err) => {
                    let output = error_output(
                        &CliError {
                            code: "startup_failed",
                            message: err.to_string(),
                            exit_code: 1,
                            json,
                        },
                        json,
                    );
                    print_cli_output(&output);
                    return Some(output.exit_code);
                }
            };
            let state = AppState::new(db);
            run_cli_command(&state, command)
        }
        CliParseOutcome::Help => CliOutput {
            stdout: help_text(),
            stderr: String::new(),
            exit_code: 0,
        },
        CliParseOutcome::Error(error) => error_output(&error, error.json),
        CliParseOutcome::NotCli => return None,
    };
    print_cli_output(&output);
    Some(output.exit_code)
}

fn print_cli_output(output: &CliOutput) {
    if !output.stdout.is_empty() {
        print!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprint!("{}", output.stderr);
    }
}

fn help_text() -> String {
    "Usage: cc-switch list [app] [--json]\nUsage: cc-switch switch <app> <provider-id> [--json]\n"
        .to_string()
}

pub fn run_cli_command(state: &AppState, command: CliCommand) -> CliOutput {
    match command {
        CliCommand::List { app, json } => match collect_provider_records(state.db.as_ref(), app) {
            Ok(records) => {
                let formatted = if json {
                    format_list_json(&records)
                } else {
                    Ok(format_list_text(&records))
                };
                match formatted {
                    Ok(stdout) => CliOutput {
                        stdout: ensure_trailing_newline(stdout),
                        stderr: String::new(),
                        exit_code: 0,
                    },
                    Err(err) => error_output(
                        &CliError {
                            code: "list_failed",
                            message: err.to_string(),
                            exit_code: 1,
                            json,
                        },
                        json,
                    ),
                }
            }
            Err(err) => error_output(
                &CliError {
                    code: "list_failed",
                    message: err.to_string(),
                    exit_code: 1,
                    json,
                },
                json,
            ),
        },
        CliCommand::Switch {
            app,
            provider_id,
            json,
        } => match execute_switch(state, app.clone(), &provider_id) {
            Ok(result) => {
                write_cli_switch_signal(&app, &provider_id);
                let formatted = if json {
                    format_switch_json(&app, &provider_id, &result.warnings)
                } else {
                    Ok(format_switch_text(&app, &provider_id))
                };
                match formatted {
                    Ok(stdout) => CliOutput {
                        stdout: ensure_trailing_newline(stdout),
                        stderr: String::new(),
                        exit_code: 0,
                    },
                    Err(err) => error_output(
                        &CliError {
                            code: "switch_failed",
                            message: err.to_string(),
                            exit_code: 1,
                            json,
                        },
                        json,
                    ),
                }
            }
            Err(err) => error_output(&CliError {
                code: err.code,
                message: err.message,
                exit_code: err.exit_code,
                json,
            }, json),
        },
    }
}

pub fn error_output(error: &CliError, json: bool) -> CliOutput {
    let stderr = if json {
        serde_json::json!({
            "ok": false,
            "code": error.code,
            "error": error.message,
        })
        .to_string()
            + "\n"
    } else {
        format!("{}\n", sanitize_text_output(&error.message))
    };

    CliOutput {
        stdout: String::new(),
        stderr,
        exit_code: error.exit_code,
    }
}

fn ensure_trailing_newline(mut output: String) -> String {
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

/// Write a signal file so the running GUI can detect the CLI switch and refresh.
/// Best-effort: silently ignores errors since the GUI will eventually see the change
/// via other mechanisms (restart, etc.).
fn write_cli_switch_signal(app_type: &AppType, provider_id: &str) {
    let config_dir = match crate::config::get_app_config_dir() {
        dir if dir.as_os_str().is_empty() => return,
        dir => dir,
    };

    let signal = CliSwitchSignal {
        app_type: app_type.as_str().to_string(),
        provider_id: provider_id.to_string(),
    };
    let content = match serde_json::to_string(&signal) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("Failed to serialize CLI switch signal: {e}");
            return;
        }
    };

    let target = config_dir.join(CLI_SWITCH_SIGNAL_FILE);
    let tmp = config_dir.join(format!("{CLI_SWITCH_SIGNAL_FILE}.tmp"));

    // Ensure config dir exists (may not on fresh test environments)
    if let Err(e) = std::fs::create_dir_all(&config_dir) {
        log::debug!("Failed to create config dir for CLI switch signal: {e}");
        return;
    }

    // Atomic write: write to tmp then rename
    if let Err(e) = std::fs::write(&tmp, &content) {
        log::debug!("Failed to write CLI switch signal tmp file: {e}");
        return;
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        log::debug!("Failed to rename CLI switch signal file: {e}");
    } else {
        log::debug!("CLI switch signal written for GUI refresh");
    }
}

fn sanitize_text_output(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { '?' } else { ch })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::Provider;
    use serial_test::serial;
    use std::env;
    use std::sync::{Arc, Mutex, OnceLock};
    use tempfile::TempDir;

    fn parse(args: &[&str]) -> CliParseOutcome {
        parse_cli_args(args.iter().map(|arg| arg.to_string()))
    }

    struct CurrentProviderSettingsSnapshot {
        values: Vec<(AppType, Option<String>)>,
    }

    impl CurrentProviderSettingsSnapshot {
        fn capture() -> Self {
            Self {
                values: AppType::all()
                    .map(|app| {
                        let current_provider = crate::settings::get_current_provider(&app);
                        (app, current_provider)
                    })
                    .collect(),
            }
        }
    }

    impl Drop for CurrentProviderSettingsSnapshot {
        fn drop(&mut self) {
            for (app, current_provider) in &self.values {
                crate::settings::set_current_provider(app, current_provider.as_deref())
                    .expect("restore current provider setting");
            }
        }
    }

    fn settings_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    struct TempHome {
        _dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload temp settings");

            Self {
                _dir: dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            crate::settings::reload_settings().expect("restore settings");
        }
    }

    fn provider(id: &str, name: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            name.to_string(),
            serde_json::json!({}),
            None,
        )
    }

    fn claude_provider(id: &str, name: &str, api_key: &str) -> Provider {
        Provider::with_id(
            id.to_string(),
            name.to_string(),
            serde_json::json!({
                "env": {
                    "ANTHROPIC_API_KEY": api_key,
                },
            }),
            None,
        )
    }

    #[test]
    #[serial]
    fn collect_provider_records_lists_one_app_and_marks_effective_current() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, None)
            .expect("clear local current provider");

        let db = Database::memory().expect("create memory db");
        db.save_provider("claude", &provider("p1", "PackyCode"))
            .expect("save p1");
        db.save_provider("claude", &provider("p2", "OpenRouter"))
            .expect("save p2");
        db.set_current_provider("claude", "p2")
            .expect("set db current provider");

        let records =
            collect_provider_records(&db, Some(AppType::Claude)).expect("collect records");

        assert_eq!(
            records,
            vec![
                CliProviderRecord {
                    app: "claude".to_string(),
                    id: "p1".to_string(),
                    name: "PackyCode".to_string(),
                    current: false,
                },
                CliProviderRecord {
                    app: "claude".to_string(),
                    id: "p2".to_string(),
                    name: "OpenRouter".to_string(),
                    current: true,
                },
            ]
        );
    }

    #[test]
    #[serial]
    fn collect_provider_records_lists_all_apps_in_app_type_order() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, None)
            .expect("clear claude current provider");
        crate::settings::set_current_provider(&AppType::Gemini, None)
            .expect("clear gemini current provider");

        let db = Database::memory().expect("create memory db");
        db.save_provider("claude", &provider("claude-p1", "Claude Provider"))
            .expect("save claude provider");
        db.save_provider("gemini", &provider("gemini-p1", "Gemini Provider"))
            .expect("save gemini provider");

        let records = collect_provider_records(&db, None).expect("collect records");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].app, "claude");
        assert_eq!(records[0].id, "claude-p1");
        assert_eq!(records[1].app, "gemini");
        assert_eq!(records[1].id, "gemini-p1");
    }

    #[test]
    #[serial]
    fn collect_provider_records_does_not_clear_stale_local_current_provider() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, Some("stale"))
            .expect("set stale local current provider");

        let db = Database::memory().expect("create memory db");
        db.save_provider("claude", &provider("p1", "PackyCode"))
            .expect("save p1");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");

        let records =
            collect_provider_records(&db, Some(AppType::Claude)).expect("collect records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "p1");
        assert!(records[0].current);
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude),
            Some("stale".to_string())
        );
    }

    #[test]
    fn format_list_text_groups_by_app_and_marks_current() {
        let records = vec![
            CliProviderRecord {
                app: "claude".to_string(),
                id: "claude-1".to_string(),
                name: "Claude One".to_string(),
                current: true,
            },
            CliProviderRecord {
                app: "claude".to_string(),
                id: "claude-2".to_string(),
                name: "Claude Two".to_string(),
                current: false,
            },
            CliProviderRecord {
                app: "codex".to_string(),
                id: "codex-1".to_string(),
                name: "Codex One".to_string(),
                current: true,
            },
        ];

        assert_eq!(
            format_list_text(&records),
            "claude\n* claude-1  Claude One\n  claude-2  Claude Two\n\ncodex\n* codex-1  Codex One\n"
        );
    }

    #[test]
    fn format_list_json_uses_camel_case_records() {
        let records = vec![CliProviderRecord {
            app: "claude".to_string(),
            id: "provider-1".to_string(),
            name: "Provider One".to_string(),
            current: true,
        }];

        assert_eq!(
            format_list_json(&records).unwrap(),
            r#"[{"app":"claude","id":"provider-1","name":"Provider One","current":true}]"#
        );
    }

    #[test]
    fn format_switch_text_includes_app_and_provider_id() {
        assert_eq!(
            format_switch_text(&AppType::Claude, "provider-1"),
            "Switched claude to provider-1\n"
        );
    }

    #[test]
    fn format_switch_json_includes_warnings() {
        let warnings = vec!["config warning".to_string()];

        assert_eq!(
            format_switch_json(&AppType::Codex, "provider-2", &warnings).unwrap(),
            r#"{"ok":true,"app":"codex","providerId":"provider-2","warnings":["config warning"]}"#
        );
    }

    #[test]
    #[serial]
    fn execute_switch_updates_current_provider_by_id() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &claude_provider("p1", "Claude One", "key-one"))
            .expect("save p1");
        db.save_provider("claude", &claude_provider("p2", "Claude Two", "key-two"))
            .expect("save p2");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");

        let result = execute_switch(&state, AppType::Claude, "p2").expect("switch provider");

        assert!(result.warnings.is_empty());
        assert_eq!(
            db.get_current_provider("claude")
                .expect("get db current provider"),
            Some("p2".to_string())
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Claude),
            Some("p2".to_string())
        );
    }

    #[test]
    #[serial]
    fn execute_switch_maps_missing_provider_to_provider_not_found_error() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db);

        let error = execute_switch(&state, AppType::Claude, "missing").unwrap_err();

        assert_eq!(error.code, "provider_not_found");
        assert_eq!(error.exit_code, 1);
        assert_eq!(error.message, "Provider not found: missing for claude");
    }

    #[test]
    #[serial]
    fn run_cli_command_outputs_text_list() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, None)
            .expect("clear local current provider");

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &provider("p1", "PackyCode"))
            .expect("save p1");
        db.save_provider("claude", &provider("p2", "OpenRouter"))
            .expect("save p2");
        db.set_current_provider("claude", "p2")
            .expect("set db current provider");

        let output = run_cli_command(
            &state,
            CliCommand::List {
                app: Some(AppType::Claude),
                json: false,
            },
        );

        assert_eq!(
            output,
            CliOutput {
                stdout: "claude\n  p1  PackyCode\n* p2  OpenRouter\n".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn run_cli_command_outputs_json_list() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, None)
            .expect("clear local current provider");

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &provider("p1", "PackyCode"))
            .expect("save p1");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");

        let output = run_cli_command(
            &state,
            CliCommand::List {
                app: Some(AppType::Claude),
                json: true,
            },
        );

        assert_eq!(
            output,
            CliOutput {
                stdout: r#"[{"app":"claude","id":"p1","name":"PackyCode","current":true}]"#
                    .to_string()
                    + "\n",
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn run_cli_command_outputs_text_switch_success() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &claude_provider("p1", "Claude One", "key-one"))
            .expect("save p1");
        db.save_provider("claude", &claude_provider("p2", "Claude Two", "key-two"))
            .expect("save p2");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");

        let output = run_cli_command(
            &state,
            CliCommand::Switch {
                app: AppType::Claude,
                provider_id: "p2".to_string(),
                json: false,
            },
        );

        assert_eq!(
            output,
            CliOutput {
                stdout: "Switched claude to p2\n".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn run_cli_command_outputs_json_switch_success() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &claude_provider("p1", "Claude One", "key-one"))
            .expect("save p1");
        db.save_provider("claude", &claude_provider("p2", "Claude Two", "key-two"))
            .expect("save p2");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");

        let output = run_cli_command(
            &state,
            CliCommand::Switch {
                app: AppType::Claude,
                provider_id: "p2".to_string(),
                json: true,
            },
        );
        let json: serde_json::Value = serde_json::from_str(output.stdout.trim_end())
            .expect("switch success output should be json");

        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert_eq!(json["ok"], true);
        assert_eq!(json["app"], "claude");
        assert_eq!(json["providerId"], "p2");
        assert!(json["warnings"].is_array());
    }

    #[test]
    #[serial]
    fn run_cli_command_outputs_json_error_to_stderr() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db);

        let output = run_cli_command(
            &state,
            CliCommand::Switch {
                app: AppType::Claude,
                provider_id: "missing".to_string(),
                json: true,
            },
        );

        assert_eq!(
            output,
            CliOutput {
                stdout: String::new(),
                stderr: r#"{"ok":false,"code":"provider_not_found","error":"Provider not found: missing for claude"}"#
                    .to_string()
                    + "\n",
                exit_code: 1,
            }
        );
    }

    #[test]
    fn run_cli_args_with_state_returns_none_for_gui_launch_without_subcommand() {
        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db);

        assert_eq!(
            run_cli_args_with_state(&state, vec!["cc-switch".to_string()]),
            None
        );
    }

    #[test]
    fn run_cli_args_with_state_outputs_help_without_running_gui() {
        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db);

        let output =
            run_cli_args_with_state(&state, vec!["cc-switch".to_string(), "--help".to_string()])
                .expect("help should be handled by cli");

        assert_eq!(output.stderr, "");
        assert_eq!(output.exit_code, 0);
        assert!(output
            .stdout
            .contains("Usage: cc-switch list [app] [--json]"));
        assert!(output
            .stdout
            .contains("Usage: cc-switch switch <app> <provider-id> [--json]"));
    }

    #[test]
    #[serial]
    fn run_cli_args_with_state_executes_list_command() {
        let _guard = settings_test_guard();
        crate::settings::reload_settings().expect("reload settings");
        let _settings_snapshot = CurrentProviderSettingsSnapshot::capture();
        crate::settings::set_current_provider(&AppType::Claude, None)
            .expect("clear local current provider");

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &provider("p1", "PackyCode"))
            .expect("save p1");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");

        let output = run_cli_args_with_state(
            &state,
            vec![
                "cc-switch".to_string(),
                "list".to_string(),
                "claude".to_string(),
            ],
        )
        .expect("list should be handled by cli");

        assert_eq!(
            output,
            CliOutput {
                stdout: "claude\n* p1  PackyCode\n".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn run_cli_args_with_state_lists_switches_then_lists_changed_current_provider() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        let db = Arc::new(Database::memory().expect("create memory db"));
        let state = AppState::new(db.clone());
        db.save_provider("claude", &claude_provider("p1", "Claude One", "key-one"))
            .expect("save p1");
        db.save_provider("claude", &claude_provider("p2", "Claude Two", "key-two"))
            .expect("save p2");
        db.set_current_provider("claude", "p1")
            .expect("set db current provider");
        crate::settings::set_current_provider(&AppType::Claude, Some("p1"))
            .expect("set local current provider");

        let first_list = run_cli_args_with_state(
            &state,
            vec![
                "cc-switch".to_string(),
                "list".to_string(),
                "claude".to_string(),
            ],
        )
        .expect("first list should be handled by cli");

        assert_eq!(first_list.stderr, "");
        assert_eq!(first_list.exit_code, 0);
        assert_eq!(
            first_list.stdout,
            "claude\n* p1  Claude One\n  p2  Claude Two\n"
        );

        let switch = run_cli_args_with_state(
            &state,
            vec![
                "cc-switch".to_string(),
                "switch".to_string(),
                "claude".to_string(),
                "p2".to_string(),
            ],
        )
        .expect("switch should be handled by cli");

        assert_eq!(
            switch,
            CliOutput {
                stdout: "Switched claude to p2\n".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }
        );

        let second_list = run_cli_args_with_state(
            &state,
            vec![
                "cc-switch".to_string(),
                "list".to_string(),
                "claude".to_string(),
            ],
        )
        .expect("second list should be handled by cli");

        assert_eq!(second_list.stderr, "");
        assert_eq!(second_list.exit_code, 0);
        assert_eq!(
            second_list.stdout,
            "claude\n  p1  Claude One\n* p2  Claude Two\n"
        );
    }

    #[test]
    fn text_output_escapes_terminal_control_characters() {
        let records = vec![CliProviderRecord {
            app: "claude".to_string(),
            id: "bad\u{1b}[31m".to_string(),
            name: "Name\u{7}Hidden\nNext".to_string(),
            current: true,
        }];

        let output = format_list_text(&records);

        assert!(!output.contains('\u{1b}'));
        assert!(!output.contains('\u{7}'));
        assert!(!output.contains("\nNext"));
        assert_eq!(output, "claude\n* bad?[31m  Name?Hidden?Next\n");
    }

    #[test]
    fn text_error_output_escapes_terminal_control_characters() {
        let output = error_output(
            &CliError {
                code: "provider_not_found",
                message: "Provider not found: bad\u{1b}]52;c;secret\u{7}".to_string(),
                exit_code: 1,
                json: false,
            },
            false,
        );

        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, "Provider not found: bad?]52;c;secret?\n");
        assert_eq!(output.exit_code, 1);
    }

    #[test]
    fn json_error_output_preserves_escaped_content() {
        let output = error_output(
            &CliError {
                code: "provider_not_found",
                message: "Provider not found: bad\u{1b}".to_string(),
                exit_code: 1,
                json: true,
            },
            true,
        );

        assert!(output.stderr.contains("\\u001b"));
    }

    #[test]
    fn parse_returns_not_cli_for_gui_launch_without_subcommand() {
        assert_eq!(parse(&["cc-switch"]), CliParseOutcome::NotCli);
    }

    #[test]
    fn parse_returns_not_cli_for_deeplink_argument() {
        assert_eq!(
            parse(&["cc-switch", "ccswitch://provider/import?token=redacted"]),
            CliParseOutcome::NotCli
        );
    }

    #[test]
    fn parse_list_without_app() {
        assert_eq!(
            parse(&["cc-switch", "list"]),
            CliParseOutcome::Command(CliCommand::List {
                app: None,
                json: false,
            })
        );
    }

    #[test]
    fn parse_list_with_app_and_json_flag() {
        assert_eq!(
            parse(&["cc-switch", "list", "claude", "--json"]),
            CliParseOutcome::Command(CliCommand::List {
                app: Some(AppType::Claude),
                json: true,
            })
        );
    }

    #[test]
    fn parse_list_with_json_before_app() {
        assert_eq!(
            parse(&["cc-switch", "list", "--json", "codex"]),
            CliParseOutcome::Command(CliCommand::List {
                app: Some(AppType::Codex),
                json: true,
            })
        );
    }

    #[test]
    fn parse_switch_with_provider_id() {
        assert_eq!(
            parse(&["cc-switch", "switch", "gemini", "provider-1"]),
            CliParseOutcome::Command(CliCommand::Switch {
                app: AppType::Gemini,
                provider_id: "provider-1".to_string(),
                json: false,
            })
        );
    }

    #[test]
    fn parse_switch_with_json_flag() {
        assert_eq!(
            parse(&["cc-switch", "switch", "--json", "opencode", "provider-2"]),
            CliParseOutcome::Command(CliCommand::Switch {
                app: AppType::OpenCode,
                provider_id: "provider-2".to_string(),
                json: true,
            })
        );
    }

    #[test]
    fn parse_unknown_cli_like_command_is_error() {
        assert_eq!(
            parse(&["cc-switch", "providers"]),
            CliParseOutcome::Error(CliError {
                code: "unknown_command",
                message: "Unknown command: providers".to_string(),
                exit_code: 2,
                json: false,
            })
        );
    }

    #[test]
    fn parse_switch_missing_provider_is_error() {
        assert_eq!(
            parse(&["cc-switch", "switch", "claude"]),
            CliParseOutcome::Error(CliError {
                code: "usage",
                message: "Usage: cc-switch switch <app> <provider-id> [--json]".to_string(),
                exit_code: 2,
                json: false,
            })
        );
    }

    #[test]
    fn parse_list_rejects_second_app_argument() {
        assert_eq!(
            parse(&["cc-switch", "list", "claude", "codex"]),
            CliParseOutcome::Error(CliError {
                code: "usage",
                message: "Usage: cc-switch list [app] [--json]".to_string(),
                exit_code: 2,
                json: false,
            })
        );
    }

    #[test]
    fn parse_error_carries_json_flag_when_json_requested() {
        let result = parse(&["cc-switch", "switch", "claude", "--json"]);
        match result {
            CliParseOutcome::Error(err) => {
                assert_eq!(err.code, "usage");
                assert!(err.json);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_command_with_json_carries_json_flag() {
        let result = parse(&["cc-switch", "bogus", "--json"]);
        match result {
            CliParseOutcome::Error(err) => {
                assert_eq!(err.code, "unknown_command");
                assert!(err.json);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn parse_unrecognized_flag_shows_combined_usage() {
        let result = parse(&["cc-switch", "--foo"]);
        match result {
            CliParseOutcome::Error(err) => {
                assert_eq!(err.code, "usage");
                assert!(err.message.contains("list"));
                assert!(err.message.contains("switch"));
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn error_output_uses_json_when_error_has_json_flag() {
        let output = error_output(
            &CliError {
                code: "usage",
                message: "bad input".to_string(),
                exit_code: 2,
                json: true,
            },
            true,
        );

        let parsed: serde_json::Value =
            serde_json::from_str(output.stderr.trim()).expect("stderr should be valid json");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["code"], "usage");
    }

    #[test]
    #[serial]
    fn write_cli_switch_signal_creates_readable_file() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        write_cli_switch_signal(&AppType::Claude, "test-provider");

        let config_dir = crate::config::get_app_config_dir();
        let signal_path = config_dir.join(CLI_SWITCH_SIGNAL_FILE);
        assert!(signal_path.exists(), "signal file should exist after write");

        let content = std::fs::read_to_string(&signal_path).expect("read signal file");
        let signal: CliSwitchSignal =
            serde_json::from_str(&content).expect("parse signal file");
        assert_eq!(signal.app_type, "claude");
        assert_eq!(signal.provider_id, "test-provider");

        // Clean up
        let _ = std::fs::remove_file(&signal_path);
    }

    #[test]
    #[serial]
    fn write_cli_switch_signal_is_atomic_no_tmp_left() {
        let _guard = settings_test_guard();
        let _home = TempHome::new();

        write_cli_switch_signal(&AppType::Codex, "p1");

        let config_dir = crate::config::get_app_config_dir();
        let tmp_path = config_dir.join(format!("{CLI_SWITCH_SIGNAL_FILE}.tmp"));
        assert!(!tmp_path.exists(), "tmp file should not linger after write");

        // Clean up
        let signal_path = config_dir.join(CLI_SWITCH_SIGNAL_FILE);
        let _ = std::fs::remove_file(&signal_path);
    }
}
