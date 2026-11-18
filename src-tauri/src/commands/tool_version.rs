use super::*;

/// 获取单个工具的版本信息（内部实现）
/// 本机工具的版本（只探测本地，不联网）。
fn probe_local_version(
    tool: &str,
    wsl_distro: Option<&str>,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
) -> Result<ShellProbe, String> {
    if let Some(distro) = wsl_distro {
        try_get_version_wsl(tool, distro, wsl_shell, wsl_shell_flag)
    } else {
        #[cfg(target_os = "windows")]
        {
            // Probe the PATH-default entry (what `tool` resolves to in a
            // terminal) first, and only fall back to the directory scan when it
            // is genuinely absent (NotFound). Two goals:
            // 1. Keep the displayed "current version" aligned with the version
            //    the user actually runs — a stale shim in a hardcoded fallback
            //    dir (e.g. an old `%APPDATA%\npm`) must not override a newer
            //    PATH install (#4701: "updated but still shows the old version").
            // 2. Mirror the non-Windows structure (`try_get_version` →
            //    `scan_cli_version`).
            // `probe_path_default_version` executes only the real executable
            //    resolved by `where` (App Execution Aliases filtered out), so
            //    it never `cmd /C tool` into a protocol handler.
            // Capture, launch, and path-resolution failures stay `Err`: they are
            // not absence, and must not be replaced by another installation.
            match probe_path_default_version(tool)? {
                ShellProbe::NotFound(_) => scan_cli_version(tool),
                found => Ok(found),
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // PATH 第一个命令优先；只有它确实没装(NotFound)才去常见目录兜底扫描。
            match try_get_version(tool)? {
                ShellProbe::NotFound(_) => scan_cli_version(tool),
                found => Ok(found),
            }
        }
    }
}

/// 本机实际安装的工具版本（和「关于」页探测的是同一个）；拿不到为 `None`。
pub(crate) fn local_tool_version(tool: &str) -> Option<String> {
    let (_, wsl_distro) = tool_env_type_and_wsl_distro(tool);
    match probe_local_version(tool, wsl_distro.as_deref(), None, None) {
        Ok(ShellProbe::Found(version)) => Some(version),
        _ => None,
    }
}

pub(super) async fn get_single_tool_version_impl(
    tool: &str,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
) -> ToolVersion {
    debug_assert!(
        known_tool(tool).is_some(),
        "unexpected tool name in get_single_tool_version_impl: {tool}"
    );

    // 判断该工具的运行环境 & WSL distro（如有）
    let (_, wsl_distro) = tool_env_type_and_wsl_distro(tool);

    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();

    // 1. 获取本地版本
    let probe = probe_local_version(tool, wsl_distro.as_deref(), wsl_shell, wsl_shell_flag);
    let mut result = tool_version_from_probe(tool, probe);

    // 2. 获取远程最新版本（npm 工具在本地领先 latest 时会按预发布通道补查，见
    //    fetch_npm_latest_for_tool / npm_prerelease_tags）
    let local = result.version.as_deref();
    let latest_version = match (tool, tool_spec(tool).npm_package) {
        ("hermes", _) => fetch_hermes_latest_version(&client, local).await,
        ("agy", _) => fetch_agy_latest_version(&client).await,
        ("opencode", Some(package)) => {
            match fetch_npm_latest_for_tool(&client, package, tool, local).await {
                Some(version) => Some(version),
                None => fetch_github_latest_version(&client, "anomalyco/opencode").await,
            }
        }
        (_, Some(package)) => fetch_npm_latest_for_tool(&client, package, tool, local).await,
        (_, None) => None,
    };

    // 3. 有新版本时，看新版本要求的 Node 与这处安装用的对不对得上
    let update_blocked_by_node = match (local, latest_version.as_deref()) {
        (Some(local), Some(latest))
            if compare_semver(local, latest) == Some(std::cmp::Ordering::Less) =>
        {
            tool_node_requirement::update_node_gap(&client, tool, latest).await
        }
        _ => None,
    };

    result.latest_version = latest_version;
    result.update_blocked_by_node = update_blocked_by_node;
    result
}

fn tool_version_from_probe(tool: &str, probe: Result<ShellProbe, String>) -> ToolVersion {
    let (version, error, installed, installed_but_broken) = match probe {
        Ok(ShellProbe::Found(v)) => (Some(v), None, Some(true), false),
        Ok(ShellProbe::VersionUnknown(e)) => (None, Some(e), Some(true), false),
        Ok(ShellProbe::FoundButFailed(e)) => (None, Some(e), Some(true), true),
        // 纯粹没装不带 error：版本列已显示「未安装」，再显示一行英文原因是重复的。
        Ok(ShellProbe::NotFound(e)) if e.ends_with(NOT_INSTALLED) => {
            (None, None, Some(false), false)
        }
        Ok(ShellProbe::NotFound(e)) => (None, Some(e), Some(false), false),
        // 捕获、启动、读取、等待或超时：安装状态未知，不能当成没装或装坏了。
        Err(e) => (None, Some(e), None, false),
    };
    let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(tool);
    ToolVersion {
        name: tool.to_string(),
        version,
        latest_version: None,
        error,
        installed,
        installed_but_broken,
        env_type,
        wsl_distro,
        update_blocked_by_node: None,
    }
}

/// 解析 "2.1.156" / "2.1.156-beta.1" → (主版本三段, 预发布段)。无法解析返回 None。
/// 与前端 `src/lib/version.ts` 的 parseVersion 语义对称(跨语言各实现一份)。
/// patch 用 u64 以容纳 codex 的 `0.1.2505172116` 时间戳式版本而不溢出。
pub(super) fn parse_semver(v: &str) -> Option<([u64; 3], Vec<String>)> {
    let v = v.trim();
    // `+local` 段命中开发构建形态(git describe `238.g<sha>`,或含 canary /
    // nightly / dev 等开发通道标识,如 Hermes main/canary 自报的
    // `0.22.1+238.g<sha>`)时,版本号可能低于最新 release、内容反而更新,与
    // release 不可比 —— 返回 None,调用方据此保守处理。普通 semver build
    // metadata(如 `1.0.0+linux.1`)按 semver 规范忽略后照常比较。
    if let Some((_, local)) = v.split_once('+') {
        if is_dev_local(local) {
            return None;
        }
    }
    let core_and_pre = v.split('+').next().unwrap_or("");
    let (core, pre) = match core_and_pre.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (core_and_pre, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None; // 多于三段,非法
    }
    let pre_segments = pre
        .map(|p| p.split('.').map(|s| s.to_string()).collect())
        .unwrap_or_default();
    Some(([major, minor, patch], pre_segments))
}

/// local version 段的开发构建形态:git describe(`238.g<sha>`)或常见开发通道标识
/// (canary / nightly / dev / dirty / snapshot)。普通 semver build metadata
/// (如 `linux.1`)不命中 —— 与前端 `src/lib/version.ts` 的 DEV_LOCAL_RE 语义对称。
static DEV_LOCAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\d+\.g[0-9a-f]{4,}|(^|\.)(canary|nightly|dev|dirty|snapshot)(\.|$)")
        .expect("Invalid dev-local regex")
});

fn is_dev_local(local: &str) -> bool {
    DEV_LOCAL_RE.is_match(local)
}

/// 比较两个版本号(遵循 semver:主版本三段优先;core 相等时有预发布 < 无预发布;
/// 预发布段逐段比 —— 数字段按数值、数字段 < 非数字段、非数字段按 ASCII、前缀相同
/// 则段更多者更大)。任一无法解析返回 None,调用方据此保守处理。
pub(super) fn compare_semver(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let (ac, ap) = parse_semver(a)?;
    let (bc, bp) = parse_semver(b)?;
    for i in 0..3 {
        match ac[i].cmp(&bc[i]) {
            Ordering::Equal => continue,
            other => return Some(other),
        }
    }
    match (ap.is_empty(), bp.is_empty()) {
        (true, true) => return Some(Ordering::Equal),
        (true, false) => return Some(Ordering::Greater),
        (false, true) => return Some(Ordering::Less),
        (false, false) => {}
    }
    for (x, y) in ap.iter().zip(bp.iter()) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(xv), Ok(yv)) => xv.cmp(&yv),
            (Ok(_), Err(_)) => Ordering::Less, // 数字段 < 非数字段
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.as_str().cmp(y.as_str()),
        };
        if ord != Ordering::Equal {
            return Some(ord);
        }
    }
    Some(ap.len().cmp(&bp.len()))
}

