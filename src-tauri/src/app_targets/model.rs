//! 写入目标的数据结构、旧单目录迁移、校验和兼容投影。
//!
//! 只做字符串层面的处理，不碰文件系统、不启动发行版：Windows / WSL 的路径规则在任何
//! 平台上都能单测。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::app_config::AppType;

/// Windows 目标的稳定标识。
pub const WINDOWS: &str = "windows";
/// WSL 目标标识的前缀：`wsl:<发行版>`。
pub const WSL_PREFIX: &str = "wsl:";

/// 第一阶段支持多目标写入的应用；其余应用沿用单目录。
pub fn supports(app: &AppType) -> bool {
    matches!(
        app,
        AppType::Claude | AppType::Codex | AppType::Gemini | AppType::OpenCode
    )
}

pub fn supported_apps() -> [AppType; 4] {
    [
        AppType::Claude,
        AppType::Codex,
        AppType::Gemini,
        AppType::OpenCode,
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsTarget {
    #[serde(default)]
    pub enabled: bool,
    /// 自定义目录；`None` 用该应用现有的默认目录解析。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WslTarget {
    /// 发行版原始名称（匹配时不区分大小写）。
    pub distro: String,
    #[serde(default)]
    pub enabled: bool,
    /// 自定义 UNC 目录；`None` 按发行版的 HOME 和应用自己的规则推导。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
}

/// 一个应用的写入目标。不存 `mode`：从 enabled 推导。未勾选目标的目录也保存。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTargetsConfig {
    /// 主环境：`windows` 或 `wsl:<发行版>`。导入、回填、外部修改检测和会话读取以它为准。
    pub primary: String,
    #[serde(default)]
    pub windows: WindowsTarget,
    #[serde(default)]
    pub wsl: Vec<WslTarget>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AppTargetsConfig {
    fn default() -> Self {
        Self {
            primary: WINDOWS.to_string(),
            windows: WindowsTarget {
                enabled: true,
                dir: None,
            },
            wsl: Vec::new(),
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TargetId {
    Windows,
    Wsl(String),
}

impl TargetId {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw == WINDOWS {
            return Some(Self::Windows);
        }
        raw.strip_prefix(WSL_PREFIX)
            .filter(|distro| !distro.is_empty())
            .map(|distro| Self::Wsl(distro.to_string()))
    }

    pub fn key(&self) -> String {
        match self {
            Self::Windows => WINDOWS.to_string(),
            Self::Wsl(distro) => format!("{WSL_PREFIX}{distro}"),
        }
    }

    /// 同一个目标（发行版名称不区分大小写）。
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Windows, Self::Windows) => true,
            (Self::Wsl(a), Self::Wsl(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }

    pub fn distro(&self) -> Option<&str> {
        match self {
            Self::Windows => None,
            Self::Wsl(distro) => Some(distro),
        }
    }

    /// 目录名安全的短标识，用于这个目标自己的状态目录。
    pub fn slug(&self) -> String {
        match self {
            Self::Windows => WINDOWS.to_string(),
            Self::Wsl(distro) => {
                let cleaned: String = distro
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                            c.to_ascii_lowercase()
                        } else {
                            '_'
                        }
                    })
                    .collect();
                format!("wsl-{cleaned}")
            }
        }
    }
}

/// 一个目标的视图（不区分 Windows / WSL 的字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSpec {
    pub id: TargetId,
    pub enabled: bool,
    pub dir: Option<String>,
}

impl AppTargetsConfig {
    pub fn targets(&self) -> Vec<TargetSpec> {
        let mut out = vec![TargetSpec {
            id: TargetId::Windows,
            enabled: self.windows.enabled,
            dir: clean(self.windows.dir.as_deref()),
        }];
        out.extend(self.wsl.iter().map(|target| TargetSpec {
            id: TargetId::Wsl(target.distro.clone()),
            enabled: target.enabled,
            dir: clean(target.dir.as_deref()),
        }));
        out
    }

