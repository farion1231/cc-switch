use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CLASH_VERGE_APP_ID: &str = "io.github.clash-verge-rev.clash-verge-rev";
const CLASH_VERGE_CONFIG_FILE: &str = "clash-verge.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEnvironmentReport {
    pub clash_verge_tun: ClashVergeTunCheck,
    pub proxy_environment: ProxyEnvironmentCheck,
    pub codex_env_file: CodexEnvFileCheck,
}

impl RelayEnvironmentReport {
    pub fn all_passed(&self) -> bool {
        !self.clash_verge_tun.enabled
            && self.proxy_environment.variables.is_empty()
            && !self.codex_env_file.exists
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashVergeTunCheck {
    pub enabled: bool,
    pub config_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEnvironmentCheck {
    pub variables: Vec<ProxyEnvironmentVariable>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEnvironmentVariable {
    pub name: String,
    pub source: ProxyEnvironmentSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyEnvironmentSource {
    Process,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEnvFileCheck {
    pub exists: bool,
    pub path: String,
}

pub fn inspect_relay_environment() -> RelayEnvironmentReport {
    let codex_home = crate::codex_home::default_codex_home_dir();
    RelayEnvironmentReport {
        clash_verge_tun: inspect_clash_verge_tun(&clash_verge_config_candidates()),
        proxy_environment: ProxyEnvironmentCheck {
            variables: detect_proxy_environment_variables(),
        },
        codex_env_file: inspect_codex_env_file(&codex_home),
    }
}

fn inspect_clash_verge_tun(paths: &[PathBuf]) -> ClashVergeTunCheck {
    let mut detected_path = None;
    for path in paths {
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        detected_path.get_or_insert_with(|| path.to_string_lossy().to_string());
        if yaml_top_level_bool(&contents, "enable_tun_mode") == Some(true) {
            return ClashVergeTunCheck {
                enabled: true,
                config_path: Some(path.to_string_lossy().to_string()),
            };
        }
    }
    ClashVergeTunCheck {
        enabled: false,
        config_path: detected_path,
    }
}

fn clash_verge_config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(base_dirs) = directories::BaseDirs::new() {
        candidates.push(
            base_dirs
                .data_dir()
                .join(CLASH_VERGE_APP_ID)
                .join(CLASH_VERGE_CONFIG_FILE),
        );
        candidates.push(
            base_dirs
                .config_dir()
                .join(CLASH_VERGE_APP_ID)
                .join(CLASH_VERGE_CONFIG_FILE),
        );
        candidates.push(
            base_dirs
                .home_dir()
                .join(".config")
                .join(CLASH_VERGE_APP_ID)
                .join(CLASH_VERGE_CONFIG_FILE),
        );
        candidates.push(
            base_dirs
                .home_dir()
                .join(".config")
                .join("clash-verge-rev")
                .join(CLASH_VERGE_CONFIG_FILE),
        );
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn yaml_top_level_bool(contents: &str, key: &str) -> Option<bool> {
    contents.lines().find_map(|line| {
        if line.chars().next().is_some_and(char::is_whitespace) {
            return None;
        }
        let line = line.split('#').next()?.trim();
        let (candidate, value) = line.split_once(':')?;
        if candidate.trim() != key {
            return None;
        }
        match value
            .trim()
            .trim_matches(['\'', '"'])
            .to_ascii_lowercase()
            .as_str()
        {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    })
}

fn is_proxy_environment_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_uppercase().as_str(),
        "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY" | "NO_PROXY" | "FTP_PROXY"
    )
}

fn proxy_variables_from_pairs<I, K, V>(
    pairs: I,
    source: ProxyEnvironmentSource,
) -> Vec<ProxyEnvironmentVariable>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let names = pairs
        .into_iter()
        .filter(|(name, value)| {
            is_proxy_environment_name(name.as_ref()) && !value.as_ref().trim().is_empty()
        })
        .map(|(name, _)| name.as_ref().trim().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    names
        .into_iter()
        .map(|name| ProxyEnvironmentVariable { name, source })
        .collect()
}

fn detect_proxy_environment_variables() -> Vec<ProxyEnvironmentVariable> {
    let mut variables =
        proxy_variables_from_pairs(std::env::vars(), ProxyEnvironmentSource::Process);
    variables.extend(detect_user_proxy_environment_variables());
    variables.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.source.cmp(&right.source))
    });
    variables.dedup();
    variables
}

#[cfg(windows)]
fn detect_user_proxy_environment_variables() -> Vec<ProxyEnvironmentVariable> {
    let user_pairs = crate::windows_integration::read_current_user_string_values("Environment")
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, value.unwrap_or_default()));
    let mut variables = proxy_variables_from_pairs(user_pairs, ProxyEnvironmentSource::User);
    let system_pairs = crate::windows_integration::read_local_machine_string_values(
        r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    )
    .unwrap_or_default()
    .into_iter()
    .map(|(name, value)| (name, value.unwrap_or_default()));
    variables.extend(proxy_variables_from_pairs(
        system_pairs,
        ProxyEnvironmentSource::System,
    ));
    variables
}

#[cfg(not(windows))]
fn detect_user_proxy_environment_variables() -> Vec<ProxyEnvironmentVariable> {
    Vec::new()
}

fn inspect_codex_env_file(codex_home: &Path) -> CodexEnvFileCheck {
    let path = codex_home.join(".env");
    CodexEnvFileCheck {
        exists: path.is_file(),
        path: path.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clash_verge_top_level_tun_setting() {
        assert_eq!(
            yaml_top_level_bool("enable_tun_mode: true\n", "enable_tun_mode"),
            Some(true)
        );
        assert_eq!(
            yaml_top_level_bool("enable_tun_mode: 'false' # off\n", "enable_tun_mode"),
            Some(false)
        );
        assert_eq!(
            yaml_top_level_bool("  enable_tun_mode: true\n", "enable_tun_mode"),
            None
        );
    }

    #[test]
    fn detects_standard_proxy_variables_without_values() {
        let variables = proxy_variables_from_pairs(
            [
                ("https_proxy", "http://127.0.0.1:7890"),
                ("NO_PROXY", "localhost"),
                ("OPENAI_API_KEY", "secret"),
                ("ALL_PROXY", ""),
            ],
            ProxyEnvironmentSource::Process,
        );
        assert_eq!(
            variables
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["HTTPS_PROXY", "NO_PROXY"]
        );
    }

    #[test]
    fn reports_codex_dotenv_presence_without_reading_contents() {
        let temp = tempfile::tempdir().unwrap();
        let missing = inspect_codex_env_file(temp.path());
        assert!(!missing.exists);

        std::fs::write(temp.path().join(".env"), "OPENAI_API_KEY=secret\n").unwrap();
        let present = inspect_codex_env_file(temp.path());
        assert!(present.exists);
        assert!(present.path.ends_with(".env"));
    }

    #[test]
    fn reports_enabled_tun_from_existing_clash_verge_config() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join(CLASH_VERGE_CONFIG_FILE);
        std::fs::write(&config, "enable_tun_mode: true\n").unwrap();

        let check = inspect_clash_verge_tun(&[config.clone()]);
        assert!(check.enabled);
        assert_eq!(
            check.config_path,
            Some(config.to_string_lossy().to_string())
        );
    }
}
