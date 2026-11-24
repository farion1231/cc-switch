use super::*;

fn push_unique_path(paths: &mut IndexSet<std::path::PathBuf>, path: std::path::PathBuf) {
    if path.as_os_str().is_empty() {
        return;
    }

    paths.insert(path);
}

fn push_env_single_dir(
    paths: &mut IndexSet<std::path::PathBuf>,
    value: Option<std::ffi::OsString>,
) {
    if let Some(raw) = value {
        push_unique_path(paths, std::path::PathBuf::from(raw));
    }
}

fn extend_from_path_list(
    paths: &mut IndexSet<std::path::PathBuf>,
    value: Option<std::ffi::OsString>,
    suffix: Option<&str>,
) {
    if let Some(raw) = value {
        for p in std::env::split_paths(&raw) {
            let dir = match suffix {
                Some(s) => p.join(s),
                None => p,
            };
            push_unique_path(paths, dir);
        }
    }
}

fn extend_from_cli_path_env(
    paths: &mut IndexSet<std::path::PathBuf>,
    value: Option<std::ffi::OsString>,
) {
    if let Some(raw) = value {
        for p in std::env::split_paths(&raw) {
            if should_skip_cli_path_env_dir(&p) {
                continue;
            }
            push_unique_path(paths, p);
        }
    }
}

