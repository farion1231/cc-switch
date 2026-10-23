//! 安装和升级前的 Node 版本检查。npm 包在 `engines.node` 里声明 Node 要求后（OpenClaw
//! 2026.9.3 起要 24.16+），用旧 Node 升级必然失败：OpenClaw 自己的 `update` 预检和包的
//! preinstall 都会拒绝；没有自带检查的包则会被 npm 照装、装完跑不起来。所以「应用」页
//! 提前标出来、不给升级按钮，执行升级前再查一次。认不出、查不到时一律放行。

use super::*;

// The published CLI omits engines.node. The repository root at the matching
// dsh-v0.2.1-rc.2 / rc.3 release tags declares this range; do not take the
// requirement from the newer development branch.
const DSH_NODE_REQUIREMENT: &str = "^22.19.0 || >=24.0.0";

fn required_node_range(tool: &str, declared: Option<String>) -> Option<String> {
    declared.or_else(|| (tool == "dsh").then(|| DSH_NODE_REQUIREMENT.to_string()))
}

fn node_gap(required: &str, current: &str) -> Option<NodeRequirementGap> {
    (node_range_satisfied(required, current) == Some(false)).then(|| NodeRequirementGap {
        required: required.into(),
        current: current.into(),
    })
}

fn install_uses_npm(tool: &str, shell: LifecycleCommandShell) -> bool {
    let Some(npm) = npm_install_command_for(tool) else {
        return false;
    };
    let command = match shell {
        LifecycleCommandShell::Posix => Some(posix_install_command_for(tool)),
        LifecycleCommandShell::WindowsBatch => {
            tool_action_shell_command_for_shell(tool, ToolLifecycleAction::Install, shell)
        }
    };
    // A native installer with an npm fallback must remain usable without system Node.
    command.as_deref() == Some(npm)
}

pub(super) async fn install_node_requirement(
    client: &reqwest::Client,
    tool: &str,
    target: &str,
) -> Option<String> {
    #[cfg(target_os = "windows")]
    let shell = if wsl_distro_for_tool(tool).is_some() {
        LifecycleCommandShell::Posix
    } else {
        LifecycleCommandShell::WindowsBatch
    };
    #[cfg(not(target_os = "windows"))]
    let shell = LifecycleCommandShell::Posix;
    if !install_uses_npm(tool, shell) {
        return None;
    }
    let package = npm_package_for(tool)?;
    required_node_range(tool, fetch_engines_node(client, package, target).await)
}

fn install_node_version(
    tool: &str,
    preference: Option<&WslShellPreferenceInput>,
) -> Option<String> {
    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_for_tool(tool) {
        return match try_get_version_wsl(
            "node",
            &distro,
            preference.and_then(|pref| pref.wsl_shell.as_deref()),
            preference.and_then(|pref| pref.wsl_shell_flag.as_deref()),
        ) {
            ShellProbe::Found(version) => Some(version),
            _ => None,
        };
    }
    let _ = (tool, preference);
    // npm launchers prefer a sibling node, then the effective PATH.
    let npm = resolve_path_default(
        "npm",
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )
    .ok()
    .flatten();
    node_version_next_to(npm.as_deref())
}

pub(super) async fn install_node_gap(
    client: &reqwest::Client,
    tool: &str,
    target: &str,
    preference: Option<&WslShellPreferenceInput>,
) -> Option<NodeRequirementGap> {
    let required = install_node_requirement(client, tool, target).await?;
    let tool = tool.to_owned();
    let preference = preference.cloned();
    let current =
        tokio::task::spawn_blocking(move || install_node_version(&tool, preference.as_ref()))
            .await
            .ok()??;
    node_gap(&required, &current)
}

fn requirement_error(tool: &str, gap: &NodeRequirementGap) -> String {
    serde_json::json!({"code":"TOOL_NODE_REQUIREMENT", "tool":tool, "required":gap.required, "current":gap.current}).to_string()
}

pub(super) fn ensure_install_node(
    tool: &str,
    required: Option<&str>,
    preference: Option<&WslShellPreferenceInput>,
) -> Result<(), String> {
    let gap = required.and_then(|range| {
        install_node_version(tool, preference).and_then(|current| node_gap(range, &current))
    });
    match gap {
        Some(gap) => Err(requirement_error(tool, &gap)),
        None => Ok(()),
    }
}

/// 目标应用要求的 Node 和安装／升级会用到的 Node 对不上。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NodeRequirementGap {
    /// npm engines.node 或已核实的官方运行时要求，原样展示。
    required: String,
    /// 升级会用到的 node 的版本（不带 `v`）。
    current: String,
}

