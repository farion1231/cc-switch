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
    let home = dirs::home_dir().unwrap_or_default();

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

/// 枚举工具在系统中的所有安装（不短路）。与 `scan_cli_version` 共用
/// `build_tool_search_paths`，但不在首个命中处停止——而是对每个去重后的真实
/// 可执行文件都跑一次 `--version`，从而能发现"升级写入 A 处、PATH 实际用 B 处"。
pub(super) fn enumerate_tool_installations(tool: &str) -> Vec<ToolInstallation> {
    let search_paths = build_tool_search_paths(tool);
    #[cfg(target_os = "windows")]
    let current_path = effective_path_string();
    #[cfg(not(target_os = "windows"))]
    let current_path = install_probe_base_path(
        cached_login_shell_path().as_deref(),
        &effective_path_os().unwrap_or_default(),
    );
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
    let mut installs: Vec<ToolInstallation> = Vec::new();
    #[cfg(target_os = "windows")]
    let pathext = std::env::var("PATHEXT").ok();

    for dir in &search_paths {
        #[cfg(target_os = "windows")]
        let new_path = format!("{};{}", dir.display(), current_path);
        #[cfg(not(target_os = "windows"))]
        let new_path = prepend_search_dir_to_path(dir, &current_path);

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

            // mise shim 换成真身后，探测用真身所在的 bin 目录：`#!/usr/bin/env node` 入口若仍走
            // shims 目录里的 node，会在 GUI 的工作目录下重新选版本，可能找不到或选错 Node。
            #[cfg(not(target_os = "windows"))]
            let new_path = match tool_path.parent() {
                Some(parent) if parent != dir.as_path() => {
                    prepend_search_dir_to_path(parent, &current_path)
                }
                _ => new_path.clone(),
            };

            let output = run_probe_version_command(&tool_path, &new_path);

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

            let path_str = tool_path.display().to_string();
            let source = infer_install_source_for_install(&tool_path, &real);

            installs.push(ToolInstallation {
                path: path_str,
                version,
                runnable,
                error,
                exited_with_error,
                source: source.to_string(),
                // 循环结束后统一标：默认入口可能在后面才被 mise shim 换成真身。
                is_path_default: false,
                // 复用上面 line ~1357 已 canonicalize 的真身,避免下游
                // installs_anchored_command 再 canonicalize 一遍同一文件。
                real: real.clone(),
            });
        }
    }

    for install in &mut installs {
        install.is_path_default = path_default.as_ref() == Some(&install.real);
    }
    // PATH 默认那处排最前，UI 一眼看到"命令行默认用的是哪处"。
    installs.sort_by_key(|i| std::cmp::Reverse(i.is_path_default));
    installs
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
        [] => Err(format!("{tool} is not installed")),
        _ => Err(format!(
            "{tool} is installed but its default installation is ambiguous"
        )),
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
    if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
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
mod tests {
    use super::*;

    /// 探测 helper 正常路径：spawn（含 pre_exec setsid）能启动、输出能捕获。
    /// `/bin/echo --version` 在 macOS/Linux 均即刻成功退出。
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn probe_version_command_captures_healthy_tool_output() {
        let out = run_probe_version_command(Path::new("/bin/echo"), std::ffi::OsStr::new(""))
            .expect("probe of /bin/echo should succeed");
        assert!(out.status.success());
    }

    #[test]
    fn legacy_kimi_native_update_reruns_the_installer() {
        let install = |path: &str, version: &str| ToolInstallation {
            path: path.to_string(),
            version: Some(version.to_string()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: infer_install_source(Path::new(path)).to_string(),
            is_path_default: true,
            real: PathBuf::from(path),
        };
        let (path, root) = if cfg!(target_os = "windows") {
            (
                r"C:\Users\sc\.kimi-code\bin\kimi.exe",
                r"C:\Users\sc\.kimi-code",
            )
        } else {
            ("/home/me/.kimi-code/bin/kimi", "/home/me/.kimi-code")
        };

        // 0.43.0 之前 `upgrade --yes` 在非 TTY 下只打印提示就 exit 0（#9032）。
        assert_eq!(
            installs_anchored_command("kimi", &[install(path, "0.23.1")]),
            Some(kimi_installer_update_command(root))
        );
        // 新版照旧走 CLI 自升级。
        let current = installs_anchored_command("kimi", &[install(path, "2.4.0")]).unwrap();
        assert!(current.contains("upgrade --yes"), "{current}");
        assert_eq!(
            installs_anchored_command("kimi", &[install(path, "0.43.0")]),
            Some(current)
        );
        // Volta 的 shim 也是 `bin/kimi`，必须交给 Volta，不能让 installer 覆盖。
        #[cfg(not(target_os = "windows"))]
        {
            let volta =
                installs_anchored_command("kimi", &[install("/home/me/.volta/bin/kimi", "0.23.1")])
                    .unwrap();
            assert!(
                volta.ends_with("volta install @moonshot-ai/kimi-code"),
                "{volta}"
            );
        }
    }

    /// 锚定落空后的兜底:认不出渠道的原生可执行文件不再退回裸 `npm i -g`(#7650),
    /// 脚本 shim / node_modules 里的入口 / 定位不到默认那处仍保持旧的静态命令。
    mod unmanaged_native_update {
        use super::super::*;

        const MACH_O_64: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];

        /// 在 tempdir 的 `rel` 位置写入 `bytes`,返回指向它的安装记录。TempDir 须保活。
        fn installed(
            dir: &tempfile::TempDir,
            rel: &str,
            bytes: &[u8],
            is_default: bool,
        ) -> ToolInstallation {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            ToolInstallation {
                path: path.to_string_lossy().to_string(),
                version: Some("1.0.0".to_string()),
                runnable: true,
                error: None,
                exited_with_error: false,
                source: infer_install_source(&path).to_string(),
                is_path_default: is_default,
                real: path,
            }
        }

        #[test]
        fn native_binary_outside_node_modules_is_unmanaged() {
            let dir = tempfile::tempdir().unwrap();
            let gemini = installed(&dir, "nix-profile/bin/gemini", &MACH_O_64, true);
            assert_eq!(
                resolve_update_command("gemini", &[gemini]),
                UpdateCommand::Unmanaged
            );
            let pi = installed(&dir, "bin/pi", b"\x7fELF\x02\x01\x01", true);
            assert_eq!(
                resolve_update_command("pi", &[pi]),
                UpdateCommand::Unmanaged
            );
        }

        #[test]
        fn script_shim_keeps_static_npm_command() {
            // asdf / nodenv 的 shim 是脚本,背后可能正是 npm 全局包,不能拒绝。
            let dir = tempfile::tempdir().unwrap();
            let shim = installed(
                &dir,
                ".asdf/shims/gemini",
                b"#!/usr/bin/env bash\nexec asdf exec gemini \"$@\"\n",
                true,
            );
            assert_eq!(
                resolve_update_command("gemini", &[shim]),
                UpdateCommand::Static(static_fallback_command("gemini"))
            );
        }

        #[test]
        fn native_binary_inside_node_modules_keeps_static_npm_command() {
            // 有的 npm 包把原生二进制放在包内,真身仍在 node_modules 下,归 npm 管。
            let dir = tempfile::tempdir().unwrap();
            let bundled = installed(
                &dir,
                "prefix/lib/node_modules/@google/gemini-cli/bin/gemini",
                &MACH_O_64,
                true,
            );
            assert_eq!(
                resolve_update_command("gemini", &[bundled]),
                UpdateCommand::Static(static_fallback_command("gemini"))
            );
        }

        #[test]
        fn unknown_default_keeps_static_command() {
            // 多处安装又定位不到默认那处:不知道命令行用的是哪份,维持旧行为(前端会弹确认)。
            let dir = tempfile::tempdir().unwrap();
            let a = installed(&dir, "a/bin/gemini", &MACH_O_64, false);
            let b = installed(&dir, "b/bin/gemini", &MACH_O_64, false);
            assert_eq!(
                resolve_update_command("gemini", &[a, b]),
                UpdateCommand::Static(static_fallback_command("gemini"))
            );
        }

        #[test]
        fn native_executable_magic() {
            let dir = tempfile::tempdir().unwrap();
            let cases: [(&str, &[u8], bool); 7] = [
                ("elf", b"\x7fELF\x02", true),
                ("macho", &MACH_O_64, true),
                ("fat", &[0xca, 0xfe, 0xba, 0xbe], true),
                ("pe.exe", b"MZ\x90\x00", true),
                ("script", b"#!/bin/sh\n", false),
                ("cmd-shim.cmd", b"@ECHO off\r\n", false),
                ("short", b"MZ", false),
            ];
            for (name, bytes, expected) in cases {
                let path = dir.path().join(name);
                std::fs::write(&path, bytes).unwrap();
                assert_eq!(is_native_executable(&path), expected, "{name}");
            }
            assert!(!is_native_executable(&dir.path().join("missing")));
        }
    }

    /// `infer_install_source` 是判定锚定 idiom 的入口——nvm/homebrew/volta/pnpm/...
    /// 各对应不同的升级命令形态。函数内部已 `replace('\\','/').to_ascii_lowercase()`
    /// 归一化,Windows 反斜杠 + 大小写差异在此处不需要分平台。这里固化"哪条路径
    /// 算哪种来源"的归类断言,避免未来调整子串顺序时静默改变分类。
    mod install_source_classification {
        use super::super::*;
        use std::path::Path;

        #[test]
        fn macos_homebrew_caskroom_is_homebrew() {
            assert_eq!(
                infer_install_source(Path::new("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex")),
                "homebrew"
            );
            // Intel prefix 不含 `/homebrew/`，必须靠 `/caskroom/` 本身命中。
            assert_eq!(
                infer_install_source(Path::new("/usr/local/Caskroom/codex/0.146.0/bin/codex")),
                "homebrew"
            );
        }

        #[test]
        fn intel_cask_launcher_uses_resolved_target() {
            // 标准 Intel cask：PATH 入口是 `/usr/local/bin/codex`，真身才在 Caskroom。
            // 只看 launcher 会落到 `system`，徽章和冲突诊断都会错。
            assert_eq!(
                infer_install_source(Path::new("/usr/local/bin/codex")),
                "system"
            );
            assert_eq!(
                infer_install_source_for_install(
                    Path::new("/usr/local/bin/codex"),
                    Path::new("/usr/local/Caskroom/codex/0.146.0/bin/codex"),
                ),
                "homebrew"
            );
            // nvm shim 仍按 launcher 分类，不能被真身路径抢走。
            assert_eq!(
                infer_install_source_for_install(
                    Path::new("/Users/me/.nvm/versions/node/v22.0.0/bin/codex"),
                    Path::new(
                        "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@openai/codex/bin/codex.js"
                    ),
                ),
                "nvm"
            );
        }

        #[test]
        fn macos_volta_with_dot_prefix() {
            assert_eq!(
                infer_install_source(Path::new("/Users/me/.volta/bin/codex")),
                "volta"
            );
        }

