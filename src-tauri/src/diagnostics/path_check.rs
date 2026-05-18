use crate::config::get_app_config_dir;
use crate::diagnostics::report::DiagnosticCheck;
use serde_json::json;

pub fn check_paths() -> Vec<DiagnosticCheck> {
    let mut checks = Vec::new();
    checks.push(path_exists_or_warning(
        "appdata_path",
        "AppData directory",
        dirs::data_dir(),
        "Windows AppData is unavailable.",
        "Repair the user profile or run from a normal Windows desktop session.",
    ));
    checks.push(path_exists_or_warning(
        "userprofile_path",
        "USERPROFILE directory",
        dirs::home_dir(),
        "USERPROFILE is unavailable.",
        "Repair the Windows user profile environment.",
    ));

    let config_dir = get_app_config_dir();
    checks.push(if config_dir.exists() {
        DiagnosticCheck::ok(
            "cc_switch_config",
            "Existing CC Switch config",
            format!("Config directory is readable: {}", config_dir.display()),
        )
    } else {
        DiagnosticCheck::warning(
            "cc_switch_config",
            "Existing CC Switch config",
            format!(
                "Config directory does not exist yet: {}",
                config_dir.display()
            ),
            "The app can create this directory on first use.",
        )
    });

    let db_path = config_dir.join("cc-switch.db");
    checks.push(if db_path.exists() {
        DiagnosticCheck::ok(
            "database_path",
            "Database file",
            format!("Database file exists: {}", db_path.display()),
        )
        .with_details(json!({ "path": db_path }))
    } else {
        DiagnosticCheck::warning(
            "database_path",
            "Database file",
            format!("Database file does not exist yet: {}", db_path.display()),
            "The app will create a fresh database on startup.",
        )
    });
    checks
}

fn path_exists_or_warning(
    id: &str,
    label: &str,
    path: Option<std::path::PathBuf>,
    missing: &str,
    suggestion: &str,
) -> DiagnosticCheck {
    match path {
        Some(path) if path.exists() => {
            DiagnosticCheck::ok(id, label, format!("Path is available: {}", path.display()))
        }
        Some(path) => DiagnosticCheck::warning(
            id,
            label,
            format!("Path does not exist: {}", path.display()),
            suggestion,
        ),
        None => DiagnosticCheck::warning(id, label, missing, suggestion),
    }
}