/// 从一次 registry 请求得到的完整 dist-tags 出发,挑选要展示的"最新版本"。
///
/// 规则:默认就是 `latest`;仅当本地版本已**严格领先** `latest`(说明用户主动在
/// 抢先通道)时,才把 `prerelease_tags` 指向的版本纳入比较,取其中能被解析、且
/// 高于 `latest` 的最高者。无法解析或不高于 latest 的脏 tag 一律落选。
fn pick_latest_version(
    dist_tags: &serde_json::Map<String, serde_json::Value>,
    prerelease_tags: &[&str],
    local_version: Option<&str>,
) -> Option<String> {
    use std::cmp::Ordering;
    let latest = dist_tags.get("latest").and_then(|v| v.as_str())?;

    // 本地是否严格领先 latest;任一无法解析则按"未领先"保守处理(只看 latest)。
    let local_ahead = local_version
        .and_then(|local| compare_semver(local, latest))
        .map(|ord| ord == Ordering::Greater)
        .unwrap_or(false);
    if prerelease_tags.is_empty() || !local_ahead {
        return Some(latest.to_string());
    }

    let mut best = latest.to_string();
    for tag in prerelease_tags {
        if let Some(candidate) = dist_tags.get(*tag).and_then(|v| v.as_str()) {
            if compare_semver(candidate, &best) == Some(Ordering::Greater) {
                best = candidate.to_string();
            }
        }
    }
    Some(best)
}

/// npm 包 dist-tags 专用端点的 URL。
///
/// 该端点的响应体就是 dist-tags 对象本身(几十到几千字节);而 `/{package}` 返回的是
/// 含每个历史版本元数据的完整 packument,codex / opencode / openclaw 这类高频发版的包
/// 已有十几到二十几 MB,一次刷新要下几十 MB(#7339)。scoped 包名的 `/` 按 registry
/// 约定转义成 `%2f`。
fn npm_dist_tags_url(package: &str) -> String {
    format!(
        "https://registry.npmjs.org/-/package/{}/dist-tags",
        package.replace('/', "%2f")
    )
}

/// 拉取 npm 包的完整 dist-tags(单次请求即含 latest/next/beta/...)。
///
/// 与 GitHub / PyPI 两条来源一样套 `LATEST_PROBE_TIMEOUT`:取不到就返回 None,由调用方
/// 显示「未知」,而不是沿用全局客户端的 600s 总超时让卡片一直「加载中」。包不存在时
/// 端点返回 404 与一个 JSON 字符串体,解析成 Map 失败,同样落到 None。
pub(super) async fn fetch_npm_dist_tags(
    client: &reqwest::Client,
    package: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let resp = client
        .get(npm_dist_tags_url(package))
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?;
    resp.json::<serde_json::Map<String, serde_json::Value>>()
        .await
        .ok()
}

/// 查询某 npm 工具要展示的"最新版本":取 `latest`,并在本地版本领先时按工具的
/// 预发布通道(见 `npm_prerelease_tags`)补查 —— 复用同一次 registry 响应,无额外请求。
async fn fetch_npm_latest_for_tool(
    client: &reqwest::Client,
    package: &str,
    tool: &str,
    local_version: Option<&str>,
) -> Option<String> {
    let dist_tags = fetch_npm_dist_tags(client, package).await?;
    pick_latest_version(
        &dist_tags,
        tool_spec(tool).npm_prerelease_tags,
        local_version,
    )
}

/// 版本探测请求的请求级超时。全局客户端是给代理转发用的（总超时 600s / 连接 30s），
/// 探测 latest 若沿用它，api.github.com / pypi.org 被阻断或握手后挂起时，Hermes 卡片
/// 与「刷新 / 全部升级」按钮会一直等；探测拿不到就退到下一来源或显示未知即可。
pub(super) const LATEST_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Helper function to fetch latest version from GitHub releases
/// Antigravity CLI 的 installer 和自升级共用的发布清单：`manifests/<平台>.json` 的 `version`。
async fn fetch_agy_latest_version(client: &reqwest::Client) -> Option<String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let url = format!(
        "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/{os}_{arch}.json"
    );
    let resp = client
        .get(&url)
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let json = resp.json::<serde_json::Value>().await.ok()?;
    json.get("version")
        .and_then(|v| v.as_str())
        .and_then(release_display_version)
}

async fn fetch_github_latest_version(client: &reqwest::Client, repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let resp = client
        .get(&url)
        .header("User-Agent", "cc-switch")
        .header("Accept", "application/vnd.github+json")
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?;
    let json = resp.json::<serde_json::Value>().await.ok()?;
    github_release_version_from_json(&json)
}

/// 从 GitHub latest release 的 JSON 中提取展示用版本号。
///
/// 优先取 release `name` 里能解析为语义版本的数字，其次才是 `tag_name`（去 `v` 前缀）：
/// Hermes 的 tag 是日历式（`v2026.8.31`），CLI 自报的语义版本 `0.21.0` 只出现在
/// name（"Hermes Agent v0.21.0 (v2026.8.31)"）里。两条路都经 `release_display_version`
/// 过滤——日历式数字若被当成版本号，前端三段解析会成功（2026 > 0）并永久判定
/// "有可用更新"，点升级后版本不变又触发"版本未变"误报；拿不到语义版本宁可返回
/// None，让调用方退到别的来源。限流（403 JSON 只有 message）同样落到 None。
fn github_release_version_from_json(json: &serde_json::Value) -> Option<String> {
    let from_name = json
        .get("name")
        .and_then(|v| v.as_str())
        .and_then(extract_version)
        .and_then(|version| release_display_version(&version));
    from_name.or_else(|| {
        json.get("tag_name")
            .and_then(|v| v.as_str())
            .and_then(|tag| release_display_version(tag.strip_prefix('v').unwrap_or(tag)))
    })
}

/// 只接受能按语义版本解析、且首段不像年份（< 1000）的字符串作展示版本。
fn release_display_version(candidate: &str) -> Option<String> {
    match parse_semver(candidate) {
        Some(([major, ..], _)) if major < 1000 => Some(candidate.to_string()),
        _ => None,
    }
}

/// 兜底来源给出的 latest 若已被本地版本严格超过，则不展示（返回 None）——
/// 展示一个比当前还旧的"最新版本"只会复现用户报告的"最新 < 当前"矛盾。
/// 任一侧无法解析时按"未领先"保守处理，照常展示。
fn drop_latest_behind_local(latest: Option<String>, local_version: Option<&str>) -> Option<String> {
    let latest = latest?;
    let local_leads = local_version
        .and_then(|local| compare_semver(local, &latest))
        .is_some_and(|ord| ord == std::cmp::Ordering::Greater);
    (!local_leads).then_some(latest)
}

/// Hermes 的「最新版本」：GitHub Releases 为主，PyPI 兜底。
///
/// cc-switch 安装/升级 Hermes 走的是官方 install.sh（`git clone` main 分支）与
/// `hermes update`（`git pull`），整条链路与 PyPI 无关；而 PyPI 的 `hermes-agent`
/// 自 0.19.0（2026-07-20）起停更，上游只在 GitHub Releases 发版
/// （#6475 / #6618 / #7033：「最新版本」长期停在 0.19.0、比当前还旧、升级按钮不出现）。
/// 仅当 GitHub 不可达或被限流时才退到 PyPI，且该值已被本地超过时不展示，宁可显示未知。
async fn fetch_hermes_latest_version(
    client: &reqwest::Client,
    local_version: Option<&str>,
) -> Option<String> {
    if let Some(version) = fetch_github_latest_version(client, "NousResearch/hermes-agent").await {
        return Some(version);
    }
    let pypi = fetch_pypi_latest_version(client, "hermes-agent").await;
    drop_latest_behind_local(pypi, local_version)
}

/// Helper function to fetch latest version from PyPI
async fn fetch_pypi_latest_version(client: &reqwest::Client, package: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{package}/json");
    match client.get(&url).timeout(LATEST_PROBE_TIMEOUT).send().await {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("info")
                    .and_then(|info| info.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// 预编译的版本号正则表达式。末尾的 `(\+[\w.-]+)?` 保留 PEP 440 local version
/// 段(git describe 风格,如 Hermes main/canary 开发构建 `0.22.1+238.g<sha>`),
/// 让前端能据此识别开发构建,避免把"版本号更低但内容更新"误报成"有新版本"。
pub(super) static VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d+\.\d+\.\d+(-[\w.]+)?(\+[\w.-]+)?").expect("Invalid version regex")
});

/// WSL 版本探测载荷先打印的哨兵。用户 shell 的启动文件（Ubuntu 的 update-motd、
/// `/etc/bash.bashrc` 的 sudo 提示、nvm 的 `Using Node vX.Y.Z` 等）输出都在它前面，
/// 解析时只看它之后的内容（#7347）。
#[cfg_attr(not(windows), allow(dead_code))]
const VERSION_PROBE_SENTINEL: &str = "__CCSWITCH_VERSION__";

/// 版本探测交给用户 shell 执行的命令：先打哨兵，再跑 `--version`。
/// 不含单引号，可以直接嵌进外层 `'...'`。
#[cfg_attr(not(windows), allow(dead_code))]
fn version_probe_payload(tool: &str) -> String {
    format!("echo {VERSION_PROBE_SENTINEL}; {tool} --version")
}
/// 取哨兵**最后一次**出现之后的输出。
///
/// 取最后一次：兜底链 `-lic || -lc || -c` 失败重试时会多次打印哨兵。
/// 按子串而非整行查找：OSC 序列可能没有换行、直接粘在哨兵前面。
/// 找不到哨兵（shell 在执行载荷之前就失败）时原样返回，保持原有行为。
#[cfg_attr(not(windows), allow(dead_code))]
fn after_version_sentinel(output: &str) -> &str {
    match output.rfind(VERSION_PROBE_SENTINEL) {
        Some(i) => output[i + VERSION_PROBE_SENTINEL.len()..].trim(),
        None => output,
    }
}

/// 判断 shell 输出是否表示"找不到该命令"。
/// 只认各 shell 的固定格式，避免把工具自身的 `module not found` 之类错误当成没装：
/// bash/zsh `command not found`，dash/sh `<tool>: not found`，fish `Unknown command`。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_shell_command_missing(err: &str, tool: &str) -> bool {
    err.contains("command not found")
        || err.contains(&format!("{tool}: not found"))
        || err.contains("Unknown command")
}

