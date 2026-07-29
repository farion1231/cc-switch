use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSshTarget {
    pub name: String,
    pub host_alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone)]
enum ConfigDirective {
    Host(Vec<String>),
    Match,
    Option { key: String, value: String },
}

/// 读取当前用户的 OpenSSH 配置。配置文件不存在是正常状态，返回空列表；
/// 其他读取错误需要上抛，避免 UI 将权限问题误报成“没有服务器”。
pub fn discover_current_user_ssh_targets(
) -> Result<Vec<DiscoveredSshTarget>, SshConfigDiscoveryError> {
    let home = crate::config::get_home_dir();
    discover_ssh_targets(&home.join(".ssh").join("config"), &home)
}

/// 从指定入口解析 SSH Host，公开该入口便于用临时目录覆盖 Include 与匹配语义。
/// 生产代码应使用 `discover_current_user_ssh_targets`，避免绕过统一的用户目录解析。
pub fn discover_ssh_targets_from_path(
    config_path: &Path,
) -> Result<Vec<DiscoveredSshTarget>, SshConfigDiscoveryError> {
    let home = config_path
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    discover_ssh_targets(config_path, home)
}

fn discover_ssh_targets(
    config_path: &Path,
    home: &Path,
) -> Result<Vec<DiscoveredSshTarget>, SshConfigDiscoveryError> {
    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let include_base = config_path.parent().unwrap_or_else(|| Path::new("."));
    let mut directives = Vec::new();
    let mut include_stack = HashSet::new();
    parse_config_file(
        config_path,
        home,
        include_base,
        &mut include_stack,
        &mut directives,
    )?;

    let mut seen = HashSet::new();
    let mut aliases = Vec::new();
    for directive in &directives {
        let ConfigDirective::Host(patterns) = directive else {
            continue;
        };
        for pattern in patterns {
            // 通配和否定 Host 只用于继承配置，不能作为用户可连接的具体服务器展示。
            if pattern.starts_with('!') || pattern.contains(['*', '?']) {
                continue;
            }
            let normalized = pattern.to_ascii_lowercase();
            if seen.insert(normalized) {
                aliases.push(pattern.clone());
            }
        }
    }

    Ok(aliases
        .into_iter()
        .map(|alias| resolve_target(&alias, &directives))
        .collect())
}