    pub fn enabled_targets(&self) -> Vec<TargetSpec> {
        self.targets().into_iter().filter(|t| t.enabled).collect()
    }

    pub fn primary_id(&self) -> Option<TargetId> {
        TargetId::parse(&self.primary)
    }

    pub fn target(&self, id: &TargetId) -> Option<TargetSpec> {
        self.targets().into_iter().find(|t| t.id.same(id))
    }

    /// 已启用、不是主环境的目标。
    pub fn followers(&self) -> Vec<TargetSpec> {
        let primary = self.primary_id();
        self.enabled_targets()
            .into_iter()
            .filter(|t| primary.as_ref().is_none_or(|p| !p.same(&t.id)))
            .collect()
    }

    /// 只有 Windows、且 Windows 是主环境：和旧单目录行为相同。
    pub fn is_windows_only(&self) -> bool {
        let enabled = self.enabled_targets();
        enabled.len() == 1 && enabled[0].id == TargetId::Windows && self.primary == WINDOWS
    }

    /// 去掉空白目录，规范主环境键的大小写。
    pub fn normalized(mut self) -> Self {
        self.windows.dir = clean(self.windows.dir.as_deref());
        for target in &mut self.wsl {
            target.distro = target.distro.trim().to_string();
            target.dir = clean(target.dir.as_deref());
        }
        if let Some(TargetId::Wsl(distro)) = self.primary_id() {
            if let Some(target) = self
                .wsl
                .iter()
                .find(|t| t.distro.eq_ignore_ascii_case(&distro))
            {
                self.primary = TargetId::Wsl(target.distro.clone()).key();
            }
        }
        self
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// 目标配置从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigSource {
    /// 用户在多环境表单里保存过。
    Saved,
    /// 从旧单目录推导，未写盘（迁移是只读、幂等的）。
    Legacy,
    /// 旧目录是 WSL 路径但认不出发行版：保留旧单目录行为，待处理。
    LegacyUnresolved,
}

/// 按迁移规则从旧单目录推导目标：
/// 1. 没有旧目录或是本机路径：只启用 Windows，旧自定义路径保留；
/// 2. 旧目录是 WSL UNC：只启用解析出的发行版，路径原样保留为自定义目录，主环境指向它；
/// 4. 认不出发行版：保留旧单目录行为并标待处理。
pub fn derive_from_legacy(legacy_dir: Option<&str>) -> (AppTargetsConfig, ConfigSource) {
    let legacy = clean(legacy_dir);
    match legacy.as_deref() {
        Some(dir) if looks_like_wsl_unc(dir) => match parse_wsl_unc(dir) {
            Some((distro, _)) if is_valid_distro_name(&distro) => (
                AppTargetsConfig {
                    primary: TargetId::Wsl(distro.clone()).key(),
                    windows: WindowsTarget {
                        enabled: false,
                        dir: None,
                    },
                    wsl: vec![WslTarget {
                        distro,
                        enabled: true,
                        dir: Some(dir.to_string()),
                    }],
                    extra: Map::new(),
                },
                ConfigSource::Legacy,
            ),
            _ => (
                AppTargetsConfig {
                    windows: WindowsTarget {
                        enabled: true,
                        dir: legacy.clone(),
                    },
                    ..AppTargetsConfig::default()
                },
                ConfigSource::LegacyUnresolved,
            ),
        },
        _ => (
            AppTargetsConfig {
                windows: WindowsTarget {
                    enabled: true,
                    dir: legacy,
                },
                ..AppTargetsConfig::default()
            },
            ConfigSource::Legacy,
        ),
    }
}

/// 有效的目标配置：保存过的优先，否则从旧单目录推导。
pub fn effective(
    saved: Option<&BTreeMap<String, AppTargetsConfig>>,
    app: &AppType,
    legacy_dir: Option<&str>,
) -> (AppTargetsConfig, ConfigSource) {
    if let Some(config) = saved.and_then(|all| all.get(app.as_str())) {
        return (config.clone().normalized(), ConfigSource::Saved);
    }
    derive_from_legacy(legacy_dir)
}

pub fn is_valid_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// 去掉 `\\?\UNC\` 前缀、统一成反斜杠。
fn unc_body(path: &str) -> Option<String> {
    let unified = path.trim().replace('/', "\\");
    let lower = unified.to_ascii_lowercase();
    if lower.starts_with(r"\\?\unc\") {
        return Some(unified[r"\\?\UNC\".len()..].to_string());
    }
    unified.strip_prefix(r"\\").map(str::to_string)
}

/// 是不是 `\\wsl$\…` 或 `\\wsl.localhost\…`（不论发行版能否认出）。
pub fn looks_like_wsl_unc(path: &str) -> bool {
    unc_body(path).is_some_and(|body| {
        let server = body.split('\\').next().unwrap_or_default();
        server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
    })
}

/// 拆出 WSL UNC 路径里的发行版和其后的 Linux 路径（`/` 开头）。
pub fn parse_wsl_unc(path: &str) -> Option<(String, String)> {
    let body = unc_body(path)?;
    let mut parts = body.split('\\');
    let server = parts.next()?;
    if !server.eq_ignore_ascii_case("wsl$") && !server.eq_ignore_ascii_case("wsl.localhost") {
        return None;
    }
    let distro = parts.next().filter(|d| !d.is_empty())?.to_string();
    let rest: Vec<&str> = parts.filter(|p| !p.is_empty()).collect();
    Some((distro, format!("/{}", rest.join("/"))))
}

/// 发行版内的 Linux 绝对路径 → `\\wsl.localhost\<发行版>\…`。
pub fn wsl_unc_for(distro: &str, linux_path: &str) -> String {
    let rest = linux_path
        .split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\\");
    if rest.is_empty() {
        format!(r"\\wsl.localhost\{distro}")
    } else {
        format!(r"\\wsl.localhost\{distro}\{rest}")
    }
}

/// 比较两个目的地是否相同用的键：大小写、斜杠方向、`wsl$` 与 `wsl.localhost` 别名、
/// 结尾分隔符都归一。
pub fn destination_key(path: &str) -> String {
    if let Some((distro, rest)) = parse_wsl_unc(path) {
        return format!(
            r"\\wsl.localhost\{}{}",
            distro.to_ascii_lowercase(),
            rest.replace('/', "\\").trim_end_matches('\\')
        );
    }
    let mut key = path.trim().replace('/', "\\").to_ascii_lowercase();
    while key.len() > 3 && key.ends_with('\\') {
        key.pop();
    }
    key
}

/// 各应用在 Linux 里的默认配置目录（按发行版里的环境变量和 HOME 推导）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxEnv {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xdg_config_home: Option<String>,
}

fn absolute_linux(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| v.starts_with('/'))
        .map(|v| {
            let trimmed = v.trim_end_matches('/');
            if trimmed.is_empty() {
                "/".to_string()
            } else {
                trimmed.to_string()
            }
        })
}

pub fn linux_default_dir(app: &AppType, env: &LinuxEnv) -> Option<String> {
    let home = absolute_linux(env.home.as_deref());
    let under_home = |suffix: &str| {
        home.as_ref().map(|home| {
            if home == "/" {
                format!("/{suffix}")
            } else {
                format!("{home}/{suffix}")
            }
        })
    };
    match app {
        AppType::Claude => {
            absolute_linux(env.claude_config_dir.as_deref()).or_else(|| under_home(".claude"))
        }
        AppType::Codex => {
            absolute_linux(env.codex_home.as_deref()).or_else(|| under_home(".codex"))
        }
        AppType::Gemini => under_home(".gemini"),
        AppType::OpenCode => absolute_linux(env.xdg_config_home.as_deref())
            .map(|xdg| format!("{}/opencode", xdg.trim_end_matches('/')))
            .or_else(|| under_home(".config/opencode")),
        _ => None,
    }
}

/// 校验失败的一项。`target` 为空表示整体错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub code: &'static str,
}

