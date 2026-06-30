use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "windows"))]
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvConflict {
    pub var_name: String,
    pub var_value: String,
    pub source_type: String, // "system" | "file"
    pub source_path: String, // Registry path or file path
}

#[cfg(target_os = "windows")]
use winreg::enums::*;
#[cfg(target_os = "windows")]
use winreg::RegKey;

/// Check environment variables for conflicts
pub fn check_env_conflicts(app: &str) -> Result<Vec<EnvConflict>, String> {
    let keywords = get_keywords_for_app(app);
    let mut conflicts = Vec::new();

    // Check system environment variables
    conflicts.extend(check_system_env(&keywords)?);

    // Check shell configuration files (Unix only)
    #[cfg(not(target_os = "windows"))]
    conflicts.extend(check_shell_configs(&keywords)?);

    Ok(conflicts)
}

/// Get the list of environment variable names that, when present in the
/// user's shell or process environment, can override what cc-switch
/// configures. Names must match exactly (case-insensitive); substring
/// matching produced false positives for unrelated vars whose names merely
/// contained "ANTHROPIC", "OPENAI", or "GEMINI" (e.g. local helper vars
/// such as `LOG_ANTHROPIC_*`, `v_gemini_remote`).
fn get_keywords_for_app(app: &str) -> Vec<&'static str> {
    match app.to_lowercase().as_str() {
        "claude" => vec![
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_REASONING_MODEL",
        ],
        "codex" => vec!["OPENAI_API_KEY", "OPENAI_BASE_URL"],
        "gemini" => vec![
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_GEMINI_API_KEY",
        ],
        _ => vec![],
    }
}

/// Check system environment variables (Windows Registry or Unix env)
#[cfg(target_os = "windows")]
fn check_system_env(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    // Check HKEY_CURRENT_USER\Environment
    if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
        for (name, value) in hkcu.enum_values().filter_map(Result::ok) {
            if keywords.iter().any(|k| name.eq_ignore_ascii_case(k)) {
                conflicts.push(EnvConflict {
                    var_name: name.clone(),
                    var_value: value.to_string(),
                    source_type: "system".to_string(),
                    source_path: "HKEY_CURRENT_USER\\Environment".to_string(),
                });
            }
        }
    }

    // Check HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\Session Manager\Environment
    if let Ok(hklm) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
    {
        for (name, value) in hklm.enum_values().filter_map(Result::ok) {
            if keywords.iter().any(|k| name.eq_ignore_ascii_case(k)) {
                conflicts.push(EnvConflict {
                    var_name: name.clone(),
                    var_value: value.to_string(),
                    source_type: "system".to_string(),
                    source_path: "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment".to_string(),
                });
            }
        }
    }

    Ok(conflicts)
}

#[cfg(not(target_os = "windows"))]
fn check_system_env(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    // Check current process environment
    for (key, value) in std::env::vars() {
        if keywords.iter().any(|k| key.eq_ignore_ascii_case(k)) {
            conflicts.push(EnvConflict {
                var_name: key,
                var_value: value,
                source_type: "system".to_string(),
                source_path: "Process Environment".to_string(),
            });
        }
    }

    Ok(conflicts)
}