fn should_skip_cli_path_env_dir(path: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        is_windows_app_execution_alias_dir(path)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

#[cfg(target_os = "windows")]
fn is_windows_app_execution_alias_dir(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    normalized
        .trim_end_matches('\\')
        .ends_with("\\microsoft\\windowsapps")
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn push_env_child_dir(
    paths: &mut IndexSet<std::path::PathBuf>,
    value: Option<std::ffi::OsString>,
    child: &str,
) {
    if let Some(raw) = value {
        push_unique_path(paths, std::path::PathBuf::from(raw).join(child));
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn extend_existing_child_search_paths(
    paths: &mut IndexSet<std::path::PathBuf>,
    base: &Path,
    suffix: Option<&str>,
) {
    if !base.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(base) {
        for entry in entries.flatten() {
            let path = match suffix {
                Some(suffix) => entry.path().join(suffix),
                None => entry.path(),
            };
            if path.exists() {
                push_unique_path(paths, path);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn extend_windows_cli_manager_search_paths(paths: &mut IndexSet<std::path::PathBuf>, home: &Path) {
    push_env_single_dir(paths, std::env::var_os("PNPM_HOME"));
    push_env_child_dir(paths, std::env::var_os("VOLTA_HOME"), "bin");
    push_env_single_dir(paths, std::env::var_os("NVM_SYMLINK"));
    push_env_child_dir(paths, std::env::var_os("SCOOP"), "shims");
    push_env_child_dir(paths, std::env::var_os("SCOOP_GLOBAL"), "shims");

    if let Some(nvm_home) = std::env::var_os("NVM_HOME") {
        let nvm_home = std::path::PathBuf::from(nvm_home);
        push_unique_path(paths, nvm_home.clone());
        extend_existing_child_search_paths(paths, &nvm_home, None);
    }

    if let Some(appdata) = dirs::data_dir() {
        let nvm_home = appdata.join("nvm");
        push_unique_path(paths, nvm_home.clone());
        extend_existing_child_search_paths(paths, &nvm_home, None);
    }

    if !home.as_os_str().is_empty() {
        push_unique_path(paths, home.join("scoop").join("shims"));
    }

    if let Some(local_data) = dirs::data_local_dir() {
        push_unique_path(paths, local_data.join("pnpm"));
        push_unique_path(paths, local_data.join("Volta").join("bin"));
        push_unique_path(paths, local_data.join("Yarn").join("bin"));
    }

    let program_data = std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\ProgramData"));
    push_unique_path(paths, program_data.join("scoop").join("shims"));
}

/// OpenCode install.sh 路径优先级（见 https://github.com/anomalyco/opencode README）:
///   $OPENCODE_INSTALL_DIR > $XDG_BIN_DIR > $HOME/bin > $HOME/.opencode/bin
/// 额外扫描 Bun 默认全局安装路径（~/.bun/bin）
/// 和 Go 安装路径（~/go/bin、$GOPATH/*/bin）。
fn opencode_extra_search_paths(
    home: &Path,
    opencode_install_dir: Option<std::ffi::OsString>,
    xdg_bin_dir: Option<std::ffi::OsString>,
    gopath: Option<std::ffi::OsString>,
) -> Vec<std::path::PathBuf> {
    let mut paths = IndexSet::new();

    push_env_single_dir(&mut paths, opencode_install_dir);
    push_env_single_dir(&mut paths, xdg_bin_dir);

    if !home.as_os_str().is_empty() {
        push_unique_path(&mut paths, home.join("bin"));
        push_unique_path(&mut paths, home.join(".opencode").join("bin"));
        push_unique_path(&mut paths, home.join(".bun").join("bin"));
        push_unique_path(&mut paths, home.join("go").join("bin"));
    }

    extend_from_path_list(&mut paths, gopath, Some("bin"));

    paths.into_iter().collect()
}

/// Grok's official installer writes the launcher to `$GROK_BIN_DIR` or, by
/// default, `~/.grok/bin`. Keep these ahead of generic npm/Node locations so
/// version probing and anchored updates can see the native distribution even
/// when the GUI process inherited a stale PATH.
fn grok_extra_search_paths(
    home: &Path,
    grok_bin_dir: Option<std::ffi::OsString>,
) -> Vec<std::path::PathBuf> {
    let mut paths = IndexSet::new();
    push_env_single_dir(&mut paths, grok_bin_dir);
    if !home.as_os_str().is_empty() {
        push_unique_path(&mut paths, home.join(".grok").join("bin"));
    }
    paths.into_iter().collect()
}

/// MiniMax Code 官方 installer 的入口：POSIX 在 `<root>/bin/mcode`，Windows 的
/// `mcode.cmd` / `mcode.ps1` 直接放在 `<root>` 下（不在 `bin`）。root 默认
/// `~/.minimax-code`，可由 `MCODE_INSTALL_DIR` 改。installer 只改 shell rc / 用户 PATH，
/// GUI 进程继承的旧 PATH 看不到，所以同 Grok 一样显式排在通用 npm/Node 目录前面。
fn mcode_extra_search_paths(
    home: &Path,
    mcode_install_dir: Option<std::ffi::OsString>,
) -> Vec<std::path::PathBuf> {
    let launcher_dir = |root: std::path::PathBuf| {
        if cfg!(target_os = "windows") {
            root
        } else {
            root.join("bin")
        }
    };
    let mut paths = IndexSet::new();
    if let Some(root) = mcode_install_dir.filter(|value| !value.is_empty()) {
        push_unique_path(&mut paths, launcher_dir(std::path::PathBuf::from(root)));
    }
    if !home.as_os_str().is_empty() {
        push_unique_path(&mut paths, launcher_dir(home.join(".minimax-code")));
    }
    paths.into_iter().collect()
}

/// PATHEXT 里 `.CMD` 是否排在 `.EXE` 之前。未设置或两者都不在时按 Windows 默认
/// （`.EXE` 在前）处理。
#[cfg(any(target_os = "windows", test))]
fn pathext_prefers_cmd_over_exe(pathext: Option<&str>) -> bool {
    let Some(pathext) = pathext else {
        return false;
    };
    let position = |ext: &str| {
        pathext
            .split(';')
            .map(str::trim)
            .position(|entry| entry.eq_ignore_ascii_case(ext))
    };
    match (position(".cmd"), position(".exe")) {
        (Some(cmd), Some(exe)) => cmd < exe,
        (Some(_), None) => true,
        _ => false,
    }
}

/// 同一目录里 `<tool>.cmd` 和 `<tool>.exe` 并存时，命令行敲 `<tool>` 只会按 PATHEXT
/// 顺序命中其中一个，另一个被遮住。调用方只对已知是同一处安装多出来的入口用它
/// （Hermes 换成 `.exe` 启动器后留下的旧 `hermes.cmd`，#8839）。
#[cfg(any(target_os = "windows", test))]
fn shadowed_by_pathext_sibling(path: &Path, pathext: Option<&str>) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    let cmd_first = pathext_prefers_cmd_over_exe(pathext);
    let shadowing_ext = if ext.eq_ignore_ascii_case("cmd") && !cmd_first {
        "exe"
    } else if ext.eq_ignore_ascii_case("exe") && cmd_first {
        "cmd"
    } else {
        return false;
    };
    path.with_extension(shadowing_ext).is_file()
}

pub(super) fn tool_executable_candidates(tool: &str, dir: &Path) -> Vec<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let extensionless = dir.join(tool);
        let exe = dir.join(format!("{tool}.exe"));
        let mut candidates = vec![dir.join(format!("{tool}.cmd"))];
        if let Some(exe) = windows_preferred_tool_executable(tool, &exe) {
            candidates.push(exe);
        }
        if windows_runnable_sibling_for_extensionless_tool(&extensionless).is_none() {
            candidates.push(extensionless);
        }
        candidates
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![dir.join(tool)]
    }
}

fn extend_mise_node_search_paths(paths: &mut IndexSet<std::path::PathBuf>, home: &Path) {
    if home.as_os_str().is_empty() {
        return;
    }

    let mise_base = home.join(".local/share/mise");
    push_unique_path(paths, mise_base.join("shims"));

    let node_installs = mise_base.join("installs").join("node");
    if node_installs.exists() {
        if let Ok(entries) = std::fs::read_dir(&node_installs) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    push_unique_path(paths, bin_path);
                }
            }
        }
    }
}
/// fnm only puts Node on PATH through `fnm env`, which prepends a per-shell
/// `%LOCALAPPDATA%\fnm_multishells\<id>` link to that shell's PATH. Nothing is
/// written to the registry PATH, so a GUI-launched CC Switch sees neither node,
/// npm nor the npm global CLIs (#8484, #6991). `<fnm dir>\aliases\default` is a
/// stable link to the default Node installation; on Windows node.exe, npm.cmd
/// and npm global commands live directly in that directory (no `bin`).
///
/// Mirrors fnm's own base-dir lookup: `FNM_DIR`, then the legacy `~\.fnm`, then
/// `%APPDATA%\fnm`. Callers keep only the directories that exist.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn fnm_default_alias_dir_candidates(
    fnm_dir: Option<std::ffi::OsString>,
    home: &Path,
    data_dir: Option<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut bases = IndexSet::new();
    if let Some(dir) = fnm_dir.filter(|value| !value.is_empty()) {
        push_unique_path(&mut bases, std::path::PathBuf::from(dir));
    }
    if !home.as_os_str().is_empty() {
        push_unique_path(&mut bases, home.join(".fnm"));
    }
    if let Some(data_dir) = data_dir {
        push_unique_path(&mut bases, data_dir.join("fnm"));
    }
    bases
        .into_iter()
        .map(|base| base.join("aliases").join("default"))
        .collect()
}

/// `extend_from_cli_path_env` consumes an `OsString` via `split_paths`. On
/// Windows this is derived from `effective_path_string`; on other platforms the
/// raw process value is returned unchanged (zero behaviour change).
#[cfg(target_os = "windows")]
pub(super) fn effective_path_os() -> Option<std::ffi::OsString> {
    Some(std::ffi::OsString::from(effective_path_string()))
}

#[cfg(not(target_os = "windows"))]
pub(super) fn effective_path_os() -> Option<std::ffi::OsString> {
    std::env::var_os("PATH")
}

/// Prepend a candidate directory without converting the existing PATH to
/// UTF-8. Unix permits arbitrary non-NUL bytes in environment values; keeping
/// this as an `OsString` ensures one non-Unicode segment cannot discard or
/// corrupt every other interpreter directory needed by an npm/python shim.
#[cfg(not(target_os = "windows"))]
pub(super) fn prepend_search_dir_to_path(
    dir: &Path,
    current_path: &std::ffi::OsStr,
) -> std::ffi::OsString {
    let mut path = dir.as_os_str().to_os_string();
    if !current_path.is_empty() {
        path.push(":");
        path.push(current_path);
    }
    path
}

/// Expand `%VAR%` environment-variable references. The registry `Path` value is
/// `REG_EXPAND_SZ`, and the `String` returned by `winreg` is not auto-expanded.
/// MSI launches can also lose user-defined variables such as `%PNPM_HOME%`.
/// Prefer process overrides, then the user and machine registry values. Registry
/// values may reference other variables; unknown/cyclic references stay literal.
#[cfg(target_os = "windows")]
pub(super) fn expand_env_chars(raw: &str) -> String {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let user = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .ok();
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
        .ok();
    expand_env_chars_with_lookup(
        raw,
        &|name| std::env::var(name).ok(),
        &|name| {
            user.as_ref()
                .and_then(|key| key.get_value::<String, _>(name).ok())
                .or_else(|| {
                    machine
                        .as_ref()
                        .and_then(|key| key.get_value::<String, _>(name).ok())
                })
        },
        &mut Vec::new(),
    )
}

#[cfg(target_os = "windows")]
fn expand_env_chars_with_lookup(
    raw: &str,
    process: &impl Fn(&str) -> Option<String>,
    registry: &impl Fn(&str) -> Option<String>,
    expanding: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find('%') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('%') {
            None => {
                out.push('%');
                out.push_str(after);
                rest = "";
                break;
            }
            Some(close) => {
                let name = &after[..close];
                let is_ident =
                    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                let value = if is_ident {
                    process(name).or_else(|| {
                        // Bound nesting even for an unusually long registry chain.
                        if expanding.len() >= 32
                            || expanding.iter().any(|var| var.eq_ignore_ascii_case(name))
                        {
                            return None;
                        }
                        let raw = registry(name)?;
                        expanding.push(name.to_owned());
                        let value =
                            expand_env_chars_with_lookup(&raw, process, registry, expanding);
                        expanding.pop();
                        Some(value)
                    })
                } else {
                    None
                };
                match value {
                    Some(value) => out.push_str(&value),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[close + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}
/// 构建某工具的候选搜索目录（原生安装优先，PATH 兜底）。
/// 单探兜底 (`scan_cli_version`) 与全量枚举 (`enumerate_tool_installations`) 共用，
/// 确保两条路径看到的是同一组安装位置。
pub(super) fn build_tool_search_paths(tool: &str) -> Vec<std::path::PathBuf> {
    let home = crate::config::get_home_dir();

    // 常见的安装路径（原生安装优先）
    // Preserve search priority without scanning all previous paths for every insertion.
    let mut search_paths = IndexSet::new();
    if tool == "kimi" {
        let install_dir = std::env::var_os("KIMI_INSTALL_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".kimi-code"));
        push_unique_path(&mut search_paths, install_dir.join("bin"));
    }
    if tool == "grok" {
        let extra_paths = grok_extra_search_paths(&home, std::env::var_os("GROK_BIN_DIR"));
        for path in extra_paths {
            push_unique_path(&mut search_paths, path);
        }
    }
    if tool == "mcode" {
        let extra_paths = mcode_extra_search_paths(&home, std::env::var_os("MCODE_INSTALL_DIR"));
        for path in extra_paths {
            push_unique_path(&mut search_paths, path);
        }
    }
    if tool == "omp" {
        // A Bun global install (`bun install -g @oh-my-pi/pi-coding-agent`) puts
        // the launcher in Bun's global bin, which is not on the PATH a GUI process
        // inherits — same reason OpenCode scans it. The official installer's
        // prebuilt binary (`~/.local/bin`) and Homebrew are covered below.
        if !home.as_os_str().is_empty() {
            push_unique_path(&mut search_paths, home.join(".bun").join("bin"));
        }
    }
    if !home.as_os_str().is_empty() {
        push_unique_path(&mut search_paths, home.join(".local/bin"));
        push_unique_path(&mut search_paths, home.join(".npm-global/bin"));
        push_unique_path(&mut search_paths, home.join("n/bin"));
        push_unique_path(&mut search_paths, home.join(".volta/bin"));
        extend_mise_node_search_paths(&mut search_paths, &home);
    }

    #[cfg(target_os = "macos")]
    {
        push_unique_path(
            &mut search_paths,
            std::path::PathBuf::from("/opt/homebrew/bin"),
        );
        push_unique_path(
            &mut search_paths,
            std::path::PathBuf::from("/usr/local/bin"),
        );
        if tool == "hermes" {
            let python_base = home.join("Library").join("Python");
            if python_base.exists() {
                if let Ok(entries) = std::fs::read_dir(&python_base) {
                    for entry in entries.flatten() {
                        let bin_path = entry.path().join("bin");
                        if bin_path.exists() {
                            push_unique_path(&mut search_paths, bin_path);
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        push_unique_path(
            &mut search_paths,
            std::path::PathBuf::from("/usr/local/bin"),
        );
        push_unique_path(&mut search_paths, std::path::PathBuf::from("/usr/bin"));
    }

    #[cfg(target_os = "windows")]
    {
        // Official standalone (non-npm) installer locations — belt-and-suspenders
        // alongside the registry-PATH merge in `effective_path_os`. These
        // installers normally register themselves on the user PATH, but some
        // per-user MSI/MSIX installs do not, and an in-app-update relaunch can
        // drop the user PATH (#6061), so add them explicitly here. Placed ahead
        // of the npm directory so a native install wins over a stale npm shim
        // (#4701).
        if let Some(local_data) = dirs::data_local_dir() {
            if tool == "codex" {
                // OpenAI Codex Installer.exe / .msi standalone install location
                // (#6061, #6047).
                push_unique_path(
                    &mut search_paths,
                    local_data
                        .join("Programs")
                        .join("OpenAI")
                        .join("Codex")
                        .join("bin"),
                );
            }
            if tool == "claude" {
                // `winget install Anthropic.ClaudeCode` / official native
                // installer location (#6278).
                push_unique_path(
                    &mut search_paths,
                    local_data.join("Programs").join("claude"),
                );
            }
            if tool == "agy" {
                // Antigravity CLI official install.ps1 location.
                push_unique_path(&mut search_paths, local_data.join("agy").join("bin"));
            }
        }
        if let Some(appdata) = dirs::data_dir() {
            push_unique_path(&mut search_paths, appdata.join("npm"));
            if tool == "hermes" {
                let python_base = appdata.join("Python");
                if python_base.exists() {
                    if let Ok(entries) = std::fs::read_dir(&python_base) {
                        for entry in entries.flatten() {
                            let scripts_path = entry.path().join("Scripts");
                            if scripts_path.exists() {
                                push_unique_path(&mut search_paths, scripts_path);
                            }
                        }
                    }
                }
            }
        }
        if tool == "hermes" {
            if let Some(local_data) = dirs::data_local_dir() {
                let programs_python = local_data.join("Programs").join("Python");
                if programs_python.exists() {
                    if let Ok(entries) = std::fs::read_dir(&programs_python) {
                        for entry in entries.flatten() {
                            let scripts_path = entry.path().join("Scripts");
                            if scripts_path.exists() {
                                push_unique_path(&mut search_paths, scripts_path);
                            }
                        }
                    }
                }
            }
        }
        push_unique_path(
            &mut search_paths,
            std::path::PathBuf::from("C:\\Program Files\\nodejs"),
        );
        extend_windows_cli_manager_search_paths(&mut search_paths, &home);
    }

    let fnm_base = home.join(".local/state/fnm_multishells");
    if fnm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&fnm_base) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    push_unique_path(&mut search_paths, bin_path);
                }
            }
        }
    }

    let nvm_base = home.join(".nvm/versions/node");
    if nvm_base.exists() {
        if let Ok(entries) = std::fs::read_dir(&nvm_base) {
            for entry in entries.flatten() {
                let bin_path = entry.path().join("bin");
                if bin_path.exists() {
                    push_unique_path(&mut search_paths, bin_path);
                }
            }
        }
    }

    if tool == "opencode" {
        let extra_paths = opencode_extra_search_paths(
            &home,
            std::env::var_os("OPENCODE_INSTALL_DIR"),
            std::env::var_os("XDG_BIN_DIR"),
            std::env::var_os("GOPATH"),
        );

        for path in extra_paths {
            push_unique_path(&mut search_paths, path);
        }
    }

    let path_env = effective_path_os();
    extend_from_cli_path_env(&mut search_paths, path_env);
    search_paths.into_iter().collect()
}

#[cfg(target_os = "windows")]
fn is_windows_command_script(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}
#[cfg(target_os = "windows")]
fn windows_runnable_sibling_for_extensionless_tool(path: &Path) -> Option<std::path::PathBuf> {
    if path.extension().is_some() {
        return None;
    }

    ["cmd", "exe"]
        .iter()
        .map(|ext| path.with_extension(ext))
        .find(|candidate| candidate.is_file())
}

/// Pi ships only through npm, but PyPI has an unrelated Python 2 package also named
/// `pi` whose console script lands in a Python `Scripts` dir as `pi.exe`. That file is
/// never the Pi coding agent: probing it reports a Python traceback as "installed but
/// broken", and uninstall would target it (#9267). Only pip's own layout is matched
/// (`<tool>.exe` directly inside a `Scripts` dir); npm writes `.cmd` shims, so a custom
/// npm prefix under some `Scripts` folder is left alone.
#[cfg(any(target_os = "windows", test))]
fn is_foreign_pip_launcher(tool: &str, path: &Path) -> bool {
    tool == "pi"
        && path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .ends_with(&format!("/scripts/{tool}.exe"))
}

/// VS Code's `bin\copilot-shim\copilot.exe` is a placeholder that only prints
/// "GitHub Copilot CLI was not found" when the real CLI is missing. It sits on PATH under
/// the tool's own name but is never the CLI, so probing it reports "installed but broken"
/// instead of "not installed". Matched by its directory, which covers `.exe` and `.cmd`.
#[cfg(any(target_os = "windows", test))]
fn is_vscode_copilot_shim(tool: &str, path: &Path) -> bool {
    tool == "copilot"
        && path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .contains("/copilot-shim/")
}

/// OpenCode Desktop has shipped both root-level and Electron `resources/` CLI sidecars.
/// Some Electron releases omitted the sidecar; `resources/app.asar` still identifies the
/// Desktop bundle, so skip its GUI instead of probing `opencode.exe` with `--version`.
/// A standalone OpenCode CLI install remains valid when no Desktop marker is present.
/// Also skips launchers that only share the tool's name (`is_foreign_pip_launcher`).
#[cfg(target_os = "windows")]
fn windows_preferred_tool_executable(tool: &str, path: &Path) -> Option<std::path::PathBuf> {
    if is_foreign_pip_launcher(tool, path) || is_vscode_copilot_shim(tool, path) {
        return None;
    }
    if tool == "opencode"
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("opencode.exe"))
    {
        if let Some(parent) = path.parent() {
            let sidecar = parent.join("opencode-cli.exe");
            if sidecar.is_file() {
                return Some(sidecar);
            }

            let resources = parent.join("resources");
            let bundled_sidecar = resources.join("opencode-cli.exe");
            if bundled_sidecar.is_file() {
                return Some(bundled_sidecar);
            }

            if resources.join("app.asar").is_file() {
                return None;
            }
        }
    }

    Some(
        windows_runnable_sibling_for_extensionless_tool(path).unwrap_or_else(|| path.to_path_buf()),
    )
}

#[cfg(target_os = "windows")]
fn windows_preferred_path_default(tool: &str, raw: &str) -> Option<std::path::PathBuf> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| {
            !is_windows_app_execution_alias_dir(Path::new(line).parent().unwrap_or(Path::new("")))
        })
        .find_map(|line| windows_preferred_tool_executable(tool, Path::new(line)))
}

#[cfg(target_os = "windows")]
pub(super) fn build_windows_tool_command(
    tool_path: &Path,
    args: &[&str],
    new_path: &str,
) -> std::process::Command {
    use std::process::Command;

    if is_windows_command_script(tool_path) {
        // `resolve_path_default` returns a canonical path so callers can
        // compare installation identities. Canonical Windows paths carry a
        // `\\?\` prefix, which `cmd /C call` rejects for batch files. Normalize
        // only at this shell boundary and keep the canonical identity intact
        // everywhere else.
        let shell_path = windows_shell_compatible_path(tool_path);
        let path = shell_path.to_string_lossy();
        let args = args
            .iter()
            .map(|arg| windows_cmd_double_quote_arg(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let command = format!(
            "call {}{}",
            win_quote_path_for_batch(&path),
            if args.is_empty() {
                String::new()
            } else {
                format!(" {args}")
            }
        );
        let mut cmd = Command::new("cmd");
        cmd.args(["/D", "/S", "/C"])
            .raw_arg(&command)
            .env("PATH", new_path)
            .creation_flags(CREATE_NO_WINDOW);
        return cmd;
    }

    let mut cmd = Command::new(tool_path);
    cmd.args(args)
        .env("PATH", new_path)
        .creation_flags(CREATE_NO_WINDOW);
    cmd
}

#[cfg(target_os = "windows")]
fn run_windows_tool_command(
    tool_path: &Path,
    args: &[&str],
    new_path: &str,
) -> std::io::Result<std::process::Output> {
    build_windows_tool_command(tool_path, args, new_path).output()
}

#[cfg(target_os = "windows")]
fn run_windows_tool_version_command(
    tool_path: &Path,
    new_path: &str,
) -> std::io::Result<std::process::Output> {
    run_windows_tool_command(tool_path, &["--version"], new_path)
}

/// Probe the version of the PATH-default entry on Windows. Uses
/// `resolve_path_default` (which merges the registry PATH and filters out
/// App Execution Aliases) to get the executable the user actually runs, then
/// runs `--version` on it. Consumed by `get_single_tool_version_impl` before
/// the directory scan, so the displayed version matches what `tool` resolves
/// to in a terminal (#4701).
///
/// Returns `NotFound` when no entry is resolved on PATH, or the resolved entry
/// fails to launch (the caller falls back to `scan_cli_version` over the
/// hardcoded/registry dirs, which still reports it as broken when nothing else
/// runs); `FoundButFailed` when an entry ran but no version came out of it —
/// it exited non-zero or printed nothing (installed but not runnable).
/// That is reported as-is without falling back, so an old
/// install elsewhere cannot mask a broken default.
#[cfg(target_os = "windows")]
pub(super) fn probe_path_default_version(tool: &str) -> ShellProbe {
    let path_default = match resolve_path_default(tool, None) {
        Ok(Some(p)) => p,
        _ => return ShellProbe::NotFound(NOT_INSTALLED.to_string()),
    };
    let current_path = effective_path_string();
    match run_windows_tool_version_command(&path_default, &current_path) {
        Ok(out) => {
            let stdout = decode_command_output(&out.stdout).trim().to_string();
            let stderr = decode_command_output(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if raw.is_empty() {
                    ShellProbe::FoundButFailed(silent_version_probe_detail(out.status))
                } else {
                    ShellProbe::Found(extract_version(raw))
                }
            } else {
                let err = if stderr.is_empty() { stdout } else { stderr };
                if err.is_empty() {
                    ShellProbe::FoundButFailed(silent_version_probe_detail(out.status))
                } else {
                    ShellProbe::FoundButFailed(last_lines(err.trim(), 4))
                }
            }
        }
        // 启动不了不一定是装坏了：`where` 会列出 npm 留下的无扩展名 sh shim，同目录
        // 没有 .cmd 时它可能排在最前，而终端会跳过它、去跑后面那份。交给目录扫描判断：
        // 别处有能跑的就以它为准，都跑不起来扫描也会报「装了但跑不起来」。
        Err(_) => ShellProbe::NotFound(NOT_INSTALLED.to_string()),
    }
}

/// 扫描常见路径查找 CLI（PATH 主命令未命中时的兜底单探）。
pub(super) fn scan_cli_version(tool: &str) -> ShellProbe {
    scan_cli_version_in(tool, &build_tool_search_paths(tool))
}

pub(super) fn scan_cli_version_in(tool: &str, search_paths: &[std::path::PathBuf]) -> ShellProbe {
    #[cfg(not(target_os = "windows"))]
    use std::process::Command;

    #[cfg(target_os = "windows")]
    let current_path = effective_path_string();
    #[cfg(not(target_os = "windows"))]
    let current_path = effective_path_os().unwrap_or_default();

    // 记录"可执行文件存在、但 `--version` 非零退出"时的首个诊断信息。
    // 典型场景：工具已安装但当前环境跑不起来（如 openclaw 要求 Node v22.19+）。
    // 这类信息比笼统的 "not installed" 有用得多，循环结束未探到版本时回传。
    let mut exec_diagnostic: Option<String> = None;
    // 文件在、却启动失败或没有任何输出时的首个诊断。没有更具体的报错时才用它：
    // 文件在就不算没装（安装位置列表也会列出它）。
    let mut silent_diagnostic: Option<String> = None;

    for path in search_paths {
        #[cfg(target_os = "windows")]
        let new_path = format!("{};{}", path.display(), current_path);

        #[cfg(not(target_os = "windows"))]
        let new_path = prepend_search_dir_to_path(path, &current_path);

        for tool_path in tool_executable_candidates(tool, path) {
            // 只认文件：与工具同名的目录不是安装，否则启动失败会被当成「装了跑不起来」
            if !tool_path.is_file() {
                continue;
            }

            #[cfg(target_os = "windows")]
            let output = run_windows_tool_version_command(&tool_path, &new_path);

            #[cfg(not(target_os = "windows"))]
            let output = {
                Command::new(&tool_path)
                    .arg("--version")
                    .env("PATH", &new_path)
                    .output()
            };

            let out = match output {
                Ok(out) => out,
                Err(e) => {
                    silent_diagnostic.get_or_insert_with(|| launch_failure_detail(&tool_path, &e));
                    continue;
                }
            };
            let stdout = decode_command_output(&out.stdout).trim().to_string();
            let stderr = decode_command_output(&out.stderr).trim().to_string();
            if out.status.success() {
                let raw = if stdout.is_empty() { &stderr } else { &stdout };
                if !raw.is_empty() {
                    return ShellProbe::Found(extract_version(raw));
                }
                silent_diagnostic.get_or_insert_with(|| silent_version_probe_detail(out.status));
            } else {
                let detail = if stderr.is_empty() { stdout } else { stderr };
                let detail = detail.trim();
                if detail.is_empty() {
                    silent_diagnostic
                        .get_or_insert_with(|| silent_version_probe_detail(out.status));
                } else if exec_diagnostic.is_none() {
                    exec_diagnostic = Some(last_lines(detail, 4));
                }
            }
        }
    }

    // 有诊断 = 找到了可执行文件但拿不到版本（装了跑不起来），具体报错优先；否则视作未安装。
    match exec_diagnostic.or(silent_diagnostic) {
        Some(detail) => ShellProbe::FoundButFailed(detail),
        None => ShellProbe::NotFound(NOT_INSTALLED.to_string()),
    }
}

/// 单个工具在系统中的一处安装，用于"多处安装互相打架"的冲突诊断。
/// 字段保持 snake_case（与 `ToolVersion` 一致），前端按同名字段读取。
#[derive(Debug, serde::Serialize)]
pub struct ToolInstallation {
    /// 候选入口路径（用户实际在 PATH 里看到/输入的那个，未解析软链）。
    pub(super) path: String,
    /// `--version` 成功时解析出的版本号。
    pub(super) version: Option<String>,
    /// `--version` 是否 exit 0（装了且能在当前环境跑起来）。
    pub(super) runnable: bool,
    /// 跑不起来时的诊断信息末尾若干行。
    pub(super) error: Option<String>,
    /// `--version` 真的跑完并以非 0 退出（命令自己报错）。超时被击杀、启动失败都不算：
    /// 那只说明这次没探到，不能证明装坏了。Codex 的卸载重装自愈只认这个信号——误判
    /// 健康的一处为损坏、重装又失败时，旧版已被卸掉，用户看到「未安装」。
    #[serde(skip)]
    pub(super) exited_with_error: bool,
    /// 由路径前缀推断的安装来源（nvm/homebrew/...），驱动 UI 徽章。
    pub(super) source: String,
    /// 是否为 PATH 解析到的那处（= 命令行默认，也是升级会作用的目标）。
    pub(super) is_path_default: bool,
    /// canonicalize 解析后的真身路径(brew 形如 `Cellar/<formula>/...`、claude 原生形如
    /// `~/.local/share/claude/versions/...`),用于 `anchored_command_from_paths` 的真身
    /// 判定。`enumerate_tool_installations` 已经为去重算过一次,这里复用避免上游
    /// `installs_anchored_command` 再 canonicalize 一遍——消除冗余 syscall + 闭合
    /// "enumerate 与 anchor 看到同一真身"的一致性边界(否则两次 canonicalize 之间
    /// symlink 被换会让锚定指向不同真身)。`#[serde(skip)]` 不外露给前端。
    #[serde(skip)]
    pub(super) real: std::path::PathBuf,
}

/// 由可执行文件路径前缀推断安装来源。纯字符串匹配、无副作用。
/// 顺序敏感：Homebrew 的 Cellar / Caskroom 真身要先于通用规则命中。
pub(super) fn infer_install_source(path: &Path) -> &'static str {
    let s = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if s.contains("/.nvm/") {
        "nvm"
    } else if s.contains("/homebrew/") || s.contains("/cellar/") || s.contains("/caskroom/") {
        "homebrew"
    // `.volta` 是 macOS/Linux 默认安装(`~/.volta/bin`),`/volta/` 兜底覆盖
    // Windows 的 `%LOCALAPPDATA%\Volta\bin` / `%VOLTA_HOME%\bin`(无前导点)。
    } else if s.contains("/.volta/") || s.contains("/volta/") {
        "volta"
    } else if s.contains("fnm_multishells") {
        "fnm"
    } else if s.contains("/mise/") {
        "mise"
    } else if s.contains("/.bun/") {
        "bun"
    // pnpm 全局包目录: macOS 一般 `~/.local/share/pnpm`(已 normalize 到 `/pnpm/`)
    // 与 Windows `%LOCALAPPDATA%\pnpm` / `%PNPM_HOME%` 都命中 `/pnpm/`。
    } else if s.contains("/pnpm/") {
        "pnpm"
    } else if s.contains("/scoop/") {
        "scoop"
    } else if s.contains("/library/python")
        || s.contains("/scripts/")
        || s.contains("/site-packages/")
    {
        "pip"
    } else {
        "system"
    }
}

/// 安装来源：Homebrew 看 canonicalize 真身，node 管理器仍看 launcher。
///
/// Intel Cask 入口是 `/usr/local/bin/codex`，不含 `/homebrew/` / `/caskroom/`，
/// 真身才在 `/usr/local/Caskroom/...`。只看 launcher 会把徽章标成 `system`。
/// nvm/volta/fnm 的分类在 shim 路径上，必须继续用未解析的入口。
fn infer_install_source_for_install(launcher: &Path, real: &Path) -> &'static str {
    let real_s = real
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if real_s.contains("/cellar/") || real_s.contains("/caskroom/") {
        "homebrew"
    } else {
        infer_install_source(launcher)
    }
}

/// OpenCode Desktop 自带的命令行（`opencode-cli.exe`，在 GUI 的 `opencode.exe` 同目录或其
/// `resources/` 下）由 Desktop 应用自己更新；不和 GUI 在一起的 `opencode-cli.exe` 不算。
fn is_opencode_desktop_sidecar(path: &Path) -> bool {
    if !path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("opencode-cli.exe")
    }) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    if parent.join("opencode.exe").is_file() {
        return true;
    }
    parent
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("resources"))
        && parent
            .parent()
            .is_some_and(|root| root.join("opencode.exe").is_file())
}

