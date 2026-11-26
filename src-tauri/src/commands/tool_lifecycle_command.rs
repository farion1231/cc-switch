use super::*;

pub(super) fn normalize_requested_tools(tools: &[String]) -> Vec<&'static str> {
    let set: std::collections::HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
    TOOLS
        .iter()
        .map(|tool| tool.id)
        .filter(|tool| set.contains(tool))
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ToolLifecycleAction {
    Install,
    Update,
}

impl FromStr for ToolLifecycleAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "install" => Ok(Self::Install),
            "update" => Ok(Self::Update),
            _ => Err(format!("Unsupported tool action: {value}")),
        }
    }
}

pub(super) fn build_tool_lifecycle_command(
    tools: &[&str],
    action: ToolLifecycleAction,
    wsl_shell_by_tool: Option<&HashMap<String, WslShellPreferenceInput>>,
    confirmed_update: Option<&str>,
) -> Result<String, String> {
    let mut lines = Vec::new();

    #[cfg(not(target_os = "windows"))]
    {
        // set -e 让任一步失败即中止;set -o pipefail 保留为管道命令的兜底防线。
        // 当前官方 installer 路径已避免 `curl | bash`,但未来若新增管道命令,
        // 仍应让管道前段失败参与整条脚本判定。
        lines.push("set -e".to_string());
        lines.push("set -o pipefail".to_string());
    }

    // .bat 以 UTF-8 写入，cmd 却按控制台代码页（中文系统 936、西文 437/850）解析，
    // 用户名含非 ASCII 字符时锚定的 npm.cmd 路径会乱码（#8034）。首行是纯 ASCII，
    // 任何代码页都能正确解析；chcp 之后的行按 UTF-8 读。CREATE_NO_WINDOW 下这是
    // 独立的隐藏控制台，不影响用户终端。
    #[cfg(target_os = "windows")]
    {
        lines.push("@chcp 65001>nul".to_string());
        lines.push("@echo off".to_string());
    }

    for tool in tools {
        let label = tool_spec(tool).display_name;
        lines.push(format!("echo ========== {label} =========="));

        let pref = wsl_shell_by_tool.and_then(|m| m.get(*tool));
        let line = build_tool_action_line(
            tool,
            action,
            pref.and_then(|p| p.wsl_shell.as_deref()),
            pref.and_then(|p| p.wsl_shell_flag.as_deref()),
            confirmed_update,
        )?;
        lines.push(line);

        #[cfg(target_os = "windows")]
        lines.push("if errorlevel 1 exit /b %errorlevel%".to_string());

        #[cfg(not(target_os = "windows"))]
        lines.push(String::new());
    }

    Ok(lines.join(if cfg!(target_os = "windows") {
        "\r\n"
    } else {
        "\n"
    }))
}

/// 官方 shell installer 都不用 `curl | bash` 这种 pipe 形式（仍然用 curl 下载，
/// 只是先落到临时文件再交给 bash 执行）:WSL 分支会在
/// `wsl.exe ... -- sh -c "<cmd>"` 子 shell 里执行命令,外层脚本的 `set -o pipefail`
/// 不会继承进去;而 WSL 默认 shell 可能是 dash/ash,也不能假设支持 `set -o pipefail`。
/// 先下载到 mktemp 文件再交给 bash,能让 curl 失败稳定变成整条命令失败。
const CLAUDE_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://claude.ai/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
const OPENCODE_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://opencode.ai/install -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
pub(super) const GROK_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://x.ai/cli/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
/// MiniMax Code 官方 installer：用 npm `--prefix` 装进 `~/.minimax-code`（`MCODE_INSTALL_DIR`
/// 可改），系统 Node 不达标时自带隔离 Node；重跑即升级（已是最新则跳过），全程无交互提示。
pub(super) const MCODE_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://filecdn.minimax.chat/public/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
/// Codex 官方独立安装器（POSIX sh 脚本）。独立安装版的 `codex update` 内部跑的正是
/// `curl ... | sh` 且没开 pipefail——实测断网时 curl 失败、sh 读到空脚本 exit 0，
/// `codex update` 仍打印 "Update ran successfully" 并 exit 0。所以不走 `codex update`，
/// 由这里下载到临时文件再执行，curl 失败会如实变成整条命令失败。
/// 只用于锚定升级（Windows 用 `CODEX_INSTALL_WINDOWS_SCRIPT`；WSL 不锚定），故按平台 gate。
#[cfg(not(target_os = "windows"))]
pub(super) const CODEX_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://chatgpt.com/codex/install.sh -o $tmp && sh $tmp; status=$?; rm -f $tmp; exit $status'";

/// Hermes 官方安装器会自带/选择合适的 Python 运行时。不要再用
/// `python3 -m pip ... || python -m pip ...`:Hermes PyPI 包要求 Python >=3.11,
/// 但 macOS 系统 `python3` 常是 3.9,而 pyenv 下 `python` shim 还可能不存在,会把
/// 真正的 Python 版本问题盖成 "python command exists in these Python versions"。
const HERMES_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
const HERMES_UPDATE_UNIX: &str =
    "hermes update || bash -c 'tmp=$(mktemp) && curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";

// The npm distribution needs Node >=22.19. The native installer bundles its runtime.
const KIMI_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://code.kimi.com/kimi-code/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
#[cfg(target_os = "windows")]
const KIMI_INSTALL_WINDOWS_SCRIPT: &str = "irm https://code.kimi.com/kimi-code/install.ps1 | iex";

// Oh My Pi 的 npm 包只能由 bun 运行（入口是 `#!/usr/bin/env bun` 的 TypeScript）。官方
// installer 有匹配架构的 bun 时走 bun，否则下载并校验预编译二进制到 `~/.local/bin`，
// 所以新装不要求本机先有 bun。
const OMP_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://omp.sh/install -o $tmp && sh $tmp; status=$?; rm -f $tmp; exit $status'";
#[cfg(target_os = "windows")]
const OMP_INSTALL_WINDOWS_SCRIPT: &str = "irm https://omp.sh/install.ps1 | iex";

/// Antigravity CLI 官方 installer：把原生二进制放进 `~/.local/bin`，再跑 `agy install`
/// 把该目录加进 shell 配置。目标处已有 `agy` 时它只提示已安装、exit 0，所以只用于新装；
/// 升级一律走 `agy update`（实测非 TTY 下照常检查并安装）。
const AGY_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://antigravity.google/cli/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";

#[cfg(target_os = "windows")]
const HERMES_INSTALL_WINDOWS_SCRIPT: &str =
    "irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1 | iex";
#[cfg(target_os = "windows")]
const GROK_INSTALL_WINDOWS_SCRIPT: &str = "irm https://x.ai/cli/install.ps1 | iex";
#[cfg(target_os = "windows")]
const MCODE_INSTALL_WINDOWS_SCRIPT: &str =
    "irm https://filecdn.minimax.chat/public/install.ps1 | iex";
#[cfg(target_os = "windows")]
const AGY_INSTALL_WINDOWS_SCRIPT: &str = "irm https://antigravity.google/cli/install.ps1 | iex";
#[cfg(target_os = "windows")]
const CODEX_INSTALL_WINDOWS_SCRIPT: &str = "irm https://chatgpt.com/codex/install.ps1 | iex";

#[cfg(target_os = "windows")]
fn powershell_encoded_command(script: &str) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let mut bytes = Vec::with_capacity(script.len() * 2);
    for unit in script.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

#[cfg(target_os = "windows")]
fn hermes_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(HERMES_INSTALL_WINDOWS_SCRIPT)
    )
}

#[cfg(target_os = "windows")]
fn grok_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(GROK_INSTALL_WINDOWS_SCRIPT)
    )
}

#[cfg(target_os = "windows")]
fn agy_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(AGY_INSTALL_WINDOWS_SCRIPT)
    )
}

#[cfg(target_os = "windows")]
fn mcode_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(MCODE_INSTALL_WINDOWS_SCRIPT)
    )
}

#[cfg(target_os = "windows")]
fn kimi_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(KIMI_INSTALL_WINDOWS_SCRIPT)
    )
}

#[cfg(target_os = "windows")]
fn omp_install_windows_command() -> String {
    format!(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(OMP_INSTALL_WINDOWS_SCRIPT)
    )
}

/// 重跑官方 installer 升级「脚本安装」那一处。显式传 `MCODE_INSTALL_DIR`，保证写回
/// 探测到的那个目录，而不是 installer 的默认目录。
#[cfg(not(target_os = "windows"))]
pub(super) fn mcode_installer_update_command(install_root: &str) -> String {
    format!(
        "MCODE_INSTALL_DIR={} {MCODE_INSTALL_UNIX}",
        shell_single_quote(install_root)
    )
}

#[cfg(target_os = "windows")]
pub(super) fn mcode_installer_update_command(install_root: &str) -> String {
    // PowerShell 单引号字面量里 `'` 写作 `''`；整段经 EncodedCommand 传入，不经 cmd 解析。
    let script = format!(
        "$env:MCODE_INSTALL_DIR = '{}'; {MCODE_INSTALL_WINDOWS_SCRIPT}",
        install_root.replace('\'', "''")
    );
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(&script)
    )
}

/// 重跑 Kimi Code 官方 installer 升级原生安装那一处。显式传 `KIMI_INSTALL_DIR`，
/// 保证写回探测到的目录（installer 覆盖 `<dir>/bin/kimi`），而不是默认的 `~/.kimi-code`。
#[cfg(not(target_os = "windows"))]
pub(super) fn kimi_installer_update_command(install_root: &str) -> String {
    format!(
        "KIMI_INSTALL_DIR={} {KIMI_INSTALL_UNIX}",
        shell_single_quote(install_root)
    )
}

#[cfg(target_os = "windows")]
pub(super) fn kimi_installer_update_command(install_root: &str) -> String {
    // PowerShell 单引号字面量里 `'` 写作 `''`；整段经 EncodedCommand 传入，不经 cmd 解析。
    let script = format!(
        "$env:KIMI_INSTALL_DIR = '{}'; {KIMI_INSTALL_WINDOWS_SCRIPT}",
        install_root.replace('\'', "''")
    );
    format!(
        "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(&script)
    )
}

/// Codex 官方独立安装器（`install.sh` / `install.ps1`）装出的一处安装。
///
/// 布局（2026-09 实测 install.sh、读 install.ps1）：发布包在
/// `<CODEX_HOME>/packages/standalone/releases/<ver>-<target>/`，`current` 指向当前版本；
/// PATH 上的入口放在 `CODEX_INSTALL_DIR`（POSIX 默认 `~/.local/bin` 下的软链，Windows 默认
/// `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin` 这个 junction），真身都解析到发布包里。
/// 它既不归 npm 管，也不在 `prefers_official_update` 里，此前锚定落空、退回裸
/// `npm i -g` 另装一份 npm 版（#7650）。
#[derive(Debug, PartialEq, Eq)]
struct CodexStandaloneInstall {
    /// 入口所在目录，重跑 installer 时作为 `CODEX_INSTALL_DIR` 写回同一处。
    install_dir: String,
    /// 从真身路径截出的 `CODEX_HOME`；GUI 进程拿不到用户 shell 里设的 `CODEX_HOME`，
    /// 不显式传会装进默认的 `~/.codex`，入口指向的那份原地不动。
    codex_home: Option<String>,
}

/// 纯字符串判定、不碰 fs，POSIX / Windows 路径都能在任一平台单测。
fn codex_standalone_install(bin_path: &str, real_target: &str) -> Option<CodexStandaloneInstall> {
    let install_dir = parent_dir(bin_path);
    if install_dir.is_empty() {
        return None;
    }
    let normalized_real = real_target.replace('\\', "/").to_ascii_lowercase();
    if let Some(idx) = normalized_real.find("/packages/standalone/") {
        // `replace` 与 `to_ascii_lowercase` 都不改字节长度，下标可直接用于原串。
        // Windows 的 real 是 canonicalize 出的 `\\?\` verbatim 路径，交给 installer 前还原。
        let home = &real_target[..idx];
        let home = match home.strip_prefix(r"\\?\UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => home.strip_prefix(r"\\?\").unwrap_or(home).to_string(),
        };
        if !home.is_empty() {
            return Some(CodexStandaloneInstall {
                install_dir,
                codex_home: Some(home),
            });
        }
    }
    // canonicalize 失败时 real 就是入口本身；Windows 默认入口目录仍能认出来，
    // 只是拿不到 CODEX_HOME，交给 installer 用默认值。
    let normalized_bin = bin_path.replace('\\', "/").to_ascii_lowercase();
    normalized_bin
        .contains("/programs/openai/codex/bin/")
        .then_some(CodexStandaloneInstall {
            install_dir,
            codex_home: None,
        })
}

pub(super) fn is_standalone_codex(launcher: &Path, real: &Path) -> bool {
    codex_standalone_install(&launcher.to_string_lossy(), &real.to_string_lossy()).is_some()
}

/// These native updaters write to a fixed location in the current user's home.
#[cfg(not(target_os = "windows"))]
pub(super) fn is_standard_curl_install(tool: &str, real: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    match tool {
        "claude" => real.parent().is_some_and(|dir| {
            home.join(".local/share/claude/versions")
                .canonicalize()
                .is_ok_and(|versions| versions == dir)
        }),
        "opencode" => home
            .join(".opencode/bin/opencode")
            .canonicalize()
            .is_ok_and(|entry| entry == real),
        _ => false,
    }
}

fn omp_distro_owned_path(path: &Path) -> bool {
    (path.starts_with("/usr") && !path.starts_with("/usr/local"))
        || ["/bin", "/sbin", "/lib", "/lib64"]
            .iter()
            .any(|root| path.starts_with(root))
}

pub(super) fn omp_update_directory(launcher: &Path) -> Option<PathBuf> {
    if !launcher.is_absolute() {
        return None;
    }
    #[cfg(target_os = "windows")]
    let launcher = windows_shell_compatible_path(launcher);
    let directory = launcher.parent()?.to_owned();
    if directory
        .to_str()?
        .contains(if cfg!(windows) { ';' } else { ':' })
    {
        return None;
    }
    Some(directory)
}

/// Keep inherited PATH literal (including quoted Windows entries). No shell parses it.
pub(super) fn omp_update_path(
    directory: &Path,
    inherited: &std::ffi::OsStr,
) -> Option<std::ffi::OsString> {
    let mut path = std::env::join_paths([directory]).ok()?;
    path.push(if cfg!(windows) { ";" } else { ":" });
    path.push(inherited);
    Some(path)
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn omp_native_update_process(
    launcher: &Path,
    home: &Path,
    inherited: &std::ffi::OsStr,
) -> Option<std::process::Command> {
    let directory = omp_update_directory(launcher)?;
    let mut command = std::process::Command::new(launcher);
    command
        .arg("update")
        .current_dir(home)
        .env("PATH", omp_update_path(&directory, inherited)?);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    Some(command)
}

fn omp_native_update_command(bin_path: &str, home: &Path) -> Option<String> {
    let directory = omp_update_directory(Path::new(bin_path))?;
    #[cfg(not(target_os = "windows"))]
    return Some(format!(
        "cd {} && PATH={}:\"$PATH\" {} update",
        shell_single_quote(&home.to_string_lossy()),
        shell_single_quote(directory.to_str()?),
        shell_single_quote(bin_path)
    ));
    #[cfg(target_os = "windows")]
    {
        let _ = (directory, home);
        // Preview only. The validated plan executes as native argv, never a batch script.
        Some(format!("{} update", win_double_quote(bin_path)))
    }
}

fn omp_managed_path(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    omp_distro_owned_path(path)
        || path.with_extension("shim").is_file()
        || path.with_extension("bunx").is_file()
        || matches!(
            infer_install_source(path),
            "npm" | "pnpm" | "bun" | "mise" | "volta" | "nvm" | "fnm" | "nix" | "scoop"
        )
        || [
            "/node_modules/",
            "/cellar/",
            "/caskroom/",
            "/nix/store/",
            "/.bun/",
            "/.volta/",
            "/.local/share/mise/",
            "/.local/share/pnpm/",
        ]
        .iter()
        .any(|marker| normalized.contains(marker))
}

/// Native installers allow PI_INSTALL_DIR. Reject manager-owned paths at every
/// link in the chain, including directory links; use only for checks and updates.
pub(super) fn is_omp_standalone_install(launcher: &Path, real: &Path) -> bool {
    if !launcher.is_absolute()
        || !launcher.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("omp") || name.eq_ignore_ascii_case("omp.exe")
        })
        || !is_native_executable(real)
        || launcher.canonicalize().ok().as_deref() != Some(real)
        || omp_managed_path(real)
    {
        return false;
    }
    let mut path = launcher.to_owned();
    for _ in 0..64 {
        if omp_managed_path(&path)
            || npm_global_package("omp", &path, real, cfg!(target_os = "windows")).is_some()
        {
            return false;
        }
        let mut prefix = PathBuf::new();
        let mut components = path.components();
        let mut redirect = None;
        while let Some(component) = components.next() {
            match component {
                std::path::Component::ParentDir => {
                    let _ = prefix.pop();
                }
                std::path::Component::CurDir => {}
                component => prefix.push(component.as_os_str()),
            }
            let candidate = prefix.join(components.as_path());
            if omp_managed_path(&candidate)
                || npm_global_package("omp", &candidate, real, cfg!(target_os = "windows"))
                    .is_some()
            {
                return false;
            }
            if let Ok(target) = std::fs::read_link(&prefix) {
                let target = if target.is_absolute() {
                    target
                } else {
                    let Some(parent) = prefix.parent() else {
                        return false;
                    };
                    parent.join(target)
                };
                redirect = Some(target.join(components.as_path()));
                break;
            }
        }
        match redirect {
            Some(target) => path = target,
            None => return true,
        }
    }
    false
}

/// 重跑官方 installer 升级独立安装那一处。显式传 `CODEX_INSTALL_DIR` / `CODEX_HOME`
/// 写回探测到的位置（实测自定义目录时会原地升级、不在默认目录多装一份）；
/// `CODEX_NON_INTERACTIVE=1` 与 `codex update` 自己重跑 installer 时的做法一致。
#[cfg(not(target_os = "windows"))]
fn codex_installer_update_command(install: &CodexStandaloneInstall) -> String {
    let mut env = format!(
        "CODEX_NON_INTERACTIVE=1 CODEX_INSTALL_DIR={}",
        shell_single_quote(&install.install_dir)
    );
    if let Some(home) = &install.codex_home {
        env.push_str(&format!(" CODEX_HOME={}", shell_single_quote(home)));
    }
    format!("{env} {CODEX_INSTALL_UNIX}")
}

#[cfg(target_os = "windows")]
fn codex_installer_update_command(install: &CodexStandaloneInstall) -> String {
    // PowerShell 单引号字面量里 `'` 写作 `''`；整段经 EncodedCommand 传入，不经 cmd 解析。
    let quote = |value: &str| value.replace('\'', "''");
    let mut script = format!(
        "$env:CODEX_NON_INTERACTIVE = '1'; $env:CODEX_INSTALL_DIR = '{}'; ",
        quote(&install.install_dir)
    );
    if let Some(home) = &install.codex_home {
        script.push_str(&format!("$env:CODEX_HOME = '{}'; ", quote(home)));
    }
    script.push_str(CODEX_INSTALL_WINDOWS_SCRIPT);
    format!(
        "powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand {}",
        powershell_encoded_command(&script)
    )
}

#[cfg(target_os = "windows")]
fn hermes_update_windows_command() -> String {
    // fallback 是 powershell.exe，不是 .cmd/.bat；这里不需要 `call`。PowerShell 的
    // `irm | iex` 已被 EncodedCommand 收进单一参数,避免 `cmd.exe` 解析管道符。
    format!("hermes update || {}", hermes_install_windows_command())
}

#[derive(Debug, Clone, Copy)]
enum LifecycleCommandShell {
    Posix,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    WindowsBatch,
}

fn bare_official_update_command(tool: &str) -> Option<String> {
    tool_spec(tool)
        .official_update_args
        .map(|args| format!("{tool} {args}"))
}

fn chain_update_commands(
    primary: String,
    fallback: String,
    shell: LifecycleCommandShell,
) -> String {
    if fallback.trim().is_empty() {
        return primary;
    }
    match shell {
        LifecycleCommandShell::Posix => format!("{primary} || {fallback}"),
        // 这段最终会被外层再包成 `call <command>`。fallback 若是 npm.cmd/pnpm.cmd,
        // `||` 右侧也必须显式 `call`,否则批处理会转移控制权并跳过后续工具。
        LifecycleCommandShell::WindowsBatch => format!("{primary} || call {fallback}"),
    }
}

