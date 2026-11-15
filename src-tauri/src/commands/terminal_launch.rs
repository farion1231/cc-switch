use super::misc::{is_valid_shell, shell_single_quote};
use crate::app_config::AppType;
use crate::services::ProviderService;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::State;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "macos")]
use super::misc::decode_command_output;
#[cfg(target_os = "windows")]
use super::misc::{
    effective_path_string, is_valid_wsl_distro_name, windows_cmd_double_quote_arg,
    windows_shell_compatible_path, wsl_auto_shell_command, wsl_distro_for_tool, CREATE_NO_WINDOW,
};

// 以下 shell 解析辅助函数仅被 macOS/Linux 的终端启动逻辑使用；Windows 非 test 编译下为死代码。
#[cfg_attr(windows, allow(dead_code))]
fn fallback_user_shell() -> &'static str {
    if cfg!(target_os = "macos") {
        "/bin/zsh"
    } else {
        "/bin/bash"
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn valid_user_shell_path(shell: &str) -> bool {
    if shell.is_empty()
        || !shell.starts_with('/')
        || !is_valid_shell(shell)
        || shell.chars().any(char::is_control)
    {
        return false;
    }

    let path = std::path::Path::new(shell);
    path.is_file() && is_executable_file(path)
}

#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
#[cfg_attr(windows, allow(dead_code))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

/// 获取用户默认 shell 的完整路径；异常或被污染的 SHELL 回退到平台默认值。
#[cfg_attr(windows, allow(dead_code))]
fn get_user_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| valid_user_shell_path(shell))
        .unwrap_or_else(|| fallback_user_shell().to_string())
}

/// 构建 exec 行：引号保护 shell 路径，交还用户 shell 让其按默认规则加载 rc 配置。
#[cfg_attr(windows, allow(dead_code))]
fn build_exec_line(shell: &str, cwd: Option<&Path>) -> String {
    let quoted_shell = shell_single_quote(shell);

    match shell.rsplit('/').next().unwrap_or(shell) {
        "zsh" => cwd
            .map(|dir| {
                let command = format!(
                    "cd {} || exit 1; exec {} -i",
                    shell_single_quote(&dir.to_string_lossy()),
                    quoted_shell
                );
                format!("exec {} -lc {}", quoted_shell, shell_single_quote(&command))
            })
            .unwrap_or_else(|| format!("exec {quoted_shell} -l")),
        _ => format!("exec {quoted_shell}"),
    }
}

/// 构建 provider 命令行：通过用户 shell 的交互模式执行，确保 GUI 启动的终端也加载用户 PATH。
#[cfg_attr(windows, allow(dead_code))]
fn build_provider_command_line(shell: &str, launch_command: &str, cwd: Option<&Path>) -> String {
    let command = cwd
        .map(|dir| {
            format!(
                "cd {} && {}",
                user_shell_quote(shell, &dir.to_string_lossy()),
                launch_command
            )
        })
        .unwrap_or_else(|| launch_command.to_string());

    format!(
        "{} {} {}",
        shell_single_quote(shell),
        provider_command_flag_for_shell(shell),
        shell_single_quote(&command)
    )
}

#[cfg_attr(windows, allow(dead_code))]
fn provider_command_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "zsh" => "-lic",
        _ => "-ic",
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn build_final_shell_cd_command(shell: &str, cwd: Option<&Path>) -> String {
    if matches!(shell.rsplit('/').next().unwrap_or(shell), "zsh") {
        return String::new();
    }

    cwd.map(|dir| {
        format!(
            "cd {} || exit 1\n",
            shell_single_quote(&dir.to_string_lossy())
        )
    })
    .unwrap_or_default()
}
/// `hermes dashboard` 的启动命令行（交给 `launch_terminal_running`）。
///
/// Windows 上 Hermes 配置目录指向 WSL（UNC 路径）时，要进对应发行版里启动，
/// 否则跑起来的是 Windows 本机的 hermes（或报找不到命令），#8689。
/// Web UI 探测的 `127.0.0.1` 端口由 WSL2 的 localhost 转发到发行版里，不用改。
pub(crate) fn hermes_dashboard_command_line() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(distro) = wsl_distro_for_tool("hermes") {
            return wsl_hermes_dashboard_line(&distro);
        }
    }
    Ok("hermes dashboard".to_string())
}

/// 用发行版里用户自己的登录交互 shell（`$SHELL -lic`）跑，PATH 才带得上
/// `~/.local/bin` 这类在 `.bashrc` / `.profile` 里加的目录。只用一个 flag、
/// 不像版本探测那样失败后换 flag 重试：dashboard 被 Ctrl+C 结束也是非零退出，
/// 重试会把它再拉起来。
#[cfg(target_os = "windows")]
fn wsl_hermes_dashboard_line(distro: &str) -> Result<String, String> {
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: {distro}"));
    }
    Ok(format!(
        "wsl.exe -d {distro} -- sh -c {}",
        windows_cmd_double_quote_arg(&wsl_auto_shell_command("hermes dashboard", Some("-lic")))
    ))
}

/// 打开指定提供商的终端
///
/// 根据提供商配置的环境变量启动一个带有该提供商特定设置的终端
/// 无需检查是否为当前激活的提供商，任何提供商都可以打开终端
#[allow(non_snake_case, clippy::too_many_arguments)]
#[tauri::command]
pub async fn open_provider_terminal(
    state: State<'_, crate::store::AppState>,
    agy_accounts: State<'_, crate::commands::AgyAccountState>,
    copilot_auth: State<'_, crate::commands::CopilotAuthState>,
    app: String,
    #[allow(non_snake_case)] providerId: String,
    cwd: Option<String>,
    #[allow(non_snake_case)] skipPermissions: Option<bool>,
    #[allow(non_snake_case)] extraArgs: Option<String>,
) -> Result<bool, String> {
    let app_type = AppType::from_str(&app).map_err(|e| e.to_string())?;
    let launch_cwd = resolve_launch_cwd(cwd)?;
    // 先拆参数：填错了直接报错，不必等到切完 agy 登录、写完临时文件。
    let extra_args = split_launch_args(extraArgs.as_deref().unwrap_or_default())?;

    // 获取提供商配置
    let providers = ProviderService::list(state.inner(), app_type.clone())
        .map_err(|e| format!("获取提供商列表失败: {e}"))?;

    let provider = providers
        .get(&providerId)
        .ok_or_else(|| format!("提供商 {providerId} 不存在"))?;

    let mut launch = match app_type {
        AppType::Agy => {
            switch_agy_to_bound_account(&agy_accounts.0, provider).await?;
            agy_terminal_launch(provider)?
        }
        AppType::Copilot => {
            let account = copilot_bound_account(&copilot_auth, provider).await?;
            copilot_terminal_launch(provider, account)?
        }
        AppType::Codex => codex_terminal_launch(state.db.as_ref(), provider)?,
        AppType::GrokBuild => grok_terminal_launch(state.db.as_ref(), provider)?,
        AppType::Claude => {
            use crate::services::provider::claude_official::{terminal_account, AccountTerminal};
            match terminal_account(provider).map_err(|e| e.to_string())? {
                Some(AccountTerminal::Dir(config_dir)) => {
                    crate::services::claude_account::sync_shared(&config_dir);
                    claude_account_terminal_launch(Some(config_dir))
                }
                Some(AccountTerminal::Default) => {
                    crate::services::provider::claude_official::ensure_default_terminal_is_official(
                        &state.db,
                    )
                    .map_err(|e| e.to_string())?;
                    claude_account_terminal_launch(None)
                }
                None => {
                    let env_vars =
                        extract_env_vars_from_config(&provider.settings_config, &app_type);
                    let drops = crate::services::provider::claude_switch_drops(
                        state.inner(),
                        &provider.settings_config,
                    )
                    .unwrap_or_else(|e| {
                        log::warn!("计算终端要挡住的 live 键失败，按原样启动: {e}");
                        Default::default()
                    });
                    claude_terminal_launch(&env_vars, &providerId, &provider.name, &drops)?
                }
            }
        }
        _ => {
            let env_vars = extract_env_vars_from_config(&provider.settings_config, &app_type);
            claude_terminal_launch(&env_vars, &providerId, &provider.name, &Default::default())?
        }
    };
    if skipPermissions == Some(true) {
        launch.skip_permissions();
    }
    if let Err(e) = launch.append_user_args(extra_args) {
        for file in &launch.cleanup {
            let _ = std::fs::remove_file(file);
        }
        return Err(e);
    }

    launch_provider_terminal(&launch, launch_cwd.as_deref())
        .map_err(|e| format!("启动终端失败: {e}"))?;

    Ok(true)
}

/// 从提供商配置中提取环境变量
fn extract_env_vars_from_config(
    config: &serde_json::Value,
    app_type: &AppType,
) -> Vec<(String, String)> {
    let mut env_vars = Vec::new();

    let Some(obj) = config.as_object() else {
        return env_vars;
    };

    // 处理 env 字段（Claude/Gemini 通用）
    if let Some(env) = obj.get("env").and_then(|v| v.as_object()) {
        for (key, value) in env {
            if let Some(str_val) = value.as_str() {
                env_vars.push((key.clone(), str_val.to_string()));
            }
        }

        // 处理 base_url: 根据应用类型添加对应的环境变量
        let base_url_key = match app_type {
            AppType::Claude | AppType::ClaudeDesktop => Some("ANTHROPIC_BASE_URL"),
            AppType::Gemini => Some("GOOGLE_GEMINI_BASE_URL"),
            _ => None,
        };

        if let Some(key) = base_url_key {
            if let Some(url_str) = env.get(key).and_then(|v| v.as_str()) {
                env_vars.push((key.to_string(), url_str.to_string()));
            }
        }
    }

    // Codex 使用 auth 字段转换为 OPENAI_API_KEY
    if *app_type == AppType::Codex {
        if let Some(auth) = obj.get("auth").and_then(|v| v.as_str()) {
            env_vars.push(("OPENAI_API_KEY".to_string(), auth.to_string()));
        }
    }

    // Gemini 使用 api_key 字段转换为 GEMINI_API_KEY
    if *app_type == AppType::Gemini {
        if let Some(api_key) = obj.get("api_key").and_then(|v| v.as_str()) {
            env_vars.push(("GEMINI_API_KEY".to_string(), api_key.to_string()));
        }
    }

    env_vars
}

fn resolve_launch_cwd(cwd: Option<String>) -> Result<Option<PathBuf>, String> {
    let Some(raw_path) = cwd.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };

    if raw_path.contains('\n') || raw_path.contains('\r') {
        return Err("目录路径包含非法换行符".to_string());
    }

    let path = Path::new(&raw_path);
    if !path.exists() {
        return Err(format!("目录不存在: {raw_path}"));
    }

    let resolved = std::fs::canonicalize(path).map_err(|e| format!("解析目录失败: {e}"))?;
    if !resolved.is_dir() {
        return Err(format!("选择的路径不是文件夹: {}", resolved.display()));
    }

    #[cfg(target_os = "windows")]
    let resolved = windows_shell_compatible_path(&resolved);

    Ok(Some(resolved))
}

