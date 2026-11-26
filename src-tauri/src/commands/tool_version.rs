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

pub(super) fn npm_package_for_version(tool: &str, version: Option<&str>) -> Option<&'static str> {
    if tool == "opencode"
        && version
            .and_then(parse_semver)
            .is_some_and(|(core, _)| core[0] >= 2)
    {
        Some(OPENCODE_V2_NPM_PACKAGE)
    } else {
        tool_spec(tool).npm_package
    }
}

fn opencode_install_path(
    environment: &str,
    wsl_distro: Option<&str>,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
) -> Option<(PathBuf, PathBuf)> {
    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro {
        let pref = WslShellPreferenceInput {
            wsl_shell: wsl_shell.map(str::to_string),
            wsl_shell_flag: wsl_shell_flag.map(str::to_string),
        };
        return tool_uninstall::wsl_installed_paths("opencode", distro, &pref);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (wsl_distro, wsl_shell, wsl_shell_flag);
        if environment == "linux" {
            return resolve_path_default_entry(
                "opencode",
                CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
            )
            .ok()
            .flatten()
            .map(|entry| (entry.launcher, entry.real));
        }
    }
    #[cfg(target_os = "windows")]
    let _ = environment;
    None
}

fn opencode_manual_update_for_version(
    environment: &str,
    version: Option<&str>,
    installation: impl FnOnce() -> Option<(PathBuf, PathBuf)>,
) -> bool {
    // WSL execution requires a verified npm launcher for both major versions.
    // On native platforms only V2 uses this additional ownership check.
    if environment == "wsl" && version.is_some_and(version_is_prerelease) {
        return true;
    }
    version.is_some()
        && (environment == "wsl"
            || npm_package_for_version("opencode", version) == Some(OPENCODE_V2_NPM_PACKAGE))
        && opencode_update_is_manual(
            environment,
            installation()
                .as_ref()
                .map(|(entry, real)| (entry.as_path(), real.as_path())),
        )
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

    #[cfg(target_os = "macos")]
    if tool == "codex" {
        if let Ok(Some(desktop)) =
            tokio::task::spawn_blocking(crate::codex_config::codex_macos_official_installation)
                .await
        {
            let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(tool);
            return ToolVersion {
                name: tool.to_string(),
                version: None,
                latest_version: None,
                error: None,
                installed_but_broken: false,
                desktop_installation: Some(desktop),
                desktop_managed: true,
                env_type,
                wsl_distro,
                update_blocked_by_node: None,
                manual_update_only: false,
            };
        }
    }

    // 使用全局 HTTP 客户端（已包含代理配置）
    let client = crate::proxy::http_client::get();

    // 1. 本地版本（`where` + `--version` 子进程，阻塞）放进 blocking 线程池，
    //    避免阻塞其他工具的探测。最新版本需根据实际安装渠道查询。
    let local_probe = {
        let tool_owned = tool.to_string();
        let wsl_shell = wsl_shell.map(str::to_string);
        let wsl_shell_flag = wsl_shell_flag.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            // 判断该工具的运行环境 & WSL distro（如有）
            let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(&tool_owned);
            let probe = probe_local_version(
                &tool_owned,
                wsl_distro.as_deref(),
                wsl_shell.as_deref(),
                wsl_shell_flag.as_deref(),
            );
            (env_type, wsl_distro, probe)
        })
    };
    let (env_type, wsl_distro, probe) = match local_probe.await {
        Ok(result) => result,
        Err(e) => {
            let (env_type, wsl_distro) = tool_env_type_and_wsl_distro(tool);
            (
                env_type,
                wsl_distro,
                ShellProbe::NotFound(format!("version probe task failed: {e}")),
            )
        }
    };

    let (local_version, local_error, installed_but_broken) = match probe {
        ShellProbe::Found(v) => (Some(v), None, false),
        ShellProbe::FoundButFailed(e) => (None, Some(e), true),
        // 纯粹没装不带 error：版本列已显示「未安装」，再显示一行英文原因是重复的。
        // WSL 的 shell 参数非法、执行失败等其它原因仍照常上报。
        ShellProbe::NotFound(e) if e.ends_with(NOT_INSTALLED) => (None, None, false),
        ShellProbe::NotFound(e) => (None, Some(e), false),
    };

    // Match the version check to the installed copy, not just the tool name.
    let location = if (local_version.is_some() || installed_but_broken) && wsl_distro.is_none() {
        let tool = tool.to_owned();
        tokio::task::spawn_blocking(move || {
            let deadline = CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT));
            #[cfg(not(target_os = "windows"))]
            if let Some(paths) = resolve_path_default_entry(&tool, deadline).ok().flatten() {
                return Some((paths.launcher, paths.real));
            }
            let launcher = locate_default_tool(&tool, deadline).ok()?;
            let real = std::fs::canonicalize(&launcher).ok()?;
            Some((launcher, real))
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    let desktop_managed = location
        .as_ref()
        .is_some_and(|(_, real)| desktop_managed_codex_cli(tool, real));
    let local = local_version.as_deref();
    let manual_update_only = location.as_ref().is_some_and(|(launcher, real)| {
        npm_prerelease_requires_manual_update(tool, launcher, real, local)
    }) || (tool == "opencode"
        && opencode_manual_update_for_version(&env_type, local, || {
            opencode_install_path(&env_type, wsl_distro.as_deref(), wsl_shell, wsl_shell_flag)
        }));
    let latest_version = if desktop_managed || local.is_none() {
        None
    } else if let Some((launcher, real)) = &location {
        latest_version_for_installation(&client, tool, launcher, real, local).await
    } else if wsl_distro.is_some() {
        wsl_latest_version(&client, tool, local).await
    } else {
        None
    };

    let npm_package = location
        .as_ref()
        .and_then(|(launcher, real)| {
            let manifest = global_node_package_manifest(tool, launcher, real)?;
            let name = manifest.get("name")?.as_str()?;
            npm_package_candidates(tool).find(|candidate| *candidate == name)
        })
        .or_else(|| {
            wsl_distro
                .is_some()
                .then(|| npm_package_for_version(tool, local))
                .flatten()
        });
    let update_blocked_by_node = match (local, latest_version.as_deref(), npm_package) {
        (Some(local), Some(latest), Some(package))
            if !manual_update_only
                && compare_semver(local, latest) == Some(std::cmp::Ordering::Less) =>
        {
            tool_node_requirement::update_node_gap(&client, tool, package, latest).await
        }
        _ => None,
    };

    ToolVersion {
        name: tool.to_string(),
        version: local_version,
        latest_version,
        error: local_error,
        installed_but_broken,
        desktop_installation: None,
        desktop_managed,
        env_type,
        wsl_distro,
        update_blocked_by_node,
        manual_update_only,
    }
}