/// 安装来源，在 [`infer_install_source_for_install`] 之上认出 OpenCode Desktop 自带的命令行。
fn infer_install_source_for_tool(tool: &str, launcher: &Path, real: &Path) -> &'static str {
    if tool == "opencode" && is_opencode_desktop_sidecar(launcher) {
        "desktop"
    } else {
        infer_install_source_for_install(launcher, real)
    }
}

/// 从 shell 输出里挑出第一个绝对路径行（trim 后以 `/` 开头），跳过交互式登录 shell
/// （`-lic`）里 .zshrc 打印的欢迎语/提示符等噪音。canonicalize 由调用方做（碰 FS）。
#[cfg(not(target_os = "windows"))]
fn first_abs_path_line(raw: &str) -> Option<&str> {
    raw.lines().map(str::trim).find(|l| l.starts_with('/'))
}

/// 从 `env` 输出里取 `PATH=` 那行的值。要求值以 `/` 开头——PATH 首段必是绝对路径，
/// 这条约束顺带跳过"某个多行值的环境变量恰好有一行以 `PATH=` 开头"的污染，
/// 与 `first_abs_path_line` 对交互式 shell 噪音的容错同理。
#[cfg(not(target_os = "windows"))]
fn path_line_from_env_output(raw: &str) -> Option<&str> {
    raw.lines()
        .filter_map(|line| line.strip_prefix("PATH="))
        .find(|value| value.starts_with('/'))
}

