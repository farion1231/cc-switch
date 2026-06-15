use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::AppError;

const SOURCE_FILE_NAME: &str = "skill-env.env";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnvState {
    pub source_path: String,
    pub output_path: String,
    pub content: String,
    pub parsed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEnvSaveResult {
    pub source_path: String,
    pub output_path: String,
    pub parsed_count: usize,
}

pub fn default_output_path() -> PathBuf {
    let home = crate::config::get_home_dir();
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("skills").join("env.ps1");
        }
        return home
            .join("AppData")
            .join("Roaming")
            .join("skills")
            .join("env.ps1");
    }

    #[cfg(not(target_os = "windows"))]
    {
        home.join(".config").join("skills").join("env.sh")
    }
}

pub fn source_path() -> PathBuf {
    crate::config::get_home_dir()
        .join(".cc-switch")
        .join(SOURCE_FILE_NAME)
}

pub fn resolve_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        return crate::config::get_home_dir();
    }
    if let Some(stripped) = trimmed.strip_prefix("~/") {
        return crate::config::get_home_dir().join(stripped);
    }
    if let Some(stripped) = trimmed.strip_prefix("~\\") {
        return crate::config::get_home_dir().join(stripped);
    }
    PathBuf::from(trimmed)
}

pub fn output_path_from_settings() -> PathBuf {
    crate::settings::get_settings()
        .skill_env_output_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(resolve_path)
        .unwrap_or_else(default_output_path)
}

pub fn read_state() -> Result<SkillEnvState, AppError> {
    let source = source_path();
    let output = output_path_from_settings();
    let content = if source.exists() {
        fs::read_to_string(&source).map_err(|e| AppError::io(&source, e))?
    } else if output.exists() {
        let generated = fs::read_to_string(&output).map_err(|e| AppError::io(&output, e))?;
        env_map_to_dotenv(&parse_generated_env(&generated))
    } else {
        String::new()
    };
    let parsed_count = parse_dotenv(&content)?.len();
    Ok(SkillEnvState {
        source_path: source.display().to_string(),
        output_path: output.display().to_string(),
        content,
        parsed_count,
    })
}

pub fn save_and_refresh(content: &str) -> Result<SkillEnvSaveResult, AppError> {
    let previous_env = read_existing_env_for_refresh();
    let env = parse_dotenv(content)?;
    let source = source_path();
    write_secure(&source, normalize_source_content(content)?.as_bytes())?;

    let output = output_path_from_settings();
    let generated = render_generated_env(&env, &source);
    write_secure(&output, generated.as_bytes())?;

    apply_to_current_process(&env, previous_env.keys());

    Ok(SkillEnvSaveResult {
        source_path: source.display().to_string(),
        output_path: output.display().to_string(),
        parsed_count: env.len(),
    })
}

pub fn refresh_from_source() -> Result<SkillEnvSaveResult, AppError> {
    let state = read_state()?;
    save_and_refresh(&state.content)
}

pub fn apply_saved_to_current_process() -> Result<usize, AppError> {
    let state = read_state()?;
    let env = parse_dotenv(&state.content)?;
    apply_to_current_process(&env, std::iter::empty::<&String>());
    Ok(env.len())
}

fn normalize_source_content(content: &str) -> Result<String, AppError> {
    parse_dotenv(content)?;
    let trimmed = content.trim_end();
    if trimmed.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("{trimmed}\n"))
    }
}

fn parse_dotenv(content: &str) -> Result<BTreeMap<String, String>, AppError> {
    let mut env = BTreeMap::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let assignment = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = assignment.split_once('=') else {
            return Err(AppError::InvalidInput(format!(
                "第 {} 行不是 KEY=value 格式",
                index + 1
            )));
        };
        let key = key.trim();
        if !is_valid_key(key) {
            return Err(AppError::InvalidInput(format!(
                "第 {} 行变量名无效: {}",
                index + 1,
                key
            )));
        }
        env.insert(key.to_string(), unquote_value(value.trim()));
    }
    Ok(env)
}