        #[test]
        fn windows_volta_localappdata_no_dot() {
            // `%LOCALAPPDATA%\Volta\bin\codex.exe` —— 没有前导点,靠兜底的 `/volta/`
            // 命中(归一化后小写)。如果只识别 `/.volta/`,Windows 这一类会落到 system。
            assert_eq!(
                infer_install_source(Path::new(
                    "C:\\Users\\me\\AppData\\Local\\Volta\\bin\\codex.exe"
                )),
                "volta"
            );
        }

        #[test]
        fn windows_pnpm_localappdata() {
            // `%LOCALAPPDATA%\pnpm\codex.cmd` —— pnpm 全局 bin 目录,识别为 pnpm 后
            // 锚定命令走 `pnpm add -g <pkg>@latest`,而不是 sibling npm。
            assert_eq!(
                infer_install_source(Path::new("C:\\Users\\me\\AppData\\Local\\pnpm\\codex.cmd")),
                "pnpm"
            );
        }

        #[test]
        fn windows_nvm_falls_back_to_system() {
            // nvm-windows 安装的工具路径不含 `.nvm`(它通常装在 `%APPDATA%\nvm` 或
            // `C:\Program Files\nodejs` symlink),刻意不识别成专属 source——锚定层
            // 会按 system → sibling npm.cmd 处理,跟 nvm-windows 的实际 idiom 一致
            // (它的全局包就是当前选中的 node 的 npm 装的)。
            assert_eq!(
                infer_install_source(Path::new(
                    "C:\\Users\\me\\AppData\\Roaming\\nvm\\v22.0.0\\codex.cmd"
                )),
                "system"
            );
        }

        #[test]
        fn windows_scoop_still_identified() {
            // Scoop 已有 `/scoop/` 分支;我们的 6 个工具都不是 scoop formula,所以这条
            // 实际不影响锚定决策(锚定层会用 sibling npm.cmd),但归类保留方便未来。
            assert_eq!(
                infer_install_source(Path::new("C:\\Users\\me\\scoop\\shims\\codex.cmd")),
                "scoop"
            );
        }
    }

    /// 锚定升级命令生成：用真实勘察到的安装路径固化为回归断言——
    /// 一台机器上 4 个工具恰好对应 4 种升级方式（原生 self-update / brew / nvm npm /
    /// homebrew npm），任何改动若打破其中一种都会立刻被这些用例拦下。
    #[cfg(not(target_os = "windows"))]
    mod anchored_upgrade {
        use super::super::*;
        use std::path::Path;

        fn inst(path: &str, is_default: bool) -> ToolInstallation {
            ToolInstallation {
                path: path.to_string(),
                version: None,
                runnable: true,
                error: None,
                exited_with_error: false,
                source: infer_install_source(Path::new(path)).to_string(),
                is_path_default: is_default,
                // 测试场景下不需要走 fs canonicalize——POSIX 锚定测试关心的是
                // path/real 都被传给 anchored_command_from_paths 的纯字符串判定,
                // 已有用例(brew_formula_extraction / claude_native_*)是直接
                // 调 anchored_command_from_paths,不通过 installs_anchored_command,
                // 这里 real 是给上层 default_install + read 用,填同值即可。
                real: std::path::PathBuf::from(path),
            }
        }

        #[test]
        fn claude_native_installer_uses_self_update() {
            // ~/.local/bin/claude → 真身在 ~/.local/share/claude/versions/,自带 self-update;
            // 它不归 npm 管,且在 PATH 里比 nvm/homebrew 更靠前,用 npm 升级纯属白装。
            // **绝对路径调用 launcher** 避免 GUI 非登录 `bash -c` 时 PATH 没有
            // ~/.local/bin 导致 `claude: not found`(exit 127)而失败。
            let cmd = anchored_command_from_paths(
                "claude",
                "/Users/me/.local/bin/claude",
                "/Users/me/.local/share/claude/versions/2.1.146",
            );
            assert_eq!(cmd.as_deref(), Some("/Users/me/.local/bin/claude update"));
        }

        #[test]
        fn grok_native_installer_uses_self_update_with_installer_fallback() {
            // ~/.grok/bin/grok is a launcher symlink into ~/.grok/downloads.
            // Updating it through npm would create or mutate a different install.
            let cmd = anchored_command_from_paths(
                "grok",
                "/Users/me/.grok/bin/grok",
                "/Users/me/.grok/downloads/grok-macos-aarch64",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    format!(
                        "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/.grok/bin/grok update || {GROK_INSTALL_UNIX}"
                    )
                    .as_str()
                )
            );
        }

        #[test]
        fn grok_native_update_falls_back_to_installer_not_npm() {
            // 反向锁定:`grok update` 内部靠 `npm view` + `npm i -g` 完成升级,GUI 的窄
            // PATH 下会 ENOENT。fallback 必须是官方 installer —— 换成 `npm i -g` 就与
            // primary 同源(无 node / 镜像缺 tarball 时一起失败),`||` 形同虚设。
            let cmd = anchored_command_from_paths(
                "grok",
                "/Users/me/.grok/bin/grok",
                "/Users/me/.grok/downloads/grok-macos-aarch64",
            )
            .expect("native grok should anchor");
            let (_, fallback) = cmd.split_once(" || ").unwrap();
            assert!(fallback.contains("x.ai/cli/install.sh"), "{cmd}");
            assert!(
                !fallback.contains("npm"),
                "npm must not be the fallback: {cmd}"
            );
        }