/// 升级到 `target` 会不会被 Node 版本挡住。只看 npm 包、且默认那处由 node 运行的安装。
pub(super) async fn update_node_gap(
    client: &reqwest::Client,
    tool: &str,
    target: &str,
) -> Option<NodeRequirementGap> {
    let package = npm_package_for(tool)?;
    #[cfg(target_os = "windows")]
    if wsl_distro_for_tool(tool).is_some() {
        return None;
    }
    let owned = tool.to_string();
    let current = tokio::task::spawn_blocking(move || update_node_version(&owned))
        .await
        .ok()??;
    let required = required_node_range(tool, fetch_engines_node(client, package, target).await)?;
    node_gap(&required, &current)
}

/// 执行升级前的检查：目标是 `latest`（`npm i -g <pkg>@latest` 与各家 self-update 的稳定通道）。
pub(super) async fn ensure_update_node(tool: &str) -> Result<(), String> {
    let Some(package) = npm_package_for(tool) else {
        return Ok(());
    };
    let client = crate::proxy::http_client::get();
    let Some(latest) = fetch_npm_dist_tags(&client, package)
        .await
        .and_then(|tags| tags.get("latest")?.as_str().map(str::to_string))
    else {
        return Ok(());
    };
    match update_node_gap(&client, tool, &latest).await {
        Some(gap) => Err(requirement_error(tool, &gap)),
        None => Ok(()),
    }
}

/// 升级默认那处时用到的 node 版本：与锚定升级同源，先找安装旁边的 node，再按 PATH 找。
/// 默认那处不由 node 运行（原生二进制、Homebrew 管、bun）时返回 None。
fn update_node_version(tool: &str) -> Option<String> {
    let installs = enumerate_tool_installations(tool);
    let inst = default_install(&installs)?;
    let real = inst.real.to_string_lossy();
    if tool == "mcode"
        && (mcode_receipt_install_root(&inst.path, McodeLayout::NATIVE).is_some()
            || mcode_script_install_root(&inst.path, &real, McodeLayout::NATIVE).is_some())
    {
        return None;
    }
    if inst.source == "bun"
        || (!real.replace('\\', "/").contains("/node_modules/")
            && tool_uninstall::npm_prefix(tool, inst).is_none())
    {
        return None;
    }
    #[cfg(not(target_os = "windows"))]
    if is_brew_managed_path(&real) {
        return None;
    }

    node_version_next_to(Some(Path::new(&inst.path)))
}

fn node_version_next_to(launcher: Option<&Path>) -> Option<String> {
    let sibling = launcher
        .and_then(Path::parent)
        .map(|dir| {
            dir.join(if cfg!(target_os = "windows") {
                "node.exe"
            } else {
                "node"
            })
        })
        .filter(|node| node.is_file());
    let node = match sibling {
        Some(node) => node,
        None => resolve_path_default(
            "node",
            CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
        )
        .ok()
        .flatten()?,
    };
    let dir = node.parent()?;
    #[cfg(not(target_os = "windows"))]
    let path = prepend_search_dir_to_path(dir, &effective_path_os().unwrap_or_default());
    #[cfg(target_os = "windows")]
    let path = format!("{};{}", dir.display(), effective_path_string());
    let output = run_probe_version_command(&node, &path).ok()?;
    if !output.status.success() {
        return None;
    }
    let text = decode_command_output(&output.stdout);
    VERSION_RE.find(&text).map(|m| m.as_str().to_string())
}

/// 读 `<package>@<version>` 的 `engines.node`。单个版本的元数据，不是整份 packument。
async fn fetch_engines_node(
    client: &reqwest::Client,
    package: &str,
    version: &str,
) -> Option<String> {
    let url = format!(
        "https://registry.npmjs.org/{}/{version}",
        package.replace('/', "%2f")
    );
    let json = client
        .get(url)
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?;
    let range = json.get("engines")?.get("node")?.as_str()?.trim();
    (!range.is_empty()).then(|| range.to_string())
}

type Core = [u64; 3];

#[derive(Debug, Clone, Copy)]
enum Bound {
    Ge(Core),
    Gt(Core),
    Le(Core),
    Lt(Core),
}

impl Bound {
    fn admits(self, v: Core) -> bool {
        match self {
            Bound::Ge(b) => v >= b,
            Bound::Gt(b) => v > b,
            Bound::Le(b) => v <= b,
            Bound::Lt(b) => v < b,
        }
    }
}

