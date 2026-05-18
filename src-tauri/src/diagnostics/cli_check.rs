use crate::agent_gateway::wt_launcher::command_exists;
use crate::diagnostics::report::DiagnosticCheck;
use serde_json::json;

pub fn check_cli() -> Vec<DiagnosticCheck> {
    let claude_available = command_exists("claude");
    let mut check = if claude_available {
        DiagnosticCheck::ok(
            "claude_cli",
            "Claude Code CLI",
            "claude is available on PATH",
        )
    } else {
        DiagnosticCheck::warning(
            "claude_cli",
            "Claude Code CLI",
            "claude was not found on PATH",
            "Install Claude Code CLI or configure its executable path before launching agents.",
        )
    };
    check = check.with_details(json!({
        "configuredClaudeExecutablePath": std::env::var("CLAUDE_CODE_PATH").ok(),
        "pathChecked": true
    }));
    vec![check]
}
