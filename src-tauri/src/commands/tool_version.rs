use super::*;

/// 获取单个工具的版本信息（内部实现）
/// 本机工具的版本（只探测本地，不联网）。
fn probe_local_version(
    tool: &str,
    wsl_distro: Option<&str>,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
) -> ShellProbe {
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
            match probe_path_default_version(tool) {
                ShellProbe::NotFound(_) => scan_cli_version(tool),
                found => found,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            // PATH 第一个命令优先；只有它确实没装(NotFound)才去常见目录兜底扫描。
            match try_get_version(tool) {
                ShellProbe::NotFound(_) => scan_cli_version(tool),
                found => found,
            }
        }
    }
}

/// 本机实际安装的工具版本（和「关于」页探测的是同一个）；拿不到为 `None`。
pub(crate) fn local_tool_version(tool: &str) -> Option<String> {
    let (_, wsl_distro) = tool_env_type_and_wsl_distro(tool);
    match probe_local_version(tool, wsl_distro.as_deref(), None, None) {
        ShellProbe::Found(version) => Some(version),
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
    let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(tool);

    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();

    // 1. 获取本地版本
    let probe = probe_local_version(tool, wsl_distro.as_deref(), wsl_shell, wsl_shell_flag);
    let (local_version, local_error, installed_but_broken) = match probe {
        ShellProbe::Found(v) => (Some(v), None, false),
        ShellProbe::FoundButFailed(e) => (None, Some(e), true),
        // 纯粹没装不带 error：版本列已显示「未安装」，再显示一行英文原因是重复的。
        // WSL 的 shell 参数非法、执行失败等其它原因仍照常上报。
        ShellProbe::NotFound(e) if e.ends_with(NOT_INSTALLED) => (None, None, false),
        ShellProbe::NotFound(e) => (None, Some(e), false),
    };

    // 2. 获取远程最新版本（npm 工具在本地领先 latest 时会按预发布通道补查，见
    //    fetch_npm_latest_for_tool / npm_prerelease_tags）
    let local = local_version.as_deref();
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

    ToolVersion {
        name: tool.to_string(),
        version: local_version,
        latest_version,
        error: local_error,
        installed_but_broken,
        env_type,
        wsl_distro,
        update_blocked_by_node,
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
        .and_then(|name| release_display_version(&extract_version(name)));
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

/// 从版本输出中提取纯版本号
pub(super) fn extract_version(raw: &str) -> String {
    VERSION_RE
        .find(raw)
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| raw.to_string())
}

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

/// CLI 版本探测的三态结果，跨平台统一各 probe（`try_get_version` /
/// `try_get_version_wsl` / `scan_cli_version`）的返回，进而在 `ToolVersion` 上给出
/// 结构化的 `installed_but_broken` 信号——避免前端靠匹配错误文案反推语义。
///
/// 关键区分"没装"与"装了但 `--version` 自身报错退出"（如工具要求更高的 Node 版本）：
/// 后者必须如实上报、不去别处捞旧版掩盖，否则"升级到新版却跑不起来"会被旧版盖住，
/// 表现为"升级成功但版本号不变"。
pub(super) enum ShellProbe {
    /// 成功拿到版本号
    Found(String),
    /// 可执行存在、但 `--version` 非零退出（携带诊断信息，如 stderr 末尾若干行）
    FoundButFailed(String),
    /// 没找到该命令（携带描述性消息，供 UI 展示）
    NotFound(String),
}

/// 在非 Windows 平台用用户 shell 执行 `{tool} --version` 探测版本。
///
/// Windows 不走此路径：`cmd /C {tool}` 可能误触发 App Execution Alias /
/// 协议处理器（曾导致 Windows 版整体被禁用），那里改由 `scan_cli_version`
/// 只执行已定位到的真实可执行文件。
#[cfg(not(target_os = "windows"))]
fn try_get_version(tool: &str) -> ShellProbe {
    use std::process::{Command, Stdio};

    let output = {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| is_valid_shell(s))
            .unwrap_or_else(|| "sh".to_string());
        let flag = default_flag_for_shell(&shell);
        let mut cmd = Command::new(shell);
        cmd.arg(flag)
            .arg(format!("{tool} --version"))
            // 与 `resolve_path_default` 相同：交互式 shell（-lic）不能读到继承来的
            // stdin，也不能因持有控制终端被 SIGTTIN 停住（从终端启动时整个应用卡死）。
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        isolate_child_process_group(&mut cmd);
        cmd.spawn().map_err(|e| e.to_string()).and_then(|child| {
            wait_child_output(
                child,
                CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
            )
        })
    };

    match output {
        Ok(out) => {
            let stdout = decode_command_output(&out.stdout).trim().to_string();
            let stderr = decode_command_output(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    ShellProbe::NotFound(NOT_INSTALLED.to_string())
                } else {
                    ShellProbe::Found(extract_version(raw))
                }
            } else {
                // exit 127 = shell 找不到命令（可放心 fallback 到搜索路径）；其它非零码
                // = 命令存在但 --version 自身报错退出，须如实上报、不 fallback 掩盖。
                let err = if stderr.is_empty() { stdout } else { stderr };
                if out.status.code() == Some(127) || err.is_empty() {
                    ShellProbe::NotFound(NOT_INSTALLED.to_string())
                } else {
                    ShellProbe::FoundButFailed(last_lines(err.trim(), 4))
                }
            }
        }
        Err(_) => ShellProbe::NotFound(NOT_INSTALLED.to_string()),
    }
}
#[cfg(any(target_os = "windows", test))]
fn wsl_version_probe_shell_command(tool: &str, flag: Option<&str>) -> String {
    wsl_auto_shell_command(&version_probe_payload(tool), flag)
}