/// 从版本输出中提取纯版本号；解析不出就是 `None`。
/// 没有“把原文当版本号”的兜底：调用方拿到 `Found` 就不再改用别的安装。
/// 发行名仍先经这里取出语义版本，再由 `release_display_version` 过滤。
pub(super) fn extract_version(raw: &str) -> Option<String> {
    VERSION_RE.find(raw).map(|m| m.as_str().to_string())
}

const VERSION_UNKNOWN: &str = "installed but its version could not be determined";

/// 工具未安装时的统一错误文案；WSL 路径会再拼上 `[WSL:{distro}] ` 前缀。
pub(super) const NOT_INSTALLED: &str = "not installed or not executable";

/// 已定位到的可执行文件跑 `--version` 却什么都没输出时的诊断。文件在、拿不到版本，
/// 就是「装了但跑不起来」：安装位置列表照样会列出它，这里若报没装，同一行会既在
/// 「已安装」组里、又显示「未安装」和安装按钮。
pub(super) fn silent_version_probe_detail(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(0) => "`--version` printed nothing".to_string(),
        Some(code) => format!("`--version` exited with code {code} and printed nothing"),
        None => "`--version` was terminated and printed nothing".to_string(),
    }
}

/// 已定位到的可执行文件启动失败（文件损坏、被安全软件拦截、不是有效程序等）时的诊断。
pub(super) fn launch_failure_detail(tool_path: &Path, err: &std::io::Error) -> String {
    format!("failed to launch {}: {err}", tool_path.display())
}

/// CLI 版本探测结果。捕获、启动、读取、等待和超时走 `Result::Err`，
/// 不占用安装状态。
///
/// 关键区分"没装"与"装了但 `--version` 自身报错退出"（如工具要求更高的 Node 版本）：
/// 后者必须如实上报、不去别处捞旧版掩盖，否则"升级到新版却跑不起来"会被旧版盖住，
/// 表现为"升级成功但版本号不变"。
#[derive(Debug)]
pub(super) enum ShellProbe {
    /// 成功拿到版本号
    Found(String),
    /// 可执行存在、但 `--version` 非零退出（携带诊断信息，如 stderr 末尾若干行）
    FoundButFailed(String),
    /// 命令成功退出，但输出里没有可解析的版本号。不能用另一处安装的版本顶替。
    VersionUnknown(String),
    /// 没找到该命令（携带描述性消息，供 UI 展示）
    NotFound(String),
}

/// stdout 非空时不回退 stderr：运行时警告也可能带 Node 等无关版本号。
pub(super) fn version_output<'a>(stdout: &'a str, stderr: &'a str) -> &'a str {
    if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    }
}

pub(super) fn successful_version_probe(stdout: &str, stderr: &str) -> ShellProbe {
    let raw = version_output(stdout, stderr);
    match extract_version(raw) {
        Some(version) => ShellProbe::Found(version),
        None => ShellProbe::VersionUnknown(format!("{VERSION_UNKNOWN}: {}", last_lines(raw, 4))),
    }
}

/// 启动文件只继承 /dev/null；执行载荷才打开捕获文件，保留登录 shell 的 PATH 和函数。
/// 不留输出管道，避免 rc 后台进程按住写端。这不是针对敌意 rc 的安全边界。
#[cfg(not(target_os = "windows"))]
pub(super) fn capture_login_shell(
    payload: &str,
    deadline: Option<CommandDeadline>,
) -> Result<std::process::Output, String> {
    use std::io::{Read, Seek};
    use std::process::{Command, Stdio};

    let new_temp = || {
        tempfile::Builder::new()
            .prefix("ccswitch-probe-")
            .tempfile()
            .map_err(|e| format!("Cannot create capture file: {e}"))
    };
    let mut stdout = new_temp()?;
    let mut stderr = new_temp()?;
    let quote = |file: &tempfile::NamedTempFile| {
        file.path()
            .to_str()
            .map(shell_single_quote)
            .ok_or_else(|| "Capture path is not valid UTF-8".to_string())
    };
    let q_out = quote(&stdout)?;
    let q_err = quote(&stderr)?;
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| is_valid_shell(s))
        .unwrap_or_else(|| "sh".to_string());
    let mut cmd = Command::new(&shell);
    // tempfile 已经建好文件，追加重定向才能兼容 rc 里的 noclobber。
    cmd.arg(default_flag_for_shell(&shell))
        .arg(format!("{payload} >>{q_out} 2>>{q_err}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    isolate_child_process_group(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Cannot run login shell: {e}"))?;
    let mut output = wait_child_output(child, deadline)?;
    let read = |file: &mut tempfile::NamedTempFile| -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        file.rewind()
            .and_then(|_| file.read_to_end(&mut bytes))
            .map_err(|e| format!("Cannot read capture file: {e}"))?;
        Ok(bytes)
    };
    output.stdout = read(&mut stdout)?;
    output.stderr = read(&mut stderr)?;
    Ok(output)
}

/// 在非 Windows 平台用用户 shell 执行 `{tool} --version` 探测版本。
///
/// Windows 不走此路径：`cmd /C {tool}` 可能误触发 App Execution Alias /
/// 协议处理器（曾导致 Windows 版整体被禁用），那里改由 `scan_cli_version`
/// 只执行已定位到的真实可执行文件。
#[cfg(not(target_os = "windows"))]
fn try_get_version(tool: &str) -> Result<ShellProbe, String> {
    let out = capture_login_shell(
        &format!("{tool} --version"),
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )?;
    let stdout = decode_command_output(&out.stdout);
    let stderr = decode_command_output(&out.stderr);
    Ok(if out.status.success() {
        successful_version_probe(&stdout, &stderr)
    } else if out.status.code() == Some(127) {
        ShellProbe::NotFound(NOT_INSTALLED.to_string())
    } else {
        let detail = if stderr.trim().is_empty() {
            &stdout
        } else {
            &stderr
        };
        ShellProbe::FoundButFailed(last_lines(detail.trim(), 4))
    })
}
#[cfg(target_os = "windows")]
fn try_get_version_wsl(
    tool: &str,
    distro: &str,
    force_shell: Option<&str>,
    force_shell_flag: Option<&str>,
) -> Result<ShellProbe, String> {
    use std::process::Command;

    // 防御性断言：tool 只能是预定义的值
    debug_assert!(known_tool(tool).is_some(), "unexpected tool name: {tool}");

    // 校验 distro 名称，防止命令注入
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("[WSL:{distro}] invalid distro name"));
    }

    // 构建 Shell 脚本检测逻辑
    let (shell, flag, cmd) = if let Some(shell) = force_shell {
        // Defensive validation: never allow an arbitrary executable name here.
        if !is_valid_shell(shell) {
            return Err(format!("[WSL:{distro}] invalid shell: {shell}"));
        }
        let shell = shell.rsplit('/').next().unwrap_or(shell);
        let flag = if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return Err(format!("[WSL:{distro}] invalid shell flag: {flag}"));
            }
            flag
        } else {
            default_flag_for_shell(shell)
        };

        (shell.to_string(), flag, version_probe_payload(tool))
    } else {
        if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return Err(format!("[WSL:{distro}] invalid shell flag: {flag}"));
            }
        }
        let cmd = wsl_auto_shell_command(&version_probe_payload(tool), force_shell_flag);

        ("sh".to_string(), "-c", cmd)
    };

    let output = Command::new("wsl.exe")
        .args(["-d", distro, "--", &shell, flag, &cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(out) => {
            let stdout = decode_command_output(&out.stdout).trim().to_string();
            let stderr = decode_command_output(&out.stderr).trim().to_string();
            // 启动文件的输出都在哨兵之前，只看它之后（#7347）
            let payload_out = after_version_sentinel(&stdout).to_string();
            if out.status.success() {
                Ok(successful_version_probe(&payload_out, &stderr))
            } else {
                let err = if stderr.is_empty() {
                    payload_out
                } else {
                    stderr
                };
                // wsl.exe 透传的退出码不总可靠，故同时用 exit 127 与 shell 的"找不到命令"
                // 文本兜底判别"没装"；其余非零退出视作"装了但 --version 报错"。
                let not_found =
                    out.status.code() == Some(127) || is_shell_command_missing(&err, tool);
                Ok(if not_found {
                    ShellProbe::NotFound(format!("[WSL:{distro}] {NOT_INSTALLED}"))
                } else {
                    ShellProbe::FoundButFailed(format!(
                        "[WSL:{distro}] {}",
                        last_lines(err.trim(), 4)
                    ))
                })
            }
        }
        Err(e) => Err(format!("[WSL:{distro}] exec failed: {e}")),
    }
}