/// 合并两段 PATH：`primary` 全部保留在前，`extra` 中未出现过的段按序追加。
/// 空段直接丢弃（`a::b` 里的空段在 POSIX 下语义是"当前目录"，注入时不该带上）。
#[cfg(not(target_os = "windows"))]
pub(crate) fn merge_path_segments(primary: &str, extra: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut merged: Vec<&str> = Vec::new();
    for segment in primary.split(':').chain(extra.split(':')) {
        if segment.is_empty() || !seen.insert(segment) {
            continue;
        }
        merged.push(segment);
    }
    merged.join(":")
}

/// 用与 `resolve_path_default` 相同的登录 shell 解析用户的真实 PATH，
/// 供 `run_tool_lifecycle_silently` 注入给安装/升级脚本，也供
/// `run_detected_tool_command_with_timeout` 注入给运行时调用的 CLI。
///
/// **要解决的不对称**：探测阶段（`try_get_version` / `resolve_path_default`）跑的是
/// `$SHELL -lic`，会读 `.zshrc`/`.zprofile`，看得到 nvm / homebrew / volta；而执行阶段
/// 是非登录 `bash -c`，继承的是 launchd 给 GUI App 的 PATH，通常只有
/// `/usr/bin:/bin:/usr/sbin:/sbin`。锚定命令自己用绝对路径调用执行体，本不受这条影响
/// ——但有两类漏网：
/// 1. **执行体在内部再 spawn 第三方 CLI**：`grok update` 靠 `npm view` 查最新版本
///    （见 `grok_native_update_command`），npm 又是 `#!/usr/bin/env node` 脚本；
///    未来任何 self-update 内部调 node/git/python 同理。
/// 2. **install 分支的 `<官方 installer> || npm i -g <pkg>@latest`**：`||` 右侧是裸命令，
///    窄 PATH 下必然 exit 127，等于没有兜底。
///
/// 把执行阶段的 PATH 拉平到探测阶段的水平，一次消除这两类。
///
/// **用 `/usr/bin/env` 而不是 `echo $PATH`**：`$PATH` 在 fish 里是 list 类型，
/// `"$PATH"` 展开成空格分隔而非冒号分隔；`env` 打印的则是子进程的真实环境，
/// 任何 shell 下格式都正确。写绝对路径又绕过了 alias / function / PATH 三重不确定性
/// （交互式 shell 会加载用户 alias）。
///
/// 解析不到（含超过 `deadline`）时返回 `None`，调用方保持原有行为（不注入），
/// 不引入新的失败模式。
#[cfg(not(target_os = "windows"))]
pub(crate) fn login_shell_path(deadline: Option<CommandDeadline>) -> Option<String> {
    use std::process::{Command, Stdio};
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| is_valid_shell(s))
        .unwrap_or_else(|| "sh".to_string());
    let flag = default_flag_for_shell(&shell);
    let mut cmd = Command::new(shell);
    cmd.arg(flag)
        .arg("/usr/bin/env")
        // 与 `resolve_path_default` 相同：交互式 rc 不能读到继承来的 stdin，
        // 也不能因持有控制终端被作业控制停住。
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_child_process_group(&mut cmd);
    let child = cmd.spawn().ok()?;
    let out = wait_child_output(child, deadline).ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = decode_command_output(&out.stdout);
    Some(path_line_from_env_output(&raw)?.to_string())
}