#[cfg(test)]
pub(super) fn wsl_version_probe_command_line(tool: &str, distro: &str) -> Result<String, String> {
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: {distro}"));
    }
    let cmd = wsl_version_probe_shell_command(tool, None);
    Ok(format!(
        "wsl.exe -d {distro} -- sh -c {}",
        windows_cmd_double_quote_arg(&cmd)
    ))
}

#[cfg(target_os = "windows")]
fn try_get_version_wsl(
    tool: &str,
    distro: &str,
    force_shell: Option<&str>,
    force_shell_flag: Option<&str>,
) -> ShellProbe {
    use std::process::Command;

    // 防御性断言：tool 只能是预定义的值
    debug_assert!(known_tool(tool).is_some(), "unexpected tool name: {tool}");

    // 校验 distro 名称，防止命令注入
    if !is_valid_wsl_distro_name(distro) {
        return ShellProbe::NotFound(format!("[WSL:{distro}] invalid distro name"));
    }

    // 构建 Shell 脚本检测逻辑
    let (shell, flag, cmd) = if let Some(shell) = force_shell {
        // Defensive validation: never allow an arbitrary executable name here.
        if !is_valid_shell(shell) {
            return ShellProbe::NotFound(format!("[WSL:{distro}] invalid shell: {shell}"));
        }
        let shell = shell.rsplit('/').next().unwrap_or(shell);
        let flag = if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return ShellProbe::NotFound(format!("[WSL:{distro}] invalid shell flag: {flag}"));
            }
            flag
        } else {
            default_flag_for_shell(shell)
        };

        (shell.to_string(), flag, version_probe_payload(tool))
    } else {
        if let Some(flag) = force_shell_flag {
            if !is_valid_shell_flag(flag) {
                return ShellProbe::NotFound(format!("[WSL:{distro}] invalid shell flag: {flag}"));
            }
        }
        let cmd = wsl_version_probe_shell_command(tool, force_shell_flag);

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
                let raw = if payload_out.is_empty() {
                    &stderr
                } else {
                    &payload_out
                };
                if raw.is_empty() {
                    ShellProbe::NotFound(format!("[WSL:{distro}] {NOT_INSTALLED}"))
                } else {
                    ShellProbe::Found(extract_version(raw))
                }
            } else {
                let err = if stderr.is_empty() {
                    payload_out
                } else {
                    stderr
                };
                // wsl.exe 透传的退出码不总可靠，故同时用 exit 127 与 shell 的"找不到命令"
                // 文本兜底判别"没装"；其余非零退出视作"装了但 --version 报错"。
                let not_found = err.is_empty()
                    || out.status.code() == Some(127)
                    || is_shell_command_missing(&err, tool);
                if not_found {
                    ShellProbe::NotFound(format!("[WSL:{distro}] {NOT_INSTALLED}"))
                } else {
                    ShellProbe::FoundButFailed(format!(
                        "[WSL:{distro}] {}",
                        last_lines(err.trim(), 4)
                    ))
                }
            }
        }
        Err(e) => ShellProbe::NotFound(format!("[WSL:{distro}] exec failed: {e}")),
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
) -> ShellProbe {
    ShellProbe::NotFound("WSL check not supported on this platform".to_string())
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
        assert_eq!(extract_version("claude 1.0.20"), "1.0.20");
        assert_eq!(extract_version("v2.3.4-beta.1"), "2.3.4-beta.1");
        // Hermes main/canary 开发构建:保留 local version 段,前端据此识别、不误报升级
        assert_eq!(
            extract_version("Hermes Agent v0.22.1+238.g8211bb9 (2026.9.24) · upstream 8211bb96"),
            "0.22.1+238.g8211bb9"
        );
        assert_eq!(extract_version("no version here"), "no version here");
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
            ShellProbe::FoundButFailed(detail) => detail,
            ShellProbe::Found(v) => panic!("expected broken, got version {v}"),
            ShellProbe::NotFound(e) => panic!("expected broken, got not found: {e}"),
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
            ShellProbe::Found(v) if v == "1.2.3"
        ));
        assert!(matches!(
            scan_cli_version_in("kimi", &[empty, same_name_dir]),
            ShellProbe::NotFound(_)
        ));
    }
}
