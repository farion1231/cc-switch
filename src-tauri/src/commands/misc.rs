#![allow(non_snake_case)]

use crate::init_status::{InitErrorPayload, SkillsMigrationPayload};
use indexmap::IndexSet;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tool_lifecycle_progress::ToolLifecycleProgress;

#[path = "tool_discovery.rs"]
pub(super) mod tool_discovery;
#[path = "tool_lifecycle_progress.rs"]
mod tool_lifecycle_progress;
#[path = "tool_node_requirement.rs"]
mod tool_node_requirement;
#[path = "tool_process.rs"]
pub(super) mod tool_process;
#[path = "tool_uninstall.rs"]
mod tool_uninstall;
#[path = "tool_version.rs"]
pub(super) mod tool_version;
use tool_discovery::*;
use tool_process::*;
use tool_version::*;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
pub(super) const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 打开外部链接
/// 外部链接白名单：只允许 `http:` / `https:` / `mailto:`。
/// 不带协议的裸域名（如 `example.com/docs`）按旧行为补 `https://`；
/// `javascript:`、`file:`、`data:` 及其他自定义协议一律拒绝（前端已过滤，这里是第二道）。
fn validate_external_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let parsed = match url::Url::parse(raw) {
        Ok(parsed) => parsed,
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            url::Url::parse(&format!("https://{raw}")).map_err(|_| "链接格式无效".to_string())?
        }
        Err(_) => return Err("链接格式无效".to_string()),
    };
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some_and(|h| !h.is_empty()) => Ok(parsed.into()),
        "mailto" => Ok(parsed.into()),
        "http" | "https" => Err("链接格式无效".to_string()),
        _ => Err("只能打开 http、https 或 mailto 链接".to_string()),
    }
}

#[tauri::command]
pub async fn open_external(app: AppHandle, url: String) -> Result<bool, String> {
    let url = validate_external_url(&url)?;

    app.opener()
        .open_url(&url, None::<String>)
        .map_err(|e| format!("打开链接失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
pub async fn copy_text_to_clipboard(text: String) -> Result<bool, String> {
    // Use spawn_blocking to avoid blocking the async runtime
    // Clipboard access can block on some platforms and may have thread/loop constraints
    tokio::task::spawn_blocking(move || {
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("访问系统剪贴板失败: {e}"))?;
        clipboard
            .set_text(text)
            .map_err(|e| format!("写入系统剪贴板失败: {e}"))?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("剪贴板任务执行失败: {e}"))?
}

#[tauri::command]
pub async fn check_for_updates(handle: AppHandle) -> Result<bool, String> {
    handle
        .opener()
        .open_url(
            "https://github.com/farion1231/cc-switch/releases/latest",
            None::<String>,
        )
        .map_err(|e| format!("打开更新页面失败: {e}"))?;

    Ok(true)
}

/// 便携版新版本压缩包的下载地址，命名与 release.yml 的「Prepare Windows Assets」一致：
/// `CC-Switch-v<版本>-Windows[-arm64]-Portable.zip`。版本号不像版本号时返回 None。
fn portable_zip_url(version: &str, arm64: bool) -> Option<String> {
    let version = version.trim().trim_start_matches('v');
    let looks_like_version = version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !looks_like_version {
        return None;
    }
    let suffix = if arm64 { "-arm64" } else { "" };
    Some(format!(
        "https://github.com/farion1231/cc-switch/releases/download/v{version}/CC-Switch-v{version}-Windows{suffix}-Portable.zip"
    ))
}

/// 便携版更新：直接下载对应架构的新版压缩包（解压覆盖即可，数据在 ~/.cc-switch 不受影响）。
/// 不做原地替换自身 exe：那样很容易被杀毒软件当成病毒拦截。
#[tauri::command]
pub async fn open_portable_update_download(
    handle: AppHandle,
    version: String,
) -> Result<bool, String> {
    let url = portable_zip_url(&version, cfg!(target_arch = "aarch64"))
        .unwrap_or_else(|| "https://github.com/farion1231/cc-switch/releases/latest".to_string());
    handle
        .opener()
        .open_url(url, None::<String>)
        .map_err(|e| format!("打开更新页面失败: {e}"))?;

    Ok(true)
}

/// 判断是否为便携版（绿色版）运行
#[tauri::command]
pub async fn is_portable_mode() -> Result<bool, String> {
    Ok(crate::app_update::is_portable())
}

/// 获取应用启动阶段的初始化错误（若有）。
/// 用于前端在早期主动拉取，避免事件订阅竞态导致的提示缺失。
#[tauri::command]
pub async fn get_init_error() -> Result<Option<InitErrorPayload>, String> {
    Ok(crate::init_status::get_init_error())
}

/// 获取 JSON→SQLite 迁移结果（若有）。
/// 只返回一次 true，之后返回 false，用于前端显示一次性 Toast 通知。
#[tauri::command]
pub async fn get_migration_result() -> Result<bool, String> {
    Ok(crate::init_status::take_migration_success())
}

/// 获取 Skills 自动导入（SSOT）迁移结果（若有）。
/// 只返回一次 Some({count})，之后返回 None，用于前端显示一次性 Toast 通知。
#[tauri::command]
pub async fn get_skills_migration_result() -> Result<Option<SkillsMigrationPayload>, String> {
    Ok(crate::init_status::take_skills_migration_result())
}

#[derive(serde::Serialize)]
pub struct ToolVersion {
    name: String,
    version: Option<String>,
    latest_version: Option<String>, // 新增字段：最新版本
    error: Option<String>,
    /// 已定位到可执行文件、但 `--version` 报错退出（装了却跑不起来，如 Node 版本不达标）。
    /// 供前端区分"未安装"与"已安装·无法运行"，无需匹配 error 文案反推语义。
    installed_but_broken: bool,
    /// 工具运行环境: "windows", "wsl", "macos", "linux", "unknown"
    env_type: String,
    /// 当 env_type 为 "wsl" 时，返回该工具绑定的 WSL distro（用于按 distro 探测 shells）
    wsl_distro: Option<String>,
    /// 有新版本、但新版本要求的 Node 比这处安装用的新：前端不给升级按钮，提示先升级 Node。
    update_blocked_by_node: Option<tool_node_requirement::NodeRequirementGap>,
}

/// 「应用」页管理的一个 CLI 工具。新增工具只在 `TOOLS` 里加一项。
struct ToolSpec {
    id: &'static str,
    display_name: &'static str,
    /// npm 上的包名：版本检查、Node 要求检查、锚定升级与卸载都读它。
    /// hermes 走自己的 CLI/installer，agy 不在 npm。
    npm_package: Option<&'static str>,
    /// `npm i -g` 能否装上它。omp 的入口是 bun 脚本，npm 装上也跑不起来。
    npm_installable: bool,
    /// `npm i -g` 时须追加的参数（前导空格已含），静态安装命令和锚定到某处 npm 的
    /// 升级命令共用。
    ///
    /// Claude Code / OpenCode 的 postinstall 把平台 optional dependency 中的原生程序放到
    /// bin/claude.exe / bin/opencode.exe。npm 12 拦截该脚本时会留下文本占位文件，
    /// Windows 执行后报“与 Windows 版本不兼容”。
    /// Grok Build 的 bin/grok 是 node launcher，postinstall 才把 optional dependency 里的
    /// 平台二进制解到 `~/.grok/bin/grok-<version>` 并 relink `grok`；被拦后 launcher 只要见到
    /// `~/.grok/bin/grok` 就直接运行它、不比对版本，于是 npm 升级 exit 0 却仍跑旧版。
    /// MiniMax Code 依赖 better-sqlite3 的安装脚本：npm 12 默认拦截依赖的 install 脚本，
    /// 只放行 `--allow-scripts` 列出的包（按注册表包名匹配），被拦后 SQLite 不可用；
    /// `--ignore-scripts=false` / `--include=optional` 抵消用户 npmrc 里的相反设置。
    /// npm 10/11 上这串参数无副作用（实测 exit 0、脚本照常执行）。整段 `--allow-scripts=…`
    /// 加双引号：逗号在 PowerShell 里会被当成数组分隔符，bash/cmd 都会剥掉这层引号。
    npm_install_args: &'static str,
    /// 工具自带的升级子命令。
    ///
    /// `omp update` 自己识别 brew / mise / nix / bun / npm / 预编译二进制并照原渠道升级，
    /// 非 TTY 下不询问，失败退出码非 0；Nix 安装它只打印提示就 exit 0，见
    /// `anchored_command_from_paths` 的 Nix 门控。
    /// MiniMax Code 虽有 `mcode update`，却刻意不填：它在 stdin/stdout 非 TTY 时只打印
    /// "No interactive confirmation is available" 就 exit 0、并不安装。静默 lifecycle 的
    /// `cmd.output()` 正是非 TTY，填上会让 `mcode update || <fallback>` 的兜底永不触发。
    official_update_args: Option<&'static str>,
    /// 该工具在 npm 上的预发布通道 tag(靠前者优先)。仅当本地版本已**严格领先**
    /// `latest` 时才会被补查 —— 让主动在抢先通道的用户(如走 Claude Code 的 `next`)
    /// 看到与所在通道对齐的"最新版本",同时绝不把稳定通道用户暴露给预发布版。
    /// 空切片表示该工具只看 `latest`、不补查。
    ///
    /// 为何不通用覆盖所有工具:各家预发布 tag 命名互不统一(codex=alpha/beta/native、
    /// gemini=nightly/preview、openclaw=alpha/beta),且 codex 的 beta/native 是
    /// `0.1.x` 时间戳式版本、gemini 有误发的 `false` tag —— 这些脏值虽会被
    /// `pick_latest_version` 的版本比较挡掉,但维护成本与误报风险不值当,故暂只为
    /// Claude Code 启用。
    npm_prerelease_tags: &'static [&'static str],
}

impl ToolSpec {
    const fn npm(id: &'static str, display_name: &'static str, package: &'static str) -> Self {
        Self {
            npm_package: Some(package),
            ..Self::not_on_npm(id, display_name)
        }
    }

    const fn not_on_npm(id: &'static str, display_name: &'static str) -> Self {
        Self {
            id,
            display_name,
            npm_package: None,
            npm_installable: true,
            npm_install_args: "",
            official_update_args: None,
            npm_prerelease_tags: &[],
        }
    }

    const fn npm_install_args(mut self, args: &'static str) -> Self {
        self.npm_install_args = args;
        self
    }

    const fn not_npm_installable(mut self) -> Self {
        self.npm_installable = false;
        self
    }

    const fn official_update(mut self, args: &'static str) -> Self {
        self.official_update_args = Some(args);
        self
    }

    const fn npm_prerelease(mut self, tags: &'static [&'static str]) -> Self {
        self.npm_prerelease_tags = tags;
        self
    }

    fn npm_install_command(&self) -> Option<String> {
        let package = self.npm_package.filter(|_| self.npm_installable)?;
        Some(format!(
            "npm i -g {package}@latest{}",
            self.npm_install_args
        ))
    }
}

/// 不认识的工具名：没有 npm 包、没有自升级命令。
static UNKNOWN_TOOL: ToolSpec = ToolSpec::not_on_npm("", "Unknown");

/// 顺序即「应用」页与批量安装 / 升级的顺序。
static TOOLS: [ToolSpec; 14] = [
    ToolSpec::npm("claude", "Claude Code", "@anthropic-ai/claude-code")
        .npm_install_args(
            " --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code",
        )
        .official_update("update")
        .npm_prerelease(&["next"]),
    ToolSpec::npm("codex", "Codex", "@openai/codex").official_update("update"),
    ToolSpec::npm("gemini", "Gemini CLI", "@google/gemini-cli"),
    ToolSpec::npm("grok", "Grok Build", "@xai-official/grok")
        .npm_install_args(
            " --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok",
        )
        .official_update("update"),
    ToolSpec::npm("opencode", "OpenCode", "opencode-ai")
        .npm_install_args(" --ignore-scripts=false --include=optional --allow-scripts=opencode-ai")
        .official_update("upgrade"),
    ToolSpec::npm("openclaw", "OpenClaw", "openclaw").official_update("update --yes"),
    ToolSpec::not_on_npm("hermes", "Hermes").official_update("update"),
    ToolSpec::npm("pi", "Pi", "@earendil-works/pi-coding-agent"),
    ToolSpec::npm("mcode", "MiniMax Code", "@minimax-ai/code").npm_install_args(
        " --ignore-scripts=false --include=optional \"--allow-scripts=@minimax-ai/code,better-sqlite3\"",
    ),
    ToolSpec::npm("kimi", "Kimi Code", "@moonshot-ai/kimi-code").official_update("upgrade --yes"),
    ToolSpec::npm("dsh", "DeepSeek Harness", "@deepseek-ai/dsh").npm_install_args(" --engine-strict"),
    ToolSpec::not_on_npm("agy", "Antigravity CLI").official_update("update"),
    ToolSpec::npm("omp", "Oh My Pi", "@oh-my-pi/pi-coding-agent")
        .not_npm_installable()
        .official_update("update"),
    ToolSpec::npm("copilot", "GitHub Copilot CLI", "@github/copilot"),
];

fn known_tool(id: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|tool| tool.id == id)
}

fn tool_spec(id: &str) -> &'static ToolSpec {
    known_tool(id).unwrap_or(&UNKNOWN_TOOL)
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WslShellPreferenceInput {
    #[serde(default)]
    pub wsl_shell: Option<String>,
    #[serde(default)]
    pub wsl_shell_flag: Option<String>,
}

// Keep platform-specific env detection in one place to avoid repeating cfg blocks.
#[cfg(target_os = "windows")]
fn tool_env_type_and_wsl_distro(tool: &str) -> (String, Option<String>) {
    if let Some(distro) = wsl_distro_for_tool(tool) {
        ("wsl".to_string(), Some(distro))
    } else {
        ("windows".to_string(), None)
    }
}

#[cfg(target_os = "macos")]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("macos".to_string(), None)
}

#[cfg(target_os = "linux")]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("linux".to_string(), None)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn tool_env_type_and_wsl_distro(_tool: &str) -> (String, Option<String>) {
    ("unknown".to_string(), None)
}

