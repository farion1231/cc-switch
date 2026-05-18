use crate::config::get_app_config_dir;
use crate::diagnostics::report::DiagnosticCheck;
use std::path::Path;

pub fn check_permissions() -> Vec<DiagnosticCheck> {
    let mut checks = Vec::new();
    if let Some(data_dir) = dirs::data_dir() {
        checks.push(write_check(
            "appdata_write",
            "AppData write permission",
            &data_dir,
        ));
    }
    if let Some(home_dir) = dirs::home_dir() {
        checks.push(write_check(
            "userprofile_write",
            "USERPROFILE write permission",
            &home_dir,
        ));
    }
    checks.push(write_check(
        "cc_switch_config_write",
        "CC Switch config write permission",
        &get_app_config_dir(),
    ));
    checks
}

fn write_check(id: &str, label: &str, dir: &Path) -> DiagnosticCheck {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return DiagnosticCheck::error(
            id,
            label,
            format!("Unable to create directory: {}", dir.display()),
            "Check directory permissions and Windows security policy.",
        )
        .with_details(serde_json::json!({ "error": e.to_string() }));
    }
    let path = dir.join(".cc-switch-agent-write-test");
    match std::fs::write(&path, b"ok").and_then(|_| std::fs::remove_file(&path)) {
        Ok(()) => DiagnosticCheck::ok(
            id,
            label,
            format!("Write check succeeded: {}", dir.display()),
        ),
        Err(e) => DiagnosticCheck::error(
            id,
            label,
            format!("Write check failed: {}", dir.display()),
            "Grant write permission or choose a writable config directory.",
        )
        .with_details(serde_json::json!({ "error": e.to_string() })),
    }
}
