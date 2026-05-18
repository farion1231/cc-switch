use crate::error::AppError;
use std::path::{Path, PathBuf};

const FORBIDDEN_CHARS: [char; 14] = [
    '\r', '\n', ';', '|', '`', '&', '<', '>', '$', '(', ')', '{', '}', '#',
];

pub fn validate_shell_value(label: &str, value: &str) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::Message(format!(
            "POWERSHELL_ARG_REJECTED: {label} cannot be empty"
        )));
    }
    if let Some(ch) = value.chars().find(|ch| FORBIDDEN_CHARS.contains(ch)) {
        return Err(AppError::Message(format!(
            "POWERSHELL_ARG_REJECTED: {label} contains forbidden character {ch:?}"
        )));
    }
    Ok(())
}

pub fn validate_optional_shell_value(label: &str, value: Option<&str>) -> Result<(), AppError> {
    if let Some(value) = value {
        validate_shell_value(label, value)?;
    }
    Ok(())
}

pub fn validate_cwd(cwd: Option<&str>) -> Result<Option<PathBuf>, AppError> {
    let Some(cwd) = cwd else {
        return Ok(None);
    };
    validate_shell_value("cwd", cwd)?;
    let path = Path::new(cwd);
    if !path.exists() {
        return Err(AppError::Message(format!(
            "POWERSHELL_ARG_REJECTED: cwd does not exist: {cwd}"
        )));
    }
    if !path.is_dir() {
        return Err(AppError::Message(format!(
            "POWERSHELL_ARG_REJECTED: cwd is not a directory: {cwd}"
        )));
    }
    Ok(Some(path.to_path_buf()))
}

pub fn quote_powershell_single(value: &str) -> Result<String, AppError> {
    validate_shell_value("powershell value", value)?;
    Ok(format!("'{}'", value.replace('\'', "''")))
}

pub fn build_env_assignment(key: &str, value: &str) -> Result<String, AppError> {
    validate_env_key(key)?;
    let quoted = quote_powershell_single(value)?;
    Ok(format!("$env:{key}={quoted}"))
}

pub fn build_command_invocation(program: &str, args: &[String]) -> Result<String, AppError> {
    validate_shell_value("program", program)?;
    let mut parts = vec![quote_powershell_single(program)?];
    for arg in args {
        parts.push(quote_powershell_single(arg)?);
    }
    Ok(format!("& {}", parts.join(" ")))
}

pub fn validate_env_key(key: &str) -> Result<(), AppError> {
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err(AppError::Message(format!(
            "POWERSHELL_ARG_REJECTED: invalid env key {key}"
        )));
    }
    Ok(())
}

pub fn build_powershell_script(
    env: &[(String, String)],
    program: &str,
    args: &[String],
    agent_id: &str,
    provider_name: &str,
    upstream_model: &str,
) -> Result<String, AppError> {
    let mut statements = Vec::new();

    // Set environment variables first
    for (key, value) in env {
        statements.push(build_env_assignment(key, value)?);
    }

    // Diagnostic output - use string concatenation to expand variables
    statements.push(format!(
        "Write-Host ('[CCSA] AGENT_ID=' + '{}')",
        agent_id.replace('\'', "''")
    ));
    statements
        .push("Write-Host ('[CCSA] ANTHROPIC_BASE_URL=' + $env:ANTHROPIC_BASE_URL)".to_string());
    statements.push(
        "Write-Host ('[CCSA] SHELL=PowerShell ' + $PSVersionTable.PSVersion.ToString() + ' PSEdition=' + $PSVersionTable.PSEdition + ' COMSPEC=' + $env:ComSpec)".to_string(),
    );
    statements.push("Write-Host '[CCSA] ANTHROPIC_AUTH_TOKEN=set'".to_string());
    statements.push(format!(
        "Write-Host ('[CCSA] PROVIDER_SNAPSHOT={}/{}')",
        provider_name.replace('\'', "''"),
        upstream_model.replace('\'', "''")
    ));

    // Validate ANTHROPIC_BASE_URL is set correctly
    statements.push(
        "if ([string]::IsNullOrWhiteSpace($env:ANTHROPIC_BASE_URL) -or -not $env:ANTHROPIC_BASE_URL.StartsWith('http://127.0.0.1:')) { Write-Host '[CCSA] ERROR: ANTHROPIC_BASE_URL is not set to local agent port!'; exit 1 }".to_string(),
    );

    // Health check - verify agent listener is reachable
    statements.push(
        "try { $healthUrl = $env:ANTHROPIC_BASE_URL + '/health'; Write-Host ('[CCSA] HEALTH_CHECK=' + $healthUrl); $r = Invoke-WebRequest -Uri $healthUrl -UseBasicParsing -TimeoutSec 5; Write-Host ('[CCSA] HEALTH_STATUS=' + $r.StatusCode) } catch { Write-Host ('[CCSA] HEALTH_CHECK_FAILED=' + $_.Exception.Message); exit 1 }".to_string(),
    );

    statements.push("Write-Host ''".to_string());
    let invocation = build_command_invocation(program, args)?;
    statements.push(format!("try {{ {invocation} }} catch {{ throw }}"));
    Ok(statements.join("; "))
}