/// `version` 是否满足 npm 版本范围 `range`。支持比较符、`^`、`~`、x 通配、`a - b` 与 `||`；
/// 认不出的写法返回 None，调用方按「不确定」放行。
fn node_range_satisfied(range: &str, version: &str) -> Option<bool> {
    let (version, _) = parse_semver(version.trim().trim_start_matches('v'))?;
    let mut unknown = false;
    for set in range.split("||") {
        match comparator_set_admits(set, version) {
            Some(true) => return Some(true),
            Some(false) => {}
            None => unknown = true,
        }
    }
    (!unknown).then_some(false)
}

fn comparator_set_admits(set: &str, version: Core) -> Option<bool> {
    let tokens: Vec<&str> = set.split_whitespace().collect();
    let mut bounds = Vec::new();
    if let [low, "-", high] = tokens.as_slice() {
        bounds.extend(comparator(">=", &parse_partial(low)?)?);
        bounds.extend(comparator("<=", &parse_partial(high)?)?);
    } else {
        // `>= 24` 这种比较符与版本之间带空格的写法也合法。
        let mut pending: Option<&str> = None;
        for token in tokens {
            let (op, rest) = split_operator(token);
            let op = match pending.take() {
                Some(_) if !op.is_empty() => return None,
                Some(pending) => pending,
                None => op,
            };
            if rest.is_empty() {
                pending = Some(op);
                continue;
            }
            bounds.extend(comparator(op, &parse_partial(rest)?)?);
        }
        if pending.is_some() {
            return None;
        }
    }
    Some(bounds.into_iter().all(|bound| bound.admits(version)))
}

fn split_operator(token: &str) -> (&str, &str) {
    for op in [">=", "<=", ">", "<", "=", "^", "~"] {
        if let Some(rest) = token.strip_prefix(op) {
            return (op, rest);
        }
    }
    ("", token)
}

/// `24` / `24.16` / `24.16.0` / `24.x` / `*` → 写出来的那几段；`v` 前缀与预发布段忽略。
fn parse_partial(raw: &str) -> Option<Vec<u64>> {
    let core = raw.trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = Vec::new();
    for segment in core.split('.') {
        if matches!(segment, "x" | "X" | "*") {
            break;
        }
        parts.push(segment.parse().ok()?);
    }
    (parts.len() <= 3).then_some(parts)
}

