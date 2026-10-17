//! Uninstall the detected installation, retaining configuration and user data.
//! Never fall back to a different package manager or delete an arbitrary directory.

use super::*;

/// Every detected installation with its own plan. The user picks one; the others stay.
pub(super) fn candidates(
    tool: &str,
    pref: Option<&WslShellPreferenceInput>,
) -> Result<Vec<ToolUninstallCandidate>, String> {
    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_for_tool(tool) {
        let plan = wsl_plan(tool, &distro, pref)?;
        let install = ToolInstallation {
            path: plan.path.clone(),
            real: PathBuf::from(&plan.real_path),
            version: None,
            runnable: true,
            error: None,
            exited_with_error: false,
            source: "wsl".into(),
            is_path_default: true,
        };
        return Ok(vec![ToolUninstallCandidate {
            install,
            plan: Some(plan),
            unavailable_reason: None,
            reinstalled_by_updater: false,
        }]);
    }
    let _ = pref;
    let installs = enumerate_tool_installations(tool);
    if installs.is_empty() {
        return Err(format!(
            "Cannot identify the installation of {} to uninstall. Check installations and try again.",
            tool_display_name(tool)
        ));
    }
    let has_grok_native = tool == "grok" && installs.iter().any(|i| known_native_install(tool, i));
    let grok_npm_mode = has_grok_native && grok_updater_uses_npm();
    Ok(installs
        .into_iter()
        .map(|install| {
            let (plan, unavailable_reason) = match plan_for_install(tool, &install) {
                Ok(plan) => (Some(plan), None),
                Err(reason) => (None, Some(reason)),
            };
            // `grok update` in npm mode reinstalls the global package next to the native one.
            let reinstalled_by_updater = grok_npm_mode
                && plan.as_ref().is_some_and(|plan| !plan.native)
                && install
                    .real
                    .to_string_lossy()
                    .replace('\\', "/")
                    .contains("/node_modules/");
            ToolUninstallCandidate {
                install,
                plan,
                unavailable_reason,
                reinstalled_by_updater,
            }
        })
        .collect())
}

fn grok_updater_uses_npm() -> bool {
    std::fs::read_to_string(crate::grok_config::get_grok_config_path())
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .is_some_and(|config| {
            config
                .get("cli")
                .and_then(|cli| cli.get("installer"))
                .and_then(toml::Value::as_str)
                == Some("npm")
        })
}

fn plan_for_install(tool: &str, inst: &ToolInstallation) -> Result<ToolUninstallPlan, String> {
    plan_and_removals(tool, inst).map(|(plan, _)| plan)
}

/// The plan plus, for native installs, the exact files it removes (the plan displays them).
fn plan_and_removals(
    tool: &str,
    inst: &ToolInstallation,
) -> Result<(ToolUninstallPlan, Vec<PathBuf>), String> {
    let mut removals = Vec::new();
    let (command, native) = if let Some(command) = package_uninstall_command(tool, inst)? {
        (command, false)
    } else if known_native_install(tool, inst) {
        removals = native_removal_paths(tool, inst);
        let paths: Vec<String> = removals
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        // Display the equivalent file operation; execution uses remove_file, never a shell.
        #[cfg(target_os = "windows")]
        let command = format!(
            "Remove-Item -LiteralPath {}",
            paths
                .iter()
                .map(|p| format!("'{}'", p.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ")
        );
        #[cfg(not(target_os = "windows"))]
        let command = format!(
            "rm -- {}",
            paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ")
        );
        (command, true)
    } else {
        return Err(format!(
            "Cannot automatically uninstall {} at {}. Use the original installer or package manager to uninstall it.",
            tool_display_name(tool), inst.path
        ));
    };
    let command = if tool == "openclaw" {
        // The official CLI removes only the Gateway service with this scope.
        // Stop on failure; config, sessions and workspaces must remain untouched.
        #[cfg(target_os = "windows")]
        let next = "call ";
        #[cfg(not(target_os = "windows"))]
        let next = "";
        format!(
            "{} uninstall --service --yes --non-interactive && {next}{command}",
            quote(&inst.path)
        )
    } else {
        command
    };
    let plan = ToolUninstallPlan {
        tool: tool.into(),
        path: inst.path.clone(),
        real_path: inst.real.to_string_lossy().into_owned(),
        command,
        native,
        wsl_distro: None,
    };
    Ok((plan, removals))
}

pub(super) fn uninstall(
    tool: &str,
    pref: Option<&WslShellPreferenceInput>,
    confirmed: &ToolUninstallPlan,
) -> Result<(), String> {
    // Never execute a command supplied by the frontend or silently switch installations
    // while the dialog is open / this task is queued behind another lifecycle operation.
    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_for_tool(tool) {
        // Run the very process the re-derived plan describes (same shell and flag).
        let (current, mut process) = wsl_plan_and_process(tool, &distro, pref)?;
        validate_confirmed_plan(&current, confirmed)?;
        let output = process
            .output()
            .map_err(|e| format!("Cannot start WSL uninstall: {e}"))?;
        return finish_lifecycle_output(&output);
    }
    let _ = pref;
    // Re-derive the plan for the installation the user picked, not whichever is the default now.
    let installs = enumerate_tool_installations(tool);
    let inst = installs
        .iter()
        .find(|inst| inst.path == confirmed.path)
        .ok_or(
        "Installation changed since the uninstall preview. Check installations and confirm again.",
    )?;
    let (current, removals) = plan_and_removals(tool, inst)?;
    validate_confirmed_plan(&current, confirmed)?;
    if current.native {
        remove_native_files(inst, &removals)?;
    } else {
        run_package_uninstall(&current.command)?;
    }
    // Some managers return zero even if the selected package was not removed.
    verify_removed(tool, &current.path)
}

fn run_package_uninstall(command: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let batch = format!("@chcp 65001>nul\r\n@echo off\r\ncall {command}\r\nexit /b %errorlevel%");
    #[cfg(target_os = "windows")]
    let command = batch.as_str();
    run_tool_lifecycle_silently(command, "tool_uninstall")
}

fn validate_confirmed_plan(
    current: &ToolUninstallPlan,
    confirmed: &ToolUninstallPlan,
) -> Result<(), String> {
    if current != confirmed {
        return Err("Installation changed since the uninstall preview. Check installations and confirm again.".into());
    }
    Ok(())
}

fn verify_removed(tool: &str, path: &str) -> Result<(), String> {
    // exists() follows symlinks and would report a dangling manager shim as removed.
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Cannot verify removal of {path}: {error}")),
        Ok(_) => Err(format!(
            "{} is still installed at {}. Use the original installer or package manager to uninstall it.",
            tool_display_name(tool), path
        )),
    }
}

fn brew_uninstall_command(tool: &str, real: &str) -> Result<Option<String>, String> {
    for (marker, flag) in [("/Caskroom/", "--cask"), ("/Cellar/", "--formula")] {
        if let Some((prefix, rest)) = real.split_once(marker) {
            let token = rest.split('/').next().unwrap_or_default();
            let owners: &[(&str, &str)] = match tool {
                "claude" => &[
                    ("/Caskroom/", "claude-code"),
                    ("/Caskroom/", "claude-code@latest"),
                ],
                "codex" => &[("/Caskroom/", "codex")],
                "gemini" => &[("/Cellar/", "gemini-cli")],
                "opencode" => &[("/Cellar/", "opencode")],
                _ => &[],
            };
            if !owners.contains(&(marker, token)) {
                return Err(format!("Cannot automatically uninstall {}: Homebrew package {token} is not a verified owner. Use the original installer or package manager.", tool_display_name(tool)));
            }
            let brew = format!("{prefix}/bin/brew");
            if !Path::new(&brew).is_file() {
                return Err(format!("Cannot find the Homebrew executable at {brew}. Use the original package manager to uninstall this application."));
            }
            return Ok(Some(format!(
                "{} uninstall {flag} {}",
                quote(&brew),
                quote(token)
            )));
        }
    }
    Ok(None)
}

fn package_uninstall_command(
    tool: &str,
    inst: &ToolInstallation,
) -> Result<Option<String>, String> {
    // Check Homebrew ownership before npm: formulae can contain lib/node_modules.
    if let Some(command) =
        brew_uninstall_command(tool, &inst.real.to_string_lossy().replace('\\', "/"))?
    {
        return Ok(Some(command));
    }
    if tool == "hermes" {
        // The official installer uses a uv venv, often without pip. Let the selected
        // CLI remove its own installation; omitting --full retains config and data.
        return Ok(Some(format!("{} uninstall --yes", quote(&inst.path))));
    }
    Ok(node_package_uninstall_command(tool, inst))
}

fn quote(value: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        win_quote_path_for_batch(value)
    }
    #[cfg(not(target_os = "windows"))]
    {
        shell_single_quote(value)
    }
}