impl FieldError {
    fn at(target: &TargetId, code: &'static str) -> Self {
        Self {
            target: Some(target.key()),
            code,
        }
    }
    fn whole(code: &'static str) -> Self {
        Self { target: None, code }
    }
}

fn has_parent_segment(path: &str) -> bool {
    path.split(['\\', '/']).any(|segment| segment == "..")
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let unc = unc_body(path).is_some_and(|body| {
        let mut parts = body.split('\\');
        parts.next().is_some_and(|s| !s.is_empty()) && parts.next().is_some_and(|s| !s.is_empty())
    });
    let home = path == "~" || path.starts_with("~/") || path.starts_with("~\\");
    // 单测在非 Windows 宿主上用临时目录模拟各个目标。
    let test_host = cfg!(test) && path.starts_with('/');
    drive || unc || home || test_host
}

/// 校验一份目标配置。`resolved` 给出每个已启用目标解析后的目录（自动目录解析不出来时
/// 没有这一项），用来查重复目的地和「未获取到用户目录」。
pub fn validate(config: &AppTargetsConfig, resolved: &BTreeMap<String, String>) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let enabled = config.enabled_targets();
    if enabled.is_empty() {
        errors.push(FieldError::whole("noTarget"));
    }
    match config.primary_id() {
        Some(primary) if enabled.iter().any(|t| t.id.same(&primary)) => {}
        _ => errors.push(FieldError::whole("primaryNotEnabled")),
    }

    let mut seen_distros: Vec<String> = Vec::new();
    for target in &config.wsl {
        let id = TargetId::Wsl(target.distro.clone());
        if !is_valid_distro_name(target.distro.trim()) {
            errors.push(FieldError::at(&id, "invalidDistro"));
            continue;
        }
        let lower = target.distro.to_ascii_lowercase();
        if seen_distros.contains(&lower) {
            errors.push(FieldError::at(&id, "duplicateDistro"));
        }
        seen_distros.push(lower);
    }

    for target in config.targets() {
        // 未勾选目标的自定义目录也要合法，免得之后勾选时才发现。
        let Some(dir) = target.dir.as_deref() else {
            // Windows 默认目录总能解析；只有 WSL 的自动目录可能拿不到 HOME。
            if target.enabled
                && matches!(target.id, TargetId::Wsl(_))
                && !resolved.contains_key(&target.id.key())
            {
                errors.push(FieldError::at(&target.id, "homeUnresolved"));
            }
            continue;
        };
        if dir.chars().any(|c| c.is_control()) {
            errors.push(FieldError::at(&target.id, "controlChar"));
            continue;
        }
        if has_parent_segment(dir) {
            errors.push(FieldError::at(&target.id, "parentSegment"));
            continue;
        }
        match &target.id {
            TargetId::Windows => {
                if looks_like_wsl_unc(dir) {
                    errors.push(FieldError::at(&target.id, "windowsRejectsWsl"));
                } else if !is_windows_absolute(dir) {
                    errors.push(FieldError::at(&target.id, "windowsNotAbsolute"));
                } else if dir
                    .get(2..)
                    .is_some_and(|tail| tail.contains([':', '<', '>', '"', '|', '?', '*']))
                {
                    errors.push(FieldError::at(&target.id, "windowsInvalidChar"));
                }
            }
            TargetId::Wsl(distro) => match parse_wsl_unc(dir) {
                Some((dir_distro, _)) if dir_distro.eq_ignore_ascii_case(distro) => {}
                // 单测在非 Windows 宿主上用临时目录模拟 WSL 目标。
                None if cfg!(test) && dir.starts_with('/') => {}
                _ => errors.push(FieldError::at(&target.id, "wslDistroMismatch")),
            },
        }
    }

    // 重复目的地：两个已启用目标实际写同一个目录。
    let mut destinations: Vec<(String, TargetId)> = Vec::new();
    for target in &enabled {
        let Some(dir) = target
            .dir
            .clone()
            .or_else(|| resolved.get(&target.id.key()).cloned())
        else {
            continue;
        };
        let key = destination_key(&dir);
        if destinations.iter().any(|(existing, _)| *existing == key) {
            errors.push(FieldError::at(&target.id, "duplicateDestination"));
        } else {
            destinations.push((key, target.id.clone()));
        }
    }

    errors
}