fn comparator(op: &str, parts: &[u64]) -> Option<Vec<Bound>> {
    let at = |i: usize| parts.get(i).copied().unwrap_or(0);
    let floor = [at(0), at(1), at(2)];
    // 最后写出的那段 +1、其后归零：`24` → 25.0.0，`24.16` → 24.17.0。
    let next = || {
        let mut core = floor;
        core[parts.len() - 1] += 1;
        for slot in core.iter_mut().skip(parts.len()) {
            *slot = 0;
        }
        core
    };
    let exact = parts.len() == 3;
    if parts.is_empty() {
        return matches!(op, "" | "=" | ">=" | "<=" | "^" | "~").then(Vec::new);
    }
    Some(match op {
        "" | "=" if exact => vec![Bound::Ge(floor), Bound::Le(floor)],
        "" | "=" => vec![Bound::Ge(floor), Bound::Lt(next())],
        ">=" => vec![Bound::Ge(floor)],
        ">" if exact => vec![Bound::Gt(floor)],
        ">" => vec![Bound::Ge(next())],
        "<" => vec![Bound::Lt(floor)],
        "<=" if exact => vec![Bound::Le(floor)],
        "<=" => vec![Bound::Lt(next())],
        "~" if parts.len() == 1 => vec![Bound::Ge(floor), Bound::Lt([at(0) + 1, 0, 0])],
        "~" => vec![Bound::Ge(floor), Bound::Lt([at(0), at(1) + 1, 0])],
        "^" => {
            let upper = if at(0) > 0 || parts.len() == 1 {
                [at(0) + 1, 0, 0]
            } else if at(1) > 0 || parts.len() == 2 {
                [0, at(1) + 1, 0]
            } else {
                [0, 0, at(2) + 1]
            };
            vec![Bound::Ge(floor), Bound::Lt(upper)]
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_checks_follow_the_selected_installer() {
        for tool in ["codex", "gemini", "openclaw", "pi", "dsh"] {
            assert!(
                install_uses_npm(tool, LifecycleCommandShell::Posix),
                "{tool}"
            );
            assert!(
                install_uses_npm(tool, LifecycleCommandShell::WindowsBatch),
                "{tool}"
            );
        }
        for tool in [
            "claude", "opencode", "grok", "mcode", "kimi", "hermes", "agy",
        ] {
            assert!(
                !install_uses_npm(tool, LifecycleCommandShell::Posix),
                "{tool}"
            );
        }
        #[cfg(target_os = "windows")]
        {
            for tool in ["claude", "opencode"] {
                assert!(
                    install_uses_npm(tool, LifecycleCommandShell::WindowsBatch),
                    "{tool}"
                );
            }
            for tool in ["grok", "mcode", "kimi", "hermes", "agy"] {
                assert!(
                    !install_uses_npm(tool, LifecycleCommandShell::WindowsBatch),
                    "{tool}"
                );
            }
        }
    }

    #[test]
    fn node_errors_preserve_structured_values_for_frontend_localization() {
        let gap = node_gap(">=24.22.1 <25 || >=26.12.1", "22.12.0").unwrap();
        let error: serde_json::Value =
            serde_json::from_str(&requirement_error("openclaw", &gap)).unwrap();
        assert_eq!(error["code"], "TOOL_NODE_REQUIREMENT");
        assert_eq!(error["tool"], "openclaw");
        assert_eq!(error["required"], ">=24.22.1 <25 || >=26.12.1");
        assert_eq!(error["current"], "22.12.0");
    }

    #[test]
    fn dsh_uses_official_node_requirement_when_npm_metadata_omits_engines() {
        let required = required_node_range("dsh", None).unwrap();
        assert!(node_gap(&required, "22.12.0").is_some());
        assert!(node_gap(&required, "22.19.0").is_none());
        assert!(node_gap(&required, "23.0.0").is_some());
        assert!(node_gap(&required, "24.0.0").is_none());
        assert!(node_gap(&required, "24.9.0").is_none());
        assert!(node_gap(&required, "26.0.0").is_none());
        assert_eq!(required_node_range("codex", None), None);
        let newer = required_node_range("dsh", Some(">=26.0.0".into())).unwrap();
        assert!(node_gap(&newer, "24.9.0").is_some());
    }

    #[test]
    fn openclaw_range_rejects_node_22_and_admits_24_and_26() {
        let range = ">=24.16.0 <25 || >=26.1.0";
        assert_eq!(node_range_satisfied(range, "22.23.2"), Some(false));
        assert_eq!(node_range_satisfied(range, "v24.15.9"), Some(false));
        assert_eq!(node_range_satisfied(range, "24.16.0"), Some(true));
        assert_eq!(node_range_satisfied(range, "25.0.0"), Some(false));
        assert_eq!(node_range_satisfied(range, "26.1.0"), Some(true));
    }

    #[test]
    fn common_engine_ranges() {
        assert_eq!(node_range_satisfied(">=20", "20.0.0"), Some(true));
        assert_eq!(node_range_satisfied(">=20", "18.20.8"), Some(false));
        assert_eq!(node_range_satisfied(">= 22.0.0", "22.1.0"), Some(true));
        assert_eq!(
            node_range_satisfied("^20.19.0 || >=22.12.0", "21.0.0"),
            Some(false)
        );
        assert_eq!(
            node_range_satisfied("^20.19.0 || >=22.12.0", "20.19.4"),
            Some(true)
        );
        assert_eq!(node_range_satisfied("~18.17", "18.18.0"), Some(false));
        assert_eq!(node_range_satisfied("18.x", "18.20.8"), Some(true));
        assert_eq!(node_range_satisfied("18 - 20", "20.9.0"), Some(true));
        assert_eq!(node_range_satisfied("18 - 20", "21.0.0"), Some(false));
        assert_eq!(node_range_satisfied(">18", "18.9.0"), Some(false));
        assert_eq!(node_range_satisfied("<=20", "20.9.0"), Some(true));
        assert_eq!(node_range_satisfied("*", "4.0.0"), Some(true));
        assert_eq!(node_range_satisfied("^0.2.3", "0.3.0"), Some(false));
    }

    #[test]
    fn unknown_syntax_never_blocks() {
        assert_eq!(node_range_satisfied("node >= 20", "18.0.0"), None);
        assert_eq!(node_range_satisfied(">=", "18.0.0"), None);
        // 一侧认不出、另一侧满足时仍算满足。
        assert_eq!(node_range_satisfied("lts/* || >=18", "20.0.0"), Some(true));
        assert_eq!(node_range_satisfied(">=20", "not-a-version"), None);
    }
}