#[tauri::command]
pub async fn get_tool_versions(
    tools: Option<Vec<String>>,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Result<Vec<ToolVersion>, String> {
    let requested: Vec<&str> = match tools.as_deref() {
        Some(tools) => normalize_requested_tools(tools),
        None => TOOLS.iter().map(|tool| tool.id).collect(),
    };
    let mut results = Vec::new();

    for tool in requested {
        let pref = wsl_shell_by_tool.as_ref().and_then(|m| m.get(tool));
        let tool_wsl_shell = pref.and_then(|p| p.wsl_shell.as_deref());
        let tool_wsl_shell_flag = pref.and_then(|p| p.wsl_shell_flag.as_deref());

        results.push(get_single_tool_version_impl(tool, tool_wsl_shell, tool_wsl_shell_flag).await);
    }

    Ok(results)
}

// 不同工具仍可能共用 pnpm/npm 的全局目录。先保守地串行化所有安装写入，
// 并在排队前锁定工具，防止页面重挂或另一 IPC 调用重复提交同一工具。
struct ToolLifecycleCoordinator {
    tools: HashMap<&'static str, Arc<tokio::sync::Mutex<()>>>,
    execution: Arc<tokio::sync::Mutex<()>>,
}

impl Default for ToolLifecycleCoordinator {
    fn default() -> Self {
        Self {
            tools: TOOLS
                .iter()
                .map(|tool| (tool.id, Arc::new(tokio::sync::Mutex::new(()))))
                .collect(),
            execution: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl ToolLifecycleCoordinator {
    #[cfg(test)]
    async fn run<F>(&self, tools: Vec<&'static str>, operation: F) -> Result<(), String>
    where
        F: FnOnce(&[&str]) -> Result<(), String> + Send + 'static,
    {
        self.run_with_queue_notice(tools, || {}, operation).await
    }

    /// 按工具加锁后串行执行 `operation`；要等前一个工具的写入结束时先调用
    /// `on_queued`，让界面显示「排队中」而不是一直转圈。
    async fn run_with_queue_notice<Q, F>(
        &self,
        tools: Vec<&'static str>,
        on_queued: Q,
        operation: F,
    ) -> Result<(), String>
    where
        Q: FnOnce(),
        F: FnOnce(&[&str]) -> Result<(), String> + Send + 'static,
    {
        let tool_guards = tools
            .iter()
            .map(|tool| {
                self.tools
                    .get(tool)
                    .ok_or_else(|| format!("Unsupported tool action target: {tool}"))?
                    .clone()
                    .try_lock_owned()
                    // 稳定错误码供前端区分后台任务仍在进行与真正的执行失败。
                    .map_err(|_| "TOOL_ACTION_IN_PROGRESS".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let execution_guard = match self.execution.clone().try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => {
                on_queued();
                self.execution.clone().lock_owned().await
            }
        };

        // 必须把锁移进 blocking 任务：即使等待它的 IPC future 被取消，
        // 子进程仍会继续运行，直到它真正退出前都不能允许下一次写入。
        tokio::task::spawn_blocking(move || {
            let _tool_guards = tool_guards;
            let _execution_guard = execution_guard;
            operation(&tools)
        })
        .await
        .map_err(|e| format!("tool lifecycle task join error: {e}"))?
    }
}

static TOOL_LIFECYCLE: Lazy<ToolLifecycleCoordinator> =
    Lazy::new(ToolLifecycleCoordinator::default);

/// Display-only plan. Execution regenerates it under the lifecycle lock and rejects changes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolUninstallPlan {
    tool: String,
    path: String,
    real_path: String,
    command: String,
    native: bool,
    wsl_distro: Option<String>,
}

/// One installation offered in the uninstall dialog: a plan, or why it must be removed manually.
#[derive(Debug, serde::Serialize)]
pub struct ToolUninstallCandidate {
    #[serde(flatten)]
    install: ToolInstallation,
    plan: Option<ToolUninstallPlan>,
    unavailable_reason: Option<String>,
    /// A copy the tool's own updater recreates (Grok in npm mode), so removing it alone is temporary.
    reinstalled_by_updater: bool,
}

#[tauri::command]
pub async fn prepare_tool_uninstall(
    tool: String,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
) -> Result<Vec<ToolUninstallCandidate>, String> {
    if known_tool(&tool).is_none() {
        return Err(format!("Unsupported tool action target: {tool}"));
    }
    tokio::task::spawn_blocking(move || {
        let pref = wsl_shell_by_tool
            .as_ref()
            .and_then(|prefs| prefs.get(&tool));
        tool_uninstall::candidates(&tool, pref)
    })
    .await
    .map_err(|e| format!("uninstall plan task join error: {e}"))?
}

#[tauri::command]
pub async fn run_tool_lifecycle_action(
    app: AppHandle,
    tools: Vec<String>,
    action: String,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
    uninstall_plan: Option<ToolUninstallPlan>,
) -> Result<(), String> {
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }

    if action == "uninstall" {
        let plan = uninstall_plan.ok_or("Preview the uninstall plan before confirming.")?;
        if requested.len() != 1 || requested[0] != plan.tool {
            return Err("The uninstall plan does not match the selected tool.".into());
        }
        let progress = ToolLifecycleProgress::from_app(app, &requested);
        let queued = progress.clone();
        return TOOL_LIFECYCLE
            .run_with_queue_notice(
                requested,
                move || queued.queued(),
                move |tools| {
                    progress.running();
                    for tool in tools {
                        let pref = wsl_shell_by_tool
                            .as_ref()
                            .and_then(|prefs| prefs.get(*tool));
                        tool_uninstall::uninstall(tool, pref, &plan, &progress)?;
                    }
                    Ok(())
                },
            )
            .await;
    }
    let action = ToolLifecycleAction::from_str(&action)?;
    if matches!(action, ToolLifecycleAction::Update) {
        for tool in &requested {
            tool_node_requirement::ensure_update_node(tool).await?;
        }
    }

    let label = match action {
        ToolLifecycleAction::Install => "tool_install",
        ToolLifecycleAction::Update => "tool_update",
    };

    // 排队结束后再探测安装目标，与执行一起持锁，避免读到另一升级的中间状态。
    let progress = ToolLifecycleProgress::from_app(app, &requested);
    let queued = progress.clone();
    TOOL_LIFECYCLE
        .run_with_queue_notice(
            requested,
            move || queued.queued(),
            move |tools| {
                progress.running();
                let command_line =
                    build_tool_lifecycle_command(tools, action, wsl_shell_by_tool.as_ref())?;
                run_tool_lifecycle_silently(&command_line, label, &progress)
            },
        )
        .await
}

/// 静默执行工具安装/更新脚本：直接捕获子进程输出并阻塞到命令真正结束，
/// 不再弹出可见终端窗口（与 `launch_terminal_running` 的"开窗即返回"形成对比，
/// 后者仍保留给 provider 切换等需要交互式终端的场景）。
/// 失败时回传 stderr/stdout 末尾若干行，供前端 toast 提示。
/// 不开终端窗口，但每行输出会经 `progress` 推给界面显示。
#[cfg(not(target_os = "windows"))]
fn run_tool_lifecycle_silently(
    command_line: &str,
    _label: &str,
    progress: &ToolLifecycleProgress,
) -> Result<(), String> {
    use std::process::Command;
    // command_line 是 bash 风格脚本（含 `set -e` 与多行命令）；强制用 bash 执行，
    // 避免用户默认 shell 为 fish/zsh 时 `set -e` 等语义不一致。
    let mut cmd = Command::new("bash");
    cmd.arg("-c").arg(command_line);
    // GUI App 继承的是 launchd 的窄 PATH，而锚定探测用的是登录 shell —— 见
    // `login_shell_path` doc。命令自身用绝对路径不受影响，但工具**内部** spawn 的
    // npm/node 等子进程、以及 install 链里裸的 npm fallback 都需要真实 PATH。
    if let Some(login_path) = login_shell_path(None) {
        let inherited = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", merge_path_segments(&login_path, &inherited));
    }
    let output = tool_lifecycle_progress::run_with_progress(cmd, progress)
        .map_err(|e| format!("启动安装进程失败: {e}"))?;
    finish_lifecycle_output(&output)
}

/// Windows 静默执行：command_line 是 .bat 内容（chcp 65001 + @echo off + call/wsl 行，CRLF 分隔），
/// 写临时 .bat 后用 `cmd /C` 执行，`CREATE_NO_WINDOW` 抑制 console 窗口。
#[cfg(target_os = "windows")]
fn run_tool_lifecycle_silently(
    command_line: &str,
    label: &str,
    progress: &ToolLifecycleProgress,
) -> Result<(), String> {
    // 每次调用使用独立目录，避免同进程并发升级时覆盖或删除另一工具的脚本。
    let script_dir = tempfile::Builder::new()
        .prefix("cc_switch_lifecycle_")
        .tempdir()
        .map_err(|e| format!("创建批处理目录失败: {e}"))?;
    let bat_file = script_dir.path().join(format!("{label}.bat"));
    std::fs::write(&bat_file, command_line).map_err(|e| format!("写入批处理文件失败: {e}"))?;

    let output = tool_lifecycle_progress::run_with_progress(
        windows_lifecycle_command(&bat_file, &effective_path_string()),
        progress,
    );

    finish_lifecycle_output(&output.map_err(|e| format!("启动安装进程失败: {e}"))?)
}

#[cfg(target_os = "windows")]
fn windows_lifecycle_command(bat_file: &Path, effective_path: &str) -> std::process::Command {
    let mut command = std::process::Command::new("cmd");
    command
        .arg("/C")
        .arg(bat_file)
        // MSI 自动重启可能丢失用户 PATH（#8171）。执行与探测共用补全后的 PATH，
        // 让裸 npm 兜底及工具内部启动的 node/npm 都能找到；仅设置子进程环境。
        .env("PATH", effective_path)
        .creation_flags(CREATE_NO_WINDOW);
    command
}

/// 把子进程退出结果转成 `Result`：成功返回 `Ok`；失败提取 stderr（空则回退 stdout）
/// 的末尾若干行作为错误详情，避免把整段安装日志塞进 toast。
fn finish_lifecycle_output(output: &std::process::Output) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = decode_command_output(&output.stderr);
    let stdout = decode_command_output(&output.stdout);
    let raw = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let detail = npm_bin_collision_detail(raw).unwrap_or_else(|| last_lines(raw, 8));
    Err(if detail.is_empty() {
        format!("命令执行失败 (exit code: {:?})", output.status.code())
    } else {
        append_npm_bin_collision_hint(detail, raw)
    })
}

/// npm 全局安装在链接可执行文件时若目标路径已被其他安装方式留下的启动器
/// 占用，会以 EEXIST 失败，原始输出只建议 `--force` 覆盖（#9173）。识别这种
/// bin 路径冲突后在原始输出后追加可执行提示：指出冲突文件、给出安全的
/// 备份改名/改用原安装方式两条路，并明确提醒不要盲目 `--force`。
/// 非此类失败原样返回，不改变既有错误文案。
fn append_npm_bin_collision_hint(detail: String, raw: &str) -> String {
    let Some(path) = npm_bin_collision_path(raw) else {
        return detail;
    };
    format!(
        "{detail}\n\nHint: npm could not create the executable because {path} already exists. \
         If it is a launcher from another installation, back it up or rename it \
         (for example to {path}.bak) and try again, or update the tool with the \
         installation method that created it. Do not use npm --force unless you \
         mean to overwrite the existing file."
    )
}

/// 从 npm 输出里提取 bin 冲突路径：要求同时出现 `EEXIST` 与
/// `File exists: <path>` 行，避免把其他含 EEXIST 字样的失败误判为路径冲突。
fn npm_bin_collision_path(raw: &str) -> Option<&str> {
    if !raw.contains("EEXIST") {
        return None;
    }
    raw.lines().find_map(|line| {
        line.split_once("File exists:")
            .map(|(_, path)| path.trim())
            .filter(|path| !path.is_empty())
    })
}

/// npm 链接可执行文件撞上不归它管的同名文件时以 EEXIST 退出（#9173）。关键的两行
/// 可能被尾部的 `--force` 建议、更新通知和日志路径挤出最后 8 行，这里单独保留，
/// 前端据此换成可操作的提示。
fn npm_bin_collision_detail(text: &str) -> Option<String> {
    let code = text.lines().find(|line| line.contains("EEXIST"))?;
    let exists = text.lines().find(|line| line.contains("File exists:"))?;
    Some(format!("{}\n{}", code.trim(), exists.trim()))
}

/// 取文本末尾最多 `n` 行（npm / pip 的关键错误通常出现在输出尾部）。
fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

pub(crate) fn decode_command_output(bytes: &[u8]) -> String {
    #[cfg(target_os = "windows")]
    {
        decode_windows_command_output(bytes)
    }

    #[cfg(not(target_os = "windows"))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(target_os = "windows")]
fn decode_windows_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    use windows_sys::Win32::Globalization::{GetACP, GetOEMCP, MultiByteToWideChar};

    fn decode_codepage(bytes: &[u8], codepage: u32) -> Option<String> {
        if codepage == 0 {
            return None;
        }

        let input_len = i32::try_from(bytes.len()).ok()?;
        unsafe {
            let wide_len = MultiByteToWideChar(
                codepage,
                0,
                bytes.as_ptr(),
                input_len,
                std::ptr::null_mut(),
                0,
            );
            if wide_len <= 0 {
                return None;
            }

            let mut wide = vec![0u16; wide_len as usize];
            let written = MultiByteToWideChar(
                codepage,
                0,
                bytes.as_ptr(),
                input_len,
                wide.as_mut_ptr(),
                wide_len,
            );
            if written <= 0 {
                return None;
            }

            Some(String::from_utf16_lossy(&wide[..written as usize]))
        }
    }

    let oem_cp = unsafe { GetOEMCP() };
    if let Some(decoded) = decode_codepage(bytes, oem_cp) {
        return decoded;
    }

    let ansi_cp = unsafe { GetACP() };
    if ansi_cp != oem_cp {
        if let Some(decoded) = decode_codepage(bytes, ansi_cp) {
            return decoded;
        }
    }

    String::from_utf8_lossy(bytes).into_owned()
}

fn normalize_requested_tools(tools: &[String]) -> Vec<&'static str> {
    let set: std::collections::HashSet<&str> = tools.iter().map(|s| s.as_str()).collect();
    TOOLS
        .iter()
        .map(|tool| tool.id)
        .filter(|tool| set.contains(tool))
        .collect()
}

#[derive(Debug, Clone, Copy)]
enum ToolLifecycleAction {
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

fn build_tool_lifecycle_command(
    tools: &[&str],
    action: ToolLifecycleAction,
    wsl_shell_by_tool: Option<&HashMap<String, WslShellPreferenceInput>>,
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
const GROK_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://x.ai/cli/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
/// MiniMax Code 官方 installer：用 npm `--prefix` 装进 `~/.minimax-code`（`MCODE_INSTALL_DIR`
/// 可改），系统 Node 不达标时自带隔离 Node；重跑即升级（已是最新则跳过），全程无交互提示。
const MCODE_INSTALL_UNIX: &str =
    "bash -c 'tmp=$(mktemp) && curl -fsSL https://filecdn.minimax.chat/public/install.sh -o $tmp && bash $tmp; status=$?; rm -f $tmp; exit $status'";
/// Codex 官方独立安装器（POSIX sh 脚本）。独立安装版的 `codex update` 内部跑的正是
/// `curl ... | sh` 且没开 pipefail——实测断网时 curl 失败、sh 读到空脚本 exit 0，
/// `codex update` 仍打印 "Update ran successfully" 并 exit 0。所以不走 `codex update`，
/// 由这里下载到临时文件再执行，curl 失败会如实变成整条命令失败。
/// 只用于锚定升级（Windows 用 `CODEX_INSTALL_WINDOWS_SCRIPT`；WSL 不锚定），故按平台 gate。
#[cfg(not(target_os = "windows"))]
const CODEX_INSTALL_UNIX: &str =
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
fn mcode_installer_update_command(install_root: &str) -> String {
    format!(
        "MCODE_INSTALL_DIR={} {MCODE_INSTALL_UNIX}",
        shell_single_quote(install_root)
    )
}

#[cfg(target_os = "windows")]
fn mcode_installer_update_command(install_root: &str) -> String {
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
fn kimi_installer_update_command(install_root: &str) -> String {
    format!(
        "KIMI_INSTALL_DIR={} {KIMI_INSTALL_UNIX}",
        shell_single_quote(install_root)
    )
}

#[cfg(target_os = "windows")]
fn kimi_installer_update_command(install_root: &str) -> String {
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
        ToolLifecycleAction::Update => {
            tool_action_shell_command_for_shell(tool, action, LifecycleCommandShell::Posix)
        }
    }
}

fn build_tool_action_line(
    tool: &str,
    action: ToolLifecycleAction,
    wsl_shell: Option<&str>,
    wsl_shell_flag: Option<&str>,
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
                match resolve_update_command(tool, &installs) {
                    UpdateCommand::Anchored(command) | UpdateCommand::Static(command) => command,
                    UpdateCommand::Unmanaged => {
                        return Err(unmanaged_update_error(tool, &installs));
                    }
                }
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
                match resolve_update_command(tool, &installs) {
                    UpdateCommand::Anchored(command) | UpdateCommand::Static(command) => command,
                    UpdateCommand::Unmanaged => {
                        return Err(unmanaged_update_error(tool, &installs));
                    }
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

/// Windows 双引号包裹基础原语:无条件加引号 + 内部 `"` 转义为 `\"`。
/// `windows_cmd_double_quote_arg`(给 wsl.exe 传 bash 命令字符串用)与
/// `win_quote_path_for_batch`(给锚定路径用)都基于它,避免两份 quoter 各自演化、
/// 未来对同一路径产生不一致引用形态。镜像 POSIX 侧 `shell_single_quote` 与
/// `quote_path_if_spaced` 的"重量基础 + 轻量条件包装"两层结构。
#[cfg(target_os = "windows")]
fn win_double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(target_os = "windows")]
pub(super) fn windows_cmd_double_quote_arg(value: &str) -> String {
    win_double_quote(value)
}

/// WSL 未指定 shell 时依次尝试的 flag：交互式初始化（nvm 等写在 rc 里）优先。
/// 卸载探测（`tool_uninstall`）按同一顺序选 flag，否则两边可能命中不同的安装。
#[cfg(any(target_os = "windows", test))]
const WSL_AUTO_SHELL_FLAGS: [&str; 3] = ["-lic", "-lc", "-c"];

/// WSL 未指定 shell 时的版本检测命令（由外层 `sh -c` 执行）：在 `$SHELL` 里按
/// `WSL_AUTO_SHELL_FLAGS` 依次重试，或只用调用方已校验过的 `flag`。
#[cfg(any(target_os = "windows", test))]
pub(super) fn wsl_auto_shell_command(payload: &str, flag: Option<&str>) -> String {
    let flags = flag.map_or(WSL_AUTO_SHELL_FLAGS.to_vec(), |flag| vec![flag]);
    let last = flags.len() - 1;
    flags
        .iter()
        .enumerate()
        .map(|(i, flag)| {
            // 只有最后一次尝试保留 stderr，供报错展示
            let quiet = if i < last { " 2>/dev/null" } else { "" };
            format!("\"${{SHELL:-sh}}\" {flag} '{payload}'{quiet}")
        })
        .collect::<Vec<_>>()
        .join(" || ")
}

/// 校验 WSL 发行版名称是否合法
/// WSL 发行版名称只允许字母、数字、连字符、下划线和点（如 `Ubuntu-22.04`）
#[cfg(target_os = "windows")]
pub(super) fn is_valid_wsl_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Validate that the given shell name is one of the allowed shells.
pub(super) fn is_valid_shell(shell: &str) -> bool {
    matches!(
        shell.rsplit('/').next().unwrap_or(shell),
        "sh" | "bash" | "zsh" | "fish" | "dash"
    )
}

/// Validate that the given shell flag is one of the allowed flags.
#[cfg(target_os = "windows")]
fn is_valid_shell_flag(flag: &str) -> bool {
    matches!(flag, "-c" | "-lc" | "-lic")
}

/// Return the default invocation flag for the given shell.
fn default_flag_for_shell(shell: &str) -> &'static str {
    match shell.rsplit('/').next().unwrap_or(shell) {
        "dash" | "sh" => "-c",
        "fish" => "-lc",
        _ => "-lic",
    }
}

/// The "effective PATH" used for CLI detection and lifecycle commands. On Windows
/// the inherited process PATH can be incomplete — most notably after an in-app self-update,
/// where the MSI/WiX-auto-launched process inherits only the machine-level PATH
/// and drops the user-level PATH (see #6061). Any CLI installed in a user-PATH
/// location (winget Claude `%LOCALAPPDATA%\Programs\claude`, the standalone
/// Codex installer `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin`, a custom npm
/// prefix `D:\npm-global`, …) then reads as "not installed".
///
/// Reconstruct the effective PATH by merging the process PATH with the machine
/// and user registry PATH values (`REG_EXPAND_SZ` expanded), so detection sees
/// the same installations a freshly logged-in shell would — regardless of how
/// the current process was launched. Process entries are kept first (a runtime
/// override wins); registry entries fill whatever the process is missing;
/// duplicates are removed.
///
/// See `env_checker::check_system_env` for the same set of registry keys; here
/// referenced variables also fall back to these keys if absent from the process.
///
/// fnm's default Node alias is appended last (see `fnm_default_alias_dir_candidates`),
/// so a Node that only exists in `fnm env` shells is still visible, while every
/// explicit PATH entry keeps precedence.
#[cfg(target_os = "windows")]
pub(super) fn effective_path_string() -> String {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let process = std::env::var_os("PATH")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let user = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .and_then(|k| k.get_value::<String, &str>("Path"))
        .map(|raw| expand_env_chars(&raw))
        .unwrap_or_default();
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment")
        .and_then(|k| k.get_value::<String, &str>("Path"))
        .map(|raw| expand_env_chars(&raw))
        .unwrap_or_default();
    let fnm = fnm_default_alias_dir_candidates(
        std::env::var_os("FNM_DIR"),
        &dirs::home_dir().unwrap_or_default(),
        dirs::data_dir(),
    )
    .into_iter()
    .filter(|dir| dir.is_dir())
    .map(|dir| dir.to_string_lossy().into_owned())
    .collect::<Vec<_>>()
    .join(";");
    merge_path_segments_win(&[&process, &user, &machine, &fnm])
}

/// Merge several Windows PATH strings (`;`-separated) in order, keeping only
/// the first occurrence of each segment (case-insensitive). Process segments
/// come first to respect runtime overrides, followed by user- and
/// machine-level registry segments.
#[cfg(target_os = "windows")]
pub(super) fn merge_path_segments_win(parts: &[&str]) -> String {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged: Vec<&str> = Vec::new();
    for part in parts {
        for seg in part.split(';') {
            let s = seg.trim();
            if s.is_empty() || !seen.insert(s.to_ascii_lowercase()) {
                continue;
            }
            merged.push(s);
        }
    }
    merged.join(";")
}

/// Convert a canonicalized Windows path back to the form accepted by shell
/// commands. `std::fs::canonicalize` prefixes local paths with `\\?\` (and UNC
/// paths with `\\?\UNC\`), but `cmd.exe` cannot `call` a batch file through
/// those verbatim paths and reports "The system cannot find the path
/// specified." Direct Win32 executable launches accept the prefix; batch
/// scripts do not.
#[cfg(target_os = "windows")]
pub(super) fn windows_shell_compatible_path(path: &Path) -> std::path::PathBuf {
    let raw = path.to_string_lossy();
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        std::path::PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(local) = raw.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(local)
    } else {
        path.to_path_buf()
    }
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
fn parent_dir(p: &str) -> String {
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
fn brew_formula_from_path(real: &str) -> Option<String> {
    brew_token_from_path(real, "Cellar")
}

/// 从 canonicalize 后的真身路径提取 Homebrew cask token：
/// `/opt/homebrew/Caskroom/codex/0.146.0/bin/codex` → `Some("codex")`。
/// 非 Caskroom 路径返回 None。Cask 是预编译二进制，不走 node；若误判成 Homebrew
/// npm 全局包，会用 sibling `npm i -g` 覆盖 `/opt/homebrew/bin/<tool>` 并触发 EEXIST。
#[cfg(not(target_os = "windows"))]
fn brew_cask_from_path(real: &str) -> Option<String> {
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
            return segs.next().filter(|s| !s.is_empty()).map(|s| s.to_string());
        }
    }
    None
}

/// Cellar formula 或 Caskroom cask 都由 Homebrew 拥有，升级必须走 brew，
/// 不能落到 sibling npm。任一命中即视为 brew-managed。
#[cfg(not(target_os = "windows"))]
fn is_brew_managed_path(real: &str) -> bool {
    brew_formula_from_path(real).is_some() || brew_cask_from_path(real).is_some()
}

/// xAI's native installer uses `~/.grok/bin` for its launchers and
/// `~/.grok/downloads/grok-<platform>` for the downloaded binary. The launcher
/// directory also covers the default Windows layout, where the executable is
/// copied instead of symlinked. Checking the real target additionally supports
/// a custom `$GROK_BIN_DIR` on POSIX, whose launcher still points into the
/// standard downloads directory.
fn is_grok_native_install(bin_path: &str, real_target: &str) -> bool {
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
enum McodeLayout {
    Posix,
    Windows,
}

impl McodeLayout {
    const NATIVE: Self = if cfg!(target_os = "windows") {
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
fn mcode_script_install_root(
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
fn kimi_native_install_root(bin_path: &str, real_target: &str) -> Option<String> {
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
fn mcode_receipt_install_root(entry: &str, layout: McodeLayout) -> Option<String> {
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
fn win_quote_path_for_batch(p: &str) -> String {
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
fn sibling_bin_with_ext(
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
fn sibling_bin(bin_path: &str, exe: &str) -> Option<String> {
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
    tool_spec(tool)
        .official_update_args
        .map(|args| format!("{} {args}", quote_path_if_spaced(bin_path)))
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
            matches!(tool, "claude" | "opencode" | "openclaw" | "kimi" | "omp")
        }
        LifecycleCommandShell::WindowsBatch => {
            matches!(
                tool,
                // OpenCode 的 Windows `upgrade` 在 anomalyco/opencode#17295 修复前可能因
                // 安装方式探测失败弹交互 prompt（spawn npm.cmd 没传 shell:true）；静默
                // lifecycle 没有 stdin 会挂死，Windows 先锚到包管理器路径，等上游修了
                // 再把 opencode 加回这里。
                "claude" | "openclaw" | "kimi" | "omp"
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
    let pkg = tool_spec(tool).npm_package?;
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
    anchored_npm_command(
        bin_path,
        &format!("i -g {pkg}@latest{}", tool_spec(tool).npm_install_args),
    )
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
fn anchored_command_from_paths(tool: &str, bin_path: &str, real_target: &str) -> Option<String> {
    let real_lower = real_target.to_ascii_lowercase();

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
            "opencode" if !real_lower.contains("/.bun/") => Some("opencode-ai"),
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
    let pkg = tool_spec(tool).npm_package?;

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
                "{} i -g {pkg}@latest{}",
                win_quote_path_for_batch(&npm),
                tool_spec(tool).npm_install_args
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
fn anchored_command_from_paths(tool: &str, bin_path: &str, real_target: &str) -> Option<String> {
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
fn installs_anchored_command(tool: &str, installs: &[ToolInstallation]) -> Option<String> {
    let inst = default_install(installs)?;
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

/// 静态命令（= 平台可安全静默执行的官方 CLI 自升级 || `npm i -g <pkg>@latest` /
/// 官方 installer）。锚定探不到默认安装时回退到它；npm fallback 仍等同于
/// "装到 PATH 第一个 npm"的旧行为。
fn static_fallback_command_for(tool: &str, action: ToolLifecycleAction) -> String {
    tool_action_shell_command(tool, action).unwrap_or_default()
}

fn static_fallback_command(tool: &str) -> String {
    static_fallback_command_for(tool, ToolLifecycleAction::Update)
}

/// 升级命令的解析结果。plan（前端确认 / 展示）与 execute（真正执行）共用
/// `resolve_update_command`，保证两边判定一致。
#[derive(Debug, PartialEq, Eq)]
enum UpdateCommand {
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

fn resolve_update_command(tool: &str, installs: &[ToolInstallation]) -> UpdateCommand {
    if let Some(command) = installs_anchored_command(tool, installs) {
        return UpdateCommand::Anchored(command);
    }
    if default_install(installs).is_some_and(is_unmanaged_native_install) {
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
fn is_native_executable(path: &Path) -> bool {
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
    let path = default_install(installs)
        .map(|inst| inst.path.as_str())
        .unwrap_or_default();
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
fn plan_command_for(tool: &str, installs: &[ToolInstallation]) -> (UpdateCommand, bool) {
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
fn is_conflicting(installs: &[ToolInstallation]) -> bool {
    if installs.len() < 2 {
        return false;
    }
    let distinct_versions: std::collections::HashSet<&Option<String>> =
        installs.iter().map(|i| &i.version).collect();
    let runnable_mixed =
        installs.iter().any(|i| i.runnable) && installs.iter().any(|i| !i.runnable);
    distinct_versions.len() > 1 || runnable_mixed
}

/// 一次"探测工具安装分布"的结果：枚举到的所有安装 + 各项衍生判定。同时服务两条
/// 路径——诊断展示（`is_conflict`）与升级确认（`needs_confirmation`/`command`/`anchored`）。
/// 字段保持 snake_case（与 `ToolInstallation` 一致），前端按同名读取。
#[derive(Debug, serde::Serialize)]
pub struct ToolInstallationReport {
    tool: String,
    /// 该工具枚举到的所有安装。
    installs: Vec<ToolInstallation>,
    /// 严阈值：≥2 且(版本分歧或运行态混合)。诊断按钮/自动补诊据此展示冲突。
    is_conflict: bool,
    /// 宽阈值：≥2 处。升级确认据此弹窗（升级只动一处，任何多处都该让用户知情）。
    needs_confirmation: bool,
    /// 锚定后将执行的升级命令（仅展示；真正执行时后端会重新生成，不信任前端回传）。
    command: String,
    /// 是否成功锚定到某处具体安装。false = 退到裸 fallback 命令（无法确定命令行实际
    /// 命中哪处，或该处无同级 npm）；前端据此给出"默认入口无法确定"的诚实文案。
    anchored: bool,
    /// 默认那处是认不出安装渠道的原生可执行文件（见 `UpdateCommand::Unmanaged`）：
    /// 不会执行升级，此时 `command` 为空；前端跳过该工具并提示用原安装方式升级。
    unmanaged: bool,
}

/// 探测各工具的安装分布：枚举所有安装、标记冲突、生成锚定升级命令。只读、无副作用。
/// 诊断按钮、升级前确认、升级后补诊共用此命令，各取所需字段——避免对同一份枚举结果
/// 散落多套下游判定。
#[tauri::command]
pub async fn probe_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }
    tokio::task::spawn_blocking(move || {
        requested
            .into_iter()
            .map(|tool| {
                let installs = enumerate_tool_installations(tool);
                let (update, needs_confirmation) = plan_command_for(tool, &installs);
                let (command, anchored, unmanaged) = match update {
                    UpdateCommand::Anchored(command) => (command, true, false),
                    UpdateCommand::Static(command) => (command, false, false),
                    UpdateCommand::Unmanaged => (String::new(), false, true),
                };
                let is_conflict = is_conflicting(&installs);
                ToolInstallationReport {
                    tool: tool.to_string(),
                    installs,
                    is_conflict,
                    needs_confirmation,
                    command,
                    anchored,
                    unmanaged,
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("probe task join error: {e}"))
}

/// 「应用」页显示每个工具的路径、安装来源和多处安装。和升级前的预检是同一份枚举，
/// 单列一个命令只为把「打开页面时的展示」和「点升级时的预检」分开调用。
#[tauri::command]
pub async fn list_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    probe_tool_installations(tools).await
}

/// 这个工具的配置目录是不是指向 WSL（只有 Windows 上会是）。
pub(crate) fn tool_runs_in_wsl(tool: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        wsl_distro_for_tool(tool).is_some()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = tool;
        false
    }
}

#[cfg(target_os = "windows")]
pub(super) fn wsl_distro_for_tool(tool: &str) -> Option<String> {
    let override_dir = match tool {
        "claude" => crate::settings::get_claude_override_dir(),
        "codex" => crate::settings::get_codex_override_dir(),
        "gemini" => crate::settings::get_gemini_override_dir(),
        "grok" => crate::settings::get_grok_override_dir(),
        "opencode" => crate::settings::get_opencode_override_dir(),
        "openclaw" => crate::settings::get_openclaw_override_dir(),
        "hermes" => crate::settings::get_hermes_override_dir(),
        "pi" => crate::settings::get_pi_override_dir(),
        "omp" => crate::settings::get_omp_override_dir(),
        "dsh" => crate::settings::get_dsh_override_dir(),
        _ => None,
    }?;

    wsl_distro_from_path(&override_dir)
}

/// 从 UNC 路径中提取 WSL 发行版名称
/// 支持 `\\wsl$\Ubuntu\...` 和 `\\wsl.localhost\Ubuntu\...` 两种格式
#[cfg(target_os = "windows")]
fn wsl_distro_from_path(path: &Path) -> Option<String> {
    use std::path::{Component, Prefix};
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            let server_name = server.to_string_lossy();
            if server_name.eq_ignore_ascii_case("wsl$")
                || server_name.eq_ignore_ascii_case("wsl.localhost")
            {
                let distro = share.to_string_lossy().to_string();
                if !distro.is_empty() {
                    return Some(distro);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg_attr(windows, allow(dead_code))]
pub(super) fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 设置窗口主题（Windows/macOS 标题栏颜色）
/// theme: "dark" | "light" | "system"
#[tauri::command]
pub async fn set_window_theme(window: tauri::Window, theme: String) -> Result<(), String> {
    use tauri::Theme;

    let tauri_theme = match theme.as_str() {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None, // system default
    };

    window.set_theme(tauri_theme).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {

    use super::*;

    /// 前端「应用」页的工具表（`TOOL_NAMES` / `TOOL_DISPLAY_NAMES`）对照
    /// `tests/config/tools.json`，这份文件由这里从 [`TOOLS`] 生成。改了 `TOOLS` 后用
    /// `CC_SWITCH_UPDATE_GOLDEN=1 cargo test tools_match_shared_fixture` 重新生成。
    #[test]
    fn tools_match_shared_fixture() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/config/tools.json");
        let tools: serde_json::Map<String, serde_json::Value> = TOOLS
            .iter()
            .map(|tool| (tool.id.to_string(), tool.display_name.into()))
            .collect();
        let expected = serde_json::to_string_pretty(&tools).unwrap() + "\n";
        if std::env::var_os("CC_SWITCH_UPDATE_GOLDEN").is_some() {
            std::fs::write(&path, &expected).unwrap();
        }
        let actual = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            actual, expected,
            "tests/config/tools.json 和 TOOLS 不一致；确认改动后用 CC_SWITCH_UPDATE_GOLDEN=1 重新生成，再同步前端 TOOL_DISPLAY_NAMES"
        );
    }

    #[test]
    fn portable_zip_url_matches_release_asset_names() {
        assert_eq!(
            portable_zip_url("4.0.12", false).as_deref(),
            Some("https://github.com/farion1231/cc-switch/releases/download/v4.0.12/CC-Switch-v4.0.12-Windows-Portable.zip")
        );
        assert_eq!(
            portable_zip_url("v4.1.0-beta.1", true).as_deref(),
            Some("https://github.com/farion1231/cc-switch/releases/download/v4.1.0-beta.1/CC-Switch-v4.1.0-beta.1-Windows-arm64-Portable.zip")
        );
        for bad in ["", "v", "latest", "4.0/../x", "4.0.0?x=1"] {
            assert!(portable_zip_url(bad, false).is_none(), "{bad}");
        }
    }

    #[test]
    fn external_url_whitelist() {
        assert_eq!(
            validate_external_url("https://example.com/a").unwrap(),
            "https://example.com/a"
        );
        assert!(validate_external_url("http://localhost:3000").is_ok());
        assert!(validate_external_url("mailto:a@example.com").is_ok());
        assert_eq!(
            validate_external_url("example.com/docs").unwrap(),
            "https://example.com/docs"
        );
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<script>alert(1)</script>",
            "vscode://open",
            "https://",
            "",
        ] {
            assert!(validate_external_url(bad).is_err(), "{bad}");
        }
    }
    use std::path::{Path, PathBuf};

    #[tokio::test]
    async fn lifecycle_coordinator_serializes_writes_and_rejects_duplicate_tools() {
        let coordinator = Arc::new(ToolLifecycleCoordinator::default());
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("installed.txt");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let first = {
            let coordinator = coordinator.clone();
            let output = output.clone();
            tokio::spawn(async move {
                coordinator
                    .run(vec!["codex"], move |_| {
                        started_tx.send(()).unwrap();
                        finish_rx.recv().unwrap();
                        std::fs::write(output, "codex").unwrap();
                        Ok(())
                    })
                    .await
            })
        };
        started_rx.await.unwrap();

        // 独立调用者（例如重挂后的页面）不能重复启动正在执行的工具。
        assert_eq!(
            coordinator
                .run(vec!["codex"], |_| panic!("duplicate must not execute"))
                .await
                .unwrap_err(),
            "TOOL_ACTION_IN_PROGRESS"
        );
        // 批次部分取锁失败时，已取得的其他工具锁也必须释放。
        assert!(coordinator
            .run(vec!["claude", "codex"], |_| panic!(
                "batch must not execute"
            ))
            .await
            .is_err());

        let second_output = output.clone();
        let second = coordinator.run(vec!["claude"], move |_| {
            assert_eq!(std::fs::read_to_string(&second_output).unwrap(), "codex");
            std::fs::write(second_output, "codex,claude").unwrap();
            Ok(())
        });
        tokio::pin!(second);
        assert!(futures::poll!(second.as_mut()).is_pending());
        assert!(!output.exists(), "first write has not finished yet");
        assert_eq!(
            coordinator
                .run(vec!["claude"], |_| panic!(
                    "queued duplicate must not execute"
                ))
                .await
                .unwrap_err(),
            "TOOL_ACTION_IN_PROGRESS"
        );

        finish_tx.send(()).unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap();
        assert_eq!(std::fs::read_to_string(output).unwrap(), "codex,claude");
    }

    #[tokio::test]
    async fn lifecycle_coordinator_reports_queue_only_while_waiting() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let coordinator = Arc::new(ToolLifecycleCoordinator::default());
        let queued = Arc::new(AtomicUsize::new(0));
        let notice = |count: &Arc<AtomicUsize>| {
            let count = count.clone();
            move || {
                count.fetch_add(1, Ordering::SeqCst);
            }
        };

        // 没有别的写入在跑时直接执行，不显示排队
        coordinator
            .run_with_queue_notice(vec!["codex"], notice(&queued), |_| Ok(()))
            .await
            .unwrap();
        assert_eq!(queued.load(Ordering::SeqCst), 0);

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel::<()>();
        let first = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .run(vec!["codex"], move |_| {
                        started_tx.send(()).unwrap();
                        finish_rx.recv().unwrap();
                        Ok(())
                    })
                    .await
            })
        };
        started_rx.await.unwrap();
        let second = coordinator.run_with_queue_notice(vec!["claude"], notice(&queued), |_| Ok(()));
        tokio::pin!(second);
        assert!(futures::poll!(second.as_mut()).is_pending());
        assert_eq!(queued.load(Ordering::SeqCst), 1);

        finish_tx.send(()).unwrap();
        first.await.unwrap().unwrap();
        second.await.unwrap();
        assert_eq!(queued.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn lifecycle_coordinator_keeps_running_locks_when_caller_is_cancelled() {
        let coordinator = Arc::new(ToolLifecycleCoordinator::default());
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let first = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move {
                coordinator
                    .run(vec!["codex"], move |_| {
                        started_tx.send(()).unwrap();
                        finish_rx.recv().unwrap();
                        Ok(())
                    })
                    .await
            })
        };
        started_rx.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(coordinator
            .run(vec!["codex"], |_| panic!(
                "running duplicate must not execute"
            ))
            .await
            .is_err());

        {
            let cancelled = coordinator.run(vec!["claude"], |_| {
                panic!("cancelled queued operation must not execute")
            });
            tokio::pin!(cancelled);
            assert!(futures::poll!(cancelled.as_mut()).is_pending());
        }
        // 尚未开始的排队请求取消后可以重试，但仍须等待正在运行的子任务退出。
        let retry = coordinator.run(vec!["claude"], |_| Ok(()));
        tokio::pin!(retry);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        finish_tx.send(()).unwrap();
        retry.await.unwrap();
        coordinator.run(vec!["codex"], |_| Ok(())).await.unwrap();
    }

    #[tokio::test]
    async fn lifecycle_coordinator_releases_locks_after_error_or_panic() {
        let coordinator = ToolLifecycleCoordinator::default();
        assert_eq!(
            coordinator
                .run(vec!["claude"], |_| Err("installer failed".to_string()))
                .await,
            Err("installer failed".to_string())
        );
        coordinator.run(vec!["claude"], |_| Ok(())).await.unwrap();
        assert!(coordinator
            .run(vec!["claude"], |_| panic!("installer panicked"))
            .await
            .unwrap_err()
            .contains("tool lifecycle task join error"));
        coordinator.run(vec!["claude"], |_| Ok(())).await.unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn concurrent_lifecycle_runs_use_independent_scripts() {
        let output_dir = tempfile::tempdir().unwrap();
        let outputs = [
            output_dir.path().join("first.txt"),
            output_dir.path().join("second.txt"),
        ];
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for output in &outputs {
                let barrier = &barrier;
                scope.spawn(move || {
                    let command = format!("@echo off\r\n>\"{}\" echo %~f0\r\n", output.display());
                    barrier.wait();
                    let progress = ToolLifecycleProgress::noop();
                    run_tool_lifecycle_silently(&command, "tool_update", &progress).unwrap();
                });
            }
        });
        let script_paths =
            outputs.map(|output| PathBuf::from(std::fs::read_to_string(output).unwrap().trim()));
        assert_ne!(script_paths[0], script_paths[1]);
        for script_path in script_paths {
            assert!(!script_path.exists(), "temporary script must be cleaned up");
            assert!(
                !script_path.parent().unwrap().exists(),
                "temporary directory must be cleaned up"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_lifecycle_restores_user_path_for_npm_and_its_child() {
        let root = tempfile::tempdir().unwrap();
        let npm_dir = root.path().join("npm 用户 & tools");
        let node_dir = root.path().join("node 用户 tools");
        std::fs::create_dir_all(&npm_dir).unwrap();
        std::fs::create_dir_all(&node_dir).unwrap();
        std::fs::write(
            npm_dir.join("npm.cmd"),
            "@echo off\r\ncmd /D /C cc_switch_lifecycle_test_node.cmd\r\nexit /b %errorlevel%\r\n",
        )
        .unwrap();
        std::fs::write(
            node_dir.join("cc_switch_lifecycle_test_node.cmd"),
            "@echo restored-user-path\r\n@exit /b 0\r\n",
        )
        .unwrap();
        let script = root.path().join("update.bat");
        std::fs::write(&script, "@echo off\r\ncall npm --version\r\n").unwrap();

        // Model an MSI-launched process with only the system directory on PATH.
        // Inject the registry contribution without modifying the real registry
        // or the test runner's process environment.
        let system_path = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let inherited = system_path.to_str().unwrap();
        let user_path = format!("{};{}", npm_dir.display(), node_dir.display());
        let effective = merge_path_segments_win(&[inherited, &user_path]);
        let parent_path = std::env::var_os("PATH");

        let missing = windows_lifecycle_command(&script, inherited)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(
            !missing.status.success(),
            "npm must be absent on the narrow PATH"
        );

        let restored = windows_lifecycle_command(&script, &effective)
            .current_dir(root.path())
            .output()
            .unwrap();
        assert!(restored.status.success(), "{restored:?}");
        assert_eq!(
            decode_command_output(&restored.stdout).trim(),
            "restored-user-path"
        );
        assert_eq!(std::env::var_os("PATH"), parent_path);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_lifecycle_preserves_process_path_priority_and_failure() {
        let root = tempfile::tempdir().unwrap();
        let preferred = root.path().join("process tools");
        let fallback = root.path().join("registry tools");
        std::fs::create_dir_all(&preferred).unwrap();
        std::fs::create_dir_all(&fallback).unwrap();
        std::fs::write(
            preferred.join("npm.cmd"),
            "@echo process-npm-failed 1>&2\r\n@exit /b 23\r\n",
        )
        .unwrap();
        std::fs::write(
            fallback.join("npm.cmd"),
            "@echo registry-npm\r\n@exit /b 0\r\n",
        )
        .unwrap();
        let script = root.path().join("update.bat");
        std::fs::write(&script, "@echo off\r\ncall npm --version\r\n").unwrap();
        let effective =
            merge_path_segments_win(&[preferred.to_str().unwrap(), fallback.to_str().unwrap()]);

        // 断言整段 stderr：临时目录在 \\wsl.localhost 下时（WSL2 nightly）以它为工作目录，
        // cmd 会先往 stderr 打一段「UNC paths are not supported」，所以换成系统目录
        let output = windows_lifecycle_command(&script, &effective)
            .current_dir(std::env::var_os("SystemRoot").unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(23));
        assert_eq!(
            finish_lifecycle_output(&output).unwrap_err(),
            "process-npm-failed"
        );
    }

    /// npm bin 路径冲突（#9173）：EEXIST + File exists 行齐备时，错误详情后
    /// 追加指出冲突路径与安全处理方式的提示，且不建议盲目 --force。
    #[test]
    fn npm_bin_collision_appends_actionable_hint() {
        let raw = "npm error code EEXIST\n\
                   npm error path /home/user/.local/bin/pi\n\
                   npm error EEXIST: file already exists\n\
                   npm error File exists: /home/user/.local/bin/pi\n\
                   npm error Remove the existing file and try again, or run npm\n\
                   npm error with --force to overwrite files recklessly.";
        let detail = append_npm_bin_collision_hint("npm error code EEXIST".to_string(), raw);
        assert!(detail.contains("/home/user/.local/bin/pi"));
        assert!(detail.contains("/home/user/.local/bin/pi.bak"));
        assert!(detail.contains("Do not use npm --force"));
    }

    /// Windows 的 npm 输出同样带 `File exists:` 行（含盘符路径），应能提取。
    #[test]
    fn npm_bin_collision_extracts_windows_path() {
        let raw = "npm error code EEXIST\nnpm error File exists: C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd";
        assert_eq!(
            npm_bin_collision_path(raw),
            Some("C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd")
        );
    }

    /// 只有 EEXIST 字样、没有 File exists 行的失败不误判，原样返回。
    #[test]
    fn npm_bin_collision_ignores_other_failures() {
        let raw = "some tool failed with EEXIST while writing its cache";
        let detail = "cache write failed".to_string();
        assert_eq!(append_npm_bin_collision_hint(detail.clone(), raw), detail);
        assert_eq!(npm_bin_collision_path("npm error code E404"), None);
    }

    #[test]
    fn npm_bin_collision_detail_survives_trailing_noise() {
        let raw = [
            "npm error code EEXIST",
            "npm error path /home/me/.local/bin/pi",
            "npm error EEXIST: file already exists",
            "npm error File exists: /home/me/.local/bin/pi",
            "npm error Remove the existing file and try again, or run npm",
            "npm error with --force to overwrite files recklessly.",
            "npm notice",
            "npm notice New major version of npm available! 10.9.2 -> 11.6.0",
            "npm notice Changelog: https://github.com/npm/cli/releases/tag/v11.6.0",
            "npm notice To update run: npm install -g npm@11.6.0",
            "npm notice",
            "npm error A complete log of this run can be found in: /tmp/debug-0.log",
        ]
        .join("\n");
        assert_eq!(
            npm_bin_collision_detail(&raw).as_deref(),
            Some("npm error code EEXIST\nnpm error File exists: /home/me/.local/bin/pi")
        );
        assert_eq!(npm_bin_collision_detail("npm error code E404"), None);
        assert_eq!(npm_bin_collision_detail("EEXIST without a path"), None);
    }

    #[test]
    fn grok_lifecycle_metadata_is_consistent() {
        let requested = vec!["unsupported".to_string(), "grok".to_string()];
        assert_eq!(normalize_requested_tools(&requested), vec!["grok"]);
        assert_eq!(tool_spec("grok").npm_package, Some("@xai-official/grok"));
        assert_eq!(
            tool_spec("grok").npm_install_command().as_deref(),
            Some(
                "npm i -g @xai-official/grok@latest --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok"
            )
        );
        assert_eq!(tool_spec("grok").official_update_args, Some("update"));

        for action in [ToolLifecycleAction::Install, ToolLifecycleAction::Update] {
            assert_eq!(
                tool_action_shell_command_for_shell("grok", action, LifecycleCommandShell::Posix),
                tool_spec("grok").npm_install_command()
            );
        }

        // Static update remains package-manager based. Only a path positively
        // identified as xAI's native install may run `grok update`.
        assert_eq!(
            tool_action_shell_command_for_shell(
                "grok",
                ToolLifecycleAction::Update,
                LifecycleCommandShell::WindowsBatch,
            ),
            tool_spec("grok").npm_install_command()
        );
    }

    #[test]
    fn dsh_lifecycle_uses_npm_and_keeps_engine_checks_for_anchored_updates() {
        let expected = "npm i -g @deepseek-ai/dsh@latest --engine-strict";
        assert_eq!(posix_install_command_for("dsh"), expected);
        assert_eq!(tool_spec("dsh").npm_package, Some("@deepseek-ai/dsh"));
        for action in [ToolLifecycleAction::Install, ToolLifecycleAction::Update] {
            assert_eq!(
                tool_action_shell_command_for_shell("dsh", action, LifecycleCommandShell::Posix)
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(
                tool_action_shell_command_for_shell(
                    "dsh",
                    action,
                    LifecycleCommandShell::WindowsBatch
                )
                .as_deref(),
                Some(expected)
            );
        }
        assert_eq!(tool_spec("dsh").official_update_args, None);
        assert_eq!(tool_spec("dsh").npm_install_args, " --engine-strict");
    }

    #[test]
    fn kimi_install_uses_bundled_runtime_instead_of_system_node() {
        let posix = tool_action_shell_command_for_shell(
            "kimi",
            ToolLifecycleAction::Install,
            LifecycleCommandShell::Posix,
        )
        .unwrap();
        assert_eq!(posix, KIMI_INSTALL_UNIX);
        assert_eq!(posix_install_command_for("kimi"), posix);
        assert!(!posix.contains("npm"));
        let update = tool_action_shell_command_for_shell(
            "kimi",
            ToolLifecycleAction::Update,
            LifecycleCommandShell::Posix,
        )
        .unwrap();
        assert_eq!(update, format!("kimi upgrade --yes || {posix}"));
        assert!(!update.contains("npm"));
    }

    #[test]
    fn kimi_native_install_root_follows_the_installer_layout() {
        assert_eq!(
            kimi_native_install_root(
                "/home/me/.kimi-code/bin/kimi",
                "/home/me/.kimi-code/bin/kimi"
            )
            .as_deref(),
            Some("/home/me/.kimi-code")
        );
        // 软链指回安装目录时写回真身那处。
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
        // npm 版不归 installer 管。
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
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn kimi_installer_update_command_targets_the_detected_dir() {
        assert_eq!(
            kimi_installer_update_command("/home/me/my kimi"),
            format!("KIMI_INSTALL_DIR='/home/me/my kimi' {KIMI_INSTALL_UNIX}")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn kimi_windows_install_is_noninteractive_and_has_no_npm_fallback() {
        use base64::Engine as _;
        let command = tool_action_shell_command_for_shell(
            "kimi",
            ToolLifecycleAction::Install,
            LifecycleCommandShell::WindowsBatch,
        )
        .unwrap();
        assert!(command.starts_with("powershell -NoProfile -NonInteractive "));
        let encoded = command.split("-EncodedCommand ").nth(1).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let script = String::from_utf16(
            &bytes
                .chunks_exact(2)
                .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert_eq!(script, KIMI_INSTALL_WINDOWS_SCRIPT);
        assert!(!command.contains("npm"));
        assert_eq!(
            wsl_tool_action_shell_command("kimi", ToolLifecycleAction::Install).as_deref(),
            Some(KIMI_INSTALL_UNIX)
        );
    }

    #[test]
    fn pi_lifecycle_metadata_matches_pinned_distribution() {
        let requested = vec!["unsupported".to_string(), "pi".to_string()];
        assert_eq!(normalize_requested_tools(&requested), vec!["pi"]);
        assert_eq!(
            tool_spec("pi").npm_package,
            Some("@earendil-works/pi-coding-agent")
        );
        assert_eq!(
            tool_spec("pi").npm_install_command().as_deref(),
            Some("npm i -g @earendil-works/pi-coding-agent@latest")
        );
        // The verified distribution exposes `pi --version`, but no updater
        // contract is assumed; upgrades stay on the package-manager path.
        assert_eq!(tool_spec("pi").official_update_args, None);
    }

    #[test]
    fn omp_lifecycle_metadata_keeps_one_package() {
        let requested = vec!["unsupported".to_string(), "omp".to_string()];
        assert_eq!(normalize_requested_tools(&requested), vec!["omp"]);
        // The version check reads this package; npm cannot run it (the entry is
        // a bun script), so there is no npm install command at all.
        assert_eq!(
            tool_spec("omp").npm_package,
            Some("@oh-my-pi/pi-coding-agent")
        );
        assert_eq!(tool_spec("omp").npm_install_command(), None);
        // `omp update` routes every install source itself and never prompts.
        assert_eq!(tool_spec("omp").official_update_args, Some("update"));
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(install_command_for("omp"), OMP_INSTALL_UNIX);
            assert_eq!(
                tool_action_shell_command_for_shell(
                    "omp",
                    ToolLifecycleAction::Update,
                    LifecycleCommandShell::Posix
                )
                .as_deref(),
                Some(format!("omp update || {OMP_INSTALL_UNIX}").as_str())
            );
        }
    }

    #[test]
    fn mcode_lifecycle_metadata_skips_non_tty_self_update() {
        let requested = vec!["unsupported".to_string(), "mcode".to_string()];
        assert_eq!(normalize_requested_tools(&requested), vec!["mcode"]);
        // npm 上同名的 `mcode` 是无关项目，官方包是 scoped 的 @minimax-ai/code。
        assert_eq!(tool_spec("mcode").npm_package, Some("@minimax-ai/code"));
        let npm = tool_spec("mcode").npm_install_command().unwrap();
        assert!(
            npm.contains("\"--allow-scripts=@minimax-ai/code,better-sqlite3\""),
            "npm 12 blocks better-sqlite3's install script without it: {npm}"
        );

        // `mcode update` exits 0 without installing when stdin/stdout are not a
        // TTY, which is exactly how silent lifecycle actions run it.
        assert_eq!(tool_spec("mcode").official_update_args, None);
        for shell in [
            LifecycleCommandShell::Posix,
            LifecycleCommandShell::WindowsBatch,
        ] {
            assert_eq!(
                tool_action_shell_command_for_shell("mcode", ToolLifecycleAction::Update, shell),
                Some(npm.clone())
            );
        }
    }

    #[test]
    fn mcode_install_root_candidate_follows_platform_layout() {
        assert_eq!(
            mcode_install_root_candidate("/opt/mcode/bin/mcode", McodeLayout::Posix),
            "/opt/mcode"
        );
        // 根目录本身叫 bin：POSIX 只去掉入口所在的那一层 bin。
        assert_eq!(
            mcode_install_root_candidate("/opt/bin/bin/mcode", McodeLayout::Posix),
            "/opt/bin"
        );
        // Windows 入口直接在根目录，根目录叫 bin 也不能再上跳一级。
        assert_eq!(
            mcode_install_root_candidate("C:\\Users\\me\\bin\\mcode.cmd", McodeLayout::Windows),
            "C:\\Users\\me\\bin"
        );
    }

    #[test]
    fn mcode_script_install_root_matches_only_installer_layout() {
        assert_eq!(
            mcode_script_install_root(
                "/Users/me/.minimax-code/bin/mcode",
                "/Users/me/.minimax-code/bin/mcode",
                McodeLayout::Posix,
            )
            .as_deref(),
            Some("/Users/me/.minimax-code")
        );
        assert_eq!(
            mcode_script_install_root(
                "C:\\Users\\me\\.minimax-code\\mcode.cmd",
                "C:\\Users\\me\\.minimax-code\\mcode.cmd",
                McodeLayout::Windows,
            )
            .as_deref(),
            Some("C:\\Users\\me\\.minimax-code")
        );
        // 用户自建软链：入口不在安装目录，但真身是 launcher。
        assert_eq!(
            mcode_script_install_root(
                "/Users/me/.local/bin/mcode",
                "/Users/me/.minimax-code/bin/mcode",
                McodeLayout::Posix,
            )
            .as_deref(),
            Some("/Users/me/.minimax-code")
        );
        // 全局 npm 安装、以及同名数据目录 `~/.minimax` 都不是 installer 布局。
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

    /// 在 tempdir 下按 `layout` 搭一个自定义 `MCODE_INSTALL_DIR`（`<tmp>/apps/<root_name>`，
    /// 目录名不是 `.minimax-code`）的 installer 布局，返回 `(TempDir, root, 入口路径)`。
    fn mcode_custom_install(
        root_name: &str,
        layout: McodeLayout,
    ) -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("apps").join(root_name);
        let entry = match layout {
            McodeLayout::Posix => root.join("bin").join("mcode"),
            McodeLayout::Windows => root.join("mcode.cmd"),
        };
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "").unwrap();
        (
            dir,
            root.to_string_lossy().to_string(),
            entry.to_string_lossy().to_string(),
        )
    }

    fn write_mcode_receipt(root: &str, product: &str, owner: &str, prefix: &str, bom: bool) {
        let json = serde_json::json!({
            "schemaVersion": 2,
            "product": product,
            "updateOwner": owner,
            "prefix": prefix,
        })
        .to_string();
        let text = if bom { format!("\u{feff}{json}") } else { json };
        std::fs::write(Path::new(root).join("install.json"), text).unwrap();
    }

    #[test]
    fn mcode_receipt_claims_custom_install_dir() {
        for (root_name, layout) in [
            ("mcode", McodeLayout::Posix),
            ("bin", McodeLayout::Posix),
            ("mcode", McodeLayout::Windows),
            // MCODE_INSTALL_DIR=C:\Users\me\bin：入口 `<root>\mcode.cmd` 的父目录名就是 bin。
            ("bin", McodeLayout::Windows),
        ] {
            let (_dir, root, entry) = mcode_custom_install(root_name, layout);
            write_mcode_receipt(&root, "minimax-code", "npm-prefix", &root, false);
            assert_eq!(
                mcode_receipt_install_root(&entry, layout).as_deref(),
                Some(root.as_str()),
                "{root_name} / {layout:?}"
            );
        }

        // 端到端：单处安装也必须原地重跑 installer，不能退到全局 `npm i -g`。
        let (_dir, root, entry) = mcode_custom_install("mcode", McodeLayout::NATIVE);
        write_mcode_receipt(&root, "minimax-code", "npm-prefix", &root, false);
        let installs = vec![ToolInstallation {
            path: entry.clone(),
            version: Some("0.5.3".to_string()),
            runnable: true,
            error: None,
            exited_with_error: false,
            source: infer_install_source(Path::new(&entry)).to_string(),
            is_path_default: false,
            real: PathBuf::from(&entry),
        }];
        assert_eq!(
            installs_anchored_command("mcode", &installs),
            Some(mcode_installer_update_command(&root))
        );
    }

    #[test]
    fn mcode_receipt_tolerates_utf8_bom() {
        // install.ps1 在 Windows PowerShell 5.1 下用 `Set-Content -Encoding UTF8` 写回执，带 BOM。
        let (_dir, root, entry) = mcode_custom_install("mcode", McodeLayout::Windows);
        write_mcode_receipt(&root, "minimax-code", "npm-prefix", &root, true);
        assert_eq!(
            mcode_receipt_install_root(&entry, McodeLayout::Windows).as_deref(),
            Some(root.as_str())
        );
    }

    #[test]
    fn mcode_receipt_rejects_foreign_or_mismatched_receipts() {
        let layout = McodeLayout::NATIVE;
        // 没有回执：不认领。
        let (_dir, _root, entry) = mcode_custom_install("mcode", layout);
        assert_eq!(mcode_receipt_install_root(&entry, layout), None);

        // 不是 MiniMax Code 的回执、或不归 npm-prefix 管：不认领。
        for (product, owner) in [
            ("other", "npm-prefix"),
            ("minimax-code", "managed-installer"),
        ] {
            let (_dir, root, entry) = mcode_custom_install("mcode", layout);
            write_mcode_receipt(&root, product, owner, &root, false);
            assert_eq!(
                mcode_receipt_install_root(&entry, layout),
                None,
                "{product}/{owner}"
            );
        }

        // prefix 指向别的目录（拷贝来的回执）：不认领。
        let elsewhere = tempfile::tempdir().unwrap();
        let (_dir, root, entry) = mcode_custom_install("mcode", layout);
        write_mcode_receipt(
            &root,
            "minimax-code",
            "npm-prefix",
            &elsewhere.path().to_string_lossy(),
            false,
        );
        assert_eq!(mcode_receipt_install_root(&entry, layout), None);
    }

    /// `parent_dir` 是锚定层"由 bin 路径推导同目录绝对路径"的基石,跨平台共用——
    /// 这里固化 `\`/`/`/混合分隔符/根边界四种情况,避免未来重构悄悄改语义。
    mod parent_dir_cases {
        use super::super::*;

        #[test]
        fn unix_path() {
            assert_eq!(
                parent_dir("/Users/me/.volta/bin/codex"),
                "/Users/me/.volta/bin"
            );
        }

        #[test]
        fn windows_backslash() {
            assert_eq!(
                parent_dir("C:\\Users\\me\\AppData\\Local\\Volta\\bin\\codex.exe"),
                "C:\\Users\\me\\AppData\\Local\\Volta\\bin"
            );
        }

        #[test]
        fn mixed_separators_takes_rightmost() {
            // Windows 上 `Path::join` 与字符串拼接可能产出混合分隔符;取**两种之中最右
            // 出现**的位置,而非"优先 `\`"——后者在混合时会取错父目录。
            assert_eq!(
                parent_dir("C:\\Users\\me/Code/openclaw\\codex.cmd"),
                "C:\\Users\\me/Code/openclaw"
            );
        }

        #[test]
        fn no_separator_returns_empty() {
            // 无父目录 → 空串,锚定层据此返 None、回退静态命令。
            assert_eq!(parent_dir("codex"), "");
        }

        #[test]
        fn separator_at_root_returns_empty() {
            // `/codex`:根目录是 index 0,`i > 0` 不满足 → 空串。同款行为对 Windows
            // 上的 `\codex` 也成立(实际不会出现,但语义对齐)。
            assert_eq!(parent_dir("/codex"), "");
            assert_eq!(parent_dir("\\codex"), "");
        }
    }

    /// Codex 官方独立安装器的识别是纯字符串判定,POSIX / Windows 布局都在这里固化。
    mod codex_standalone_detection {
        use super::super::*;

        #[test]
        fn posix_default_layout() {
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
        }

        #[test]
        fn windows_verbatim_real_is_restored() {
            // canonicalize 出的 `\\?\` 前缀要剥掉再交给 installer;大小写保持原样。
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
        }

        #[test]
        fn windows_default_entry_without_resolved_real() {
            // canonicalize 失败时 real 就是入口本身:仍按默认入口目录认出,只是不传 CODEX_HOME。
            let bin = r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe";
            assert_eq!(
                codex_standalone_install(bin, bin),
                Some(CodexStandaloneInstall {
                    install_dir: r"C:\Users\me\AppData\Local\Programs\OpenAI\Codex\bin".to_string(),
                    codex_home: None,
                })
            );
        }

        #[test]
        fn other_install_channels_are_not_standalone() {
            for (bin, real) in [
                (
                    "/Users/me/.nvm/versions/node/v22.19.0/bin/codex",
                    "/Users/me/.nvm/versions/node/v22.19.0/lib/node_modules/@openai/codex/bin/codex.js",
                ),
                (
                    "/opt/homebrew/bin/codex",
                    "/opt/homebrew/Caskroom/codex/0.157.0/bin/codex",
                ),
                // app-server 守护进程的发布包不是 PATH 上的 CLI。
                (
                    "/Users/me/.local/bin/codex",
                    "/Users/me/.codex/packages/app-server-daemon/current/bin/codex",
                ),
                (r"C:\Users\me\AppData\Roaming\npm\codex.cmd", r"C:\Users\me\AppData\Roaming\npm\codex.cmd"),
            ] {
                assert_eq!(codex_standalone_install(bin, real), None, "{bin}");
            }
        }
    }

    /// Windows-only 锚定升级回归(等价类压缩到 3 种 idiom:volta/pnpm/npm)。整块通过
    /// `cfg(target_os = "windows")` gate,在 macOS/Linux 上不参与 cargo test;Windows
    /// CI 跑全套验证。tempdir 模拟 sibling 入口存在/不存在,锁定"扩展名顺序优先级 +
    /// 含空格路径自动加双引号 + 探不到 sibling → None 退静态"三件事。
    #[cfg(target_os = "windows")]
    mod anchored_upgrade_windows {
        use super::super::*;

        /// 在 tempdir 下创建子目录 `subdir`(空字符串则用 tempdir 根),放入 `entry`
        /// 与若干 `siblings` 假文件。返回 `(TempDir, 子目录, 入口绝对路径)`——TempDir
        /// 必须保活,否则析构后 fs 文件消失、`is_file()` 失败,测试假绿。
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

        /// **必须与 `win_quote_path_for_batch` 主体保持镜像**——给 anchored 测试动态算
        /// expected,让用例在 temp 根目录含空格 / `&` / `(` / `%` 等特殊字符的开发机上
        /// 也能通过(默认 Windows `%TEMP%` = `C:\Users\<user>\AppData\Local\Temp`,
        /// 用户名带空格的机器整条 path 含空格、生产代码会正确加引号、测试硬编码无引号
        /// expected 会假失败)。
        ///
        /// 镜像引入"两边必须同步"的隐性依赖——回归防护层是 `win_quote_*` 那 7 个独立
        /// 单测,它们用硬编码字面值锁住 quoting 规则本身,即便此镜像漂移也会被那一组
        /// 测试 catch;反之亦然。
        fn expect_quoted_path(p: &str) -> String {
            let escaped = p.replace('%', "%%%%");
            let needs_quote = p
                .chars()
                .any(|c| matches!(c, ' ' | '&' | '(' | ')' | '^' | ';' | '<' | '>' | '|' | ','));
            if needs_quote {
                format!("\"{escaped}\"")
            } else {
                escaped
            }
        }

        #[test]
        fn volta_windows_uses_volta_install() {
            // tempdir 路径里不含 "volta" 子串,所以在 tempdir 下手建一个 `Volta` 子目录
            // 才能让 `infer_install_source` 通过路径 normalize 后命中 `/volta/` 分支。
            // sibling 候选顺序 `[exe, cmd]`——Volta 是 Rust 写的 native binary,首选 .exe。
            // expected 通过 `expect_quoted_path` 算出,以适应 temp 根目录含特殊字符的环境。
            let (_dir, sub, bin_path) = setup_sibling("Volta", "codex.cmd", &["volta.exe"]);
            let cmd = anchored_command_from_paths("codex", &bin_path, &bin_path);
            let volta_full = format!("{}\\volta.exe", sub.to_string_lossy());
            // codex 自 5092fe51 起不在 prefers_official_update：直接锚定包管理器，无 `codex update ||` 前缀。
            let expected = format!("{} install @openai/codex", expect_quoted_path(&volta_full));
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn pnpm_windows_uses_pnpm_add() {
            // bin_path 落 `%LOCALAPPDATA%\pnpm\codex.cmd`,sibling 有 `pnpm.cmd` → 锚定到
            // `<dir>\pnpm.cmd add -g @openai/codex@latest`。用 add+@latest 而非 update,
            // 兼容"之前没通过 pnpm 装过"的幂等性场景。
            let (_dir, sub, bin_path) = setup_sibling("pnpm", "codex.cmd", &["pnpm.cmd"]);
            let cmd = anchored_command_from_paths("codex", &bin_path, &bin_path);
            let pnpm_full = format!("{}\\pnpm.cmd", sub.to_string_lossy());
            // codex 自 5092fe51 起不在 prefers_official_update：直接锚定包管理器，无 `codex update ||` 前缀。
            let expected = format!(
                "{} add -g @openai/codex@latest",
                expect_quoted_path(&pnpm_full)
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn opencode_windows_uses_package_fallback_without_official_upgrade() {
            let (_dir, sub, bin_path) = setup_sibling("pnpm", "opencode.cmd", &["pnpm.cmd"]);
            let cmd = anchored_command_from_paths("opencode", &bin_path, &bin_path);
            let pnpm_full = format!("{}\\pnpm.cmd", sub.to_string_lossy());
            let expected = format!(
                "{} add -g opencode-ai@latest",
                expect_quoted_path(&pnpm_full)
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn opencode_windows_static_fallback_skips_official_upgrade() {
            let cmd = static_fallback_command("opencode");
            assert_eq!(cmd, tool_spec("opencode").npm_install_command().unwrap());
            assert!(!cmd.contains("opencode upgrade"));
        }

        #[test]
        fn npm_windows_default_branch() {
            // 任意 system 类路径(不命中 volta/pnpm)→ 兜底 sibling npm.cmd 锚定。
            // 模拟 nvm-windows 的实际形态:`<NVM_HOME>\v22.0.0\codex.cmd`。
            let (_dir, sub, bin_path) = setup_sibling("v22.0.0", "codex.cmd", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("codex", &bin_path, &bin_path);
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            // codex 自 5092fe51 起不在 prefers_official_update：直接锚定包管理器，无 `codex update ||` 前缀。
            let expected = format!(
                "{} i -g @openai/codex@latest",
                expect_quoted_path(&npm_full)
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn grok_windows_anchors_to_sibling_npm() {
            let (_dir, sub, bin_path) = setup_sibling("v22.0.0", "grok.cmd", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("grok", &bin_path, &bin_path);
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            let expected = format!(
                "{} i -g @xai-official/grok@latest{}",
                expect_quoted_path(&npm_full),
                tool_spec("grok").npm_install_args
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn grok_native_windows_uses_self_update_with_installer_fallback() {
            // sibling 有 npm.cmd 也**不能**拿它当 fallback:`grok update` 本身就是靠 npm
            // 分发的(见 grok_native_update_command doc),npm fallback 与 primary 同源、
            // 会一起失败。fallback 必须是官方 PowerShell installer —— 唯一不经 npm 的路径。
            let (_dir, _sub, bin_path) = setup_sibling(".grok/bin", "grok.exe", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("grok", &bin_path, &bin_path).unwrap();
            let expected = format!(
                "{} update || {}",
                expect_quoted_path(&bin_path),
                grok_install_windows_command()
            );
            assert_eq!(cmd, expected);
            // fallback 是 powershell.exe 不是 .cmd/.bat —— `||` 右侧不该有 `call`。
            assert!(
                !cmd.contains("|| call"),
                "powershell needs no `call`: {cmd}"
            );
            assert!(!cmd.contains("npm"), "npm must not be the fallback: {cmd}");
        }

        #[test]
        fn mcode_script_install_windows_reruns_installer_in_place() {
            // sibling 有 npm.cmd 也不能用：脚本安装不归全局 npm 管，npm 会装到另一处。
            let (_dir, sub, bin_path) =
                setup_sibling(".minimax-code", "mcode.cmd", &["mcode.ps1", "npm.cmd"]);
            let cmd = anchored_command_from_paths("mcode", &bin_path, &bin_path).unwrap();
            assert_eq!(cmd, mcode_installer_update_command(&sub.to_string_lossy()));
            assert!(!cmd.contains("npm"), "npm must not be used: {cmd}");
        }

        #[test]
        fn mcode_windows_installer_update_pins_install_root() {
            let cmd = mcode_installer_update_command("C:\\Users\\o'brien\\.minimax-code");
            let expected = powershell_encoded_command(
                "$env:MCODE_INSTALL_DIR = 'C:\\Users\\o''brien\\.minimax-code'; irm https://filecdn.minimax.chat/public/install.ps1 | iex",
            );
            assert_eq!(
                cmd.split_once("-EncodedCommand ")
                    .map(|(_, encoded)| encoded),
                Some(expected.as_str())
            );
        }

        #[test]
        fn codex_standalone_windows_reruns_installer_in_place() {
            // #7650:独立安装的 codex.exe 旁边没有 npm.cmd,此前退回 `call npm i -g`
            // 另装一份 npm 版。
            let bin = r"C:\Users\o'brien\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe";
            let real = r"\\?\C:\Users\o'brien\.codex\packages\standalone\releases\0.157.0-x86_64-pc-windows-msvc\bin\codex.exe";
            let cmd = anchored_command_from_paths("codex", bin, real).unwrap();
            let expected = powershell_encoded_command(
                "$env:CODEX_NON_INTERACTIVE = '1'; $env:CODEX_INSTALL_DIR = 'C:\\Users\\o''brien\\AppData\\Local\\Programs\\OpenAI\\Codex\\bin'; $env:CODEX_HOME = 'C:\\Users\\o''brien\\.codex'; irm https://chatgpt.com/codex/install.ps1 | iex",
            );
            assert_eq!(
                cmd.split_once("-EncodedCommand ")
                    .map(|(_, encoded)| encoded),
                Some(expected.as_str())
            );
            assert!(!cmd.contains("npm"), "npm must not be used: {cmd}");
        }

        #[test]
        fn mcode_windows_npm_install_keeps_allow_scripts() {
            let (_dir, sub, bin_path) = setup_sibling("v22.19.0", "mcode.cmd", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("mcode", &bin_path, &bin_path);
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            let expected = format!(
                "{} i -g @minimax-ai/code@latest{}",
                expect_quoted_path(&npm_full),
                tool_spec("mcode").npm_install_args
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn mcode_windows_install_prefers_powershell_with_npm_fallback() {
            let install = static_fallback_command_for("mcode", ToolLifecycleAction::Install);
            let native = mcode_install_windows_command();
            assert_eq!(
                install,
                format!(
                    "{native} || call {}",
                    tool_spec("mcode").npm_install_command().unwrap()
                )
            );
            let expected_encoded = powershell_encoded_command(MCODE_INSTALL_WINDOWS_SCRIPT);
            assert_eq!(
                native
                    .split_once("-EncodedCommand ")
                    .map(|(_, encoded)| encoded),
                Some(expected_encoded.as_str())
            );
        }

        #[test]
        fn grok_windows_install_prefers_powershell_with_npm_fallback() {
            let install = static_fallback_command_for("grok", ToolLifecycleAction::Install);
            let native = grok_install_windows_command();
            assert!(
                install.starts_with(&native),
                "native installer first: {install}"
            );
            assert!(
                install.ends_with(&format!(
                    "|| call {}",
                    tool_spec("grok").npm_install_command().unwrap()
                )),
                "npm fallback should remain available: {install}"
            );
            let expected_encoded = powershell_encoded_command(GROK_INSTALL_WINDOWS_SCRIPT);
            assert_eq!(
                native
                    .split_once("-EncodedCommand ")
                    .map(|(_, encoded)| encoded),
                Some(expected_encoded.as_str())
            );
        }

        #[test]
        fn windows_no_sibling_uses_cli_update_without_package_fallback() {
            // sibling 包管理器不存在(纯独立二进制)时,仍可锚定到 CLI 自身跑官方 update。
            // 只是没有包管理器 fallback。用 claude —— codex 自 5092fe51 起一律走 npm 锚定,
            // 已不再有官方 self-update 分支,故改用仍在 prefers_official_update 的 claude 覆盖此路径。
            let (_dir, _sub, bin_path) = setup_sibling("", "claude.cmd", &[]);
            let cmd = anchored_command_from_paths("claude", &bin_path, &bin_path);
            let expected = format!("{} update", expect_quoted_path(&bin_path));
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn hermes_windows_uses_cli_update() {
            // Hermes 自带 `hermes update`,不要再回退到 py/python/pip。即便同目录有
            // npm.cmd,也不应走 npm 分支。
            let (_dir, _sub, bin_path) = setup_sibling("", "hermes.exe", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("hermes", &bin_path, &bin_path);
            let expected = format!("{} update", expect_quoted_path(&bin_path));
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn hermes_windows_static_fallback_uses_powershell_installer_without_pip() {
            let install = static_fallback_command_for("hermes", ToolLifecycleAction::Install);
            assert!(
                install
                    .starts_with("powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand "),
                "should use PowerShell EncodedCommand installer: {install}"
            );
            let encoded = install
                .split_once("-EncodedCommand ")
                .map(|(_, encoded)| encoded)
                .expect("installer should include encoded command");
            assert_eq!(
                encoded,
                powershell_encoded_command(HERMES_INSTALL_WINDOWS_SCRIPT)
            );
            let install_prefix = install
                .split_once("-EncodedCommand ")
                .map(|(prefix, _)| prefix)
                .expect("installer should include encoded command");
            assert!(
                !install_prefix.contains("|")
                    && !install_prefix.contains("-Command")
                    && !install_prefix.contains("python")
                    && !install_prefix.contains("pip"),
                "should hide PowerShell pipe from cmd.exe and avoid system Python/pip: {install}"
            );

            let update = static_fallback_command("hermes");
            assert!(
                update.starts_with(
                    "hermes update || powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand "
                ),
                "should try CLI update before PowerShell installer: {update}"
            );
            let fallback = update
                .split_once("||")
                .map(|(_, fallback)| fallback)
                .expect("update should include a fallback command");
            let fallback_prefix = fallback
                .split_once("-EncodedCommand ")
                .map(|(prefix, _)| prefix)
                .expect("fallback should include encoded command");
            assert!(
                !fallback_prefix.contains('|')
                    && !fallback_prefix.contains("-Command")
                    && !update.contains("call powershell")
                    && !fallback_prefix.contains("python")
                    && !fallback_prefix.contains("pip"),
                "PowerShell fallback should be encoded, not called like a batch file or use pip: {update}"
            );
        }

        #[test]
        fn windows_path_with_space_is_double_quoted() {
            // 含空格的路径(`C:\Program Files\...`)在生成命令时必须用双引号包,否则
            // bat / cmd /C 解析会把第一个空格当 token 分隔符,后续参数串错。**精确等值断言
            // 锁定引号位置**(starts_with+contains 会放过"双引号位置错了但仍能命中"的回归)。
            let (_dir, sub, bin_path) = setup_sibling("Program Files", "codex.cmd", &["npm.cmd"]);
            let cmd = anchored_command_from_paths("codex", &bin_path, &bin_path);
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            // codex 走 npm 锚定(5092fe51),含空格的 npm 全路径仍必须被双引号包裹。
            let expected = format!(
                "{} i -g @openai/codex@latest",
                expect_quoted_path(&npm_full)
            );
            assert_eq!(cmd.as_deref(), Some(expected.as_str()));
        }

        #[test]
        fn windows_full_batch_line_for_percent_path_uses_quadruple_escape() {
            // **完整生成的 batch 行**(`call ` + anchored cmd)对含字面 `%` 的路径必须
            // 4 倍转义 `%foo%` → `%%%%foo%%%%`:.bat parser 一轮还原为 `%%foo%%`,call
            // 二轮再还原为 `%foo%` 字面。helper 单测验证的是 `win_quote_path_for_batch`
            // 内部转义,这条 integration 测验证 anchored_command_from_paths 输出 + call
            // 包装后,**最终落到 .bat 的字符串**仍然闭合两轮 expansion。
            let (_dir, sub, bin_path) = setup_sibling("path%foo%", "codex.cmd", &["npm.cmd"]);
            let anchored = anchored_command_from_paths("codex", &bin_path, &bin_path).unwrap();
            // build_tool_action_line Windows 分支最终拼的就是 `call <anchored>`(中间
            // 没有其他变换),这里直接用 format! 复刻那一步,无需暴露内部 API。
            let batch_line = format!("call {anchored}");
            // 用 `expect_quoted_path` 算 npm 全路径的期望 quoting,**同时覆盖 temp 根
            // 含空格的环境**(否则 sub 本身含空格 + 子目录 `path%foo%` 触发 4 倍 `%` 转义
            // 会让 expected 漏引号、假失败)。
            let npm_full = format!("{}\\npm.cmd", sub.to_string_lossy());
            // codex 走 npm 锚定(5092fe51)：batch 行 = `call <npm 全路径> i -g ...`，
            // 含字面 `%` 的 npm 路径仍须 4 倍转义。
            let expected = format!(
                "call {} i -g @openai/codex@latest",
                expect_quoted_path(&npm_full)
            );
            assert_eq!(batch_line, expected);
            // 双重锁定:确认 4 倍转义子串存在 + 不出现"残留的二倍转义或字面 `%foo%`"。
            assert!(
                batch_line.contains("%%%%foo%%%%"),
                "batch 行应含 4 倍转义 `%%%%foo%%%%`: {batch_line}"
            );
            assert!(
                !batch_line.contains("path%foo%"),
                "batch 行不应含未转义的字面 `%foo%`(会被 call 二次解析展开): {batch_line}"
            );
        }
    }

    /// Windows-only helpers 单测——在 macOS/Linux 上整块通过 cfg 排除,不参与 `cargo test`。
    /// Windows CI(或本机 Windows 跑 cargo test)会激活这些用例。覆盖:①双引号
    /// quoting 镜像 POSIX 版;②sibling_bin_with_ext 在 fs 上按 ext 顺序探到第一个存在的、
    /// 全部不存在/空 dir 时返 None。tempdir 提供干净 fs 沙盒。
    #[cfg(target_os = "windows")]
    mod windows_helpers {
        use super::super::*;

        #[test]
        fn win_quote_clean_path_stays_bare() {
            // 普通路径不含特殊字符 → 不加引号,命令展示干净。
            assert_eq!(
                win_quote_path_for_batch("C:\\Users\\me\\npm.cmd"),
                "C:\\Users\\me\\npm.cmd"
            );
        }

        #[test]
        fn win_quote_spaced_path_gets_quoted() {
            assert_eq!(
                win_quote_path_for_batch("C:\\Program Files\\nodejs\\npm.cmd"),
                "\"C:\\Program Files\\nodejs\\npm.cmd\""
            );
        }

        #[test]
        fn win_quote_ampersand_path_gets_quoted() {
            // `&` 是 cmd 命令分隔符,NTFS 允许在路径中出现;没有引号会让 `call C:\A&B\npm.cmd`
            // 被解析为 `call C:\A` + `B\npm.cmd` 两条命令,执行错乱。
            assert_eq!(
                win_quote_path_for_batch("C:\\Tools&Dev\\npm.cmd"),
                "\"C:\\Tools&Dev\\npm.cmd\""
            );
        }

        #[test]
        fn win_quote_parens_path_gets_quoted() {
            // `(` / `)` 在 .bat 中是代码块语义,引号内才是字面意义。
            assert_eq!(
                win_quote_path_for_batch("C:\\Foo(x86)\\npm.cmd"),
                "\"C:\\Foo(x86)\\npm.cmd\""
            );
        }

        #[test]
        fn win_quote_caret_path_gets_quoted() {
            // `^` 是 cmd 的 escape character;包引号后是字面意义。
            assert_eq!(
                win_quote_path_for_batch("C:\\foo^bar\\npm.cmd"),
                "\"C:\\foo^bar\\npm.cmd\""
            );
        }

        #[test]
        fn win_quote_percent_is_escaped_to_quadruple_percent() {
            // `%` 经历 .bat 一轮 + call 二轮 expansion,要让 call 最终看到字面 `%FOO%`
            // 需要源 .bat 里写 `%%%%FOO%%%%`(一轮 → `%%FOO%%`,二轮 → `%FOO%` 字面)。
            // 用 `%%` 二倍转义只在 echo / 直接执行场景对,call 调用时会被还原成 variable
            // reference 进而被替换。**这一条用例锁住"call 二次解析"必须被 4 倍转义闭合**。
            assert_eq!(
                win_quote_path_for_batch("C:\\path%foo%\\npm.cmd"),
                "C:\\path%%%%foo%%%%\\npm.cmd"
            );
        }

        #[test]
        fn win_quote_percent_with_space_gets_both() {
            // `%` 4 倍转义与外层引号正交——含空格触发引号、含 `%` 触发 `%%%%` 转义,叠加。
            assert_eq!(
                win_quote_path_for_batch("C:\\my %dir%\\npm.cmd"),
                "\"C:\\my %%%%dir%%%%\\npm.cmd\""
            );
        }

        #[test]
        fn win_quote_needs_quote_uses_original_path() {
            // 回归 guard:`needs_quote` 判定基于**原路径**,不能用 escape 后字符串——
            // 否则原本无 token 边界字符的路径(如 `C:\path%foo%\npm.cmd`)在 escape
            // 引入更多 `%` 后被错误识别成"需要引号"。这是实现 bug 的隐性入口。
            // 入参不含任何 token 边界字符 → 不应加外层引号、只做 `%` 4 倍转义。
            let out = win_quote_path_for_batch("C:\\foo%bar%\\npm.cmd");
            assert!(!out.starts_with('"'), "纯 `%` 路径不应加外层引号: {out}");
        }

        #[test]
        fn sibling_bin_picks_first_existing_extension() {
            // 同目录同时存在 `npm.cmd` 和 `npm.exe` 时,候选顺序 `[cmd, exe]` 应取 .cmd——
            // 这是 Node.js 官方 installer 装出来的 idiom(.cmd 是入口、.exe 是 wrapper)。
            let dir = tempfile::tempdir().unwrap();
            let cmd_path = dir.path().join("npm.cmd");
            let exe_path = dir.path().join("npm.exe");
            std::fs::write(&cmd_path, "").unwrap();
            std::fs::write(&exe_path, "").unwrap();

            let codex = dir.path().join("codex.cmd").to_string_lossy().to_string();
            let found = sibling_bin_with_ext(&codex, "npm", &["cmd", "exe"]).unwrap();
            assert_eq!(found, cmd_path.to_string_lossy());
        }

        #[test]
        fn sibling_bin_volta_prefers_exe() {
            // Volta 是 Rust 写的 native binary,扩展名顺序应是 [exe, cmd]——若只有 .exe
            // 存在(常见情形),探到的就是它。
            let dir = tempfile::tempdir().unwrap();
            let exe_path = dir.path().join("volta.exe");
            std::fs::write(&exe_path, "").unwrap();

            let codex = dir.path().join("codex.exe").to_string_lossy().to_string();
            let found = sibling_bin_with_ext(&codex, "volta", &["exe", "cmd"]).unwrap();
            assert_eq!(found, exe_path.to_string_lossy());
        }

        #[test]
        fn sibling_bin_returns_none_when_none_exist() {
            // 同目录下没有任何候选 → None,锚定层据此退到静态命令。
            let dir = tempfile::tempdir().unwrap();
            let codex = dir.path().join("codex.cmd").to_string_lossy().to_string();
            assert!(sibling_bin_with_ext(&codex, "npm", &["cmd", "exe"]).is_none());
        }

        #[test]
        fn sibling_bin_returns_none_when_no_parent() {
            // bin_path 没有目录部分(纯文件名) → parent_dir 空串 → 返 None。
            assert!(sibling_bin_with_ext("codex.cmd", "npm", &["cmd"]).is_none());
        }

        #[test]
        fn wsl_hermes_command_uses_unix_installer_not_powershell_or_pip() {
            // 跨 wsl.exe 边界后跑的是 Linux,Windows PowerShell installer 不适用;
            // 也不要再走 python3/python pip 链,避免 Python 版本/pyenv shim 问题。
            let update_cmd =
                wsl_tool_action_shell_command("hermes", ToolLifecycleAction::Update).unwrap();
            assert!(
                update_cmd.starts_with("hermes update || bash -c 'tmp=$(mktemp) && curl -fsSL "),
                "WSL hermes 更新应先尝试 CLI 自更新再回退官方 installer,得到: {update_cmd}"
            );
            let fallback = update_cmd
                .split_once("||")
                .map(|(_, fallback)| fallback)
                .expect("update should include installer fallback");
            assert!(
                !fallback.contains('|')
                    && fallback.contains(" -o $tmp && bash $tmp")
                    && !update_cmd.contains("powershell")
                    && !update_cmd.contains("pip"),
                "WSL hermes fallback 不能依赖 pipefail/Windows installer/pip,得到: {update_cmd}"
            );

            let install_cmd =
                wsl_tool_action_shell_command("hermes", ToolLifecycleAction::Install).unwrap();
            assert!(
                install_cmd.starts_with("bash -c 'tmp=$(mktemp) && curl -fsSL "),
                "WSL hermes 安装应直接走官方 Unix installer,得到: {install_cmd}"
            );
            assert!(
                !install_cmd.contains('|') && install_cmd.contains(" -o $tmp && bash $tmp"),
                "WSL hermes 安装不应依赖 pipefail,得到: {install_cmd}"
            );
        }

        #[test]
        fn wsl_hermes_install_line_does_not_depend_on_outer_pipefail() {
            let line = build_wsl_tool_action_line("Ubuntu", HERMES_INSTALL_UNIX, None, None)
                .expect("valid WSL command line");
            assert!(line.starts_with("wsl.exe -d Ubuntu -- sh -c "));
            assert!(
                !line.contains("| bash") && line.contains(" -o $tmp && bash $tmp"),
                "WSL 子 shell 内不能出现 curl 管道安装器: {line}"
            );
        }

        #[test]
        fn wsl_install_uses_posix_install_priority() {
            let claude =
                wsl_tool_action_shell_command("claude", ToolLifecycleAction::Install).unwrap();
            assert!(
                claude.starts_with("bash -c 'tmp=$(mktemp) && curl -fsSL https://claude.ai/install.sh ")
                    && claude.ends_with(" || npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"),
                "WSL claude install should prefer native POSIX installer with npm fallback: {claude}"
            );
            assert!(!claude.contains("| bash"));

            let opencode =
                wsl_tool_action_shell_command("opencode", ToolLifecycleAction::Install).unwrap();
            assert!(
                opencode.starts_with(
                    "bash -c 'tmp=$(mktemp) && curl -fsSL https://opencode.ai/install "
                ) && opencode.contains(" || npm i -g opencode-ai@latest"),
                "WSL opencode install should prefer native POSIX installer with npm fallback: {opencode}"
            );
            assert!(!opencode.contains("| bash"));

            let codex =
                wsl_tool_action_shell_command("codex", ToolLifecycleAction::Install).unwrap();
            assert_eq!(codex, "npm i -g @openai/codex@latest");

            let grok = wsl_tool_action_shell_command("grok", ToolLifecycleAction::Install).unwrap();
            assert!(
                grok.starts_with(
                    "bash -c 'tmp=$(mktemp) && curl -fsSL https://x.ai/cli/install.sh "
                ) && grok.ends_with(&format!(
                    " || {}",
                    tool_spec("grok").npm_install_command().unwrap()
                )),
                "WSL grok install should prefer native POSIX installer with npm fallback: {grok}"
            );
            assert!(!grok.contains("| bash"));
        }

        #[test]
        fn wsl_npm_tools_use_posix_update_chain_without_batch_call() {
            // WSL 内跑的是 POSIX shell,不能带 Windows batch 的 `call`。同时 update
            // fallback 仍应先尝试官方 CLI 自升级。
            let cmd = wsl_tool_action_shell_command("claude", ToolLifecycleAction::Update).unwrap();
            assert_eq!(
                cmd,
                "claude update || npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"
            );
        }
    }

    /// install 端的"上游推荐 || npm 兜底"短路链:把工具→官方安装方式这一上游事实
    /// 固化为回归断言。任何方案改动若打破短路链结构或 URL,都会被这些用例拦下。
    #[cfg(not(target_os = "windows"))]
    mod install_strategy {
        use super::super::*;

        #[test]
        fn claude_install_prefers_native_with_npm_fallback() {
            // Anthropic 现在主推 native installer(claude.ai/install.sh),
            // 网络不通时短路到 npm 仍能装上;两段都得在,顺序也得对。
            let cmd = install_command_for("claude");
            assert!(
                cmd.contains("https://claude.ai/install.sh"),
                "should include official installer URL: {cmd}"
            );
            assert!(
                cmd.contains("@anthropic-ai/claude-code@latest"),
                "should keep npm package as fallback: {cmd}"
            );
            let parts: Vec<&str> = cmd.split("||").collect();
            assert_eq!(parts.len(), 2, "should be a two-step short-circuit chain");
            assert!(parts[0].contains("install.sh"), "native first: {cmd}");
            assert!(
                !parts[0].contains('|'),
                "native installer should avoid pipe: {cmd}"
            );
            assert!(parts[1].contains("npm i -g"), "npm second: {cmd}");
        }

        #[test]
        fn opencode_install_prefers_native_with_npm_fallback() {
            // SST 自家 install.sh 与 claude 同形态:bash 脚本、网络下载、装到 ~/.opencode/bin。
            let cmd = install_command_for("opencode");
            assert!(
                cmd.contains("https://opencode.ai/install"),
                "should include official installer URL: {cmd}"
            );
            assert!(
                cmd.contains("opencode-ai@latest"),
                "should keep npm package as fallback: {cmd}"
            );
            assert!(cmd.contains("||"), "should chain fallback: {cmd}");
            assert!(
                !cmd.split("||").next().unwrap_or_default().contains('|'),
                "native installer should avoid pipe: {cmd}"
            );
        }

        #[test]
        fn codex_install_keeps_static_npm() {
            // OpenAI 暂无独立 native installer,保持原裸 npm,不引入兜底链(无东西可兜底)。
            let cmd = install_command_for("codex");
            assert_eq!(cmd, "npm i -g @openai/codex@latest");
            assert!(!cmd.contains("||"));
        }

        #[test]
        fn gemini_install_keeps_static_npm() {
            // Google 文档同时支持 brew/npm,但本表保持与 update fallback 一致的 npm。
            // 用户若已装 brew gemini-cli,update 路径的锚定会识别 formula → brew upgrade,
            // 所以 install 端不强行替用户决策"用 brew 还是 npm"。
            let cmd = install_command_for("gemini");
            assert_eq!(cmd, "npm i -g @google/gemini-cli@latest");
        }

        #[test]
        fn grok_install_prefers_native_with_npm_fallback() {
            let cmd = install_command_for("grok");
            assert!(
                cmd.contains("https://x.ai/cli/install.sh"),
                "should include official installer URL: {cmd}"
            );
            assert!(
                cmd.contains("@xai-official/grok@latest"),
                "should keep npm package as fallback: {cmd}"
            );
            let parts: Vec<&str> = cmd.split("||").collect();
            assert_eq!(parts.len(), 2, "should be a two-step short-circuit chain");
            assert!(parts[0].contains("install.sh"), "native first: {cmd}");
            assert!(
                !parts[0].contains('|'),
                "native installer should avoid pipe: {cmd}"
            );
            // npm 12 拦截 postinstall 时升级 exit 0 却留在旧版（#8863）。
            assert_eq!(
                parts[1].trim(),
                tool_spec("grok").npm_install_command().unwrap(),
                "npm second: {cmd}"
            );
        }

        #[test]
        fn mcode_install_prefers_official_installer_with_npm_fallback() {
            let cmd = install_command_for("mcode");
            let (installer, fallback) = cmd
                .split_once(" || ")
                .expect("install should chain an npm fallback");
            assert!(
                installer.contains("https://filecdn.minimax.chat/public/install.sh"),
                "official installer first: {cmd}"
            );
            assert!(
                !installer.contains('|'),
                "native installer should avoid pipe: {cmd}"
            );
            assert_eq!(
                Some(fallback),
                tool_spec("mcode").npm_install_command().as_deref()
            );
        }

        #[test]
        fn openclaw_install_keeps_static_npm() {
            let cmd = install_command_for("openclaw");
            assert_eq!(cmd, "npm i -g openclaw@latest");
        }

        #[test]
        fn pi_install_uses_the_verified_pinned_package() {
            let cmd = install_command_for("pi");
            assert_eq!(cmd, "npm i -g @earendil-works/pi-coding-agent@latest");
            assert!(!cmd.contains("||"));
        }

        #[test]
        fn update_fallbacks_use_official_cli_only_when_supported() {
            assert_eq!(
                static_fallback_command("claude"),
                "claude update || npm i -g @anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code"
            );
            assert_eq!(
                static_fallback_command("codex"),
                "npm i -g @openai/codex@latest"
            );
            assert!(!static_fallback_command("codex").contains("codex update"));
            assert_eq!(
                static_fallback_command("gemini"),
                "npm i -g @google/gemini-cli@latest"
            );
            assert!(!static_fallback_command("gemini").contains("gemini update"));
            assert_eq!(
                static_fallback_command("grok"),
                "npm i -g @xai-official/grok@latest --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok"
            );
            assert!(!static_fallback_command("grok").contains("grok update"));
            assert_eq!(
                static_fallback_command("opencode"),
                "opencode upgrade || npm i -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai"
            );
            assert_eq!(
                static_fallback_command("openclaw"),
                "openclaw update --yes || npm i -g openclaw@latest"
            );
            assert_eq!(
                static_fallback_command("pi"),
                "npm i -g @earendil-works/pi-coding-agent@latest"
            );
            assert!(!static_fallback_command("pi").contains("pi update"));
            assert_eq!(
                Some(static_fallback_command("mcode")),
                tool_spec("mcode").npm_install_command()
            );
            assert!(!static_fallback_command("mcode").contains("mcode update"));
        }

        #[test]
        fn hermes_install_uses_official_installer() {
            // Hermes 官方 installer 会处理 Python 3.11+/uv 等运行时;不要再从 cc-switch
            // 里走 `python3 || python` pip 链。
            let cmd = install_command_for("hermes");
            assert!(
                cmd.starts_with("bash -c 'tmp=$(mktemp) && curl -fsSL ")
                    && cmd.contains("install.sh -o $tmp && bash $tmp"),
                "should use official installer: {cmd}"
            );
            assert!(
                !cmd.contains('|') && !cmd.contains("python") && !cmd.contains("pip"),
                "should not depend on pipefail or system Python/pip: {cmd}"
            );
        }

        #[test]
        fn agy_installs_with_official_script_and_updates_without_reinstalling() {
            let cmd = install_command_for("agy");
            assert!(
                cmd.contains("curl -fsSL https://antigravity.google/cli/install.sh -o $tmp")
                    && !cmd.contains('|')
                    && !cmd.contains("npm"),
                "should use only the official installer: {cmd}"
            );
            // The installer exits 0 when agy already exists, so it must not back up `agy update`.
            assert_eq!(static_fallback_command("agy"), "agy update");
        }

        #[test]
        fn hermes_update_fallback_uses_cli_update_then_installer() {
            // 锚定失败时也不回退 pip:先让 PATH 上的 hermes 自更新,找不到/失败再跑官方
            // installer。这样 pyenv 的 `python` shim 不会参与错误路径。
            let cmd = static_fallback_command("hermes");
            assert!(
                cmd.starts_with("hermes update || bash -c 'tmp=$(mktemp) && curl -fsSL "),
                "should try CLI update before official installer: {cmd}"
            );
            let fallback = cmd
                .split_once("||")
                .map(|(_, fallback)| fallback)
                .expect("update should include installer fallback");
            assert!(
                !fallback.contains('|') && !cmd.contains("python") && !cmd.contains("pip"),
                "should not depend on pipefail or system Python/pip: {cmd}"
            );
        }
    }

    #[cfg(target_os = "windows")]
    mod wsl_helpers {
        use super::super::*;

        #[test]
        fn test_is_valid_shell() {
            assert!(is_valid_shell("bash"));
            assert!(is_valid_shell("zsh"));
            assert!(is_valid_shell("sh"));
            assert!(is_valid_shell("fish"));
            assert!(is_valid_shell("dash"));
            assert!(is_valid_shell("/usr/bin/bash"));
            assert!(is_valid_shell("/bin/zsh"));
            assert!(!is_valid_shell("powershell"));
            assert!(!is_valid_shell("cmd"));
            assert!(!is_valid_shell(""));
        }

        #[test]
        fn test_is_valid_shell_flag() {
            assert!(is_valid_shell_flag("-c"));
            assert!(is_valid_shell_flag("-lc"));
            assert!(is_valid_shell_flag("-lic"));
            assert!(!is_valid_shell_flag("-x"));
            assert!(!is_valid_shell_flag(""));
            assert!(!is_valid_shell_flag("--login"));
        }

        #[test]
        fn test_default_flag_for_shell() {
            assert_eq!(default_flag_for_shell("sh"), "-c");
            assert_eq!(default_flag_for_shell("dash"), "-c");
            assert_eq!(default_flag_for_shell("/bin/dash"), "-c");
            assert_eq!(default_flag_for_shell("fish"), "-lc");
            assert_eq!(default_flag_for_shell("bash"), "-lic");
            assert_eq!(default_flag_for_shell("zsh"), "-lic");
            assert_eq!(default_flag_for_shell("/usr/bin/zsh"), "-lic");
        }

        #[test]
        fn test_is_valid_wsl_distro_name() {
            assert!(is_valid_wsl_distro_name("Ubuntu"));
            assert!(is_valid_wsl_distro_name("Ubuntu-22.04"));
            assert!(is_valid_wsl_distro_name("my_distro"));
            assert!(!is_valid_wsl_distro_name(""));
            assert!(!is_valid_wsl_distro_name("distro with spaces"));
            assert!(!is_valid_wsl_distro_name(&"a".repeat(65)));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn merge_path_segments_win_preserves_order_and_dedupes_case_insensitively() {
        let merged = merge_path_segments_win(&[
            r"C:\a;C:\B;%SystemRoot%\system32",
            r"C:\b;C:\a", // dup of C:\a (case-insensitive) and C:\B
            "",
        ]);
        assert_eq!(merged, r"C:\a;C:\B;%SystemRoot%\system32");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn effective_path_keeps_process_entries_with_non_unicode_path() {
        use std::os::windows::ffi::OsStringExt;

        const CHILD: &str = "CC_SWITCH_TEST_NON_UNICODE_PATH";
        if std::env::var_os(CHILD).is_some() {
            assert!(std::env::var("PATH").is_err());
            let effective = effective_path_string();
            assert!(effective.starts_with(r"C:\cc-switch-path-test;"));
            assert!(effective.contains(r";D:\cc-switch-path-test"));
            return;
        }
        // Use a subprocess so other tests never observe a modified process PATH.
        let mut path: Vec<u16> = r"C:\cc-switch-path-test;C:\".encode_utf16().collect();
        path.push(0xD800); // Unpaired UTF-16 surrogate.
        path.extend(r";D:\cc-switch-path-test".encode_utf16());
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::misc::tests::effective_path_keeps_process_entries_with_non_unicode_path",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", std::ffi::OsString::from_wide(&path))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_shell_compatible_path_strips_verbatim_prefixes() {
        assert_eq!(
            windows_shell_compatible_path(Path::new(r"\\?\C:\tools\codex.cmd")),
            PathBuf::from(r"C:\tools\codex.cmd")
        );
        assert_eq!(
            windows_shell_compatible_path(Path::new(r"\\?\UNC\server\share\tools\codex.cmd")),
            PathBuf::from(r"\\server\share\tools\codex.cmd")
        );
        assert_eq!(
            windows_shell_compatible_path(Path::new(r"C:\tools\codex.cmd")),
            PathBuf::from(r"C:\tools\codex.cmd")
        );
    }
}