/// [`login_shell_path`] 的进程级缓存：GUI 启动后 PATH 不会再变，解析一次、全进程共用
/// （MCP 连通性检测、Codex 模型目录探测）。限时 10 秒，免得卡住的 shell 配置让每次调用都等；
/// 第一次调用会阻塞，异步代码里放进 `spawn_blocking`。
#[cfg(not(target_os = "windows"))]
pub(crate) fn cached_login_shell_path() -> Option<String> {
    static LOGIN_SHELL_PATH: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
        login_shell_path(CommandDeadline::from_timeout(Some(
            std::time::Duration::from_secs(10),
        )))
    });
    LOGIN_SHELL_PATH.clone()
}

/// 用与 `try_get_version` 相同的登录 shell 解析 PATH 默认命中的可执行文件路径，
/// canonicalize 后作为"命令行默认 / 升级目标"的锚点（与升级会作用的那处对齐）。
#[cfg(not(target_os = "windows"))]
pub(super) fn resolve_path_default(
    tool: &str,
    deadline: Option<CommandDeadline>,
) -> Result<Option<std::path::PathBuf>, String> {
    use std::process::{Command, Stdio};

    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| is_valid_shell(s))
        .unwrap_or_else(|| "sh".to_string());
    let flag = default_flag_for_shell(&shell);
    let mut cmd = Command::new(shell);
    cmd.arg(flag)
        .arg(format!("command -v {tool}"))
        // 改 spawn 后 stdin 不再像 output() 那样默认置 null，须显式关闭：
        // 继承来的 stdin 可能是终端/管道，交互式 rc 里的读操作会永久阻塞。
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_child_process_group(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to locate {tool}: {e}"))?;
    let out = wait_child_output(child, deadline)?;
    if !out.status.success() {
        return Ok(None);
    }
    let raw = decode_command_output(&out.stdout);
    // 不能死取第一行：交互式 .zshrc 可能先打印欢迎语（如 "🚀 Welcome back"），
    // command -v 的真实路径在其后；取第一个 `/` 开头的行才稳。
    let Some(first) = first_abs_path_line(&raw) else {
        return Ok(None);
    };
    Ok(std::fs::canonicalize(first).ok())
}

