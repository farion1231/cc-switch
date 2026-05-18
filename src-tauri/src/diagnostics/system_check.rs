use crate::diagnostics::report::DiagnosticCheck;
use serde_json::json;

pub fn check_system() -> Vec<DiagnosticCheck> {
    let mut checks = Vec::new();
    checks.push(DiagnosticCheck::ok(
        "cpu_architecture",
        "CPU architecture",
        std::env::consts::ARCH,
    ));
    checks.push(
        DiagnosticCheck::ok(
            "operating_system",
            "Operating system",
            format!("{} {}", std::env::consts::OS, windows_version()),
        )
        .with_details(json!({
            "family": std::env::consts::FAMILY,
            "arch": std::env::consts::ARCH
        })),
    );
    checks
}

fn windows_version() -> String {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "version unavailable".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        "non-Windows validation host".to_string()
    }
}