/// 在终端里启动的一次供应商会话：跑什么命令、只给它哪些环境变量、退出后删哪些临时
/// 文件。打开终端那一层（各家终端的 AppleScript、Linux 候选列表、Windows start）对所有
/// 应用共用，只有这里按应用不同。
struct ProviderLaunch {
    /// 启动前打印的说明
    title: &'static str,
    /// 说明下面打印的路径（只在 macOS / Linux 打印）
    #[cfg_attr(windows, allow(dead_code))]
    detail: Option<PathBuf>,
    program: &'static str,
    /// macOS / Linux 的参数，交给用户 shell 执行前逐个加引号
    #[cfg_attr(windows, allow(dead_code))]
    args: Vec<String>,
    /// Windows .bat 里执行的命令
    #[cfg_attr(not(windows), allow(dead_code))]
    batch_command: String,
    /// 只给这一个进程的环境变量；`None` 表示要清掉（用户 rc 里可能 export 过同名变量）。
    /// macOS / Linux 写进 0600 的临时文件、读完立即删除；Windows 写进 .bat 的 `setlocal` 段。
    env: Vec<(&'static str, Option<String>)>,
    /// 只在 .bat 里设置、给 `batch_command` 引用的变量（比如可能含非 ASCII 的路径）
    #[cfg_attr(not(windows), allow(dead_code))]
    batch_vars: Vec<(&'static str, String)>,
    /// 退出时删除的临时文件，都在系统临时目录里
    cleanup: Vec<PathBuf>,
}

impl ProviderLaunch {
    /// 用户在「打开终端」里勾了跳过权限确认：只加给这一次启动，不写进任何配置。
    fn skip_permissions(&mut self) {
        // Claude Code 和 agy 的参数同名；Codex 跳过确认的同时也关掉沙箱；Grok Build 用自己的；
        // Copilot CLI 的 `--allow-all` 放开工具、路径和 URL 三类确认。
        let flag = match self.program {
            "codex" => "--dangerously-bypass-approvals-and-sandbox",
            "grok" => "--always-approve",
            "copilot" => "--allow-all",
            _ => "--dangerously-skip-permissions",
        };
        self.args.push(flag.to_string());
        self.batch_command.push(' ');
        self.batch_command.push_str(flag);
    }

    /// 用户在「打开终端」里填的附加参数，接在 CC Switch 生成的参数后面，同样只给这一次。
    /// Windows 上每个参数经 .bat 变量传、用引号包住，cmd 的 `&`、`|` 之类不会接出别的命令。
    fn append_user_args(&mut self, args: Vec<String>) -> Result<(), String> {
        // Codex 的启动行带 `call`：`%` 会再展开一遍、引号里的 `^` 会加倍，和 `-c` 值同一条限制。
        if cfg!(target_os = "windows") && args.iter().any(|arg| arg.contains(['"', '%', '^'])) {
            return Err("附加参数里有 Windows 终端传不过去的字符（\" % ^）".to_string());
        }
        for (arg, var) in args.into_iter().zip(USER_ARG_BATCH_VARS) {
            // 程序按 Windows 的规则拆命令行：收尾引号前的反斜杠要成对，否则 `C:\work\` 会把
            // 引号当成内容吞掉。
            let trailing = arg.len() - arg.trim_end_matches('\\').len();
            let quoted_value = format!("{arg}{}", "\\".repeat(trailing));
            self.batch_command.push_str(&format!(" \"%{var}%\""));
            self.batch_vars.push((var, quoted_value));
            self.args.push(arg);
        }
        Ok(())
    }
}

/// 附加参数在 .bat 里经这些变量传，也是附加参数的个数上限（各平台一样）。
const USER_ARG_BATCH_VARS: [&str; 16] = [
    "CC_SWITCH_ARG0",
    "CC_SWITCH_ARG1",
    "CC_SWITCH_ARG2",
    "CC_SWITCH_ARG3",
    "CC_SWITCH_ARG4",
    "CC_SWITCH_ARG5",
    "CC_SWITCH_ARG6",
    "CC_SWITCH_ARG7",
    "CC_SWITCH_ARG8",
    "CC_SWITCH_ARG9",
    "CC_SWITCH_ARG10",
    "CC_SWITCH_ARG11",
    "CC_SWITCH_ARG12",
    "CC_SWITCH_ARG13",
    "CC_SWITCH_ARG14",
    "CC_SWITCH_ARG15",
];

/// 把附加参数按空白拆开：单引号或双引号括起来的部分算同一个参数（引号本身去掉），没有
/// 反斜杠转义（Windows 路径里全是反斜杠）。只拆成参数，不经 shell，`;`、`&`、`|` 原样交给 CLI。
fn split_launch_args(input: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in input.chars() {
        if c.is_control() && !(quote.is_none() && c.is_whitespace()) {
            return Err("附加参数里不能有换行等控制字符".to_string());
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    args.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            None => {
                current.push(c);
                in_word = true;
            }
        }
    }
    if quote.is_some() {
        return Err("附加参数里的引号没有配对".to_string());
    }
    if in_word {
        args.push(current);
    }
    if args.len() > USER_ARG_BATCH_VARS.len() {
        return Err(format!("附加参数最多 {} 个", USER_ARG_BATCH_VARS.len()));
    }
    Ok(args)
}

/// Codex 的 `-c` 值在 .bat 里经这些变量传（值里可能有 `&` 之类 cmd 的特殊字符）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const CODEX_OVERRIDE_BATCH_VARS: [&str; 24] = [
    "CC_SWITCH_CODEX_C0",
    "CC_SWITCH_CODEX_C1",
    "CC_SWITCH_CODEX_C2",
    "CC_SWITCH_CODEX_C3",
    "CC_SWITCH_CODEX_C4",
    "CC_SWITCH_CODEX_C5",
    "CC_SWITCH_CODEX_C6",
    "CC_SWITCH_CODEX_C7",
    "CC_SWITCH_CODEX_C8",
    "CC_SWITCH_CODEX_C9",
    "CC_SWITCH_CODEX_C10",
    "CC_SWITCH_CODEX_C11",
    "CC_SWITCH_CODEX_C12",
    "CC_SWITCH_CODEX_C13",
    "CC_SWITCH_CODEX_C14",
    "CC_SWITCH_CODEX_C15",
    "CC_SWITCH_CODEX_C16",
    "CC_SWITCH_CODEX_C17",
    "CC_SWITCH_CODEX_C18",
    "CC_SWITCH_CODEX_C19",
    "CC_SWITCH_CODEX_C20",
    "CC_SWITCH_CODEX_C21",
    "CC_SWITCH_CODEX_C22",
    "CC_SWITCH_CODEX_C23",
];

/// Codex：卡就是当前直连的那家时直接跑 `codex`；否则用 `-c` 只对这个进程换成它，Key 经
/// 环境变量传（见 `codex_terminal`），不改 `config.toml`。
fn codex_terminal_launch(
    db: &crate::database::Database,
    provider: &crate::provider::Provider,
) -> Result<ProviderLaunch, String> {
    use crate::services::provider::codex_terminal;
    let plan = codex_terminal::terminal_launch(db, provider).map_err(|e| e.to_string())?;
    let mut overrides = plan.overrides;
    let mut cleanup = Vec::new();
    if let Some(catalog) = plan.catalog {
        let path = unique_temp_path("codex_catalog", "json");
        std::fs::write(&path, catalog).map_err(|e| format!("写入临时模型目录失败: {e}"))?;
        overrides.push(codex_terminal::catalog_override(&path.to_string_lossy()));
        cleanup.push(path);
    }
    codex_launch(&provider.name, overrides, plan.env, cleanup.clone()).inspect_err(|_| {
        for file in &cleanup {
            let _ = std::fs::remove_file(file);
        }
    })
}

fn codex_launch(
    name: &str,
    overrides: Vec<String>,
    env: Vec<(&'static str, Option<String>)>,
    cleanup: Vec<PathBuf>,
) -> Result<ProviderLaunch, String> {
    if overrides.len() > CODEX_OVERRIDE_BATCH_VARS.len() {
        return Err(format!("「{name}」要覆盖的 Codex 配置项太多"));
    }
    // cmd 不认 `\"`，值里有双引号就会把后面的 `&` 等字符露在引号外面；`call` 会把 `%` 再展开
    // 一遍、把引号里的 `^` 加倍。
    if cfg!(target_os = "windows")
        && overrides
            .iter()
            .any(|value| value.contains(['"', '%', '^']))
    {
        return Err(format!(
            "「{name}」的 Codex 配置里有 Windows 终端传不过去的字符（\" % ^），请先切换到这张卡再打开终端"
        ));
    }

    let mut args = Vec::new();
    // npm 装的是 codex.cmd：不加 `call` 跑完不回到启动 .bat，后面删临时文件（含 Key 的
    // .bat 本身）的几行就不执行了。
    let mut batch_command = String::from("call codex");
    let mut batch_vars = Vec::new();
    for (value, var) in overrides.into_iter().zip(CODEX_OVERRIDE_BATCH_VARS) {
        args.push("-c".to_string());
        args.push(value.clone());
        batch_command.push_str(&format!(" -c \"%{var}%\""));
        batch_vars.push((var, value));
    }
    Ok(ProviderLaunch {
        title: if args.is_empty() {
            "Starting Codex with its current config"
        } else {
            "Using this provider for Codex in this terminal only (config.toml is unchanged)"
        },
        detail: None,
        program: "codex",
        args,
        batch_command,
        env,
        batch_vars,
        cleanup,
    })
}

/// Grok Build：直接跑 `grok`，读的就是 live 的 `config.toml`，所以只能打开当前供应商。
///
/// Grok CLI 没有只对一个进程换配置的办法：没有 `-c` 之类的覆盖参数，`GROK_CONFIG_PATH`
/// 不生效（1.0.70 实测），`GROK_HOME` 会把会话、登录、技能一起换到别的目录。
/// `GROK_HOME` 定到 CC Switch 管的目录（设置里可以改），Grok 读的就是这里写的那份。
fn grok_terminal_launch(
    db: &crate::database::Database,
    provider: &crate::provider::Provider,
) -> Result<ProviderLaunch, String> {
    use crate::mode::current::{self, Purpose};
    let in_use = current::provider_for(db, &AppType::GrokBuild, Purpose::InUse)
        .map_err(|e| e.to_string())?;
    if in_use.as_deref() != Some(provider.id.as_str()) {
        return Err(crate::error::AppError::localized(
            "grokbuild.terminal.not_current",
            format!(
                "Grok Build 不能只对一个终端换供应商：请先切换到「{}」，再打开终端",
                provider.name
            ),
            format!(
                "Grok Build can't use a different provider in a single terminal. Switch to \"{}\" first, then open the terminal",
                provider.name
            ),
        )
        .to_string());
    }
    Ok(grok_launch(
        crate::grok_config::get_grok_config_dir()
            .to_string_lossy()
            .into_owned(),
    ))
}

fn grok_launch(grok_home: String) -> ProviderLaunch {
    ProviderLaunch {
        title: "Starting Grok Build with its current config",
        detail: None,
        program: "grok",
        args: Vec::new(),
        // npm 装的是 grok.cmd：不加 `call` 跑完不回到启动 .bat，后面的清理就不执行了。
        batch_command: "call grok".to_string(),
        env: vec![("GROK_HOME", Some(grok_home))],
        batch_vars: Vec::new(),
        cleanup: Vec::new(),
    }
}

/// Claude Code：把卡上的 env 和 `/model` 菜单写进临时 settings 文件，
/// `claude --settings <文件>` 启动。`drops` 是切到这张卡时会从 live 清掉的键，在临时文件里
/// 挡住（见 [`write_claude_config`]）。
fn claude_terminal_launch(
    env_vars: &[(String, String)],
    provider_id: &str,
    provider_name: &str,
    drops: &crate::services::provider::SwitchDrops,
) -> Result<ProviderLaunch, String> {
    let config_file = unique_temp_path(&format!("claude_{provider_id}"), "json");
    write_claude_config(&config_file, env_vars, provider_name, drops)?;
    let config_name = config_file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(ProviderLaunch {
        title: "Using provider-specific claude config:",
        detail: Some(config_file.clone()),
        program: "claude",
        args: vec![
            "--settings".to_string(),
            config_file.to_string_lossy().into_owned(),
        ],
        // 配置文件和 .bat 同在 temp_dir（%TEMP% 含用户名），用 %~dp0 引用：目录由 cmd
        // 运行时展开，不进 .bat 字节，非 ASCII 用户名下也不会乱码（#8034）。
        batch_command: format!(
            "claude --settings \"%~dp0{}\"",
            escape_windows_batch_value(&config_name)
        ),
        env: Vec::new(),
        batch_vars: Vec::new(),
        cleanup: vec![config_file],
    })
}

/// Claude 官方卡绑了账号：`CLAUDE_CONFIG_DIR` 放在启动环境里（不用 `--settings`）。
/// 变量在进程启动前就设好，Claude Code 一开始预取钥匙串读的就是这个账号的条目
/// （写在 settings 里的话 macOS 上前 30 秒会用默认登录的令牌）。会把它指向别处的变量一并清掉。
/// 绑号卡的终端：清掉会改走别家服务或别的登录的变量，再指定账号目录；`None` 是账号正在默认
/// 槽位里，去掉 `CLAUDE_CONFIG_DIR` 用默认目录。
fn claude_account_terminal_launch(config_dir: Option<PathBuf>) -> ProviderLaunch {
    let mut env: Vec<(&'static str, Option<String>)> =
        crate::services::claude_account::login::CLAUDE_ACCOUNT_UNSET
            .iter()
            .map(|name| (*name, None))
            .collect();
    env.push((
        "CLAUDE_CONFIG_DIR",
        config_dir
            .as_ref()
            .map(|dir| dir.to_string_lossy().into_owned()),
    ));
    ProviderLaunch {
        title: if config_dir.is_some() {
            "Using this Claude account's config directory:"
        } else {
            "Using this Claude account via Claude Code's current sign-in."
        },
        detail: config_dir,
        program: "claude",
        args: Vec::new(),
        batch_command: "claude".to_string(),
        env,
        batch_vars: Vec::new(),
        cleanup: Vec::new(),
    }
}

/// 官方卡绑了授权中心的账号时，先把 agy 的全局登录切到它（已经是它就不写）。没绑的卡跟着
/// agy 当前的登录走。不能切号的平台（目前除 macOS、Windows 外）忽略绑定，照常打开。
async fn switch_agy_to_bound_account(
    accounts: &crate::services::agy_account::AgyAccountManager,
    provider: &crate::provider::Provider,
) -> Result<(), String> {
    if !crate::services::provider::agy_direct::is_official(provider) {
        return Ok(());
    }
    let Some(account_id) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("agy_oauth"))
        .filter(|id| !id.trim().is_empty())
    else {
        return Ok(());
    };
    if !crate::services::agy_account::keychain::supported() {
        log::info!("[AgyAccount] 这个平台不能切号，忽略卡片绑定的账号");
        return Ok(());
    }
    if !accounts
        .list()
        .await
        .iter()
        .any(|account| account.id == account_id)
    {
        return Err(crate::error::AppError::localized(
            "agy.account.bound_missing",
            format!(
                "「{}」绑定的 Antigravity 账号不在这台电脑的授权中心里：请先在授权中心添加它，或编辑这张卡改选账号",
                provider.name
            ),
            format!(
                "The Antigravity account bound to \"{}\" is not in this computer's Auth Center. Add it there, or edit the card to choose another account.",
                provider.name
            ),
        )
        .to_string());
    }
    accounts
        .switch_to(&account_id)
        .await
        .map_err(|error| error.to_string())
}

/// Antigravity CLI：官方登录卡跑 `agy`，卡上设了模型就加 `--model`（只对这次会话生效）；
/// Key 卡用它的私有 `--gemini_dir`，Key 和地址只经环境变量传给这一个进程。
fn agy_terminal_launch(provider: &crate::provider::Provider) -> Result<ProviderLaunch, String> {
    let launch = crate::services::provider::agy_direct::terminal_launch(provider)
        .map_err(|e| e.to_string())?;
    let Some(dir) = launch.gemini_dir else {
        let (args, batch_command, batch_vars) = match launch.model {
            Some(model) => (
                vec![format!("--model={model}")],
                format!("agy \"--model=%{AGY_MODEL_BATCH_VAR}%\""),
                vec![(AGY_MODEL_BATCH_VAR, model)],
            ),
            None => (Vec::new(), "agy".to_string(), Vec::new()),
        };
        return Ok(ProviderLaunch {
            title: "Starting Antigravity CLI with your signed-in account",
            detail: None,
            program: "agy",
            args,
            batch_command,
            env: launch.env,
            batch_vars,
            cleanup: Vec::new(),
        });
    };
    Ok(ProviderLaunch {
        title: "Using a private Antigravity profile for this provider:",
        detail: Some(dir.clone()),
        program: "agy",
        args: vec![format!("--gemini_dir={}", dir.to_string_lossy())],
        batch_command: format!("agy \"--gemini_dir=%{AGY_DIR_BATCH_VAR}%\""),
        env: launch.env,
        batch_vars: vec![(AGY_DIR_BATCH_VAR, dir.to_string_lossy().into_owned())],
        cleanup: Vec::new(),
    })
}

/// 恢复 Antigravity 第三方 Key 模式的会话：和「打开终端」同样带私有目录和 Key，再接上
/// `--conversation`。卡上已经没有 Key 时私有目录对不上，直接报错，不退回官方登录。
pub(crate) fn open_agy_resume_terminal(
    provider: &crate::provider::Provider,
    conversation: &str,
    cwd: Option<String>,
) -> Result<(), String> {
    let mut launch = agy_terminal_launch(provider)?;
    // 只有 Key 卡有私有目录（启动前打印的那行路径）；官方卡的参数可能只有 `--model`。
    if launch.detail.is_none() {
        return Err(format!(
            "「{}」已不再带 API Key，无法恢复它在私有目录里的会话",
            provider.name
        ));
    }
    let resume = format!("--conversation={conversation}");
    launch.args.push(resume.clone());
    launch.batch_command = format!("{} {resume}", launch.batch_command);
    let cwd = resolve_launch_cwd(cwd)?;
    launch_provider_terminal(&launch, cwd.as_deref()).map_err(|e| format!("启动终端失败: {e}"))
}

/// 官方卡绑定的授权中心 GitHub 账号的 Token 和域名；没绑时为 `None`，跟着 Copilot CLI 自己的登录走。
async fn copilot_bound_account(
    auth: &crate::commands::CopilotAuthState,
    provider: &crate::provider::Provider,
) -> Result<Option<(String, String)>, String> {
    let Some(account_id) = crate::services::provider::copilot_direct::bound_account_id(provider)
    else {
        return Ok(None);
    };
    let credentials = auth
        .0
        .read()
        .await
        .github_credentials_for_account(&account_id)
        .await;
    match credentials {
        Some(credentials) => Ok(Some(credentials)),
        None => Err(crate::error::AppError::localized(
            "copilot.account.bound_missing",
            format!(
                "「{}」绑定的 GitHub 账号不在授权中心里：请先在授权中心添加它，或编辑这张卡改选账号",
                provider.name
            ),
            format!(
                "The GitHub account bound to \"{}\" is not in the Auth Center. Add it there, or edit the card to choose another account.",
                provider.name
            ),
        )
        .to_string()),
    }
}

/// GitHub Copilot CLI：官方卡跑 `copilot`，卡上设了模型就加 `--model`；绑了授权中心账号时
/// 经 `COPILOT_GITHUB_TOKEN` 只给这一个进程。自带模型卡要先添加进 providers.json，启动时
/// `--model=<卡片 id>/<第一个模型>`，并清掉所有 `COPILOT_PROVIDER_*`（有它们 Copilot 就不读
/// providers.json）。
fn copilot_terminal_launch(
    provider: &crate::provider::Provider,
    account: Option<(String, String)>,
) -> Result<ProviderLaunch, String> {
    let launch = crate::services::provider::copilot_direct::terminal_launch(provider)
        .map_err(|e| e.to_string())?;
    copilot_launch_command(&provider.name, launch, account)
}

fn copilot_launch_command(
    provider_name: &str,
    launch: crate::copilot_config::CopilotLaunch,
    account: Option<(String, String)>,
) -> Result<ProviderLaunch, String> {
    let mut env = launch.env;
    if !launch.byok {
        if let Some((token, domain)) = account {
            env.push(("COPILOT_GITHUB_TOKEN", Some(token)));
            env.push(("COPILOT_GH_HOST", Some(domain)));
        }
    }
    // npm 装的是 copilot.cmd：不加 `call` 跑完不回到启动 .bat，后面删临时文件（含 Key、
    // GitHub 令牌的 .bat 本身）的几行就不执行了。`call` 会把 `%` 再展开一遍、把引号里的 `^`
    // 加倍，cmd 也不认 `\"`，所以模型名里有这些字符时不在 Windows 上启动。
    // 模型名会拼进启动命令：控制字符（换行）在任何平台都拒绝，保存时已经校验过，这里兜底。
    if launch
        .model
        .as_deref()
        .is_some_and(|model| model.chars().any(char::is_control))
    {
        return Err(format!(
            "「{provider_name}」的模型名里有换行等控制字符，不能用来启动"
        ));
    }
    if cfg!(target_os = "windows")
        && launch
            .model
            .as_deref()
            .is_some_and(|model| model.contains(['"', '%', '^']))
    {
        return Err(format!(
            "「{provider_name}」的模型名里有 Windows 终端传不过去的字符（\" % ^）"
        ));
    }
    let (mut args, mut batch_command, batch_vars) = match launch.model {
        Some(model) => (
            vec![format!("--model={model}")],
            format!("call copilot \"--model=%{COPILOT_MODEL_BATCH_VAR}%\""),
            vec![(COPILOT_MODEL_BATCH_VAR, model)],
        ),
        None => (Vec::new(), "call copilot".to_string(), Vec::new()),
    };
    // 只会是 copilot_config::REASONING_EFFORTS 里的小写单词（已校验），可以直接写进命令。
    if let Some(effort) = launch.reasoning_effort {
        args.push(format!("--reasoning-effort={effort}"));
        batch_command.push_str(&format!(" --reasoning-effort={effort}"));
    }
    Ok(ProviderLaunch {
        title: if launch.byok {
            "Starting GitHub Copilot CLI with this provider's model"
        } else {
            "Starting GitHub Copilot CLI with your GitHub sign-in"
        },
        detail: None,
        program: "copilot",
        args,
        batch_command,
        env,
        batch_vars,
        cleanup: Vec::new(),
    })
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const COPILOT_MODEL_BATCH_VAR: &str = "CC_SWITCH_COPILOT_MODEL";

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const AGY_DIR_BATCH_VAR: &str = "CC_SWITCH_AGY_DIR";
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const AGY_MODEL_BATCH_VAR: &str = "CC_SWITCH_AGY_MODEL";

/// macOS / Linux 的启动脚本里，环境变量先以这个前缀的名字读进来，再由 `env` 改名交给
/// 程序：用户 shell 的 rc 文件在中间执行，直接用原名会被 rc 里的 export 覆盖。
#[cfg_attr(windows, allow(dead_code))]
const LAUNCH_ENV_PREFIX: &str = "CC_SWITCH_LAUNCH_ENV_";

/// 同一进程里连开几个终端也不会撞名。
fn unique_temp_path(stem: &str, ext: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "{stem}_{}_{nanos:x}{}.{ext}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// 根据平台打开终端运行 `launch`。
fn launch_provider_terminal(launch: &ProviderLaunch, cwd: Option<&Path>) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let (script_file, env_file) = write_unix_launcher(launch, cwd)?;
        #[cfg(target_os = "macos")]
        let result = launch_macos_terminal(&script_file);
        #[cfg(target_os = "linux")]
        let result = launch_linux_terminal(&script_file);
        if result.is_err() {
            for file in launch.cleanup.iter().chain([&script_file, &env_file]) {
                let _ = std::fs::remove_file(file);
            }
        }
        result
    }

    #[cfg(target_os = "windows")]
    {
        let result = launch_windows_terminal(launch, cwd);
        if result.is_err() {
            for file in &launch.cleanup {
                let _ = std::fs::remove_file(file);
            }
        }
        result
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (launch, cwd);
        Err("不支持的操作系统".to_string())
    }
}

/// shell 里的一个词：只含安全字符时原样写，否则加单引号。
#[cfg_attr(windows, allow(dead_code))]
fn shell_word(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '='));
    if plain {
        value.to_string()
    } else {
        shell_single_quote(value)
    }
}