fn parse_config_file(
    path: &Path,
    home: &Path,
    include_base: &Path,
    include_stack: &mut HashSet<PathBuf>,
    output: &mut Vec<ConfigDirective>,
) -> Result<(), SshConfigDiscoveryError> {
    let identity = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !include_stack.insert(identity.clone()) {
        // OpenSSH 的 Include 可以形成环；发现功能跳过当前递归分支，避免设置页被永久阻塞。
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|source| SshConfigDiscoveryError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    for raw_line in content.lines() {
        let Some((key, value)) = parse_line(raw_line) else {
            continue;
        };
        match key.as_str() {
            "host" => output.push(ConfigDirective::Host(split_words(&value))),
            "match" => output.push(ConfigDirective::Match),
            "include" => {
                for include in split_words(&value) {
                    let pattern = resolve_include_pattern(&include, home, include_base);
                    for included_path in expand_path_pattern(&pattern)? {
                        if included_path.is_file() {
                            parse_config_file(
                                &included_path,
                                home,
                                include_base,
                                include_stack,
                                output,
                            )?;
                        }
                    }
                }
            }
            _ => output.push(ConfigDirective::Option { key, value }),
        }
    }
    include_stack.remove(&identity);
    Ok(())
}

fn resolve_target(alias: &str, directives: &[ConfigDirective]) -> DiscoveredSshTarget {
    let mut active = true;
    let mut hostname = None;
    let mut username = None;
    let mut port = None;
    let mut identity_file = None;

    for directive in directives {
        match directive {
            ConfigDirective::Host(patterns) => active = host_patterns_match(patterns, alias),
            // Match 支持用户、地址和命令等运行时条件；发现阶段无法可靠求值，
            // 因此保守跳过该块，直到后续 Host 重新建立明确作用域。
            ConfigDirective::Match => active = false,
            ConfigDirective::Option { key, value } if active => match key.as_str() {
                // OpenSSH 对标量配置采用“首个获得的值生效”，默认 Host * 通常位于文件末尾。
                "hostname" if hostname.is_none() => hostname = non_empty(value),
                "user" if username.is_none() => username = non_empty(value),
                "port" if port.is_none() => {
                    port = value.parse::<u16>().ok().filter(|value| *value > 0)
                }
                "identityfile" if identity_file.is_none() => identity_file = non_empty(value),
                _ => {}
            },
            ConfigDirective::Option { .. } => {}
        }
    }

    DiscoveredSshTarget {
        name: alias.to_string(),
        host_alias: alias.to_string(),
        hostname,
        username,
        port,
        identity_file,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = unquote(value.trim());
    (!value.is_empty()).then(|| value.to_string())
}

fn host_patterns_match(patterns: &[String], alias: &str) -> bool {
    let alias = alias.to_ascii_lowercase();
    let mut positive_match = false;
    for pattern in patterns {
        let (negated, pattern) = pattern
            .strip_prefix('!')
            .map_or((false, pattern.as_str()), |pattern| (true, pattern));
        if wildcard_match(&pattern.to_ascii_lowercase(), &alias) {
            if negated {
                return false;
            }
            positive_match = true;
        }
    }
    positive_match
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn parse_line(line: &str) -> Option<(String, String)> {
    let line = strip_comment(line).trim();
    if line.is_empty() {
        return None;
    }
    let split = line
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || *ch == '=')
        .map(|(index, _)| index)?;
    let key = line[..split].trim().to_ascii_lowercase();
    let value = line[split..]
        .trim_start_matches(|ch: char| ch.is_whitespace() || ch == '=')
        .trim()
        .to_string();
    (!key.is_empty() && !value.is_empty()).then_some((key, value))
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (index, ch) in line.char_indices() {
        match ch {
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => return &line[..index],
            _ => {}
        }
    }
    line
}

fn split_words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in value.chars() {
        match ch {
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn unquote(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if matches!(
            (bytes[0], bytes[value.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'')
        ) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn resolve_include_pattern(pattern: &str, home: &Path, include_base: &Path) -> PathBuf {
    if pattern == "~" {
        return home.to_path_buf();
    }
    if let Some(relative) = pattern
        .strip_prefix("~/")
        .or_else(|| pattern.strip_prefix("~\\"))
    {
        return home.join(relative);
    }
    let path = PathBuf::from(pattern);
    if path.is_absolute() {
        path
    } else {
        include_base.join(path)
    }
}

fn expand_path_pattern(pattern: &Path) -> Result<Vec<PathBuf>, SshConfigDiscoveryError> {
    let mut candidates = vec![PathBuf::new()];
    for component in pattern.components() {
        match component {
            Component::Normal(segment) if contains_wildcard(segment) => {
                let matcher = segment.to_string_lossy().to_string();
                let mut expanded = Vec::new();
                for base in candidates {
                    let entries = match fs::read_dir(&base) {
                        Ok(entries) => entries,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(source) => {
                            return Err(SshConfigDiscoveryError::Read { path: base, source })
                        }
                    };
                    for entry in entries {
                        let entry = entry.map_err(|source| SshConfigDiscoveryError::Read {
                            path: base.clone(),
                            source,
                        })?;
                        let name = entry.file_name().to_string_lossy().to_string();
                        if wildcard_match(&matcher.to_ascii_lowercase(), &name.to_ascii_lowercase())
                        {
                            expanded.push(entry.path());
                        }
                    }
                }
                expanded.sort();
                candidates = expanded;
            }
            _ => {
                for candidate in &mut candidates {
                    candidate.push(component.as_os_str());
                }
            }
        }
    }
    Ok(candidates)
}

fn contains_wildcard(value: &std::ffi::OsStr) -> bool {
    value.to_string_lossy().contains(['*', '?'])
}

#[derive(Debug, thiserror::Error)]
pub enum SshConfigDiscoveryError {
    #[error("无法读取 SSH 配置 {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
}