fn tool_action_shell_command_for_shell(
    tool: &str,
    action: ToolLifecycleAction,
    shell: LifecycleCommandShell,
) -> Option<String> {
    if matches!(tool, "kimi" | "omp") {
        let install = match shell {
            LifecycleCommandShell::Posix if tool == "kimi" => KIMI_INSTALL_UNIX.to_string(),
            LifecycleCommandShell::Posix => OMP_INSTALL_UNIX.to_string(),
            #[cfg(target_os = "windows")]
            LifecycleCommandShell::WindowsBatch if tool == "kimi" => kimi_install_windows_command(),
            #[cfg(target_os = "windows")]
            LifecycleCommandShell::WindowsBatch => omp_install_windows_command(),
            #[cfg(not(target_os = "windows"))]
            LifecycleCommandShell::WindowsBatch => return None,
        };
        return Some(match action {
            ToolLifecycleAction::Install => install,
            ToolLifecycleAction::Update => {
                chain_update_commands(bare_official_update_command(tool)?, install, shell)
            }
        });
    }
    // xAI's and MiniMax's primary Windows distribution is the official PowerShell
    // installer. Keep npm as the network/policy fallback, matching the POSIX installer chain.
    #[cfg(target_os = "windows")]
    if matches!(action, ToolLifecycleAction::Install)
        && matches!(shell, LifecycleCommandShell::WindowsBatch)
    {
        let installer = match tool {
            "grok" => Some(grok_install_windows_command()),
            "mcode" => Some(mcode_install_windows_command()),
            _ => None,
        };
        if let Some(installer) = installer {
            return Some(chain_update_commands(
                installer,
                tool_spec(tool).npm_install_command()?,
                shell,
            ));
        }
    }

    // 没有 npm 包。重跑 installer 不会升级（已安装就直接退出），升级失败也不拿它兜底。
    if tool == "agy" {
        return Some(match (action, shell) {
            (ToolLifecycleAction::Install, LifecycleCommandShell::Posix) => {
                AGY_INSTALL_UNIX.to_string()
            }
            #[cfg(target_os = "windows")]
            (ToolLifecycleAction::Install, LifecycleCommandShell::WindowsBatch) => {
                agy_install_windows_command()
            }
            #[cfg(not(target_os = "windows"))]
            (ToolLifecycleAction::Install, LifecycleCommandShell::WindowsBatch) => return None,
            (ToolLifecycleAction::Update, _) => "agy update".to_string(),
        });
    }

    if tool == "hermes" {
        return Some(
            match (action, shell) {
                (ToolLifecycleAction::Install, LifecycleCommandShell::Posix) => HERMES_INSTALL_UNIX,
                (ToolLifecycleAction::Update, LifecycleCommandShell::Posix) => HERMES_UPDATE_UNIX,
                #[cfg(target_os = "windows")]
                (ToolLifecycleAction::Install, LifecycleCommandShell::WindowsBatch) => {
                    return Some(hermes_install_windows_command());
                }
                #[cfg(target_os = "windows")]
                (ToolLifecycleAction::Update, LifecycleCommandShell::WindowsBatch) => {
                    return Some(hermes_update_windows_command());
                }
                #[cfg(not(target_os = "windows"))]
                (_, LifecycleCommandShell::WindowsBatch) => return None,
            }
            .to_string(),
        );
    }

    let install = tool_spec(tool).npm_install_command()?;
    match action {
        ToolLifecycleAction::Install => Some(install),
        ToolLifecycleAction::Update => match prefers_official_update(tool, shell)
            .then(|| bare_official_update_command(tool))
            .flatten()
        {
            Some(update) => Some(chain_update_commands(update, install, shell)),
            None => Some(install),
        },
    }
}

fn tool_action_shell_command(tool: &str, action: ToolLifecycleAction) -> Option<String> {
    #[cfg(target_os = "windows")]
    let shell = LifecycleCommandShell::WindowsBatch;
    #[cfg(not(target_os = "windows"))]
    let shell = LifecycleCommandShell::Posix;

    tool_action_shell_command_for_shell(tool, action, shell)
}

/// Windows host 上的 WSL 分支专用:`tool_action_shell_command` 在 Windows target 编译
/// 出的版本会包含 Windows batch 语义(例如 `|| call npm ...`)且 hermes 会返回
/// Windows PowerShell installer,但跨 `wsl.exe` 边界后跑的是 Linux。这个 wrapper
/// 强制生成 POSIX 版命令。
#[cfg(target_os = "windows")]
fn wsl_tool_action_shell_command(tool: &str, action: ToolLifecycleAction) -> Option<String> {
    match action {
        ToolLifecycleAction::Install => {
            let command = posix_install_command_for(tool);
            if command.is_empty() {
                None
            } else {
                Some(command)
            }
        }
        ToolLifecycleAction::Update if tool == "opencode" => bare_official_update_command(tool),
        ToolLifecycleAction::Update => {
            tool_action_shell_command_for_shell(tool, action, LifecycleCommandShell::Posix)
        }
    }
}

pub(super) fn update_confirmation_token(
    tool: &str,
    command: &str,
    installs: &[ToolInstallation],
) -> String {
    use sha2::{Digest, Sha256};
    let mut entries: Vec<_> = installs.iter().collect();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let mut digest = Sha256::new();
    digest.update(tool.as_bytes());
    digest.update([0]);
    digest.update(command.as_bytes());
    for entry in entries {
        digest.update([0]);
        digest.update(entry.path.as_bytes());
        digest.update([0]);
        digest.update(entry.real.as_os_str().as_encoded_bytes());
        digest.update([u8::from(entry.is_path_default)]);
    }
    format!("{:x}", digest.finalize())
}

fn checked_update_command(
    tool: &str,
    installs: &[ToolInstallation],
    confirmed_update: Option<&str>,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    if tool == "omp"
        && !default_install(installs).is_some_and(|install| {
            // The existing Bun shim route remains available. This immutable layout
            // decision cannot turn into a standalone preview during a later FS read.
            install.source == "bun" && infer_install_source(Path::new(&install.path)) == "bun"
        })
    {
        return Err("Select and confirm the detected Oh My Pi installation to update it.".into());
    }
    let command = match resolve_update_command(tool, installs) {
        UpdateCommand::Anchored(command) | UpdateCommand::Static(command) => command,
        UpdateCommand::Unmanaged => return Err(unmanaged_update_error(tool, installs)),
    };
    if installs.len() >= 2 || confirmed_update.is_some() {
        let current = update_confirmation_token(tool, &command, installs);
        if confirmed_update != Some(current.as_str()) {
            return Err("The installation changed or needs review. Check the installed versions and confirm the update again.".to_string());
        }
    }
    Ok(command)
}

fn build_tool_action_line(
    tool: &str,
    action: ToolLifecycleAction,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
    confirmed_update: Option<&str>,
) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        // ① WSL 工具(override 是 UNC `\\wsl$\<distro>\...`):锚定的绝对路径是 Windows
        //    主机路径,跨 wsl.exe 进入 distro 文件系统后无效;且 enumerate 不参与 WSL。
        //    install 走 POSIX 安装优先级,update 走 POSIX 静态/官方 update 命令,
        //    再通过 wsl.exe -d distro -- sh 包一层。
        //    **必须用 wsl_tool_action_shell_command 而非 tool_action_shell_command**:
        //    后者在 Windows target 给 hermes 返回 PowerShell installer,且 Windows batch
        //    语义也不适合跨 wsl.exe;这里统一替换为 POSIX 版安装/更新命令。
        if let Some(distro) = wsl_distro_for_tool(tool) {
            if confirmed_update.is_some() {
                return Err(
                    "The installation environment changed. Confirm the update again.".into(),
                );
            }
            if tool == "opencode" && matches!(action, ToolLifecycleAction::Update) {
                return Err("Run the OpenCode WSL update separately.".into());
            }
            let command = wsl_tool_action_shell_command(tool, action)
                .ok_or_else(|| format!("Unsupported tool action target: {tool}"))?;
            return build_wsl_tool_action_line(&distro, &command, wsl_shell, wsl_shell_flag);
        }
        // ② Windows 原生 update 锚定;install 走静态(install.sh 是 bash 脚本,Windows
        //    无意义)。**`enumerate_tool_installations` 在这里 per-tool 重做、与前端
        //    probe 阶段算过的结果不共享是 by design**:run_tool_lifecycle_action 是
        //    独立 IPC 调用,不信任前端回传的命令字符串(避免命令注入面扩大);前端是
        //    逐工具触发 lifecycle,batch 化会破坏"逐工具独立成败"的 UX。
        let command = match action {
            ToolLifecycleAction::Update => {
                let installs = enumerate_tool_installations(tool);
                checked_update_command(tool, &installs, confirmed_update)?
            }
            ToolLifecycleAction::Install => {
                static_fallback_command_for(tool, ToolLifecycleAction::Install)
            }
        };
        if command.is_empty() {
            return Err(format!("Unsupported tool action target: {tool}"));
        }
        // .bat 调用 .cmd/.bat 必须用 `call` 否则当前脚本被替换、后续 `if errorlevel`
        // 行被跳过;对 .exe 加 call 无害(等同直接调用)。锚定命令头部可能是 .cmd
        // (npm/pnpm)或 .exe(volta),静态命令头部是 `npm`(也是 .cmd)、`py` 等——
        // 全部加 `call ` 前缀,风格统一且语义正确。含空格的头部已被 `win_quote_path_for_batch`
        // 加上双引号,call 对带引号的路径解析正常。
        Ok(format!("call {command}"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (wsl_shell, wsl_shell_flag);
        // update 锚定到命令行实际命中的那处（写回同一个 node / brew / 原生安装器），
        // 而非裸 `npm` 落到 PATH 第一个 npm；install 走「上游推荐 || npm 兜底」短路链
        // （有 native installer 的工具如 claude/opencode/hermes），其余仍裸 npm。
        let command = match action {
            ToolLifecycleAction::Update => {
                let installs = enumerate_tool_installations(tool);
                let command = checked_update_command(tool, &installs, confirmed_update)?;
                if confirmed_update.is_some() {
                    let target = default_install(&installs)
                        .ok_or("The confirmed update target is unavailable.")?;
                    format!(
                        r#"[[ {} -ef {} ]] || {{ echo 'The confirmed update target changed.' >&2; exit 1; }}; {command}"#,
                        shell_single_quote(&target.path),
                        shell_single_quote(&target.real.to_string_lossy()),
                    )
                } else {
                    command
                }
            }
            ToolLifecycleAction::Install => install_command_for(tool),
        };
        if command.is_empty() {
            return Err(format!("Unsupported tool action target: {tool}"));
        }
        Ok(command)
    }
}

#[cfg(target_os = "windows")]
fn build_wsl_tool_action_line(
    distro: &str,
    command: &str,
    force_shell: Option<&str>,
    force_shell_flag: Option<&str>,
) -> Result<String, String> {
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: {distro}"));
    }

    let shell = force_shell
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .unwrap_or("sh");
    if !is_valid_shell(shell) {
        return Err(format!("Invalid WSL shell: {shell}"));
    }

    let flag = if let Some(flag) = force_shell_flag {
        if !is_valid_shell_flag(flag) {
            return Err(format!("Invalid WSL shell flag: {flag}"));
        }
        flag
    } else {
        default_flag_for_shell(shell)
    };

    Ok(format!(
        "wsl.exe -d {distro} -- {shell} {flag} {}",
        windows_cmd_double_quote_arg(command)
    ))
}
/// 取路径的父目录(纯字符串截断,不碰 fs):`/a/b/npm` → `/a/b`、`C:\a\b\npm.cmd`
/// → `C:\a\b`、混合分隔符 `C:\a/b\npm` → `C:\a/b`。无父目录返回空串。
///
/// 平台无关:`\` 和 `/` 都识别,取两者最右出现位置。`Option<usize>` 的 Ord 让
/// `None < Some(_)`,所以 `rfind('\\').max(rfind('/'))` 自动取存在的那个、两者都
/// 存在时取靠右的——比 `or_else` 优先取一种正确(混合分隔符不会拿错父目录)。
/// 跨平台 fs separator 在两侧均接受,使 macOS/Linux 上的 cargo test 也能跑 Windows
/// 路径用例(`parent_dir_cases::mixed_separators_takes_rightmost`)。空串语义由上游
/// `sibling_bin` 的 `is_empty()` 检查转成 None → 锚定整体退化到静态兜底。
pub(super) fn parent_dir(p: &str) -> String {
    match p.rfind('\\').max(p.rfind('/')) {
        Some(i) if i > 0 => p[..i].to_string(),
        _ => String::new(),
    }
}

/// 从 canonicalize 后的真身路径提取 Homebrew formula 名：
/// `/opt/homebrew/Cellar/gemini-cli/0.13.0/...` → `Some("gemini-cli")`。
/// 非 Cellar 路径（= 不是 formula，可能是 Homebrew 的 node 装的 npm 全局包）返回 None。
/// 关键区分：formula 即便内部用 node，真身也落在 `Cellar/<formula>/` 下；而 Homebrew
/// npm 全局包落在 `/opt/homebrew/lib/node_modules`（不含 Cellar）。两者升级命令不同。
#[cfg(not(target_os = "windows"))]
pub(super) fn brew_formula_from_path(real: &str) -> Option<String> {
    brew_token_from_path(real, "Cellar")
}

/// 从 canonicalize 后的真身路径提取 Homebrew cask token：
/// `/opt/homebrew/Caskroom/codex/0.146.0/bin/codex` → `Some("codex")`。
/// 非 Caskroom 路径返回 None。Cask 是预编译二进制，不走 node；若误判成 Homebrew
/// npm 全局包，会用 sibling `npm i -g` 覆盖 `/opt/homebrew/bin/<tool>` 并触发 EEXIST。
#[cfg(not(target_os = "windows"))]
pub(super) fn brew_cask_from_path(real: &str) -> Option<String> {
    brew_token_from_path(real, "Caskroom")
}