/// Check shell configuration files for environment variable exports (Unix only)
#[cfg(not(target_os = "windows"))]
fn check_shell_configs(keywords: &[&str]) -> Result<Vec<EnvConflict>, String> {
    let mut conflicts = Vec::new();

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let config_files = vec![
        format!("{}/.bashrc", home),
        format!("{}/.bash_profile", home),
        format!("{}/.zshrc", home),
        format!("{}/.zprofile", home),
        format!("{}/.profile", home),
        "/etc/profile".to_string(),
        "/etc/bashrc".to_string(),
    ];

    for file_path in config_files {
        let Ok(content) = fs::read_to_string(&file_path) else {
            continue;
        };

        // Track shell function/block depth so assignments inside function
        // bodies are ignored. Variables defined in `name() { ... }` are
        // either function-scoped or — when written as the inline command
        // prefix `VAR=value cmd ...` — only visible to the spawned child
        // process, neither of which leaks into a process started later from
        // the same login shell. A naive `{`/`}` counter is good enough for
        // typical rc files; braces inside strings/comments cause harmless
        // over-skipping rather than false positives.
        let mut brace_depth: i32 = 0;

        for (line_num, line) in content.lines().enumerate() {
            let line_delta: i32 = line
                .chars()
                .map(|c| match c {
                    '{' => 1,
                    '}' => -1,
                    _ => 0,
                })
                .sum();
            let depth_after = brace_depth + line_delta;
            // Skip the line if we are inside a block at any point during it,
            // i.e. either entered the line inside a block, or left the line
            // inside one. This correctly skips both `name() {` and `}`.
            let inside_block = brace_depth > 0 || depth_after > 0;
            brace_depth = depth_after.max(0);

            if inside_block {
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Only flag explicit `export VAR=...`. A bare `VAR=value` at the
            // top of an rc file is most often the inline command-prefix
            // form (`VAR=value some_cmd`) — POSIX semantics make that
            // assignment visible only to `some_cmd` — or an unexported
            // shell variable. Neither leaks into descendant processes that
            // would override what cc-switch configures.
            let Some(rest) = trimmed.strip_prefix("export ") else {
                continue;
            };
            let Some(eq_pos) = rest.find('=') else {
                continue;
            };
            let var_name = rest[..eq_pos].trim();
            let var_value = rest[eq_pos + 1..].trim();

            if keywords.iter().any(|k| var_name.eq_ignore_ascii_case(k)) {
                conflicts.push(EnvConflict {
                    var_name: var_name.to_string(),
                    var_value: var_value
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string(),
                    source_type: "file".to_string(),
                    source_path: format!("{}:{}", file_path, line_num + 1),
                });
            }
        }
    }

    Ok(conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_keywords_returns_exact_var_names() {
        let claude = get_keywords_for_app("claude");
        assert!(claude.contains(&"ANTHROPIC_API_KEY"));
        assert!(claude.contains(&"ANTHROPIC_AUTH_TOKEN"));
        assert!(claude.contains(&"ANTHROPIC_BASE_URL"));

        let codex = get_keywords_for_app("codex");
        assert!(codex.contains(&"OPENAI_API_KEY"));

        let gemini = get_keywords_for_app("gemini");
        assert!(gemini.contains(&"GEMINI_API_KEY"));
        assert!(gemini.contains(&"GOOGLE_API_KEY"));

        assert_eq!(get_keywords_for_app("unknown"), Vec::<&str>::new());
    }

    // The shell-config scanner reads `$HOME` at runtime, so the helper below
    // mutates the process env. Tests that touch HOME must run serially or
    // they will race each other.
    #[cfg(not(target_os = "windows"))]
    fn scan_with_rc(rc_content: &str, app: &str) -> Vec<EnvConflict> {
        let dir = tempfile::tempdir().unwrap();
        let rc_path = dir.path().join(".zshrc");
        fs::write(&rc_path, rc_content).unwrap();

        // Re-run check_shell_configs against a synthetic $HOME so we only
        // see hits from the file we control.
        let prev_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", dir.path());
        let keywords = get_keywords_for_app(app);
        let result = check_shell_configs(&keywords).unwrap();
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        result
    }

    #[cfg(not(target_os = "windows"))]
    #[serial_test::serial]
    #[test]
    fn test_skips_inline_command_prefix_inside_function() {
        // Reproduces the user-reported false-positive from issue #516:
        // every var here is scoped to the spawned `claude` child only.
        let rc = r#"
cc() {
  ANTHROPIC_AUTH_TOKEN="sk-test" \
  ANTHROPIC_BASE_URL="https://example.com" \
  claude --dangerously-skip-permissions "$@"
}
"#;
        let conflicts = scan_with_rc(rc, "claude");
        assert!(
            conflicts.is_empty(),
            "function-body inline-prefix vars must not be flagged: {conflicts:?}"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[serial_test::serial]
    #[test]
    fn test_flags_top_level_export() {
        let rc = r#"
# real export — would actually leak to children
export ANTHROPIC_API_KEY="sk-real"
"#;
        let conflicts = scan_with_rc(rc, "claude");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].var_name, "ANTHROPIC_API_KEY");
        assert_eq!(conflicts[0].var_value, "sk-real");
    }

    #[cfg(not(target_os = "windows"))]
    #[serial_test::serial]
    #[test]
    fn test_does_not_flag_unrelated_var_with_keyword_substring() {
        // Pre-fix: contains("ANTHROPIC") matched these and false-positived.
        let rc = r#"
export LOG_ANTHROPIC_REQUESTS="1"
export MY_OPENAI_PROXY_LOG="/tmp/log"
local v_gemini_remote="something"
"#;
        let claude = scan_with_rc(rc, "claude");
        let codex = scan_with_rc(rc, "codex");
        let gemini = scan_with_rc(rc, "gemini");
        assert!(claude.is_empty(), "claude false-positive: {claude:?}");
        assert!(codex.is_empty(), "codex false-positive: {codex:?}");
        assert!(gemini.is_empty(), "gemini false-positive: {gemini:?}");
    }

    #[cfg(not(target_os = "windows"))]
    #[serial_test::serial]
    #[test]
    fn test_does_not_flag_bare_top_level_assignment() {
        // Without `export`, this is either a non-exported shell var or
        // (more commonly) syntactically the start of an inline command
        // prefix on a continued line. Either way it does not pollute
        // child processes.
        let rc = r#"ANTHROPIC_API_KEY="sk-not-exported"
"#;
        let conflicts = scan_with_rc(rc, "claude");
        assert!(conflicts.is_empty(), "{conflicts:?}");
    }
}
