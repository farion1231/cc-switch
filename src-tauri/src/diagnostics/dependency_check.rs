use crate::agent_gateway::wt_launcher::command_exists;
use crate::diagnostics::report::DiagnosticCheck;

pub fn check_dependencies() -> Vec<DiagnosticCheck> {
    vec![
        command_check(
            "powershell",
            "PowerShell",
            "powershell.exe",
            true,
            "Install or repair Windows PowerShell.",
        ),
        command_check(
            "windows_terminal",
            "Windows Terminal",
            "wt.exe",
            false,
            "Install Windows Terminal, or the launcher will fall back to a PowerShell window.",
        ),
    ]
}

fn command_check(
    id: &str,
    label: &str,
    command: &str,
    required: bool,
    suggestion: &str,
) -> DiagnosticCheck {
    if command_exists(command) {
        DiagnosticCheck::ok(id, label, format!("{command} is available"))
    } else if required {
        DiagnosticCheck::error(id, label, format!("{command} was not found"), suggestion)
    } else {
        DiagnosticCheck::warning(id, label, format!("{command} was not found"), suggestion)
    }
}