/// 交给用户 shell 执行的启动命令。有环境变量时经 `env` 设置或清除，值从启动脚本读进来的
/// `CC_SWITCH_LAUNCH_ENV_<n>` 取，不出现在命令行里。
#[cfg_attr(windows, allow(dead_code))]
#[cfg(test)]
fn unix_launch_command(launch: &ProviderLaunch) -> String {
    unix_launch_command_in(launch, "/bin/sh")
}

/// 同上，参数按用户 shell 的规则加引号：这条命令由用户 shell 解析（见
/// `build_provider_command_line`），附加参数又是用户随手填的文本。
#[cfg_attr(windows, allow(dead_code))]
fn unix_launch_command_in(launch: &ProviderLaunch, shell: &str) -> String {
    let mut words = Vec::new();
    if !launch.env.is_empty() {
        words.push("env".to_string());
        for (index, (name, value)) in launch.env.iter().enumerate() {
            // 中转用的变量也不留给程序。
            words.push(format!("-u {LAUNCH_ENV_PREFIX}{index}"));
            if value.is_none() {
                words.push(format!("-u {name}"));
            }
        }
        for (index, (name, value)) in launch.env.iter().enumerate() {
            if value.is_some() {
                words.push(format!("{name}=\"${LAUNCH_ENV_PREFIX}{index}\""));
            }
        }
    }
    words.push(launch.program.to_string());
    words.extend(launch.args.iter().map(|arg| user_shell_word(shell, arg)));
    words.join(" ")
}

/// 0600 环境变量文件的内容，启动脚本 `.` 进来后立即删除。
#[cfg_attr(windows, allow(dead_code))]
fn unix_launch_env_file(launch: &ProviderLaunch) -> String {
    launch
        .env
        .iter()
        .enumerate()
        .filter_map(|(index, (_, value))| {
            value
                .as_ref()
                .map(|value| format!("{LAUNCH_ENV_PREFIX}{index}={}\n", shell_single_quote(value)))
        })
        .collect()
}

/// 启动脚本：读进环境变量文件并删掉它，跑启动命令，删掉临时文件，最后换成用户的交互 shell。
#[cfg_attr(windows, allow(dead_code))]
fn build_unix_launcher_script(
    launch: &ProviderLaunch,
    shell: &str,
    cwd: Option<&Path>,
    script_file: &Path,
    env_file: Option<&Path>,
) -> String {
    let mut temp_files: Vec<String> = launch
        .cleanup
        .iter()
        .map(PathBuf::as_path)
        .chain(env_file)
        .map(|file| shell_single_quote(&file.to_string_lossy()))
        .collect();
    temp_files.push(shell_single_quote(&script_file.to_string_lossy()));
    let remove_temp_files = format!("rm -f -- {}", temp_files.join(" "));

    let mut script = String::from("#!/usr/bin/env sh\n");
    script.push_str(&format!(
        "trap {} EXIT\n",
        shell_single_quote(&remove_temp_files)
    ));
    if let Some(env_file) = env_file {
        let quoted = shell_single_quote(&env_file.to_string_lossy());
        script.push_str(&format!("set -a\n. {quoted}\nset +a\nrm -f -- {quoted}\n"));
    }
    script.push_str(&format!("echo {}\n", shell_single_quote(launch.title)));
    if let Some(detail) = &launch.detail {
        script.push_str(&format!(
            "echo {}\n",
            shell_single_quote(&detail.to_string_lossy())
        ));
    }
    script.push_str(&build_provider_command_line(
        shell,
        &unix_launch_command_in(launch, shell),
        cwd,
    ));
    script.push('\n');
    // exec 不会触发 EXIT trap，所以在换成交互 shell 之前自己删。
    script.push_str(&remove_temp_files);
    script.push('\n');
    let launch_vars: Vec<String> = (0..launch.env.len())
        .map(|index| format!("{LAUNCH_ENV_PREFIX}{index}"))
        .collect();
    if !launch_vars.is_empty() {
        script.push_str(&format!("unset {}\n", launch_vars.join(" ")));
    }
    script.push_str(&build_final_shell_cd_command(shell, cwd));
    script.push_str(&build_exec_line(shell, cwd));
    script.push('\n');
    script
}

/// 写好启动脚本（和需要时的 0600 环境变量文件），返回两者路径。
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn write_unix_launcher(
    launch: &ProviderLaunch,
    cwd: Option<&Path>,
) -> Result<(PathBuf, PathBuf), String> {
    use std::os::unix::fs::PermissionsExt;

    let shell = get_user_shell();
    let script_file = unique_temp_path("cc_switch_launcher", "sh");
    let env_file = unique_temp_path("cc_switch_launch_env", "sh");
    let env_content = unix_launch_env_file(launch);
    let has_env = !env_content.is_empty();
    if has_env {
        crate::config::atomic_write_private(&env_file, env_content.as_bytes())
            .map_err(|e| format!("写入启动环境失败: {e}"))?;
    }
    let script_content = build_unix_launcher_script(
        launch,
        &shell,
        cwd,
        &script_file,
        has_env.then_some(env_file.as_path()),
    );
    std::fs::write(&script_file, &script_content).map_err(|e| format!("写入启动脚本失败: {e}"))?;
    std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("设置脚本权限失败: {e}"))?;
    Ok((script_file, env_file))
}