/// 旧 `*ConfigDir` 的兼容投影：主环境的有效目录。Windows 主环境没有自定义目录时为
/// `None`（沿用默认解析）；WSL 主环境用自定义目录或解析出的自动目录。
pub fn legacy_projection(
    config: &AppTargetsConfig,
    resolved: &BTreeMap<String, String>,
) -> Option<String> {
    let primary = config.primary_id()?;
    let target = config.target(&primary)?;
    match primary {
        TargetId::Windows => target.dir,
        TargetId::Wsl(_) => target
            .dir
            .or_else(|| resolved.get(&target.id.key()).cloned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn legacy_local_dir_keeps_windows_only() {
        let (config, source) = derive_from_legacy(Some(r"D:\cfg\.claude"));
        assert_eq!(source, ConfigSource::Legacy);
        assert!(config.is_windows_only());
        assert_eq!(config.windows.dir.as_deref(), Some(r"D:\cfg\.claude"));
        let (config, _) = derive_from_legacy(None);
        assert!(config.is_windows_only());
        assert_eq!(config.windows.dir, None);
    }

    #[test]
    fn legacy_wsl_unc_migrates_to_that_distro_only() {
        let legacy = r"\\wsl$\Ubuntu-24.04\home\me\.claude";
        let (config, source) = derive_from_legacy(Some(legacy));
        assert_eq!(source, ConfigSource::Legacy);
        assert_eq!(config.primary, "wsl:Ubuntu-24.04");
        assert!(!config.windows.enabled);
        assert_eq!(config.wsl.len(), 1);
        assert!(config.wsl[0].enabled);
        assert_eq!(config.wsl[0].dir.as_deref(), Some(legacy));
        // 投影回去还是原路径：升级后仍只写原目录。
        assert_eq!(
            legacy_projection(&config, &BTreeMap::new()).as_deref(),
            Some(legacy)
        );
        // 幂等：再从投影推导一次，结果相同。
        assert_eq!(derive_from_legacy(Some(legacy)).0, config);
    }

    #[test]
    fn unrecognised_wsl_path_keeps_the_legacy_behaviour() {
        let (config, source) = derive_from_legacy(Some(r"\\wsl$\"));
        assert_eq!(source, ConfigSource::LegacyUnresolved);
        assert_eq!(config.windows.dir.as_deref(), Some(r"\\wsl$\"));
        assert_eq!(
            legacy_projection(&config, &BTreeMap::new()).as_deref(),
            Some(r"\\wsl$\")
        );
    }

    #[test]
    fn saved_config_wins_over_legacy() {
        let mut all = BTreeMap::new();
        let saved = AppTargetsConfig {
            primary: "wsl:ubuntu".into(),
            windows: WindowsTarget {
                enabled: true,
                dir: Some("  ".into()),
            },
            wsl: vec![WslTarget {
                distro: "Ubuntu".into(),
                enabled: true,
                dir: None,
            }],
            extra: Map::new(),
        };
        all.insert("claude".to_string(), saved);
        let (config, source) = effective(Some(&all), &AppType::Claude, Some(r"D:\x"));
        assert_eq!(source, ConfigSource::Saved);
        assert_eq!(config.primary, "wsl:Ubuntu", "primary key normalised");
        assert_eq!(config.windows.dir, None, "blank dir dropped");
        assert_eq!(config.followers().len(), 1);
        assert_eq!(config.followers()[0].id, TargetId::Windows);
    }

    #[test]
    fn unc_parsing_handles_aliases_and_prefixes() {
        assert_eq!(
            parse_wsl_unc(r"\\wsl.localhost\Debian\opt\dev\.gemini"),
            Some(("Debian".into(), "/opt/dev/.gemini".into()))
        );
        assert_eq!(
            parse_wsl_unc("//wsl$/Ubuntu/home/a"),
            Some(("Ubuntu".into(), "/home/a".into()))
        );
        assert_eq!(
            parse_wsl_unc(r"\\?\UNC\wsl$\Ubuntu\root"),
            Some(("Ubuntu".into(), "/root".into()))
        );
        assert_eq!(parse_wsl_unc(r"\\server\share\x"), None);
        assert_eq!(parse_wsl_unc(r"C:\Users"), None);
        assert_eq!(
            destination_key(r"\\wsl$\UBUNTU\home\me\.claude\"),
            destination_key(r"\\wsl.localhost\Ubuntu\home\me\.claude")
        );
        assert_eq!(
            wsl_unc_for("Ubuntu", "/home/me/.codex"),
            r"\\wsl.localhost\Ubuntu\home\me\.codex"
        );
    }

    #[test]
    fn linux_defaults_follow_each_apps_rules() {
        let env = LinuxEnv {
            home: Some("/home/me/".into()),
            ..Default::default()
        };
        assert_eq!(
            linux_default_dir(&AppType::Claude, &env).as_deref(),
            Some("/home/me/.claude")
        );
        assert_eq!(
            linux_default_dir(&AppType::OpenCode, &env).as_deref(),
            Some("/home/me/.config/opencode")
        );
        let env = LinuxEnv {
            home: Some("/root".into()),
            codex_home: Some("/srv/codex".into()),
            xdg_config_home: Some("/x/cfg".into()),
            claude_config_dir: Some("relative/ignored".into()),
        };
        assert_eq!(
            linux_default_dir(&AppType::Codex, &env).as_deref(),
            Some("/srv/codex")
        );
        assert_eq!(
            linux_default_dir(&AppType::OpenCode, &env).as_deref(),
            Some("/x/cfg/opencode")
        );
        assert_eq!(
            linux_default_dir(&AppType::Claude, &env).as_deref(),
            Some("/root/.claude")
        );
        assert_eq!(
            linux_default_dir(&AppType::Gemini, &LinuxEnv::default()),
            None
        );
    }

    fn two_targets() -> AppTargetsConfig {
        AppTargetsConfig {
            primary: WINDOWS.into(),
            windows: WindowsTarget {
                enabled: true,
                dir: None,
            },
            wsl: vec![WslTarget {
                distro: "Ubuntu".into(),
                enabled: true,
                dir: None,
            }],
            extra: Map::new(),
        }
    }

    fn codes(errors: &[FieldError]) -> Vec<&'static str> {
        errors.iter().map(|e| e.code).collect()
    }

    #[test]
    fn validation_requires_a_target_and_an_enabled_primary() {
        let mut config = two_targets();
        config.windows.enabled = false;
        config.wsl[0].enabled = false;
        let errors = validate(&config, &BTreeMap::new());
        assert!(codes(&errors).contains(&"noTarget"));
        assert!(codes(&errors).contains(&"primaryNotEnabled"));

        let mut config = two_targets();
        config.primary = "wsl:Debian".into();
        let errors = validate(
            &config,
            &resolved(&[("wsl:Ubuntu", r"\\wsl.localhost\Ubuntu\home\me\.claude")]),
        );
        assert_eq!(codes(&errors), vec!["primaryNotEnabled"]);
    }

    #[test]
    fn validation_keeps_wsl_paths_on_their_own_row() {
        let mut config = two_targets();
        config.windows.dir = Some(r"\\wsl$\Ubuntu\home\me\.claude".into());
        config.wsl[0].dir = Some(r"\\wsl.localhost\Debian\home\me\.claude".into());
        let errors = validate(&config, &BTreeMap::new());
        assert_eq!(
            errors,
            vec![
                FieldError::at(&TargetId::Windows, "windowsRejectsWsl"),
                FieldError::at(&TargetId::Wsl("Ubuntu".into()), "wslDistroMismatch"),
            ]
        );
    }

    #[test]
    fn validation_rejects_relative_parent_and_unresolved_dirs() {
        let mut config = two_targets();
        config.windows.dir = Some(r"relative\dir".into());
        let errors = validate(&config, &BTreeMap::new());
        assert_eq!(
            codes(&errors),
            vec!["windowsNotAbsolute", "homeUnresolved"],
            "{errors:?}"
        );

        let mut config = two_targets();
        config.wsl[0].dir = Some(r"\\wsl$\Ubuntu\home\..\etc".into());
        let errors = validate(&config, &BTreeMap::new());
        assert_eq!(codes(&errors), vec!["parentSegment"]);

        let mut config = two_targets();
        config.windows.dir = Some(r"C:\Users\me\.claude".into());
        config.wsl[0].dir = Some(r"\\wsl$\Ubuntu\home\me\.claude".into());
        assert!(validate(&config, &BTreeMap::new()).is_empty());

        // 未勾选的行解析不出 HOME 不算错。
        let mut config = two_targets();
        config.wsl[0].enabled = false;
        assert!(validate(&config, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn validation_detects_duplicate_destinations_through_aliases() {
        let mut config = two_targets();
        config.wsl.push(WslTarget {
            distro: "Debian".into(),
            enabled: true,
            dir: Some(r"\\wsl$\Debian\home\me\.claude".into()),
        });
        config.wsl[0].dir = Some(r"\\wsl$\Ubuntu\home\me\.claude".into());
        assert!(validate(&config, &BTreeMap::new()).is_empty());

        let auto = resolved(&[("wsl:Ubuntu", r"\\wsl.localhost\Ubuntu\home\me\.claude")]);
        let mut config = two_targets();
        config.wsl.push(WslTarget {
            distro: "ubuntu".into(),
            enabled: false,
            dir: None,
        });
        let errors = validate(&config, &auto);
        assert_eq!(codes(&errors), vec!["duplicateDistro"]);
    }

    #[test]
    fn projection_points_at_the_primary_environment() {
        let mut config = two_targets();
        config.windows.dir = Some(r"D:\c".into());
        assert_eq!(
            legacy_projection(&config, &BTreeMap::new()).as_deref(),
            Some(r"D:\c")
        );
        config.primary = "wsl:Ubuntu".into();
        let auto = resolved(&[("wsl:Ubuntu", r"\\wsl.localhost\Ubuntu\home\me\.claude")]);
        assert_eq!(
            legacy_projection(&config, &auto).as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\me\.claude")
        );
    }

    #[test]
    fn target_ids_round_trip() {
        assert_eq!(TargetId::parse("windows"), Some(TargetId::Windows));
        assert_eq!(
            TargetId::parse("wsl:Ubuntu-24.04"),
            Some(TargetId::Wsl("Ubuntu-24.04".into()))
        );
        assert_eq!(TargetId::parse("wsl:"), None);
        assert_eq!(TargetId::parse("linux"), None);
        assert!(TargetId::Wsl("A".into()).same(&TargetId::Wsl("a".into())));
        assert_eq!(TargetId::Wsl("Ubuntu 24".into()).slug(), "wsl-ubuntu_24");
    }
}