pub(super) fn volta_package_for_version(tool: &str, local: Option<&str>) -> Option<&'static str> {
    if !tool_spec(tool).npm_installable
        || (tool == "opencode" && local.and_then(parse_semver).is_none())
    {
        return None;
    }
    npm_package_for_version(tool, local)
}

async fn wsl_latest_version(
    client: &reqwest::Client,
    tool: &str,
    local: Option<&str>,
) -> Option<String> {
    match (tool, npm_package_for_version(tool, local)) {
        ("hermes", _) => fetch_hermes_latest_version(client, local).await,
        ("agy", _) => fetch_agy_latest_version(client).await,
        ("opencode", Some(package)) => {
            match fetch_npm_latest_for_tool(client, package, tool, local).await {
                Some(version) => Some(version),
                None if package == OPENCODE_V2_NPM_PACKAGE => {
                    fetch_opencode_v2_release(client).await
                }
                None => fetch_github_latest_version(client, "anomalyco/opencode").await,
            }
        }
        (_, Some(package)) => fetch_npm_latest_for_tool(client, package, tool, local).await,
        (_, None) => None,
    }
}

pub(super) async fn latest_version_for_installation(
    client: &reqwest::Client,
    tool: &str,
    launcher: &Path,
    real: &Path,
    local: Option<&str>,
) -> Option<String> {
    if desktop_managed_codex_cli(tool, real) {
        return None;
    }
    if verified_volta_manager(launcher).is_some() {
        let tags = fetch_npm_dist_tags(client, volta_package_for_version(tool, local)?).await?;
        // The confirmed updater explicitly installs @latest. Do not advertise
        // a tool-specific next/beta tag that this plan would not install.
        return pick_latest_version(&tags, &[], None)
            .filter(|version| !version_is_prerelease(version));
    }
    #[cfg(not(target_os = "windows"))]
    if is_brew_managed_path(real.to_str()?) {
        let install = homebrew_installation(real)?;
        return tokio::task::spawn_blocking(move || fetch_homebrew_release(install))
            .await
            .ok()
            .flatten();
    }
    if tool == "codex" && is_standalone_codex(launcher, real) {
        return fetch_codex_installer_release(client).await;
    }
    #[cfg(not(target_os = "windows"))]
    if is_standard_curl_install(tool, real) {
        return match tool {
            "claude" => {
                let version = client
                    .get("https://downloads.claude.ai/claude-code-releases/latest")
                    .timeout(LATEST_PROBE_TIMEOUT)
                    .send()
                    .await
                    .ok()?
                    .error_for_status()
                    .ok()?
                    .text()
                    .await
                    .ok()?;
                release_display_version(version.trim())
            }
            "opencode" if npm_package_for_version(tool, local) == Some(OPENCODE_V2_NPM_PACKAGE) => {
                match fetch_opencode_v2_installer_version(client).await {
                    Some(version) => Some(version),
                    None => fetch_opencode_v2_release(client).await,
                }
            }
            "opencode" => fetch_github_latest_version(client, "anomalyco/opencode").await,
            _ => None,
        };
    }
    if let Some(manifest) = global_node_package_manifest(tool, launcher, real) {
        let name = manifest.get("name")?.as_str()?;
        let package = npm_package_candidates(tool).find(|candidate| *candidate == name)?;
        if package == OPENCODE_V2_NPM_PACKAGE {
            return match fetch_npm_latest_for_tool(client, package, tool, local).await {
                Some(version) => Some(version),
                None => fetch_opencode_v2_release(client).await,
            };
        }
        return fetch_npm_latest_for_tool(client, package, tool, local).await;
    }

    if tool == "omp" && is_omp_standalone_install(launcher, real) {
        return fetch_omp_binary_release(launcher, local).await;
    }
    if is_native_executable(real) {
        match tool {
            "codebuddy" => return fetch_codebuddy_installer_release(client).await,
            "copilot" => return fetch_github_latest_version(client, "github/copilot-cli").await,
            _ => {}
        }
    }
    let native_installer = match tool {
        #[cfg(not(target_os = "windows"))]
        "pi" => additional_script_install(tool, launcher, real),
        "grok" => is_grok_native_install(launcher.to_str()?, real.to_str()?),
        "mcode" => {
            mcode_receipt_install_root(launcher.to_str()?, McodeLayout::NATIVE).is_some()
                || mcode_script_install_root(
                    launcher.to_str()?,
                    real.to_str()?,
                    McodeLayout::NATIVE,
                )
                .is_some()
        }
        "kimi" => kimi_install_dir(dirs::home_dir().as_deref())
            .is_some_and(|root| is_kimi_script_install(launcher, real, &root)),
        _ => false,
    };
    if native_installer {
        return fetch_npm_latest_for_tool(client, tool_spec(tool).npm_package?, tool, local).await;
    }

    match (tool, infer_install_source(launcher)) {
        ("hermes", _) => fetch_hermes_latest_version(client, local).await,
        ("agy", _) => fetch_agy_latest_version(client).await,
        (_, "nvm" | "fnm" | "mise" | "volta" | "pnpm" | "bun") => {
            let package = tool_spec(tool).npm_package?;
            fetch_npm_latest_for_tool(client, package, tool, local).await
        }
        _ => None,
    }
}