/// 写入 claude 配置文件（含凭据，0600）
///
/// 这个文件叠在用户级 `settings.json` 上：`env` 按键合并、顶层按键覆盖。所以：
/// - `modelPicker` 一定要写：它按层级整个胜出，不写的话路由 / 聚合模式的菜单会漏进这个
///   终端，选了就把别家的模型发给这张卡；
/// - 切换会清掉、卡上又没有的键（`drops`）写成空值挡住：Claude Code 把空串当没设（2.1.314
///   实测：代理的占位凭据不再随请求发出，没配的档位回到内置模型）。
fn write_claude_config(
    config_file: &std::path::Path,
    env_vars: &[(String, String)],
    provider_name: &str,
    drops: &crate::services::provider::SwitchDrops,
) -> Result<(), String> {
    use serde_json::Value;
    let mut config_obj = serde_json::Map::new();
    let mut env_obj = serde_json::Map::new();

    for (key, value) in env_vars {
        env_obj.insert(key.clone(), Value::String(value.clone()));
    }

    let model_picker = crate::live::project::claude::terminal_model_picker(&env_obj, provider_name);
    for key in drops.env.keys() {
        env_obj
            .entry(key.clone())
            .or_insert_with(|| Value::String(String::new()));
    }
    for (key, value) in &drops.top {
        let empty = match value {
            Value::String(_) => Value::String(String::new()),
            Value::Object(_) => Value::Object(serde_json::Map::new()),
            Value::Array(_) => Value::Array(Vec::new()),
            _ => continue,
        };
        config_obj.insert(key.clone(), empty);
    }
    config_obj.insert("env".to_string(), Value::Object(env_obj));
    config_obj.insert("modelPicker".to_string(), model_picker);

    let config_json =
        serde_json::to_string_pretty(&config_obj).map_err(|e| format!("序列化配置失败: {e}"))?;

    crate::config::atomic_write_private(config_file, config_json.as_bytes())
        .map_err(|e| format!("写入配置文件失败: {e}"))
}

/// macOS: 根据用户首选终端启动
#[cfg(target_os = "macos")]
fn launch_macos_terminal(script_file: &std::path::Path) -> Result<(), String> {
    let preferred = crate::settings::get_preferred_terminal();
    let terminal = preferred.as_deref().unwrap_or("terminal");
    let script_file = script_file.to_path_buf();

    // Try the preferred terminal first, fall back to Terminal.app if it fails
    // Note: Kitty doesn't need the -e flag, others do
    let result = match terminal {
        "iterm2" => launch_macos_iterm2(&script_file),
        "warp" => launch_macos_warp(&script_file),
        "alacritty" => launch_macos_open_app("Alacritty", &script_file, true),
        "kitty" => launch_macos_open_app("kitty", &script_file, false),
        "ghostty" => launch_macos_ghostty(&script_file),
        "otty" => launch_macos_otty(&script_file),
        "wezterm" => launch_macos_open_app("WezTerm", &script_file, true),
        "kaku" => launch_macos_open_app("Kaku", &script_file, true),
        _ => launch_macos_terminal_app(&script_file),
    };

    // If preferred terminal fails and it's not the default, try Terminal.app as fallback
    if result.is_err() && terminal != "terminal" {
        log::warn!(
            "首选终端 {} 启动失败，回退到 Terminal.app: {:?}",
            terminal,
            result.as_ref().err()
        );
        return launch_macos_terminal_app(&script_file);
    }

    result
}

/// Escape a value as an AppleScript string literal.
#[cfg(target_os = "macos")]
fn applescript_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build the launcher command literal used by AppleScript.
#[cfg(target_os = "macos")]
fn applescript_launcher_command(script_file: &std::path::Path) -> String {
    applescript_string_literal(&format!(
        "sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    ))
}

/// Build a launcher command that replaces the terminal-created shell session.
#[cfg(target_os = "macos")]
fn applescript_exec_launcher_command(script_file: &std::path::Path) -> String {
    applescript_string_literal(&format!(
        "exec sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    ))
}

/// macOS: Terminal.app AppleScript.
/// A cold `activate` creates a default empty window before `do script` opens the command session.
/// Use `launch` for cold starts so `do script` can create the only new session without reusing restored windows.
#[cfg(target_os = "macos")]
fn build_macos_terminal_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "Terminal" is running
tell application "Terminal"
    if was_running then
        activate
        do script launcher_script
    else
        launch
        do script launcher_script
        activate
    end if
end tell"#,
        launcher = applescript_exec_launcher_command(script_file)
    )
}

/// Run AppleScript through `osascript -e` with shared error handling.
#[cfg(target_os = "macos")]
fn run_terminal_osascript(applescript: &str, terminal_label: &str) -> Result<(), String> {
    use std::process::Command;

    let output = Command::new("osascript")
        .arg("-e")
        .arg(applescript)
        .output()
        .map_err(|e| format!("执行 osascript 失败: {e}"))?;

    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "{terminal_label} 执行失败 (exit code: {:?}): {}",
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}

/// macOS: Terminal.app
#[cfg(target_os = "macos")]
fn launch_macos_terminal_app(script_file: &std::path::Path) -> Result<(), String> {
    run_terminal_osascript(
        &build_macos_terminal_applescript(script_file),
        "Terminal.app",
    )
}

#[cfg(target_os = "macos")]
fn launch_macos_otty(script_file: &std::path::Path) -> Result<(), String> {
    use std::process::Command;

    let otty_cli = find_macos_otty_cli().ok_or_else(|| {
        "未找到 Otty CLI。请将 Otty 安装到 /Applications 或 ~/Applications。".to_string()
    })?;

    let command = build_macos_dash_c_command(script_file);
    let tab_result = Command::new(&otty_cli)
        .args(["tab", "new", "--window", "0", "--command", &command])
        .output()
        .map_err(|e| format!("启动 Otty CLI 失败: {e}"))?;

    if tab_result.status.success() {
        return Ok(());
    }

    log::debug!(
        "Otty 新建 Tab 失败，改为新建窗口: {}",
        decode_command_output(&tab_result.stderr)
    );

    let window_result = Command::new(&otty_cli)
        .args(["open", "--command", &command])
        .output()
        .map_err(|e| format!("启动 Otty CLI 失败: {e}"))?;

    if window_result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Otty 新建窗口失败 (exit code: {:?}): {}",
            window_result.status.code(),
            decode_command_output(&window_result.stderr)
        ))
    }
}

#[cfg(target_os = "macos")]
fn find_macos_otty_cli() -> Option<std::path::PathBuf> {
    macos_otty_cli_candidates()
        .into_iter()
        .find(|path| path.is_file() && is_executable_file(path))
}

#[cfg(target_os = "macos")]
fn macos_otty_cli_candidates() -> Vec<std::path::PathBuf> {
    let mut candidates = vec![std::path::PathBuf::from(
        "/Applications/Otty.app/Contents/MacOS/otty-cli",
    )];

    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(
            std::path::PathBuf::from(home).join("Applications/Otty.app/Contents/MacOS/otty-cli"),
        );
    }

    candidates.push(std::path::PathBuf::from("/usr/local/bin/otty"));
    candidates.push(std::path::PathBuf::from("/opt/homebrew/bin/otty"));

    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            candidates.push(directory.join("otty"));
            candidates.push(directory.join("otty-cli"));
        }
    }

    candidates
}

/// macOS: iTerm2
#[cfg(target_os = "macos")]
fn build_macos_iterm2_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_script to {launcher}
set was_running to application "iTerm" is running
tell application "iTerm"
    if was_running then
        activate
        if (count of windows) = 0 then
            create window with default profile
        else
            tell current window
                create tab with default profile
            end tell
        end if
    else
        activate
        set waited to 0
        repeat while (count of windows) = 0
            delay 0.1
            set waited to waited + 1
            if waited >= 30 then exit repeat
        end repeat
        if (count of windows) = 0 then
            create window with default profile
        end if
    end if
    tell current session of current window
        write text launcher_script
    end tell
end tell"#,
        launcher = applescript_exec_launcher_command(script_file)
    )
}

/// macOS: iTerm2
#[cfg(target_os = "macos")]
fn launch_macos_iterm2(script_file: &std::path::Path) -> Result<(), String> {
    run_terminal_osascript(&build_macos_iterm2_applescript(script_file), "iTerm2")
}

/// Keep the launcher path inside a `sh -c` string.
/// A bare `.sh` passed through `open --args` may also be opened as a document.
#[cfg(target_os = "macos")]
fn build_macos_dash_c_command(script_file: &std::path::Path) -> String {
    format!(
        "exec sh {}",
        shell_single_quote(&script_file.to_string_lossy())
    )
}

/// macOS: Ghostty.
/// Warm starts use AppleScript to create one command window.
/// Cold starts use `initial-command` so the first default surface runs the launcher.
/// Do not use `initial-window=false` plus `new window`: cold launch can still create the default window first.
#[cfg(target_os = "macos")]
fn build_macos_ghostty_applescript(script_file: &std::path::Path) -> String {
    format!(
        r#"set launcher_command to {launcher}
set was_running to application "Ghostty" is running
if was_running then
    tell application "Ghostty"
        new window with configuration {{command:launcher_command}}
    end tell
else
    do shell script "open -na Ghostty --args --quit-after-last-window-closed=true " & quoted form of ("--initial-command=" & launcher_command)
end if
"#,
        launcher = applescript_launcher_command(script_file)
    )
}

/// macOS: Ghostty
#[cfg(target_os = "macos")]
fn launch_macos_ghostty(script_file: &std::path::Path) -> Result<(), String> {
    match run_terminal_osascript(&build_macos_ghostty_applescript(script_file), "Ghostty") {
        Ok(()) => Ok(()),
        Err(applescript_error) => {
            log::warn!(
                "Ghostty AppleScript launch failed, falling back to open -na: {applescript_error}"
            );
            launch_macos_open_app("Ghostty", script_file, true)
        }
    }
}

/// macOS: 使用 open -na 启动支持 --args 参数的终端（Alacritty/Kitty/WezTerm/Kaku）
#[cfg(target_os = "macos")]
fn launch_macos_open_app(
    app_name: &str,
    script_file: &std::path::Path,
    use_e_flag: bool,
) -> Result<(), String> {
    use std::process::Command;

    let mut cmd = Command::new("open");
    cmd.arg("-na").arg(app_name).arg("--args");

    if use_e_flag {
        cmd.arg("-e");
    }
    // Keep the script path inside `sh -c`; a trailing bare `.sh` can be opened as a document.
    cmd.arg("sh")
        .arg("-c")
        .arg(build_macos_dash_c_command(script_file));

    let output = cmd
        .output()
        .map_err(|e| format!("启动 {app_name} 失败: {e}"))?;

    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "{} 启动失败 (exit code: {:?}): {}",
            app_name,
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn launch_macos_warp(script_file: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let mut cmd = Command::new("open");
    cmd.arg("-a").arg("Warp");

    // Warp URI scheme cannot work well with script_file, because:
    //
    // 1. script_file's name ends up with .sh, so Warp would open the file rather than execute it
    // 2. script_file has no execution permission, so we need to add one more indirection
    let mut second_script_file = tempfile::Builder::new()
        .disable_cleanup(true)
        .permissions(std::fs::Permissions::from_mode(0o755))
        .tempfile()
        .map_err(|e| format!("Failed to create temporary script file: {e}"))?;

    writeln!(
        &mut second_script_file,
        r#"#!/usr/bin/env sh

        rm -- "$0"

        exec sh {quoted_script}
        "#,
        quoted_script = shell_single_quote(&script_file.to_string_lossy()),
    )
    .map_err(|e| format!("Failed to write to temporary script file for Warp: {e}"))?;

    let mut warp_url = url::Url::parse("warp://action/new_tab").unwrap();
    warp_url
        .query_pairs_mut()
        .append_pair("path", &second_script_file.path().to_string_lossy());
    let warp_url = warp_url.to_string();
    cmd.arg(warp_url);

    let output = cmd.output().map_err(|e| format!("启动 Warp 失败: {e}"))?;
    if !output.status.success() {
        let stderr = decode_command_output(&output.stderr);
        return Err(format!(
            "Warp 启动失败 (exit code: {:?}): {}",
            output.status.code(),
            stderr
        ));
    }

    Ok(())
}

/// Linux: 根据用户首选终端启动
#[cfg(target_os = "linux")]
fn launch_linux_terminal(script_file: &std::path::Path) -> Result<(), String> {
    use std::process::Command;

    let preferred = crate::settings::get_preferred_terminal();

    // Default terminal list with their arguments
    let default_terminals = [
        ("gnome-terminal", vec!["--"]),
        ("konsole", vec!["-e"]),
        ("xfce4-terminal", vec!["-e"]),
        ("mate-terminal", vec!["--"]),
        ("lxterminal", vec!["-e"]),
        ("alacritty", vec!["-e"]),
        ("kitty", vec!["-e"]),
        ("ghostty", vec!["-e"]),
    ];

    // Build terminal list: preferred terminal first (if specified), then defaults
    let terminals_to_try: Vec<(&str, Vec<&str>)> = if let Some(ref pref) = preferred {
        // Find the preferred terminal's args from default list
        let pref_args = default_terminals
            .iter()
            .find(|(name, _)| *name == pref.as_str())
            .map(|(_, args)| args.to_vec())
            .unwrap_or_else(|| vec!["-e"]); // Default args for unknown terminals

        let mut list = vec![(pref.as_str(), pref_args)];
        // Add remaining terminals as fallbacks
        for (name, args) in &default_terminals {
            if *name != pref.as_str() {
                list.push((*name, args.to_vec()));
            }
        }
        list
    } else {
        default_terminals
            .iter()
            .map(|(name, args)| (*name, args.to_vec()))
            .collect()
    };

    let mut last_error = String::from("未找到可用的终端");

    for (terminal, args) in terminals_to_try {
        // Check if terminal exists in common paths
        let terminal_exists = std::path::Path::new(&format!("/usr/bin/{}", terminal)).exists()
            || std::path::Path::new(&format!("/bin/{}", terminal)).exists()
            || std::path::Path::new(&format!("/usr/local/bin/{}", terminal)).exists()
            || which_command(terminal);

        if terminal_exists {
            let result = Command::new(terminal)
                .args(&args)
                .arg("sh")
                .arg(script_file.to_string_lossy().as_ref())
                .spawn();

            match result {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = format!("执行 {} 失败: {}", terminal, e);
                }
            }
        }
    }

    Err(last_error)
}