/// Homebrew 安装目录形如 `<prefix>/{Cellar,Caskroom}/<token>/<version>/...`。
/// 取出 `marker` 后的第一段作为 formula 名或 cask token；大小写不敏感，兼容
/// `/usr/local/Caskroom`（Intel）与 `/opt/homebrew/Caskroom`（Apple Silicon）。
#[cfg(not(target_os = "windows"))]
fn brew_token_from_path(real: &str, marker: &str) -> Option<String> {
    let mut segs = real.split('/');
    while let Some(seg) = segs.next() {
        if seg.eq_ignore_ascii_case(marker) {
            return segs
                .next()
                .filter(|s| {
                    s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
                        && s.bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || b"-.+_@".contains(&ch))
                })
                .map(str::to_owned);
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub(super) struct HomebrewInstallation {
    pub brew: PathBuf,
    pub kind: &'static str,
    pub token: String,
}

#[cfg(not(target_os = "windows"))]
pub(super) fn homebrew_installation(real: &Path) -> Option<HomebrewInstallation> {
    let directory = real.ancestors().find(|path| {
        path.file_name().is_some_and(|name| {
            name.eq_ignore_ascii_case("Cellar") || name.eq_ignore_ascii_case("Caskroom")
        })
    })?;
    let kind = if directory.file_name()?.eq_ignore_ascii_case("Caskroom") {
        "cask"
    } else {
        "formula"
    };
    let real = real.to_str()?;
    let token = if kind == "cask" {
        brew_cask_from_path(real)?
    } else {
        brew_formula_from_path(real)?
    };
    if !token
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        || !token
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"-.+_@".contains(&ch))
    {
        return None;
    }

    let brew = directory.parent()?.join("bin/brew");
    brew.is_file()
        .then_some(HomebrewInstallation { brew, kind, token })
}

/// Cellar formula 或 Caskroom cask 都由 Homebrew 拥有，升级必须走 brew，
/// 不能落到 sibling npm。任一命中即视为 brew-managed。
#[cfg(not(target_os = "windows"))]
pub(super) fn is_brew_managed_path(real: &str) -> bool {
    brew_formula_from_path(real).is_some() || brew_cask_from_path(real).is_some()
}

/// xAI's native installer uses `~/.grok/bin` for its launchers and
/// `~/.grok/downloads/grok-<platform>` for the downloaded binary. The launcher
/// directory also covers the default Windows layout, where the executable is
/// copied instead of symlinked. Checking the real target additionally supports
/// a custom `$GROK_BIN_DIR` on POSIX, whose launcher still points into the
/// standard downloads directory.
pub(super) fn is_grok_native_install(bin_path: &str, real_target: &str) -> bool {
    [bin_path, real_target].iter().any(|path| {
        let normalized = path.replace('\\', "/").to_ascii_lowercase();
        normalized.contains("/.grok/bin/") || normalized.contains("/.grok/downloads/grok-")
    })
}

fn last_path_segment_lower(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// MiniMax Code 官方 installer 的入口布局（见 `mcode_extra_search_paths`）：POSIX 为
/// `<root>/bin/mcode`（读 `<root>/current` 再转发的 sh launcher，不是软链），Windows 的
/// `mcode.cmd` 直接放在 `<root>` 下。显式传参而不是在函数里 `cfg`，两种布局都能在任一
/// 平台上单测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum McodeLayout {
    Posix,
    Windows,
}

impl McodeLayout {
    pub(super) const NATIVE: Self = if cfg!(target_os = "windows") {
        Self::Windows
    } else {
        Self::Posix
    };
}

/// 从入口路径推出安装根目录候选（installer 的 `MCODE_INSTALL_DIR`）。只是候选，是否真
/// 属于 installer 由调用方判定。只有 POSIX 布局才去掉入口所在的那层 `bin`：Windows 的
/// 根目录本身可以叫 `bin`（如 `MCODE_INSTALL_DIR=C:\Users\me\bin`），不能再上跳一级。
fn mcode_install_root_candidate(path: &str, layout: McodeLayout) -> String {
    let dir = parent_dir(path);
    if layout == McodeLayout::Posix && last_path_segment_lower(&dir) == "bin" {
        parent_dir(&dir)
    } else {
        dir
    }
}

/// 若入口位于默认安装目录 `.minimax-code`，返回该目录。纯字符串、不碰 fs（与 POSIX
/// 锚定的纯函数约定一致）；自定义 `MCODE_INSTALL_DIR` 由 `installs_anchored_command`
/// 里的 `mcode_receipt_install_root` 按回执认领，这里只兜底回执缺失的情况。
/// 先看 `bin_path`：Windows 的 `real` 是 canonicalize 出的 `\\?\` verbatim 路径，
/// 不宜原样交给 installer；`real_target` 覆盖用户自建软链指向 launcher 的情况。
pub(super) fn mcode_script_install_root(
    bin_path: &str,
    real_target: &str,
    layout: McodeLayout,
) -> Option<String> {
    [bin_path, real_target].iter().find_map(|path| {
        let root = mcode_install_root_candidate(path, layout);
        (last_path_segment_lower(&root) == ".minimax-code").then_some(root)
    })
}

/// Kimi Code 的 `upgrade` 子命令 0.43.0 起才有 `-y, --yes`。更早的版本把 `--yes` 当成主命令
/// 的全局选项吞掉，非 TTY（静默 lifecycle 正是如此）下只打印手动升级提示就 exit 0，
/// `kimi upgrade --yes || <fallback>` 的兜底永不触发，用户看到「版本没有变化」（#9032）。
const KIMI_UPGRADE_YES_MIN_VERSION: &str = "0.43.0";

fn kimi_upgrade_ignores_yes(version: Option<&str>) -> bool {
    version.and_then(|v| compare_semver(v, KIMI_UPGRADE_YES_MIN_VERSION))
        == Some(std::cmp::Ordering::Less)
}

/// Kimi Code 官方 installer 的布局：两平台都是 `<KIMI_INSTALL_DIR>/bin/kimi[.exe]`
/// 原生二进制。npm 版的真身在 `node_modules` 下（Windows 入口是 `kimi.cmd`），不算。
/// 先看真身：POSIX 用户自建的软链（如 `~/.local/bin/kimi`）指回安装目录时，installer
/// 必须写回真身所在的那处；Windows 的真身是 canonicalize 出的 `\\?\` verbatim 路径，
/// 交给 installer 前还原。纯字符串、不碰 fs。
pub(super) fn kimi_native_install_root(bin_path: &str, real_target: &str) -> Option<String> {
    let normalized_real = real_target.replace('\\', "/").to_ascii_lowercase();
    if normalized_real.contains("/node_modules/") {
        return None;
    }
    #[cfg(not(target_os = "windows"))]
    if is_brew_managed_path(real_target) {
        return None;
    }
    let real = match real_target.strip_prefix(r"\\?\UNC\") {
        Some(unc) => format!(r"\\{unc}"),
        None => real_target
            .strip_prefix(r"\\?\")
            .unwrap_or(real_target)
            .to_string(),
    };
    let root = [real.as_str(), bin_path].into_iter().find_map(|path| {
        if !matches!(last_path_segment_lower(path).as_str(), "kimi" | "kimi.exe") {
            return None;
        }
        let bin_dir = parent_dir(path);
        if last_path_segment_lower(&bin_dir) != "bin" {
            return None;
        }
        let root = parent_dir(&bin_dir);
        (!root.is_empty()).then_some(root)
    });
    root
}

pub(super) fn kimi_install_dir(home: Option<&Path>) -> Option<PathBuf> {
    std::env::var_os("KIMI_INSTALL_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".kimi-code")))
}

pub(super) fn is_kimi_script_install(launcher: &Path, real: &Path, expected: &Path) -> bool {
    launcher
        .to_str()
        .zip(real.to_str())
        .and_then(|(launcher, real)| kimi_native_install_root(launcher, real))
        .map(PathBuf::from)
        .is_some_and(|root| {
            root == expected
                || root
                    .canonicalize()
                    .ok()
                    .is_some_and(|root| expected.canonicalize().ok() == Some(root))
        })
}

/// 旧版 Kimi Code（见 `KIMI_UPGRADE_YES_MIN_VERSION`）的升级命令：跳过会假成功的
/// `kimi upgrade --yes`。包管理器认得的安装（npm / Volta / bun / Homebrew）走锚定的包管理器；
/// 先查它，因为 Volta 的 `bin/kimi[.exe]` shim 不解析到 `node_modules`，会被误认成原生
/// 布局、让 installer 覆盖 shim。其余原生安装重跑官方 installer。都认不出时返回 None，
/// 交回普通锚定。
fn kimi_legacy_update_command(bin_path: &str, real_target: &str) -> Option<String> {
    #[cfg(not(target_os = "windows"))]
    let package_command =
        package_manager_anchored_command_from_paths("kimi", bin_path, real_target);
    #[cfg(target_os = "windows")]
    let package_command = package_manager_anchored_command_from_paths("kimi", bin_path);
    package_command.or_else(|| {
        kimi_native_install_root(bin_path, real_target)
            .map(|root| kimi_installer_update_command(&root))
    })
}

/// 按 installer 写在 `<root>/install.json` 的回执认领安装目录——自定义
/// `MCODE_INSTALL_DIR` 的目录名不固定，只能靠它（schema 1/2 都有 `product` /
/// `updateOwner` / `prefix`）。返回回执里的 `prefix` 原值：installer 以
/// `path.resolve(prefix) === path.resolve(MCODE_INSTALL_DIR)` 判定归属，传原值才会
/// 原地升级而非走 repair 迁移。要求 `prefix` 与回执所在目录是同一目录，挡住拷贝来的回执。
pub(super) fn mcode_receipt_install_root(entry: &str, layout: McodeLayout) -> Option<String> {
    let root = std::path::PathBuf::from(mcode_install_root_candidate(entry, layout));
    let text = std::fs::read_to_string(root.join("install.json")).ok()?;
    // install.ps1 用 `Set-Content -Encoding UTF8` 写回执，Windows PowerShell 5.1 会带
    // UTF-8 BOM；serde_json 不跳过 BOM，不剥掉就在第 1 列报错、退回全局 npm。
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let receipt: serde_json::Value = serde_json::from_str(text).ok()?;
    let field = |key: &str| receipt.get(key).and_then(serde_json::Value::as_str);
    if field("product")? != "minimax-code" || field("updateOwner")? != "npm-prefix" {
        return None;
    }
    let prefix = field("prefix")?;
    let same_dir = std::fs::canonicalize(prefix).ok()? == std::fs::canonicalize(&root).ok()?;
    same_dir.then(|| prefix.to_string())
}

// Reference releases-v1 launcher emitted by the official installer, verified
// against the isolated Pi1.6.1 payload. Compare executable statements below;
// harmless comment/indent changes do not change the dispatch identity.
#[cfg(not(target_os = "windows"))]
const PI_RELEASE_SHELL_LAUNCHER: &str = r#"#!/bin/sh
case "$0" in
  */*) pi_launcher="$0" ;;
  *) pi_launcher=$(command -v "$0") || exit 127 ;;
esac
while [ -L "$pi_launcher" ]; do
  pi_link=$(readlink "$pi_launcher") || exit 1
  case "$pi_link" in
    /*) pi_launcher="$pi_link" ;;
    *) pi_launcher=${pi_launcher%/*}/$pi_link ;;
  esac
done
pi_bin_dir=${pi_launcher%/*}
pi_agent_dir=${pi_bin_dir%/*}
pi_current_file=$pi_agent_dir/install/current-version
if ! IFS= read -r pi_current_version < "$pi_current_file"; then
  printf 'Could not read managed Pi version from %s.\n' "$pi_current_file" >&2
  exit 1
fi
case "$pi_current_version" in
  ""|.|..|*[!0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz._+-]*)
    printf 'Managed Pi version file is invalid: %s\n' "$pi_current_file" >&2
    exit 1
    ;;
esac
pi_release_dir=$pi_agent_dir/install/releases/$pi_current_version
pi_release_bin=$pi_release_dir/node_modules/.bin/pi
if [ ! -x "$pi_release_bin" ]; then
  printf 'Managed Pi executable is missing: %s\n' "$pi_release_bin" >&2
  exit 1
fi
# Node.js installed by the Pi installer is not added to shell profiles, so put
# it on PATH for pi's shebang and for child processes: npm in pi update and
# commands run through pi's bash tool.
pi_node_bin=${XDG_DATA_HOME:-$HOME/.local/share}/pi-node/current/bin
if [ -x "$pi_node_bin/node" ]; then
  PATH=$pi_node_bin:$PATH
  export PATH
fi
PI_MANAGED_INSTALL_ROOT=$pi_agent_dir/install
export PI_MANAGED_INSTALL_ROOT
exec "$pi_release_bin" "$@"
"#;

/// Recognize both the original JS launcher and the current releases-v1 shell
/// launcher. A directory name alone does not establish installer ownership.
pub(super) fn is_pi_managed_install(entry: &str) -> bool {
    // Preserve the original lexical and canonical sidecar/receipt recognition
    // for JS launchers; only the shell layout must follow shell dispatch paths.
    let legacy = |path: &Path| {
        // A leftover JS sidecar must not bypass dispatch checks for the new
        // shell launcher after an in-place upgrade.
        if pi_install_text(path, 16 * 1024).is_some_and(|text| {
            text.lines().any(|line| {
                line.trim_matches([' ', '\t'])
                    == "pi_current_file=$pi_agent_dir/install/current-version"
            })
        }) {
            return false;
        }
        path.parent()
            .filter(|bin| bin.join("pi-launcher.js").is_file())
            .and_then(Path::parent)
            .and_then(pi_managed_receipt)
            .is_some()
    };
    if legacy(Path::new(entry)) || std::fs::canonicalize(entry).is_ok_and(|path| legacy(&path)) {
        return true;
    }
    #[cfg(not(target_os = "windows"))]
    let entry = {
        // The shell resolves links on the launcher itself, but preserves directory
        // aliases. Match that dispatch path rather than canonicalizing parents.
        let mut path = PathBuf::from(entry);
        if !path.is_absolute() {
            return false;
        }
        for depth in 0..=32 {
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                return false;
            };
            if !metadata.file_type().is_symlink() {
                break;
            }
            if depth == 32 {
                return false;
            }
            let Ok(link) = std::fs::read_link(&path) else {
                return false;
            };
            path = if link.is_absolute() {
                link
            } else {
                path.parent().unwrap_or_else(|| Path::new("/")).join(link)
            };
        }
        path
    };
    #[cfg(target_os = "windows")]
    let entry = PathBuf::from(entry);
    let Some(bin_dir) = entry.parent() else {
        return false;
    };
    let Some(agent_dir) = bin_dir.parent() else {
        return false;
    };
    #[cfg(not(target_os = "windows"))]
    let marker = agent_dir.join("install/managed-install.json");
    let Some(receipt) = pi_managed_receipt(agent_dir) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        drop(receipt);
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        let verify = || -> Option<()> {
            if receipt.get("schemaVersion")?.as_u64()? != 1
                || receipt.get("layout")?.as_str()? != "releases-v1"
            {
                return None;
            }
            use std::os::unix::fs::PermissionsExt;
            let entry_metadata = std::fs::metadata(&entry).ok()?;
            if !entry_metadata.is_file() || entry_metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
            let launcher = pi_install_text(&entry, 16 * 1024)?;
            let statements = launcher
                .split('\n')
                .map(|line| line.trim_matches([' ', '\t']))
                .filter(|line| !line.is_empty() && !line.starts_with('#'));
            let reference = PI_RELEASE_SHELL_LAUNCHER
                .split('\n')
                .map(|line| line.trim_matches([' ', '\t']))
                .filter(|line| !line.is_empty() && !line.starts_with('#'));
            if !launcher.starts_with("#!/bin/sh\n") || !statements.eq(reference) {
                return None;
            }
            let agent_root = std::fs::canonicalize(agent_dir).ok()?;
            let managed_root = std::fs::canonicalize(agent_dir.join("install")).ok()?;
            let releases_root = std::fs::canonicalize(agent_dir.join("install/releases")).ok()?;
            let marker = std::fs::canonicalize(marker).ok()?;
            let current = std::fs::canonicalize(agent_dir.join("install/current-version")).ok()?;
            for (child, parent) in [
                (&managed_root, &agent_root),
                (&releases_root, &managed_root),
                (&marker, &managed_root),
                (&current, &managed_root),
            ] {
                if child == parent || !child.starts_with(parent) {
                    return None;
                }
            }
            let version = pi_install_current_version(&current)?;
            if version.is_empty()
                || matches!(version.as_str(), "." | "..")
                || !version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
            {
                return None;
            }
            let release = agent_dir.join("install/releases").join(&version);
            let release_root = std::fs::canonicalize(&release).ok()?;
            let package = tool_spec("pi").npm_package?;
            let package_dir = release.join("node_modules").join(package);
            let owner = std::fs::canonicalize(&package_dir).ok()?;
            let manifest_path = std::fs::canonicalize(package_dir.join("package.json")).ok()?;
            let bin_root = std::fs::canonicalize(release.join("node_modules/.bin")).ok()?;
            for (child, parent) in [
                (&release_root, &releases_root),
                (&owner, &release_root),
                (&manifest_path, &owner),
                (&bin_root, &release_root),
            ] {
                if child == parent || !child.starts_with(parent) {
                    return None;
                }
            }
            let manifest: serde_json::Value =
                serde_json::from_str(&pi_install_text(&manifest_path, 128 * 1024)?).ok()?;
            if manifest.get("name")?.as_str()? != package
                || manifest.get("version")?.as_str()? != version
            {
                return None;
            }
            let target = std::fs::canonicalize(release.join("node_modules/.bin/pi")).ok()?;
            let metadata = std::fs::metadata(&target).ok()?;
            if !metadata.is_file()
                || metadata.permissions().mode() & 0o111 == 0
                || !target.starts_with(owner)
            {
                return None;
            }
            Some(())
        };
        verify().is_some()
    }
}

#[cfg(not(target_os = "windows"))]
fn pi_install_current_version(path: &Path) -> Option<String> {
    use std::io::{BufRead, Read};
    if !std::fs::metadata(path).is_ok_and(|meta| meta.is_file()) {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let mut line = Vec::new();
    std::io::BufReader::with_capacity(256, file.take(257))
        .read_until(b'\n', &mut line)
        .ok()?;
    if line.len() > 256 || line.pop()? != b'\n' {
        return None;
    }
    String::from_utf8(line).ok()
}

fn pi_managed_receipt(agent_dir: &Path) -> Option<serde_json::Value> {
    let text = pi_install_text(&agent_dir.join("install/managed-install.json"), 128 * 1024)?;
    let receipt: serde_json::Value =
        serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()?;
    (receipt.get("kind").and_then(serde_json::Value::as_str) == Some("pi-managed-install"))
        .then_some(receipt)
}

fn pi_install_text(path: &Path, limit: u64) -> Option<String> {
    use std::io::Read;
    if !std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() <= limit) {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= limit).then_some(text)
}

/// 含空格才用 POSIX 单引号包一层,否则保持裸路径——命令展示更干净。
/// claude / brew / volta / bun / npm 五个锚定分支共用,避免"含空格"判定漂移。
///
/// **仅按空格判定,不防其他 shell 元字符**(`$` / `` ` `` / `'` / `"` / `;` 等)。
/// 调用方传入的是探测得到的可执行路径(`enumerate_tool_installations` 里来源于
/// `Path::display()`),实际 macOS/Linux 上 home dir 名几乎不允许这类字符、
/// npm/brew/volta/bun 也不会装到含这类字符的路径,与 diff 前内联在 npm 分支里的
/// `if npm.contains(' ')` 实现等价。若未来要扩广,改成 `shell_single_quote` 无条件
/// 包裹即可,但会失去"无空格时的清洁展示"。
#[cfg(not(target_os = "windows"))]
fn quote_path_if_spaced(p: &str) -> String {
    if p.contains(' ') {
        shell_single_quote(p)
    } else {
        p.to_string()
    }
}

/// 锚定路径走 `.bat` 文件且**被 `call` 调用**,需要为 batch 特殊字符做两层防御:
///
/// **(1) `%` 经历两轮 percent expansion → 用 4 个 `%` 转义**。.bat 中字面 `%` 的
/// 标准转义是 `%%`,但 `call` 命令(Microsoft `call /?`:"percent (%) expansion is
/// performed on each parameter")**在 batch parser 处理完 `%%` → `%` 后自己再做一轮**。
/// 所以源 .bat 里写 `%%FOO%%`,batch 一轮变 `%FOO%`,call 二轮当成 variable reference
/// 又展开一次——要让最终 call 看到字面 `%FOO%` 必须写 `%%%%FOO%%%%`(一轮 → `%%FOO%%`,
/// 二轮 → `%FOO%` 字面)。这是 cmd 唯一**引号无法保护**的字符:引号内的 `%` 仍参与
/// 两轮 expansion。
///
/// **(2) token 边界 / escape 字符触发外层双引号**:`' '` `'&'` `'('` `')'` `'^'`
/// `';'` `'<'` `'>'` `'|'` `','` 任一出现即包引号。NTFS 允许这些字符出现在路径中,
/// 不包会让 cmd 把路径切成多 token、`^` 又会触发 escape;引号内它们是字面意义,
/// 而且 call 二次解析对引号内的它们也不会做特殊处理(`^` 在引号内失去 escape 作用,
/// token 边界字符在引号内是字面)。
///
/// `!`(delayed expansion)只在 `setlocal enabledelayedexpansion` 下生效——我们
/// .bat 头只有 `chcp 65001` 和 `@echo off`、没开,所以不需要处理。`'` 在 cmd 中无特殊意义。
///
/// 镜像 POSIX `quote_path_if_spaced` 的"轻量条件包装"语义:不含任何特殊字符就保持
/// 裸路径(命令展示更干净),否则用 `win_double_quote` 包并做必要转义。
#[cfg(target_os = "windows")]
pub(super) fn win_quote_path_for_batch(p: &str) -> String {
    // `%` 经历两轮 expansion:.bat parser 一轮 + `call` 二轮(Microsoft `call /?`:
    // "percent (%) expansion is performed on each parameter")。要让 call 最终看到
    // 字面 `%` 需要 4 个 → `%%%%`(batch 一轮 → `%%`,call 二轮 → `%` 字面)。
    // 引号内仍参与两轮 expansion,所以这一步独立于外层引号、必须无条件做。
    let escaped = if p.contains('%') {
        p.replace('%', "%%%%")
    } else {
        p.to_string()
    };
    // 注:`needs_quote` 基于**原路径** `p` 判断,不能用 `escaped`——后者引入的 `%`
    // 字符不算"特殊触发字符",否则含 `%` 的路径会被错误地额外加引号。
    let needs_quote = p
        .chars()
        .any(|c| matches!(c, ' ' | '&' | '(' | ')' | '^' | ';' | '<' | '>' | '|' | ','));
    if needs_quote {
        win_double_quote(&escaped)
    } else {
        escaped
    }
}

/// Windows 版 sibling 推导:在 `<bin_path 父目录>` 下按 `ext_candidates` 顺序找
/// 第一个存在的 `<exe_basename>.<ext>` 文件,返回该绝对路径。
///
/// **与 POSIX `sibling_bin` 的关键区别:这里碰 fs**——Windows 上 npm/pnpm 的入口
/// 实际扩展名可能是 `.cmd` 也可能是 `.exe`(Node.js installer 装的是 `npm.cmd`、
/// 部分 pnpm 是 `pnpm.exe`),纯字符串拼接无法知道哪个真的存在,猜错会拼出
/// "GUI 执行时 file not found" 的命令。fs 检查放进 helper、单测用 tempdir 覆盖,
/// 让上层 `anchored_command_from_paths` 仍保持"接收已锚定路径"的接口形态。
///
/// **TOCTOU 是 by design**:预检 `is_file` 是为了让确认对话框展示真实命令字符串;
/// 检查到执行之间被外部进程(卸载器 / nvm switch / 杀软隔离)移走文件 → cmd /C
/// 报 ENOENT,toast 显示错误。不要在执行前再做二次预检——双重 syscall 也解决不了 race。
///
/// 候选扩展名顺序按工具 idiom:npm/pnpm 优先 `.cmd`(node 装的),volta 优先 `.exe`
/// (Volta 是 Rust 写的 native binary)。
///
/// **不用 `which::which_in` 的理由**:per-tool 扩展名优先级(volta 偏 `.exe`、npm/pnpm
/// 偏 `.cmd`)与 PATHEXT 的固定顺序不一致,而且只为这一处加 `which` 依赖收益不抵 audit
/// surface。`PathBuf::join` 让 separator 选择交给 std,避免 `format!("{dir}\\...")`
/// 硬编码 `\\` 在混合分隔符 bin_path 下产出丑陋路径。
///
/// 空 dir 或所有候选都不存在 → None,上游退化到静态命令,与 POSIX 路径同款语义。
#[cfg(target_os = "windows")]
pub(super) fn sibling_bin_with_ext(
    bin_path: &str,
    exe_basename: &str,
    ext_candidates: &[&str],
) -> Option<String> {
    let dir = parent_dir(bin_path);
    if dir.is_empty() {
        return None;
    }
    let dir = std::path::PathBuf::from(dir);
    for ext in ext_candidates {
        let candidate = dir.join(format!("{exe_basename}.{ext}"));
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// 返回 `<bin_path 同目录>/<exe>` 的绝对路径。bin_path 是命令行命中的入口
/// (如 `/opt/homebrew/bin/gemini`、`~/.volta/bin/codex`),`exe` 是与之共处一个
/// bin 目录的另一个可执行(`brew` / `volta` / `bun` / `npm`)——这些包管理器
/// 都把自己的 cli 跟它们安装的命令并列放在同一个 bin 目录,所以"同目录推导"
/// 是可靠的绝对路径来源。
///
/// **dir 为空(bin_path 不含 `/`) → 返回 None**:此时无法推导出绝对路径,让上游
/// `anchored_command_from_paths` 整体退化为 None,调用方落到静态命令兜底——而非
/// 悄悄拼出 `npm i -g <pkg>` 这种依赖 PATH 的指令,违背"必须绝对路径"不变量。
/// 实际从 `enumerate_tool_installations` 走的 bin_path 都是 `Path::display()` 出
/// 来的绝对路径,这条防线不期望被触发,但闭合了 helper 与函数文档的语义一致。
#[cfg(not(target_os = "windows"))]
pub(super) fn sibling_bin(bin_path: &str, exe: &str) -> Option<String> {
    let dir = parent_dir(bin_path);
    if dir.is_empty() {
        None
    } else {
        Some(format!("{dir}/{exe}"))
    }
}

/// Build an npm command anchored to the same Node installation as `bin_path`.
///
/// npm's POSIX launcher is normally a JavaScript file with an
/// `#!/usr/bin/env node` shebang. Calling npm by absolute path therefore does
/// not make it independent of PATH: `env` still needs to find `node`. GUI apps
/// commonly inherit only the system PATH, while nvm/fnm/mise keep Node in a
/// user directory. Prefixing npm's sibling directory makes both npm and its
/// transitive Node interpreter resolve to the installation we selected.
#[cfg(not(target_os = "windows"))]
fn anchored_npm_command(bin_path: &str, args: &str) -> Option<String> {
    let dir = parent_dir(bin_path);
    if dir.is_empty() {
        return None;
    }
    let npm = sibling_bin(bin_path, "npm")?;
    Some(format!(
        "PATH={}:\"$PATH\" {} {args}",
        shell_single_quote(&dir),
        quote_path_if_spaced(&npm)
    ))
}

/// POSIX 官方自升级内部再调 npm 时，用环境变量放行该包的 postinstall 与 optional
/// dependency（含义同 `npm_install_extra_args`），只作用于紧随其后的那条命令。
#[cfg(not(target_os = "windows"))]
fn npm_scripts_env_prefix(pkg: &str) -> String {
    format!("npm_config_allow_scripts={pkg} npm_config_ignore_scripts=false npm_config_include=optional ")
}

#[cfg(not(target_os = "windows"))]
fn anchored_official_update_command(tool: &str, bin_path: &str) -> Option<String> {
    tool_spec(tool).official_update_args.map(|args| {
        let executable = if tool == "pi" {
            shell_single_quote(bin_path)
        } else {
            quote_path_if_spaced(bin_path)
        };
        format!("{executable} {args}")
    })
}

#[cfg(target_os = "windows")]
fn anchored_official_update_command(tool: &str, bin_path: &str) -> Option<String> {
    tool_spec(tool)
        .official_update_args
        .map(|args| format!("{} {args}", win_quote_path_for_batch(bin_path)))
}

/// Grok Build 原生安装的升级命令 = `<bin 绝对> update || <官方 installer>`。
///
/// **为什么唯独 native Grok 的 self-update 需要 fallback**（claude native / hermes 都没有）：
/// `grok update` 虽然是自包含 Rust 二进制的子命令，**却把 npm 当成自己的分发管道**——
/// 先 spawn `npm view @xai-official/grok version --json` 查最新版，再 spawn
/// `npm i -g @xai-official/grok@<version>` 安装，由该包的 `postinstall.js` 从平台 optional
/// 依赖里解出二进制、安置成 `~/.grok/bin/grok-<version>` 并 relink `grok`。而 `npm` 自身是
/// `#!/usr/bin/env node` 脚本 → **native 安装也隐式硬依赖 PATH 里同时有 `npm` + `node`**。
/// GUI 进程 PATH 由 launchd 给、`run_tool_lifecycle_silently` 又是非登录 `bash -c`，
/// nvm/homebrew 下的 node+npm 均不可见 → grok 内部 spawn 得到 ENOENT，只向用户抛出
/// 费解的 `Error: No such file or directory (os error 2)`（实测复现）。
///
/// **这是上游换了机制**：0.2.111 及更早版本自更新是直接下载二进制到 `~/.grok/downloads/`
/// （彼时 `is_grok_native_install` 假设的 "native = 不碰 npm" 成立），0.2.112 起改走上述 npm
/// 管道（落点随之从 `downloads/` 变为 `bin/`）。**副作用**：npm 全局包那一处安装是
/// `grok update` 自己装出来的，非用户手动所为，故 native 用户也会被 enumerate 到两处；
/// 两处由同一次 postinstall 同步，版本恒等。
///
/// 这正是 `anchored_command_from_paths` 那条"绝对路径 + 必要时把解释器目录放 PATH 首位"
/// 不变量没覆盖的第三类：**执行体自身既不需要解释器、也已用绝对路径，却在内部再 spawn
/// 第三方 CLI**。`login_shell_path` 的 PATH 注入已让绝大多数机器上的 primary 直接成功；
/// 这条 fallback 覆盖的是"这台机器根本没装 node"——用官方 installer 装的用户完全可能如此。
///
/// **fallback 必须是官方 installer，不能是 `npm i -g`**：后者与 primary **同源**——primary
/// 失败的两种现实原因（本机无 node；npm registry 指向未同步该 tarball 的镜像，见
/// npmmirror dist-tag 事故）都会让 npm fallback 一并失败，`||` 形同虚设。官方 installer
/// 是唯一 node-free 的独立路径（只需 `curl`，在 `/usr/bin`，窄 PATH 下可用），且落点同为
/// `~/.grok/bin`，锚定语义分毫不动 —— 真正的降级冗余要求 fallback 与 primary **失败模式不相关**。
///
/// **这条 fallback 还兼具第三重作用：修复上游锚点（勿在重构时丢掉）**。
/// grok 的更新路径由 `~/.grok/config.toml` 的 `[cli] installer` 决定（`npm` / `internal` /
/// `gh-release`），而官方 install.sh 会**无条件把该字段覆写为 `internal`**（其 awk 段落先插入
/// `installer = "internal"`、再跳过 `[cli]` 段里已有的 `installer`/`channel` 行）。于是：
/// 用户一旦因 install 端的 npm fallback 被切进 npm 模式（postinstall.js 会写 `installer = "npm"`
/// 并在每次 npm 更新时重写，自我巩固），只要 npm 路径出任何问题，这里的 `||` 就会把他拉回
/// node-free 的 internal 模式，并顺带修好 npm 模式漏更新的 `~/.grok/bin/agent` launcher。
/// **实测验证**（2026-07-30，窄 PATH + `installer = "npm"`）：primary 抛 `os error 2` → fallback
/// 接管 → 装上最新版 → config 写回 `internal` → agent 对齐 → `.zshrc` 幂等更新不重复。
/// ⇒ install 端保留 npm fallback 是安全的（它是 x.ai 不可达时的唯一退路，防火墙场景需要），
/// 其副作用由本链自愈；**把这里换成 npm fallback 会同时废掉降级冗余和这条自愈路径**。
#[cfg(not(target_os = "windows"))]
fn grok_native_update_command(update: String) -> String {
    chain_update_commands(
        update,
        GROK_INSTALL_UNIX.to_string(),
        LifecycleCommandShell::Posix,
    )
}

/// Windows 版同上，fallback 换成官方 PowerShell installer。
/// **不走 `chain_update_commands`**：它会给 `||` 右侧加 `call`，而这里的 fallback 是
/// `powershell.exe`（不是 `.cmd`/`.bat`），不需要 `call`——与 `hermes_update_windows_command`
/// 同一理由。
#[cfg(target_os = "windows")]
fn grok_native_update_command(update: String) -> String {
    format!("{update} || {}", grok_install_windows_command())
}

/// 哪些工具的"官方 self-update"优先于包管理器升级（生成 `<tool> update || <pkg-mgr>`）。
///
/// **codex 刻意不在此列**：`codex update` 在 npm 安装上只是裸 `npm install -g
/// @openai/codex`（无 `@latest` / `--include=optional` / 不先卸载），却只检查 exit code、
/// 无条件打印 “Update ran successfully”。当 npm 把平台二进制 optional 依赖
/// `@openai/codex-<triple>` 漏装时它仍 **exit 0 假成功**，使外层 `||` 兜底被短路、损坏被
/// 成功 toast 掩盖（用户报告的 “Missing optional dependency” 即源于此）。因此 codex 一律走
/// npm 锚定升级；真正损坏（`runnable=false`）时由 `installs_anchored_command` 的门控改用
/// `codex_repair_command` 的 uninstall+install 自愈，而非交给 codex 自身的 self-update。
fn prefers_official_update(tool: &str, shell: LifecycleCommandShell) -> bool {
    match shell {
        LifecycleCommandShell::Posix => {
            matches!(
                tool,
                "claude" | "opencode" | "openclaw" | "kimi" | "omp" | "codebuddy"
            )
        }
        LifecycleCommandShell::WindowsBatch => {
            matches!(
                tool,
                // OpenCode 的 Windows `upgrade` 在 anomalyco/opencode#17295 修复前可能因
                // 安装方式探测失败弹交互 prompt（spawn npm.cmd 没传 shell:true）；静默
                // lifecycle 没有 stdin 会挂死，Windows 先锚到包管理器路径，等上游修了
                // 再把 opencode 加回这里。
                "claude" | "openclaw" | "kimi" | "omp" | "codebuddy"
            )
        }
    }
}

/// Codex 平台分发包损坏的自愈命令。Codex 的 npm 包是「主包 `@openai/codex`（纯 JS
/// launcher）+ 平台二进制 optional 依赖 `@openai/codex-<triple>`」的分发模式（同 esbuild/swc）。
/// 当平台二进制缺失时 codex 跑不起来——`enumerate_tool_installations` 跑 `--version` 会拿到
/// “Missing optional dependency” 的非 0 退出，标记 `runnable=false`。此状态下普通
/// `npm i -g @pkg@latest` 是 **no-op**：npm 视 optional 依赖缺失为非致命，reify 又认为主包已是
/// 最新（外加半损坏留下的空 nested `node_modules` 残骸强化「tree 已满足」判断），不会补回平台
/// 二进制。唯一实测可靠的修复是先 `uninstall` 清掉残骸、再 `install` 装回完整的主包 + 平台二进制
/// （实测输出 `added 2 packages`）。
///
/// 锚定到与 codex 入口同目录的 npm，并把该目录放到每条 npm 命令的 PATH 首位，使 npm
/// 的 `#!/usr/bin/env node` 能找到同一安装下的 node，不依赖 GUI 非登录进程的 PATH。
/// `|| true` 让 uninstall 失败（如 nvm 上对半损坏包静默返回非 0）不触发外层 `set -e`
/// 中止，但随后的 install 若失败仍会被 `set -e` 捕获并上报给前端 toast。
///
/// **仅对会锚定到 sibling npm 的 node 管理器来源（nvm/fnm/mise/homebrew npm）生效**：
/// `runnable=false` 是宽信号（权限 / node 版本 / 任意 `--version` 失败皆可触发），非 npm
/// 全局安装各有自己的二进制分发与修复方式，无脑套 npm uninstall+install 会出错——Homebrew
/// formula / cask（real 在 `Cellar/` 或 `Caskroom/`）本应 `brew upgrade`，npm 够不到它反而旁路装第二份 npm
/// 全局 codex；Volta/Bun 本应 `volta install`/`bun add`，且 `~/.bun/bin` 下没有 npm、
/// `sibling_bin` 会拼出不存在的路径；system/未知来源无可靠 sibling npm。这些来源一律返回
/// None，让上游继续走 source-specific 的 `anchored_command_from_paths`。白名单与
/// `package_manager_anchored_command_from_paths` 的 sibling-npm 分支对齐。
/// 刻意**不**额外用 `inst.error` 文本确认「确系缺二进制」：enumerate 只保留 stderr 末尾 4 行，
/// 而 codex.js 抛错的 "Missing optional dependency" 行会被尾部 node stack `at ...` 行挤出窗口
/// （实测用户原始错误即如此），强加该条件反而漏修真实缺包；对 npm 全局安装，uninstall+install
/// 对各类损坏都是合理且不会更糟的修复。
#[cfg(not(target_os = "windows"))]
fn codex_repair_command(bin_path: &str, real: &str) -> Option<String> {
    // brew formula / cask（real 在 Cellar 或 Caskroom）→ 不归 npm 管，交回 anchored 走 brew upgrade。
    // 官方独立安装同理：`CODEX_INSTALL_DIR` 可以指到 Homebrew / nvm 的 bin 目录，入口来源
    // 会被判成 homebrew / nvm，但真身在 standalone 发布包里，交回 anchored 重跑 installer。
    if is_brew_managed_path(real) || codex_standalone_install(bin_path, real).is_some() {
        return None;
    }
    // 只认会落到 sibling npm 的 node 管理器来源；volta/bun/system/未知交回 anchored。
    if !matches!(
        infer_install_source(Path::new(bin_path)),
        "nvm" | "fnm" | "mise" | "homebrew"
    ) {
        return None;
    }
    let pkg = "@openai/codex";
    let uninstall = anchored_npm_command(bin_path, &format!("uninstall -g {pkg}"))?;
    let install = anchored_npm_command(bin_path, &format!("i -g {pkg}@latest"))?;
    Some(format!("{uninstall} || true; {install}"))
}

/// Windows 暂不做平台分发自愈：Windows 上 codex 的破坏模式不同（EPERM 文件锁 / 版本 bump
/// 残留，见 openai/codex#21872、#19824），且 `.bat` 链的错误处理与 POSIX `set -e` 语义不同，
/// 需要单独设计；先在本问题实际发生的 POSIX 平台落地。返回 None → 上游走正常锚定命令。
#[cfg(target_os = "windows")]
fn codex_repair_command(_bin_path: &str, _real: &str) -> Option<String> {
    None
}

#[cfg(not(target_os = "windows"))]
fn package_manager_anchored_command_from_paths(
    tool: &str,
    bin_path: &str,
    real_target: &str,
) -> Option<String> {
    // Cask 必须先于 formula：两者互斥（真身不会同时落在 Caskroom 和 Cellar），
    // 但先拦 cask 能避免未来路径里同时出现这两个段时误走 formula。
    if let Some(cask) = brew_cask_from_path(real_target) {
        let brew = sibling_bin(bin_path, "brew")?;
        return Some(format!(
            "{} upgrade --cask {cask}",
            quote_path_if_spaced(&brew)
        ));
    }
    if let Some(formula) = brew_formula_from_path(real_target) {
        let brew = sibling_bin(bin_path, "brew")?;
        return Some(format!("{} upgrade {formula}", quote_path_if_spaced(&brew)));
    }
    let NpmPackage {
        name: pkg,
        install_args,
    } = npm_package_for_install(tool, bin_path, real_target)?;
    let source = infer_install_source(Path::new(bin_path));
    // omp 的 npm 包只能由 bun 运行：除 bun 外的 node 管理器装不出能跑的 omp。
    if tool == "omp" && source != "bun" {
        return None;
    }
    match source {
        "volta" => {
            let volta = sibling_bin(bin_path, "volta")?;
            return Some(format!("{} install {pkg}", quote_path_if_spaced(&volta)));
        }
        "bun" => {
            let bun = sibling_bin(bin_path, "bun")?;
            return Some(format!(
                "{} add -g {pkg}@latest",
                quote_path_if_spaced(&bun)
            ));
        }
        // 自带同级 npm 的 node 管理器：落到下面锚定到那处的 npm。
        "nvm" | "fnm" | "mise" | "homebrew" => {}
        // system / 未知来源通常没有同级 npm，不能拼 `<dir>/npm`。若工具有官方
        // self-update，上层会直接锚到 CLI 自身；否则返回 None 走静态兜底。
        _ => return None,
    }
    anchored_npm_command(bin_path, &format!("i -g {pkg}@latest{install_args}"))
}

/// 给定工具、原始 bin 路径（命令行命中的入口）、canonicalize 后的真身路径，
/// 推断"写回同一处"的锚定升级命令。**POSIX 版是纯函数（不碰 FS）**——真实 canonicalize
/// 由调用方做（`installs_anchored_command` 复用 enumerate 时算出的 `inst.real`),
/// 便于单测覆盖各包管理器分支。Windows 版同名函数因 sibling 扩展名歧义必须读 fs,
/// 是刻意保留的平台差异(详见 Windows 版本 doc)。
///
/// **关键不变量：返回的命令必须用绝对路径调用执行体；若执行体通过
/// `#!/usr/bin/env` 查找解释器，还必须把其同级 bin 目录显式放到 PATH 首位**。
/// 这条命令最终在 `run_tool_lifecycle_silently` 的非登录 `bash -c` 里执行——
/// GUI App 启动的进程 PATH 由 launchd / Windows Service / systemd 给,通常**不含**
/// `~/.local/bin` / `/opt/homebrew/bin` / `~/.volta/bin` 等用户级 bin 目录;而探测
/// 阶段 `try_get_version` 用的是 `$SHELL -lic`(登录+交互式,会读 .zshrc/.zprofile),
/// 两者 PATH 不对称。裸 `claude update` / `brew upgrade ...` 在 GUI 进程里大概率
/// `command not found`(exit 127)→ `set -e` 中止 → 用户看到失败 toast,锚定决策却
/// 已展示给用户"将写回原生那处"——欺骗性故障。
///
/// 判定顺序（命中即返回）：
/// ① Hermes → `<bin_path 绝对> update`;Hermes CLI 自己知道安装环境,避免 cc-switch
///    猜系统 `python3`/`python` 时撞上 Python 版本或 pyenv shim 问题。
/// ② Claude / Grok 原生安装器 → `<bin_path 绝对> update`；
///    bin_path 指向 launcher,launcher 内部 dispatch update 子命令。它不归 npm 管,
///    且在 PATH 里比 nvm/homebrew 更靠前,用 npm 升级会装到别处且被原生那份遮蔽。
///    MiniMax Code 的脚本安装同理不归全局 npm 管,但 `mcode update` 非 TTY 下不安装
///    (见 `official_update_args` 上方注释),改为带 `MCODE_INSTALL_DIR` 重跑官方 installer。
///    Codex 独立安装器同理:`codex update` 断网时假成功(见 `CODEX_INSTALL_UNIX`),
///    改为带 `CODEX_INSTALL_DIR` / `CODEX_HOME` 重跑官方 installer。
/// ③ Homebrew formula / cask（真身在 `Cellar/<formula>/` 或 `Caskroom/<token>/`）
///    → `<bin_path 同目录>/brew upgrade [--cask] <name>`；由 Homebrew 拥有,避免
///    self-update 或 sibling npm 改动包管理器管理的安装。
/// ④ 其余支持官方自升级的工具 → `<bin_path 绝对> update/upgrade || <原锚定包管理器命令>`；
///    Codex 的 self-update 只在部分 release 可用,所以保留 npm/brew/bun/volta fallback。
/// ⑤ 不支持官方自升级的 npm 全局包(例如 Gemini CLI，以及非 native 的 Grok Build) → 锚定到
///    "那处 bin 目录的 npm"。
#[cfg(not(target_os = "windows"))]
pub(super) fn anchored_command_from_paths(
    tool: &str,
    bin_path: &str,
    real_target: &str,
) -> Option<String> {
    let real_lower = real_target.to_ascii_lowercase();

    if tool == "omp" && is_omp_standalone_install(Path::new(bin_path), Path::new(real_target)) {
        return omp_native_update_command(bin_path, &crate::config::get_home_dir());
    }

    // Antigravity 原生二进制自升级（Homebrew cask 也标了 auto_updates，`brew upgrade` 默认跳过它）。
    if tool == "hermes" || tool == "agy" {
        return anchored_official_update_command(tool, bin_path);
    }
    if tool == "claude"
        && (real_lower.contains("/.local/share/claude/")
            || real_lower.contains("/claude/versions/"))
    {
        return anchored_official_update_command(tool, bin_path);
    }
    if tool == "grok" && is_grok_native_install(bin_path, real_target) {
        // `[cli] installer = "npm"`（npm 包的 postinstall 写入）下，`grok update` 会 spawn
        // `npm i -g @xai-official/grok@<version>`，靠 postinstall relink `~/.grok/bin/grok`。
        // npm 12 拦截后仍 exit 0，`||` 后的官方 installer 不会接手，用户继续跑旧版。
        // 环境变量只作用于 `||` 左侧这一条；internal / gh-release 模式不经 npm，不受影响。
        return Some(grok_native_update_command(format!(
            "{}{}",
            npm_scripts_env_prefix("@xai-official/grok"),
            anchored_official_update_command(tool, bin_path)?
        )));
    }
    if tool == "mcode" {
        if let Some(root) = mcode_script_install_root(bin_path, real_target, McodeLayout::Posix) {
            return Some(mcode_installer_update_command(&root));
        }
    }
    if tool == "codex" {
        if let Some(install) = codex_standalone_install(bin_path, real_target) {
            return Some(codex_installer_update_command(&install));
        }
    }
    // Nix 安装 `omp update` 只提示去改 flake 就 exit 0，执行它会报假成功。
    if tool == "omp" && real_lower.starts_with("/nix/store/") {
        return None;
    }
    let package_command = package_manager_anchored_command_from_paths(tool, bin_path, real_target);
    if is_brew_managed_path(real_target) {
        return package_command;
    }
    if prefers_official_update(tool, LifecycleCommandShell::Posix) {
        let mut update = anchored_official_update_command(tool, bin_path)?;
        // npm-installed Claude / OpenCode delegate updates to npm. Keep the npm 12
        // postinstall and optional dependency settings scoped to this process
        // tree; other installation sources retain their own update policy.
        // Gate on the real target rather than the launcher's source so npm
        // prefixes classified as `system` (Intel Homebrew node, nodejs.org pkg,
        // ~/.npm-global, distro node) are covered too: they have no sibling-npm
        // fallback, so a false-success update there could never self-heal.
        // Bun-installed OpenCode upgrades through bun, not npm.
        let npm_scripts_package = match tool {
            "claude" => Some("@anthropic-ai/claude-code"),
            "opencode" if !real_lower.contains("/.bun/") => {
                npm_package_for_install(tool, bin_path, real_target).map(|pkg| pkg.name)
            }
            _ => None,
        };
        if let Some(pkg) = npm_scripts_package {
            if real_lower.contains(&format!("/node_modules/{pkg}/")) {
                update = format!("{}{update}", npm_scripts_env_prefix(pkg));
            }
        }
        return Some(match package_command {
            Some(fallback) => chain_update_commands(update, fallback, LifecycleCommandShell::Posix),
            None => update,
        });
    }
    package_command
}

#[cfg(target_os = "windows")]
fn package_manager_anchored_command_from_paths(tool: &str, bin_path: &str) -> Option<String> {
    // omp 的 npm 包只能由 bun 运行，这里的 npm / Volta / pnpm 都装不出能跑的 omp。
    if tool == "omp" {
        return None;
    }
    let NpmPackage {
        name: pkg,
        install_args,
    } = npm_package_for_install(tool, bin_path, bin_path)?;

    match infer_install_source(Path::new(bin_path)) {
        "volta" => {
            let volta = sibling_bin_with_ext(bin_path, "volta", &["exe", "cmd"])?;
            Some(format!(
                "{} install {pkg}",
                win_quote_path_for_batch(&volta)
            ))
        }
        "pnpm" => {
            let pnpm = sibling_bin_with_ext(bin_path, "pnpm", &["cmd", "exe"])?;
            Some(format!(
                "{} add -g {pkg}@latest",
                win_quote_path_for_batch(&pnpm)
            ))
        }
        // 兜底 = npm 类:Scoop / Chocolatey / winget / nvm-windows / MS Store nodejs /
        // system / 任何识别不到专属来源的 → sibling npm.cmd。
        _ => {
            let npm = sibling_bin_with_ext(bin_path, "npm", &["cmd", "exe"])?;
            Some(format!(
                "{} i -g {pkg}@latest{install_args}",
                win_quote_path_for_batch(&npm),
            ))
        }
    }
}

/// Windows 版锚定命令生成。对平台确认可静默运行的工具优先使用官方 CLI 自升级；
/// 对 npm/Volta/pnpm 这类可确认写回位置的安装，再接一个包管理器 fallback。不存在 brew/bun/claude-native
/// (Windows 没 Homebrew、Bun for Windows 仍 preview；Grok native 使用 PowerShell installer)。
/// Scoop/Chocolatey/winget/nvm-windows/MS Store node 都归 npm 类——它们都只是"如何装
/// node"的不同入口,全局包真正的 idiom 仍是 sibling `npm.cmd`。
///
/// **与 POSIX 版的语义差异**:POSIX 版是纯函数(不碰 fs),Windows 版通过
/// `sibling_bin_with_ext` 读 fs 来探明扩展名(`.cmd` vs `.exe`)——Node installer
/// 装 `.cmd`、Volta 装 `.exe`,纯字符串拼接无法消歧。这一平台差异**被刻意保留**:
/// 测试用 tempdir 隔离 fs,生产侧 TOCTOU 是 by design(见 `sibling_bin_with_ext` doc)。
///
/// `real_target` 维持与 POSIX 版的签名对称，并辅助识别 Grok native。若未来加 Scoop
/// persist 锚定(scoop 装的工具真身在 `<scoop_root>/persist/<app>/...`),也从这里取真身。
///
/// **关键不变量同 POSIX 版:返回的命令必须用绝对路径,不依赖 PATH**。Windows GUI
/// 进程 PATH 由 Service Control Manager / explorer.exe 给,通常不含用户 `%LOCALAPPDATA%`
/// 下的 Volta/pnpm 路径;`$SHELL -lic` 的探测时 PATH 与执行时 PATH 不对称。
///
/// 判定顺序(命中即返回):
/// ① hermes / Grok native → `<bin_path> update`;CLI 自己处理安装环境。
///    MiniMax Code 脚本安装 → 带 `MCODE_INSTALL_DIR` 重跑官方 PowerShell installer。
///    Codex 独立安装 → 带 `CODEX_INSTALL_DIR` / `CODEX_HOME` 重跑官方 PowerShell installer。
/// ② 支持官方自升级且 Windows 可安全静默执行的工具 → `<bin_path> update/upgrade || call <包管理器 fallback>`。
/// ③ 其余 npm 工具 → sibling `npm.cmd`/`.exe` i -g <pkg>@latest。
///
/// 包管理器 fallback 的 sibling 探测都通过 `sibling_bin_with_ext`(碰 fs):该处无候选
/// 扩展名存在时,支持官方自升级的工具仍返回 `<bin_path> update/upgrade`,其余工具
/// 才返 None 让上游兜回静态命令、`anchored=false`。
#[cfg(target_os = "windows")]
pub(super) fn anchored_command_from_paths(
    tool: &str,
    bin_path: &str,
    real_target: &str,
) -> Option<String> {
    if tool == "omp" && is_omp_standalone_install(Path::new(bin_path), Path::new(real_target)) {
        return omp_native_update_command(bin_path, &crate::config::get_home_dir());
    }
    // Antigravity 原生二进制自升级（Homebrew cask 也标了 auto_updates，`brew upgrade` 默认跳过它）。
    if tool == "hermes" || tool == "agy" {
        return anchored_official_update_command(tool, bin_path);
    }
    if tool == "grok" && is_grok_native_install(bin_path, real_target) {
        return Some(grok_native_update_command(
            anchored_official_update_command(tool, bin_path)?,
        ));
    }
    if tool == "mcode" {
        if let Some(root) = mcode_script_install_root(bin_path, real_target, McodeLayout::Windows) {
            return Some(mcode_installer_update_command(&root));
        }
    }
    if tool == "codex" {
        if let Some(install) = codex_standalone_install(bin_path, real_target) {
            return Some(codex_installer_update_command(&install));
        }
    }
    let package_command = package_manager_anchored_command_from_paths(tool, bin_path);
    if prefers_official_update(tool, LifecycleCommandShell::WindowsBatch) {
        let update = anchored_official_update_command(tool, bin_path)?;
        return Some(match package_command {
            Some(fallback) => {
                chain_update_commands(update, fallback, LifecycleCommandShell::WindowsBatch)
            }
            None => update,
        });
    }
    package_command
}

/// 基于已枚举的安装列表生成锚定升级命令（复用 enumerate 结果，避免二次探测）。
/// 读取 enumerate 时已 canonicalize 写入的 `inst.real`,**不再二次 canonicalize**——
/// 既消除冗余 syscall,也闭合"enumerate 与 anchor 看到同一真身"的一致性边界
/// (两次 canonicalize 之间 symlink 被换会让锚定指向不同真身)。
///
/// 全平台共用——`anchored_command_from_paths` 自身是 cfg 二选一(POSIX 五分支 /
/// Windows 三分支),这里只负责取默认那处 + 转发。
pub(super) fn installs_anchored_command(
    tool: &str,
    installs: &[ToolInstallation],
) -> Option<String> {
    let inst = default_install(installs)?;
    // The managed Pi launcher supplies its own release root and Node runtime.
    // A global npm fallback would install a second copy instead of updating it.
    if tool == "pi" {
        return plan_selected_tool_update(tool, inst).map(|plan| plan.command);
    }
    let real = inst.real.to_string_lossy();
    // Codex 平台分发包损坏自愈：主包在但平台二进制缺失时 codex 跑不起来
    // （runnable=false），此时正常锚定的 `npm i -g @latest` 是 no-op 修不好——改用
    // uninstall+install 重装补回平台二进制。**但仅限会锚定到 sibling npm 的 node 管理器
    // 来源**（codex_repair_command 内按 source/real 收窄，brew/volta/bun/system 交回下方
    // source-specific 锚定，避免误用 npm 重装）。runnable=true 的正常升级也走下方普通锚定
    // 路径（且因 codex 不在 prefers_official_update，不会再跑会假成功掩盖损坏的 `codex update`）。
    // 只认 `--version` 真的退出报错：探测超时 / 启动失败的那处可能是好的，先卸载再装
    // 一旦装失败就把旧版丢了；普通 `npm i -g` 失败时 npm 会回滚、旧版还在。
    if tool == "codex" && !inst.runnable && inst.exited_with_error {
        if let Some(cmd) = codex_repair_command(&inst.path, &real) {
            return Some(cmd);
        }
    }
    // MiniMax Code 脚本安装按回执认领（读 fs，故不放进纯函数 `anchored_command_from_paths`）。
    // 否则自定义 `MCODE_INSTALL_DIR` 会退到全局 `npm i -g`：单处安装不弹确认，静默升级了
    // 别处，命令行实际用的那处版本不变。
    if tool == "mcode" {
        if let Some(root) = [inst.path.as_str(), real.as_ref()]
            .into_iter()
            .find_map(|entry| mcode_receipt_install_root(entry, McodeLayout::NATIVE))
        {
            return Some(mcode_installer_update_command(&root));
        }
    }
    // 版本号来自 `--version` 探测，所以放在这里而不是纯路径的 `anchored_command_from_paths`。
    if tool == "kimi" && kimi_upgrade_ignores_yes(inst.version.as_deref()) {
        if let Some(cmd) = kimi_legacy_update_command(&inst.path, &real) {
            return Some(cmd);
        }
    }
    anchored_command_from_paths(tool, &inst.path, &real)
}

pub(super) fn npm_prerelease_requires_manual_update(
    tool: &str,
    launcher: &Path,
    real: &Path,
    version: Option<&str>,
) -> bool {
    npm_global_package(tool, launcher, real, cfg!(target_os = "windows")).is_some_and(
        |(_, manifest)| {
            version
                .or_else(|| manifest.get("version").and_then(serde_json::Value::as_str))
                .is_some_and(tool_version::version_is_prerelease)
        },
    )
}

/// Build a preview only when the updater and its installation root are known.
pub(super) fn plan_selected_tool_update(
    tool: &str,
    install: &ToolInstallation,
) -> Option<ToolUpdatePlan> {
    #[cfg(target_os = "windows")]
    if wsl_distro_for_tool(tool).is_some() {
        return None;
    }
    if (tool == "opencode" && install.source == "desktop")
        || desktop_managed_codex_cli(tool, &install.real)
        || npm_prerelease_requires_manual_update(
            tool,
            Path::new(&install.path),
            &install.real,
            install.version.as_deref(),
        )
        || (tool == "opencode"
            && tool_version::npm_package_for_version(tool, install.version.as_deref())
                == Some(OPENCODE_V2_NPM_PACKAGE)
            && opencode_update_is_manual(
                std::env::consts::OS,
                Some((Path::new(&install.path), &install.real)),
            ))
    {
        return None;
    }
    let launcher = Path::new(&install.path);
    let repair_codex = tool == "codex" && !install.runnable && install.exited_with_error;
    let (source, command) = if let Some(manager) = verified_volta_manager(launcher) {
        (
            "volta",
            volta_owned_update_command(tool, &manager, install.version.as_deref())?,
        )
    } else {
        selected_tool_update_command(tool, launcher, &install.real, repair_codex)?
    };
    Some(ToolUpdatePlan {
        tool: tool.to_string(),
        path: install.path.clone(),
        real_path: install.real.to_str()?.to_owned(),
        version: install.version.clone(),
        source: source.to_owned(),
        command,
    })
}

fn selected_tool_update_command(
    tool: &str,
    launcher: &Path,
    real: &Path,
    repair_codex: bool,
) -> Option<(&'static str, String)> {
    #[cfg(not(target_os = "windows"))]
    {
        if is_brew_managed_path(real.to_str()?) {
            let install = homebrew_installation(real)?;

            return Some((
                "homebrew",
                format!(
                    "{} upgrade --{}{} {}",
                    shell_single_quote(install.brew.to_str()?),
                    install.kind,
                    if tool == "agy" && install.kind == "cask" && install.token == "antigravity-cli"
                    {
                        " --greedy"
                    } else {
                        ""
                    },
                    shell_single_quote(&install.token),
                ),
            ));
        }
    }

    if tool == "omp" && is_omp_standalone_install(launcher, real) {
        return Some((
            "curl",
            omp_native_update_command(launcher.to_str()?, &crate::config::get_home_dir())?,
        ));
    }

    if tool == "pi" && is_pi_managed_install(launcher.to_str()?) {
        return Some((
            "curl",
            anchored_official_update_command(tool, launcher.to_str()?)?,
        ));
    }

    if tool == "codex" {
        if let Some(install) = codex_standalone_install(launcher.to_str()?, real.to_str()?) {
            return Some(("curl", codex_installer_update_command(&install)));
        }
    }

    #[cfg(not(target_os = "windows"))]
    if is_standard_curl_install(tool, real) {
        let args = match tool {
            "claude" => "update",
            "opencode" => "upgrade --method curl",
            _ => return None,
        };
        return Some((
            "curl",
            format!("{} {args}", shell_single_quote(launcher.to_str()?)),
        ));
    }

    let (prefix, manifest) = npm_global_package(tool, launcher, real, cfg!(target_os = "windows"))?;
    let name = manifest.get("name")?.as_str()?;
    let package = npm_package_candidates(tool).find(|candidate| *candidate == name)?;
    let npm = selected_npm_executable(launcher)?;
    let args = if package == OPENCODE_V2_NPM_PACKAGE {
        " --ignore-scripts=false --allow-scripts=@opencode/cli"
    } else {
        tool_spec(tool).npm_install_args
    };
    #[cfg(not(target_os = "windows"))]
    let command = {
        let prefix = shell_single_quote(prefix.to_str()?);
        let npm_prefix = format!(
            "PATH={}:\"$PATH\" {}",
            shell_single_quote(npm.parent()?.to_str()?),
            shell_single_quote(npm.to_str()?),
        );
        let install = format!(
            "{npm_prefix} install -g --prefix {prefix} --engine-strict --include=optional {}{args}",
            shell_single_quote(&format!("{package}@latest")),
        );
        if repair_codex {
            let target = shell_single_quote(&format!("{package}@latest"));
            let package = shell_single_quote(package);
            // Stage a runnable replacement and cache its packages before
            // touching the existing prefix. Reinstall from that cache offline.
            format!(
                r#"probe=$(mktemp -d {prefix}/lib/node_modules/@openai/.cc-switch-codex-repair.XXXXXXXX) || exit 1
trap 'rm -rf -- "$probe"' EXIT
mkdir -p -- "$probe/lib/node_modules" &&
{npm_prefix} install -g --prefix "$probe" --cache "$probe/cache" --engine-strict --include=optional {target}{args} &&
PATH={}:"$PATH" "$probe/bin/codex" --version &&
( {npm_prefix} uninstall -g --prefix {prefix} {package} || true; {install} --offline --cache "$probe/cache" )"#,
                shell_single_quote(npm.parent()?.to_str()?),
            )
        } else {
            install
        }
    };
    #[cfg(target_os = "windows")]
    let command = {
        if repair_codex {
            return None;
        }
        format!(
            "{} install -g --prefix {} --engine-strict --include=optional {package}@latest{args}",
            win_quote_path_for_batch(windows_shell_compatible_path(&npm).to_str()?),
            win_quote_path_for_batch(windows_shell_compatible_path(&prefix).to_str()?),
        )
    };
    Some(("npm", command))
}

/// Resolve the npm executable used by the update command for this installation.
pub(super) fn selected_npm_executable(launcher: &Path) -> Option<PathBuf> {
    let bin = launcher.parent()?;
    #[cfg(not(target_os = "windows"))]
    let npm = bin.join("npm");
    #[cfg(target_os = "windows")]
    let npm = bin.join("npm.cmd");
    // Use the Node toolchain alongside this installation where available.
    // For custom npm prefixes the explicit prefix still pins the destination.
    if npm.is_file() {
        Some(npm)
    } else {
        locate_default_tool(
            "npm",
            CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT)),
        )
        .ok()
    }
}

/// 静态命令（= 平台可安全静默执行的官方 CLI 自升级 || `npm i -g <pkg>@latest` /
/// 官方 installer）。锚定探不到默认安装时回退到它；npm fallback 仍等同于
/// "装到 PATH 第一个 npm"的旧行为。
fn static_fallback_command_for(tool: &str, action: ToolLifecycleAction) -> String {
    tool_action_shell_command(tool, action).unwrap_or_default()
}

pub(super) fn static_fallback_command(tool: &str) -> String {
    static_fallback_command_for(tool, ToolLifecycleAction::Update)
}

/// 升级命令的解析结果。plan（前端确认 / 展示）与 execute（真正执行）共用
/// `resolve_update_command`，保证两边判定一致。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum UpdateCommand {
    /// 锚定到命令行实际命中的那处。
    Anchored(String),
    /// 定位不到默认那处，或那处认不出渠道但可能归 npm 管（asdf/nodenv shim 等脚本）
    /// → 静态命令，保持旧行为。
    Static(String),
    /// 默认那处是原生可执行文件且不在 `node_modules` 里：不归 npm 管，也认不出是哪个
    /// 安装器装的（winget、Scoop、Nix、手动下载的二进制……）。退回裸 `npm i -g` 只会
    /// 另装一份 npm 版——要么被原生那份遮蔽、版本号不动，要么反过来顶替它（#7650），
    /// 所以不执行，由前端提示用户用原来的安装方式升级。
    Unmanaged,
}

pub(super) fn verified_wsl_opencode_npm_owner<'a>(
    entry: &'a str,
    real: &str,
) -> Option<(&'a str, &'static str)> {
    let prefix = entry
        .strip_suffix("/bin/opencode")
        .filter(|prefix| prefix.starts_with('/'))?;
    let package = ["opencode-ai", OPENCODE_V2_NPM_PACKAGE]
        .into_iter()
        .find(|package| real.starts_with(&format!("{prefix}/lib/node_modules/{package}/")))?;
    Some((prefix, package))
}

pub(super) fn opencode_update_is_manual(env: &str, installation: Option<(&Path, &Path)>) -> bool {
    if env == "wsl" {
        return installation.is_none_or(|(entry, real)| {
            verified_wsl_opencode_npm_owner(&entry.to_string_lossy(), &real.to_string_lossy())
                .is_none()
        });
    }
    env == "windows"
        || (env == "linux"
            && installation.is_some_and(|(entry, real)| {
                let is_system_path = |path: &Path| {
                    let path = path.to_string_lossy();
                    path.starts_with("/usr/bin/") || path.starts_with("/usr/lib/")
                };
                (is_system_path(entry) || is_system_path(real))
                    && !real.to_string_lossy().contains("/node_modules/")
            }))
}

fn volta_owned_update_command(tool: &str, manager: &Path, local: Option<&str>) -> Option<String> {
    let package = tool_version::volta_package_for_version(tool, local)?;
    let root = effective_volta_home()?;
    render_volta_update_command(package, manager, &root)
}

fn render_volta_update_command(package: &str, manager: &Path, root: &Path) -> Option<String> {
    // Reject the original OS strings before Windows normalization can replace
    // unpaired UTF-16 surrogates through a lossy extended-path conversion.
    manager.to_str()?;
    root.to_str()?;
    #[cfg(not(target_os = "windows"))]
    {
        Some(format!(
            "VOLTA_HOME={} {} install {package}@latest",
            shell_single_quote(root.to_str()?),
            shell_single_quote(manager.to_str()?)
        ))
    }
    #[cfg(target_os = "windows")]
    {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let encoded_path =
            STANDARD.encode(windows_shell_compatible_path(manager).to_str()?.as_bytes());
        let encoded_root =
            STANDARD.encode(windows_shell_compatible_path(root).to_str()?.as_bytes());
        let script = format!(
            "$ErrorActionPreference = 'Stop'; $env:VOLTA_HOME = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_root}')); $manager = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_path}')); & $manager install '{package}@latest'; exit $LASTEXITCODE"
        );
        Some(format!(
            "powershell -NoProfile -NonInteractive -EncodedCommand {}",
            powershell_encoded_command(&script)
        ))
    }
}

pub(super) fn resolve_update_command(tool: &str, installs: &[ToolInstallation]) -> UpdateCommand {
    // OpenCode Desktop 自己更新它带的命令行；npm 升级只会另装一份。
    if tool == "opencode" && default_install(installs).is_some_and(|inst| inst.source == "desktop")
    {
        return UpdateCommand::Unmanaged;
    }
    if default_install(installs).is_some_and(|install| {
        npm_prerelease_requires_manual_update(
            tool,
            Path::new(&install.path),
            &install.real,
            install.version.as_deref(),
        )
    }) {
        return UpdateCommand::Unmanaged;
    }
    if tool == "opencode"
        && default_install(installs).is_some_and(|install| {
            tool_version::npm_package_for_version(tool, install.version.as_deref())
                == Some(OPENCODE_V2_NPM_PACKAGE)
                && opencode_update_is_manual(
                    std::env::consts::OS,
                    Some((Path::new(&install.path), &install.real)),
                )
        })
    {
        return UpdateCommand::Unmanaged;
    }
    if default_install(installs)
        .is_some_and(|install| desktop_managed_codex_cli(tool, &install.real))
    {
        return UpdateCommand::Unmanaged;
    }
    #[cfg(target_os = "macos")]
    if tool == "codex"
        && default_install(installs).is_none()
        && crate::codex_config::codex_macos_desktop_installation().is_some()
    {
        return UpdateCommand::Unmanaged;
    }
    // Volta's shim may live in Homebrew's Cellar. Update the selected npm
    // package, never the formula that supplied the manager/template.
    if let Some(install) = default_install(installs) {
        if let Some(manager) = verified_volta_manager(Path::new(&install.path)) {
            return volta_owned_update_command(tool, &manager, install.version.as_deref())
                .map(UpdateCommand::Anchored)
                .unwrap_or(UpdateCommand::Unmanaged);
        }
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(install) = default_install(installs)
        .filter(|install| install.real.to_str().is_some_and(is_brew_managed_path))
    {
        return plan_selected_tool_update(tool, install)
            .map(|plan| UpdateCommand::Anchored(plan.command))
            .unwrap_or(UpdateCommand::Unmanaged);
    }
    // A native Copilot binary has no verified self-updater here. A sibling npm
    // must not make installing a second copy look like updating this one.
    if tool == "copilot" && default_install(installs).is_some_and(is_unmanaged_native_install) {
        return UpdateCommand::Unmanaged;
    }
    // These managers have independent global stores. Never let an app's generic
    // self-updater or an npm fallback guess which installation to replace.
    if let Some(install) = default_install(installs)
        .filter(|install| tool == "pi" || matches!(install.source.as_str(), "yarn" | "vite"))
    {
        return plan_selected_tool_update(tool, install)
            .map(|plan| UpdateCommand::Anchored(plan.command))
            .unwrap_or(UpdateCommand::Unmanaged);
    }
    if let Some(command) = installs_anchored_command(tool, installs) {
        return UpdateCommand::Anchored(command);
    }
    if installs.len() >= 2 || default_install(installs).is_some_and(is_unmanaged_native_install) {
        return UpdateCommand::Unmanaged;
    }
    UpdateCommand::Static(static_fallback_command(tool))
}

/// 只认「原生可执行文件 + 真身不在 `node_modules` 里」。npm 全局包的真身都在
/// `node_modules/<pkg>/` 下（多为 JS 入口，个别包把原生二进制放在包内）；asdf / nodenv
/// 的 shim、Windows npm 的 `.cmd` 是文本脚本，都不算——它们背后可能正是 npm 全局包，
/// 拒绝会误伤，保持旧的静态命令。
fn is_unmanaged_native_install(inst: &ToolInstallation) -> bool {
    let real = inst
        .real
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    !real.contains("/node_modules/") && is_native_executable(&inst.real)
}

/// 按文件头识别原生可执行文件：ELF、Mach-O（单架构 / universal）、Windows PE。
pub(super) fn is_native_executable(path: &Path) -> bool {
    use std::io::Read;

    let mut magic = [0u8; 4];
    let read = std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic));
    if read.is_err() {
        return false;
    }
    matches!(
        magic,
        [0x7f, b'E', b'L', b'F']
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xfe, 0xed, 0xfa, 0xce]
            | [0xca, 0xfe, 0xba, 0xbe]
    ) || magic.starts_with(b"MZ")
}

/// 执行路径的兜底报错。正常流程里前端已按 probe 的 `unmanaged` 跳过该工具并给出
/// 本地化提示；只有 probe 失败、前端直接执行时才会走到这里。
fn unmanaged_update_error(tool: &str, installs: &[ToolInstallation]) -> String {
    let default = default_install(installs);
    let path = default.map(|inst| inst.path.as_str()).unwrap_or_default();
    if tool == "opencode" && default.is_some_and(|inst| inst.source == "desktop") {
        return format!(
            "{} at {path} is bundled with OpenCode Desktop; update it with the Desktop app",
            tool_spec(tool).display_name
        );
    }
    format!(
        "{} at {path} was not installed by npm or a recognized installer; update it the way it was installed",
        tool_spec(tool).display_name
    )
}

/// 新装(install)的命令:对有官方 installer 的工具走「上游推荐 || npm 兜底」短路链,
/// 其余工具透传到 install 静态命令。update fallback 会在平台可安全静默执行时
/// 优先跑官方 CLI 自升级,但 install 端不能先跑 `tool update`,
/// 否则“未安装时安装”的路径会多一次无效失败。
///
/// 设计理由:
/// - install 没有锚点可言(从无到有),但**有"上游推荐方式"这一事实** ——
///   Anthropic、xAI 和 SST(OpenCode)都已将自家 native installer 列为首推、把 npm 列为替代方式。
///   把这层认知补进来,让 install 表与 update 端的锚定决策树共用同一份"上游事实"。
/// - Hermes 使用官方 installer,避免用系统 Python/pip 安装时踩 Python >=3.11 与 pyenv
///   `python` shim 问题;更新路径若能锚定已安装 CLI,则走 `<hermes> update`。
///   **Hermes 没有 npm 包,install 端不享受 `||` 降级**——上游 installer 不可达就只能等。
/// - 对**有 npm 包**的工具(claude/grok/opencode/mcode),短路链(POSIX `||`)保证官方脚本不可达/
///   防火墙拦截时仍能装上,降级到裸 `npm i -g`。官方脚本本身不用 pipe,
///   所以这条路径在 WSL 的 `sh -c` 子 shell 中也不依赖外层 `pipefail`。
/// - Windows 上 Claude/OpenCode 原生不启用（对应 installer 都是 bash 脚本）；Grok
///   与 MiniMax Code 使用官方 PowerShell installer，并同样保留 npm fallback。WSL 作为 Linux 环境
///   复用这套 POSIX 安装优先级。
fn installer_with_npm_fallback(installer: &str, tool: &str) -> String {
    match tool_spec(tool).npm_install_command() {
        Some(npm) => {
            chain_update_commands(installer.to_string(), npm, LifecycleCommandShell::Posix)
        }
        None => installer.to_string(),
    }
}

fn posix_install_command_for(tool: &str) -> String {
    match tool {
        "claude" => installer_with_npm_fallback(CLAUDE_INSTALL_UNIX, tool),
        // Grok 的 npm fallback **会切换用户的分发模式**（该包 postinstall 把
        // `~/.grok/config.toml` 的 `[cli] installer` 写成 `npm`，此后 `grok update` 一律走
        // npm、隐式依赖 node）。仍然保留它：官方 installer 不可达（防火墙 / x.ai 被拦）时
        // 这是唯一退路，而副作用可自愈——`grok_native_update_command` 的 `||` 官方 installer
        // 会在 npm 路径出问题时把 `installer` 覆写回 `internal`（见该函数 doc 的实测记录）。
        "grok" => installer_with_npm_fallback(GROK_INSTALL_UNIX, tool),
        "opencode" => installer_with_npm_fallback(OPENCODE_INSTALL_UNIX, tool),
        "mcode" => installer_with_npm_fallback(MCODE_INSTALL_UNIX, tool),
        "hermes" => HERMES_INSTALL_UNIX.to_string(),
        "kimi" => KIMI_INSTALL_UNIX.to_string(),
        "omp" => OMP_INSTALL_UNIX.to_string(),
        "agy" => AGY_INSTALL_UNIX.to_string(),
        _ => static_fallback_command_for(tool, ToolLifecycleAction::Install),
    }
}

#[cfg(not(target_os = "windows"))]
fn install_command_for(tool: &str) -> String {
    posix_install_command_for(tool)
}

/// 计算某工具的升级命令与"是否需确认"。全平台共用一份:
/// - **Windows + WSL 工具**(override 是 `\\wsl$\<distro>\...` UNC 路径)的升级规划
///   始终走 POSIX 静态命令、不锚定:锚定命令是 Windows 主机绝对路径,跨 `wsl.exe`
///   边界进入 distro 文件系统后完全无效;且 `enumerate_tool_installations` 不参与
///   WSL 文件系统、锚定无锚点。这一类显式短路到 `(unix_static, false, false)`,
///   前端不会弹确认。
///   **必须用 `wsl_tool_action_shell_command`(unix 版)而非 `static_fallback_command`**
///   ——后者读 `tool_action_shell_command`,Windows target 给 hermes 返回 PowerShell
///   installer,跨 wsl.exe 后不适用;`build_tool_action_line` 的 WSL 分支也用同一 wrapper,
///   保证 plan 展示给前端的命令与实际执行落 .bat 的命令一致。
/// - 其他平台与 Windows 原生工具走 `resolve_update_command`:命中 → 锚定;
///   None(无默认 / sibling 不存在等)→ 静态兜底、`anchored=false`,
///   前端据此给"默认入口无法确定"诚实文案;默认那处是认不出渠道的原生可执行文件
///   → `Unmanaged`,前端跳过并提示用原安装方式升级。
pub(super) fn plan_command_for(tool: &str, installs: &[ToolInstallation]) -> (UpdateCommand, bool) {
    #[cfg(target_os = "windows")]
    {
        if wsl_distro_for_tool(tool).is_some() {
            let cmd = wsl_tool_action_shell_command(tool, ToolLifecycleAction::Update)
                .unwrap_or_default();
            return (UpdateCommand::Static(cmd), false);
        }
    }
    (resolve_update_command(tool, installs), installs.len() >= 2)
}

/// 多处安装是否构成"真冲突"：≥2 处，且(版本分歧 或 有的能跑有的跑不起来)。
/// 同版本装两份且都能跑不算冲突（不打扰用户）。诊断展示据此判定。
pub(super) fn is_conflicting(installs: &[ToolInstallation]) -> bool {
    if installs.len() < 2 {
        return false;
    }
    let distinct_versions: std::collections::HashSet<&Option<String>> =
        installs.iter().map(|i| &i.version).collect();
    let runnable_mixed =
        installs.iter().any(|i| i.runnable) && installs.iter().any(|i| !i.runnable);
    distinct_versions.len() > 1 || runnable_mixed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn volta_extended_paths_reject_unpaired_surrogates_before_normalization() {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = r"\\?\C:\Volta\".encode_utf16().collect();
        units.push(0xd800);
        let invalid = PathBuf::from(std::ffi::OsString::from_wide(&units));
        let valid = Path::new(r"C:\Volta\bin\volta.exe");
        assert!(render_volta_update_command("@github/copilot", valid, &invalid).is_none());
        assert!(
            render_volta_update_command("@github/copilot", &invalid, Path::new(r"C:\Volta"))
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn volta_non_unicode_paths_decline_automatic_updates() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"/tmp/volta-\xff".to_vec()));
        let valid = Path::new("/tmp/volta/bin/volta");
        assert!(render_volta_update_command("@github/copilot", valid, &invalid).is_none());
        assert!(
            render_volta_update_command("@github/copilot", &invalid, Path::new("/tmp/volta"))
                .is_none()
        );
        assert!(
            render_volta_update_command("@github/copilot", valid, Path::new("/tmp/volta"))
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn pi_shell_release_receipt_requires_matching_package_and_target() {
        let temp = tempfile::tempdir().unwrap();
        let agent = temp.path().join("custom-agent");
        let bin = agent.join("bin");
        let install = agent.join("install");
        let modules = install.join("releases/1.6.1/node_modules");
        let package = modules.join("@earendil-works/pi-coding-agent");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(package.join("dist")).unwrap();
        std::fs::create_dir_all(modules.join(".bin")).unwrap();
        let launcher = bin.join("pi");
        std::fs::write(&launcher, PI_RELEASE_SHELL_LAUNCHER).unwrap();
        std::fs::write(
            install.join("managed-install.json"),
            r#"{"kind":"pi-managed-install","schemaVersion":1,"layout":"releases-v1"}"#,
        )
        .unwrap();
        std::fs::write(install.join("current-version"), "1.6.1\n").unwrap();
        let manifest = package.join("package.json");
        std::fs::write(
            &manifest,
            r#"{"name":"@earendil-works/pi-coding-agent","version":"1.6.1"}"#,
        )
        .unwrap();
        let cli = package.join("dist/cli.js");
        std::fs::write(&cli, "fixture payload").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = modules.join(".bin/pi");
        std::os::unix::fs::symlink(&cli, &link).unwrap();
        assert!(is_pi_managed_install(launcher.to_str().unwrap()));
        let modified = PI_RELEASE_SHELL_LAUNCHER.replace(
            "pi_release_bin=$pi_release_dir/node_modules/.bin/pi",
            "pi_release_bin=/outside/pi",
        );
        std::fs::write(&launcher, modified).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(
            &launcher,
            PI_RELEASE_SHELL_LAUNCHER.replace(
                "# Node.js installed",
                "# Updated comment: Node.js installed",
            ),
        )
        .unwrap();
        assert!(is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(
            &launcher,
            PI_RELEASE_SHELL_LAUNCHER.replace(
                "pi_agent_dir=${pi_bin_dir%/*}",
                "\u{a0}pi_agent_dir=${pi_bin_dir%/*}",
            ),
        )
        .unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(&launcher, PI_RELEASE_SHELL_LAUNCHER).unwrap();
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&launcher, &alias).unwrap();
        assert!(is_pi_managed_install(alias.to_str().unwrap()));
        let same_agent_alias = agent.join("cli");
        std::os::unix::fs::symlink(&bin, &same_agent_alias).unwrap();
        assert!(is_pi_managed_install(
            same_agent_alias.join("pi").to_str().unwrap()
        ));
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        let alias_root = temp.path().join("alias-root");
        std::fs::create_dir_all(&alias_root).unwrap();
        std::os::unix::fs::symlink(&bin, alias_root.join("bin")).unwrap();
        let directory_alias = alias_root.join("bin/pi");
        assert!(!is_pi_managed_install(directory_alias.to_str().unwrap()));
        std::fs::write(bin.join("pi-launcher.js"), "// leftover from old installer").unwrap();
        assert!(!is_pi_managed_install(directory_alias.to_str().unwrap()));
        assert!(is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::remove_file(bin.join("pi-launcher.js")).unwrap();
        assert!(selected_tool_update_command("pi", &directory_alias, &launcher, false).is_none());
        assert!(!tool_discovery::additional_script_install(
            "pi",
            &directory_alias,
            &launcher
        ));
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        let unmanaged = temp.path().join("unmanaged/bin/pi");
        std::fs::create_dir_all(unmanaged.parent().unwrap()).unwrap();
        std::fs::write(&unmanaged, PI_RELEASE_SHELL_LAUNCHER).unwrap();
        std::fs::remove_file(&launcher).unwrap();
        std::os::unix::fs::symlink(&unmanaged, &launcher).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::remove_file(&launcher).unwrap();
        std::fs::write(&launcher, PI_RELEASE_SHELL_LAUNCHER).unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        let external_install = temp.path().join("external-install");
        std::fs::rename(&install, &external_install).unwrap();
        std::os::unix::fs::symlink(&external_install, &install).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::remove_file(&install).unwrap();
        std::fs::rename(external_install, &install).unwrap();
        let borrowed = temp.path().join("borrowed-package.json");
        std::fs::copy(&manifest, &borrowed).unwrap();
        std::fs::remove_file(&manifest).unwrap();
        std::os::unix::fs::symlink(&borrowed, &manifest).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::remove_file(&manifest).unwrap();
        std::fs::copy(borrowed, &manifest).unwrap();
        let inst = ToolInstallation {
            path: launcher.to_string_lossy().into_owned(),
            real: launcher.clone(),
            source: "curl".into(),
            version: Some("1.6.1".into()),
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        let plan = plan_selected_tool_update("pi", &inst).unwrap();
        assert_eq!(plan.source, "curl");
        assert!(plan.command.ends_with(" update") && !plan.command.contains("npm"));
        std::fs::write(install.join("current-version"), "../outside").unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(install.join("current-version"), "1.6.1").unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(install.join("current-version"), " 1.6.1\n").unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(
            install.join("current-version"),
            format!("1.6.1\n{}", "ignored trailing data".repeat(40)),
        )
        .unwrap();
        assert!(is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(install.join("current-version"), "1.6.1\n").unwrap();
        std::fs::write(&manifest, r#"{"name":"unrelated","version":"1.6.1"}"#).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::write(
            &manifest,
            r#"{"name":"@earendil-works/pi-coding-agent","version":"1.6.1"}"#,
        )
        .unwrap();
        let external = temp.path().join("external-package");
        std::fs::create_dir_all(external.join("dist")).unwrap();
        std::fs::copy(&manifest, external.join("package.json")).unwrap();
        std::fs::write(external.join("dist/cli.js"), "external payload").unwrap();
        std::fs::remove_dir_all(&package).unwrap();
        std::os::unix::fs::symlink(&external, &package).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
        std::fs::remove_file(&package).unwrap();
        std::fs::create_dir_all(package.join("dist")).unwrap();
        std::fs::copy(external.join("package.json"), &manifest).unwrap();
        std::fs::write(&cli, "fixture payload").unwrap();
        std::fs::remove_file(&link).unwrap();
        let outside = temp.path().join("unrelated");
        std::fs::write(&outside, "unrelated payload").unwrap();
        std::os::unix::fs::symlink(outside, link).unwrap();
        assert!(!is_pi_managed_install(launcher.to_str().unwrap()));
    }

    #[test]
    fn volta_update_package_respects_runtime_and_known_release_line() {
        assert!(tool_version::volta_package_for_version("omp", Some("19.2.1")).is_none());
        assert!(tool_version::volta_package_for_version("opencode", None).is_none());
        assert!(tool_version::volta_package_for_version("opencode", Some("broken")).is_none());
        assert_eq!(
            tool_version::volta_package_for_version("opencode", Some("2.4.2")),
            Some(OPENCODE_V2_NPM_PACKAGE)
        );
        assert_eq!(
            tool_version::volta_package_for_version("opencode", Some("1.19.6")),
            Some("opencode-ai")
        );
    }

    #[cfg(unix)]
    #[test]
    fn volta_shim_symlink_uses_its_template_manager() {
        let temp = tempfile::tempdir().unwrap();
        let template = temp.path().join("Cellar/volta/2.0.2/bin");
        let bin = temp.path().join("custom-data/bin");
        std::fs::create_dir_all(&template).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(template.join("volta-shim"), b"MZshim").unwrap();
        std::fs::write(template.join("volta"), "fixture manager").unwrap();
        let launcher = bin.join("copilot");
        std::os::unix::fs::symlink(template.join("volta-shim"), &launcher).unwrap();
        assert_eq!(
            verified_volta_manager(&launcher),
            Some(std::fs::canonicalize(template.join("volta")).unwrap())
        );
        let inst = ToolInstallation {
            path: launcher.to_string_lossy().into_owned(),
            real: template.join("volta-shim"),
            source: "volta".into(),
            version: Some("0.0.400".into()),
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        let plan = plan_selected_tool_update("copilot", &inst).unwrap();
        assert_eq!(plan.source, "volta");
        let UpdateCommand::Anchored(command) = resolve_update_command("copilot", &[inst]) else {
            panic!("Missing template manager route");
        };
        assert!(!command.contains(&format!(
            "VOLTA_HOME={}",
            shell_single_quote(bin.parent().unwrap().to_str().unwrap())
        )));
        assert!(command.contains(&shell_single_quote(
            std::fs::canonicalize(template.join("volta"))
                .unwrap()
                .to_str()
                .unwrap()
        )));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn volta_windows_launch_failure_is_not_success() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("custom/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["copilot.exe", "volta-shim.exe", "volta.exe"] {
            std::fs::write(bin.join(name), b"MZnot an executable").unwrap();
        }
        let launcher = bin.join("copilot.exe");
        let inst = ToolInstallation {
            path: launcher.to_string_lossy().into_owned(),
            real: launcher,
            source: "volta".into(),
            version: None,
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        let UpdateCommand::Anchored(command) = resolve_update_command("copilot", &[inst]) else {
            panic!("No fixture route");
        };
        let encoded = command.split_whitespace().last().unwrap();
        let output = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "failed manager launch was reported as success: {output:?}"
        );
    }

    #[test]
    fn volta_native_copilot_keeps_its_verified_manager_route() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("tools(v2)’/custom-toolchain/bin");
        std::fs::create_dir_all(&bin).unwrap();
        let native = bin.join(if cfg!(windows) {
            "copilot.exe"
        } else {
            "copilot"
        });
        let volta = bin.join(if cfg!(windows) { "volta.exe" } else { "volta" });
        std::fs::write(&native, b"MZ\0\0").unwrap();
        let shim = bin.join(if cfg!(windows) {
            "volta-shim.exe"
        } else {
            "volta-shim"
        });
        std::fs::write(&shim, b"MZ\0\0").unwrap();
        std::fs::write(&volta, "fixture manager").unwrap();
        std::fs::write(
            bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }),
            "wrong fallback",
        )
        .unwrap();
        let inst = ToolInstallation {
            path: native.to_string_lossy().into_owned(),
            real: native,
            source: "volta".into(),
            version: Some("0.0.400".into()),
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        let UpdateCommand::Anchored(command) =
            resolve_update_command("copilot", std::slice::from_ref(&inst))
        else {
            panic!("Volta route was lost")
        };
        #[cfg(target_os = "windows")]
        let command = {
            use base64::{engine::general_purpose::STANDARD, Engine as _};
            let bytes = STANDARD
                .decode(command.split_whitespace().last().unwrap())
                .unwrap();
            String::from_utf16(
                &bytes
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        assert!(command.contains("install"));
        assert!(command.contains("@github/copilot") && !command.contains("npm"));
        assert!(!command.contains(&format!(
            "VOLTA_HOME={}",
            shell_single_quote(bin.parent().unwrap().to_str().unwrap())
        )));
        #[cfg(target_os = "windows")]
        assert!(command.contains("$ErrorActionPreference = 'Stop'"));
        #[cfg(not(target_os = "windows"))]
        assert!(command.contains(&shell_single_quote(volta.to_str().unwrap())));
        assert!(verified_volta_manager(Path::new(&inst.path)).is_some());
        // A standalone binary beside a genuine manager is not a Volta shim.
        std::fs::write(&inst.real, b"MZstandalone vendor binary").unwrap();
        assert!(verified_volta_manager(Path::new(&inst.path)).is_none());
        assert!(matches!(
            resolve_update_command("copilot", std::slice::from_ref(&inst)),
            UpdateCommand::Unmanaged
        ));
        std::fs::write(&inst.real, b"MZ\0\0").unwrap();
        let unrelated = temp.path().join("volta/.local/bin/copilot");
        std::fs::create_dir_all(unrelated.parent().unwrap()).unwrap();
        std::fs::write(
            unrelated
                .parent()
                .unwrap()
                .join(if cfg!(windows) { "volta.exe" } else { "volta" }),
            "unrelated manager",
        )
        .unwrap();
        assert!(verified_volta_manager(&unrelated).is_none());
        std::fs::remove_file(volta).unwrap();
        assert!(matches!(
            resolve_update_command("copilot", &[inst]),
            UpdateCommand::Unmanaged
        ));
    }

    #[test]
    fn native_copilot_does_not_fall_back_to_a_sibling_npm() {
        let temp = tempfile::tempdir().unwrap();
        let native = temp.path().join(if cfg!(windows) {
            "copilot.exe"
        } else {
            "copilot"
        });
        std::fs::write(&native, b"MZ\0\0").unwrap();
        std::fs::write(
            temp.path()
                .join(if cfg!(windows) { "npm.cmd" } else { "npm" }),
            "fixture npm",
        )
        .unwrap();
        let inst = ToolInstallation {
            path: native.to_string_lossy().into_owned(),
            real: native,
            source: "system".into(),
            version: Some("0.0.400".into()),
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        assert!(matches!(
            resolve_update_command("copilot", &[inst]),
            UpdateCommand::Unmanaged
        ));
    }

    #[cfg(unix)]
    #[test]
    fn pi_update_keeps_metacharacters_in_the_selected_launcher_literal() {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("pi$HOME;not-a-command");
        std::fs::write(&bin, "#!/bin/sh\nprintf '%s' \"$1\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let command = anchored_official_update_command("pi", bin.to_str().unwrap()).unwrap();
        let output = std::process::Command::new("/bin/bash")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"update");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn confirmed_upgrade_rejects_a_changed_default_installation() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let installation = |prefix: &Path, is_path_default| ToolInstallation {
            path: prefix.join("bin/gemini").to_str().unwrap().into(),
            real: prefix.join("Cellar/gemini-cli/1.0.0/bin/gemini"),
            version: Some("1.0.0".into()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: "homebrew".into(),
            is_path_default,
        };
        let mut installs = [installation(&first, true), installation(&second, false)];
        assert_eq!(
            resolve_update_command("gemini", &installs),
            UpdateCommand::Unmanaged
        );
        for prefix in [&first, &second] {
            std::fs::create_dir_all(prefix.join("bin")).unwrap();
            std::fs::write(prefix.join("bin/brew"), "#!/bin/sh\n").unwrap();
        }
        let command = match resolve_update_command("gemini", &installs) {
            UpdateCommand::Anchored(command) => command,
            other => panic!("Expected an anchored command, got {other:?}"),
        };
        let confirmed = update_confirmation_token("gemini", &command, &installs);
        assert_eq!(
            checked_update_command("gemini", &installs, Some(&confirmed)),
            Ok(command)
        );
        assert!(checked_update_command("gemini", &installs, None).is_err());
        installs[0].is_path_default = false;
        installs[1].is_path_default = true;
        assert!(checked_update_command("gemini", &installs, Some(&confirmed)).is_err());
        assert!(checked_update_command("gemini", &installs[1..], Some(&confirmed)).is_err());
    }

    #[test]
    fn wsl_opencode_requires_a_verified_npm_owner() {
        let entry = Path::new("/home/test/.local/bin/opencode");
        assert!(opencode_update_is_manual(
            "wsl",
            Some((entry, Path::new("/opt/distro/opencode")))
        ));
        assert!(!opencode_update_is_manual(
            "wsl",
            Some((
                entry,
                Path::new("/home/test/.local/lib/node_modules/@opencode/cli/bin/opencode")
            ))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn antigravity_cask_uses_homebrew_including_auto_updating_casks() {
        let root = tempfile::tempdir().unwrap();
        let brew = root.path().join("bin/brew");
        std::fs::create_dir_all(brew.parent().unwrap()).unwrap();
        std::fs::write(&brew, "").unwrap();
        let real = root.path().join("Caskroom/antigravity-cli/1.0/bin/agy");
        let launcher = root.path().join("bin/agy");
        let (source, command) =
            selected_tool_update_command("agy", &launcher, &real, false).unwrap();
        assert_eq!(source, "homebrew");
        assert!(command.contains(" upgrade --cask --greedy 'antigravity-cli'"));
        assert!(command.starts_with(&shell_single_quote(brew.to_str().unwrap())));
        assert!(!command.contains("agy update"));
    }

    #[cfg(unix)]
    #[test]
    fn omp_updater_resolves_the_selected_copy_before_the_path_default() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let chosen = root.path().join("chosen 'quoted/omp");
        let default = root.path().join("default/omp");
        for entry in [&chosen, &default] {
            std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
            std::fs::write(entry, "#!/bin/sh\ncommand -v omp\n").unwrap();
            std::fs::set_permissions(entry, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let command = omp_native_update_command(chosen.to_str().unwrap(), root.path()).unwrap();
        let path = format!(
            "{}:{}",
            default.parent().unwrap().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let output = std::process::Command::new("sh")
            .args(["-c", &command])
            .env("PATH", path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            chosen.to_str().unwrap()
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_omp_legacy_preserves_bun_but_never_executes_a_standalone_preview() {
        let root = tempfile::tempdir().unwrap();
        let launcher = root.path().join(".bun/bin/omp.exe");
        let package = root
            .path()
            .join(".bun/install/global/node_modules/@oh-my-pi/pi-coding-agent");
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        let real = package.join("cli.js");
        std::fs::write(&real, "// Bun CLI").unwrap();
        let install = ToolInstallation {
            path: launcher.to_string_lossy().into_owned(),
            real,
            version: Some("1.0.0".into()),
            source: "bun".into(),
            runnable: true,
            error: None,
            exited_with_error: false,
            is_path_default: true,
        };
        assert!(plan_selected_tool_update("omp", &install).is_none());
        for present in [false, true] {
            if present {
                std::fs::write(&launcher, b"MZfixture").unwrap();
            }
            assert!(
                checked_update_command("omp", std::slice::from_ref(&install), None)
                    .unwrap()
                    .contains("update")
            );
        }
        let mut native = install;
        native.path = root
            .path()
            .join("native/omp.exe")
            .to_string_lossy()
            .into_owned();
        native.source = "system".into();
        native.real = PathBuf::from(&native.path);
        for present in [false, true] {
            if present {
                std::fs::create_dir_all(native.real.parent().unwrap()).unwrap();
                std::fs::write(&native.real, b"MZfixture").unwrap();
            }
            assert!(checked_update_command("omp", std::slice::from_ref(&native), None).is_err());
        }
    }

    #[test]
    fn native_omp_process_has_literal_argv_and_no_shell_length_limit() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home ^%PATH% '‘’“”");
        let launcher = home.join(if cfg!(windows) { "omp.exe" } else { "omp" });
        let inherited = std::ffi::OsString::from("x".repeat(9000));
        let command = omp_native_update_process(&launcher, &home, &inherited).unwrap();
        assert_eq!(command.get_program(), launcher.as_os_str());
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("update")]
        );
        assert_eq!(command.get_current_dir(), Some(home.as_path()));
        let path = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .unwrap()
            .1
            .unwrap();
        assert!(path.to_string_lossy().ends_with(&"x".repeat(9000)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_omp_update_preserves_literal_percent_and_selected_path() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home ^%PATH% '‘’“”");
        let chosen = home.join("chosen%PATH%").join("omp.exe");
        let default = root.path().join("Tools & Utilities");
        std::fs::create_dir_all(chosen.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&default).unwrap();
        // The WSL2 nightly deliberately puts TEMP/TMP on a UNC share. MSVC's
        // manifest linker needs native storage; execution below still uses UNC.
        let native_build = [
            std::env::var_os("RUNNER_TEMP").map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Temp")),
            std::env::var_os("SystemRoot").map(|p| PathBuf::from(p).join("Temp")),
        ]
        .into_iter()
        .flatten()
        .map(|path| windows_shell_compatible_path(&path))
        .filter(|path| path.is_absolute() && !path.to_string_lossy().starts_with("\\\\"))
        .find_map(|path| {
            tempfile::Builder::new()
                .prefix("ccs_omp_native_")
                .tempdir_in(path)
                .ok()
        })
        .expect(
            "a writable native Windows temporary directory is required for the fixture compiler",
        );
        let source = native_build.path().join("fixture.rs");
        let executable = native_build.path().join("omp.exe");
        std::fs::write(
            &source,
            r#"fn main() {
            println!("{}", std::env::current_exe().unwrap().display());
            println!("{}", std::env::current_dir().unwrap().display());
            println!("{}", std::env::var("PATH").unwrap());
            std::process::exit(7);
        }"#,
        )
        .unwrap();
        let compiler =
            std::process::Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
                .arg(&source)
                .arg("-o")
                .arg(&executable)
                .current_dir(native_build.path())
                .env("TEMP", native_build.path())
                .env("TMP", native_build.path())
                .output()
                .unwrap();
        assert!(compiler.status.success(), "{compiler:?}");
        std::fs::copy(&executable, &chosen).unwrap();
        let quoted_default = format!("\"{}\"", default.display());
        let path = format!(
            "{};{}",
            quoted_default,
            std::env::var("PATH").unwrap_or_default()
        );
        let output = omp_native_update_process(&chosen, &home, std::ffi::OsStr::new(&path))
            .unwrap()
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<_> = stdout.lines().collect();
        let normalized = |path: &Path| {
            windows_shell_compatible_path(path)
                .to_string_lossy()
                .to_ascii_lowercase()
        };
        assert_eq!(normalized(Path::new(lines[0])), normalized(&chosen));
        assert_eq!(normalized(Path::new(lines[1])), normalized(&home));
        assert!(lines[2].starts_with(&format!("{};", chosen.parent().unwrap().display())));
        assert!(lines[2].contains(&quoted_default));
    }

    #[cfg(unix)]
    #[test]
    fn linked_npm_native_omp_keeps_its_verified_package_owner() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let package = root.path().join("linked-package");
        let prefix = root.path().join("prefix");
        let launcher = prefix.join("bin/omp");
        let real = package.join("omp");
        let owner = prefix.join("lib/node_modules/@oh-my-pi/pi-coding-agent");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        std::fs::create_dir_all(owner.parent().unwrap()).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@oh-my-pi/pi-coding-agent","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(&real, b"\x7fELFfixture").unwrap();
        symlink(&package, &owner).unwrap();
        symlink(&real, &launcher).unwrap();
        let npm = prefix.join("bin/npm");
        std::fs::write(&npm, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&npm, std::fs::Permissions::from_mode(0o755)).unwrap();
        let real = real.canonicalize().unwrap();
        assert!(npm_global_package("omp", &launcher, &real, false).is_some());
        assert!(!is_omp_standalone_install(&launcher, &real));
        let (source, command) =
            selected_tool_update_command("omp", &launcher, &real, false).unwrap();
        assert_eq!(source, "npm");
        assert!(command.contains("@oh-my-pi/pi-coding-agent@latest"));
        let project_link = root.path().join("project/node_modules/.bin/omp");
        std::fs::create_dir_all(project_link.parent().unwrap()).unwrap();
        symlink(&real, &project_link).unwrap();
        assert!(npm_global_package("omp", &project_link, &real, false).is_none());
        assert!(!is_omp_standalone_install(&project_link, &real));
        assert!(selected_tool_update_command("omp", &project_link, &real, false).is_none());
        let proxy = root.path().join("proxy/bin/omp");
        std::fs::create_dir_all(proxy.parent().unwrap()).unwrap();
        symlink(&project_link, &proxy).unwrap();
        assert!(!is_omp_standalone_install(&proxy, &real));
        std::fs::remove_file(&proxy).unwrap();
        symlink(&launcher, &proxy).unwrap();
        assert!(!is_omp_standalone_install(&proxy, &real));
        std::fs::remove_file(&proxy).unwrap();
        symlink("../../prefix/bin/omp", &proxy).unwrap();
        assert!(!is_omp_standalone_install(&proxy, &real));
        let directory_proxy = root.path().join("directory-proxy");
        symlink(project_link.parent().unwrap(), &directory_proxy).unwrap();
        assert!(!is_omp_standalone_install(
            &directory_proxy.join("omp"),
            &real
        ));
    }

    #[test]
    fn distro_omp_binaries_do_not_use_standalone_release_metadata() {
        for path in [
            "/usr/bin/omp",
            "/usr/lib/omp/omp",
            "/bin/omp",
            "/usr/share/omp/omp",
        ] {
            assert!(omp_distro_owned_path(Path::new(path)));
        }
        for path in ["/usr/local/bin/omp", "/home/me/custom/bin/omp"] {
            assert!(!omp_distro_owned_path(Path::new(path)));
        }
    }

    #[test]
    fn omp_binary_release_detection_supports_custom_installer_directories() {
        let root = tempfile::tempdir().unwrap();
        for (directory, expected) in [
            (".local/bin", true),
            ("custom/bin", true),
            ("Cellar/omp/1/bin", false),
            ("lib/node_modules/omp", false),
            ("AppData/Local/mise/installs/omp/bin", false),
            ("nix/store/omp/bin", false),
            ("scoop/shims", false),
            ("scoop/apps/oh-my-pi/current", false),
        ] {
            let entry =
                root.path()
                    .join(directory)
                    .join(if cfg!(windows) { "omp.exe" } else { "omp" });
            std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
            std::fs::write(&entry, b"\x7fELFfixture").unwrap();
            let real = entry.canonicalize().unwrap();
            assert_eq!(
                is_omp_standalone_install(&entry, &real),
                expected,
                "{directory}"
            );
            if expected {
                for extension in ["bunx", "shim"] {
                    std::fs::write(entry.with_extension(extension), b"manager shim metadata")
                        .unwrap();
                    assert!(!is_omp_standalone_install(&entry, &real));
                    std::fs::remove_file(entry.with_extension(extension)).unwrap();
                }
            }
            if expected {
                let install = ToolInstallation {
                    path: entry.to_string_lossy().into_owned(),
                    real: real.clone(),
                    version: Some("1.0.0".into()),
                    source: "system".into(),
                    runnable: true,
                    error: None,
                    exited_with_error: false,
                    is_path_default: false,
                };
                let plan = plan_selected_tool_update("omp", &install)
                    .expect("secondary native copy must have a reviewed update plan");
                assert_eq!(plan.path, install.path);
                assert_eq!(plan.real_path, real.to_string_lossy());
                assert_eq!(plan.source, "curl");
                assert!(plan.command.contains(entry.to_str().unwrap()));
                assert!(!plan.command.contains("--stable") && !plan.command.contains("--canary"));
            }
            std::fs::write(&entry, b"#!/bin/sh\n").unwrap();
            assert!(!is_omp_standalone_install(&entry, &real));
        }
    }

    #[cfg(unix)]
    #[test]
    fn selected_npm_updates_match_the_reviewed_package_and_repair_mode() {
        let fixture = tempfile::tempdir().unwrap();
        let bin = fixture.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("npm"), "").unwrap();

        for (tool, package, repair) in [
            ("codex", "@openai/codex", true),
            ("opencode", OPENCODE_V2_NPM_PACKAGE, false),
        ] {
            let package_dir = fixture.path().join("lib/node_modules").join(package);
            std::fs::create_dir_all(&package_dir).unwrap();
            std::fs::write(
                package_dir.join("package.json"),
                serde_json::json!({"name": package, "version": "2.0.0"}).to_string(),
            )
            .unwrap();
            let script = package_dir.join("cli.js");
            std::fs::write(&script, "").unwrap();
            let launcher = bin.join(tool);
            std::os::unix::fs::symlink(&script, &launcher).unwrap();
            let real = script.canonicalize().unwrap();
            let (_, command) =
                selected_tool_update_command(tool, &launcher, &real, repair).unwrap();
            assert!(command.contains(&format!("{package}@latest")));
            if repair {
                assert!(
                    command.contains("/lib/node_modules/@openai/.cc-switch-codex-repair.XXXXXXXX")
                );
                let probe = command.find(r#""$probe/bin/codex" --version"#).unwrap();
                let uninstall = command.find("uninstall -g --prefix").unwrap();
                assert!(probe < uninstall);
                assert!(command[probe..uninstall].contains("&&"));
                assert_eq!(command.matches("--prefix").count(), 3);
                assert!(command[uninstall..].contains(r#"--offline --cache "$probe/cache""#));
            } else {
                assert!(command.contains("--allow-scripts=@opencode/cli"));
                assert!(!command.contains("--allow-scripts=opencode-ai"));
            }
        }
    }

    #[test]
    fn pi_managed_install_anchors_to_self_update() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join(".pi/agent/bin");
        let install_dir = dir.path().join(".pi/agent/install");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&install_dir).unwrap();

        let entry_name = if cfg!(target_os = "windows") {
            "pi.cmd"
        } else {
            "pi"
        };
        let entry = bin_dir.join(entry_name);
        std::fs::write(&entry, "#!/bin/sh\n").unwrap();
        std::fs::write(bin_dir.join("pi-launcher.js"), "// launcher\n").unwrap();
        std::fs::write(
            install_dir.join("managed-install.json"),
            "\u{feff}{\"kind\":\"pi-managed-install\",\"schemaVersion\":1,\"layout\":\"releases-v1\"}",
        )
        .unwrap();

        let entry_str = entry.to_string_lossy().to_string();
        assert!(is_pi_managed_install(&entry_str));

        let inst = ToolInstallation {
            path: entry_str.clone(),
            version: Some("1.5.2".to_string()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: infer_install_source(&entry).to_string(),
            is_path_default: true,
            real: entry.clone(),
        };
        let expected = anchored_official_update_command("pi", &entry_str).unwrap();
        assert_eq!(
            resolve_update_command("pi", &[inst]),
            UpdateCommand::Anchored(expected.clone())
        );
        assert!(expected.ends_with(" update") && !expected.contains("npm"));

        #[cfg(unix)]
        {
            let alias_root = dir.path().join("legacy-alias");
            std::fs::create_dir_all(&alias_root).unwrap();
            std::os::unix::fs::symlink(&bin_dir, alias_root.join("bin")).unwrap();
            let alias = alias_root.join("bin/pi");
            assert!(is_pi_managed_install(alias.to_str().unwrap()));
            assert!(tool_discovery::additional_script_install(
                "pi", &alias, &entry
            ));
            assert!(selected_tool_update_command("pi", &alias, &entry, false).is_some());
        }

        // Preserve the legacy parser's single-BOM acceptance boundary.
        std::fs::write(
            install_dir.join("managed-install.json"),
            "\u{feff}\u{feff}{\"kind\":\"pi-managed-install\"}",
        )
        .unwrap();
        assert!(!is_pi_managed_install(&entry_str));

        // Wrong kind or missing launcher/metadata must not match.
        std::fs::write(
            install_dir.join("managed-install.json"),
            r#"{"kind":"other"}"#,
        )
        .unwrap();
        assert!(!is_pi_managed_install(&entry_str));

        std::fs::write(
            install_dir.join("managed-install.json"),
            r#"{"kind":"pi-managed-install"}"#,
        )
        .unwrap();
        std::fs::remove_file(bin_dir.join("pi-launcher.js")).unwrap();
        assert!(!is_pi_managed_install(&entry_str));
    }

    #[test]
    fn opencode_desktop_cli_is_unmanaged_only_when_default() {
        let install = |path: &str, source: &str, is_path_default: bool| ToolInstallation {
            path: path.to_string(),
            version: Some("1.0.0".to_string()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: source.to_string(),
            is_path_default,
            real: Path::new(path).to_path_buf(),
        };

        assert_eq!(
            resolve_update_command(
                "opencode",
                &[install("desktop/opencode-cli.exe", "desktop", true)]
            ),
            UpdateCommand::Unmanaged
        );

        let desktop = install("desktop/opencode-cli.exe", "desktop", false);
        let prefix = tempfile::tempdir().unwrap();
        let bin = if cfg!(windows) {
            prefix.path().to_owned()
        } else {
            prefix.path().join("bin")
        };
        std::fs::create_dir_all(&bin).unwrap();
        let launcher = bin.join(if cfg!(windows) {
            "opencode.cmd"
        } else {
            "opencode"
        });
        std::fs::write(&launcher, "").unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }), "").unwrap();
        let standalone = install(launcher.to_str().unwrap(), "npm", true);
        let UpdateCommand::Anchored(command) =
            resolve_update_command("opencode", &[desktop, standalone])
        else {
            panic!("expected an update anchored to the standalone installation");
        };
        assert!(command.contains(bin.to_str().unwrap()));
        assert!(!command.contains("opencode-cli.exe"));
    }

    #[test]
    fn all_tools_lifecycle_invariants() {
        let requested = vec![
            "unsupported".to_string(),
            "grok".to_string(),
            "pi".to_string(),
        ];
        let normalized = normalize_requested_tools(&requested);
        assert!(normalized.contains(&"grok") && normalized.contains(&"pi"));
        assert!(!normalized.contains(&"unsupported"));

        for tool in TOOLS.iter() {
            let id = tool.id;
            let posix_install = posix_install_command_for(id);
            assert!(!posix_install.is_empty(), "{id}: empty POSIX install");
            let posix_primary = posix_install.split("||").next().unwrap_or_default();
            assert!(
                !posix_primary.contains('|'),
                "{id}: POSIX installer must not depend on pipefail pipe: {posix_install}"
            );
            assert!(
                !posix_install.contains("pip"),
                "{id}: POSIX install must not invoke pip: {posix_install}"
            );

            let update = static_fallback_command(id);
            assert!(!update.is_empty(), "{id}: empty update fallback");
            #[cfg(not(target_os = "windows"))]
            assert!(
                !update.contains("pip"),
                "{id}: update fallback must not invoke pip: {update}"
            );

            #[cfg(not(target_os = "windows"))]
            let shell = LifecycleCommandShell::Posix;
            #[cfg(target_os = "windows")]
            let shell = LifecycleCommandShell::WindowsBatch;

            if !prefers_official_update(id, shell) && !matches!(id, "hermes" | "agy") {
                assert!(
                    !update.starts_with(&format!("{id} update"))
                        && !update.starts_with(&format!("{id} upgrade")),
                    "{id}: unsupported self-update leaked into fallback: {update}"
                );
            }

            #[cfg(target_os = "windows")]
            {
                let win_install = static_fallback_command_for(id, ToolLifecycleAction::Install);
                assert!(!win_install.is_empty(), "{id}: empty Windows install");
                if id == "hermes" {
                    assert!(
                        !update.contains("call powershell"),
                        "hermes powershell fallback must not use batch `call`: {update}"
                    );
                }
                for cmd in [&win_install, &update] {
                    if cmd.contains("powershell") {
                        let (prefix, _) = cmd.split_once("-EncodedCommand ").unwrap_or_else(|| {
                            panic!("{id}: PowerShell installer must use -EncodedCommand: {cmd}")
                        });
                        assert!(
                            !prefix.contains("-Command") && !prefix.contains("pip"),
                            "{id}: PowerShell prefix invalid: {cmd}"
                        );
                    } else {
                        assert!(
                            !cmd.contains("pip"),
                            "{id}: Windows command must not invoke pip: {cmd}"
                        );
                    }
                }
                for action in [ToolLifecycleAction::Install, ToolLifecycleAction::Update] {
                    let wsl = wsl_tool_action_shell_command(id, action)
                        .unwrap_or_else(|| panic!("{id}: missing WSL command"));
                    assert!(
                        !wsl.contains("powershell")
                            && !wsl.contains("|| call ")
                            && !wsl.contains("pip"),
                        "{id}: WSL command leaked Windows syntax or pip: {wsl}"
                    );
                }
            }
        }
    }

    #[test]
    fn parent_dir_extracts_rightmost_separator() {
        for (input, expected) in [
            ("/Users/me/.volta/bin/codex", "/Users/me/.volta/bin"),
            (
                r"C:\Users\me\AppData\Local\Volta\bin\codex.exe",
                r"C:\Users\me\AppData\Local\Volta\bin",
            ),
            (
                r"C:\Users\me/Code/openclaw\codex.cmd",
                r"C:\Users\me/Code/openclaw",
            ),
            ("codex", ""),
            ("/codex", ""),
            (r"\codex", ""),
        ] {
            assert_eq!(parent_dir(input), expected, "{input}");
        }
    }

    #[test]
    fn installer_root_detection() {
        // Kimi native installer root
        assert_eq!(
            kimi_native_install_root(
                "/home/me/.kimi-code/bin/kimi",
                "/home/me/.kimi-code/bin/kimi"
            )
            .as_deref(),
            Some("/home/me/.kimi-code")
        );
        assert_eq!(
            kimi_native_install_root("/home/me/.local/bin/kimi", "/opt/kimi/bin/kimi").as_deref(),
            Some("/opt/kimi")
        );
        assert_eq!(
            kimi_native_install_root(
                r"C:\Users\sc\.kimi-code\bin\kimi.exe",
                r"\\?\C:\Users\sc\.kimi-code\bin\kimi.exe"
            )
            .as_deref(),
            Some(r"C:\Users\sc\.kimi-code")
        );
        assert_eq!(
            kimi_native_install_root(
                "/home/me/.nvm/versions/node/v22.19.0/bin/kimi",
                "/home/me/.nvm/versions/node/v22.19.0/lib/node_modules/@moonshot-ai/kimi-code/dist/main.mjs"
            ),
            None
        );
        assert_eq!(
            kimi_native_install_root(
                r"C:\Users\sc\AppData\Roaming\npm\kimi.cmd",
                r"\\?\C:\Users\sc\AppData\Roaming\npm\kimi.cmd"
            ),
            None
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            kimi_installer_update_command("/home/me/my kimi"),
            format!("KIMI_INSTALL_DIR='/home/me/my kimi' {KIMI_INSTALL_UNIX}")
        );

        // MiniMax Code installer root candidate & layout
        assert_eq!(
            mcode_install_root_candidate("/opt/bin/bin/mcode", McodeLayout::Posix),
            "/opt/bin"
        );
        assert_eq!(
            mcode_install_root_candidate(r"C:\Users\me\bin\mcode.cmd", McodeLayout::Windows),
            r"C:\Users\me\bin"
        );
        assert_eq!(
            mcode_script_install_root(
                "/Users/me/.local/bin/mcode",
                "/Users/me/.minimax-code/bin/mcode",
                McodeLayout::Posix,
            )
            .as_deref(),
            Some("/Users/me/.minimax-code")
        );
        assert_eq!(
            mcode_script_install_root(
                r"C:\Users\me\.minimax-code\mcode.cmd",
                r"C:\Users\me\.minimax-code\mcode.cmd",
                McodeLayout::Windows,
            )
            .as_deref(),
            Some(r"C:\Users\me\.minimax-code")
        );
        assert_eq!(
            mcode_script_install_root(
                "/opt/homebrew/bin/mcode",
                "/opt/homebrew/lib/node_modules/@minimax-ai/code/cli.js",
                McodeLayout::Posix,
            ),
            None
        );
        assert_eq!(
            mcode_script_install_root(
                "/Users/me/.minimax/bin/mcode",
                "/Users/me/.minimax/bin/mcode",
                McodeLayout::Posix,
            ),
            None
        );
    }

    #[test]
    fn codebuddy_lifecycle_metadata_uses_self_update_with_npm_fallback() {
        let requested = vec!["unsupported".to_string(), "codebuddy".to_string()];
        assert_eq!(normalize_requested_tools(&requested), vec!["codebuddy"]);
        let spec = tool_spec("codebuddy");
        assert_eq!(spec.display_name, "CodeBuddy");
        assert_eq!(spec.npm_package, Some("@tencent-ai/codebuddy-code"));
        let npm = "npm i -g @tencent-ai/codebuddy-code@latest";
        // No dependency runs an install script, so npm 12 needs no `--allow-scripts`.
        assert_eq!(spec.npm_install_command().as_deref(), Some(npm));
        // `codebuddy update` never prompts and exits 1 when it cannot update.
        assert_eq!(spec.official_update_args, Some("update"));

        assert_eq!(
            tool_action_shell_command_for_shell(
                "codebuddy",
                ToolLifecycleAction::Install,
                LifecycleCommandShell::Posix,
            )
            .as_deref(),
            Some(npm)
        );
        assert_eq!(
            tool_action_shell_command_for_shell(
                "codebuddy",
                ToolLifecycleAction::Update,
                LifecycleCommandShell::Posix,
            )
            .as_deref(),
            Some(format!("codebuddy update || {npm}").as_str())
        );
        assert_eq!(
            tool_action_shell_command_for_shell(
                "codebuddy",
                ToolLifecycleAction::Update,
                LifecycleCommandShell::WindowsBatch,
            )
            .as_deref(),
            Some(format!("codebuddy update || call {npm}").as_str())
        );
    }

    #[test]
    fn codex_standalone_detection() {
        assert_eq!(
            codex_standalone_install(
                "/Users/me/.local/bin/codex",
                "/Users/me/.codex/packages/standalone/releases/0.157.0-aarch64-apple-darwin/bin/codex",
            ),
            Some(CodexStandaloneInstall {
                install_dir: "/Users/me/.local/bin".to_string(),
                codex_home: Some("/Users/me/.codex".to_string()),
            })
        );
        // Windows `\\?\` & `\\?\UNC\` verbatim prefixes are restored before passing to installer.
        assert_eq!(
            codex_standalone_install(
                r"C:\Users\Me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe",
                r"\\?\C:\Users\Me\.Codex\Packages\Standalone\releases\0.157.0-x86_64-pc-windows-msvc\bin\codex.exe",
            ),
            Some(CodexStandaloneInstall {
                install_dir: r"C:\Users\Me\AppData\Local\Programs\OpenAI\Codex\bin".to_string(),
                codex_home: Some(r"C:\Users\Me\.Codex".to_string()),
            })
        );
        assert_eq!(
            codex_standalone_install(
                r"\\server\share\bin\codex.exe",
                r"\\?\UNC\server\share\cx\packages\standalone\current\bin\codex.exe",
            )
            .and_then(|install| install.codex_home),
            Some(r"\\server\share\cx".to_string())
        );
        let default_win_bin = r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe";
        assert_eq!(
            codex_standalone_install(default_win_bin, default_win_bin),
            Some(CodexStandaloneInstall {
                install_dir: r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin".to_string(),
                codex_home: None,
            })
        );
        for (bin, real) in [
            (
                "/Users/me/.nvm/versions/node/v22.19.0/bin/codex",
                "/Users/me/.nvm/versions/node/v22.19.0/lib/node_modules/@openai/codex/bin/codex.js",
            ),
            (
                "/opt/homebrew/bin/codex",
                "/opt/homebrew/Caskroom/codex/0.157.0/bin/codex",
            ),
            (
                "/Users/me/.local/bin/codex",
                "/Users/me/.codex/packages/app-server-daemon/current/bin/codex",
            ),
            (
                r"C:\Users\me\AppData\Roaming\npm\codex.cmd",
                r"C:\Users\me\AppData\Roaming\npm\codex.cmd",
            ),
        ] {
            assert_eq!(codex_standalone_install(bin, real), None, "{bin}");
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn opencode_v2_npm_upgrade_keeps_the_selected_package() {
        let bin = "/home/test/.nvm/versions/node/v24/bin/opencode";
        let real = "/home/test/.nvm/versions/node/v24/lib/node_modules/@opencode/cli/bin/opencode";
        let command = anchored_command_from_paths("opencode", bin, real).unwrap();
        assert!(command.contains("npm_config_allow_scripts=@opencode/cli"));
        assert!(command.contains("npm i -g @opencode/cli@latest"));
        assert!(!command.contains("opencode-ai"));
    }

    #[cfg(target_os = "windows")]
    mod windows {
        use super::super::*;

        fn setup_sibling(
            subdir: &str,
            entry: &str,
            siblings: &[&str],
        ) -> (tempfile::TempDir, std::path::PathBuf, String) {
            let dir = tempfile::tempdir().unwrap();
            let sub = if subdir.is_empty() {
                dir.path().to_path_buf()
            } else {
                dir.path().join(subdir)
            };
            std::fs::create_dir_all(&sub).unwrap();
            std::fs::write(sub.join(entry), "").unwrap();
            for s in siblings {
                std::fs::write(sub.join(s), "").unwrap();
            }
            let bin_path = sub.join(entry).to_string_lossy().to_string();
            (dir, sub, bin_path)
        }

        #[test]
        fn win_quote_path_for_batch_handles_metacharacters_and_percent() {
            for (input, expected) in [
                (r"C:\Users\me\npm.cmd", r"C:\Users\me\npm.cmd"),
                (
                    r"C:\Program Files\nodejs\npm.cmd",
                    r#""C:\Program Files\nodejs\npm.cmd""#,
                ),
                (r"C:\Tools&Dev\npm.cmd", r#""C:\Tools&Dev\npm.cmd""#),
                (r"C:\Foo(x86)\npm.cmd", r#""C:\Foo(x86)\npm.cmd""#),
                (r"C:\foo^bar\npm.cmd", r#""C:\foo^bar\npm.cmd""#),
                // `%` 经历 .bat + call 两轮展开须 4 倍转义，且纯 `%` 路径不额外加引号
                (r"C:\path%foo%\npm.cmd", r"C:\path%%%%foo%%%%\npm.cmd"),
                (r"C:\my %dir%\npm.cmd", r#""C:\my %%%%dir%%%%\npm.cmd""#),
            ] {
                assert_eq!(win_quote_path_for_batch(input), expected, "{input}");
            }
        }

        #[test]
        fn sibling_bin_with_ext_respects_extension_priority() {
            let dir = tempfile::tempdir().unwrap();
            let npm_cmd = dir.path().join("npm.cmd");
            let npm_exe = dir.path().join("npm.exe");
            let volta_exe = dir.path().join("volta.exe");
            std::fs::write(&npm_cmd, "").unwrap();
            std::fs::write(&npm_exe, "").unwrap();
            std::fs::write(&volta_exe, "").unwrap();

            let codex = dir.path().join("codex.cmd").to_string_lossy().to_string();
            assert_eq!(
                sibling_bin_with_ext(&codex, "npm", &["cmd", "exe"]).as_deref(),
                Some(npm_cmd.to_string_lossy().as_ref())
            );
            assert_eq!(
                sibling_bin_with_ext(&codex, "volta", &["exe", "cmd"]).as_deref(),
                Some(volta_exe.to_string_lossy().as_ref())
            );
            assert!(sibling_bin_with_ext(&codex, "pnpm", &["cmd", "exe"]).is_none());
            assert!(sibling_bin_with_ext("codex.cmd", "npm", &["cmd"]).is_none());
        }

        #[test]
        fn windows_anchored_upgrade_channels() {
            let (_dir, sub, bin_path) = setup_sibling("Volta", "codex.cmd", &["volta.exe"]);
            let volta = win_quote_path_for_batch(&format!("{}\\volta.exe", sub.to_string_lossy()));
            assert_eq!(
                anchored_command_from_paths("codex", &bin_path, &bin_path).as_deref(),
                Some(format!("{volta} install @openai/codex").as_str())
            );

            let (_dir, sub, bin_path) = setup_sibling("pnpm", "opencode.cmd", &["pnpm.cmd"]);
            let pnpm = win_quote_path_for_batch(&format!("{}\\pnpm.cmd", sub.to_string_lossy()));
            assert_eq!(
                anchored_command_from_paths("opencode", &bin_path, &bin_path).as_deref(),
                Some(format!("{pnpm} add -g opencode-ai@latest").as_str())
            );
            // OpenCode 2 的 shim 指向 @opencode/cli；装 opencode-ai 会被 pnpm 静默顶掉入口。
            std::fs::write(
                &bin_path,
                "@\"%~dp0\\global\\5\\node_modules\\@opencode\\cli\\bin\\opencode.exe\" %*",
            )
            .unwrap();
            assert_eq!(
                anchored_command_from_paths("opencode", &bin_path, &bin_path).as_deref(),
                Some(format!("{pnpm} add -g @opencode/cli@latest").as_str())
            );

            let (_dir, sub, bin_path) = setup_sibling("v22.19.0", "mcode.cmd", &["npm.cmd"]);
            let npm = win_quote_path_for_batch(&format!("{}\\npm.cmd", sub.to_string_lossy()));
            assert_eq!(
                anchored_command_from_paths("mcode", &bin_path, &bin_path).as_deref(),
                Some(
                    format!(
                        "{npm} i -g @minimax-ai/code@latest{}",
                        tool_spec("mcode").npm_install_args
                    )
                    .as_str()
                )
            );

            // Standalone CLI without sibling manager falls back to self-update only.
            let (_dir, _sub, bin_path) = setup_sibling("", "claude.cmd", &[]);
            assert_eq!(
                anchored_command_from_paths("claude", &bin_path, &bin_path).as_deref(),
                Some(format!("{} update", win_quote_path_for_batch(&bin_path)).as_str())
            );

            // Grok native uses PowerShell fallback without `|| call` or npm.
            let (_dir, _sub, bin_path) = setup_sibling(".grok/bin", "grok.exe", &["npm.cmd"]);
            let grok = anchored_command_from_paths("grok", &bin_path, &bin_path).unwrap();
            assert_eq!(
                grok,
                format!(
                    "{} update || {}",
                    win_quote_path_for_batch(&bin_path),
                    grok_install_windows_command()
                )
            );
            assert!(!grok.contains("|| call") && !grok.contains("npm"), "{grok}");
        }

        #[test]
        fn windows_installer_update_commands_escape_single_quotes() {
            let (_dir, sub, bin_path) =
                setup_sibling(".minimax-code", "mcode.cmd", &["mcode.ps1", "npm.cmd"]);
            let mcode = anchored_command_from_paths("mcode", &bin_path, &bin_path).unwrap();
            assert_eq!(
                mcode,
                mcode_installer_update_command(&sub.to_string_lossy())
            );
            assert!(!mcode.contains("npm"), "{mcode}");

            let mcode_quoted = mcode_installer_update_command(r"C:\Users\o'brien\.minimax-code");
            let expected_mcode = powershell_encoded_command(
                "$env:MCODE_INSTALL_DIR = 'C:\\Users\\o''brien\\.minimax-code'; irm https://filecdn.minimax.chat/public/install.ps1 | iex",
            );
            assert_eq!(
                mcode_quoted.split_once("-EncodedCommand ").map(|(_, e)| e),
                Some(expected_mcode.as_str())
            );

            let codex_bin = r"C:\Users\o'brien\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe";
            let codex_real = r"\\?\C:\Users\o'brien\.codex\packages\standalone\releases\0.157.0-x86_64-pc-windows-msvc\bin\codex.exe";
            let codex = anchored_command_from_paths("codex", codex_bin, codex_real).unwrap();
            let expected_codex = powershell_encoded_command(
                "$env:CODEX_NON_INTERACTIVE = '1'; $env:CODEX_INSTALL_DIR = 'C:\\Users\\o''brien\\AppData\\Local\\Programs\\OpenAI\\Codex\\bin'; $env:CODEX_HOME = 'C:\\Users\\o''brien\\.codex'; irm https://chatgpt.com/codex/install.ps1 | iex",
            );
            assert_eq!(
                codex.split_once("-EncodedCommand ").map(|(_, e)| e),
                Some(expected_codex.as_str())
            );
            assert!(!codex.contains("npm"), "{codex}");
        }

        #[test]
        fn windows_full_batch_line_for_percent_path_uses_quadruple_escape() {
            let (_dir, sub, bin_path) = setup_sibling("path%foo%", "codex.cmd", &["npm.cmd"]);
            let anchored = anchored_command_from_paths("codex", &bin_path, &bin_path).unwrap();
            let batch_line = format!("call {anchored}");
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            assert_eq!(
                batch_line,
                format!(
                    "call {} i -g @openai/codex@latest",
                    win_quote_path_for_batch(&npm_full)
                )
            );
            assert!(batch_line.contains("%%%%foo%%%%"), "{batch_line}");
            assert!(!batch_line.contains("path%foo%"), "{batch_line}");
        }

        #[test]
        fn wsl_action_line_quotes_unix_installer_without_pipefail() {
            let line = build_wsl_tool_action_line("Ubuntu", HERMES_INSTALL_UNIX, None, None)
                .expect("valid WSL command line");
            assert!(line.starts_with("wsl.exe -d Ubuntu -- sh -c "), "{line}");
            assert!(!line.contains("| bash") && line.contains(" -o $tmp && bash $tmp"));
        }
    }
}