fn omp_checked_latest_version(stdout: &str, stderr: &str, current: Option<&str>) -> Option<String> {
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some((_, version)) = line.split_once("New version available:") {
            return release_display_version(&extract_version(version));
        }
    }
    (stdout.contains("Already up to date") || stderr.contains("Already up to date"))
        .then(|| current.and_then(release_display_version))
        .flatten()
}

/// Let the selected native CLI resolve its own effective channel, overlays and ABI.
/// --check performs no installation; timeout/failure/unrecognized output stays unknown.
async fn fetch_omp_binary_release(launcher: &Path, current: Option<&str>) -> Option<String> {
    let launcher = launcher.to_owned();
    let current = current.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        use std::process::Stdio;
        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = std::process::Command::new(&launcher);
            command.args(["update", "--check"]);
            if let Some(env) =
                login_shell_env(CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)))
            {
                if let Some(path) = env.path.as_deref() {
                    command.env(
                        "PATH",
                        merge_path_segments(path, &std::env::var("PATH").unwrap_or_default()),
                    );
                }
                apply_login_shell_vars(&mut command, &env.vars);
            }
            isolate_child_process_group(&mut command);
            command
        };
        #[cfg(target_os = "windows")]
        let mut command =
            build_windows_tool_command(&launcher, &["update", "--check"], &effective_path_string());
        let directory = omp_update_directory(&launcher)?;
        let path = command
            .get_envs()
            .find_map(|(key, value)| {
                (key == "PATH")
                    .then(|| value.map(std::ffi::OsStr::to_os_string))
                    .flatten()
            })
            .unwrap_or_else(|| std::env::var_os("PATH").unwrap_or_default());
        let path = omp_update_path(&directory, &path)?;
        command.env("PATH", path);
        command
            .current_dir(crate::config::get_home_dir())
            .env("NO_COLOR", "1")
            .env("FORCE_COLOR", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = wait_child_output(
            command.spawn().ok()?,
            CommandDeadline::from_timeout(Some(LATEST_PROBE_TIMEOUT)),
        )
        .ok()?;
        if !output.status.success() {
            return None;
        }
        omp_checked_latest_version(
            &decode_command_output(&output.stdout),
            &decode_command_output(&output.stderr),
            current.as_deref(),
        )
    })
    .await
    .ok()
    .flatten()
}

#[cfg(not(target_os = "windows"))]
fn fetch_homebrew_release(install: HomebrewInstallation) -> Option<String> {
    let mut command = std::process::Command::new(&install.brew);
    command
        .args([
            "info",
            "--json=v2",
            &format!("--{}", install.kind),
            &install.token,
        ])
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ANALYTICS", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    isolate_child_process_group(&mut command);
    let output = wait_child_output(
        command.spawn().ok()?,
        CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(10))),
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let info: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    homebrew_release_version(&info, install.kind, &install.token)
}