pub fn build_encoded_command(script: &str) -> String {
    use base64::{engine::general_purpose, Engine as _};

    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    general_purpose::STANDARD.encode(bytes)
}

pub fn build_agent_window_title(
    agent_id: &str,
    provider_name: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Result<String, AppError> {
    validate_shell_value("agent_id", agent_id)?;
    let short_id = if agent_id.len() > 8 {
        &agent_id[..8]
    } else {
        agent_id
    };

    // Extract folder name from CWD (the deepest directory name)
    let folder = cwd
        .and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.is_empty())
        })
        .unwrap_or("")
        .to_string();

    let title = if let Some(name) = provider_name {
        if !name.is_empty() && !folder.is_empty() {
            format!("CCSA:{}/{}/{}", short_id, name, &folder)
        } else if !name.is_empty() {
            format!("CCSA:{}/{}", short_id, name)
        } else {
            format!("CCSA:{}", short_id)
        }
    } else {
        format!("CCSA:{}", short_id)
    };
    validate_shell_value("window title", &title)?;
    Ok(title)
}

pub fn build_process_marker_query(agent_id: &str) -> Result<String, AppError> {
    let marker = build_agent_window_title(agent_id, None, None)?;
    let quoted_marker = marker.replace('\'', "''");
    Ok(format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{quoted_marker}*' }} | Select-Object -ExpandProperty ProcessId"
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        build_encoded_command, build_powershell_script, quote_powershell_single,
        validate_optional_shell_value, validate_shell_value,
    };
    use base64::Engine as _;

    #[test]
    fn rejects_shell_control_characters() {
        for value in ["abc;def", "abc|def", "abc`def", "abc\ndef", "abc&def"] {
            assert!(validate_shell_value("arg", value).is_err(), "{value}");
        }
        assert!(validate_optional_shell_value("session_id", None).is_ok());
    }

    #[test]
    fn quotes_single_quotes_safely() {
        assert_eq!(
            quote_powershell_single("O'Hara").expect("quote"),
            "'O''Hara'"
        );
    }

    #[test]
    fn default_script_does_not_set_claude_config_dir() {
        let script = build_powershell_script(
            &[
                (
                    "ANTHROPIC_BASE_URL".to_string(),
                    "http://127.0.0.1:15722".to_string(),
                ),
                (
                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                    "PROXY_MANAGED".to_string(),
                ),
            ],
            "claude",
            &[],
            "agent-1",
            "DeepSeek",
            "deepseek-v4-pro[1M]",
        )
        .expect("script");
        assert!(!script.contains("$env:CLAUDE_CONFIG_DIR ="));
        assert!(script.contains("ANTHROPIC_BASE_URL"));
        assert!(script.contains("ANTHROPIC_AUTH_TOKEN"));
        assert!(script.contains("[CCSA]"));
        assert!(script.contains("[CCSA] SHELL=PowerShell"));
        assert!(script.contains("DeepSeek"));
        assert!(script.contains("HEALTH_CHECK"));
        assert!(script.contains("127.0.0.1"));
    }

    #[test]
    fn encoded_command_is_utf16le_base64_without_raw_delimiters() {
        let script = build_powershell_script(
            &[(
                "ANTHROPIC_BASE_URL".to_string(),
                "http://127.0.0.1:15722".to_string(),
            )],
            "claude",
            &["--resume".to_string(), "session-1".to_string()],
            "agent-1",
            "TestProvider",
            "test-model",
        )
        .expect("script");
        assert!(script.contains("; "));

        let encoded = build_encoded_command(&script);
        assert!(!encoded.is_empty());
        assert!(!encoded.contains(';'));

        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 decode");
        let decoded_units = decoded_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        let decoded = String::from_utf16(&decoded_units).expect("utf16 decode");
        assert_eq!(decoded, script);
    }

    #[test]
    fn encoded_command_preserves_chinese_text() {
        let script = "$env:TEST='中文路径'; & 'claude' '会话'";
        let encoded = build_encoded_command(script);
        let decoded_bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("base64 decode");
        let decoded_units = decoded_bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        let decoded = String::from_utf16(&decoded_units).expect("utf16 decode");
        assert_eq!(decoded, script);
    }

    #[test]
    fn process_query_builder_rejects_bad_agent_id() {
        assert!(super::build_process_marker_query("agent;bad").is_err());
        assert!(super::build_process_marker_query("agent-1").is_ok());
    }
}
