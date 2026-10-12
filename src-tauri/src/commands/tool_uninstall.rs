//! Uninstall the detected installation, retaining configuration and user data.
//! Never fall back to a different package manager or delete an arbitrary directory.

use super::*;

pub(super) fn uninstall(tool: &str, pref: Option<&WslShellPreferenceInput>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    if let Some(distro) = wsl_distro_for_tool(tool) {
        let command = wsl_npm_uninstall_command(tool)?;
        let line = build_wsl_tool_action_line(
            &distro,
            &command,
            pref.and_then(|p| p.wsl_shell.as_deref()),
            pref.and_then(|p| p.wsl_shell_flag.as_deref()),
        )?;
        return run_tool_lifecycle_silently(&line, "tool_uninstall");
    }
    let _ = pref;
    let installs = enumerate_tool_installations(tool);
    let inst = default_install(&installs).ok_or_else(|| {
        format!("Cannot identify the installation of {} to uninstall. Check installations and try again.", tool_display_name(tool))
    })?;
    if let Some(command) = package_uninstall_command(tool, inst) {
        #[cfg(target_os = "windows")]
        let command =
            format!("@chcp 65001>nul\r\n@echo off\r\ncall {command}\r\nexit /b %errorlevel%");
        run_tool_lifecycle_silently(&command, "tool_uninstall")?;
    } else if known_native_install(tool, inst) {
        remove_native_executable(inst)?;
    } else {
        return Err(format!(
            "Cannot automatically uninstall {} at {}. Use the original installer or package manager to uninstall it.",
            tool_display_name(tool), inst.path
        ));
    }
    // Some managers return zero even if the selected package was not removed.
    if Path::new(&inst.path).exists() {
        return Err(format!(
            "{} is still installed at {}. Use the original installer or package manager to uninstall it.",
            tool_display_name(tool), inst.path
        ));
    }
    Ok(())
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
    let package = npm_package_for(tool)?;
    for entry in [Path::new(&inst.path), inst.real.as_path()] {
        for ancestor in entry.ancestors().skip(1) {
            let modules = ancestor.join("node_modules");
            let manifest = modules.join(package).join("package.json");
            let value: serde_json::Value = match std::fs::read(&manifest)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(value) => value,
                None => continue,
            };
            if value.get("name").and_then(|name| name.as_str()) != Some(package) {
                continue;
            }
            let package_dir = std::fs::canonicalize(modules.join(package)).ok()?;
            let owns_target = inst.real.starts_with(&package_dir);
            // Windows npm shims are files, not symlinks. Require both the shim's
            // location and its package reference; a nearby unrelated package is not ownership.
            let owns_shim = entry.parent() == Some(ancestor)
                && !is_native_executable(entry)
                && std::fs::read_to_string(entry).is_ok_and(|script| {
                    script
                        .replace('\\', "/")
                        .contains(&format!("node_modules/{package}/"))
                });
            if !owns_target && !owns_shim {
                continue;
            }
            // POSIX global packages live in <prefix>/lib/node_modules; Windows in <prefix>/node_modules.
            #[cfg(not(target_os = "windows"))]
            if ancestor.file_name().is_some_and(|name| name == "lib") {
                return ancestor.parent().map(Path::to_path_buf);
            }
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn package_uninstall_command(tool: &str, inst: &ToolInstallation) -> Option<String> {
    #[cfg(not(target_os = "windows"))]
    {
        let real = inst.real.to_string_lossy();
        for (marker, flag) in [("/Caskroom/", "--cask "), ("/Cellar/", "")] {
            if let Some((prefix, rest)) = real.split_once(marker) {
                let token = rest.split('/').next()?;
                let brew = format!("{prefix}/bin/brew");
                return Some(format!("{} uninstall {flag}{}", quote(&brew), quote(token)));
            }
        }
    }
    if tool == "hermes" {
        let parent = Path::new(&inst.path).parent()?;
        let python = [
            parent.join("python.exe"),
            parent.join("python"),
            parent.parent()?.join("python.exe"),
        ]
        .into_iter()
        .find(|path| path.is_file())?;
        return Some(format!(
            "{} -m pip uninstall -y hermes-agent",
            quote(&python.to_string_lossy())
        ));
    }
    let package = npm_package_for(tool)?;
    let manager = match inst.source.as_str() {
        "volta" => Some(("volta", "uninstall")),
        "pnpm" => Some(("pnpm", "remove -g")),
        "bun" => Some(("bun", "remove -g")),
        _ => None,
    };
    if let Some((manager, args)) = manager {
        let manager = manager_next_to(&inst.path, manager)?;
        return Some(format!("{} {args} {package}", quote(&manager)));
    }
    let prefix = npm_prefix(tool, inst)?;
    let npm = manager_next_to(&inst.path, "npm").unwrap_or_else(|| "npm".to_string());
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
    let real = inst.real.to_string_lossy();
    let normalized = real.replace('\\', "/").to_ascii_lowercase();
    if normalized.contains("/node_modules/") {
        return false;
    }
    match tool {
        "claude" => {
            normalized.contains("/.local/share/claude/versions/")
                || normalized.ends_with("/.local/bin/claude")
                || normalized.ends_with("/.local/bin/claude.exe")
        }
        "codex" => codex_standalone_install(&inst.path, &real).is_some(),
        "grok" => is_grok_native_install(&inst.path, &real),
        "opencode" => {
            normalized.ends_with("/.opencode/bin/opencode")
                || normalized.ends_with("/.opencode/bin/opencode.exe")
        }
        "mcode" => mcode_receipt_install_root(&inst.path, McodeLayout::NATIVE).is_some(),
        _ => false,
    }
}

/// Remove only the selected executable and its launcher. No recursive deletion:
/// native installers may keep settings, sessions and projects next to their binaries.
fn remove_native_executable(inst: &ToolInstallation) -> Result<(), String> {
    let entry = Path::new(&inst.path);
    // Validate both paths before changing either. A replaced symlink must not delete a new target.
    if std::fs::canonicalize(entry).map_err(|e| e.to_string())? != inst.real {
        return Err(
            "Installation changed during uninstall. Check installations and try again.".into(),
        );
    }
    for path in [inst.real.as_path(), entry] {
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
fn wsl_npm_uninstall_command(tool: &str) -> Result<String, String> {
    let package = npm_package_for(tool).ok_or_else(|| {
        "Use the original installer to uninstall this WSL application.".to_string()
    })?;
    // Resolve ownership inside the selected distro; never run the Windows package manager.
    Ok(format!(
        "entry=$(command -v {tool}) && real=$(readlink -f -- \"$entry\") && case \"$real\" in */lib/node_modules/{package}/*) prefix=${{real%/lib/node_modules/{package}/*}}; npm uninstall -g --prefix \"$prefix\" {package} && {{ [ ! -e \"$entry\" ] || {{ echo 'Application is still installed' >&2; exit 1; }}; }} ;; *) echo 'Use the original installer or package manager to uninstall this WSL application.' >&2; exit 1 ;; esac"
    ))
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
            source: "system".into(),
            is_path_default: true,
        }
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
        let inst = installation(&executable);
        let command = package_uninstall_command("codex", &inst).unwrap();
        assert!(command.contains("uninstall -g --prefix"));
        assert!(command.contains(&quote(&prefix.to_string_lossy())));
        assert!(command.ends_with("@openai/codex"));
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"another-package"}"#,
        )
        .unwrap();
        assert!(package_uninstall_command("codex", &inst).is_none());
    }

    #[test]
    fn unknown_executable_is_not_removed_or_assumed_to_be_npm() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("codex.exe");
        std::fs::write(&executable, "MZ").unwrap();
        let inst = installation(&executable);
        assert!(package_uninstall_command("codex", &inst).is_none());
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
            npm_prefix("codex", &installation(&executable)),
            Some(temp.path().to_path_buf())
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_uninstall_checks_the_selected_linux_prefix() {
        let command = wsl_npm_uninstall_command("codex").unwrap();
        assert!(command.contains("command -v codex"));
        assert!(command.contains("/lib/node_modules/@openai/codex/"));
        assert!(command.contains("npm uninstall -g --prefix"));
        assert!(!command.contains("call "));
        assert!(wsl_npm_uninstall_command("hermes").is_err());
    }

    #[test]
    fn native_uninstall_keeps_configuration_and_other_files() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("claude.exe");
        let config = temp.path().join("settings.json");
        std::fs::write(&executable, "MZ").unwrap();
        std::fs::write(&config, "settings").unwrap();
        remove_native_executable(&installation(&executable)).unwrap();
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
        assert!(remove_native_executable(&inst).is_err());
        assert!(executable.exists());
    }
}