/// 非 Windows 平台的 WSL 版本检测存根
/// 注意：此函数实际上不会被调用，因为 `wsl_distro_from_path` 在非 Windows 平台总是返回 None。
/// 保留此函数是为了保持 API 一致性，防止未来重构时遗漏。
#[cfg(not(target_os = "windows"))]
fn try_get_version_wsl(
    _tool: &str,
    _distro: &str,
    _force_shell: Option<&str>,
    _force_shell_flag: Option<&str>,
) -> Result<ShellProbe, String> {
    Err("WSL check not supported on this platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_command_missing_matches_shell_formats_only() {
        assert!(is_shell_command_missing(
            "bash: line 1: claude: command not found",
            "claude"
        ));
        assert!(is_shell_command_missing(
            "zsh:1: command not found: claude",
            "claude"
        ));
        assert!(is_shell_command_missing(
            "sh: 1: claude: not found",
            "claude"
        ));
        assert!(is_shell_command_missing(
            "fish: Unknown command: claude",
            "claude"
        ));
        assert!(!is_shell_command_missing(
            "Error: Cannot find module 'x'\nmodule not found",
            "claude"
        ));
        assert!(!is_shell_command_missing("config file not found", "claude"));
    }

    #[test]
    fn version_probe_sentinel_drops_shell_startup_output() {
        assert_eq!(
            version_probe_payload("claude"),
            "echo __CCSWITCH_VERSION__; claude --version"
        );

        // Ubuntu 的 update-motd 在当天第一个交互式 login shell 里打印 MOTD，
        // 整段取第一个版本号会拿到 24.04.4（#7347）
        let motd = "Welcome to Ubuntu 24.04.4 LTS (GNU/Linux 6.6.87.2-microsoft-standard-WSL2 x86_64)\n\n * Documentation:  https://help.ubuntu.com\n__CCSWITCH_VERSION__\n2.1.270 (Claude Code)";
        assert_eq!(after_version_sentinel(motd), "2.1.270 (Claude Code)");

        // 兜底链 `-lic || -lc || -c` 会多次打印哨兵：取最后一次之后
        let chained = "__CCSWITCH_VERSION__\nbash: warning\n__CCSWITCH_VERSION__\n1.2.3";
        assert_eq!(after_version_sentinel(chained), "1.2.3");

        // OSC 序列没有换行、直接粘在哨兵前面（地址取自 RFC 5737 文档保留段）
        let glued = "\x1b]1337;RemoteHost=user@198.51.100.23\x07__CCSWITCH_VERSION__\n0.154.0";
        assert_eq!(after_version_sentinel(glued), "0.154.0");

        // 版本打在 stderr 的工具：哨兵之后为空，调用方回退到 stderr
        assert_eq!(after_version_sentinel("__CCSWITCH_VERSION__\n"), "");

        // 没有哨兵（shell 在执行载荷前就失败）：原样返回，行为不变
        assert_eq!(
            after_version_sentinel("sh: 1: bad: not found"),
            "sh: 1: bad: not found"
        );
    }

    #[test]
    fn test_extract_version() {
        for (raw, expected) in [
            ("claude 1.0.20", "1.0.20"),
            ("2.1.220 (Claude Code)", "2.1.220"),
            ("codex-cli 0.145.0", "0.145.0"),
            ("0.46.0", "0.46.0"),
            ("v22.12.0", "22.12.0"),
            ("v2.3.4-beta.1", "2.3.4-beta.1"),
            ("hermes, version 1.4.0", "1.4.0"),
            ("@xai-official/grok 1.2.3", "1.2.3"),
            (
                "Hermes Agent v0.22.1+238.g8211bb9 (2026.9.24) · upstream 8211bb96",
                "0.22.1+238.g8211bb9",
            ),
        ] {
            assert_eq!(
                extract_version(raw).as_deref(),
                Some(expected),
                "应解析出版本号：{raw:?}"
            );
        }
        assert_eq!(extract_version("no version here"), None);
        assert_eq!(
            extract_version("tool 1.2.3\nUpdate available: 1.2.4").as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn successful_probe_results_keep_installation_facts() {
        for (stdout, stderr, expected) in [
            ("9.9.9", "", Some("9.9.9")),
            ("", "9.9.9", Some("9.9.9")),
            ("development-build", "", None),
            ("checking installation", "Warning: Node 20.11.0", None),
            ("", "", None),
        ] {
            for output in [
                stdout.to_string(),
                format!("MOTD 24.04.4\n{VERSION_PROBE_SENTINEL}\n{stdout}"),
            ] {
                let payload = after_version_sentinel(&output);
                let info = tool_version_from_probe(
                    "claude",
                    Ok(successful_version_probe(payload, stderr)),
                );
                let json = serde_json::to_value(&info).unwrap();
                assert_eq!(json["installed"], true);
                assert_eq!(json["installed_but_broken"], false);
                assert_eq!(info.version.as_deref(), expected);
            }
        }
    }

    #[test]
    fn probe_failures_do_not_claim_an_installation() {
        for (probe, installed, broken) in [
            (Err("cannot create capture file".to_string()), None, false),
            (
                Ok(ShellProbe::NotFound(NOT_INSTALLED.to_string())),
                Some(false),
                false,
            ),
            (
                Ok(ShellProbe::FoundButFailed("exit 2".to_string())),
                Some(true),
                true,
            ),
        ] {
            let info = tool_version_from_probe("claude", probe);
            assert_eq!(info.installed, installed);
            assert_eq!(info.installed_but_broken, broken);
            assert!(info.version.is_none());
            assert_eq!(info.error.is_some(), installed != Some(false));
        }
    }

    #[test]
    fn local_versions_are_dev_builds_and_incomparable() {
        // 开发构建(git describe / 开发通道标识)与任何版本不可比
        assert!(parse_semver("0.22.1+238.g8211bb9").is_none());
        assert!(parse_semver("0.22.1+canary.20261015T070051Z").is_none());
        assert!(compare_semver("0.22.2", "0.22.1+238.g8211bb9").is_none());
        assert!(compare_semver("0.22.1+238.g8211bb9", "0.22.1").is_none());
        // 普通 semver build metadata 按规范忽略后照常比较
        assert_eq!(parse_semver("1.0.0+linux.1"), Some(([1, 0, 0], vec![])));
        assert_eq!(
            compare_semver("1.0.1", "1.0.0+linux.1"),
            Some(std::cmp::Ordering::Greater)
        );
        // 正式版与预发布版不受影响
        assert_eq!(
            compare_semver("0.22.2", "0.22.1"),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(
            compare_semver("2.1.156-beta.1", "2.1.156"),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn github_release_version_prefers_semver_in_name_over_calendar_tag() {
        // Hermes 官方 release：tag 日历式，语义版本只在 name 里；2026-08-19 起括号内还多了个 v
        for (name, tag, want) in [
            ("Hermes Agent v0.20.4 (2026.8.18)", "v2026.8.18", "0.20.4"),
            ("Hermes Agent v0.21.0 (v2026.8.31)", "v2026.8.31", "0.21.0"),
            (
                "Hermes Agent v0.20.3 (2026.8.16.2)",
                "v2026.8.16.2",
                "0.20.3",
            ),
        ] {
            let json = serde_json::json!({ "name": name, "tag_name": tag });
            assert_eq!(
                github_release_version_from_json(&json).as_deref(),
                Some(want),
                "{name}"
            );
        }
    }

    #[test]
    fn github_release_version_falls_back_to_semver_tag() {
        // opencode：name == tag，走 name 或 tag 结果一致
        let named = serde_json::json!({ "name": "v1.18.18", "tag_name": "v1.18.18" });
        assert_eq!(
            github_release_version_from_json(&named).as_deref(),
            Some("1.18.18")
        );
        let prose = serde_json::json!({ "name": "August refresh", "tag_name": "v1.18.18" });
        assert_eq!(
            github_release_version_from_json(&prose).as_deref(),
            Some("1.18.18")
        );
        let unnamed = serde_json::json!({ "tag_name": "v1.2.3" });
        assert_eq!(
            github_release_version_from_json(&unnamed).as_deref(),
            Some("1.2.3")
        );
    }

    #[test]
    fn github_release_version_rejects_calendar_versions_and_rate_limit_body() {
        // name 与 tag 都只有日历式数字：不得把 2026.8.31 当版本号（前端会永久判定"可更新"）
        let calendar = serde_json::json!({
            "name": "Hermes Agent (2026.8.31)",
            "tag_name": "v2026.8.31"
        });
        assert_eq!(github_release_version_from_json(&calendar), None);
        let four_seg = serde_json::json!({ "tag_name": "v2026.8.16.2" });
        assert_eq!(github_release_version_from_json(&four_seg), None);
        // GitHub 未认证限流响应只有 message / documentation_url
        let limited = serde_json::json!({
            "message": "API rate limit exceeded for 1.2.3.4.",
            "documentation_url": "https://docs.github.com/rest"
        });
        assert_eq!(github_release_version_from_json(&limited), None);
        assert_eq!(
            github_release_version_from_json(&serde_json::json!({})),
            None
        );
    }

    #[test]
    fn hermes_pypi_fallback_is_hidden_when_local_leads() {
        let pypi = || Some("0.19.0".to_string());
        // GitHub 不可达、PyPI 仍停在 0.19.0：本地 0.21.0 时不展示旧值（否则"最新 < 当前"）
        assert_eq!(drop_latest_behind_local(pypi(), Some("0.21.0")), None);
        // 本地等于 / 落后 PyPI，或本地未知：照常展示
        assert_eq!(
            drop_latest_behind_local(pypi(), Some("0.19.0")).as_deref(),
            Some("0.19.0")
        );
        assert_eq!(
            drop_latest_behind_local(pypi(), Some("0.18.2")).as_deref(),
            Some("0.19.0")
        );
        assert_eq!(
            drop_latest_behind_local(pypi(), None).as_deref(),
            Some("0.19.0")
        );
        // 本地无法解析时保守视为未领先
        assert_eq!(
            drop_latest_behind_local(pypi(), Some("unknown")).as_deref(),
            Some("0.19.0")
        );
        assert_eq!(drop_latest_behind_local(None, Some("0.21.0")), None);
    }

    #[test]
    fn test_compare_semver() {
        use std::cmp::Ordering;
        assert_eq!(
            compare_semver("2.1.156", "2.1.154"),
            Some(Ordering::Greater)
        );
        assert_eq!(compare_semver("2.1.154", "2.1.156"), Some(Ordering::Less));
        assert_eq!(compare_semver("2.1.156", "2.1.156"), Some(Ordering::Equal));
        // 预发布 < 同核心正式版
        assert_eq!(
            compare_semver("2.1.156-beta.1", "2.1.156"),
            Some(Ordering::Less)
        );
        // core 更高的预发布仍高于较低的正式版（gemini nightly 场景）
        assert_eq!(
            compare_semver("0.45.0-nightly.1", "0.44.1"),
            Some(Ordering::Greater)
        );
        // 大 patch（codex 时间戳式）不溢出
        assert_eq!(
            compare_semver("0.1.2505172116", "0.135.0"),
            Some(Ordering::Less)
        );
        // 无法解析返回 None（gemini 的 `false` 脏 tag）
        assert_eq!(compare_semver("false", "1.0.0"), None);
    }

    #[test]
    fn test_pick_latest_version() {
        use serde_json::json;
        let tags = json!({
            "latest": "2.1.154",
            "next": "2.1.156",
            "stable": "2.1.145"
        });
        let map = tags.as_object().unwrap();

        // 本地领先 latest（在 next 通道）→ 补查到 next，数字对齐
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.156")),
            Some("2.1.156".to_string())
        );
        // 本地等于 latest → 不补查，仍显示 latest
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.154")),
            Some("2.1.154".to_string())
        );
        // 本地落后 latest（稳定通道用户）→ 不补查，不被推向预发布版
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.145")),
            Some("2.1.154".to_string())
        );
        // 无预发布白名单 → 永远只看 latest（不解析 local，避免脏 local 触发）
        assert_eq!(
            pick_latest_version(map, &[], Some("2.1.156")),
            Some("2.1.154".to_string())
        );
        // 本地版本未知 → 保守只看 latest
        assert_eq!(
            pick_latest_version(map, &["next"], None),
            Some("2.1.154".to_string())
        );
    }

    #[test]
    fn test_pick_latest_version_filters_dirty_prerelease() {
        use serde_json::json;
        // 模拟 codex：beta 是低于 latest 的时间戳式脏版本
        let tags = json!({
            "latest": "0.135.0",
            "beta": "0.1.2505172116"
        });
        let map = tags.as_object().unwrap();
        // 即便本地领先 latest，低于 latest 的脏 beta 也不会被选
        assert_eq!(
            pick_latest_version(map, &["beta"], Some("0.200.0")),
            Some("0.135.0".to_string())
        );
    }

    #[test]
    fn test_npm_dist_tags_url() {
        // 普通包名直接拼进路径
        assert_eq!(
            npm_dist_tags_url("openclaw"),
            "https://registry.npmjs.org/-/package/openclaw/dist-tags"
        );
        // scoped 包名的 `/` 按 registry 约定转义成 %2f
        assert_eq!(
            npm_dist_tags_url("@openai/codex"),
            "https://registry.npmjs.org/-/package/@openai%2fcodex/dist-tags"
        );
    }

    /// 文件在、却拿不到版本（起不来 / 没输出）算「装了但跑不起来」，不算没装：
    /// 安装位置列表照样列出这类文件，报没装会让应用页同一行既在「已安装」组又显示安装按钮。
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn scan_cli_version_reports_present_but_silent_executable_as_broken() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir should be created");
        let make_dir = |name: &str, script: Option<&str>| {
            let dir = root.path().join(name);
            std::fs::create_dir_all(&dir).expect("dir should be created");
            let tool = dir.join("kimi");
            match script {
                Some(body) => {
                    std::fs::write(&tool, format!("#!/bin/sh\n{body}\n"))
                        .expect("script should be written");
                    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755))
                        .expect("script should be executable");
                }
                // 不可执行的文件：启动失败
                None => std::fs::write(&tool, "").expect("file should be written"),
            }
            dir
        };
        let silent = make_dir("silent", Some("exit 0"));
        let silent_failure = make_dir("silent-failure", Some("exit 3"));
        let not_executable = make_dir("not-executable", None);
        let loud_failure = make_dir("loud-failure", Some("echo boom >&2\nexit 1"));
        let working = make_dir("working", Some("echo 'kimi 1.2.3'"));
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).expect("dir should be created");
        // 与工具同名的目录不是安装
        let same_name_dir = root.path().join("same-name-dir");
        std::fs::create_dir_all(same_name_dir.join("kimi")).expect("dir should be created");

        let broken_detail = |paths: &[PathBuf]| match scan_cli_version_in("kimi", paths) {
            Ok(ShellProbe::FoundButFailed(detail)) => detail,
            Ok(ShellProbe::Found(v)) => panic!("expected broken, got version {v}"),
            Ok(ShellProbe::VersionUnknown(e)) => {
                panic!("expected broken, got version unknown: {e}")
            }
            Ok(ShellProbe::NotFound(e)) => panic!("expected broken, got not found: {e}"),
            Err(e) => panic!("expected broken, got error: {e}"),
        };

        assert!(broken_detail(std::slice::from_ref(&silent)).contains("printed nothing"));
        assert!(broken_detail(std::slice::from_ref(&silent_failure))
            .contains("exited with code 3 and printed nothing"));
        assert!(
            broken_detail(std::slice::from_ref(&not_executable)).starts_with("failed to launch")
        );
        // 具体报错优先于笼统诊断
        assert_eq!(broken_detail(&[silent.clone(), loud_failure]), "boom");
        // 别处有能跑的安装仍以它为准
        assert!(matches!(
            scan_cli_version_in("kimi", &[silent, working]),
            Ok(ShellProbe::Found(v)) if v == "1.2.3"
        ));
        assert!(matches!(
            scan_cli_version_in("kimi", &[empty, same_name_dir]),
            Ok(ShellProbe::NotFound(_))
        ));
    }

    #[cfg(unix)]
    mod version_probe_seam {
        use super::super::{try_get_version, ShellProbe};
        use std::os::unix::fs::PermissionsExt;

        const FIXTURE_VERSION: &str = "9.9.9";

        /// 真实世界那组噪音：不换行的 OSC + 版本形状横幅 + `/` 开头的诱饵行。
        /// `survives_shell_integration_osc_prefix` 与 `/bin/sh` 回退那条共用同一份。
        const RC_NOISE_SHELL_INTEGRATION: &str = concat!(
            r#"printf '\033]1337;RemoteHost=user@198.51.100.23\007\033]1337;CurrentDir=/home/user\007'"#,
            "\nprint 'Using Node v18.20.4'\nprint '/opt/decoy/bin/claude'",
        );

        /// 造探测环境；返回的 `TempDir` 一 drop 就清理，所以必须由调用方持有。
        fn fixture(rc_noise: &str) -> tempfile::TempDir {
            fixture_with_tool(rc_noise, &format!("echo '{FIXTURE_VERSION} (Fixture CLI)'"))
        }

        /// 同上，但由调用方给出假工具的脚本体——`keeps_stdout_and_stderr_in_separate_files`
        /// 需要一个同时往两条流写的工具。
        fn fixture_with_tool(rc_noise: &str, tool_body: &str) -> tempfile::TempDir {
            let dir = tempfile::tempdir().expect("tempdir");
            let bin = dir.path().join("bin");
            std::fs::create_dir_all(&bin).expect("mkdir bin");

            let tool = bin.join("claude");
            std::fs::write(&tool, format!("#!/bin/sh\n{tool_body}\n")).expect("write fake tool");
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake tool");

            // rc 先把假工具放进 PATH，再吐噪音——顺序与真实 rc 一致。
            std::fs::write(
                dir.path().join(".zshrc"),
                format!("export PATH={}:$PATH\n{rc_noise}\n", bin.display()),
            )
            .expect("write .zshrc");
            dir
        }

        /// zsh 缺席：CI 上直接失败，本地才允许跳过。
        ///
        /// 静默跳过 = libtest 记一个 pass，`eprintln!` 还被捕获——三条 CI 腿里有两条
        /// （ubuntu 无 /bin/zsh、windows 整模块 cfg 掉）会"什么都没测"地报绿，
        /// 而假绿正是这组测试要防的东西。
        fn zsh_available() -> bool {
            if std::path::Path::new("/bin/zsh").exists() {
                return true;
            }
            assert!(
                std::env::var("CI").is_err(),
                "version_probe_seam 需要 /bin/zsh：CI 上缺席即失败，不得静默跳过"
            );
            eprintln!("SKIPPED version_probe_seam: /bin/zsh 不存在，无法构造 rc 噪音环境");
            false
        }

        /// `try_get_version` 从进程环境读 SHELL，子 shell 从进程环境读 ZDOTDIR，二者都是
        /// 全局的——故整组用 #[serial] 串行，并由本守卫在 drop 时（含 panic 展开）还原，
        /// 免得一个失败的测试把 SHELL/ZDOTDIR 泄漏给同进程的其它测试。
        /// PATH 也存档：`enumerate_tool_installations` 那条用例要往进程 PATH 前置 fixture
        /// 目录（`build_tool_search_paths` 只从那里看得见它），改动同样得随守卫还原。
        struct EnvGuard {
            shell: Option<std::ffi::OsString>,
            zdotdir: Option<std::ffi::OsString>,
            path: Option<std::ffi::OsString>,
        }

        impl EnvGuard {
            fn set(shell: &str, zdotdir: &std::path::Path) -> Self {
                let guard = Self {
                    shell: std::env::var_os("SHELL"),
                    zdotdir: std::env::var_os("ZDOTDIR"),
                    path: std::env::var_os("PATH"),
                };
                std::env::set_var("SHELL", shell);
                std::env::set_var("ZDOTDIR", zdotdir);
                guard
            }
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                for (key, saved) in [
                    ("SHELL", &self.shell),
                    ("ZDOTDIR", &self.zdotdir),
                    ("PATH", &self.path),
                ] {
                    match saved {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }

        /// 假工具**只**经由 fixture rc 里那句 `export PATH=` 可达——进程 PATH 一个字节不改。
        /// 若把 fixture 的 bin 也前置到进程 PATH，工具不经 rc 就能找到，这组用例便无法区分
        /// 「隔离起作用了」和「根本没有噪音需要隔离」：rc 完全不执行时它们照样通过。
        fn probe_with(rc_noise: &str) -> Result<ShellProbe, String> {
            let dir = fixture(rc_noise);
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            try_get_version("claude")
        }

        #[track_caller]
        fn assert_found_fixture_version(probe: Result<ShellProbe, String>, case: &str) {
            match probe {
                Ok(ShellProbe::Found(v)) => assert_eq!(v, FIXTURE_VERSION, "{case}"),
                other => panic!("{case}: 期望 Found({FIXTURE_VERSION})，实得 {other:?}"),
            }
        }

        /// 启动文件隔离不能排除函数/shim 自身输出；待按工具格式解析时启用。
        #[test]
        #[ignore = "shim banners require tool-specific version parsing"]
        #[serial_test::serial]
        fn shim_banner_must_not_replace_the_tool_version() {
            if !zsh_available() {
                return;
            }
            let probe =
                probe_with(r#"claude() { echo 'Now using node v18.20.4'; command claude "$@"; }"#);
            assert_found_fixture_version(probe, "shim 横幅不能替代工具版本");
        }

        /// rc 留下一个继承描述符的后台进程——`sleep 20 &` 是版本管理器预热、
        /// 更新检查的常规写法：它不重定向任何东西，三条标准描述符照单全收。
        ///
        /// 工具输出若走管道，读端要等**所有**写端关闭才 EOF，这个后台进程会把它
        /// 按住 20 秒。本用例钉的是 `try_get_version`：它这条链（`get_tool_versions`
        /// 内联调用、无 `spawn_blocking`、无 deadline）没有任何超时兼底，一旦回到管道
        /// 就会把关于面板卡死。（`resolve_path_default` 不同：它走 `wait_child_output`，
        /// 超时会击杀整棵进程树，不依赖本用例护着。）
        ///
        /// 直接在测试线程上跑，卡墙钟预算：管道回归会阻塞约 20s，超预算即失败。**不另起线程**
        /// ——起了就没人 join，超时返回时 fixture 与 `EnvGuard` 随本函数一起 drop，`#[serial]`
        /// 的锁也在函数返回时放掉，那个还在跑的探测便与下一条 serial 测试抢同一份全局环境。
        #[test]
        #[serial_test::serial]
        fn returns_promptly_when_the_rc_leaves_a_background_process() {
            if !zsh_available() {
                return;
            }
            let start = std::time::Instant::now();
            let probe = probe_with("sleep 20 &");
            let elapsed = start.elapsed();
            assert!(
                elapsed < std::time::Duration::from_secs(15),
                "探测耗时 {elapsed:?}：rc 的后台进程按住了承接工具输出的描述符——\
                 载荷回退成管道（`.output()`）就是这个形状"
            );
            assert_found_fixture_version(probe, "rc 留下后台进程");
        }

        /// rc 里一句 `setopt noclobber`（常见的防手滑设置）会让 `>` 拒绝写已存在的文件，
        /// 而承接工具输出的临时文件必然已存在——重定向失败即空载荷，探测退化成 NotFound。
        /// 钉死重定向必须用 `>>`。
        #[test]
        #[serial_test::serial]
        fn survives_rc_setting_noclobber() {
            if !zsh_available() {
                return;
            }
            let dir = fixture("setopt noclobber");
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            assert_found_fixture_version(try_get_version("claude"), "rc 设置 noclobber");
            // 路径解析用同一套传输，也就同样吃这一刀：`>` 在 noclobber 下写不进已存在的
            // 临时文件 → 载荷为空 → None → `is_path_default` 永久 false。
            assert_eq!(
                super::super::super::resolve_path_default("claude", None)
                    .expect("resolve_path_default 不应报错"),
                std::fs::canonicalize(dir.path().join("bin/claude")).ok(),
                "noclobber 下路径解析仍应拿到 fixture 入口（用 `>` 就拿不到）"
            );
        }

        /// rc 在初始化中途把 fd 1 重定向走——powerlevel10k instant-prompt 捕获
        /// 控制台输出的真实机制。此时噪音和工具的版本输出会一起消失，
        /// 除非重定向绑在命令本身而不是继承来的 fd 1。
        #[test]
        #[serial_test::serial]
        fn survives_rc_reassigning_stdout_mid_init() {
            if !zsh_available() {
                return;
            }
            let probe = probe_with("print NOISE\nexec 1>/dev/null 2>/dev/null");
            assert_found_fixture_version(probe, "rc 中途重定向 fd 1");
        }

        /// 报告里那个故障本身，钉在版本通道上。
        ///
        /// 终端 shell integration 的 OSC 序列不以换行结尾，会粘在工具 `--version` 输出的
        /// 行首；rc 再打印一行形似版本号的东西（`Using Node v18.20.4` 是版本管理器的常见
        /// 横幅），首个匹配就落到它头上。三段噪音全部经 rc 继承来的 fd 1 写出，而工具的
        /// stdout 已被重定向到独立临时文件——rc 够不着它，故解析只看得到 `9.9.9`。
        ///
        /// OSC 载荷里用 `198.51.100.23`（RFC 5737 文档保留段）：一旦它泄进版本通道，
        /// 断言会得到 `198.51.100`；`/`-开头的诱饵行同理会得到 `18.20.4`。
        #[test]
        #[serial_test::serial]
        fn survives_shell_integration_osc_prefix() {
            if !zsh_available() {
                return;
            }
            let dir = fixture(RC_NOISE_SHELL_INTEGRATION);
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            assert_found_fixture_version(
                try_get_version("claude"),
                "OSC + 版本形状诱饵挤进版本通道",
            );
            assert_eq!(
                super::super::super::resolve_path_default("claude", None)
                    .expect("resolve_path_default 不应报错"),
                std::fs::canonicalize(dir.path().join("bin/claude")).ok(),
                "OSC 粘在 command -v 路径前时仍应拿到 fixture 入口"
            );
        }

        /// 解释器**不在**入口同级目录：`npm config set prefix ~/.npm-global`（官方推荐的免
        /// sudo 装法）配 nvm / brew 的 node 就是这个形状——入口在 `~/.npm-global/bin`，
        /// `node` 在别处，只有用户登录 shell 的那份 `$PATH` 才解析得到。GUI 进程那份
        /// launchd PATH（`/usr/bin:/bin:…`）里没有用户的 node；由 shell 执行工具正是为了
        /// 不必自己复现这份环境。解释器取一个真实机器上不可能存在的名字，本机装没装 node
        /// 都不影响结论。
        #[test]
        #[serial_test::serial]
        fn resolves_an_interpreter_outside_the_entry_directory() {
            if !zsh_available() {
                return;
            }
            const INTERPRETER: &str = "ccswitch-fixture-interp-elsewhere";

            let dir = tempfile::tempdir().expect("tempdir");
            let bin = dir.path().join("npm-global/bin");
            let node_bin = dir.path().join("node/bin");
            std::fs::create_dir_all(&bin).expect("mkdir bin");
            std::fs::create_dir_all(&node_bin).expect("mkdir node bin");

            let interp = node_bin.join(INTERPRETER);
            std::fs::write(&interp, "#!/bin/sh\nexec /bin/sh \"$@\"\n").expect("write interpreter");
            std::fs::set_permissions(&interp, std::fs::Permissions::from_mode(0o755))
                .expect("chmod interpreter");

            let tool = bin.join("claude");
            std::fs::write(
                &tool,
                format!("#!/usr/bin/env {INTERPRETER}\necho '{FIXTURE_VERSION} (Fixture CLI)'\n"),
            )
            .expect("write fake tool");
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake tool");

            std::fs::write(
                dir.path().join(".zshrc"),
                format!(
                    "export PATH={}:{}:$PATH\n",
                    bin.display(),
                    node_bin.display()
                ),
            )
            .expect("write .zshrc");

            let _env = EnvGuard::set("/bin/zsh", dir.path());
            assert_found_fixture_version(try_get_version("claude"), "解释器不在入口同级目录");
        }

        /// 两条流各自落到**独立**临时文件，且成功分支只看 stdout（stdout 为空才看 stderr）。
        ///
        /// 用一个同时往两条流写的 fixture，两个方向各钉一次：
        /// - 真版本在 stdout，stderr 是版本形状的警告。**stderr 先写**是刻意的：若把两条流
        ///   合并成一个文件，诱饵就排在前面、首个匹配落到 `20.11.0`；把两个文件对调、或改成
        ///   优先取 stderr，同样取到诱饵。三种改法都会让这条断言变红。
        /// - 反向：stdout 为空、真版本在 stderr。这条钉的是 stderr 确实被单独捕获了；
        ///   若 stderr 没落进自己的文件，这里只会拿到空串。
        #[test]
        #[serial_test::serial]
        fn keeps_stdout_and_stderr_in_separate_files() {
            if !zsh_available() {
                return;
            }
            for (tool_body, case) in [
                (
                    format!(
                        "echo 'Warning: requires Node v20.11.0' >&2\necho '{FIXTURE_VERSION} (Fixture CLI)'"
                    ),
                    "真版本在 stdout，stderr 是版本形状诱饵",
                ),
                (
                    format!("echo '{FIXTURE_VERSION} (Fixture CLI)' >&2"),
                    "stdout 为空，真版本在 stderr",
                ),
            ] {
                let dir = fixture_with_tool("", &tool_body);
                let _env = EnvGuard::set("/bin/zsh", dir.path());
                assert_found_fixture_version(try_get_version("claude"), case);
            }
        }

        #[test]
        #[serial_test::serial]
        fn dash_missing_command_does_not_fail_installation_enumeration() {
            let dir = tempfile::tempdir().expect("tempdir");
            let _env = EnvGuard::set("/bin/dash", dir.path());
            std::env::set_var("PATH", "/usr/bin:/bin");
            let tool = "__cc_switch_missing_tool_5817__";
            assert_eq!(
                super::super::super::resolve_path_default(tool, None).unwrap(),
                None
            );
            assert!(super::super::super::enumerate_tool_installations(tool)
                .unwrap()
                .is_empty());
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let reports = runtime
                .block_on(super::super::super::probe_tool_installations(vec![
                    "claude".into(),
                    "codex".into(),
                ]))
                .expect("a missing tool must not fail the whole batch");
            assert_eq!(reports.len(), 2);
        }

        /// 对照：工具确实不存在时必须仍报 NotFound。
        /// 缺了这条，一个「永远返回 Found」的实现也能通过上面几条。
        #[test]
        #[serial_test::serial]
        fn still_reports_not_found_when_the_tool_is_absent() {
            if !zsh_available() {
                return;
            }
            let dir = tempfile::tempdir().expect("tempdir");
            // PATH 必须**替换**而不是前置：继承来的 PATH 里有本机真实安装的工具，
            // 否则这条对照会因为探测到真家伙而"通过"，把自己变成假绿。
            std::fs::write(dir.path().join(".zshrc"), "export PATH=/usr/bin:/bin\n")
                .expect("write .zshrc");
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            let probe = try_get_version("claude");
            assert!(
                matches!(probe, Ok(ShellProbe::NotFound(_))),
                "工具不存在时应为 NotFound，实得 {probe:?}"
            );
        }

        /// 工具**跑通了**、退出码 0，但输出里没有版本号。
        ///
        /// 若是报 NotFound，调用方（`probe_local_version`）会 fallback 到
        /// `scan_cli_version`，拿另一处旧安装的版本当“当前版本”展示——用户实际在用的
        /// PATH 首选那处反而被藏起来。所以版本未知仍应保留“已安装且能运行”的事实。
        #[test]
        #[serial_test::serial]
        fn successful_run_without_a_version_is_not_reported_as_not_installed() {
            if !zsh_available() {
                return;
            }
            let dir = fixture_with_tool("", "printf 'development-build\\n'");
            let old_bin = dir.path().join("old/bin");
            std::fs::create_dir_all(&old_bin).unwrap();
            std::fs::write(old_bin.join("claude"), "#!/bin/sh\necho 1.0.0\n").unwrap();
            std::fs::set_permissions(
                old_bin.join("claude"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            std::env::set_var(
                "PATH",
                super::super::super::prepend_search_dir_to_path(
                    &old_bin,
                    &std::env::var_os("PATH").unwrap_or_default(),
                ),
            );
            let probe = super::super::probe_local_version("claude", None, None, None);
            assert!(
                matches!(&probe, Ok(ShellProbe::VersionUnknown(_))),
                "成功执行但版本未知不得触发跨安装 fallback，实得 {probe:?}"
            );
            let info = super::super::tool_version_from_probe("claude", probe);
            assert_eq!(info.installed, Some(true));
            assert!(!info.installed_but_broken);
            assert!(info.version.is_none());
        }

        #[test]
        #[serial_test::serial]
        fn silent_nonzero_exit_is_not_reported_as_not_installed() {
            if !zsh_available() {
                return;
            }
            let dir = fixture_with_tool("", "exit 23");
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            let probe = try_get_version("claude");
            assert!(
                matches!(&probe, Ok(ShellProbe::FoundButFailed(_))),
                "{probe:?}"
            );
            let info = super::super::tool_version_from_probe("claude", probe);
            assert_eq!(info.installed, Some(true));
            assert!(info.installed_but_broken);
        }

        /// 捕获失败不得变成未安装，也不得从另一处安装规划升级。
        /// TMPDIR 是进程级的，必须在 --exact 子进程内改，避免影响并行 tempfile 调用。
        #[test]
        fn uncreatable_capture_file_is_not_reported_as_not_installed() {
            const CHILD_ENV: &str = "CC_SWITCH_VERSION_PROBE_TMPDIR_CHILD";
            if std::env::var_os(CHILD_ENV).is_some() {
                run_tmpdir_probe_child();
                return;
            }
            let test_name = std::thread::current()
                .name()
                .expect("libtest 应为测试线程命名")
                .to_string();
            let output = std::process::Command::new(
                std::env::current_exe().expect("current test executable"),
            )
            .arg(test_name)
            .arg("--exact")
            .arg("--nocapture")
            .env(CHILD_ENV, "1")
            .output()
            .expect("spawn isolated tmpdir probe");

            assert!(
                output.status.success(),
                "隔离 TMPDIR 探测失败\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn run_tmpdir_probe_child() {
            let tmpdir = tempfile::tempdir().expect("tmpdir");
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::ffi::OsStrExt;
                let non_utf8 = tmpdir.path().join(std::ffi::OsStr::from_bytes(b"tmp-\xff"));
                std::fs::create_dir(&non_utf8).unwrap();
                std::env::set_var("TMPDIR", non_utf8);
                assert!(try_get_version("claude")
                    .unwrap_err()
                    .contains("not valid UTF-8"));
            }
            std::env::set_var("TMPDIR", tmpdir.path().join("missing"));

            let probe = try_get_version("claude");
            assert!(probe.is_err(), "捕获失败必须报探测错误，实得 {probe:?}");
            let version = super::super::tool_version_from_probe("claude", probe);
            assert!(version.installed.is_none());
            assert!(!version.installed_but_broken);
            // 路径解析同属“用不了登录 shell”的故障，不能退化成「PATH 上没有」：
            // `locate_default_tool` 会据此另选一处旧安装作为升级/卸载目标。
            assert!(
                super::super::super::resolve_path_default("claude", None).is_err(),
                "建不出临时文件时路径解析应回 Err，不得退化成 Ok(None)"
            );
            // 即使能从缓存/扫描得到两处安装，也不能绕过丢失的默认入口去更新 npm 那处。
            let brew = tmpdir.path().join("homebrew/bin");
            let npm = tmpdir.path().join("npm/bin");
            for bin in [&brew, &npm] {
                std::fs::create_dir_all(bin).unwrap();
                std::fs::write(bin.join("gemini"), "#!/bin/sh\necho 9.9.9\n").unwrap();
                std::fs::set_permissions(
                    bin.join("gemini"),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            std::env::set_var("PATH", std::env::join_paths([&brew, &npm]).unwrap());
            assert!(super::super::super::build_tool_action_line(
                "gemini",
                super::super::super::ToolLifecycleAction::Update,
                None,
                None
            )
            .is_err());
            let runtime = tokio::runtime::Runtime::new().unwrap();
            assert!(runtime
                .block_on(super::super::super::probe_tool_installations(vec![
                    "gemini".into()
                ]))
                .is_err());
        }

        /// rc 用 `TRAPEXIT` 改写 shell 的退出码——`resolve_path_default` 刻意不看退出码，
        /// 因为那是 **shell 的**、不是 `command -v` 的：工具明明在，退出码却是 3，而临时
        /// 文件里躺着正确路径。据退出码判就丢掉一次成功的解析，
        /// `enumerate_tool_installations` 随之失去 `is_path_default` 锚点。
        ///
        /// 只钉路径解析这一侧。`try_get_version` 反过来**必须**认这个退出码：那是工具
        /// `--version` 自身的退出码，"没装(127)"与"装了但报错"全靠它区分；TRAPEXIT 在
        /// 那一侧得到 `FoundButFailed` 是既定行为，不是回归。
        #[test]
        #[serial_test::serial]
        fn ignores_an_rc_rewritten_shell_exit_status() {
            if !zsh_available() {
                return;
            }
            let dir = fixture("TRAPEXIT() { return 3 }");
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            assert_eq!(
                super::super::super::resolve_path_default("claude", None)
                    .expect("resolve_path_default 不应报错"),
                std::fs::canonicalize(dir.path().join("bin/claude")).ok(),
                "rc 改写 shell 退出码后，路径解析仍应拿到临时文件里那条路径"
            );
        }

        /// rc 从 stdin 读输入（`read -r`：确认提示、版本管理器的交互式引导都这么写）。
        /// 子进程若继承调用方的 stdin，`read` 会吃掉本不属于它的字节，没人关闭时更是
        /// **永久**阻塞——不是慢，是挂死，整个关于面板再不返回。钉死 `stdin(Stdio::null())`。
        ///
        /// 直接换 fd 0 会污染整个多线程 libtest 进程，`#[serial]` 也挡不住未标记测试。
        /// 因此父用例重启当前测试二进制、用 `--exact` 只跑自己；只有那个隔离子进程才把 fd 0
        /// 换成装着已知字节的文件。marker 为空才说明探测没有读到调用方 stdin。
        #[test]
        fn returns_promptly_when_the_rc_reads_from_stdin() {
            const CHILD_ENV: &str = "CC_SWITCH_VERSION_PROBE_STDIN_CHILD";
            if std::env::var_os(CHILD_ENV).is_some() {
                run_stdin_probe_child();
                return;
            }
            if !zsh_available() {
                return;
            }

            let test_name = std::thread::current()
                .name()
                .expect("libtest 应为测试线程命名")
                .to_string();
            let output = std::process::Command::new(
                std::env::current_exe().expect("current test executable"),
            )
            .arg(test_name)
            .arg("--exact")
            .arg("--nocapture")
            .env(CHILD_ENV, "1")
            .output()
            .expect("spawn isolated stdin probe");

            assert!(
                output.status.success(),
                "隔离 stdin 探测失败\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn run_stdin_probe_child() {
            let mut feed = tempfile::NamedTempFile::new().expect("feed");
            std::io::Write::write_all(&mut feed, b"LEAKED\n").expect("write feed");
            // dup2 共享同一个 open file description（含读写偏移）——不倒回去，子进程一上来
            // 就是 EOF，这条用例会退化成"什么都没测"。
            std::io::Seek::rewind(&mut feed).expect("rewind feed");
            let marker = tempfile::NamedTempFile::new().expect("marker");
            let rc = format!(
                "read -r ANSWER\nprint -rn -- \"$ANSWER\" >>{}",
                super::super::super::shell_single_quote(&marker.path().to_string_lossy())
            );

            // fd 0 的置换只发生在这个仅运行本用例的隔离进程中。还原仍走 Drop，确保中途
            // 断言失败时 descriptor 不泄漏，也让 helper 能独立重复调用。
            let saved = unsafe { dup(0) };
            assert!(saved >= 0, "dup(0) 失败");
            let _fd = Fd0Guard(saved);
            let feed_fd = std::os::fd::AsRawFd::as_raw_fd(feed.as_file());
            assert!(unsafe { dup2(feed_fd, 0) } >= 0, "dup2 失败");

            let dir = fixture(&rc);
            let _env = EnvGuard::set("/bin/zsh", dir.path());
            let start = std::time::Instant::now();
            let probe = try_get_version("claude");
            // 路径解析必须在 fd 0 仍是 feed 时跑，否则断言就分辨不出守卫在不在。
            let path_default = super::super::super::resolve_path_default("claude", None)
                .expect("resolve_path_default 不应报错");
            let elapsed = start.elapsed();

            assert!(
                elapsed < std::time::Duration::from_secs(15),
                "探测耗时 {elapsed:?}：rc 的 `read` 在等一条继承来的 stdin"
            );
            assert!(
                std::fs::read(marker.path())
                    .expect("read marker")
                    .is_empty(),
                "rc 的 `read` 读到了调用方 stdin 里的字节——两条探测里有一条没把 stdin 置 null"
            );
            assert_found_fixture_version(probe, "rc 从 stdin 读输入");
            assert_eq!(
                path_default,
                std::fs::canonicalize(dir.path().join("bin/claude")).ok(),
                "rc 从 stdin 读输入时，路径解析仍应返回并拿到 fixture 入口"
            );
        }

        /// 把进程 fd 0 还原回 `saved` 并关掉它。`assert` 覆盖还原本身失败的情形——
        /// 静默失败会把一条坏掉的 fd 0 留给同进程后续所有测试。
        struct Fd0Guard(i32);

        impl Drop for Fd0Guard {
            fn drop(&mut self) {
                let restored = unsafe { dup2(self.0, 0) };
                unsafe { close(self.0) };
                assert!(restored >= 0, "还原 fd 0 失败");
            }
        }

        // 只为上面那条用例换 fd 0；三个声明就够，不值得为此背一个 `libc` dev-dependency。
        extern "C" {
            fn dup(fd: i32) -> i32;
            fn dup2(old: i32, new: i32) -> i32;
            fn close(fd: i32) -> i32;
        }

        /// `resolve_path_default` 在返回前自己做掉 canonicalize。少了它，返回的是**入口**
        /// 路径，而 `enumerate_tool_installations` 的比对对象 `real` 是 canonicalize 后的
        /// 真身，`is_path_default` 恒为 false：升级失去锚点，UI 却照样展示"将写回原生那处"
        /// ——`anchored_command_from_paths` doc 里记的那种欺骗性故障。
        ///
        /// 故意用软链入口（`bin/claude` → `libexec/claude.sh`）：只有入口 ≠ 真身时，
        /// 漏掉的 canonicalize 才看得出来。
        #[test]
        #[serial_test::serial]
        fn marks_the_symlinked_path_default_installation() {
            if !zsh_available() {
                return;
            }
            let dir = tempfile::tempdir().expect("tempdir");
            let bin = dir.path().join("bin");
            let libexec = dir.path().join("libexec");
            std::fs::create_dir_all(&bin).expect("mkdir bin");
            std::fs::create_dir_all(&libexec).expect("mkdir libexec");

            let real_tool = libexec.join("claude.sh");
            std::fs::write(
                &real_tool,
                format!("#!/bin/sh\necho '{FIXTURE_VERSION} (Fixture CLI)'\n"),
            )
            .expect("write fake tool");
            std::fs::set_permissions(&real_tool, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake tool");
            std::os::unix::fs::symlink("../libexec/claude.sh", bin.join("claude"))
                .expect("symlink entry");
            std::fs::write(
                dir.path().join(".zshrc"),
                format!("export PATH={}:$PATH\n", bin.display()),
            )
            .expect("write .zshrc");

            let _env = EnvGuard::set("/bin/zsh", dir.path());
            // `build_tool_search_paths` 只从进程 PATH 看得见 fixture 目录。**前置**而非替换：
            // 同进程里并发跑的其它测试仍要用得到原来那些目录。还原交给 EnvGuard。
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );

            let entry = bin.join("claude").display().to_string();
            let installs = super::super::super::enumerate_tool_installations("claude")
                .expect("enumerate fixture installations");
            let found = installs
                .iter()
                .find(|i| i.path == entry)
                .unwrap_or_else(|| panic!("枚举结果里没有 fixture 入口 {entry}：{installs:?}"));
            assert!(
                found.is_path_default,
                "fixture 入口应被标记为 PATH 默认那处（调用点漏了 canonicalize 就恒为 false）"
            );
        }
    }
}