fn parse_generated_env(content: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            if let Some((key, value)) = rest.trim().split_once('=') {
                let key = key.trim();
                if is_valid_key(key) {
                    env.insert(key.to_string(), unquote_value(value.trim()));
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("$env:") {
            if let Some((key, value)) = rest.trim().split_once('=') {
                let key = key.trim();
                if is_valid_key(key) {
                    env.insert(key.to_string(), unquote_powershell_value(value.trim()));
                }
            }
        }
    }
    env
}

fn env_map_to_dotenv(env: &BTreeMap<String, String>) -> String {
    env.iter()
        .map(|(key, value)| format!("{key}={}", quote_dotenv_value(value)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_generated_env(env: &BTreeMap<String, String>, source: &Path) -> String {
    let header = format!(
        "# Generated by CC Switch from {}\n# Edit variables in CC Switch instead of modifying this file directly.\n",
        source.display()
    );
    #[cfg(target_os = "windows")]
    {
        let mut output = header;
        for (key, value) in env {
            output.push_str(&format!("$env:{key} = {}\n", quote_powershell(value)));
        }
        return output;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut output = header;
        for (key, value) in env {
            output.push_str(&format!("export {key}={}\n", quote_shell(value)));
        }
        output
    }
}

fn read_existing_env_for_refresh() -> BTreeMap<String, String> {
    let source = source_path();
    if let Ok(content) = fs::read_to_string(&source) {
        return parse_dotenv(&content).unwrap_or_default();
    }
    let output = output_path_from_settings();
    if let Ok(content) = fs::read_to_string(&output) {
        return parse_generated_env(&content);
    }
    BTreeMap::new()
}

fn apply_to_current_process<'a, I>(env: &BTreeMap<String, String>, previous_keys: I)
where
    I: IntoIterator<Item = &'a String>,
{
    for key in previous_keys {
        if !env.contains_key(key) {
            std::env::remove_var(key);
        }
    }
    for (key, value) in env {
        std::env::set_var(key, value);
    }
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn unquote_value(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return unescape_basic(&value[1..value.len() - 1]);
        }
    }
    value.to_string()
}

fn unescape_basic(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
            } else {
                output.push(ch);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn quote_dotenv_value(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@'))
    {
        return value.to_string();
    }
    format!("\"{}\"", escape_double_quoted(value))
}

fn quote_shell(value: &str) -> String {
    format!("\"{}\"", escape_double_quoted(value))
}

#[cfg(target_os = "windows")]
fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn escape_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

fn unquote_powershell_value(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'' {
            return value[1..value.len() - 1].replace("''", "'");
        }
        if bytes[0] == b'"' && bytes[value.len() - 1] == b'"' {
            return unescape_powershell_double_quoted(&value[1..value.len() - 1]);
        }
    }
    value.to_string()
}

fn unescape_powershell_double_quoted(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            if let Some(next) = chars.next() {
                output.push(match next {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
            } else {
                output.push(ch);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn write_secure(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| AppError::io(path, e))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::io(path, e))?;
        file.write_all(bytes).map_err(|e| AppError::io(path, e))?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        fs::write(path, bytes).map_err(|e| AppError::io(path, e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotenv_and_quotes_shell_output() {
        let env = parse_dotenv("FOO=bar\nTOKEN=\"a b$c\"\nexport ENABLED=true\n").unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("TOKEN").unwrap(), "a b$c");
        assert_eq!(env.get("ENABLED").unwrap(), "true");
        let generated = render_generated_env(&env, Path::new("/tmp/source.env"));
        #[cfg(target_os = "windows")]
        assert!(generated.contains("$env:TOKEN = 'a b$c'"));
        #[cfg(not(target_os = "windows"))]
        assert!(generated.contains("export TOKEN=\"a b\\$c\""));
    }

    #[test]
    fn parses_existing_shell_exports() {
        let env = parse_generated_env("export OT_BASE_DIR=\"/tmp/ot\"\n# comment\nBAD-LINE=x\n");
        assert_eq!(env.get("OT_BASE_DIR").unwrap(), "/tmp/ot");
        assert!(!env.contains_key("BAD-LINE"));
    }

    #[test]
    fn parses_existing_powershell_exports_without_expanding_literals() {
        let env = parse_generated_env(
            "$env:TOKEN = 'abc$def'\n$env:QUOTE = 'it''s'\n$env:PATH = \"a`$b\"\n",
        );
        assert_eq!(env.get("TOKEN").unwrap(), "abc$def");
        assert_eq!(env.get("QUOTE").unwrap(), "it's");
        assert_eq!(env.get("PATH").unwrap(), "a$b");
    }

    #[test]
    #[cfg(unix)]
    fn write_secure_tightens_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("skill-env.env");
        fs::write(&path, b"old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        write_secure(&path, b"SECRET=value\n").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "SECRET=value\n");
    }

    #[test]
    #[serial_test::serial]
    fn applies_saved_source_env_on_startup_without_clearing_inherited_env() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join(".cc-switch");
        fs::create_dir_all(&source_dir).unwrap();
        fs::write(
            source_dir.join(SOURCE_FILE_NAME),
            "CC_SWITCH_SKILL_ENV_STARTUP_TEST=loaded\n",
        )
        .unwrap();

        let previous_home = std::env::var("CC_SWITCH_TEST_HOME").ok();
        let previous_loaded = std::env::var("CC_SWITCH_SKILL_ENV_STARTUP_TEST").ok();
        let previous_inherited = std::env::var("CC_SWITCH_SKILL_ENV_INHERITED_TEST").ok();

        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        std::env::remove_var("CC_SWITCH_SKILL_ENV_STARTUP_TEST");
        std::env::set_var("CC_SWITCH_SKILL_ENV_INHERITED_TEST", "keep");

        let applied = apply_saved_to_current_process().unwrap();

        assert_eq!(applied, 1);
        assert_eq!(
            std::env::var("CC_SWITCH_SKILL_ENV_STARTUP_TEST").unwrap(),
            "loaded"
        );
        assert_eq!(
            std::env::var("CC_SWITCH_SKILL_ENV_INHERITED_TEST").unwrap(),
            "keep"
        );

        match previous_home {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
        match previous_loaded {
            Some(value) => std::env::set_var("CC_SWITCH_SKILL_ENV_STARTUP_TEST", value),
            None => std::env::remove_var("CC_SWITCH_SKILL_ENV_STARTUP_TEST"),
        }
        match previous_inherited {
            Some(value) => std::env::set_var("CC_SWITCH_SKILL_ENV_INHERITED_TEST", value),
            None => std::env::remove_var("CC_SWITCH_SKILL_ENV_INHERITED_TEST"),
        }
    }
}
