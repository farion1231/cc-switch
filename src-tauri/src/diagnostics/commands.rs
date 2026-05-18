use crate::config::get_app_config_dir;
use crate::diagnostics::cli_check::check_cli;
use crate::diagnostics::dependency_check::check_dependencies;
use crate::diagnostics::path_check::check_paths;
use crate::diagnostics::permission_check::check_permissions;
use crate::diagnostics::port_check::check_ports;
use crate::diagnostics::report::{
    export_zip, redact_value, DiagnosticCheck, DiagnosticError, DiagnosticReport,
};
use crate::diagnostics::system_check::check_system;
use crate::store::AppState;
use serde_json::json;
use std::collections::BTreeMap;
use tauri::State;

type CommandResult<T> = Result<T, DiagnosticError>;

#[tauri::command]
pub async fn diagnostics_run_all(state: State<'_, AppState>) -> CommandResult<DiagnosticReport> {
    Ok(run_all_checks(&state))
}

#[tauri::command]
pub async fn diagnostics_check_dependencies() -> CommandResult<Vec<DiagnosticCheck>> {
    let mut checks = check_dependencies();
    checks.extend(check_cli());
    Ok(checks)
}

#[tauri::command]
pub async fn diagnostics_check_ports() -> CommandResult<Vec<DiagnosticCheck>> {
    Ok(check_ports())
}

#[tauri::command]
pub async fn diagnostics_check_permissions() -> CommandResult<Vec<DiagnosticCheck>> {
    Ok(check_permissions())
}

#[tauri::command]
pub async fn diagnostics_export_report(state: State<'_, AppState>) -> CommandResult<String> {
    let report = run_all_checks(&state);
    let export_dir = get_app_config_dir().join("diagnostics");
    let extra_files = build_export_files(&state, &report);
    let path = export_zip(&export_dir, &report, extra_files)?;
    Ok(path.display().to_string())
}

fn run_all_checks(state: &State<'_, AppState>) -> DiagnosticReport {
    let mut checks = Vec::new();
    checks.extend(check_system());
    checks.extend(check_dependencies());
    checks.extend(check_cli());
    checks.extend(check_paths());
    checks.extend(check_permissions());
    checks.extend(check_ports());
    checks.extend(check_database(state));
    DiagnosticReport::new(checks)
}

fn check_database(state: &State<'_, AppState>) -> Vec<DiagnosticCheck> {
    let mut checks = Vec::new();
    match state.db.schema_version() {
        Ok(version) if version == crate::Database::expected_schema_version() => {
            checks.push(
                DiagnosticCheck::ok(
                    "database_schema_version",
                    "Database schema version",
                    format!("Schema version is {version}"),
                )
                .with_details(json!({
                    "actual": version,
                    "expected": crate::Database::expected_schema_version()
                })),
            );
        }
        Ok(version) => checks.push(
            DiagnosticCheck::error(
                "database_schema_version",
                "Database schema version",
                format!(
                    "Schema version is {version}, expected {}",
                    crate::Database::expected_schema_version()
                ),
                "Restart the app to re-run migration. Agent Gateway should remain degraded until schema v11 is ready.",
            )
            .with_details(json!({
                "actual": version,
                "expected": crate::Database::expected_schema_version()
            })),
        ),
        Err(error) => checks.push(
            DiagnosticCheck::error(
                "database_readability",
                "Database readability",
                "Database schema version could not be read.",
                "Export diagnostics and restore from backup if the issue persists.",
            )
            .with_details(json!({ "error": error.to_string() })),
        ),
    }
    if state.db.verify_agent_gateway_schema_ready().is_err() {
        checks.push(DiagnosticCheck::warning(
            "agent_gateway_schema",
            "Agent Gateway schema",
            "Agent Gateway tables are not fully ready.",
            "Agent Gateway will be disabled or degraded; existing Provider/MCP/Skills/Sessions/Usage pages remain usable.",
        ));
    } else {
        checks.push(DiagnosticCheck::ok(
            "agent_gateway_schema",
            "Agent Gateway schema",
            "Agent Gateway tables are ready.",
        ));
    }
    checks
}

fn build_export_files(
    state: &State<'_, AppState>,
    report: &DiagnosticReport,
) -> BTreeMap<&'static str, String> {
    let mut files = BTreeMap::new();
    files.insert(
        "app.log",
        "App log collection is not configured in this build.\n".to_string(),
    );
    files.insert(
        "agent_gateway.log",
        serde_json::to_string_pretty(
            &state
                .db
                .list_agent_instances()
                .map(|agents| json!({ "agents": agents }))
                .unwrap_or_else(|error| json!({ "error": error.to_string() })),
        )
        .unwrap_or_default(),
    );
    files.insert(
        "proxy.log",
        "Proxy runtime log collection is not configured in this build.\n".to_string(),
    );
    files.insert(
        "db_schema.json",
        serde_json::to_string_pretty(&json!({
            "actual": state.db.schema_version().ok(),
            "expected": crate::Database::expected_schema_version()
        }))
        .unwrap_or_default(),
    );
    let providers = state
        .db
        .get_all_providers("claude")
        .map(|providers| redact_value(&json!(providers)))
        .unwrap_or_else(|error| json!({ "error": error.to_string() }));
    files.insert(
        "providers_redacted.json",
        serde_json::to_string_pretty(&providers).unwrap_or_default(),
    );
    files.insert(
        "environment.txt",
        format!(
            "generated_at={}\nos={}\narch={}\napp_config_dir={}\n",
            report.generated_at,
            std::env::consts::OS,
            std::env::consts::ARCH,
            get_app_config_dir().display()
        ),
    );
    files.insert(
        "port_status.txt",
        serde_json::to_string_pretty(&check_ports()).unwrap_or_default(),
    );
    files.insert(
        "process_status.txt",
        "Process snapshot collection is limited to Agent Gateway commands in this build.\n"
            .to_string(),
    );
    files
}
