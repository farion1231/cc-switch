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

/// Get relevant keywords for each app
fn get_keywords_for_app(app: &str) -> Vec<&str> {
    match app.to_lowercase().as_str() {
        "claude" => vec!["ANTHROPIC"],
        "codex" => vec!["OPENAI"],
        "gemini" => vec!["GEMINI", "GOOGLE_GEMINI"],
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
            if keywords.iter().any(|k| name.to_uppercase().contains(k)) {
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
            if keywords.iter().any(|k| name.to_uppercase().contains(k)) {
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
        if keywords.iter().any(|k| key.to_uppercase().contains(k)) {
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
        if let Ok(content) = fs::read_to_string(&file_path) {
            for (line_num, line) in content.lines().enumerate() {
                let Some(assignment) = extract_exporting_assignment(line.trim()) else {
                    continue;
                };

                let Some(eq_pos) = assignment.find('=') else {
                    continue;
                };
                let var_name = assignment[..eq_pos].trim();
                let var_value = assignment[eq_pos + 1..].trim();

                // Reject anything that isn't a valid shell identifier — this filters
                // out aliases (`alias gemini=...`), functions, and lines with spaces.
                if !is_valid_env_name(var_name) {
                    continue;
                }

                if keywords.iter().any(|k| var_name.to_uppercase().contains(k)) {
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
    }

    Ok(conflicts)
}

/// Strip exporting builtins (`export`, `declare -x`, `typeset -gx`, …) from a
/// trimmed shell line and return the inner `NAME=VALUE` portion. Returns None
/// for comments, non-assignment lines, or `declare`/`typeset` calls without
/// `-x` (which create function-local, non-exported variables).
#[cfg(not(target_os = "windows"))]
fn extract_exporting_assignment(trimmed: &str) -> Option<&str> {
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("export ") {
        return Some(rest.trim_start());
    }

    // bash `declare` and zsh `typeset` only export when `-x` is in the flags
    // (e.g. `declare -x FOO=bar`, `typeset -gx FOO=bar`).
    for builtin in ["declare", "typeset"] {
        let Some(after) = trimmed.strip_prefix(builtin) else {
            continue;
        };
        let Some(first) = after.chars().next() else {
            continue;
        };
        if !first.is_whitespace() {
            // e.g. `declared=foo` — different variable, fall through to bare path.
            continue;
        }

        let mut s = after.trim_start();
        let mut has_x = false;
        while let Some(rest) = s.strip_prefix('-') {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let flag = &rest[..end];
            if flag == "-" {
                // `--` end-of-options marker
                s = rest[end..].trim_start();
                break;
            }
            if flag.contains('x') {
                has_x = true;
            }
            s = rest[end..].trim_start();
        }

        return if has_x { Some(s) } else { None };
    }

    // Bare `NAME=VALUE` — common in rc files paired with a later `export NAME`.
    if trimmed.contains('=') {
        Some(trimmed)
    } else {
        None
    }
}

/// A valid POSIX-ish env var name: starts with letter or `_`, then alphanumerics/`_`.
#[cfg(not(target_os = "windows"))]
fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_keywords() {
        assert_eq!(get_keywords_for_app("claude"), vec!["ANTHROPIC"]);
        assert_eq!(get_keywords_for_app("codex"), vec!["OPENAI"]);
        assert_eq!(
            get_keywords_for_app("gemini"),
            vec!["GEMINI", "GOOGLE_GEMINI"]
        );
        assert_eq!(get_keywords_for_app("unknown"), Vec::<&str>::new());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_is_valid_env_name() {
        assert!(is_valid_env_name("GEMINI_API_KEY"));
        assert!(is_valid_env_name("_FOO"));
        assert!(is_valid_env_name("Foo123"));

        // aliases / functions / multi-word — must be rejected
        assert!(!is_valid_env_name("alias gemini"));
        assert!(!is_valid_env_name("function gemini()"));
        assert!(!is_valid_env_name("1FOO"));
        assert!(!is_valid_env_name(""));
        assert!(!is_valid_env_name("FOO-BAR"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_extract_exporting_assignment() {
        // export
        assert_eq!(
            extract_exporting_assignment("export GEMINI_API_KEY=foo"),
            Some("GEMINI_API_KEY=foo")
        );

        // bash `declare -x`
        assert_eq!(
            extract_exporting_assignment("declare -x OPENAI_API_KEY=sk-x"),
            Some("OPENAI_API_KEY=sk-x")
        );

        // zsh `typeset -gx` (combined flags)
        assert_eq!(
            extract_exporting_assignment("typeset -gx GEMINI_API_KEY=bar"),
            Some("GEMINI_API_KEY=bar")
        );

        // Multiple flags including -x
        assert_eq!(
            extract_exporting_assignment("declare -r -x ANTHROPIC_API_KEY=k"),
            Some("ANTHROPIC_API_KEY=k")
        );

        // declare without -x is function-local — must be skipped
        assert_eq!(
            extract_exporting_assignment("declare GEMINI_API_KEY=local"),
            None
        );
        assert_eq!(
            extract_exporting_assignment("typeset -g GEMINI_API_KEY=local"),
            None
        );

        // alias must fall through and be rejected later by is_valid_env_name
        assert_eq!(
            extract_exporting_assignment("alias gemini=\"/path/to/gemini -y\""),
            Some("alias gemini=\"/path/to/gemini -y\"")
        );

        // bare assignment is preserved
        assert_eq!(
            extract_exporting_assignment("FOO=bar"),
            Some("FOO=bar")
        );

        // comments and empty lines
        assert_eq!(extract_exporting_assignment("# export FOO=bar"), None);
        assert_eq!(extract_exporting_assignment(""), None);

        // `declared` (different variable name starting with "declare") should not be eaten
        assert_eq!(
            extract_exporting_assignment("declared=foo"),
            Some("declared=foo")
        );
    }
}