        #[test]
        fn grok_custom_bin_dir_is_native_when_target_is_official_download() {
            let cmd = anchored_command_from_paths(
                "grok",
                "/Users/me/bin/grok",
                "/Users/me/.grok/downloads/grok-macos-aarch64",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    format!(
                        "npm_config_allow_scripts=@xai-official/grok npm_config_ignore_scripts=false npm_config_include=optional /Users/me/bin/grok update || {GROK_INSTALL_UNIX}"
                    )
                    .as_str()
                )
            );
        }

        #[test]
        fn gemini_homebrew_formula_uses_brew_upgrade() {
            // /opt/homebrew/bin/gemini → Cellar/gemini-cli/...:是 brew formula 而非 npm 全局包,
            // 且 formula 名(gemini-cli) ≠ npm 包名(@google/gemini-cli)。
            // **brew 与 formula 入口同目录**,用 `<dir>/brew` 绝对路径调用,避免 GUI
            // 非登录 `bash -c` 时 PATH 没有 /opt/homebrew/bin 导致 `brew: not found`。
            let cmd = anchored_command_from_paths(
                "gemini",
                "/opt/homebrew/bin/gemini",
                "/opt/homebrew/Cellar/gemini-cli/0.13.0/libexec/lib/node_modules/@google/gemini-cli/dist/index.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/opt/homebrew/bin/brew upgrade gemini-cli")
            );
        }

        #[test]
        fn codex_homebrew_formula_uses_brew_not_self_update() {
            // Homebrew formula 归 brew 管理;即使 Codex 有 self-update,也不先改动
            // Cellar 内的安装内容。
            let cmd = anchored_command_from_paths(
                "codex",
                "/opt/homebrew/bin/codex",
                "/opt/homebrew/Cellar/codex/1.2.3/bin/codex",
            );
            assert_eq!(cmd.as_deref(), Some("/opt/homebrew/bin/brew upgrade codex"));
        }

        #[test]
        fn codex_homebrew_cask_uses_brew_upgrade_cask() {
            // `/opt/homebrew/bin/codex` → Caskroom/codex/...:是 brew cask 而非 npm 全局包。
            // 若误走 sibling `npm i -g @openai/codex`，npm bin-links 会因
            // `/opt/homebrew/bin/codex` 不属于它而 EEXIST（#6562）。
            let cmd = anchored_command_from_paths(
                "codex",
                "/opt/homebrew/bin/codex",
                "/opt/homebrew/Caskroom/codex/0.146.0/bin/codex",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/opt/homebrew/bin/brew upgrade --cask codex")
            );
        }

        #[test]
        fn intel_homebrew_cask_without_homebrew_prefix_still_uses_brew() {
            // Intel 默认 prefix 是 `/usr/local`，Caskroom 路径不含 `/homebrew/`。
            // 真身解析必须靠 `Caskroom` 段本身，不能依赖 prefix 子串。
            let cmd = anchored_command_from_paths(
                "codex",
                "/usr/local/bin/codex",
                "/usr/local/Caskroom/codex/0.146.0/bin/codex",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/usr/local/bin/brew upgrade --cask codex")
            );
        }

        #[test]
        fn gemini_nvm_anchors_to_npm_without_cli_update() {
            let cmd = anchored_command_from_paths(
                "gemini",
                "/Users/me/.nvm/versions/node/v22.14.0/bin/gemini",
                "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@google/gemini-cli/dist/index.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    "PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @google/gemini-cli@latest"
                )
            );
        }

        #[test]
        fn grok_nvm_anchors_to_npm_without_cli_update() {
            let cmd = anchored_command_from_paths(
                "grok",
                "/Users/me/.nvm/versions/node/v22.14.0/bin/grok",
                "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@xai-official/grok/bin/grok",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    "PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @xai-official/grok@latest --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok"
                )
            );
        }

        #[test]
        fn mcode_script_install_reruns_official_installer_in_place() {
            // `~/.minimax-code/bin/mcode` 是官方 installer 的 sh launcher，不归全局 npm 管；
            // `mcode update` 非 TTY 下不安装，所以改为带 MCODE_INSTALL_DIR 重跑 installer。
            let expected =
                format!("MCODE_INSTALL_DIR='/Users/me/.minimax-code' {MCODE_INSTALL_UNIX}");
            for bin_path in [
                "/Users/me/.minimax-code/bin/mcode",
                "/Users/me/.local/bin/mcode",
            ] {
                let cmd = anchored_command_from_paths(
                    "mcode",
                    bin_path,
                    "/Users/me/.minimax-code/bin/mcode",
                );
                assert_eq!(cmd.as_deref(), Some(expected.as_str()));
            }
        }

        #[test]
        fn mcode_nvm_anchors_to_npm_with_allow_scripts() {
            let cmd = anchored_command_from_paths(
                "mcode",
                "/Users/me/.nvm/versions/node/v22.19.0/bin/mcode",
                "/Users/me/.nvm/versions/node/v22.19.0/lib/node_modules/@minimax-ai/code/cli.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    "PATH='/Users/me/.nvm/versions/node/v22.19.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.19.0/bin/npm i -g @minimax-ai/code@latest --ignore-scripts=false --include=optional \"--allow-scripts=@minimax-ai/code,better-sqlite3\""
                )
            );
        }

        #[test]
        fn claude_npm_sources_scope_postinstall_settings_to_primary_and_full_fallback() {
            for (path, dir) in [
                (
                    "/Users/me/.nvm/versions/node/v22/bin/claude",
                    "/Users/me/.nvm/versions/node/v22/bin",
                ),
                (
                    "/Users/me/.local/share/fnm_multishells/12345_abc/bin/claude",
                    "/Users/me/.local/share/fnm_multishells/12345_abc/bin",
                ),
                (
                    "/Users/me/.local/share/mise/installs/node/22/bin/claude",
                    "/Users/me/.local/share/mise/installs/node/22/bin",
                ),
                ("/opt/homebrew/bin/claude", "/opt/homebrew/bin"),
            ] {
                let mut install = inst(path, true);
                install.real = PathBuf::from(format!(
                    "{}/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                    dir.trim_end_matches("/bin")
                ));
                let command = installs_anchored_command("claude", &[install]).unwrap();
                let (primary, fallback) = command.split_once(" || ").unwrap();
                assert_eq!(
                    primary,
                    format!(
                        "npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional {path} update"
                    )
                );
                assert_eq!(
                    fallback,
                    format!(
                        "PATH='{dir}':\"$PATH\" {dir}/npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"
                    )
                );
            }
        }

        #[test]
        fn opencode_npm_sources_scope_postinstall_settings_to_primary_and_fallback() {
            // opencode-ai's postinstall lays the platform binary over the text
            // placeholder at bin/opencode.exe; npm 12 blocks it by default (#8863).
            let dir = "/Users/me/.nvm/versions/node/v22/bin";
            let path = format!("{dir}/opencode");
            let mut install = inst(&path, true);
            install.real = PathBuf::from(
                "/Users/me/.nvm/versions/node/v22/lib/node_modules/opencode-ai/bin/opencode.exe",
            );
            let command = installs_anchored_command("opencode", &[install]).unwrap();
            let (primary, fallback) = command.split_once(" || ").unwrap();
            assert_eq!(
                primary,
                format!(
                    "npm_config_allow_scripts=opencode-ai npm_config_ignore_scripts=false npm_config_include=optional {path} upgrade"
                )
            );
            assert_eq!(
                fallback,
                format!(
                    "PATH='{dir}':\"$PATH\" {dir}/npm i -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai"
                )
            );
            assert_eq!(
                anchored_command_from_paths(
                    "opencode",
                    "/usr/local/bin/opencode",
                    "/usr/local/lib/node_modules/opencode-ai/bin/opencode.exe",
                ),
                Some(
                    "npm_config_allow_scripts=opencode-ai npm_config_ignore_scripts=false npm_config_include=optional /usr/local/bin/opencode upgrade"
                        .to_string()
                )
            );
        }

        #[test]
        fn claude_system_npm_prefixes_scope_postinstall_settings_without_fallback() {
            // Launchers classified as `system` have no sibling-npm fallback, so the
            // primary update is their only chance to keep npm 12 from breaking Claude.
            for (path, real) in [
                (
                    "/usr/local/bin/claude",
                    "/usr/local/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                ),
                (
                    "/Users/me/.npm-global/bin/claude",
                    "/Users/me/.npm-global/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
                ),
                (
                    "/usr/bin/claude",
                    "/usr/lib/node_modules/@anthropic-ai/claude-code/cli.js",
                ),
            ] {
                assert_eq!(
                    anchored_command_from_paths("claude", path, real),
                    Some(format!(
                        "npm_config_allow_scripts=@anthropic-ai/claude-code npm_config_ignore_scripts=false npm_config_include=optional {path} update"
                    ))
                );
            }
        }

        #[test]
        fn claude_other_install_sources_keep_their_update_policy() {
            for (path, real, expected) in [
                (
                    "/Users/me/.nvm/versions/node/v22/bin/claude",
                    "/Users/me/.local/share/claude/versions/2.1.146",
                    "/Users/me/.nvm/versions/node/v22/bin/claude update",
                ),
                (
                    "/opt/homebrew/bin/claude",
                    "/opt/homebrew/Cellar/claude-code/2.1.146/bin/claude",
                    "/opt/homebrew/bin/brew upgrade claude-code",
                ),
                (
                    "/Users/me/.volta/bin/claude",
                    "/Users/me/.volta/bin/claude",
                    "/Users/me/.volta/bin/claude update || /Users/me/.volta/bin/volta install @anthropic-ai/claude-code",
                ),
                (
                    "/Users/me/.bun/bin/claude",
                    "/Users/me/.bun/bin/claude",
                    "/Users/me/.bun/bin/claude update || /Users/me/.bun/bin/bun add -g @anthropic-ai/claude-code@latest",
                ),
                (
                    "/usr/local/bin/claude",
                    "/usr/local/bin/claude",
                    "/usr/local/bin/claude update",
                ),
            ] {
                assert_eq!(
                    anchored_command_from_paths("claude", path, real).as_deref(),
                    Some(expected)
                );
            }
        }

        #[test]
        fn claude_updater_child_inherits_scoped_settings_without_parent_leakage() {
            use std::os::unix::fs::PermissionsExt;
            use std::process::Command;

            let temp = tempfile::tempdir().expect("temp dir should be created");
            let bin = temp.path().join("home dir/.nvm/versions/node/v22/bin");
            std::fs::create_dir_all(&bin).unwrap();
            let claude = bin.join("claude");
            let npm = bin.join("npm");
            std::fs::write(
                &claude,
                "#!/bin/sh\n[ \"$1\" = update ] || exit 99\nnpm install -g @anthropic-ai/claude-code@latest\nexit \"$TEST_CLAUDE_EXIT\"\n",
            )
            .unwrap();
            std::fs::write(
                &npm,
                "#!/bin/sh\nprintf '%s|%s|%s|%s\n' \"\u{24}{npm_config_allow_scripts-unset}\" \"\u{24}{npm_config_ignore_scripts-unset}\" \"\u{24}{npm_config_include-unset}\" \"\u{24}*\"\n[ \"$1\" != i ] || exit \"$TEST_FALLBACK_EXIT\"\n",
            )
            .unwrap();
            for executable in [&claude, &npm] {
                std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755))
                    .unwrap();
            }

            let mut install = inst(&claude.to_string_lossy(), true);
            install.real = temp.path().join(
                "home dir/.nvm/versions/node/v22/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe",
            );
            let command = installs_anchored_command("claude", &[install]).unwrap();
            let script = format!(
                "set -e\n{command}\nprintf 'after|%s|%s|%s\n' \"$npm_config_allow_scripts\" \"$npm_config_ignore_scripts\" \"$npm_config_include\""
            );
            let primary = "@anthropic-ai/claude-code|false|optional|install -g @anthropic-ai/claude-code@latest\n";
            let fallback = "previous-package|true|prod|i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code\n";
            for (primary_exit, fallback_exit, expected_code, expected_output) in [
                (
                    "0",
                    "0",
                    0,
                    format!("{primary}after|previous-package|true|prod\n"),
                ),
                (
                    "1",
                    "0",
                    0,
                    format!("{primary}{fallback}after|previous-package|true|prod\n"),
                ),
                ("1", "42", 42, format!("{primary}{fallback}")),
            ] {
                let output = Command::new("/bin/bash")
                    .args(["-c", &script])
                    .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
                    .env("npm_config_allow_scripts", "previous-package")
                    .env("npm_config_ignore_scripts", "true")
                    .env("npm_config_include", "prod")
                    .env("TEST_CLAUDE_EXIT", primary_exit)
                    .env("TEST_FALLBACK_EXIT", fallback_exit)
                    .output()
                    .expect("Claude update chain should start");
                assert_eq!(
                    output.status.code(),
                    Some(expected_code),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert_eq!(String::from_utf8_lossy(&output.stdout), expected_output);
            }
        }

        #[test]
        fn codex_nvm_anchors_to_that_npm() {
            // Codex 不走 self-update（`codex update` 在 npm 安装上只是裸 `npm install -g`，
            // 却会假成功掩盖平台二进制漏装）——直接锚定到同一个 node 的 npm，而非 PATH
            // 第一个 npm。损坏时的 uninstall+install 自愈见 codex_missing_platform_binary_*。
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/me/.nvm/versions/node/v22.14.0/bin/codex",
                "/Users/me/.nvm/versions/node/v22.14.0/lib/node_modules/@openai/codex/bin/codex.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
            );
        }

        #[test]
        fn homebrew_npm_global_package_anchors_not_brew() {
            // openclaw 装在 Homebrew node 的全局目录(lib/node_modules，非 Cellar)：
            // 是 npm 全局包，官方 update 失败后走 npm 锚定而非 brew upgrade。
            let cmd = anchored_command_from_paths(
                "openclaw",
                "/opt/homebrew/bin/openclaw",
                "/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/opt/homebrew/bin/openclaw update --yes || PATH='/opt/homebrew/bin':\"$PATH\" /opt/homebrew/bin/npm i -g openclaw@latest")
            );
        }

        #[test]
        fn volta_self_update_chain_anchors_to_volta() {
            // `~/.volta/bin` 通常不在 GUI 非登录 `bash -c` 的 PATH 里,且用户可能
            // PATH 上还有另一份 volta → 必须绝对路径锚定到命令行命中的这一份。
            // 用 openclaw（仍在 prefers_official_update）覆盖 volta 分支的 self-update 链;
            // codex 已改为不 self-update（见 codex_volta_anchors_to_volta_install）。
            let cmd = anchored_command_from_paths(
                "openclaw",
                "/Users/me/.volta/bin/openclaw",
                "/Users/me/.volta/tools/image/packages/openclaw/lib/node_modules/openclaw",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/Users/me/.volta/bin/openclaw update --yes || /Users/me/.volta/bin/volta install openclaw")
            );
        }

        #[test]
        fn codex_volta_anchors_to_volta_install() {
            // codex 锚定到命令行命中的那份 volta，但不 self-update：纯 `volta install`。
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/me/.volta/bin/codex",
                "/Users/me/.volta/tools/image/packages/codex/lib/node_modules/@openai/codex",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/Users/me/.volta/bin/volta install @openai/codex")
            );
        }

        #[test]
        fn bun_uses_bun_add() {
            // OpenCode 先跑官方 upgrade;失败后 bun 同 volta:绝对路径写回原安装源。
            let cmd = anchored_command_from_paths(
                "opencode",
                "/Users/me/.bun/bin/opencode",
                "/Users/me/.bun/install/global/node_modules/opencode-ai/bin/opencode",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/Users/me/.bun/bin/opencode upgrade || /Users/me/.bun/bin/bun add -g opencode-ai@latest")
            );
        }

        #[test]
        fn omp_updates_through_its_own_updater_with_source_specific_fallbacks() {
            // Homebrew owns its formula even though `omp update` would also delegate.
            assert_eq!(
                anchored_command_from_paths(
                    "omp",
                    "/opt/homebrew/bin/omp",
                    "/opt/homebrew/Cellar/omp/14.1.0/bin/omp",
                )
                .as_deref(),
                Some("/opt/homebrew/bin/brew upgrade omp")
            );
            // A bun global keeps bun as the fallback.
            assert_eq!(
                anchored_command_from_paths(
                    "omp",
                    "/Users/me/.bun/bin/omp",
                    "/Users/me/.bun/install/global/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
                )
                .as_deref(),
                Some("/Users/me/.bun/bin/omp update || /Users/me/.bun/bin/bun add -g @oh-my-pi/pi-coding-agent@latest")
            );
            // The installer's prebuilt binary updates itself; no npm fallback is invented.
            assert_eq!(
                anchored_command_from_paths(
                    "omp",
                    "/Users/me/.local/bin/omp",
                    "/Users/me/.local/bin/omp",
                )
                .as_deref(),
                Some("/Users/me/.local/bin/omp update")
            );
            // npm and other node managers cannot run omp, so they never become a fallback.
            assert_eq!(
                anchored_command_from_paths(
                    "omp",
                    "/Users/me/.nvm/versions/node/v22.0.0/bin/omp",
                    "/Users/me/.nvm/versions/node/v22.0.0/lib/node_modules/@oh-my-pi/pi-coding-agent/src/cli.ts",
                )
                .as_deref(),
                Some("/Users/me/.nvm/versions/node/v22.0.0/bin/omp update")
            );
            // `omp update` exits 0 on Nix without updating: leave it to the user.
            assert_eq!(
                anchored_command_from_paths(
                    "omp",
                    "/Users/me/.nix-profile/bin/omp",
                    "/nix/store/abc-omp-14.1.0/bin/omp",
                ),
                None
            );
        }

        #[test]
        fn volta_path_with_space_is_quoted() {
            // volta 分支用 `<dir>/volta`,目录含空格时同样要 POSIX 引号包裹。
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/my name/.volta/bin/codex",
                "/Users/my name/.volta/tools/image/packages/codex/lib/node_modules/@openai/codex",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("'/Users/my name/.volta/bin/volta' install @openai/codex")
            );
        }

        #[test]
        fn bun_path_with_space_is_quoted() {
            // bun 分支与 volta 共享 sibling_bin + quote_path_if_spaced,
            // 这条用例锁住 `bun add -g` 命令头部的引号包裹形态。
            let cmd = anchored_command_from_paths(
                "opencode",
                "/Users/my name/.bun/bin/opencode",
                "/Users/my name/.bun/install/global/node_modules/opencode-ai/bin/opencode",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("'/Users/my name/.bun/bin/opencode' upgrade || '/Users/my name/.bun/bin/bun' add -g opencode-ai@latest")
            );
        }

        #[test]
        fn hermes_uses_cli_update_anchor() {
            // Hermes 自带 `hermes update`;锚定到命令行默认那处 CLI,避免 cc-switch 猜
            // 系统 Python/pip 时撞上 Python >=3.11 或 pyenv shim 问题。
            let cmd = anchored_command_from_paths(
                "hermes",
                "/usr/local/bin/hermes",
                "/usr/local/bin/hermes",
            );
            assert_eq!(cmd.as_deref(), Some("/usr/local/bin/hermes update"));
        }

        #[test]
        fn agy_updates_itself_even_from_a_homebrew_cask() {
            // The cask is auto_updates, so `brew upgrade` skips it; agy replaces its own binary.
            for real in [
                "/opt/homebrew/Caskroom/antigravity-cli/1.3.4,5524158944247808/antigravity",
                "/opt/homebrew/bin/agy",
            ] {
                let cmd = anchored_command_from_paths("agy", "/opt/homebrew/bin/agy", real);
                assert_eq!(cmd.as_deref(), Some("/opt/homebrew/bin/agy update"));
            }
        }

        #[test]
        fn opencode_native_install_uses_cli_upgrade_without_package_fallback() {
            // opencode install.sh 装到 ~/.opencode/bin（独立二进制、无同级 npm）：
            // 不能锚定到 `<dir>/npm`（必失败），但可以锚定到 CLI 自身跑官方 upgrade。
            let cmd = anchored_command_from_paths(
                "opencode",
                "/Users/me/.opencode/bin/opencode",
                "/Users/me/.opencode/bin/opencode",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("/Users/me/.opencode/bin/opencode upgrade")
            );
        }

        #[test]
        fn go_bin_opencode_uses_cli_upgrade_without_package_fallback() {
            // ~/go/bin 同理：无同级 npm，但 OpenCode 官方 upgrade 可由 CLI 自己处理。
            let cmd = anchored_command_from_paths(
                "opencode",
                "/Users/me/go/bin/opencode",
                "/Users/me/go/bin/opencode",
            );
            assert_eq!(cmd.as_deref(), Some("/Users/me/go/bin/opencode upgrade"));
        }

        #[test]
        fn fnm_install_anchors_to_that_npm() {
            // fnm 是自带同级 npm 的 node 管理器 → 锚定到那处的 npm。
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/me/.local/share/fnm_multishells/12345_abc/bin/codex",
                "/Users/me/.local/share/fnm_multishells/12345_abc/lib/node_modules/@openai/codex/bin/codex.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some(
                    "PATH='/Users/me/.local/share/fnm_multishells/12345_abc/bin':\"$PATH\" /Users/me/.local/share/fnm_multishells/12345_abc/bin/npm i -g @openai/codex@latest"
                )
            );
        }

        #[test]
        fn path_with_space_is_quoted() {
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/my name/.nvm/versions/node/v22/bin/codex",
                "/Users/my name/.nvm/versions/node/v22/lib/node_modules/@openai/codex/bin/codex.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("PATH='/Users/my name/.nvm/versions/node/v22/bin':\"$PATH\" '/Users/my name/.nvm/versions/node/v22/bin/npm' i -g @openai/codex@latest")
            );
        }

        #[test]
        fn npm_anchor_supplies_sibling_node_to_env_shebang() {
            use std::os::unix::fs::PermissionsExt;
            use std::process::Command;

            let temp = tempfile::tempdir().expect("temp dir should be created");
            let bin = temp.path().join("home dir/.nvm/versions/node/v22.14.0/bin");
            std::fs::create_dir_all(&bin).expect("node bin should be created");

            let marker = temp.path().join("sibling-node-used");
            let node = bin.join("node");
            let npm = bin.join("npm");
            std::fs::write(
                &node,
                format!(
                    "#!/bin/sh\nprintf sibling-node > {}\n",
                    shell_single_quote(&marker.to_string_lossy())
                ),
            )
            .expect("fake node should be written");
            std::fs::write(&npm, "#!/usr/bin/env node\n").expect("fake npm should be written");
            for executable in [&node, &npm] {
                let mut permissions = std::fs::metadata(executable)
                    .expect("fake executable metadata should exist")
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(executable, permissions)
                    .expect("fake executable should be executable");
            }

            let codex = bin.join("codex").to_string_lossy().into_owned();
            let real = temp
                .path()
                .join("home dir/.nvm/versions/node/v22.14.0/lib/node_modules/@openai/codex/bin/codex.js")
                .to_string_lossy()
                .into_owned();
            let command = anchored_command_from_paths("codex", &codex, &real)
                .expect("nvm codex should produce an anchored npm command");
            let output = Command::new("/bin/bash")
                .args(["-c", &command])
                .env("PATH", "/usr/bin:/bin")
                .output()
                .expect("anchored npm command should start");

            assert!(
                output.status.success(),
                "anchored npm command failed: {}",
                decode_command_output(&output.stderr)
            );
            assert_eq!(
                std::fs::read_to_string(marker).expect("sibling node should leave a marker"),
                "sibling-node"
            );
        }

        #[test]
        fn claude_native_path_with_space_is_quoted() {
            // claude 分支同样要 POSIX 引号包裹含空格的 bin_path,
            // 否则 `/Users/my name/.local/bin/claude update` 会被 shell 拆词。
            let cmd = anchored_command_from_paths(
                "claude",
                "/Users/my name/.local/bin/claude",
                "/Users/my name/.local/share/claude/versions/2.1.146",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("'/Users/my name/.local/bin/claude' update")
            );
        }

        #[test]
        fn brew_path_with_space_is_quoted() {
            // brew 分支用 `<bin_path 同目录>/brew`,目录含空格时同样要引号包裹。
            let cmd = anchored_command_from_paths(
                "gemini",
                "/opt/my brew/bin/gemini",
                "/opt/my brew/Cellar/gemini-cli/0.13.0/libexec/lib/node_modules/@google/gemini-cli/dist/index.js",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("'/opt/my brew/bin/brew' upgrade gemini-cli")
            );
        }

        #[test]
        fn brew_cask_path_with_space_is_quoted() {
            let cmd = anchored_command_from_paths(
                "codex",
                "/opt/my brew/bin/codex",
                "/opt/my brew/Caskroom/codex/0.146.0/bin/codex",
            );
            assert_eq!(
                cmd.as_deref(),
                Some("'/opt/my brew/bin/brew' upgrade --cask codex")
            );
        }

        #[test]
        fn brew_formula_extraction() {
            assert_eq!(
                brew_formula_from_path("/opt/homebrew/Cellar/gemini-cli/0.13.0/bin/gemini")
                    .as_deref(),
                Some("gemini-cli")
            );
            // node 全局包不在 Cellar 下 → 不是 formula。
            assert_eq!(
                brew_formula_from_path("/opt/homebrew/lib/node_modules/openclaw/openclaw.mjs"),
                None
            );
            assert_eq!(
                brew_formula_from_path("/Users/me/.nvm/versions/node/v22/lib/node_modules/x"),
                None
            );
            // Caskroom 不是 formula。
            assert_eq!(
                brew_formula_from_path("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex"),
                None
            );
        }

        #[test]
        fn brew_cask_extraction() {
            assert_eq!(
                brew_cask_from_path("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex").as_deref(),
                Some("codex")
            );
            // Intel prefix 不含 /homebrew/，仍能抽出 token。
            assert_eq!(
                brew_cask_from_path("/usr/local/Caskroom/codex/0.146.0/bin/codex").as_deref(),
                Some("codex")
            );
            // formula / npm 全局包都不是 cask。
            assert_eq!(
                brew_cask_from_path("/opt/homebrew/Cellar/codex/1.2.3/bin/codex"),
                None
            );
            assert_eq!(
                brew_cask_from_path("/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js"),
                None
            );
        }

        #[test]
        fn sibling_bin_returns_none_when_bin_path_has_no_directory() {
            // bin_path 不含 `/` → parent_dir 返回空 → sibling_bin 不能拼出绝对路径
            // → None,让上游 anchored_command_from_paths 整体退化为静态命令兜底,
            // 而不是悄悄拼出 `npm i -g <pkg>` 这种依赖 PATH 的指令(违背"必须绝对路径"
            // 不变量)。实际从 enumerate_tool_installations 走的 bin_path 都是绝对路径,
            // 这条防线不期望被触发,但闭合了 helper 与函数文档的语义一致。
            assert_eq!(sibling_bin("codex", "npm"), None);
            assert_eq!(sibling_bin("", "brew"), None);
            // 含 `/` 即可拼出绝对路径——这是常规路径。
            assert_eq!(
                sibling_bin("/opt/homebrew/bin/gemini", "brew").as_deref(),
                Some("/opt/homebrew/bin/brew")
            );
        }

        #[test]
        fn default_install_prefers_path_default() {
            let installs = vec![
                inst("/opt/homebrew/bin/openclaw", false),
                inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", true),
            ];
            assert_eq!(
                default_install(&installs).map(|i| i.path.as_str()),
                Some("/Users/me/.nvm/versions/node/v22/bin/openclaw")
            );
        }

        #[test]
        fn default_install_falls_back_to_sole_entry() {
            let installs = vec![inst("/opt/homebrew/bin/gemini", false)];
            assert_eq!(
                default_install(&installs).map(|i| i.path.as_str()),
                Some("/opt/homebrew/bin/gemini")
            );
        }

        #[test]
        fn default_install_none_when_ambiguous() {
            let installs = vec![
                inst("/opt/homebrew/bin/openclaw", false),
                inst("/Users/me/.nvm/versions/node/v22/bin/openclaw", false),
            ];
            assert!(default_install(&installs).is_none());
        }

        #[test]
        fn codex_missing_platform_binary_self_heals_via_uninstall_install() {
            // 平台二进制缺失 → `codex --version` 报 "Missing optional dependency" 退出非 0
            // → enumerate 标记 runnable=false。此状态下普通 `npm i -g @latest` 是 no-op 修不好,
            // 升级路径改用 uninstall+install 重装补回平台二进制（`|| true` 让 uninstall 在
            // set -e 下对半损坏包返回非 0 时仍继续 install）。
            let mut broken = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
            broken.runnable = false;
            broken.exited_with_error = true;
            assert_eq!(
                installs_anchored_command("codex", &[broken]).as_deref(),
                Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm uninstall -g @openai/codex || true; PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
            );
        }

        #[test]
        fn codex_probe_timeout_uses_plain_npm_not_uninstall() {
            // `--version` 超时被击杀 / 启动失败：runnable=false 但不是命令自己报错，不能当成
            // 装坏了。走卸载重装时，一旦装失败旧版就没了，用户看到「未安装」；普通
            // `npm i -g` 失败 npm 会回滚、旧版还在。
            let mut probe_failed = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
            probe_failed.runnable = false;
            probe_failed.error = Some("Command timed out after 10s".to_string());
            let cmd = installs_anchored_command("codex", &[probe_failed]);
            assert_eq!(
                cmd.as_deref(),
                Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
            );
            assert!(!cmd.unwrap().contains("uninstall"));
        }

        #[test]
        fn codex_runnable_uses_plain_npm_not_self_heal() {
            // 正常（runnable=true）的 codex 升级：锚定 npm，既不重装、也不跑会假成功
            // 掩盖损坏的 `codex update`。
            let healthy = inst("/Users/me/.nvm/versions/node/v22.14.0/bin/codex", true);
            let cmd = installs_anchored_command("codex", &[healthy]);
            assert_eq!(
                cmd.as_deref(),
                Some("PATH='/Users/me/.nvm/versions/node/v22.14.0/bin':\"$PATH\" /Users/me/.nvm/versions/node/v22.14.0/bin/npm i -g @openai/codex@latest")
            );
            assert!(!cmd.unwrap().contains("uninstall"));
        }

        #[test]
        fn codex_broken_homebrew_formula_uses_brew_not_npm_repair() {
            // brew formula 装的坏 codex（real 在 Cellar）：自愈门控必须收窄放行，回落到
            // `brew upgrade codex`——若误走 npm 重装，npm 够不到 Cellar 那份、反而旁路
            // 装第二份 npm 全局 codex 制造双安装。
            let broken = ToolInstallation {
                path: "/opt/homebrew/bin/codex".to_string(),
                version: None,
                runnable: false,
                error: None,
                exited_with_error: true,
                source: "homebrew".to_string(),
                is_path_default: true,
                real: std::path::PathBuf::from("/opt/homebrew/Cellar/codex/1.2.3/bin/codex"),
            };
            assert_eq!(
                installs_anchored_command("codex", &[broken]).as_deref(),
                Some("/opt/homebrew/bin/brew upgrade codex")
            );
        }

        #[test]
        fn codex_broken_homebrew_cask_uses_brew_not_npm_repair() {
            // brew cask 装的坏 codex（real 在 Caskroom）：与 formula 同理，必须回落到
            // `brew upgrade --cask`。误走 npm uninstall+install 会撞 EEXIST，或旁路
            // 装第二份 npm 全局包争抢同一个 `/opt/homebrew/bin/codex`。
            let broken = ToolInstallation {
                path: "/opt/homebrew/bin/codex".to_string(),
                version: None,
                runnable: false,
                error: None,
                exited_with_error: true,
                source: "homebrew".to_string(),
                is_path_default: true,
                real: std::path::PathBuf::from("/opt/homebrew/Caskroom/codex/0.146.0/bin/codex"),
            };
            assert_eq!(
                installs_anchored_command("codex", &[broken]).as_deref(),
                Some("/opt/homebrew/bin/brew upgrade --cask codex")
            );
        }

        #[test]
        fn codex_broken_volta_uses_volta_install_not_npm_repair() {
            // volta 装的坏 codex：回落到 `volta install`，不走 npm 重装。
            let mut broken = inst("/Users/me/.volta/bin/codex", true);
            broken.runnable = false;
            broken.exited_with_error = true;
            assert_eq!(
                installs_anchored_command("codex", &[broken]).as_deref(),
                Some("/Users/me/.volta/bin/volta install @openai/codex")
            );
        }

        #[test]
        fn codex_broken_bun_uses_bun_add_not_phantom_npm() {
            // bun 装的坏 codex：回落到 `bun add`，且**绝不**拼出 `~/.bun/bin/npm`
            // （bun 目录下没有 npm，那条路径不存在、执行会直接失败）。
            let mut broken = inst("/Users/me/.bun/bin/codex", true);
            broken.runnable = false;
            broken.exited_with_error = true;
            let cmd = installs_anchored_command("codex", &[broken]);
            assert_eq!(
                cmd.as_deref(),
                Some("/Users/me/.bun/bin/bun add -g @openai/codex@latest")
            );
            assert!(!cmd.unwrap().contains("npm"));
        }

        #[test]
        fn codex_standalone_reruns_official_installer_in_place() {
            // #7650:官方独立安装器装的 codex 此前被判 system 来源、锚定落空,退回裸
            // `npm i -g` 另装一份 npm 版。现在带 CODEX_INSTALL_DIR / CODEX_HOME 重跑官方
            // installer;不用 `codex update`——它断网时 exit 0 假成功。
            let cmd = anchored_command_from_paths(
                "codex",
                "/Users/me/.local/bin/codex",
                "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
            )
            .unwrap();
            assert_eq!(
                cmd,
                format!(
                    "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR='/Users/me/.local/bin' CODEX_HOME='/Users/me/.codex' {CODEX_INSTALL_UNIX}"
                )
            );
            assert!(!cmd.contains("npm"), "npm must not be used: {cmd}");
            assert!(
                !cmd.contains("codex update"),
                "codex update fakes success: {cmd}"
            );
        }

        #[test]
        fn codex_standalone_custom_dirs_are_written_back() {
            // 自定义 CODEX_INSTALL_DIR / CODEX_HOME 装的一处:不显式传回去,installer 会装进
            // 默认的 ~/.local/bin 与 ~/.codex,命令行实际在用的那份原地不动。
            let cmd = anchored_command_from_paths(
                "codex",
                "/opt/tools/codex bin/codex",
                "/data/codex-home/packages/standalone/releases/0.157.0-x86_64-unknown-linux-musl/bin/codex",
            );
            assert_eq!(
                cmd,
                Some(format!(
                    "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR='/opt/tools/codex bin' CODEX_HOME='/data/codex-home' {CODEX_INSTALL_UNIX}"
                ))
            );
        }

        #[test]
        fn codex_broken_standalone_reinstalls_via_installer_not_npm_repair() {
            // 跑不起来的独立安装版:npm 自愈门控只放行 nvm/fnm/mise/homebrew,这里交回锚定,
            // 由官方 installer 重装同一处。
            let broken = ToolInstallation {
                path: "/Users/me/.local/bin/codex".to_string(),
                version: None,
                runnable: false,
                error: None,
                exited_with_error: true,
                source: "system".to_string(),
                is_path_default: true,
                real: std::path::PathBuf::from(
                    "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
                ),
            };
            let cmd = installs_anchored_command("codex", &[broken]).unwrap();
            assert!(cmd.ends_with(CODEX_INSTALL_UNIX), "{cmd}");
            assert!(!cmd.contains("npm"), "{cmd}");
        }

        #[test]
        fn codex_broken_standalone_in_node_manager_dir_skips_npm_repair() {
            // CODEX_INSTALL_DIR 指到 Homebrew / nvm 的 bin 目录时,入口路径会被判成
            // homebrew / nvm 来源;npm 自愈门控只看入口来源的话会抢先返回 npm 重装,
            // 把独立安装版换成 npm 版。真身仍在 standalone 发布包里,必须交回 installer。
            for bin in [
                "/opt/homebrew/bin/codex",
                "/Users/me/.nvm/versions/node/v22.19.0/bin/codex",
            ] {
                let broken = ToolInstallation {
                    path: bin.to_string(),
                    version: None,
                    runnable: false,
                    error: None,
                    exited_with_error: true,
                    source: infer_install_source(Path::new(bin)).to_string(),
                    is_path_default: true,
                    real: std::path::PathBuf::from(
                        "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
                    ),
                };
                let cmd = installs_anchored_command("codex", &[broken]).unwrap();
                assert!(cmd.ends_with(CODEX_INSTALL_UNIX), "{bin}: {cmd}");
                assert!(!cmd.contains("npm"), "{bin}: {cmd}");
            }
        }

        #[test]
        fn claude_homebrew_cask_uses_brew_not_self_update_or_npm() {
            // cask 归 brew 管:与 formula 一样不先跑 `claude update`,也不挂 npm fallback。
            let cmd = anchored_command_from_paths(
                "claude",
                "/opt/homebrew/bin/claude",
                "/opt/homebrew/Caskroom/claude-code/2.1.274/claude",
            )
            .unwrap();
            assert_eq!(cmd, "/opt/homebrew/bin/brew upgrade --cask claude-code");
            assert!(!cmd.contains("claude update"));
            assert!(!cmd.contains("npm"));
        }

        #[test]
        fn first_abs_path_line_skips_shell_noise() {
            // 交互式 .zshrc 先打印欢迎语（如 powerlevel10k / 自定义提示），
            // command -v 的真实路径在其后 → 跳过噪音取真路径。
            assert_eq!(
                first_abs_path_line("🚀 Welcome back!\n/Users/me/.local/bin/claude\n"),
                Some("/Users/me/.local/bin/claude")
            );
            // 无噪音时取第一行。
            assert_eq!(
                first_abs_path_line("/opt/homebrew/bin/gemini\n"),
                Some("/opt/homebrew/bin/gemini")
            );
            // 输出里没有任何绝对路径 → None。
            assert_eq!(first_abs_path_line("welcome\nbye\n"), None);
        }

        #[test]
        fn path_line_from_env_output_survives_shell_noise() {
            // `$SHELL -lic /usr/bin/env` 的 stdout 前面可能有交互式 rc 的欢迎语。
            let raw = "🚀 Welcome back, Jason!\nSHELL=/bin/zsh\nPATH=/opt/homebrew/bin:/usr/bin\nHOME=/Users/me\n";
            assert_eq!(
                path_line_from_env_output(raw),
                Some("/opt/homebrew/bin:/usr/bin")
            );
            // 多行值的环境变量里恰好有一行以 `PATH=` 开头时，「值须以 / 开头」把它筛掉。
            let poisoned = "SOME_SCRIPT=line1\nPATH=not-a-path\nPATH=/usr/bin:/bin\n";
            assert_eq!(path_line_from_env_output(poisoned), Some("/usr/bin:/bin"));
            // 完全没有 PATH 行 → None，调用方保持不注入。
            assert_eq!(path_line_from_env_output("HOME=/Users/me\n"), None);
        }

        #[test]
        fn merge_path_segments_dedupes_preserving_login_order() {
            // 登录 shell 的段全部在前且保序；继承 PATH 里的新段追加在后。
            assert_eq!(
                merge_path_segments(
                    "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin",
                    "/usr/bin:/bin:/usr/sbin"
                ),
                "/Users/me/.nvm/versions/node/v22/bin:/usr/bin:/bin:/usr/sbin"
            );
            // 空段（`a::b` 在 POSIX 下意为当前目录）不该被注入。
            assert_eq!(merge_path_segments("/usr/bin::/bin", ""), "/usr/bin:/bin");
        }

        #[test]
        fn is_conflicting_thresholds() {
            let make = |version: Option<&str>, runnable: bool| ToolInstallation {
                path: "/x".to_string(),
                version: version.map(str::to_string),
                runnable,
                error: None,
                exited_with_error: false,
                source: "nvm".to_string(),
                is_path_default: false,
                real: std::path::PathBuf::from("/x"),
            };
            // 单处 → 不冲突。
            assert!(!is_conflicting(&[make(Some("1.0.0"), true)]));
            // 两处同版本、都能跑 → 不冲突（同版本装两遍不打扰）。
            assert!(!is_conflicting(&[
                make(Some("1.0.0"), true),
                make(Some("1.0.0"), true)
            ]));
            // 版本分歧 → 冲突。
            assert!(is_conflicting(&[
                make(Some("1.0.0"), true),
                make(Some("2.0.0"), true)
            ]));
            // 同版本但运行态混合（一个能跑、一个跑不起来）→ 冲突。
            assert!(is_conflicting(&[
                make(Some("1.0.0"), true),
                make(Some("1.0.0"), false)
            ]));
        }
    }

    #[test]
    fn opencode_extra_search_paths_includes_install_and_fallback_dirs() {
        let home = PathBuf::from("/home/tester");
        let install_dir = Some(std::ffi::OsString::from("/custom/opencode/bin"));
        let xdg_bin_dir = Some(std::ffi::OsString::from("/xdg/bin"));
        let gopath =
            std::env::join_paths([PathBuf::from("/go/path1"), PathBuf::from("/go/path2")]).ok();

        let paths = opencode_extra_search_paths(&home, install_dir, xdg_bin_dir, gopath);

        assert_eq!(paths[0], PathBuf::from("/custom/opencode/bin"));
        assert_eq!(paths[1], PathBuf::from("/xdg/bin"));
        assert!(paths.contains(&PathBuf::from("/home/tester/bin")));
        assert!(paths.contains(&PathBuf::from("/home/tester/.opencode/bin")));
        assert!(paths.contains(&PathBuf::from("/home/tester/.bun/bin")));
        assert!(paths.contains(&PathBuf::from("/home/tester/go/bin")));
        assert!(paths.contains(&PathBuf::from("/go/path1/bin")));
        assert!(paths.contains(&PathBuf::from("/go/path2/bin")));
    }

    #[test]
    fn opencode_extra_search_paths_deduplicates_repeated_entries() {
        let home = PathBuf::from("/home/tester");
        let same_dir = Some(std::ffi::OsString::from("/same/path"));

        let paths = opencode_extra_search_paths(&home, same_dir.clone(), same_dir, None);

        let count = paths
            .iter()
            .filter(|path| path.as_path() == Path::new("/same/path"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn opencode_extra_search_paths_deduplicates_bun_default_dir() {
        let home = PathBuf::from("/home/tester");
        let paths = opencode_extra_search_paths(&home, None, None, None);

        let count = paths
            .iter()
            .filter(|path| path.as_path() == Path::new("/home/tester/.bun/bin"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn grok_extra_search_paths_prefers_override_then_default_native_dir() {
        let home = PathBuf::from("/home/tester");
        let paths =
            grok_extra_search_paths(&home, Some(std::ffi::OsString::from("/custom/grok/bin")));

        assert_eq!(paths[0], PathBuf::from("/custom/grok/bin"));
        assert_eq!(paths[1], PathBuf::from("/home/tester/.grok/bin"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn mcode_extra_search_paths_use_installer_bin_dir() {
        let home = PathBuf::from("/home/tester");
        let paths = mcode_extra_search_paths(&home, Some(std::ffi::OsString::from("/opt/mcode")));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/opt/mcode/bin"),
                PathBuf::from("/home/tester/.minimax-code/bin"),
            ]
        );
        // 空的 MCODE_INSTALL_DIR 视同未设置（installer 同样回退默认目录）。
        let paths = mcode_extra_search_paths(&home, Some(std::ffi::OsString::new()));
        assert_eq!(paths, vec![PathBuf::from("/home/tester/.minimax-code/bin")]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn mcode_extra_search_paths_use_installer_root_on_windows() {
        // Windows installer 把 mcode.cmd / mcode.ps1 放在安装根目录，不在 bin 下。
        let home = PathBuf::from("C:\\Users\\tester");
        let paths = mcode_extra_search_paths(&home, None);
        assert_eq!(paths, vec![home.join(".minimax-code")]);
    }

    #[test]
    fn grok_extra_search_paths_deduplicates_default_override() {
        let home = PathBuf::from("/home/tester");
        let paths = grok_extra_search_paths(
            &home,
            Some(std::ffi::OsString::from("/home/tester/.grok/bin")),
        );

        assert_eq!(paths, vec![PathBuf::from("/home/tester/.grok/bin")]);
    }

    #[test]
    fn unique_search_paths_preserve_order_and_skip_empty_entries() {
        let first = PathBuf::from("first/bin");
        let second = PathBuf::from("second/bin");
        let third = PathBuf::from("third/bin");
        let mut paths = IndexSet::new();

        for path in [
            PathBuf::new(),
            first.clone(),
            second.clone(),
            first.clone(),
            PathBuf::new(),
            third.clone(),
            second.clone(),
        ] {
            push_unique_path(&mut paths, path);
        }

        assert_eq!(
            paths.into_iter().collect::<Vec<_>>(),
            vec![first, second, third]
        );
    }

    #[test]
    fn unique_search_paths_keep_path_equality_and_first_spelling() {
        let first = PathBuf::from("node//bin");
        let equivalent = PathBuf::from("node/./bin");
        let parent = PathBuf::from("node/../node/bin");
        let different_case = PathBuf::from("Node/bin");
        assert_eq!(first, equivalent);
        let mut paths = IndexSet::new();

        for path in [
            first.clone(),
            equivalent,
            parent.clone(),
            different_case.clone(),
        ] {
            push_unique_path(&mut paths, path);
        }

        assert_eq!(paths[0].as_os_str(), first.as_os_str());
        assert_eq!(
            paths.into_iter().collect::<Vec<_>>(),
            vec![first, parent, different_case]
        );
    }

    #[test]
    fn unique_search_paths_handle_many_multishell_paths_and_late_duplicates() {
        let expected: Vec<_> = (0..4096)
            .map(|i| PathBuf::from(format!("fnm_multishells/{i}/bin")))
            .collect();
        let mut paths = IndexSet::new();

        for path in expected.iter().chain(expected.iter().rev()) {
            push_unique_path(&mut paths, path.clone());
        }

        assert_eq!(paths.into_iter().collect::<Vec<_>>(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn unique_search_paths_preserve_non_utf8_paths() {
        use std::os::unix::ffi::OsStringExt;

        let first = PathBuf::from(std::ffi::OsString::from_vec(b"node-\xff/bin".to_vec()));
        let second = PathBuf::from(std::ffi::OsString::from_vec(b"node-\xfe/bin".to_vec()));
        let mut paths = IndexSet::new();
        for path in [first.clone(), second.clone(), first.clone()] {
            push_unique_path(&mut paths, path);
        }

        assert_eq!(paths.into_iter().collect::<Vec<_>>(), vec![first, second]);
    }

    #[test]
    fn cli_path_env_search_paths_include_path_entries_and_dedupe() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).expect("first dir should be created");
        std::fs::create_dir_all(&second).expect("second dir should be created");

        let path_env = std::env::join_paths([first.clone(), second.clone(), first.clone()])
            .expect("test path env should be joinable");
        let mut paths = IndexSet::from([first.clone()]);

        extend_from_cli_path_env(&mut paths, Some(path_env));

        assert!(paths.contains(&second));
        assert_eq!(paths.iter().filter(|path| *path == &first).count(), 1);
    }

    #[test]
    fn child_search_paths_include_existing_children_with_suffix() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let base = temp.path().join("node");
        let bin = base.join("25.8.0").join("bin");
        std::fs::create_dir_all(&bin).expect("version bin should be created");

        let mut paths = IndexSet::new();
        extend_existing_child_search_paths(&mut paths, &base, Some("bin"));

        assert!(paths.contains(&bin));
    }

    #[test]
    fn env_child_dir_appends_child_and_dedupes() {
        let base = std::ffi::OsString::from("/custom/toolchain");
        let mut paths = IndexSet::new();

        push_env_child_dir(&mut paths, Some(base.clone()), "bin");
        push_env_child_dir(&mut paths, Some(base), "bin");

        assert_eq!(
            paths.into_iter().collect::<Vec<_>>(),
            vec![PathBuf::from("/custom/toolchain").join("bin")]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cli_path_env_skips_windows_apps_alias_dir() {
        assert!(is_windows_app_execution_alias_dir(Path::new(
            r"C:\Users\tester\AppData\Local\Microsoft\WindowsApps"
        )));
        assert!(!is_windows_app_execution_alias_dir(Path::new(
            r"C:\Users\tester\AppData\Roaming\npm"
        )));
    }

    #[test]
    fn fnm_default_alias_dir_candidates_follow_fnm_base_dir_order() {
        let home = Path::new("/home/tester");
        let data_dir = std::path::PathBuf::from("/home/tester/AppData/Roaming");
        let alias = |base: &str| {
            std::path::PathBuf::from(base)
                .join("aliases")
                .join("default")
        };

        assert_eq!(
            fnm_default_alias_dir_candidates(Some("/opt/fnm".into()), home, Some(data_dir.clone())),
            vec![
                alias("/opt/fnm"),
                alias("/home/tester/.fnm"),
                alias("/home/tester/AppData/Roaming/fnm"),
            ]
        );
        // Empty FNM_DIR / home are ignored instead of becoming relative paths.
        assert_eq!(
            fnm_default_alias_dir_candidates(Some("".into()), Path::new(""), Some(data_dir)),
            vec![alias("/home/tester/AppData/Roaming/fnm")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepend_search_dir_to_path_preserves_non_utf8_bytes() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let current = std::ffi::OsString::from_vec(b"/usr/bin:/tmp/\xff/bin".to_vec());
        let combined = prepend_search_dir_to_path(Path::new("/candidate/bin"), &current);

        assert_eq!(
            combined.as_os_str().as_bytes(),
            b"/candidate/bin:/usr/bin:/tmp/\xff/bin"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_probe_base_path_puts_login_path_before_inherited() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let inherited = std::ffi::OsString::from_vec(b"/usr/bin:/tmp/\xff/bin".to_vec());
        assert_eq!(
            install_probe_base_path(Some("/opt/homebrew/bin:/usr/bin"), &inherited)
                .as_os_str()
                .as_bytes(),
            b"/opt/homebrew/bin:/usr/bin:/usr/bin:/tmp/\xff/bin"
        );
        // 解析不到登录 shell：沿用继承的 PATH。
        assert_eq!(install_probe_base_path(None, &inherited), inherited);
        assert_eq!(install_probe_base_path(Some(""), &inherited), inherited);
        assert_eq!(
            install_probe_base_path(Some("/login/bin"), std::ffi::OsStr::new("")),
            std::ffi::OsString::from("/login/bin")
        );
    }

    /// `#!/usr/bin/env node` 的启动器所在目录里没有 node、继承的 PATH 里也没有：只有补上登录
    /// shell 的 PATH，`--version` 探测才跑得起来（#9810）。
    #[cfg(unix)]
    #[test]
    fn install_probe_finds_interpreter_through_login_path() {
        use std::os::unix::fs::PermissionsExt;
        let write_exec = |path: &Path, body: &str| {
            std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
            std::fs::write(path, body).expect("write script");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        };
        let interpreter = "cc-switch-test-fake-node-9810";
        let dir = tempfile::tempdir().expect("create temp dir");
        let node_bin = dir.path().join("node/bin");
        write_exec(
            &node_bin.join(interpreter),
            "#!/bin/sh\necho 'codex-cli 0.170.0'\n",
        );
        let npm_bin = dir.path().join("npm/bin");
        let launcher = npm_bin.join("codex");
        write_exec(&launcher, &format!("#!/usr/bin/env {interpreter}\n"));

        let inherited = std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
        let without_login =
            prepend_search_dir_to_path(&npm_bin, &install_probe_base_path(None, inherited));
        let out = run_probe_version_command(&launcher, &without_login).expect("spawn");
        assert!(!out.status.success());

        let login_path = node_bin.to_string_lossy().into_owned();
        let with_login = prepend_search_dir_to_path(
            &npm_bin,
            &install_probe_base_path(Some(&login_path), inherited),
        );
        let out = run_probe_version_command(&launcher, &with_login).expect("spawn");
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "codex-cli 0.170.0"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn expand_env_chars_preserves_unknown_vars_and_plain_text() {
        // No percent signs -> verbatim.
        assert_eq!(
            expand_env_chars(r"C:\Program Files\nodejs"),
            r"C:\Program Files\nodejs"
        );
        // Undefined variable is preserved verbatim (nothing dropped).
        assert_eq!(
            expand_env_chars(r"D:\npm-global\%DEFINITELY_NOT_A_REAL_VAR_xyz%\bin"),
            r"D:\npm-global\%DEFINITELY_NOT_A_REAL_VAR_xyz%\bin"
        );
        // Empty percent pair is not treated as a variable name.
        assert_eq!(expand_env_chars(r"C:\path\%%\tail"), r"C:\path\%%\tail");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn expand_env_chars_recovers_missing_user_variables_and_nested_references() {
        let process = |name: &str| match name.to_ascii_uppercase().as_str() {
            "USERPROFILE" => Some(r"C:\Users\tester".into()),
            "OVERRIDE" => Some(r"D:\runtime".into()),
            _ => None,
        };
        let registry = |name: &str| match name.to_ascii_uppercase().as_str() {
            "PNPM_HOME" => Some(r"%LOCALAPPDATA%\pnpm".into()),
            "LOCALAPPDATA" => Some(r"%USERPROFILE%\AppData\Local".into()),
            "OVERRIDE" => Some(r"D:\registry".into()),
            "CYCLE_A" => Some("%CYCLE_B%".into()),
            "CYCLE_B" => Some("%cycle_a%".into()),
            _ => None,
        };
        let expanded = expand_env_chars_with_lookup(
            r"%pnpm_home%;%OVERRIDE%;%MISSING%;%CYCLE_A%;%PNPM_HOME%;unfinished%",
            &process,
            &registry,
            &mut Vec::new(),
        );
        assert_eq!(
            expanded,
            r"C:\Users\tester\AppData\Local\pnpm;D:\runtime;%MISSING%;%cycle_a%;C:\Users\tester\AppData\Local\pnpm;unfinished%"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn build_tool_search_paths_includes_standalone_installer_dirs() {
        // Non-npm installer locations must be scanned even when the process PATH
        // dropped them (regression guard for #6061 / #6278 / #6047).
        let local_data = dirs::data_local_dir().expect("LOCALAPPDATA should resolve");

        let codex_paths = build_tool_search_paths("codex");
        assert!(codex_paths.contains(
            &local_data
                .join("Programs")
                .join("OpenAI")
                .join("Codex")
                .join("bin")
        ));

        let claude_paths = build_tool_search_paths("claude");
        assert!(claude_paths.contains(&local_data.join("Programs").join("claude")));

        // The standalone Codex dir is codex-specific; it must not pollute other tools.
        assert!(!build_tool_search_paths("gemini").contains(
            &local_data
                .join("Programs")
                .join("OpenAI")
                .join("Codex")
                .join("bin")
        ));
    }

    #[test]
    fn omp_search_paths_include_the_bun_global_bin() {
        // A GUI process does not inherit `~/.bun/bin`: without it a Bun global
        // omp is reported as not installed and updates lose their anchor.
        let home = dirs::home_dir().expect("home should resolve");
        let bun_bin = home.join(".bun").join("bin");
        assert!(build_tool_search_paths("omp").contains(&bun_bin));
        // Other Bun-installable tools keep their previous search set.
        assert!(!build_tool_search_paths("gemini").contains(&bun_bin));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_path_lookup_ignores_same_named_file_in_current_directory() {
        let current_dir = tempfile::tempdir().expect("current directory should be created");
        let path_dir = tempfile::tempdir().expect("PATH directory should be created");
        std::fs::write(current_dir.path().join("codex.cmd"), "@echo current\r\n")
            .expect("current-directory shim should be created");
        let expected = path_dir.path().join("codex.cmd");
        std::fs::write(&expected, "@echo path\r\n").expect("PATH shim should be created");

        let effective_path =
            std::env::join_paths([path_dir.path()]).expect("test PATH should join");
        let output = windows_path_lookup_command("codex", &effective_path)
            .current_dir(current_dir.path())
            .output()
            .expect("where.exe should execute");
        let stderr = decode_command_output(&output.stderr);
        let matches = decode_command_output(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .collect::<Vec<_>>();

        assert!(output.status.success(), "where.exe failed: {stderr}");
        assert_eq!(matches.len(), 1);
        assert_eq!(
            std::fs::canonicalize(&matches[0]).expect("where.exe match should canonicalize"),
            std::fs::canonicalize(&expected).expect("expected PATH shim should canonicalize")
        );
    }

    #[test]
    fn mise_node_search_paths_include_shims_and_installed_node_bins() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let home = temp.path();
        let node_bin = home
            .join(".local/share/mise/installs/node/25.8.0")
            .join("bin");
        std::fs::create_dir_all(&node_bin).expect("node bin should be created");

        let mut paths = IndexSet::new();
        extend_mise_node_search_paths(&mut paths, home);

        assert!(paths.contains(&home.join(".local/share/mise/shims")));
        assert!(paths.contains(&node_bin));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn mise_shim_target_resolves_shim_through_mise_which() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir should be created");
        let root = temp.path();
        let node_bin = root.join("installs/node/22.12.0/bin");
        std::fs::create_dir_all(&node_bin).expect("node bin should be created");
        let target = node_bin.join("claude");
        std::fs::write(&target, "").expect("target should be written");

        let mise_dir = root.join("bin");
        std::fs::create_dir_all(&mise_dir).expect("mise dir should be created");
        let mise = mise_dir.join("mise");
        std::fs::write(
            &mise,
            format!(
                "#!/bin/sh\n[ \"$1\" = which ] && [ \"$2\" = claude ] && echo '{}'\n",
                target.display()
            ),
        )
        .expect("fake mise should be written");
        std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755))
            .expect("fake mise should be executable");

        let shims = root.join("shims");
        std::fs::create_dir_all(&shims).expect("shims dir should be created");
        let shim = shims.join("claude");
        std::os::unix::fs::symlink(&mise, &shim).expect("shim symlink should be created");
        let shim_real = std::fs::canonicalize(&shim).expect("shim should canonicalize");

        assert_eq!(
            mise_shim_target("claude", &shim, &shim_real, None),
            Some(target.clone())
        );
        // 不是 mise shim 的候选不去问 mise。
        let target_real = std::fs::canonicalize(&target).expect("target should canonicalize");
        assert_eq!(
            mise_shim_target("claude", &target, &target_real, None),
            None
        );
        // mise 说不出路径（工具没装在当前版本里）时不猜。
        assert_eq!(mise_shim_target("codex", &shim, &shim_real, None), None);

        // 枚举安装时 shim 换成真身，和 installs 目录那处去重成同一处（#9515）；
        // shim 模式下默认入口是 mise 本体，也换成真身。
        let mut path_default = Some(shim_real.clone());
        let (path, real) = resolve_mise_shim_install(
            "claude",
            shim.clone(),
            shim_real.clone(),
            &mut path_default,
            None,
        );
        assert_eq!(path, target);
        assert_eq!(real, target_real);
        assert_eq!(path_default, Some(target_real.clone()));
        // 解析不了的 shim 原样保留，默认入口不动。
        let mut path_default = Some(shim_real.clone());
        let (path, real) = resolve_mise_shim_install(
            "codex",
            shim.clone(),
            shim_real.clone(),
            &mut path_default,
            None,
        );
        assert_eq!(path, shim);
        assert_eq!(real, shim_real);
        assert_eq!(path_default, Some(shim_real));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn tool_executable_candidates_non_windows_uses_plain_binary_name() {
        let dir = PathBuf::from("/usr/local/bin");
        let candidates = tool_executable_candidates("opencode", &dir);

        assert_eq!(candidates, vec![PathBuf::from("/usr/local/bin/opencode")]);
    }

    #[test]
    fn pathext_order_decides_whether_cmd_beats_exe() {
        assert!(!pathext_prefers_cmd_over_exe(None));
        assert!(!pathext_prefers_cmd_over_exe(Some(
            ".COM;.EXE;.BAT;.CMD;.VBS;.JS"
        )));
        assert!(pathext_prefers_cmd_over_exe(Some(".com;.cmd;.exe")));
        assert!(pathext_prefers_cmd_over_exe(Some(".COM;.CMD")));
        assert!(!pathext_prefers_cmd_over_exe(Some(".COM;.BAT")));
    }

    #[test]
    fn same_dir_cmd_is_shadowed_by_exe_sibling_issue_8839() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let cmd = dir.path().join("hermes.cmd");
        let exe = dir.path().join("hermes.exe");
        std::fs::write(&cmd, "").expect("cmd launcher should be created");

        let default_pathext = Some(".COM;.EXE;.BAT;.CMD");
        // A lone launcher is the install itself.
        assert!(!shadowed_by_pathext_sibling(&cmd, default_pathext));

        std::fs::write(&exe, "").expect("exe launcher should be created");
        assert!(shadowed_by_pathext_sibling(&cmd, default_pathext));
        assert!(!shadowed_by_pathext_sibling(&exe, default_pathext));

        // A PATHEXT that lists .CMD first makes the .cmd the one that runs.
        let cmd_first = Some(".COM;.CMD;.EXE");
        assert!(!shadowed_by_pathext_sibling(&cmd, cmd_first));
        assert!(shadowed_by_pathext_sibling(&exe, cmd_first));

        // A different tool's launcher in the same directory never shadows.
        let other = dir.path().join("codex.cmd");
        std::fs::write(&other, "").expect("other launcher should be created");
        assert!(!shadowed_by_pathext_sibling(&other, default_pathext));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tool_executable_candidates_windows_includes_cmd_exe_and_plain_name() {
        let dir = PathBuf::from("C:\\tools");
        let candidates = tool_executable_candidates("opencode", &dir);

        assert_eq!(
            candidates,
            vec![
                PathBuf::from("C:\\tools\\opencode.cmd"),
                PathBuf::from("C:\\tools\\opencode.exe"),
                PathBuf::from("C:\\tools\\opencode"),
            ]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tool_executable_candidates_windows_prefers_opencode_desktop_cli_sidecar() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let gui = dir.path().join("opencode.exe");
        let sidecar = dir.path().join("opencode-cli.exe");
        std::fs::write(&gui, "").expect("desktop executable should be created");
        std::fs::write(&sidecar, "").expect("CLI sidecar should be created");

        let candidates = tool_executable_candidates("opencode", dir.path());

        assert_eq!(
            candidates,
            vec![dir.path().join("opencode.cmd"), sidecar.clone()]
        );
        assert!(!candidates.contains(&gui));
        assert_eq!(
            windows_preferred_tool_executable("opencode", &gui),
            Some(sidecar)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tool_executable_candidates_windows_prefers_opencode_resources_cli_sidecar() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let gui = dir.path().join("opencode.exe");
        let resources = dir.path().join("resources");
        let sidecar = resources.join("opencode-cli.exe");
        std::fs::create_dir(&resources).expect("resources dir should be created");
        std::fs::write(&gui, "").expect("desktop executable should be created");
        std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");
        std::fs::write(&sidecar, "").expect("CLI sidecar should be created");

        let candidates = tool_executable_candidates("opencode", dir.path());

        assert_eq!(
            candidates,
            vec![dir.path().join("opencode.cmd"), sidecar.clone()]
        );
        assert!(!candidates.contains(&gui));
        assert_eq!(
            windows_preferred_tool_executable("opencode", &gui),
            Some(sidecar)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tool_executable_candidates_windows_skips_electron_desktop_without_cli_sidecar() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let gui = dir.path().join("opencode.exe");
        let resources = dir.path().join("resources");
        std::fs::create_dir(&resources).expect("resources dir should be created");
        std::fs::write(&gui, "").expect("desktop executable should be created");
        std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");

        let candidates = tool_executable_candidates("opencode", dir.path());

        assert_eq!(candidates, vec![dir.path().join("opencode.cmd")]);
        assert!(!candidates.contains(&gui));
        assert_eq!(windows_preferred_tool_executable("opencode", &gui), None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_preferred_tool_executable_keeps_standalone_opencode_exe() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let standalone = dir.path().join("opencode.exe");
        std::fs::write(&standalone, "").expect("standalone executable should be created");

        assert_eq!(
            windows_preferred_tool_executable("opencode", &standalone),
            Some(standalone)
        );
    }

    #[test]
    fn foreign_pip_launcher_only_rejects_pi_exe_in_scripts_dir() {
        let pip_pi = Path::new(r"D:\soft\Python\Python311\Scripts\pi.exe");
        assert!(is_foreign_pip_launcher("pi", pip_pi));
        assert!(is_foreign_pip_launcher(
            "pi",
            Path::new(r"C:\venvs\tools\SCRIPTS\PI.EXE")
        ));
        assert!(!is_foreign_pip_launcher(
            "pi",
            Path::new(r"C:\Users\me\AppData\Roaming\npm\pi.cmd")
        ));
        // A custom npm prefix under a `Scripts` folder writes `.cmd` shims.
        assert!(!is_foreign_pip_launcher(
            "pi",
            Path::new(r"D:\Scripts\npm-global\pi.cmd")
        ));
        assert!(!is_foreign_pip_launcher(
            "pi",
            Path::new(r"D:\Scripts\pi.cmd")
        ));
        // Bun's global shims are `.exe` but live in `.bun\bin`.
        assert!(!is_foreign_pip_launcher(
            "pi",
            Path::new(r"C:\Users\me\.bun\bin\pi.exe")
        ));
        // Hermes really is installed by pip.
        assert!(!is_foreign_pip_launcher(
            "hermes",
            Path::new(r"D:\soft\Python\Python311\Scripts\hermes.exe")
        ));
    }

    #[test]
    fn vscode_copilot_shim_is_rejected_but_real_copilot_is_kept() {
        assert!(is_vscode_copilot_shim(
            "copilot",
            Path::new(r"D:\Microsoft VS Code\bin\copilot-shim\copilot.exe")
        ));
        assert!(is_vscode_copilot_shim(
            "copilot",
            Path::new(r"C:\Program Files\Microsoft VS Code\bin\COPILOT-SHIM\copilot.cmd")
        ));
        // npm's global shim is the real CLI.
        assert!(!is_vscode_copilot_shim(
            "copilot",
            Path::new(r"C:\Users\me\AppData\Roaming\npm\copilot.cmd")
        ));
        // Only the copilot tool is affected.
        assert!(!is_vscode_copilot_shim(
            "codex",
            Path::new(r"D:\x\copilot-shim\codex.exe")
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_path_default_skips_pip_pi_and_uses_npm_pi() {
        let root = tempfile::tempdir().expect("temp dir should be created");
        let scripts = root.path().join("Python311").join("Scripts");
        std::fs::create_dir_all(&scripts).expect("Scripts dir should be created");
        let pip_pi = scripts.join("pi.exe");
        std::fs::write(&pip_pi, "").expect("pip launcher should be created");
        assert!(tool_executable_candidates("pi", &scripts)
            .iter()
            .all(|candidate| candidate != &pip_pi));

        let npm = root.path().join("npm");
        std::fs::create_dir_all(&npm).expect("npm dir should be created");
        let npm_pi = npm.join("pi.cmd");
        std::fs::write(&npm_pi, "").expect("npm shim should be created");

        let raw = format!("{}\r\n{}\r\n", pip_pi.display(), npm_pi.display());
        assert_eq!(windows_preferred_path_default("pi", &raw), Some(npm_pi));
        assert_eq!(
            windows_preferred_path_default("pi", &format!("{}\r\n", pip_pi.display())),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_path_default_skips_desktop_without_sidecar_and_uses_next_cli() {
        let desktop_dir = tempfile::tempdir().expect("desktop temp dir should be created");
        let resources = desktop_dir.path().join("resources");
        std::fs::create_dir(&resources).expect("resources dir should be created");
        let desktop = desktop_dir.path().join("opencode.exe");
        std::fs::write(&desktop, "").expect("desktop executable should be created");
        std::fs::write(resources.join("app.asar"), "").expect("app.asar should be created");

        let cli_dir = tempfile::tempdir().expect("CLI temp dir should be created");
        let cli = cli_dir.path().join("opencode.exe");
        std::fs::write(&cli, "").expect("standalone executable should be created");

        let raw = format!("{}\r\n{}\r\n", desktop.display(), cli.display());

        assert_eq!(windows_preferred_path_default("opencode", &raw), Some(cli));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tool_executable_candidates_windows_skips_shadowed_npm_unix_shim() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let extensionless = dir.path().join("codex");
        let cmd = dir.path().join("codex.cmd");
        std::fs::write(&extensionless, "").expect("extensionless shim should be created");
        std::fs::write(&cmd, "").expect("cmd shim should be created");

        let candidates = tool_executable_candidates("codex", dir.path());

        assert_eq!(candidates, vec![cmd.clone(), dir.path().join("codex.exe")]);
        assert!(!candidates.contains(&extensionless));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_runnable_sibling_prefers_cmd_over_extensionless_tool() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let extensionless = dir.path().join("codex");
        let cmd = dir.path().join("codex.cmd");
        std::fs::write(&extensionless, "").expect("extensionless shim should be created");
        std::fs::write(&cmd, "").expect("cmd shim should be created");

        let preferred = windows_runnable_sibling_for_extensionless_tool(&extensionless);

        assert_eq!(preferred.as_deref(), Some(cmd.as_path()));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn run_windows_tool_version_command_accepts_canonicalized_cmd_path() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let cmd = dir.path().join("codex.cmd");
        std::fs::write(&cmd, "@echo off\r\necho codex-cli 0.144.3\r\n")
            .expect("cmd shim should be created");
        let canonical = std::fs::canonicalize(&cmd).expect("cmd shim should canonicalize");
        assert!(
            canonical.to_string_lossy().starts_with(r"\\?\"),
            "Windows canonical paths should use the verbatim prefix: {}",
            canonical.display()
        );

        let current_path = std::env::var("PATH").unwrap_or_default();
        let output = run_windows_tool_version_command(&canonical, &current_path)
            .expect("canonicalized cmd shim should execute");
        let stderr = decode_command_output(&output.stderr);

        assert!(output.status.success(), "cmd shim failed: {stderr}");
        assert_eq!(
            decode_command_output(&output.stdout).trim(),
            "codex-cli 0.144.3"
        );
    }
}