fn manager_next_to(entry: &str, manager: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        sibling_bin_with_ext(entry, manager, &["cmd", "exe"])
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path = sibling_bin(entry, manager)?;
        Path::new(&path).is_file().then_some(path)
    }
}

/// Read package metadata instead of assuming every script or native binary is npm-owned.
/// The explicit prefix also supports user npm prefixes without a sibling npm executable.
fn npm_prefix(tool: &str, inst: &ToolInstallation) -> Option<PathBuf> {
    npm_prefix_for_layout(tool, inst, cfg!(target_os = "windows"))
}

fn npm_prefix_for_layout(tool: &str, inst: &ToolInstallation, windows: bool) -> Option<PathBuf> {
    let package = npm_package_for(tool)?;
    let launcher = Path::new(&inst.path);
    let bin = launcher.parent()?;
    // A local node_modules/.bin entry is never a global npm launcher. Pi's
    // managed releases use precisely that layout, even though they contain npm metadata.
    let prefix = if windows {
        if bin.file_name().is_some_and(|name| name == ".bin") {
            return None;
        }
        bin
    } else {
        if bin.file_name()? != "bin" {
            return None;
        }
        bin.parent()?
    };
    let modules = prefix.join(if windows {
        "node_modules"
    } else {
        "lib/node_modules"
    });
    let package_dir = std::fs::canonicalize(modules.join(package)).ok()?;
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(package_dir.join("package.json")).ok()?).ok()?;
    if value.get("name").and_then(|name| name.as_str()) != Some(package) {
        return None;
    }
    let owns_target = inst.real.starts_with(&package_dir);
    // Windows npm shims are files, not symlinks. Their package reference is
    // required as well as the manifest and the global launcher layout.
    let owns_shim = windows
        && !is_native_executable(launcher)
        && std::fs::read_to_string(launcher).is_ok_and(|script| {
            script
                .replace('\\', "/")
                .contains(&format!("node_modules/{package}/"))
        });
    (owns_target || owns_shim).then(|| prefix.to_path_buf())
}

fn node_package_uninstall_command(tool: &str, inst: &ToolInstallation) -> Option<String> {
    let package = npm_package_for(tool)?;
    // A source badge or a sibling manager is not proof of package ownership.
    // pnpm/Bun commands remain available manually for the clients that document
    // them; until their global roots are verified, do not guess or fall back to npm.
    // Homebrew/mise can also supply Node itself: after excluding their own
    // formulae and shims, an independently verified npm layout is still valid.
    if !matches!(
        inst.source.as_str(),
        "system" | "nvm" | "fnm" | "homebrew" | "mise"
    ) {
        return None;
    }
    if tool == "mcode"
        && (mcode_receipt_install_root(&inst.path, McodeLayout::NATIVE).is_some()
            || mcode_script_install_root(
                &inst.path,
                &inst.real.to_string_lossy(),
                McodeLayout::NATIVE,
            )
            .is_some())
    {
        return None;
    }
    let prefix = npm_prefix(tool, inst)?;
    let npm = manager_next_to(&inst.path, "npm").or_else(|| {
        resolve_path_default(
            "npm",
            CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
        )
        .ok()
        .flatten()
        .filter(|path| path.is_file())
        .map(|path| {
            // PATH lookup canonicalizes Windows paths; batch `call` rejects verbatim paths.
            #[cfg(target_os = "windows")]
            let path = windows_shell_compatible_path(&path);
            path.to_string_lossy().into_owned()
        })
    })?;
    let command = format!(
        "{} uninstall -g --prefix {} {package}",
        quote(&npm),
        quote(&prefix.to_string_lossy())
    );
    #[cfg(not(target_os = "windows"))]
    let command = if Path::new(&npm).is_absolute() {
        format!("PATH={}:\"$PATH\" {command}", quote(&parent_dir(&npm)))
    } else {
        command
    };
    Some(command)
}

fn known_native_install(tool: &str, inst: &ToolInstallation) -> bool {
    if inst.source != "system" {
        return false;
    }
    let real = inst.real.to_string_lossy();
    let normalized = real.replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("/node_modules/") || !is_native_executable(&inst.real) {
        return false;
    }
    match tool {
        "claude" => {
            normalized
                .split_once("/.local/share/claude/versions/")
                .is_some_and(|(_, version)| !version.is_empty() && !version.contains('/'))
                || normalized.ends_with("/.local/bin/claude")
                || normalized.ends_with("/.local/bin/claude.exe")
        }
        "codex" => normalized
            .split_once("/packages/standalone/releases/")
            .and_then(|(_, release)| release.split_once('/'))
            .is_some_and(|(version, binary)| {
                !version.is_empty()
                    && matches!(
                        binary,
                        "bin/codex" | "bin/codex.exe" | "codex" | "codex.exe"
                    )
            }),
        "grok" => grok_native_root(&inst.real).is_some(),
        // The current curl installer (including Git Bash on Windows) uses this
        // directory. Do not invent OPENCODE_INSTALL_DIR / XDG_BIN_DIR layouts.
        "opencode" => {
            normalized.ends_with("/.opencode/bin/opencode")
                || normalized.ends_with("/.opencode/bin/opencode.exe")
        }
        _ => false,
    }
}

const GROK_PLATFORMS: [&str; 6] = [
    "macos-aarch64",
    "macos-x86_64",
    "linux-aarch64",
    "linux-x86_64",
    "windows-aarch64.exe",
    "windows-x86_64.exe",
];

/// Names the official Grok installer writes: `bin/grok`, `bin/grok-<version>` (0.2.112+),
/// `downloads/grok-<platform>` and `downloads/grok-<version>-<platform>`.
fn is_grok_binary_name(dir: &str, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let Some(rest) = name.strip_prefix("grok-") else {
        return dir == "bin" && matches!(name.as_str(), "grok" | "grok.exe");
    };
    match dir {
        "bin" => is_grok_version(rest.strip_suffix(".exe").unwrap_or(rest)),
        "downloads" => GROK_PLATFORMS.iter().any(|platform| {
            rest == *platform
                || rest
                    .strip_suffix(platform)
                    .and_then(|version| version.strip_suffix('-'))
                    .is_some_and(is_grok_version)
        }),
        _ => false,
    }
}