#[cfg(target_os = "windows")]
fn windows_path_lookup_command(
    tool: &str,
    effective_path: &std::ffi::OsStr,
) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // Use the system copy explicitly so a project-local `where.exe` cannot
    // hijack the passive lookup before the PATH-only pattern is evaluated.
    let where_exe = PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
    )
    .join("System32")
    .join("where.exe");
    let mut command = Command::new(where_exe);
    command
        // `$PATH:pattern` is where.exe's documented environment-variable
        // search form. Unlike a bare pattern, it does not search the current
        // directory before PATH.
        .arg(format!("$PATH:{tool}"))
        .env("PATH", effective_path)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[cfg(target_os = "windows")]
pub(super) fn resolve_path_default(
    tool: &str,
    deadline: Option<CommandDeadline>,
) -> Result<Option<std::path::PathBuf>, String> {
    // Restrict `where` to the merged effective PATH. A bare `where {tool}` also
    // searches the current directory first, which would let a project-local
    // `codex.cmd` be executed by a passive version check. The `$PATH:pattern`
    // form searches only the supplied environment variable while still seeing
    // registry PATH entries lost by an in-app-update relaunch (#6061).
    let current_path = effective_path_os().unwrap_or_default();
    let child = windows_path_lookup_command(tool, &current_path)
        .spawn()
        .map_err(|e| format!("Failed to locate {tool}: {e}"))?;
    let out = wait_child_output(child, deadline)?;
    if !out.status.success() {
        return Ok(None);
    }
    let raw = decode_command_output(&out.stdout);
    // `where` lists every match on PATH in order. Skip entries that cannot be
    // safely probed (App Execution Aliases and Desktop GUI executables) and keep
    // looking so a later real CLI can still be the PATH default.
    let Some(preferred) = windows_preferred_path_default(tool, &raw) else {
        return Ok(None);
    };
    Ok(std::fs::canonicalize(preferred).ok())
}

/// 升级预检/冲突诊断的单条子进程探测预算。枚举会对每个工具开一次登录 shell、对每处
/// 安装跑一次 `--version`，任何一条挂死（.zshrc 阻塞、nvm shim 指向已删除的 node 等）
/// 都会卡住整个"全部升级"预检——到点整组击杀，该条按探测失败降级，预检继续。
pub(super) const INSTALL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// 安装枚举 `--version` 探测的基础 PATH（候选目录再前置在它前面）：登录 shell 的 PATH 在前、
/// 继承的 PATH 垫底。Finder 启动时继承的是 launchd 的窄 PATH，`#!/usr/bin/env node` 的 npm
/// 启动器只要所在目录里没有 node 就 exit 127，健康的安装被标成「无法运行」（#9810）；运行时
/// 命令（#8653）、Codex 模型目录探测（#9764）早已这样补。解析不到登录 shell 时沿用继承的 PATH。
/// 拼接用 `OsString`，继承 PATH 里的非 UTF-8 段原样保留。
#[cfg(not(target_os = "windows"))]
fn install_probe_base_path(
    login_path: Option<&str>,
    inherited: &std::ffi::OsStr,
) -> std::ffi::OsString {
    let Some(login_path) = login_path.filter(|path| !path.is_empty()) else {
        return inherited.to_os_string();
    };
    let mut path = std::ffi::OsString::from(login_path);
    if !inherited.is_empty() {
        path.push(":");
        path.push(inherited);
    }
    path
}