#[cfg(not(target_os = "windows"))]
fn homebrew_release_version(info: &serde_json::Value, kind: &str, token: &str) -> Option<String> {
    let matching = |key: &str, name: &str| {
        let mut entries = info
            .get(key)?
            .as_array()?
            .iter()
            .filter(|entry| entry.get(name).and_then(|value| value.as_str()) == Some(token));
        let entry = entries.next()?;
        entries.next().is_none().then_some(entry)
    };
    let version = if kind == "cask" {
        let entry = matching("casks", "token")?;
        let installed = entry.get("installed")?.as_str()?;
        if installed.is_empty() {
            return None;
        }
        entry.get("version")?.as_str()?.split(',').next()?
    } else {
        let entry = matching("formulae", "name")?;
        if entry.get("installed")?.as_array()?.is_empty() {
            return None;
        }
        entry.get("versions")?.get("stable")?.as_str()?
    };
    release_display_version(version)
}

/// This is the release channel used by the official CodeBuddy native installer.
async fn fetch_codebuddy_installer_release(client: &reqwest::Client) -> Option<String> {
    let response = client
        .get("https://acc-1258344699.cos.accelerate.myqcloud.com/@tencent-ai/codebuddy-code/releases/latest")
        .timeout(LATEST_PROBE_TIMEOUT)
        .send().await.ok()?.error_for_status().ok()?;
    let text = response.text().await.ok()?;
    release_display_version(text.trim())
}

async fn fetch_codex_installer_release(client: &reqwest::Client) -> Option<String> {
    if let Ok(response) = client
        .get("https://releases.openai.com/codex/channels/latest")
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
    {
        if let Ok(info) = response.json::<serde_json::Value>().await {
            if let Some(version) = info
                .get("tag_name")
                .and_then(|value| value.as_str())
                .map(|tag| tag.trim_start_matches("rust-v").trim_start_matches('v'))
                .and_then(release_display_version)
            {
                return Some(version);
            }
        }
    }
    // The official standalone installer also falls back to GitHub Releases.
    fetch_github_latest_version(client, "openai/codex").await
}