/// Check if a command exists using `which`
#[cfg(target_os = "linux")]
fn which_command(cmd: &str) -> bool {
    use std::process::Command;
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Windows: 根据用户首选终端启动
#[cfg(target_os = "windows")]
fn launch_windows_terminal(launch: &ProviderLaunch, cwd: Option<&Path>) -> Result<(), String> {
    let preferred = crate::settings::get_preferred_terminal();
    let terminal = preferred.as_deref().unwrap_or("cmd");

    let bat_file = unique_temp_path("cc_switch_launcher", "bat");
    let (cwd_command, cwd_env) = build_windows_cwd_command(cwd);
    let mut launch_env: Vec<(String, String)> = cwd_env
        .map(|value| (WINDOWS_CWD_ENV.to_string(), value))
        .into_iter()
        .collect();
    let (set_lines, extra_env) = build_windows_launch_env(launch);
    launch_env.extend(extra_env);
    let launch_env: Vec<(&str, &str)> = launch_env
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let cleanup: String = launch
        .cleanup
        .iter()
        .filter_map(|file| file.file_name())
        .map(|name| {
            format!(
                "del \"%~dp0{}\" >nul 2>&1\r\n",
                escape_windows_batch_value(&name.to_string_lossy())
            )
        })
        .collect();

    // setlocal 段里设的变量（含 Key）在程序退出后随 endlocal 撤掉，不留在这个 cmd 会话里。
    // 注册表里开了延迟展开的机器上，值里的 `!名字!` 会被替换：这一段显式关掉。
    let content = format!(
        "@echo off\r\n{cwd_command}setlocal DisableDelayedExpansion\r\n{set_lines}echo {title}\r\n{command}\r\nendlocal\r\n{cleanup}del \"%~f0\" >nul 2>&1\r\n",
        title = escape_windows_batch_value(launch.title),
        command = launch.batch_command,
    );

    std::fs::write(&bat_file, &content).map_err(|e| format!("写入批处理文件失败: {e}"))?;

    let bat_path = bat_file.to_string_lossy();
    let ps_cmd = format!("& '{}'", bat_path);

    // Try the preferred terminal first
    let result = match terminal {
        "powershell" => run_windows_start_command_with_env(
            &[
                windows_powershell_program(&effective_path_string()),
                "-NoExit",
                "-Command",
                &ps_cmd,
            ],
            "PowerShell",
            &launch_env,
        ),
        "wt" => run_windows_start_command_with_env(
            &["wt", "cmd", "/K", &bat_path],
            "Windows Terminal",
            &launch_env,
        ),
        // "cmd" or default
        _ => run_windows_start_command_with_env(&["cmd", "/K", &bat_path], "cmd", &launch_env),
    };

    // If preferred terminal fails and it's not the default, try cmd as fallback
    if result.is_err() && terminal != "cmd" {
        log::warn!(
            "首选终端 {} 启动失败，回退到 cmd: {:?}",
            terminal,
            result.as_ref().err()
        );
        return run_windows_start_command_with_env(&["cmd", "/K", &bat_path], "cmd", &launch_env);
    }

    result
}

/// fish 的单引号里 `\'` 和 `\\` 仍是转义，POSIX 的 `'"'"'` 写法会让引号错位、接出别的命令。
#[cfg_attr(windows, allow(dead_code))]
fn is_fish_shell(shell: &str) -> bool {
    shell.rsplit('/').next().unwrap_or(shell) == "fish"
}

/// 交给用户 shell（而不是启动脚本的 sh）解析的一段文本加引号。
#[cfg_attr(windows, allow(dead_code))]
fn user_shell_quote(shell: &str, value: &str) -> String {
    if is_fish_shell(shell) {
        format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
    } else {
        shell_single_quote(value)
    }
}

/// `shell_word` 的用户 shell 版本：只含安全字符时原样写。
#[cfg_attr(windows, allow(dead_code))]
fn user_shell_word(shell: &str, value: &str) -> String {
    let word = shell_word(value);
    // zsh 把开头的 `=名字` 展开成命令路径（`=sh` → `/bin/sh`），找不到还会直接报错。
    if word == value && !value.starts_with('=') {
        word
    } else {
        user_shell_quote(shell, value)
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_windows_unc_path(path: &str) -> bool {
    path.starts_with(r"\\")
}

/// .bat 里 `setlocal` 段的 `set` 行，以及要经启动进程环境传入的变量。纯 ASCII 的值直接
/// 写进文件（`%` 双写）；含非 ASCII 的按 #8034 的办法经环境变量转一手，免得按代码页乱码。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn build_windows_launch_env(launch: &ProviderLaunch) -> (String, Vec<(String, String)>) {
    let mut lines = String::new();
    let mut carried = Vec::new();
    let entries = launch
        .env
        .iter()
        .map(|(name, value)| (*name, value.clone()))
        .chain(
            launch
                .batch_vars
                .iter()
                .map(|(name, value)| (*name, Some(value.clone()))),
        );
    for (index, (name, value)) in entries.enumerate() {
        match value {
            None => lines.push_str(&format!("set \"{name}=\"\r\n")),
            Some(value) if value.is_ascii() => {
                lines.push_str(&format!("set \"{name}={}\"\r\n", value.replace('%', "%%")))
            }
            Some(value) => {
                let carrier = format!("CC_SWITCH_INTERNAL_ENV_{index}");
                lines.push_str(&format!(
                    "set \"{name}=%{carrier}%\"\r\nset \"{carrier}=\"\r\n"
                ));
                carried.push((carrier, value));
            }
        }
    }
    (lines, carried)
}

/// 非 ASCII 的项目目录经这个环境变量传给 Claude 启动 .bat。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const WINDOWS_CWD_ENV: &str = "CC_SWITCH_INTERNAL_CWD";

/// 生成切到项目目录的 .bat 行，第二项是需要给启动进程设置的 `WINDOWS_CWD_ENV` 值。
///
/// cmd 按控制台代码页解码 .bat 字节，非 ASCII 路径以 UTF-8 写进文件会乱码（#8034）；
/// 环境变量在 Windows 内部保持 Unicode，所以这类路径改由环境变量传入。纯 ASCII
/// 路径照旧写进文件，不依赖终端继承环境变量（旧版 Windows Terminal 不继承）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn build_windows_cwd_command_str(path: &str) -> (String, Option<String>) {
    let (target, env_value, cleanup) = if path.is_ascii() {
        (escape_windows_batch_value(path), None, String::new())
    } else {
        (
            format!("%{WINDOWS_CWD_ENV}%"),
            Some(path.to_string()),
            format!("set \"{WINDOWS_CWD_ENV}=\"\r\n"),
        )
    };

    let command = if is_windows_unc_path(path) {
        // `cmd.exe` cannot make a UNC path current via `cd`; `pushd` maps it first.
        format!("pushd \"{target}\" || exit /b 1\r\n{cleanup}")
    } else {
        format!("cd /d \"{target}\" || exit /b 1\r\n{cleanup}")
    };
    (command, env_value)
}

#[cfg(target_os = "windows")]
fn build_windows_cwd_command(cwd: Option<&Path>) -> (String, Option<String>) {
    cwd.map(|dir| build_windows_cwd_command_str(&dir.to_string_lossy()))
        .unwrap_or_default()
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn escape_windows_batch_value(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('(', "^(")
        .replace(')', "^)")
}
/// Windows: Run a start command with common error handling
#[cfg(target_os = "windows")]
fn run_windows_start_command(args: &[&str], terminal_name: &str) -> Result<(), String> {
    run_windows_start_command_with_env(args, terminal_name, &[])
}

/// 组装 `cmd /C start ...`。终端里跑的 `claude` 等命令按这里的 PATH 查找：CC Switch
/// 被 MSI 自动重启后只剩机器级 PATH（#6061），或是在装 CLI 之前就已启动，进程 PATH
/// 都会缺用户级条目（#3877、#8446）。和检测、生命周期命令（#8173）一样用补全后的 PATH，
/// 只影响启动的终端，不改 CC Switch 自身的环境；Windows Terminal 从命令行开新标签时
/// 也沿用调用方的环境。
#[cfg(target_os = "windows")]
fn windows_start_command(
    args: &[&str],
    envs: &[(&str, &str)],
    effective_path: &str,
) -> std::process::Command {
    let mut command = std::process::Command::new("cmd");
    command
        .arg("/C")
        .arg("start")
        .args(args)
        .env("PATH", effective_path)
        .envs(envs.iter().copied())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// 同 `run_windows_start_command`，额外给启动的终端设置环境变量（`start` 会继承）。
///
/// stdio 指向 NUL、只等 `cmd` 的退出码，不能用 `output()` 捕获：`start` 打开的终端会
/// 继承这里的 stdout/stderr 管道句柄，管道要等那个窗口和里面的进程全部退出才到 EOF，
/// 调用就一直挂到用户关掉终端，界面也一直转圈（#6712）。启动失败仍能从退出码看出来。
#[cfg(target_os = "windows")]
fn run_windows_start_command_with_env(
    args: &[&str],
    terminal_name: &str,
    envs: &[(&str, &str)],
) -> Result<(), String> {
    use std::process::Stdio;

    let status = windows_start_command(args, envs, &effective_path_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("启动 {} 失败: {e}", terminal_name))?;

    if !status.success() {
        return Err(format!(
            "{} 启动失败 (exit code: {:?})",
            terminal_name,
            status.code()
        ));
    }

    Ok(())
}

/// 首选终端为 PowerShell 时启动哪个程序。`powershell` 永远是系统自带的 Windows
/// PowerShell 5.1；装了 PowerShell 7+ 的用户默认用的是 `pwsh`，所以补全后的 PATH 里
/// 有 `pwsh.exe` 就用它，没有才退回 `powershell`。Microsoft Store 版的 `pwsh.exe` 是
/// App Execution Alias（跟随不了的重解析点），`Path::exists` 会判成不存在，这里用
/// `symlink_metadata`。
#[cfg(target_os = "windows")]
fn windows_powershell_program(path: &str) -> &'static str {
    let has_pwsh = std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| std::fs::symlink_metadata(dir.join("pwsh.exe")).is_ok());
    if has_pwsh {
        "pwsh"
    } else {
        "powershell"
    }
}

/// 打开用户首选终端并在其中执行一段可信命令脚本。脚本尾部 `read -r` / `pause`
/// 是刻意设计的——让命令退出后窗口不要瞬间关闭，用户才看得到 `command
/// not found` / `ModuleNotFoundError` 这类诊断信息。
///
/// **Security**：`command_line` 会被原样拼进 shell/batch 脚本，调用方必须
/// 保证它是可信字符串（当前只由后端硬编码调用）。
pub(crate) fn launch_terminal_running(command_line: &str, label: &str) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let pid = std::process::id();

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let (script_file, script_content) = {
        let file = temp_dir.join(format!("cc_switch_{}_{}.sh", label, pid));
        let content = format!(
            r#"#!/usr/bin/env sh
trap 'rm -f "{script_path}"' EXIT
echo "[cc-switch] Starting: {label}"
echo ""
{cmd}
echo ""
echo "[cc-switch] Command exited. Press Enter to close."
read -r _
"#,
            script_path = file.display(),
            label = label,
            cmd = command_line,
        );
        (file, content)
    };

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(&script_file, &script_content)
            .map_err(|e| format!("写入启动脚本失败: {e}"))?;
        std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置脚本权限失败: {e}"))?;

        let preferred = crate::settings::get_preferred_terminal();
        let terminal = preferred.as_deref().unwrap_or("terminal");

        let result = match terminal {
            "iterm2" => launch_macos_iterm2(&script_file),
            "warp" => launch_macos_warp(&script_file),
            "alacritty" => launch_macos_open_app("Alacritty", &script_file, true),
            "kitty" => launch_macos_open_app("kitty", &script_file, false),
            "ghostty" => launch_macos_ghostty(&script_file),
            "otty" => launch_macos_otty(&script_file),
            "wezterm" => launch_macos_open_app("WezTerm", &script_file, true),
            "kaku" => launch_macos_open_app("Kaku", &script_file, true),
            _ => launch_macos_terminal_app(&script_file),
        };

        if result.is_err() && terminal != "terminal" {
            log::warn!(
                "首选终端 {} 启动失败，回退到 Terminal.app: {:?}",
                terminal,
                result.as_ref().err()
            );
            return launch_macos_terminal_app(&script_file);
        }
        result
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        std::fs::write(&script_file, &script_content)
            .map_err(|e| format!("写入启动脚本失败: {e}"))?;
        std::fs::set_permissions(&script_file, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置脚本权限失败: {e}"))?;

        let preferred = crate::settings::get_preferred_terminal();
        let default_terminals = [
            ("gnome-terminal", vec!["--"]),
            ("konsole", vec!["-e"]),
            ("xfce4-terminal", vec!["-e"]),
            ("mate-terminal", vec!["--"]),
            ("lxterminal", vec!["-e"]),
            ("alacritty", vec!["-e"]),
            ("kitty", vec!["-e"]),
            ("ghostty", vec!["-e"]),
        ];

        let terminals_to_try: Vec<(&str, Vec<&str>)> = if let Some(ref pref) = preferred {
            let pref_args = default_terminals
                .iter()
                .find(|(name, _)| *name == pref.as_str())
                .map(|(_, args)| args.to_vec())
                .unwrap_or_else(|| vec!["-e"]);
            let mut list = vec![(pref.as_str(), pref_args)];
            for (name, args) in &default_terminals {
                if *name != pref.as_str() {
                    list.push((*name, args.to_vec()));
                }
            }
            list
        } else {
            default_terminals
                .iter()
                .map(|(name, args)| (*name, args.to_vec()))
                .collect()
        };

        let mut last_error = String::from("未找到可用的终端");

        for (terminal, args) in terminals_to_try {
            let terminal_exists = which_command(terminal)
                || ["/usr/bin", "/bin", "/usr/local/bin"]
                    .iter()
                    .any(|dir| std::path::Path::new(&format!("{}/{}", dir, terminal)).exists());

            if terminal_exists {
                let spawn_result = Command::new(terminal)
                    .args(&args)
                    .arg("sh")
                    .arg(script_file.to_string_lossy().as_ref())
                    .spawn();
                match spawn_result {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        last_error = format!("执行 {} 失败: {}", terminal, e);
                    }
                }
            }
        }

        let _ = std::fs::remove_file(&script_file);
        Err(last_error)
    }

    #[cfg(target_os = "windows")]
    {
        let preferred = crate::settings::get_preferred_terminal();
        let terminal = preferred.as_deref().unwrap_or("cmd");

        let bat_file = temp_dir.join(format!("cc_switch_{}_{}.bat", label, pid));
        let content = format!(
            "@echo off\r\necho [cc-switch] Starting: {label}\r\necho.\r\n{cmd}\r\necho.\r\necho [cc-switch] Command exited. Press any key to close.\r\npause >nul\r\ndel \"%~f0\" >nul 2>&1\r\n",
            label = label,
            cmd = command_line,
        );
        std::fs::write(&bat_file, &content).map_err(|e| format!("写入批处理文件失败: {e}"))?;

        let bat_path = bat_file.to_string_lossy();
        let ps_cmd = format!("& '{}'", bat_path);

        let result = match terminal {
            "powershell" => run_windows_start_command(
                &[
                    windows_powershell_program(&effective_path_string()),
                    "-NoExit",
                    "-Command",
                    &ps_cmd,
                ],
                "PowerShell",
            ),
            "wt" => run_windows_start_command(&["wt", "cmd", "/K", &bat_path], "Windows Terminal"),
            _ => run_windows_start_command(&["cmd", "/K", &bat_path], "cmd"),
        };

        let final_result = if result.is_err() && terminal != "cmd" {
            log::warn!(
                "首选终端 {} 启动失败，回退到 cmd: {:?}",
                terminal,
                result.as_ref().err()
            );
            run_windows_start_command(&["cmd", "/K", &bat_path], "cmd")
        } else {
            result
        };

        // The .bat self-deletes (`del "%~f0"`) after it runs, but that only
        // fires if *some* terminal actually launched it. If every attempt
        // failed, sweep the temp file ourselves to avoid pollution.
        if final_result.is_err() {
            let _ = std::fs::remove_file(&bat_file);
        }
        final_result
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (temp_dir, pid, command_line, label);
        Err("不支持的操作系统".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "windows")]
    use crate::commands::misc::{decode_command_output, merge_path_segments_win};

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_powershell_program_prefers_pwsh_on_path() {
        let root = tempfile::tempdir().unwrap();
        let empty = root.path().join("empty");
        let pwsh_dir = root.path().join("PowerShell 7");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(&pwsh_dir).unwrap();
        std::fs::write(pwsh_dir.join("pwsh.exe"), "").unwrap();

        let without = empty.to_str().unwrap();
        assert_eq!(windows_powershell_program(without), "powershell");
        assert_eq!(windows_powershell_program(""), "powershell");

        let with = format!("{};{}", empty.display(), pwsh_dir.display());
        assert_eq!(windows_powershell_program(&with), "pwsh");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_start_command_passes_restored_path_to_the_terminal() {
        let root = tempfile::tempdir().unwrap();
        let tool_dir = root.path().join("claude 用户 & bin");
        std::fs::create_dir_all(&tool_dir).unwrap();
        std::fs::write(
            tool_dir.join("cc_switch_start_test_tool.cmd"),
            "@echo restored-terminal-path\r\n@exit /b 0\r\n",
        )
        .unwrap();

        // `start /B /WAIT` runs the child in the same hidden console, so the
        // test observes what the launched terminal would resolve.
        let args = [
            "/B",
            "/WAIT",
            "cmd",
            "/D",
            "/C",
            "cc_switch_start_test_tool",
        ];
        let system_path = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let inherited = system_path.to_str().unwrap();
        let effective = merge_path_segments_win(&[inherited, tool_dir.to_str().unwrap()]);
        let parent_path = std::env::var_os("PATH");

        let missing = windows_start_command(&args, &[], inherited)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            !decode_command_output(&missing.stdout).contains("restored-terminal-path"),
            "the tool must be absent on the narrow PATH: {missing:?}"
        );

        let restored = windows_start_command(&args, &[], &effective)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            decode_command_output(&restored.stdout).contains("restored-terminal-path"),
            "{restored:?}"
        );
        assert_eq!(std::env::var_os("PATH"), parent_path);
    }

    #[test]
    fn test_build_provider_command_line_uses_user_shell_environment() {
        let claude = format!(
            "claude --settings {}",
            shell_word("/tmp/claude config.json")
        );
        assert_eq!(
            build_provider_command_line("/bin/zsh", &claude, None),
            "'/bin/zsh' -lic 'claude --settings '\"'\"'/tmp/claude config.json'\"'\"''"
        );
        assert_eq!(
            build_provider_command_line("/bin/bash", &claude, Some(Path::new("/tmp/project"))),
            r#"'/bin/bash' -ic 'cd '"'"'/tmp/project'"'"' && claude --settings '"'"'/tmp/claude config.json'"'"''"#
        );
        assert_eq!(
            build_provider_command_line(
                "/bin/sh",
                &claude,
                Some(Path::new("/tmp/project O'Brien"))
            ),
            r#"'/bin/sh' -c 'cd '"'"'/tmp/project O'"'"'"'"'"'"'"'"'Brien'"'"' && claude --settings '"'"'/tmp/claude config.json'"'"''"#
        );
    }

    fn agy_key_launch() -> ProviderLaunch {
        ProviderLaunch {
            title: "Using a private Antigravity profile for this provider:",
            detail: Some(PathBuf::from("/home/me/.cc-switch/agy/relay")),
            program: "agy",
            args: vec!["--gemini_dir=/home/me/.cc-switch/agy/relay".to_string()],
            batch_command: format!("agy \"--gemini_dir=%{AGY_DIR_BATCH_VAR}%\""),
            env: vec![
                ("GEMINI_API_KEY", Some("k'$(echo no)`x`%PATH%".to_string())),
                ("GOOGLE_GEMINI_BASE_URL", None),
            ],
            batch_vars: vec![(
                AGY_DIR_BATCH_VAR,
                r"C:\Users\张三\.cc-switch\agy\relay".to_string(),
            )],
            cleanup: Vec::new(),
        }
    }

    #[test]
    fn launch_command_keeps_secrets_out_of_the_command_line() {
        let launch = agy_key_launch();
        assert_eq!(
            unix_launch_command(&launch),
            "env -u CC_SWITCH_LAUNCH_ENV_0 -u CC_SWITCH_LAUNCH_ENV_1 -u GOOGLE_GEMINI_BASE_URL GEMINI_API_KEY=\"$CC_SWITCH_LAUNCH_ENV_0\" agy --gemini_dir=/home/me/.cc-switch/agy/relay"
        );
        assert_eq!(
            unix_launch_env_file(&launch),
            "CC_SWITCH_LAUNCH_ENV_0='k'\"'\"'$(echo no)`x`%PATH%'\n"
        );
        let plain = ProviderLaunch {
            env: Vec::new(),
            ..agy_key_launch()
        };
        assert_eq!(
            unix_launch_command(&plain),
            "agy --gemini_dir=/home/me/.cc-switch/agy/relay"
        );
        assert_eq!(unix_launch_env_file(&plain), "");
    }

    #[test]
    fn split_launch_args_groups_quoted_values_without_a_shell() {
        assert_eq!(split_launch_args("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            split_launch_args(
                "--debug --append-system-prompt \"be brief; ok\" --mcp-config 'C:\\a b\\mcp.json'\n-x=\"\""
            )
            .unwrap(),
            vec![
                "--debug",
                "--append-system-prompt",
                "be brief; ok",
                "--mcp-config",
                "C:\\a b\\mcp.json",
                "-x=",
            ]
        );
        // 引号可以接在词中间，引号里也能放另一种引号
        assert_eq!(
            split_launch_args("--name=\"it's\" ''").unwrap(),
            vec!["--name=it's", ""]
        );
        assert!(split_launch_args("--append-system-prompt \"oops").is_err());
        assert!(split_launch_args("\"line\nbreak\"").is_err());
        let too_many = vec!["-v"; USER_ARG_BATCH_VARS.len() + 1].join(" ");
        assert!(split_launch_args(&too_many).is_err());
    }

    #[test]
    fn fish_gets_its_own_quoting_for_user_text() {
        let mut launch = claude_account_terminal_launch(None);
        launch.env.clear();
        launch
            .append_user_args(vec!["x\\'; printf INJECTED; #".into(), "C:\\work\\".into()])
            .unwrap();
        assert_eq!(
            unix_launch_command_in(&launch, "/opt/homebrew/bin/fish"),
            "claude 'x\\\\\\'; printf INJECTED; #' 'C:\\\\work\\\\'"
        );
        assert_eq!(
            unix_launch_command_in(&launch, "/bin/zsh"),
            "claude 'x\\'\"'\"'; printf INJECTED; #' 'C:\\work\\'"
        );
        // .bat 里收尾的反斜杠成对，Unix 参数保持原样
        assert_eq!(launch.batch_vars[1].1, "C:\\work\\\\");
        assert_eq!(launch.args[1], "C:\\work\\");
        assert_eq!(
            build_provider_command_line("/usr/bin/fish", "claude", Some(Path::new("/tmp/a\\'b"))),
            "'/usr/bin/fish' -ic 'cd '\"'\"'/tmp/a\\\\\\'\"'\"'b'\"'\"' && claude'"
        );
        let mut launch = claude_account_terminal_launch(None);
        launch.env.clear();
        launch.append_user_args(vec!["=sh".into()]).unwrap();
        assert_eq!(unix_launch_command_in(&launch, "/bin/zsh"), "claude '=sh'");
    }

    #[test]
    fn user_args_follow_the_generated_args_and_pass_through_batch_vars() {
        let mut launch = claude_account_terminal_launch(None);
        launch.env.clear();
        launch.skip_permissions();
        launch
            .append_user_args(vec!["--append-system-prompt".into(), "a & b | c".into()])
            .unwrap();
        assert_eq!(
            unix_launch_command(&launch),
            "claude --dangerously-skip-permissions --append-system-prompt 'a & b | c'"
        );
        assert_eq!(
            launch.batch_command,
            "claude --dangerously-skip-permissions \"%CC_SWITCH_ARG0%\" \"%CC_SWITCH_ARG1%\""
        );
        assert_eq!(
            launch.batch_vars,
            vec![
                ("CC_SWITCH_ARG0", "--append-system-prompt".to_string()),
                ("CC_SWITCH_ARG1", "a & b | c".to_string()),
            ]
        );
    }

    #[test]
    fn skip_permissions_appends_the_flag_on_every_platform() {
        let mut launch = claude_account_terminal_launch(None);
        launch.env.clear();
        launch.skip_permissions();
        assert_eq!(
            unix_launch_command(&launch),
            "claude --dangerously-skip-permissions"
        );
        assert_eq!(
            launch.batch_command,
            "claude --dangerously-skip-permissions"
        );

        let mut launch = agy_key_launch();
        launch.skip_permissions();
        assert!(unix_launch_command(&launch).ends_with(
            "agy --gemini_dir=/home/me/.cc-switch/agy/relay --dangerously-skip-permissions"
        ));
        assert_eq!(
            launch.batch_command,
            format!("agy \"--gemini_dir=%{AGY_DIR_BATCH_VAR}%\" --dangerously-skip-permissions")
        );
    }

    #[test]
    fn grok_launch_runs_plain_grok_against_the_managed_home() {
        let mut launch = grok_launch("/home/me/.grok".to_string());
        launch.skip_permissions();
        launch
            .append_user_args(vec!["--model".to_string(), "grok-4.6".to_string()])
            .unwrap();
        assert_eq!(launch.program, "grok");
        assert_eq!(launch.args, vec!["--always-approve", "--model", "grok-4.6"]);
        assert_eq!(
            launch.batch_command,
            "call grok --always-approve \"%CC_SWITCH_ARG0%\" \"%CC_SWITCH_ARG1%\""
        );
        assert_eq!(
            launch.env,
            vec![("GROK_HOME", Some("/home/me/.grok".to_string()))]
        );
        assert!(unix_launch_command(&launch).starts_with(
            "env -u CC_SWITCH_LAUNCH_ENV_0 GROK_HOME=\"$CC_SWITCH_LAUNCH_ENV_0\" grok "
        ));
    }

    #[test]
    fn codex_launch_passes_overrides_as_separate_arguments() {
        let mut launch = codex_launch(
            "Kimi",
            vec![
                "model_provider='custom'".to_string(),
                "model_providers.custom={name='K',base_url='https://k.example/v1?a=1&b=2'}"
                    .to_string(),
            ],
            vec![("CC_SWITCH_CODEX_API_KEY", Some("sk-kimi".to_string()))],
            Vec::new(),
        )
        .unwrap();
        launch.skip_permissions();
        assert_eq!(
            launch.args,
            vec![
                "-c",
                "model_provider='custom'",
                "-c",
                "model_providers.custom={name='K',base_url='https://k.example/v1?a=1&b=2'}",
                "--dangerously-bypass-approvals-and-sandbox",
            ]
        );
        assert!(!unix_launch_command(&launch).contains("sk-kimi"));
        assert_eq!(
            launch.batch_command,
            "call codex -c \"%CC_SWITCH_CODEX_C0%\" -c \"%CC_SWITCH_CODEX_C1%\" --dangerously-bypass-approvals-and-sandbox"
        );
        assert_eq!(launch.batch_vars[1].0, "CC_SWITCH_CODEX_C1");

        let plain = codex_launch("Kimi", Vec::new(), Vec::new(), Vec::new()).unwrap();
        assert_eq!(plain.batch_command, "call codex");
        assert!(plain.args.is_empty());
    }

    #[test]
    fn agy_login_launch_picks_the_model_for_this_session_only() {
        let mut row = crate::provider::Provider::with_id(
            "login".into(),
            "Google".into(),
            serde_json::json!({"env":{},"config":{"model":{"name":"Gemini 3.1 Pro (High)"}}}),
            None,
        );
        row.category = Some("official".into());
        let launch = agy_terminal_launch(&row).unwrap();
        assert!(launch.env.is_empty());
        assert_eq!(launch.detail, None);
        assert_eq!(
            unix_launch_command(&launch),
            "agy '--model=Gemini 3.1 Pro (High)'"
        );
        assert_eq!(
            launch.batch_command,
            format!("agy \"--model=%{AGY_MODEL_BATCH_VAR}%\"")
        );
        assert_eq!(
            launch.batch_vars,
            vec![(AGY_MODEL_BATCH_VAR, "Gemini 3.1 Pro (High)".to_string())]
        );
        // 带了 `--model` 的官方卡也没有私有目录，会话恢复不会把它当成 Key 卡（在开终端前就报错）。
        assert!(open_agy_resume_terminal(&row, "abc", None).is_err());

        row.settings_config = serde_json::json!({"env":{},"config":{}});
        let launch = agy_terminal_launch(&row).unwrap();
        assert_eq!(unix_launch_command(&launch), "agy");
        assert_eq!(launch.batch_command, "agy");
    }

    fn copilot_byok_launch(settings: serde_json::Value) -> crate::copilot_config::CopilotLaunch {
        crate::copilot_config::prepare_launch("relay", &settings).unwrap()
    }

    #[test]
    fn copilot_byok_launch_selects_the_card_model_and_clears_byok_variables() {
        let launch = copilot_byok_launch(serde_json::json!({
            "provider": {"baseUrl":"https://relay.test/v1","apiKey":"fixture-key","wireApi":"responses"},
            "models": [{"id":"gpt-5.4"}]
        }));
        // A bound account never applies to bring-your-own-model cards.
        let launch =
            copilot_launch_command("Relay", launch, Some(("gho_x".into(), "github.com".into())))
                .unwrap();
        let command = unix_launch_command(&launch);
        assert!(
            command.ends_with(" copilot --model=relay/gpt-5.4"),
            "{command}"
        );
        assert!(!command.contains("fixture-key"), "{command}");
        assert!(!command.contains("gho_x"), "{command}");
        // providers.json is ignored while any COPILOT_PROVIDER_* is set, so all are cleared.
        for key in crate::copilot_config::BYOK_ENV_KEYS {
            assert!(command.contains(&format!("-u {key}")), "{key}: {command}");
        }
        assert!(command.contains("-u COPILOT_MODEL"), "{command}");
        let env_file = unix_launch_env_file(&launch);
        assert!(!env_file.contains("fixture-key"));
        assert!(!env_file.contains("gho_x"));
        assert_eq!(
            launch.batch_command,
            format!("call copilot \"--model=%{COPILOT_MODEL_BATCH_VAR}%\"")
        );
        assert_eq!(
            launch.batch_vars,
            vec![(COPILOT_MODEL_BATCH_VAR, "relay/gpt-5.4".to_string())]
        );
    }

    #[test]
    fn copilot_launch_passes_the_card_reasoning_effort() {
        let launch = copilot_byok_launch(serde_json::json!({
            "provider": {"baseUrl":"https://relay.test/v1","apiKey":"fixture-key","wireApi":"responses"},
            "models": [{"id":"deepseek-v4-flash"}],
            "reasoningEffort":"none"
        }));
        let mut launch = copilot_launch_command("Relay", launch, None).unwrap();
        assert!(
            unix_launch_command(&launch)
                .ends_with(" copilot --model=relay/deepseek-v4-flash --reasoning-effort=none"),
            "{}",
            unix_launch_command(&launch)
        );
        assert_eq!(
            launch.batch_command,
            format!("call copilot \"--model=%{COPILOT_MODEL_BATCH_VAR}%\" --reasoning-effort=none")
        );

        launch.skip_permissions();
        assert!(
            unix_launch_command(&launch).ends_with(" --reasoning-effort=none --allow-all"),
            "{}",
            unix_launch_command(&launch)
        );
        assert!(launch
            .batch_command
            .ends_with(" --reasoning-effort=none --allow-all"));

        // 「打开终端」里的附加参数接在 `--allow-all` 后面
        launch
            .append_user_args(vec!["--add-dir".into(), "/tmp/a b".into()])
            .unwrap();
        assert!(
            unix_launch_command(&launch)
                .ends_with(" --reasoning-effort=none --allow-all --add-dir '/tmp/a b'"),
            "{}",
            unix_launch_command(&launch)
        );
        assert!(launch.batch_command.ends_with(
            " --reasoning-effort=none --allow-all \"%CC_SWITCH_ARG0%\" \"%CC_SWITCH_ARG1%\""
        ));
    }

    #[test]
    fn copilot_login_launch_uses_the_model_flag_and_bound_account() {
        let mut row = crate::provider::Provider::with_id(
            "login".into(),
            "GitHub".into(),
            serde_json::json!({"env":{"COPILOT_MODEL":"claude-sonnet-4.6"}}),
            None,
        );
        row.category = Some("official".into());
        let launch = copilot_terminal_launch(&row, None).unwrap();
        let command = unix_launch_command(&launch);
        assert!(
            command.ends_with(" copilot --model=claude-sonnet-4.6"),
            "{command}"
        );
        assert!(
            command.contains("-u COPILOT_PROVIDER_BASE_URL"),
            "{command}"
        );
        assert_eq!(
            launch.batch_command,
            format!("call copilot \"--model=%{COPILOT_MODEL_BATCH_VAR}%\"")
        );

        let launch = copilot_terminal_launch(
            &row,
            Some(("gho_fixture".into(), "github.example.com".into())),
        )
        .unwrap();
        assert!(!unix_launch_command(&launch).contains("gho_fixture"));
        let env_file = unix_launch_env_file(&launch);
        assert!(env_file.contains("gho_fixture"));
        assert!(env_file.contains("github.example.com"));

        row.settings_config = serde_json::json!({"env":{}});
        let launch = copilot_terminal_launch(&row, None).unwrap();
        assert!(unix_launch_command(&launch).ends_with(" copilot"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_launcher_script_passes_env_past_rc_files_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        // A user rc that exports its own Gemini variables must not win.
        std::fs::write(
            home.join(".bashrc"),
            "export GEMINI_API_KEY=from-rc\nexport GOOGLE_GEMINI_BASE_URL=https://rc.test\n",
        )
        .unwrap();
        let out = tmp.path().join("out");
        std::fs::write(
            bin.join("agy"),
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"${{GEMINI_API_KEY-unset}}\" \"${{GOOGLE_GEMINI_BASE_URL-unset}}\" \"${{CC_SWITCH_LAUNCH_ENV_0-unset}}\" \"$@\" > '{}'\n",
                out.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(bin.join("agy"), std::fs::Permissions::from_mode(0o755)).unwrap();

        let launch = agy_key_launch();
        let script = tmp.path().join("launcher.sh");
        let env_file = tmp.path().join("env.sh");
        std::fs::write(&env_file, unix_launch_env_file(&launch)).unwrap();
        // Replace the final interactive shell with a no-op so the test returns.
        let content =
            build_unix_launcher_script(&launch, "/bin/bash", None, &script, Some(&env_file))
                .replace("exec '/bin/bash'", "exit 0 #");
        std::fs::write(&script, content).unwrap();
        let status = std::process::Command::new("sh")
            .arg(&script)
            .env("HOME", &home)
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("GEMINI_API_KEY", "from-gui")
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            std::fs::read_to_string(&out).unwrap(),
            "k'$(echo no)`x`%PATH%\nunset\nunset\n--gemini_dir=/home/me/.cc-switch/agy/relay\n"
        );
        assert!(
            !env_file.exists(),
            "env file is removed right after reading"
        );
        assert!(!script.exists());
    }

    #[test]
    fn windows_launch_env_scopes_values_and_carries_non_ascii_paths() {
        let (lines, carried) = build_windows_launch_env(&agy_key_launch());
        assert_eq!(
            lines,
            "set \"GEMINI_API_KEY=k'$(echo no)`x`%%PATH%%\"\r\n\
             set \"GOOGLE_GEMINI_BASE_URL=\"\r\n\
             set \"CC_SWITCH_AGY_DIR=%CC_SWITCH_INTERNAL_ENV_2%\"\r\n\
             set \"CC_SWITCH_INTERNAL_ENV_2=\"\r\n"
        );
        assert_eq!(
            carried,
            vec![(
                "CC_SWITCH_INTERNAL_ENV_2".to_string(),
                r"C:\Users\张三\.cc-switch\agy\relay".to_string()
            )]
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_terminal_config_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let launch = claude_terminal_launch(
            &[("ANTHROPIC_AUTH_TOKEN".into(), "t".into())],
            "p",
            "P",
            &Default::default(),
        )
        .unwrap();
        let file = &launch.cleanup[0];
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            launch.args,
            vec![
                "--settings".to_string(),
                file.to_string_lossy().into_owned()
            ]
        );
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn claude_terminal_config_writes_the_cards_own_model_picker() {
        let read = |env: &[(&str, &str)]| {
            let env: Vec<(String, String)> = env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let launch =
                claude_terminal_launch(&env, "p", "DeepSeek", &Default::default()).unwrap();
            let file = &launch.cleanup[0];
            let value: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
            std::fs::remove_file(file).unwrap();
            value["modelPicker"].clone()
        };

        // 卡上显式配了档位：只列这几档，`model` 写档位别名。
        let picker = read(&[
            ("ANTHROPIC_MODEL", "deepseek-v4-pro"),
            ("ANTHROPIC_DEFAULT_OPUS_MODEL", "deepseek-v4-pro[1M]"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "deepseek-v4-flash"),
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME", "Flash"),
        ]);
        assert_eq!(
            picker,
            serde_json::json!({
                "replaceBuiltInOptions": true,
                "options": [
                    {
                        "model": "opus",
                        "label": "Opus · deepseek-v4-pro",
                        "description": "DeepSeek · deepseek-v4-pro · 1M",
                    },
                    {
                        "model": "haiku",
                        "label": "Haiku · Flash",
                        "description": "DeepSeek · deepseek-v4-flash",
                    },
                ],
            })
        );

        // 一档都没配：空菜单，挡住用户级菜单，显示内置的。
        let picker = read(&[("ANTHROPIC_MODEL", "deepseek-v4-pro")]);
        assert_eq!(
            picker,
            serde_json::json!({ "replaceBuiltInOptions": false, "options": [] })
        );
    }

    #[test]
    fn claude_terminal_config_shadows_what_the_switch_clears() {
        let drops = crate::services::provider::SwitchDrops {
            env: serde_json::json!({
                "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "ccs-claude-kimi--k3[1M]",
            })
            .as_object()
            .unwrap()
            .clone(),
            top: serde_json::json!({
                "model": "ccs-claude-kimi--k3[1M]",
                "modelOverrides": { "a": "b" },
                "modelPicker": { "replaceBuiltInOptions": true, "options": [] },
            })
            .as_object()
            .unwrap()
            .clone(),
        };
        let launch = claude_terminal_launch(
            &[
                ("ANTHROPIC_API_KEY".into(), "sk".into()),
                (
                    "ANTHROPIC_DEFAULT_OPUS_MODEL".into(),
                    "deepseek-v4-pro".into(),
                ),
            ],
            "p",
            "DeepSeek",
            &drops,
        )
        .unwrap();
        let file = &launch.cleanup[0];
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        std::fs::remove_file(file).unwrap();

        // 卡上有的照写，没有的写空值挡住用户级的。
        assert_eq!(value["env"]["ANTHROPIC_API_KEY"], "sk");
        assert_eq!(
            value["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"],
            "deepseek-v4-pro"
        );
        assert_eq!(value["env"]["ANTHROPIC_AUTH_TOKEN"], "");
        assert_eq!(value["model"], "");
        assert_eq!(value["modelOverrides"], serde_json::json!({}));
        // 菜单写卡自己的，不是空对象。
        assert_eq!(value["modelPicker"]["options"][0]["model"], "opus");
    }

    #[test]
    fn claude_terminal_config_without_drops_writes_only_the_card() {
        let launch = claude_terminal_launch(
            &[("ANTHROPIC_MODEL".into(), "deepseek-v4-pro".into())],
            "p",
            "DeepSeek",
            &Default::default(),
        )
        .unwrap();
        let file = &launch.cleanup[0];
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
        std::fs::remove_file(file).unwrap();
        let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["env", "modelPicker"]);
        assert_eq!(
            value["modelPicker"],
            serde_json::json!({ "replaceBuiltInOptions": false, "options": [] })
        );
    }

    #[test]
    fn resolve_launch_cwd_accepts_existing_directory() {
        let resolved =
            resolve_launch_cwd(Some(std::env::temp_dir().to_string_lossy().into_owned()))
                .expect("temp dir should resolve")
                .expect("temp dir should be present");

        assert!(resolved.is_dir());
    }

    #[test]
    fn resolve_launch_cwd_rejects_missing_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let missing = std::env::temp_dir().join(format!("cc-switch-missing-{unique}"));

        let error = resolve_launch_cwd(Some(missing.to_string_lossy().into_owned()))
            .expect_err("missing directory should fail");

        assert!(error.contains("目录不存在"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn iterm2_applescript_cold_start_avoids_current_window_before_one_exists() {
        let script = build_macos_iterm2_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        let cold_start_branch = script
            .split("else\n        activate")
            .nth(1)
            .expect("cold start branch should be present")
            .split("    end if\n    tell current session")
            .next()
            .expect("cold start branch should end before writing command");

        assert!(cold_start_branch.contains("repeat while (count of windows) = 0"));
        assert!(cold_start_branch.contains("create window with default profile"));
        assert!(!cold_start_branch.contains("tell current window"));
        assert!(!cold_start_branch.contains("create tab with default profile"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn iterm2_applescript_keeps_new_tab_behavior_for_existing_windows() {
        let script = build_macos_iterm2_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        let running_branch = script
            .split("if was_running then")
            .nth(1)
            .expect("already-running branch should be present")
            .split("else\n        activate")
            .next()
            .expect("already-running branch should end before cold start branch");

        assert!(running_branch.contains("if (count of windows) = 0 then"));
        assert!(running_branch.contains("create window with default profile"));
        assert!(running_branch.contains("create tab with default profile"));
    }

    /// Terminal `activate` creates a default empty window on cold start; `launch` does not.
    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_applescript_cold_start_uses_launch_before_do_script() {
        let script = build_macos_terminal_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        assert!(
            script.contains(r#"set was_running to application "Terminal" is running"#),
            "missing was_running detection:\n{script}"
        );
        // Cold launches avoid `activate` until after `do script`, so no default empty window is created first.
        assert!(
            script.contains(
                "else\n        launch\n        do script launcher_script\n        activate"
            ),
            "cold start should launch before activating:\n{script}"
        );
        // Already-running launches should create a fresh session.
        assert!(
            script.contains(
                "if was_running then\n        activate\n        do script launcher_script\n"
            ),
            "already-running branch should use bare do script:\n{script}"
        );
        assert!(
            script.contains(r#"set launcher_script to "exec sh '/tmp/cc_switch_launcher.sh'""#),
            "Terminal should replace the auto-created shell:\n{script}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn otty_launcher_command_executes_the_temporary_script() {
        assert_eq!(
            build_macos_dash_c_command(Path::new("/tmp/cc_switch_launcher.sh")),
            "exec sh '/tmp/cc_switch_launcher.sh'"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn otty_cli_candidates_include_bundle_and_installed_cli_locations() {
        let candidates = macos_otty_cli_candidates();

        assert!(candidates.contains(&PathBuf::from(
            "/Applications/Otty.app/Contents/MacOS/otty-cli"
        )));
        assert!(candidates.contains(&PathBuf::from("/usr/local/bin/otty")));
        assert!(candidates.contains(&PathBuf::from("/opt/homebrew/bin/otty")));
    }

    /// Restored windows should not receive the launcher command.
    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_applescript_does_not_hijack_restored_windows() {
        let script = build_macos_terminal_applescript(Path::new("/tmp/cc_switch_launcher.sh"));
        assert!(
            !script.contains(" in window 1"),
            "should not inject into an existing/restored Terminal window:\n{script}"
        );
        assert!(
            !script.contains("count of windows"),
            "should not infer restored-window safety from window count:\n{script}"
        );
    }

    /// Ghostty cold starts use `initial-command`; warm starts use the scripting dictionary.
    #[cfg(target_os = "macos")]
    #[test]
    fn ghostty_applescript_cold_start_uses_initial_command() {
        let script = build_macos_ghostty_applescript(Path::new("/tmp/cc_switch_launcher.sh"));

        // Warm launches execute through the AppleScript command property, not `open -na ... -e`.
        assert!(
            script.contains(r#"set launcher_command to "sh '/tmp/cc_switch_launcher.sh'""#),
            "missing launcher_command:\n{script}"
        );
        assert!(script.contains("if was_running then"));
        assert!(script.contains("new window with configuration {command:launcher_command}"));
        assert!(
            !script.contains(" --args -e"),
            "should not execute through open -na -e:\n{script}"
        );
        // Cold launches make Ghostty's first default surface execute the launcher.
        assert!(script.contains(r#"set was_running to application "Ghostty" is running"#));
        assert!(
            script.contains(
                r#"do shell script "open -na Ghostty --args --quit-after-last-window-closed=true " & quoted form of ("--initial-command=" & launcher_command)"#
            ),
            "cold start should use initial-command:\n{script}"
        );
        assert!(
            !script.contains("--initial-window=false"),
            "should not rely on initial-window=false:\n{script}"
        );
        assert!(
            !script.contains("delay 0.5"),
            "should not rely on a fixed delay:\n{script}"
        );
        assert!(
            !script.contains("old_ids"),
            "should not track default windows for closing:\n{script}"
        );
        assert!(
            !script.contains("close window"),
            "should not close a default window:\n{script}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dash_c_command_wraps_script_path_inside_quoted_arg() {
        // The script path must stay inside the `-c` string, not as a bare argv.
        let s = build_macos_dash_c_command(Path::new("/tmp/cc_switch_launcher_1.sh"));
        assert_eq!(s, "exec sh '/tmp/cc_switch_launcher_1.sh'");

        // Spaces and single quotes must stay shell-safe too.
        let s2 = build_macos_dash_c_command(Path::new("/Users/me/it's dir/x.sh"));
        assert_eq!(s2, r#"exec sh '/Users/me/it'"'"'s dir/x.sh'"#);
    }

    /// AppleScript launchers need both shell-path quoting and AppleScript string quoting.
    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_builders_safely_quote_special_paths() {
        // First shell-quote the path, then wrap the whole command as an AppleScript string.
        let expected = r#""sh '/Users/me/it'\"'\"'s dir/x.sh'""#;
        let p = Path::new("/Users/me/it's dir/x.sh");
        assert_eq!(applescript_launcher_command(p), expected);
        assert_eq!(
            applescript_exec_launcher_command(p),
            r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#
        );
        assert!(
            build_macos_terminal_applescript(p)
                .contains(r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#),
            "Terminal did not quote safely"
        );
        assert!(
            build_macos_iterm2_applescript(p)
                .contains(r#""exec sh '/Users/me/it'\"'\"'s dir/x.sh'""#),
            "iTerm2 did not quote safely"
        );
        assert!(
            build_macos_ghostty_applescript(p).contains(expected),
            "Ghostty did not keep the non-exec launcher"
        );
    }

    #[test]
    fn build_windows_cwd_command_str_uses_cd_for_drive_paths() {
        let (command, env) = build_windows_cwd_command_str(r"C:\work\repo");

        assert_eq!(command, "cd /d \"C:\\work\\repo\" || exit /b 1\r\n");
        assert_eq!(env, None);
    }

    #[test]
    fn build_windows_cwd_command_str_uses_pushd_for_unc_paths() {
        let (command, _) = build_windows_cwd_command_str(r"\\wsl$\Ubuntu\home\coder\repo");

        assert_eq!(
            command,
            "pushd \"\\\\wsl$\\Ubuntu\\home\\coder\\repo\" || exit /b 1\r\n"
        );
    }

    #[test]
    fn build_windows_cwd_command_str_escapes_batch_metacharacters() {
        let (command, _) = build_windows_cwd_command_str(r"\\server\share\100%&(test)");

        assert_eq!(
            command,
            "pushd \"\\\\server\\share\\100%%^&^(test^)\" || exit /b 1\r\n"
        );
    }

    #[test]
    fn build_windows_cwd_command_str_passes_non_ascii_paths_via_env() {
        // cmd 按控制台代码页解码 .bat，中文路径不能写进文件（#8034）。
        let path = r"C:\Users\林增文\项目";
        let (command, env) = build_windows_cwd_command_str(path);

        assert!(command.is_ascii(), "batch line must stay ASCII: {command}");
        assert_eq!(
            command,
            "cd /d \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 1\r\nset \"CC_SWITCH_INTERNAL_CWD=\"\r\n"
        );
        assert_eq!(env.as_deref(), Some(path));

        let (unc_command, unc_env) = build_windows_cwd_command_str(r"\\server\共享\repo");
        assert!(unc_command.starts_with("pushd \"%CC_SWITCH_INTERNAL_CWD%\" || exit /b 1\r\n"));
        assert_eq!(unc_env.as_deref(), Some(r"\\server\共享\repo"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn wsl_hermes_dashboard_runs_inside_the_distro_login_shell() {
        let line = wsl_hermes_dashboard_line("Ubuntu-22.04").expect("valid WSL command line");
        assert_eq!(
            line,
            r#"wsl.exe -d Ubuntu-22.04 -- sh -c "\"${SHELL:-sh}\" -lic 'hermes dashboard'""#
        );
        assert!(wsl_hermes_dashboard_line("Ubuntu & calc").is_err());
    }

    #[test]
    fn test_build_exec_line() {
        assert_eq!(build_exec_line("/bin/zsh", None), "exec '/bin/zsh' -l");
        assert_eq!(build_exec_line("/bin/bash", None), "exec '/bin/bash'");
        assert_eq!(
            build_exec_line("/opt/homebrew dir/bin/fish", None),
            "exec '/opt/homebrew dir/bin/fish'"
        );
        assert_eq!(build_exec_line("/bin/sh", None), "exec '/bin/sh'");
        assert_eq!(
            build_exec_line("/tmp/shell'quote/zsh", None),
            "exec '/tmp/shell'\"'\"'quote/zsh' -l"
        );
        assert_eq!(
            build_exec_line("/bin/zsh", Some(Path::new("/tmp/project"))),
            r#"exec '/bin/zsh' -lc 'cd '"'"'/tmp/project'"'"' || exit 1; exec '"'"'/bin/zsh'"'"' -i'"#
        );
    }

    #[test]
    fn test_build_final_shell_cd_command() {
        assert_eq!(build_final_shell_cd_command("/bin/zsh", None), "");
        assert_eq!(
            build_final_shell_cd_command("/bin/zsh", Some(Path::new("/tmp/project"))),
            ""
        );
        assert_eq!(
            build_final_shell_cd_command("/bin/bash", Some(Path::new("/tmp/project O'Brien"))),
            "cd '/tmp/project O'\"'\"'Brien' || exit 1\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_user_shell_fallback() {
        // $SHELL 未设置时应按平台 fallback
        // 此测试验证 fallback 逻辑，但不验证环境变量值（取决于运行环境）
        let shell = get_user_shell();
        // 至少应返回一个合法的绝对路径
        assert!(valid_user_shell_path(&shell));
        // basename 应为合法 shell 名
        let basename = shell.rsplit('/').next().unwrap_or("sh");
        assert!(["sh", "bash", "zsh", "fish", "dash"].contains(&basename));
    }

    #[cfg(unix)]
    fn set_test_executable(path: &Path, executable: bool) {
        use std::os::unix::fs::PermissionsExt;

        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("fixture permissions should be set");
    }

    #[cfg(unix)]
    #[test]
    fn test_valid_user_shell_path() {
        let temp = tempfile::tempdir().expect("temp dir should be created");
        let executable_zsh = temp.path().join("zsh");
        std::fs::write(&executable_zsh, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&executable_zsh, true);

        let executable_fish_dir = temp.path().join("homebrew dir/bin");
        std::fs::create_dir_all(&executable_fish_dir)
            .expect("shell fixture directory should be created");
        let executable_fish = executable_fish_dir.join("fish");
        std::fs::write(&executable_fish, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&executable_fish, true);

        let non_executable_bash = temp.path().join("bash");
        std::fs::write(&non_executable_bash, "#!/usr/bin/env sh\n")
            .expect("shell fixture should be written");
        set_test_executable(&non_executable_bash, false);

        assert!(valid_user_shell_path(&executable_zsh.to_string_lossy()));
        assert!(valid_user_shell_path(&executable_fish.to_string_lossy()));
        assert!(!valid_user_shell_path(""));
        assert!(!valid_user_shell_path("zsh"));
        assert!(!valid_user_shell_path(
            &temp.path().join("missing/zsh").to_string_lossy()
        ));
        assert!(!valid_user_shell_path(
            &non_executable_bash.to_string_lossy()
        ));
        assert!(!valid_user_shell_path(
            &temp.path().join("zsh; rm -rf /").to_string_lossy()
        ));
        assert!(!valid_user_shell_path(&format!(
            "{}\n/bin/bash",
            executable_zsh.to_string_lossy()
        )));
        assert!(!valid_user_shell_path("/usr/bin/powershell"));
    }
}