/// 带超时的 `--version` 探测（非 Windows）。与 `scan_cli_version` 的裸 `output()`
/// 不同：stdin 显式置 null、经 `isolate_child_process_group` 进独立会话，超时可整组击杀。
/// `new_path` 取 `&OsStr` 而非 `&str`：与 `prepend_search_dir_to_path` 同一约定，
/// 非 UTF-8 的 PATH 段不能在传递途中被有损转换丢弃。
#[cfg(not(target_os = "windows"))]
pub(super) fn run_probe_version_command(
    tool_path: &Path,
    new_path: &std::ffi::OsStr,
) -> Result<std::process::Output, String> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(tool_path);
    cmd.arg("--version")
        .env("PATH", new_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_child_process_group(&mut cmd);
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    wait_child_output(
        child,
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )
}

/// 带超时的 `--version` 探测（Windows）。命令构造与 `run_windows_tool_version_command`
/// 完全一致（.cmd/.bat 经 `cmd /C call`、其余直连），但改 spawn + `wait_child_output`：
/// 挂死的 .cmd shim / CLI 到点由 `terminate_child_tree`（taskkill /T /F）整树击杀，
/// 预检不再被单个候选卡死。scan 路径保持原 helper 不受影响。
#[cfg(target_os = "windows")]
pub(super) fn run_probe_version_command(
    tool_path: &Path,
    new_path: &str,
) -> Result<std::process::Output, String> {
    use std::process::Stdio;

    let mut cmd = build_windows_tool_command(tool_path, &["--version"], new_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    wait_child_output(
        child,
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )
}

/// 工具的一处安装位置：只定位，不跑 `--version`。
pub(super) struct LocatedInstall {
    /// 候选入口路径（mise shim 已换成真身）。
    pub(super) path: std::path::PathBuf,
    /// canonicalize 后的真身路径，去重与默认判定都按它。
    pub(super) real: std::path::PathBuf,
    pub(super) source: &'static str,
    pub(super) is_path_default: bool,
}

/// 定位工具的所有安装（不短路、不跑 `--version`）：搜索目录与 `scan_cli_version` 共用
/// `build_tool_search_paths`，按真身去重，并做和诊断 / 升级预检一样的校正——Windows 上
/// Hermes 被 PATHEXT 遮住的旧入口不算（#8839）、mise shim 换成真身（#9515）、命令行默认
/// 那处（登录 shell `command -v` / Windows `where`）标出来并排最前。
///
/// 第二个返回值是解析到的命令行默认入口真身（可能不在搜索目录里，例如只在登录 shell
/// PATH 里的 asdf、pnpm 全局安装）。
pub(super) fn locate_tool_installations(
    tool: &str,
) -> (Vec<LocatedInstall>, Option<std::path::PathBuf>) {
    let search_paths = build_tool_search_paths(tool);
    // 必须带超时：deadline 传 None 时 wait_child_output 无限等，登录 shell 一挂
    // 整个预检就永久卡死（且前端此阶段无任何反馈）。定位失败仅丢失 is_path_default
    // 标记，非致命。
    let mut path_default = resolve_path_default(
        tool,
        CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
    )
    .ok()
    .flatten();

    let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
    let mut installs: Vec<LocatedInstall> = Vec::new();
    #[cfg(target_os = "windows")]
    let pathext = std::env::var("PATHEXT").ok();

    for dir in &search_paths {
        for tool_path in tool_executable_candidates(tool, dir) {
            // 与 scan_cli_version 一致，只认文件，同名目录不算一处安装
            if !tool_path.is_file() {
                continue;
            }
            // Hermes 发布启动器时换成 .exe 不会删掉旧的 .cmd，两者同在它自己的 bin 目录，
            // 只算命令行实际命中的那个（#8839）。其他工具同目录并存可能是两处真实安装
            // （如 npm prefix 与原生安装同目录），保持分开列出。
            #[cfg(target_os = "windows")]
            if tool == "hermes" && shadowed_by_pathext_sibling(&tool_path, pathext.as_deref()) {
                continue;
            }
            // canonicalize 解析软链后去重：/opt/homebrew/bin/x → Cellar/...、nvm shim 等
            // 多个入口可能指向同一真实文件，只算一处安装。
            let real = std::fs::canonicalize(&tool_path).unwrap_or_else(|_| tool_path.clone());
            let (tool_path, real) = resolve_mise_shim_install(
                tool,
                tool_path,
                real,
                &mut path_default,
                CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
            );
            if !seen.insert(real.clone()) {
                continue;
            }
            let source = infer_install_source_for_tool(tool, &tool_path, &real);
            installs.push(LocatedInstall {
                path: tool_path,
                real,
                source,
                // 循环结束后统一标：默认入口可能在后面才被 mise shim 换成真身。
                is_path_default: false,
            });
        }
    }

    for install in &mut installs {
        install.is_path_default = path_default.as_ref() == Some(&install.real);
    }
    // PATH 默认那处排最前，UI 一眼看到"命令行默认用的是哪处"。
    installs.sort_by_key(|i| std::cmp::Reverse(i.is_path_default));
    (installs, path_default)
}

/// 枚举工具在系统中的所有安装（不短路），并对每个去重后的真实可执行文件都跑一次
/// `--version`，从而能发现"升级写入 A 处、PATH 实际用 B 处"。定位见
/// [`locate_tool_installations`]。
pub(super) fn enumerate_tool_installations(tool: &str) -> Vec<ToolInstallation> {
    let (located, _) = locate_tool_installations(tool);
    if located.is_empty() {
        return Vec::new();
    }
    #[cfg(target_os = "windows")]
    let current_path = effective_path_string();
    #[cfg(not(target_os = "windows"))]
    let current_path = install_probe_base_path(
        cached_login_shell_path().as_deref(),
        &effective_path_os().unwrap_or_default(),
    );

    located
        .into_iter()
        .map(|install| {
            // 探测时把入口所在目录前置到 PATH（mise shim 换成真身后就是真身的 bin 目录：
            // `#!/usr/bin/env node` 入口若仍走 shims 目录里的 node，会在 GUI 的工作目录下
            // 重新选版本，可能找不到或选错 Node）。
            let dir = install.path.parent().unwrap_or(Path::new(""));
            #[cfg(target_os = "windows")]
            let new_path = format!("{};{}", dir.display(), current_path);
            #[cfg(not(target_os = "windows"))]
            let new_path = prepend_search_dir_to_path(dir, &current_path);

            let output = run_probe_version_command(&install.path, &new_path);

            let (version, runnable, error, exited_with_error) = match output {
                Ok(out) if out.status.success() => {
                    let stdout = decode_command_output(&out.stdout).trim().to_string();
                    let stderr = decode_command_output(&out.stderr).trim().to_string();
                    let raw = if stdout.is_empty() { stderr } else { stdout };
                    (Some(extract_version(&raw)), true, None, false)
                }
                Ok(out) => {
                    let stderr = decode_command_output(&out.stderr).trim().to_string();
                    let stdout = decode_command_output(&out.stdout).trim().to_string();
                    let detail = if stderr.is_empty() { stdout } else { stderr };
                    let detail = detail.trim();
                    let error = if detail.is_empty() {
                        None
                    } else {
                        Some(last_lines(detail, 4))
                    };
                    (None, false, error, true)
                }
                Err(e) => (None, false, Some(e), false),
            };

            ToolInstallation {
                path: install.path.display().to_string(),
                version,
                runnable,
                error,
                exited_with_error,
                source: install.source.to_string(),
                is_path_default: install.is_path_default,
                // 复用定位时已 canonicalize 的真身，避免下游 installs_anchored_command
                // 再 canonicalize 一遍同一文件。
                real: install.real,
            }
        })
        .collect()
}

/// 「应用」页安装概览里的一处安装：只有路径和来源，不跑命令。
#[derive(Debug, serde::Serialize)]
pub(super) struct ToolInstallLocation {
    path: String,
    source: &'static str,
    is_path_default: bool,
}

/// 「应用」页打开时的安装概览：和诊断 / 升级预检同一份定位（同样的去重与默认判定），
/// 但不对每处安装跑 `--version`。命令行默认入口不在搜索目录里时（只在登录 shell PATH
/// 里的 asdf、pnpm 全局、自定义 npm prefix 等）也列出来，免得装了的工具先被归进「未安装」。
pub(super) fn list_tool_install_locations(tool: &str) -> Vec<ToolInstallLocation> {
    let (located, path_default) = locate_tool_installations(tool);
    let mut locations: Vec<ToolInstallLocation> = Vec::with_capacity(located.len() + 1);
    if let Some(default) = path_default
        .filter(|path| path.is_file() && !located.iter().any(|install| &install.real == path))
    {
        locations.push(ToolInstallLocation {
            source: infer_install_source_for_tool(tool, &default, &default),
            path: default.display().to_string(),
            is_path_default: true,
        });
    }
    locations.extend(located.into_iter().map(|install| ToolInstallLocation {
        path: install.path.display().to_string(),
        source: install.source,
        is_path_default: install.is_path_default,
    }));
    locations
}

/// mise 的 shim 是指向 mise 本体的软链，canonicalize 只能解析到 mise（Homebrew 装的 mise
/// 还会落进 Cellar 被标成 homebrew），和它调度的 `installs/node/<版本>/bin/<tool>` 算成两处
/// 安装（#9515）。换成 mise 自己说的真身（入口与真身一起换）再去重；shim 模式下登录 shell
/// 的默认入口也是 mise 本体，一并换成真身。不是 mise shim 或解析不了时原样返回。
#[cfg(not(target_os = "windows"))]
fn resolve_mise_shim_install(
    tool: &str,
    tool_path: std::path::PathBuf,
    real: std::path::PathBuf,
    path_default: &mut Option<std::path::PathBuf>,
    deadline: Option<CommandDeadline>,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let Some(target) = mise_shim_target(tool, &tool_path, &real, deadline) else {
        return (tool_path, real);
    };
    let target_real = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
    if path_default.as_ref() == Some(&real) {
        *path_default = Some(target_real.clone());
    }
    (target, target_real)
}

#[cfg(target_os = "windows")]
fn resolve_mise_shim_install(
    _tool: &str,
    tool_path: std::path::PathBuf,
    real: std::path::PathBuf,
    _path_default: &mut Option<std::path::PathBuf>,
    _deadline: Option<CommandDeadline>,
) -> (std::path::PathBuf, std::path::PathBuf) {
    (tool_path, real)
}
/// 从枚举结果里取"命令行实际命中的那处"：优先 `is_path_default`；否则（解析不到
/// PATH 默认、但只有一处）取唯一那处；多处且无默认标记 → None（无从锚定）。
///
/// 全平台共用——POSIX 和 Windows 版的 `anchored_command_from_paths` 都通过
/// `installs_anchored_command` 调它,取默认那处再 canonicalize 拿真身。
pub(super) fn default_install(installs: &[ToolInstallation]) -> Option<&ToolInstallation> {
    installs.iter().find(|i| i.is_path_default).or_else(|| {
        if installs.len() == 1 {
            installs.first()
        } else {
            None
        }
    })
}

pub(super) fn locate_default_tool(
    tool: &str,
    deadline: Option<CommandDeadline>,
) -> Result<std::path::PathBuf, String> {
    let path_default = resolve_path_default(tool, deadline)?;

    let mut seen = std::collections::HashSet::new();
    let mut candidates = Vec::new();
    for dir in build_tool_search_paths(tool) {
        for candidate in tool_executable_candidates(tool, &dir) {
            if !candidate.exists() {
                continue;
            }
            let real = std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
            if path_default.as_ref() == Some(&real) {
                return Ok(candidate);
            }
            if seen.insert(real.clone()) {
                candidates.push((candidate, real));
            }
        }
    }

    if let Some(path) = path_default {
        return Ok(path);
    }

    #[cfg(not(target_os = "windows"))]
    if candidates.len() > 1 {
        if let Some(target) = candidates
            .iter()
            .find_map(|(candidate, real)| mise_shim_target(tool, candidate, real, deadline))
        {
            return Ok(target);
        }
    }

    match candidates.as_slice() {
        [(only, _)] => Ok(only.clone()),
        [] => desktop_bundled_tool(tool).ok_or_else(|| format!("{tool} is not installed")),
        _ => Err(format!(
            "{tool} is installed but its default installation is ambiguous"
        )),
    }
}

/// 只装了 Codex 桌面版的人没有独立 CLI：运行时命令（插件页、守护进程重启）退到桌面版自带的
/// 那个（#9850），与模型目录探测用的是同一份候选。只在这里兜底、不进安装枚举：应用页的检测、
/// 升级、卸载仍只管独立安装的 CLI（#8468）。
fn desktop_bundled_tool(tool: &str) -> Option<std::path::PathBuf> {
    if tool == "codex" {
        crate::codex_config::codex_desktop_cli()
    } else {
        None
    }
}

/// mise 的 shim 是指向 mise 本体的软链，canonicalize 后和它调度的真身不是同一个文件，
/// 按真身去重时 shim 与 `installs/node/<版本>/bin/<tool>` 会被算成两处安装。登录 shell
/// 问不出默认入口时，让 mise 自己说 shim 现在指向哪个文件（按用户的 mise 配置选版本，
/// 与终端里跑这个 shim 的结果一致），直接用那个真身。不是 mise shim 或解析失败返回 None。
#[cfg(not(target_os = "windows"))]
fn mise_shim_target(
    tool: &str,
    candidate: &Path,
    real: &Path,
    deadline: Option<CommandDeadline>,
) -> Option<std::path::PathBuf> {
    use std::ffi::OsStr;
    use std::process::{Command, Stdio};

    let is_mise_shim = candidate.parent().and_then(Path::file_name) == Some(OsStr::new("shims"))
        && real.file_name() == Some(OsStr::new("mise"));
    if !is_mise_shim {
        return None;
    }

    let mut cmd = Command::new(real);
    cmd.args(["which", tool])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // GUI 进程的工作目录是 `/`；在 home 下解析，与新开终端看到的 mise 配置一致。
    cmd.current_dir(crate::config::get_home_dir());
    isolate_child_process_group(&mut cmd);
    let out = wait_child_output(cmd.spawn().ok()?, deadline).ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = decode_command_output(&out.stdout);
    let target = std::path::PathBuf::from(first_abs_path_line(&raw)?);
    target.is_file().then_some(target)
}

#[cfg(test)]
#[path = "tool_discovery/tests.rs"]
mod tests;