fn is_grok_version(version: &str) -> bool {
    version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// The `.grok` directory owning an installer binary, or None for any other file.
fn grok_native_root(real: &Path) -> Option<PathBuf> {
    let name = real.file_name()?.to_str()?;
    let dir = real.parent()?;
    let kind = dir.file_name()?.to_str()?.to_ascii_lowercase();
    let root = dir.parent()?;
    (root.file_name()?.eq_ignore_ascii_case(".grok") && is_grok_binary_name(&kind, name))
        .then(|| root.to_path_buf())
}

/// The Grok installer keeps every version it fetched plus `grok` / `agent` links to one of
/// them. Removing only the current binary would leave the others and a working `agent`.
fn grok_installer_files(root: &Path) -> Vec<PathBuf> {
    let mut launchers = Vec::new();
    let mut binaries = Vec::new();
    for kind in ["bin", "downloads"] {
        let Ok(entries) = std::fs::read_dir(root.join(kind)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if metadata.file_type().is_symlink() {
                // Only installer launchers that still point at one of its binaries.
                let launcher = kind == "bin"
                    && matches!(name.as_str(), "grok" | "grok.exe" | "agent" | "agent.exe");
                if launcher
                    && std::fs::canonicalize(&path)
                        .is_ok_and(|target| grok_native_root(&target).as_deref() == Some(root))
                {
                    launchers.push(path);
                }
            } else if metadata.is_file()
                && is_grok_binary_name(kind, &name)
                && is_native_executable(&path)
            {
                binaries.push(path);
            }
        }
    }
    launchers.sort();
    binaries.sort();
    launchers.extend(binaries);
    launchers
}

/// Files removed for a native install: the selected launcher, other launcher symlinks to
/// the same binary in the search directories, the binary, and for Grok the rest of the
/// installer's binaries. Launchers first, so a failure never leaves a dangling command.
fn native_removal_paths(tool: &str, inst: &ToolInstallation) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::from(&inst.path)];
    for dir in build_tool_search_paths(tool) {
        for launcher in tool_executable_candidates(tool, &dir) {
            let is_symlink = std::fs::symlink_metadata(&launcher)
                .is_ok_and(|metadata| metadata.file_type().is_symlink());
            if is_symlink && std::fs::canonicalize(&launcher).is_ok_and(|real| real == inst.real) {
                paths.push(launcher);
            }
        }
    }
    paths.push(inst.real.clone());
    if tool == "grok" {
        if let Some(root) = grok_native_root(&inst.real) {
            paths.extend(grok_installer_files(&root));
        }
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

/// Remove only the listed files. No recursive deletion:
/// native installers may keep settings, sessions and projects next to their binaries.
fn remove_native_files(inst: &ToolInstallation, paths: &[PathBuf]) -> Result<(), String> {
    let entry = Path::new(&inst.path);
    // Validate before changing anything. A replaced symlink must not delete a new target.
    if std::fs::canonicalize(entry).map_err(|e| e.to_string())? != inst.real {
        return Err(
            "Installation changed during uninstall. Check installations and try again.".into(),
        );
    }
    for path in paths {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
                std::fs::remove_file(path)
                    .map_err(|e| format!("Cannot remove {}: {e}", path.display()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(format!("Refusing to remove directory {}", path.display())),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn wsl_plan(
    tool: &str,
    distro: &str,
    pref: Option<&WslShellPreferenceInput>,
) -> Result<ToolUninstallPlan, String> {
    wsl_plan_and_process(tool, distro, pref).map(|(plan, _)| plan)
}

/// The plan plus the exact process it describes. Without an explicit shell, the probe
/// tries the version check's flags in order and the removal reuses the winning flag.
#[cfg(target_os = "windows")]
fn wsl_plan_and_process(
    tool: &str,
    distro: &str,
    pref: Option<&WslShellPreferenceInput>,
) -> Result<(ToolUninstallPlan, std::process::Command), String> {
    let shell = WslShell::from_pref(pref)?;
    let probe = wsl_probe_script(tool);
    let child = wsl_process(distro, &shell.probe_args(&probe))?
        .spawn()
        .map_err(|e| format!("Cannot inspect WSL installation: {e}"))?;
    let output = wait_child_output(
        child,
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )?;
    finish_lifecycle_output(&output)?;
    let stdout = decode_command_output(&output.stdout);
    let (path, real_path) = parse_wsl_installation(&stdout)?;
    let auto_flag = match shell {
        WslShell::Explicit { .. } => None,
        WslShell::Auto { .. } => Some(parse_wsl_shell_flag(&stdout)?),
    };
    let script = wsl_uninstall_command(tool, &path, &real_path)?;
    let process = wsl_process(distro, &shell.run_args(&script, auto_flag))?;
    let command = format!(
        "wsl.exe {}",
        process
            .get_args()
            .map(|arg| windows_cmd_double_quote_arg(&arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let plan = ToolUninstallPlan {
        tool: tool.into(),
        path,
        real_path,
        command,
        native: false,
        wsl_distro: Some(distro.into()),
    };
    Ok((plan, process))
}

#[cfg(any(target_os = "windows", test))]
fn wsl_probe_script(tool: &str) -> String {
    format!(
        "sh -c {}",
        shell_single_quote(&format!(
            "entry=$(command -v {tool}) && real=$(readlink -f -- \"$entry\") && [ -f \"$real\" ] && printf '__CC_SWITCH_UNINSTALL__\\n%s\\n%s\\n' \"$entry\" \"$real\""
        ))
    )
}

#[cfg(any(target_os = "windows", test))]
const WSL_SHELL_FLAG_MARKER: &str = "__CC_SWITCH_SHELL_FLAG__ ";

/// Auto mode probe, run by an outer `sh -c`: `$1` is the probe, the remaining
/// arguments are flags. Like the version check, try each flag in `$SHELL` until
/// one succeeds (stderr kept only for the last), then report which flag won.
#[cfg(any(target_os = "windows", test))]
const WSL_SHELL_PROBE_LAUNCHER: &str = r#"s=${SHELL:-sh}; script=$1; shift; while [ $# -gt 1 ]; do out=$("$s" "$1" "$script" 2>/dev/null) && { printf '__CC_SWITCH_SHELL_FLAG__ %s\n%s\n' "$1" "$out"; exit 0; }; shift; done; out=$("$s" "$1" "$script") && printf '__CC_SWITCH_SHELL_FLAG__ %s\n%s\n' "$1" "$out""#;

/// Auto mode removal: run `$1` in `$SHELL` with exactly the probe's flag `$2`.
/// Never retry with another flag; a different environment could reach another copy.
#[cfg(any(target_os = "windows", test))]
const WSL_SHELL_RUN_LAUNCHER: &str = r#"s=${SHELL:-sh}; exec "$s" "$2" "$1""#;

#[cfg(any(target_os = "windows", test))]
fn parse_wsl_shell_flag(stdout: &str) -> Result<&'static str, String> {
    let flag = stdout
        .rsplit_once(WSL_SHELL_FLAG_MARKER)
        .and_then(|(_, rest)| rest.lines().next())
        .map(str::trim);
    WSL_AUTO_SHELL_FLAGS
        .into_iter()
        .find(|known| Some(*known) == flag)
        .ok_or_else(|| {
            "Cannot identify the WSL shell. Use the original installer to uninstall it.".into()
        })
}

/// The shell WSL commands run in: the user's preference, or the version check's
/// auto mode (`$SHELL` with `WSL_AUTO_SHELL_FLAGS`, or one preferred flag).
#[cfg(target_os = "windows")]
enum WslShell<'a> {
    Explicit { shell: &'a str, flag: &'a str },
    Auto { flag: Option<&'a str> },
}

#[cfg(target_os = "windows")]
impl<'a> WslShell<'a> {
    fn from_pref(pref: Option<&'a WslShellPreferenceInput>) -> Result<Self, String> {
        let flag = pref.and_then(|p| p.wsl_shell_flag.as_deref());
        let shell = match pref.and_then(|p| p.wsl_shell.as_deref()) {
            Some(shell) => {
                let shell = shell.rsplit('/').next().unwrap_or(shell);
                if !is_valid_shell(shell) {
                    return Err(format!("Invalid WSL shell: {shell}"));
                }
                WslShell::Explicit {
                    shell,
                    flag: flag.unwrap_or_else(|| default_flag_for_shell(shell)),
                }
            }
            None => WslShell::Auto { flag },
        };
        if let WslShell::Explicit { flag, .. } | WslShell::Auto { flag: Some(flag) } = &shell {
            if !is_valid_shell_flag(flag) {
                return Err(format!("Invalid WSL shell flag: {flag}"));
            }
        }
        Ok(shell)
    }

    fn probe_args<'s>(&'s self, script: &'s str) -> Vec<&'s str> {
        match self {
            WslShell::Explicit { shell, flag } => vec![*shell, *flag, script],
            WslShell::Auto { flag } => {
                let mut args = vec!["sh", "-c", WSL_SHELL_PROBE_LAUNCHER, "cc-switch", script];
                match flag {
                    Some(flag) => args.push(*flag),
                    None => args.extend(WSL_AUTO_SHELL_FLAGS),
                }
                args
            }
        }
    }

    fn run_args<'s>(&'s self, script: &'s str, auto_flag: Option<&'static str>) -> Vec<&'s str> {
        match self {
            WslShell::Explicit { shell, flag } => vec![*shell, *flag, script],
            WslShell::Auto { .. } => vec![
                "sh",
                "-c",
                WSL_SHELL_RUN_LAUNCHER,
                "cc-switch",
                script,
                auto_flag.unwrap_or("-c"),
            ],
        }
    }
}

#[cfg(target_os = "windows")]
fn wsl_process(distro: &str, shell_args: &[&str]) -> Result<std::process::Command, String> {
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: {distro}"));
    }
    // Pass an argv directly and use --exec: cmd.exe / WSL's implicit shell must
    // not expand $entry or command substitutions before the chosen shell runs.
    let mut process = std::process::Command::new("wsl.exe");
    process
        .args(["-d", distro, "--exec"])
        .args(shell_args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    Ok(process)
}

#[cfg(any(target_os = "windows", test))]
fn parse_wsl_installation(stdout: &str) -> Result<(String, String), String> {
    let mut lines = stdout
        .rsplit_once("__CC_SWITCH_UNINSTALL__\n")
        .map(|(_, paths)| paths.lines())
        .ok_or(
            "Cannot identify the WSL installation. Use the original installer to uninstall it.",
        )?;
    let path = lines.next().unwrap_or_default();
    let real = lines.next().unwrap_or_default();
    if !path.starts_with('/') || !real.starts_with('/') || lines.next().is_some() {
        return Err("Cannot identify the WSL installation paths. Use the original installer to uninstall it.".into());
    }
    Ok((path.into(), real.into()))
}

#[cfg(any(target_os = "windows", test))]
fn wsl_uninstall_command(tool: &str, path: &str, real: &str) -> Result<String, String> {
    if real.contains("/Cellar/") || real.contains("/Caskroom/") {
        // WSL support is limited to verified npm prefixes and Hermes' own uninstaller.
        // A formula's libexec/node_modules must not fall through to npm.
        return Err("Use Homebrew to uninstall this WSL application.".into());
    }
    let removal = if tool == "hermes" {
        format!("{} uninstall --yes", shell_single_quote(path))
    } else {
        let package = npm_package_for(tool)
            .ok_or("Use the original installer to uninstall this WSL application.")?;
        let (prefix, _) = real
            .split_once(&format!("/lib/node_modules/{package}/"))
            .filter(|(prefix, _)| !prefix.is_empty() && path == format!("{prefix}/bin/{tool}"))
            .ok_or(
                "Use the original installer or package manager to uninstall this WSL application.",
            )?;
        format!(
            "npm uninstall -g --prefix {} {package}",
            shell_single_quote(prefix)
        )
    };
    let removal = if tool == "openclaw" {
        format!(
            "{} uninstall --service --yes --non-interactive && {removal}",
            shell_single_quote(path)
        )
    } else {
        removal
    };
    // The outer shell may be fish. All POSIX syntax stays inside an explicit sh.
    // Recheck the target just before running; do not accept a dangling shim as success.
    let script = format!(
        "entry={} && [ \"$(command -v {tool})\" = \"$entry\" ] && [ \"$(readlink -f -- \"$entry\")\" = {} ] && {removal} && {{ {{ [ ! -e \"$entry\" ] && [ ! -L \"$entry\" ]; }} || {{ echo 'Application is still installed' >&2; exit 1; }}; }}",
        shell_single_quote(path), shell_single_quote(real)
    );
    Ok(format!("sh -c {}", shell_single_quote(&script)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installation(path: &Path) -> ToolInstallation {
        ToolInstallation {
            path: path.to_string_lossy().into_owned(),
            real: std::fs::canonicalize(path).unwrap(),
            version: None,
            runnable: false,
            error: None,
            exited_with_error: false,
            source: "system".into(),
            is_path_default: true,
        }
    }

    fn write_fixture_npm(entry: &Path) -> PathBuf {
        let npm = entry
            .parent()
            .unwrap()
            .join(if cfg!(target_os = "windows") {
                "npm.cmd"
            } else {
                "npm"
            });
        std::fs::write(&npm, "fixture manager").unwrap();
        npm
    }

    #[test]
    fn npm_uninstall_uses_verified_package_and_exact_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("custom npm prefix");
        #[cfg(target_os = "windows")]
        let modules = prefix.join("node_modules");
        #[cfg(not(target_os = "windows"))]
        let modules = prefix.join("lib/node_modules");
        let package = modules.join("@openai/codex");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"@openai/codex"}"#).unwrap();
        let executable = package.join("codex.js");
        std::fs::write(&executable, "launcher").unwrap();
        #[cfg(target_os = "windows")]
        let entry = prefix.join("codex.cmd");
        #[cfg(not(target_os = "windows"))]
        let entry = prefix.join("bin/codex");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(
            &entry,
            r#"node "%dp0%\node_modules\@openai\codex\codex.js""#,
        )
        .unwrap();
        let inst = ToolInstallation {
            real: if cfg!(target_os = "windows") {
                std::fs::canonicalize(&entry).unwrap()
            } else {
                std::fs::canonicalize(&executable).unwrap()
            },
            ..installation(&entry)
        };
        write_fixture_npm(&entry);
        let command = package_uninstall_command("codex", &inst).unwrap().unwrap();
        assert!(command.contains("uninstall -g --prefix"));
        assert!(command.contains(&quote(&prefix.to_string_lossy())));
        assert!(command.ends_with("@openai/codex"));
        for source in ["homebrew", "mise", "nvm", "fnm"] {
            let managed_node = ToolInstallation {
                source: source.into(),
                real: inst.real.clone(),
                ..installation(&entry)
            };
            assert_eq!(
                package_uninstall_command("codex", &managed_node)
                    .unwrap()
                    .unwrap(),
                command
            );
        }
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"another-package"}"#,
        )
        .unwrap();
        assert!(package_uninstall_command("codex", &inst).unwrap().is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_npm_path_fallback_uses_batch_compatible_path() {
        use std::os::windows::process::CommandExt;
        use std::time::Duration;

        const TEST_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
        const FIXTURE_ROOT: &str = "CC_SWITCH_TEST_NPM_UNINSTALL_ROOT";
        if let Some(root) = std::env::var_os(FIXTURE_ROOT) {
            let root = PathBuf::from(root);
            let prefix = root.join("custom npm prefix");
            let entry = prefix.join("codex.cmd");
            let npm = root.join("Program Files").join("nodejs").join("npm.cmd");
            let marker = npm.with_file_name("npm-args.txt");
            let inst = installation(&entry);
            assert!(manager_next_to(&inst.path, "npm").is_none());

            let resolved = resolve_path_default(
                "npm",
                CommandDeadline::from_timeout(Some(TEST_PROBE_TIMEOUT)),
            )
            .unwrap()
            .expect("npm should resolve from the separate PATH directory");
            assert_eq!(resolved, std::fs::canonicalize(&npm).unwrap());
            assert!(resolved.to_string_lossy().starts_with(r"\\?\"));

            let plan = plan_for_install("codex", &inst).unwrap();
            let arguments = format!(
                "uninstall -g --prefix {} @openai/codex",
                quote(&prefix.to_string_lossy())
            );
            // PATH lookup expands 8.3 aliases (e.g. RUNNER~1) before removing the
            // verbatim prefix, so the batch path may differ from the fixture spelling.
            let npm_for_batch = windows_shell_compatible_path(&resolved);
            assert_eq!(
                plan.command,
                format!("{} {arguments}", quote(&npm_for_batch.to_string_lossy()))
            );
            assert!(!plan.native);
            assert!(!plan.command.contains(r"\\?\"));
            assert!(!marker.exists(), "planning must not execute npm");

            run_package_uninstall(&plan.command)
                .expect("PATH npm.cmd should execute via batch call");
            assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), arguments);
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("custom npm prefix");
        let package = prefix.join("node_modules/@openai/codex");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"@openai/codex"}"#).unwrap();
        std::fs::write(
            prefix.join("codex.cmd"),
            "@echo off\r\nrem node_modules/@openai/codex/codex.js\r\n",
        )
        .unwrap();
        let node_dir = temp.path().join("Program Files").join("nodejs");
        std::fs::create_dir_all(&node_dir).unwrap();
        std::fs::write(
            node_dir.join("npm.cmd"),
            "@echo off\r\n>\"%~dp0npm-args.txt\" echo %*\r\nexit /b 0\r\n",
        )
        .unwrap();

        // Isolate PATH in a child test process so parallel tests keep their environment.
        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(node_dir.clone()).chain(std::env::split_paths(&inherited_path)),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::misc::tool_uninstall::tests::windows_npm_path_fallback_uses_batch_compatible_path",
                "--nocapture",
            ])
            .env(FIXTURE_ROOT, temp.path())
            .env("PATH", path)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "PATH fallback regression failed:\n{}\n{}",
            decode_command_output(&output.stdout),
            decode_command_output(&output.stderr)
        );
        assert!(node_dir.join("npm-args.txt").is_file());
    }

    #[test]
    fn unknown_executable_is_not_removed_or_assumed_to_be_npm() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("codex.exe");
        std::fs::write(&executable, "MZ").unwrap();
        let inst = installation(&executable);
        assert!(package_uninstall_command("codex", &inst).unwrap().is_none());
        assert!(!known_native_install("codex", &inst));
        assert!(executable.exists());
    }

    #[test]
    fn nearby_npm_package_does_not_claim_an_unrelated_executable() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("node_modules/@openai/codex");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"@openai/codex"}"#).unwrap();
        let executable = temp.path().join("codex.cmd");
        std::fs::write(&executable, "unrelated launcher").unwrap();
        assert!(npm_prefix("codex", &installation(&executable)).is_none());
        std::fs::write(
            &executable,
            r#"node "%dp0%\node_modules\@openai\codex\bin\codex.js""#,
        )
        .unwrap();
        assert_eq!(
            npm_prefix_for_layout("codex", &installation(&executable), true),
            Some(temp.path().to_path_buf())
        );
    }

    #[test]
    fn npm_layout_rejects_local_dependencies_and_installer_launchers() {
        for windows in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let release = temp.path().join(".pi/agent/install/releases/1.0");
            let package = release.join("node_modules/@earendil-works/pi-coding-agent");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(
                package.join("package.json"),
                r#"{"name":"@earendil-works/pi-coding-agent"}"#,
            )
            .unwrap();
            let executable = package.join("cli.js");
            std::fs::write(&executable, "launcher").unwrap();
            let local_bin = release.join("node_modules/.bin/pi");
            std::fs::create_dir_all(local_bin.parent().unwrap()).unwrap();
            std::fs::write(&local_bin, "launcher").unwrap();
            let inst = ToolInstallation {
                real: std::fs::canonicalize(&executable).unwrap(),
                ..installation(&local_bin)
            };
            assert!(npm_prefix_for_layout("pi", &inst, windows).is_none());
            // Even a direct package target is not a global installation.
            assert!(npm_prefix_for_layout("pi", &installation(&executable), windows).is_none());

            let bin = temp.path().join(".local/bin/pi");
            std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
            std::fs::write(&bin, "#!/bin/sh\nexec release/node_modules/.bin/pi").unwrap();
            assert!(npm_prefix_for_layout("pi", &installation(&bin), windows).is_none());
        }
    }

    #[test]
    fn posix_npm_layout_requires_lib_modules_and_a_matching_global_bin() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("custom prefix");
        let package = prefix.join("lib/node_modules/@openai/codex");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"@openai/codex"}"#).unwrap();
        let executable = package.join("codex.js");
        std::fs::write(&executable, "launcher").unwrap();
        let inst = ToolInstallation {
            path: prefix.join("bin/codex").to_string_lossy().into_owned(),
            ..installation(&executable)
        };
        assert_eq!(npm_prefix_for_layout("codex", &inst, false), Some(prefix));
        let unrelated = ToolInstallation {
            path: temp.path().join("bin/codex").to_string_lossy().into_owned(),
            ..inst
        };
        assert!(npm_prefix_for_layout("codex", &unrelated, false).is_none());
    }

    #[test]
    fn source_badges_and_sibling_managers_do_not_prove_package_ownership() {
        for (tool, manager) in [
            ("codex", "volta"),
            ("opencode", "pnpm"),
            ("openclaw", "bun"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let entry = temp.path().join(format!("{tool}.cmd"));
            std::fs::write(&entry, "unrelated launcher").unwrap();
            for suffix in ["", ".cmd", ".exe"] {
                std::fs::write(temp.path().join(format!("{manager}{suffix}")), "manager").unwrap();
            }
            let inst = ToolInstallation {
                source: manager.into(),
                ..installation(&entry)
            };
            assert!(plan_for_install(tool, &inst).is_err());
            assert!(entry.exists());
        }
    }

    #[test]
    fn minimax_installer_receipt_does_not_imply_a_native_or_global_npm_install() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("custom minimax");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("install.json"),
            serde_json::json!({
                "product": "minimax-code", "updateOwner": "npm-prefix", "prefix": root,
            })
            .to_string(),
        )
        .unwrap();
        #[cfg(target_os = "windows")]
        let entry = root.join("mcode.cmd");
        #[cfg(not(target_os = "windows"))]
        let entry = root.join("bin/mcode");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        for contents in ["#!/bin/sh\nexec release/mcode", "MZ"] {
            std::fs::write(&entry, contents).unwrap();
            assert!(plan_for_install("mcode", &installation(&entry)).is_err());
        }
    }

    #[test]
    fn openclaw_removes_only_the_service_before_its_npm_package() {
        let temp = tempfile::tempdir().unwrap();
        #[cfg(target_os = "windows")]
        let modules = temp.path().join("node_modules");
        #[cfg(not(target_os = "windows"))]
        let modules = temp.path().join("lib/node_modules");
        let package = modules.join("openclaw");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"openclaw"}"#).unwrap();
        let executable = package.join("openclaw.mjs");
        std::fs::write(&executable, "launcher").unwrap();
        #[cfg(target_os = "windows")]
        let entry = temp.path().join("openclaw.cmd");
        #[cfg(not(target_os = "windows"))]
        let entry = temp.path().join("bin/openclaw");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "node node_modules/openclaw/openclaw.mjs").unwrap();
        let inst = ToolInstallation {
            real: if cfg!(target_os = "windows") {
                std::fs::canonicalize(&entry).unwrap()
            } else {
                std::fs::canonicalize(executable).unwrap()
            },
            ..installation(&entry)
        };
        write_fixture_npm(&entry);
        let plan = plan_for_install("openclaw", &inst).unwrap();
        assert!(plan.command.starts_with(&format!(
            "{} uninstall --service --yes --non-interactive && ",
            quote(&inst.path)
        )));
        assert!(plan.command.contains("uninstall -g --prefix"));
        for flag in ["--all", "--state", "--workspace", "--app"] {
            assert!(!plan.command.contains(flag));
        }
        assert!(
            entry.exists(),
            "planning must not execute the service uninstaller"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_service_failure_blocks_package_removal_and_success_continues() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("prefix with spaces");
        let package = root.join("node_modules/openclaw");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"openclaw"}"#).unwrap();
        let entry = root.join("openclaw.cmd");
        let marker = root.join("npm-ran.txt");
        let npm = root.join("npm.cmd");
        std::fs::write(
            &npm,
            format!(
                "@echo off\r\necho removed>\"{}\"\r\nexit /b 0\r\n",
                marker.display()
            ),
        )
        .unwrap();
        for exit_code in [23, 0] {
            // The package reference models an npm .cmd shim without running a real CLI.
            std::fs::write(&entry, format!("@echo off\r\nrem node_modules/openclaw/openclaw.mjs\r\nexit /b {exit_code}\r\n")).unwrap();
            let plan = plan_for_install("openclaw", &installation(&entry)).unwrap();
            let result = run_package_uninstall(&plan.command);
            assert_eq!(result.is_ok(), exit_code == 0);
            assert_eq!(marker.exists(), exit_code == 0);
        }
    }

    #[test]
    fn wsl_uninstall_checks_the_selected_linux_prefix() {
        let command = wsl_uninstall_command(
            "codex",
            "/usr/local/bin/codex",
            "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
        )
        .unwrap();
        assert!(command.starts_with("sh -c '"));
        assert!(command.contains("command -v codex"));
        assert!(command.contains("/lib/node_modules/@openai/codex/"));
        assert!(command.contains("npm uninstall -g --prefix"));
        assert!(!command.contains("call "));
        assert!(command.contains("[ ! -L"));
        assert!(wsl_uninstall_command(
            "codex",
            "/usr/local/bin/codex",
            "/usr/local/Cellar/mise/1/bin/mise"
        )
        .is_err());
        let hermes = wsl_uninstall_command(
            "hermes",
            "/home/me/.local/bin/hermes",
            "/home/me/hermes/venv/bin/hermes",
        )
        .unwrap();
        assert!(hermes.contains("uninstall --yes"));
        assert!(!hermes.contains("pip"));
        assert!(!hermes.contains("--full"));
        for (path, real) in [
            (
                "/home/me/project/node_modules/.bin/codex",
                "/home/me/project/node_modules/@openai/codex/bin/codex.js",
            ),
            (
                "/home/me/.local/bin/codex",
                "/home/me/release/lib/node_modules/@openai/codex/bin/codex.js",
            ),
        ] {
            assert!(wsl_uninstall_command("codex", path, real).is_err());
        }
        let openclaw = wsl_uninstall_command(
            "openclaw",
            "/usr/local/bin/openclaw",
            "/usr/local/lib/node_modules/openclaw/openclaw.mjs",
        )
        .unwrap();
        assert!(openclaw.contains("uninstall --service --yes --non-interactive && npm uninstall"));
    }

    /// The auto-mode probe must land on the same installation as the version check
    /// (`wsl_auto_shell_command`), including a PATH set only by interactive init
    /// (bash's guarded ~/.bashrc, dash's $ENV), and the removal must run there too.
    #[cfg(unix)]
    #[test]
    fn wsl_auto_probe_and_removal_match_the_version_check_shell() {
        use std::{fs, os::unix::fs::PermissionsExt};
        for shell in ["/bin/bash", "/bin/dash"] {
            if !Path::new(shell).exists() {
                continue;
            }
            let home = tempfile::tempdir().unwrap();
            let rc_bin = home.path().join("interactive");
            let base_bin = home.path().join("base");
            for dir in [&rc_bin, &base_bin] {
                fs::create_dir_all(dir).unwrap();
                let tool = dir.join("codex");
                fs::write(&tool, "#!/bin/sh\necho 1.0.0\n").unwrap();
                fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
            }
            let export = format!("export PATH='{}':\"$PATH\"\n", rc_bin.display());
            // Ubuntu's ~/.profile sources ~/.bashrc, which returns unless interactive.
            fs::write(
                home.path().join(".bashrc"),
                format!("case $- in *i*) ;; *) return ;; esac\n{export}"),
            )
            .unwrap();
            fs::write(home.path().join(".profile"), ". \"$HOME/.bashrc\"\n").unwrap();
            fs::write(home.path().join(".shrc"), &export).unwrap();
            let run = |args: &[&str]| {
                let mut command = std::process::Command::new("sh");
                command
                    .args(args)
                    .env_clear()
                    .env("HOME", home.path())
                    .env("SHELL", shell)
                    .env("PATH", format!("{}:/usr/bin:/bin", base_bin.display()))
                    .stdin(std::process::Stdio::null());
                if shell.ends_with("dash") {
                    command.env("ENV", home.path().join(".shrc"));
                }
                let output = command.output().unwrap();
                assert!(output.status.success(), "{shell}: {output:?}");
                String::from_utf8_lossy(&output.stdout).into_owned()
            };
            let last = |out: String| out.lines().last().unwrap_or_default().to_string();
            let rc_tool = rc_bin.join("codex").display().to_string();
            let base_tool = base_bin.join("codex").display().to_string();

            let detected = last(run(&[
                "-c",
                &wsl_auto_shell_command("command -v codex", None),
            ]));
            assert_eq!(detected, rc_tool, "{shell}: version check");
            // A bare `sh -c` reaches the other copy.
            assert_eq!(last(run(&["-c", "command -v codex"])), base_tool);

            let probe = wsl_probe_script("codex");
            let mut args = vec!["-c", WSL_SHELL_PROBE_LAUNCHER, "cc-switch", &probe];
            args.extend(WSL_AUTO_SHELL_FLAGS);
            let stdout = run(&args);
            let (path, _) = parse_wsl_installation(&stdout).unwrap();
            assert_eq!(path, detected, "{shell}: uninstall probe");
            let flag = parse_wsl_shell_flag(&stdout).unwrap();
            assert_eq!(flag, "-lic", "{shell}");
            let removal = run(&[
                "-c",
                WSL_SHELL_RUN_LAUNCHER,
                "cc-switch",
                "command -v codex",
                flag,
            ]);
            assert_eq!(last(removal), detected, "{shell}: removal");
        }
    }

    #[test]
    fn wsl_version_check_command_is_unchanged() {
        let p = "echo s; codex --version";
        assert_eq!(
            wsl_auto_shell_command(p, None),
            format!("\"${{SHELL:-sh}}\" -lic '{p}' 2>/dev/null || \"${{SHELL:-sh}}\" -lc '{p}' 2>/dev/null || \"${{SHELL:-sh}}\" -c '{p}'")
        );
        assert_eq!(
            wsl_auto_shell_command(p, Some("-lc")),
            format!("\"${{SHELL:-sh}}\" -lc '{p}'")
        );
    }

    #[test]
    fn wsl_shell_flag_must_be_reported_and_known() {
        assert_eq!(
            parse_wsl_shell_flag(
                "noise\n__CC_SWITCH_SHELL_FLAG__ -lc\n__CC_SWITCH_UNINSTALL__\n/a\n/b\n"
            ),
            Ok("-lc")
        );
        assert!(parse_wsl_shell_flag("__CC_SWITCH_UNINSTALL__\n/a\n/b\n").is_err());
        assert!(parse_wsl_shell_flag("__CC_SWITCH_SHELL_FLAG__ -x\n").is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_uninstall_passes_the_script_verbatim_to_the_selected_shell() {
        let script = wsl_uninstall_command(
            "codex",
            "/home/me/npm prefix's/bin/codex",
            "/home/me/npm prefix's/lib/node_modules/@openai/codex/bin/codex.js",
        )
        .unwrap();
        let fish = WslShellPreferenceInput {
            wsl_shell: Some("/usr/bin/fish".into()),
            wsl_shell_flag: Some("-lc".into()),
        };
        let explicit = WslShell::from_pref(Some(&fish)).unwrap();
        assert_eq!(explicit.probe_args(&script), ["fish", "-lc", &script]);
        assert_eq!(explicit.run_args(&script, None), ["fish", "-lc", &script]);
        let process = wsl_process("Ubuntu", &explicit.run_args(&script, None)).unwrap();
        let args: Vec<_> = process
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["-d", "Ubuntu", "--exec", "fish", "-lc", &script]);
        assert!(wsl_process("invalid distro", &[&script]).is_err());

        let launcher = |s: &'static str| ["sh", "-c", s, "cc-switch", script.as_str()];
        let auto = WslShell::from_pref(None).unwrap();
        let mut probe = launcher(WSL_SHELL_PROBE_LAUNCHER).to_vec();
        probe.extend(WSL_AUTO_SHELL_FLAGS);
        assert_eq!(auto.probe_args(&script), probe);
        let mut run = launcher(WSL_SHELL_RUN_LAUNCHER).to_vec();
        run.push("-lic");
        assert_eq!(auto.run_args(&script, Some("-lic")), run);

        let one_flag = WslShellPreferenceInput {
            wsl_shell: None,
            wsl_shell_flag: Some("-lc".into()),
        };
        let mut probe = launcher(WSL_SHELL_PROBE_LAUNCHER).to_vec();
        probe.push("-lc");
        assert_eq!(
            WslShell::from_pref(Some(&one_flag))
                .unwrap()
                .probe_args(&script),
            probe
        );

        for invalid in [
            WslShellPreferenceInput {
                wsl_shell: Some("other".into()),
                wsl_shell_flag: None,
            },
            WslShellPreferenceInput {
                wsl_shell: None,
                wsl_shell_flag: Some("-x".into()),
            },
        ] {
            assert!(WslShell::from_pref(Some(&invalid)).is_err());
        }
    }

    #[test]
    fn brew_uninstall_rejects_unrelated_packages_and_shims() {
        for (tool, real) in [
            ("codex", "/opt/homebrew/Cellar/mise/1/bin/mise"),
            ("claude", "/opt/homebrew/Cellar/volta/1/bin/volta-shim"),
            ("grok", "/opt/homebrew/Cellar/grok/1/bin/grok"),
            ("claude", "/opt/homebrew/Cellar/claude-code/1/bin/claude"),
            ("codex", "/opt/homebrew/Cellar/codex/1/bin/codex"),
            ("codex", "/opt/homebrew/Caskroom/unrelated/1/bin/codex"),
            ("gemini", "/opt/homebrew/Caskroom/gemini-cli/1/gemini"),
        ] {
            let inst = ToolInstallation {
                path: format!("/Users/me/.local/share/mise/shims/{tool}"),
                real: real.into(),
                version: None,
                runnable: false,
                error: None,
                exited_with_error: false,
                source: "homebrew".into(),
                is_path_default: true,
            };
            assert!(plan_for_install(tool, &inst).is_err(), "{tool}: {real}");
        }
    }

    #[test]
    fn brew_uninstall_allows_only_the_matching_formula_or_cask() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().to_string_lossy().replace('\\', "/");
        let brew = format!("{prefix}/bin/brew");
        std::fs::create_dir_all(temp.path().join("bin")).unwrap();
        std::fs::write(&brew, "fixture brew").unwrap();

        for (tool, location, token, flag) in [
            ("claude", "Caskroom", "claude-code", "--cask"),
            ("claude", "Caskroom", "claude-code@latest", "--cask"),
            ("codex", "Caskroom", "codex", "--cask"),
            ("gemini", "Cellar", "gemini-cli", "--formula"),
            ("opencode", "Cellar", "opencode", "--formula"),
        ] {
            let real = format!("{prefix}/{location}/{token}/1/bin/{tool}");
            let command = brew_uninstall_command(tool, &real).unwrap().unwrap();
            assert_eq!(
                command,
                format!("{} uninstall {flag} {}", quote(&brew), quote(token))
            );
        }
        let inst = ToolInstallation {
            path: format!("{prefix}/bin/gemini"),
            real: format!("{prefix}/Cellar/gemini-cli/1/libexec/lib/node_modules/@google/gemini-cli/dist/index.js").into(),
            version: None,
            runnable: true,
            error: None,
            exited_with_error: false,
            source: "homebrew".into(),
            is_path_default: true,
        };
        let plan = plan_for_install("gemini", &inst).unwrap();
        assert!(plan.command.contains("uninstall --formula"));
        assert!(!plan.command.contains("npm"));
        std::fs::remove_file(&brew).unwrap();
        assert!(
            plan_for_install("gemini", &inst).is_err(),
            "do not preview a missing brew executable"
        );
    }

    #[test]
    fn hermes_plan_uses_the_selected_cli_without_python_or_pip() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let entry = bin.join("hermes");
        std::fs::write(&entry, "hermes launcher").unwrap();
        let inst = installation(&entry);
        let plan = plan_for_install("hermes", &inst).unwrap();
        assert_eq!(
            plan.command,
            format!("{} uninstall --yes", quote(&inst.path))
        );
        assert_eq!(plan.path, inst.path);
        assert_eq!(plan.real_path, inst.real.to_string_lossy());
        assert!(entry.exists(), "preparing a plan must be read-only");
        std::fs::write(bin.join("python"), "unrelated Python").unwrap();
        assert_eq!(plan_for_install("hermes", &inst).unwrap(), plan);
    }

    #[test]
    fn confirmation_rejects_a_changed_target_or_command() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("hermes");
        std::fs::write(&entry, "launcher").unwrap();
        let plan = plan_for_install("hermes", &installation(&entry)).unwrap();
        assert!(validate_confirmed_plan(&plan, &plan).is_ok());
        let mut changed = plan.clone();
        changed.path.push_str("-other");
        assert!(validate_confirmed_plan(&changed, &plan).is_err());
        changed = plan.clone();
        changed.real_path.push_str("-other");
        assert!(validate_confirmed_plan(&changed, &plan).is_err());
        changed = plan.clone();
        changed.command.push_str(" --full");
        assert!(validate_confirmed_plan(&changed, &plan).is_err());
        changed = plan.clone();
        changed.wsl_distro = Some("Ubuntu".into());
        assert!(validate_confirmed_plan(&changed, &plan).is_err());
        assert!(entry.exists());
    }

    #[test]
    fn claude_native_plan_rejects_a_user_wrapper() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let entry = bin.join("claude");
        std::fs::write(&entry, "#!/bin/sh\necho my wrapper\n").unwrap();
        assert!(plan_for_install("claude", &installation(&entry)).is_err());
        assert!(entry.exists());
        std::fs::write(&entry, b"\x7fELF").unwrap();
        assert!(
            plan_for_install("claude", &installation(&entry))
                .unwrap()
                .native
        );
    }

    #[test]
    fn native_layout_requires_the_real_target_to_belong_to_the_installer() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("unrelated.exe");
        std::fs::write(&executable, "MZ").unwrap();
        for (tool, launcher) in [
            ("grok", ".grok/bin/grok"),
            ("codex", "Programs/OpenAI/Codex/bin/codex.exe"),
        ] {
            let inst = ToolInstallation {
                path: temp.path().join(launcher).to_string_lossy().into_owned(),
                ..installation(&executable)
            };
            assert!(!known_native_install(tool, &inst));
        }
        for (tool, binary) in [
            ("grok", ".grok/downloads/grok-linux-x86_64"),
            ("grok", ".grok/downloads/grok-1.0.3-macos-aarch64"),
            ("grok", ".grok/bin/grok-1.0.50"),
            ("grok", ".grok/bin/grok-1.1.0-beta.1.exe"),
            (
                "codex",
                ".codex/packages/standalone/releases/0.157.0-linux/bin/codex",
            ),
            (
                "codex",
                ".codex/packages/standalone/releases/0.150.0-linux/codex",
            ),
            ("opencode", ".opencode/bin/opencode"),
        ] {
            let path = temp.path().join(binary);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"\x7fELF").unwrap();
            assert!(known_native_install(tool, &installation(&path)));
        }
        assert!(executable.exists());
    }

    #[cfg(unix)]
    #[test]
    fn removal_verification_rejects_dangling_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let entry = temp.path().join("codex");
        std::os::unix::fs::symlink(temp.path().join("missing-mise"), &entry).unwrap();
        assert!(!entry.exists());
        assert!(verify_removed("codex", &entry.to_string_lossy()).is_err());
        std::fs::remove_file(&entry).unwrap();
        assert!(verify_removed("codex", &entry.to_string_lossy()).is_ok());
    }

    #[test]
    fn wsl_probe_requires_an_unambiguous_absolute_target() {
        assert_eq!(
            parse_wsl_installation(
                "login output\n__CC_SWITCH_UNINSTALL__\n/usr/bin/codex\n/opt/codex\n"
            )
            .unwrap(),
            ("/usr/bin/codex".into(), "/opt/codex".into())
        );
        assert!(parse_wsl_installation("__CC_SWITCH_UNINSTALL__\ncodex\n/opt/codex\n").is_err());
        assert!(parse_wsl_installation(
            "__CC_SWITCH_UNINSTALL__\n/usr/bin/codex\n/opt/codex\nextra\n"
        )
        .is_err());
    }

    #[test]
    fn native_uninstall_keeps_configuration_and_other_files() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("claude.exe");
        let config = temp.path().join("settings.json");
        std::fs::write(&executable, "MZ").unwrap();
        std::fs::write(&config, "settings").unwrap();
        let inst = installation(&executable);
        remove_native_files(&inst, std::slice::from_ref(&inst.real)).unwrap();
        assert!(!executable.exists());
        assert_eq!(std::fs::read_to_string(config).unwrap(), "settings");
    }

    #[test]
    fn native_uninstall_rejects_changed_target() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("claude.exe");
        std::fs::write(&executable, "MZ").unwrap();
        let mut inst = installation(&executable);
        inst.real = temp.path().join("another.exe");
        assert!(remove_native_files(&inst, std::slice::from_ref(&executable)).is_err());
        assert!(executable.exists());
    }

    #[test]
    fn grok_binary_names_follow_the_installer_layout() {
        for (dir, name) in [
            ("bin", "grok"),
            ("bin", "grok.exe"),
            ("bin", "grok-1.0.50"),
            ("downloads", "grok-macos-aarch64"),
            ("downloads", "grok-1.0.3-linux-x86_64"),
            ("downloads", "grok-1.0.3-windows-x86_64.exe"),
        ] {
            assert!(is_grok_binary_name(dir, name), "{dir}/{name}");
        }
        for (dir, name) in [
            ("bin", "agent"),
            ("bin", "grok-latest"),
            ("bin", "grok-macos-aarch64"),
            ("downloads", "grok-1.0.3"),
            ("downloads", "grok-1.0.3-freebsd-x86_64"),
            ("downloads", "grok"),
            ("lib", "grok-1.0.50"),
        ] {
            assert!(!is_grok_binary_name(dir, name), "{dir}/{name}");
        }
        let temp = tempfile::tempdir().unwrap();
        let other = temp.path().join("tools/bin/grok-1.0.50");
        assert!(grok_native_root(&other).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn grok_native_removal_covers_every_installer_binary_and_launcher() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap().join(".grok");
        let bin = root.join("bin");
        let downloads = root.join("downloads");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();
        let current = bin.join("grok-1.0.50");
        let previous = bin.join("grok-1.0.46");
        let fetched = downloads.join("grok-1.0.3-macos-aarch64");
        let legacy = downloads.join("grok-macos-aarch64");
        for binary in [&current, &previous, &fetched, &legacy] {
            std::fs::write(binary, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        }
        let config = root.join("config.toml");
        let session = root.join("sessions/one.json");
        std::fs::create_dir_all(session.parent().unwrap()).unwrap();
        std::fs::write(&config, "[cli]\ninstaller = \"internal\"\n").unwrap();
        std::fs::write(&session, "{}").unwrap();
        let notes = bin.join("grok-notes.txt");
        std::fs::write(&notes, "user file").unwrap();
        let unrelated = temp.path().join("elsewhere");
        std::fs::write(&unrelated, [0x7f, b'E', b'L', b'F']).unwrap();
        symlink("grok-1.0.50", bin.join("grok")).unwrap();
        symlink("../downloads/grok-1.0.3-macos-aarch64", bin.join("agent")).unwrap();
        let foreign_agent = root.join("bin/agent.exe");
        symlink(&unrelated, &foreign_agent).unwrap();

        let inst = ToolInstallation {
            path: bin.join("grok").to_string_lossy().into_owned(),
            ..installation(&current)
        };
        assert!(known_native_install("grok", &inst));
        let (plan, removals) = plan_and_removals("grok", &inst).unwrap();
        assert!(plan.native);
        assert_eq!(removals.first(), Some(&bin.join("grok")));
        for path in [&current, &previous, &fetched, &legacy, &bin.join("agent")] {
            assert!(removals.contains(path), "{}", path.display());
            assert!(plan.command.contains(&*path.to_string_lossy()));
        }
        for kept in [&config, &session, &notes, &unrelated, &foreign_agent] {
            assert!(!removals.contains(kept), "{}", kept.display());
        }

        remove_native_files(&inst, &removals).unwrap();
        assert!(std::fs::symlink_metadata(bin.join("grok")).is_err());
        assert!(std::fs::symlink_metadata(bin.join("agent")).is_err());
        for path in [&current, &previous, &fetched, &legacy] {
            assert!(!path.exists());
        }
        for kept in [&config, &session, &notes, &unrelated] {
            assert!(kept.exists());
        }
        assert!(std::fs::symlink_metadata(&foreign_agent).is_ok());
    }
}