/// Classify the channel independently of non-comparable build metadata.
pub(super) fn version_is_prerelease(version: &str) -> bool {
    parse_semver(version.split('+').next().unwrap_or(version))
        .is_some_and(|(_, pre)| !pre.is_empty())
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

#[cfg(not(target_os = "windows"))]
async fn fetch_opencode_v2_installer_version(client: &reqwest::Client) -> Option<String> {
    // The official v2 install script reads its release and package from this endpoint.
    let metadata = client
        .get("https://opencode.ai/update/api/latest/cli/npm")
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?;
    let version = release_display_version(metadata.get("version")?.as_str()?)?;
    (npm_package_for_version("opencode", Some(&version)) == Some(OPENCODE_V2_NPM_PACKAGE))
        .then_some(version)
}

// GitHub's /latest endpoint intentionally stays on the OpenCode 1 release line.
async fn fetch_opencode_v2_release(client: &reqwest::Client) -> Option<String> {
    let releases: Vec<serde_json::Value> = client
        .get("https://api.github.com/repos/anomalyco/opencode/releases?per_page=30")
        .header("User-Agent", "cc-switch")
        .header("Accept", "application/vnd.github+json")
        .timeout(LATEST_PROBE_TIMEOUT)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    opencode_v2_release_version(&releases)
}

fn opencode_v2_release_version(releases: &[serde_json::Value]) -> Option<String> {
    releases
        .iter()
        .filter(|release| {
            release.get("prerelease").and_then(|value| value.as_bool()) == Some(false)
        })
        .filter_map(|release| release.get("tag_name")?.as_str())
        .filter_map(|tag| tag.strip_prefix('v'))
        .filter_map(|version| {
            let (core, pre) = parse_semver(version)?;
            (core[0] >= 2 && pre.is_empty()).then_some((core, version))
        })
        .max_by_key(|(core, _)| *core)
        .map(|(_, version)| version.to_string())
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
    Regex::new(r"\d+\.\d+\.\d+(-[\w.-]+)?(\+[\w.-]+)?").expect("Invalid version regex")
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
    format!("echo {VERSION_PROBE_SENTINEL}; echo {VERSION_PROBE_SENTINEL} >&2; {tool} --version")
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

/// Display, per-install diagnostics and WSL update verification share one parser.
pub(super) fn parse_tool_version_output(tool: &str, stdout: &str, stderr: &str) -> Option<String> {
    if tool != "opencode" {
        let raw = if stdout.trim().is_empty() {
            stderr
        } else {
            stdout
        };
        return (!raw.trim().is_empty()).then(|| extract_version(raw.trim()));
    }
    static LINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^\s*(?:opencode\s+)?v?(\d+\.\d+\.\d+)(.*)$")
            .expect("valid OpenCode version candidate")
    });
    static VALID: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z_.-]+)?(?:\+[0-9A-Za-z_.-]+)?$")
            .expect("valid OpenCode version syntax")
    });
    let mut versions = std::collections::BTreeSet::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let Some(captures) = LINE.captures(line) else {
            continue;
        };
        let core = &captures[1];
        let tail = &captures[2];
        let trimmed = tail.trim();
        let version = if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("(OpenCode)") {
            core.to_owned()
        } else if tail.chars().next().is_some_and(char::is_whitespace) {
            // Whitespace cannot split a version from its channel/build suffix.
            if trimmed.starts_with('-') || trimmed.starts_with('+') {
                return None;
            }
            // e.g. `22.14.123 (Node.js)` is a diagnostic, not another app version.
            continue;
        } else {
            // Preserve the entire suffix (including malformed Unicode whitespace)
            // until validation; never discard an invalid candidate in mixed output.
            let mut version = format!("{core}{}", tail.trim_end());
            if version.to_ascii_lowercase().ends_with("(opencode)") {
                let prefix = &version[..version.len() - "(OpenCode)".len()];
                if prefix.chars().last().is_some_and(char::is_whitespace) {
                    version = prefix.trim_end().to_owned();
                }
            }
            version
        };
        if !VALID.is_match(&version) {
            return None;
        }
        parse_semver(version.split('+').next()?)?;
        versions.insert(version);
    }
    (versions.len() == 1)
        .then(|| versions.into_iter().next())
        .flatten()
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

    // Summary and per-copy probes must read the same Volta store. A fresh
    // login shell can otherwise override the cached root used by update plans.
    let deadline = CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT));
    let volta = resolve_path_default_entry(tool, deadline)
        .ok()
        .flatten()
        .filter(|entry| verified_volta_manager(&entry.launcher).is_some());
    let located_volta = volta.is_some();
    let output = if let Some(entry) = volta {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let path = cached_login_shell_path().map_or_else(
            || inherited.clone(),
            |login| merge_path_segments(&login, &inherited.to_string_lossy()).into(),
        );
        run_probe_version_command(
            &entry.launcher,
            &prepend_search_dir_to_path(entry.launcher.parent().unwrap(), &path),
            deadline,
        )
    } else {
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
                    if located_volta {
                        ShellProbe::FoundButFailed(
                            "The located Volta shim returned no version".into(),
                        )
                    } else {
                        ShellProbe::NotFound(NOT_INSTALLED.to_string())
                    }
                } else {
                    parse_tool_version_output(tool, &stdout, &stderr)
                        .map(ShellProbe::Found)
                        .unwrap_or_else(|| {
                            ShellProbe::FoundButFailed(
                                "Cannot verify the reported OpenCode version.".into(),
                            )
                        })
                }
            } else {
                // exit 127 = shell 找不到命令（可放心 fallback 到搜索路径）；其它非零码
                // = 命令存在但 --version 自身报错退出，须如实上报、不 fallback 掩盖。
                let err = if stderr.is_empty() { stdout } else { stderr };
                if located_volta {
                    ShellProbe::FoundButFailed(if err.is_empty() {
                        "The located Volta shim could not run".into()
                    } else {
                        version_probe_error(&err)
                    })
                } else if out.status.code() == Some(127) || err.is_empty() {
                    ShellProbe::NotFound(NOT_INSTALLED.to_string())
                } else {
                    ShellProbe::FoundButFailed(version_probe_error(&err))
                }
            }
        }
        Err(error) if located_volta => ShellProbe::FoundButFailed(error),
        Err(_) => ShellProbe::NotFound(NOT_INSTALLED.to_string()),
    }
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
            let payload_err = after_version_sentinel(&stderr).to_string();
            if out.status.success() {
                let raw = if payload_out.is_empty() {
                    &payload_err
                } else {
                    &payload_out
                };
                if raw.is_empty() {
                    ShellProbe::NotFound(format!("[WSL:{distro}] {NOT_INSTALLED}"))
                } else {
                    parse_tool_version_output(tool, &payload_out, &payload_err)
                        .map(ShellProbe::Found)
                        .unwrap_or_else(|| {
                            ShellProbe::FoundButFailed(
                                "Cannot verify the reported OpenCode version.".into(),
                            )
                        })
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
                        version_probe_error(&err)
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
    fn opencode_v2_github_fallback_skips_legacy_and_prerelease_tags() {
        let releases = vec![
            serde_json::json!({"tag_name":"v1.19.6","prerelease":false}),
            serde_json::json!({"tag_name":"process-daemon-v3.0.0","prerelease":false}),
            serde_json::json!({"tag_name":"v2.4.0-beta.1","prerelease":true}),
            serde_json::json!({"tag_name":"v2.2.7","prerelease":false}),
            serde_json::json!({"tag_name":"v2.3.2","prerelease":false}),
        ];
        assert_eq!(
            opencode_v2_release_version(&releases).as_deref(),
            Some("2.3.2")
        );
    }

    #[test]
    fn opencode_uses_the_matching_npm_release_line() {
        assert_eq!(
            npm_package_for_version("opencode", Some("1.19.6")),
            Some("opencode-ai")
        );
        assert_eq!(
            npm_package_for_version("opencode", Some("2.3.2")),
            Some(OPENCODE_V2_NPM_PACKAGE)
        );
        assert_eq!(
            npm_package_for_version("opencode", None),
            Some("opencode-ai")
        );
    }

    #[test]
    fn linux_open_code_two_uses_the_package_launcher_for_update_classification() {
        assert!(opencode_manual_update_for_version(
            "linux",
            Some("2.3.2"),
            || { Some(("/usr/bin/opencode".into(), "/opt/opencode/opencode".into())) }
        ));
        assert!(!opencode_manual_update_for_version(
            "linux",
            Some("2.3.2"),
            || {
                Some((
                    "/home/test/.npm/bin/opencode".into(),
                    "/home/test/.npm/lib/node_modules/@opencode/cli/bin/opencode".into(),
                ))
            }
        ));
    }

    #[test]
    fn opencode_display_and_update_parse_only_unambiguous_app_versions() {
        assert_eq!(
            parse_tool_version_output("opencode", "Node 22.14.0\n2.3.2-next.1", "").as_deref(),
            Some("2.3.2-next.1")
        );
        assert_eq!(
            parse_tool_version_output("opencode", "Node 22.14.0", "opencode 2.3.2-next.1")
                .as_deref(),
            Some("2.3.2-next.1")
        );
        assert_eq!(
            parse_tool_version_output("opencode", "22.14.0\n2.3.2-next.1", ""),
            None
        );
        assert_eq!(
            parse_tool_version_output("opencode", "Node 22.14.0", ""),
            None
        );
    }

    #[test]
    fn release_channel_ignores_build_metadata() {
        assert!(!version_is_prerelease("2.3.2+glibc.2.31.0-x64"));
        assert!(!version_is_prerelease("2.3.2+linux-x64"));
        assert!(version_is_prerelease("2.3.2-next.1+nightly"));
    }

    #[test]
    fn version_probe_preserves_hyphenated_prerelease_identifiers() {
        let version = extract_version("opencode 2.3.2--preview.1\nA newer release is available");
        assert_eq!(version, "2.3.2--preview.1");
        assert!(parse_semver(&version).is_some_and(|(_, pre)| !pre.is_empty()));
    }

    #[test]
    fn omp_native_check_uses_the_selected_updaters_result() {
        for (stdout, stderr, expected) in [
            (
                "Current version: 1.0.0\nNew version available: 2.0.0",
                "",
                Some("2.0.0"),
            ),
            (
                "Current channel: canary\nNew version available: 2.0.0-canary.1",
                "",
                Some("2.0.0-canary.1"),
            ),
            ("", "New version available: 2.0.0", Some("2.0.0")),
            ("Already up to date", "", Some("1.0.0")),
            ("Current version: 1.0.0", "unrecognized output", None),
        ] {
            assert_eq!(
                omp_checked_latest_version(stdout, stderr, Some("1.0.0")).as_deref(),
                expected
            );
        }
    }

    #[test]
    fn wsl_prereleases_require_manual_update_even_with_verified_npm_ownership() {
        for version in [
            "1.19.6-beta.1",
            "2.3.2-next.1",
            "2.3.2-next.1+nightly",
            "2.3.2--preview.1+local",
        ] {
            assert!(opencode_manual_update_for_version(
                "wsl",
                Some(version),
                || {
                    Some((
                        "/usr/local/bin/opencode".into(),
                        "/usr/local/lib/node_modules/opencode-ai/bin/opencode".into(),
                    ))
                }
            ));
        }
    }

    #[test]
    fn wsl_open_code_one_requires_verified_npm_ownership_before_offering_update() {
        let launcher = "/home/test/.opencode/bin/opencode";
        assert!(opencode_manual_update_for_version(
            "wsl",
            Some("1.19.6"),
            || { Some((launcher.into(), launcher.into())) }
        ));
        assert!(!opencode_manual_update_for_version(
            "wsl",
            Some("1.19.6"),
            || {
                Some((
                    "/home/test/.npm/bin/opencode".into(),
                    "/home/test/.npm/lib/node_modules/opencode-ai/bin/opencode".into(),
                ))
            }
        ));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn homebrew_releases_match_the_installed_package_and_kind() {
        let info = serde_json::json!({
            "casks": [
                {"token":"claude-code","installed":"2.1.322","version":"2.1.323,checksum"},
                {"token":"uninstalled","installed":null,"version":"9.9.9"}
            ],
            "formulae":[{"name":"opencode","installed":[{"version":"2.3.1"}],"versions":{"stable":"2.3.2"}}]
        });
        assert_eq!(
            super::homebrew_release_version(&info, "cask", "claude-code").as_deref(),
            Some("2.1.323")
        );
        assert_eq!(
            super::homebrew_release_version(&info, "formula", "opencode").as_deref(),
            Some("2.3.2")
        );
        assert!(super::homebrew_release_version(&info, "formula", "claude-code").is_none());
        assert!(super::homebrew_release_version(&info, "cask", "uninstalled").is_none());
    }

    #[test]
    fn shell_command_missing_and_version_sentinel() {
        for msg in [
            "bash: line 1: claude: command not found",
            "zsh:1: command not found: claude",
            "sh: 1: claude: not found",
            "fish: Unknown command: claude",
        ] {
            assert!(is_shell_command_missing(msg, "claude"), "{msg}");
        }
        assert!(!is_shell_command_missing(
            "Error: Cannot find module 'x'\nmodule not found",
            "claude"
        ));
        assert!(!is_shell_command_missing("config file not found", "claude"));

        assert_eq!(
            version_probe_payload("claude"),
            "echo __CCSWITCH_VERSION__; echo __CCSWITCH_VERSION__ >&2; claude --version"
        );
        let motd = "Welcome to Ubuntu 24.04.4 LTS\n__CCSWITCH_VERSION__\n2.1.270 (Claude Code)";
        assert_eq!(after_version_sentinel(motd), "2.1.270 (Claude Code)");
        assert_eq!(
            after_version_sentinel("__CCSWITCH_VERSION__\nwarn\n__CCSWITCH_VERSION__\n1.2.3"),
            "1.2.3"
        );
        assert_eq!(
            after_version_sentinel(
                "\x1b]1337;RemoteHost=u@198.51.100.23\x07__CCSWITCH_VERSION__\n0.154.0"
            ),
            "0.154.0"
        );
        assert_eq!(after_version_sentinel("__CCSWITCH_VERSION__\n"), "");
        assert_eq!(
            after_version_sentinel("sh: 1: bad: not found"),
            "sh: 1: bad: not found"
        );
    }

    #[cfg(unix)]
    #[test]
    fn wsl_display_version_payload_isolates_both_startup_streams() {
        for stream in ["", " >&2"] {
            let payload = version_probe_payload("opencode");
            let script = format!("printf '22.14.0'; printf '20.1.0' >&2; opencode() {{ printf '2.3.2\\n'{stream}; }}; {payload}");
            let output = std::process::Command::new("sh")
                .args(["-c", &script])
                .output()
                .unwrap();
            assert!(output.status.success());
            let stdout = decode_command_output(&output.stdout);
            let stderr = decode_command_output(&output.stderr);
            assert_eq!(
                parse_tool_version_output(
                    "opencode",
                    after_version_sentinel(&stdout),
                    after_version_sentinel(&stderr)
                )
                .as_deref(),
                Some("2.3.2")
            );
        }
        assert!(
            parse_tool_version_output("opencode", "opencode ２.３.２-beta.1\n2.3.2", "").is_none()
        );
        assert_eq!(
            parse_tool_version_output("opencode", "opencode 2.3.2-beta.1   (OpenCode)", "")
                .as_deref(),
            Some("2.3.2-beta.1")
        );
        for (output, version) in [
            ("opencode 2.3.2+linux-x64  (OpenCode)", "2.3.2+linux-x64"),
            ("opencode 2.3.2-beta.1\t(OpenCode)", "2.3.2-beta.1"),
            ("opencode 2.3.2-beta.1\u{a0}(OpenCode)", "2.3.2-beta.1"),
        ] {
            assert_eq!(
                parse_tool_version_output("opencode", output, "").as_deref(),
                Some(version)
            );
        }
        for diagnostic in ["22.14.0 (Node.js)", "22.14.123 (Node.js)"] {
            assert_eq!(
                parse_tool_version_output("opencode", "2.3.2", diagnostic).as_deref(),
                Some("2.3.2")
            );
        }
        for invalid in [
            "2.3.2-beta.１",
            "2.3.2-beta.é",
            "2.3.2+build.１",
            "2.3.2–beta.1",
            "2.3.2-beta.\u{a0}1",
            "2.3.2\u{a0}-beta.1",
            "2.3.2\u{a0}+linux-x64",
        ] {
            assert!(parse_tool_version_output(
                "opencode",
                &format!("opencode {invalid}\n2.3.2"),
                ""
            )
            .is_none());
        }
    }

    #[test]
    fn version_extraction_and_semver_comparison() {
        use std::cmp::Ordering;

        assert_eq!(extract_version("claude 1.0.20"), "1.0.20");
        assert_eq!(extract_version("v2.3.4-beta.1"), "2.3.4-beta.1");
        assert_eq!(
            extract_version("Hermes Agent v0.22.1+238.g8211bb9 (2026.9.24) · upstream 8211bb96"),
            "0.22.1+238.g8211bb9"
        );
        assert_eq!(extract_version("no version here"), "no version here");

        // Dev builds with git/canary local metadata are incomparable; standard build metadata is ignored.
        assert!(parse_semver("0.22.1+238.g8211bb9").is_none());
        assert!(parse_semver("0.22.1+canary.20261015T070051Z").is_none());
        assert!(compare_semver("0.22.2", "0.22.1+238.g8211bb9").is_none());
        assert_eq!(
            compare_semver("1.0.1", "1.0.0+linux.1"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_semver("2.1.156-beta.1", "2.1.156"),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_semver("0.45.0-nightly.1", "0.44.1"),
            Some(Ordering::Greater)
        );
        assert_eq!(
            compare_semver("0.1.2505172116", "0.135.0"),
            Some(Ordering::Less)
        );
        assert_eq!(compare_semver("false", "1.0.0"), None);
    }

    #[test]
    fn github_release_version_extracts_semver_and_rejects_calendar_or_errors() {
        for (payload, expected) in [
            (
                serde_json::json!({ "name": "Hermes Agent v0.20.4 (2026.8.18)", "tag_name": "v2026.8.18" }),
                Some("0.20.4"),
            ),
            (
                serde_json::json!({ "name": "Hermes Agent v0.21.0 (v2026.8.31)", "tag_name": "v2026.8.31" }),
                Some("0.21.0"),
            ),
            (
                serde_json::json!({ "name": "August refresh", "tag_name": "v1.18.18" }),
                Some("1.18.18"),
            ),
            (serde_json::json!({ "tag_name": "v1.2.3" }), Some("1.2.3")),
            (
                serde_json::json!({ "name": "Hermes Agent (2026.8.31)", "tag_name": "v2026.8.31" }),
                None,
            ),
            (serde_json::json!({ "tag_name": "v2026.8.16.2" }), None),
            (
                serde_json::json!({ "message": "API rate limit exceeded for 1.2.3.4." }),
                None,
            ),
            (serde_json::json!({}), None),
        ] {
            assert_eq!(
                github_release_version_from_json(&payload).as_deref(),
                expected,
                "{payload}"
            );
        }
    }

    #[test]
    fn drop_latest_behind_local_hides_stale_fallback() {
        let pypi = || Some("0.19.0".to_string());
        assert_eq!(drop_latest_behind_local(pypi(), Some("0.21.0")), None);
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
        assert_eq!(
            drop_latest_behind_local(pypi(), Some("unknown")).as_deref(),
            Some("0.19.0")
        );
    }

    #[test]
    fn pick_latest_version_respects_prerelease_channels_and_filters_dirty_tags() {
        let tags = serde_json::json!({
            "latest": "2.1.154",
            "next": "2.1.156",
            "stable": "2.1.145"
        });
        let map = tags.as_object().unwrap();
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.156")).as_deref(),
            Some("2.1.156")
        );
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.154")).as_deref(),
            Some("2.1.154")
        );
        assert_eq!(
            pick_latest_version(map, &["next"], Some("2.1.145")).as_deref(),
            Some("2.1.154")
        );
        assert_eq!(
            pick_latest_version(map, &[], Some("2.1.156")).as_deref(),
            Some("2.1.154")
        );
        assert_eq!(
            pick_latest_version(map, &["next"], None).as_deref(),
            Some("2.1.154")
        );

        let dirty = serde_json::json!({ "latest": "0.135.0", "beta": "0.1.2505172116" });
        assert_eq!(
            pick_latest_version(dirty.as_object().unwrap(), &["beta"], Some("0.200.0")).as_deref(),
            Some("0.135.0")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn scan_cli_version_reports_present_but_silent_executable_as_broken() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temp dir should be created");
        let make_dir = |name: &str, script: Option<&str>| {
            let dir = root.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let tool = dir.join("kimi");
            match script {
                Some(body) => {
                    std::fs::write(&tool, format!("#!/bin/sh\n{body}\n")).unwrap();
                    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755))
                        .unwrap();
                }
                None => std::fs::write(&tool, "").unwrap(),
            }
            dir
        };
        let silent = make_dir("silent", Some("exit 0"));
        let silent_failure = make_dir("silent-failure", Some("exit 3"));
        let not_executable = make_dir("not-executable", None);
        let loud_failure = make_dir("loud-failure", Some("echo boom >&2\nexit 1"));
        let working = make_dir("working", Some("echo 'kimi 1.2.3'"));
        let empty = root.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let same_name_dir = root.path().join("same-name-dir");
        std::fs::create_dir_all(same_name_dir.join("kimi")).unwrap();

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
        assert_eq!(broken_detail(&[silent.clone(), loud_failure]), "boom");
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
