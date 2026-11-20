#![allow(non_snake_case)]

use crate::init_status::{InitErrorPayload, SkillsMigrationPayload};
use indexmap::IndexSet;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::LazyLock;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tool_lifecycle_progress::ToolLifecycleProgress;

#[path = "tool_discovery.rs"]
pub(super) mod tool_discovery;
#[path = "tool_lifecycle_command.rs"]
mod tool_lifecycle_command;
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
use tool_lifecycle_command::*;
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
        // 局部变量按声明的逆序释放：工具锁要先于执行锁放开，否则排队的下一个写入
        // 拿到执行锁、跑完之后，紧接着对这个工具的请求还会撞上没放开的工具锁。
        tokio::task::spawn_blocking(move || {
            let _execution_guard = execution_guard;
            let _tool_guards = tool_guards;
            operation(&tools)
        })
        .await
        .map_err(|e| format!("tool lifecycle task join error: {e}"))?
    }
}

static TOOL_LIFECYCLE: LazyLock<ToolLifecycleCoordinator> =
    LazyLock::new(ToolLifecycleCoordinator::default);

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

/// Windows 双引号包裹基础原语:无条件加引号 + 内部 `"` 转义为 `\"`。
/// `windows_cmd_double_quote_arg`(给 wsl.exe 传 bash 命令字符串用)与
/// `win_quote_path_for_batch`(给锚定路径用)都基于它,避免两份 quoter 各自演化、
/// 未来对同一路径产生不一致引用形态。镜像 POSIX 侧 `shell_single_quote` 与
/// `quote_path_if_spaced` 的"重量基础 + 轻量条件包装"两层结构。
#[cfg(any(target_os = "windows", test))]
fn win_double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(any(target_os = "windows", test))]
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
#[cfg(any(target_os = "windows", test))]
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
#[cfg(any(target_os = "windows", test))]
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
    let app = known_tool(tool)?
        .id
        .parse::<crate::app_config::AppType>()
        .ok()?;
    let override_dir = crate::settings::get_override_dir(&app)?;

    wsl_distro_from_path(&override_dir)
}

#[cfg(test)]
fn wsl_distro_for_override(tool: &str, raw: &str) -> Option<String> {
    let app = known_tool(tool)?
        .id
        .parse::<crate::app_config::AppType>()
        .ok()?;
    if !matches!(
        app,
        crate::app_config::AppType::Claude
            | crate::app_config::AppType::Codex
            | crate::app_config::AppType::Gemini
            | crate::app_config::AppType::GrokBuild
            | crate::app_config::AppType::OpenCode
            | crate::app_config::AppType::OpenClaw
            | crate::app_config::AppType::Hermes
            | crate::app_config::AppType::Pi
            | crate::app_config::AppType::Omp
            | crate::app_config::AppType::Dsh
            | crate::app_config::AppType::Agy
    ) {
        return None;
    }
    let (distro, _) = crate::config::wsl_unc_to_linux(raw)?;
    is_valid_wsl_distro_name(&distro).then_some(distro)
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

/// Carries user-controlled paths, arguments, credentials, and the final POSIX payload
/// across `WSLENV` (`/u`) so none of them ever touch Windows `.bat` bytes or `wsl.exe` argv.
#[cfg(any(target_os = "windows", test))]
#[derive(Default)]
pub(super) struct WslEnvCarrier {
    pub(super) entries: Vec<(String, String)>,
}

#[cfg(any(target_os = "windows", test))]
impl WslEnvCarrier {
    pub(super) fn var_for(&mut self, value: &str) -> String {
        if let Some((name, _)) = self.entries.iter().find(|(_, existing)| existing == value) {
            return name.clone();
        }
        let name = format!("CC_SWITCH_INTERNAL_WSL_{}", self.entries.len());
        self.entries.push((name.clone(), value.to_string()));
        name
    }

    pub(super) fn finish_exec(&mut self, mut steps: Vec<String>, exec_args: &[String]) -> String {
        steps.push(
            "if [ -d \"$HOME/.local/bin\" ]; then case \":$PATH:\" in *\":$HOME/.local/bin:\"*) ;; *) PATH=\"$PATH:$HOME/.local/bin\";; esac; fi"
                .to_string(),
        );
        steps.push(format!("set -- {}", exec_args.join(" ")));
        let payload_var = format!("CC_SWITCH_INTERNAL_WSL_{}", self.entries.len());
        let mut unset_names: Vec<&str> =
            self.entries.iter().map(|(name, _)| name.as_str()).collect();
        unset_names.push(&payload_var);
        steps.push(format!("unset {}", unset_names.join(" ")));
        steps.push("exec agy \"$@\"".to_string());
        let payload = steps.join(" && ");
        self.entries.push((payload_var.clone(), payload));
        payload_var
    }

    pub(super) fn wslenv_spec(&self) -> String {
        self.entries
            .iter()
            .map(|(name, _)| format!("{name}/u"))
            .collect::<Vec<_>>()
            .join(":")
    }

    pub(super) fn wslenv_set_lines(&self) -> String {
        let spec = self.wslenv_spec();
        if spec.is_empty() {
            return String::new();
        }
        format!(
            "if defined WSLENV set \"WSLENV=%WSLENV%:{spec}\"\r\nif not defined WSLENV set \"WSLENV={spec}\"\r\n"
        )
    }
}

/// Runs `$SHELL` with an appropriate login/interactive flag to populate `PATH`, then
/// delegates POSIX setup and `exec agy` in `$payload_var` to POSIX `sh`. Safe even
/// when `$SHELL` is `fish`, `dash`, or `sh`.
#[cfg(any(target_os = "windows", test))]
pub(super) fn wsl_login_inner(payload_var: &str) -> String {
    format!(
        "s=${{SHELL:-sh}}; case ${{s##*/}} in dash|sh) f=-c;; fish) f=-lc;; bash|zsh) f=-lic;; *) s=sh; f=-c;; esac; exec \"$s\" $f 'exec sh -c \"${payload_var}\"'"
    )
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn wsl_login_command_line(distro: &str, payload_var: &str) -> Result<String, String> {
    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("Invalid WSL distro name: {distro}"));
    }
    Ok(format!(
        "wsl.exe -d {distro} -- sh -c {}",
        windows_cmd_double_quote_arg(&wsl_login_inner(payload_var))
    ))
}

#[cfg(test)]
fn agy_wsl_lifecycle_command(distro: &str, action: ToolLifecycleAction) -> Result<String, String> {
    let inner = wsl_tool_action_shell_command("agy", action)
        .ok_or_else(|| "Unsupported tool action target: agy".to_string())?;
    build_wsl_tool_action_line(distro, &inner, None, None)
}

#[cfg(any(target_os = "windows", test))]
fn agy_wsl_usage_payload(
    distro: &str,
    gemini_root: &str,
) -> Result<(String, WslEnvCarrier), String> {
    let root = crate::config::path_into_wsl(gemini_root, distro)
        .ok_or_else(|| format!("Antigravity config directory is not inside WSL distro {distro}"))?;
    let mut carrier = WslEnvCarrier::default();
    let root_var = carrier.var_for(&root);
    let steps = vec![
        "unset GEMINI_API_KEY GOOGLE_GEMINI_BASE_URL".to_string(),
        format!(
            "echo {}",
            crate::services::subscription_agy::USAGE_OUTPUT_SENTINEL
        ),
    ];
    let payload_var = carrier.finish_exec(
        steps,
        &[
            format!("\"--gemini_dir=${root_var}\""),
            "--print".to_string(),
            "/usage".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ],
    );
    Ok((payload_var, carrier))
}

#[cfg(test)]
fn agy_wsl_usage_command(distro: &str, gemini_root: &str) -> Result<String, String> {
    let (payload_var, _) = agy_wsl_usage_payload(distro, gemini_root)?;
    wsl_login_command_line(distro, &payload_var)
}

#[cfg(any(target_os = "windows", test))]
fn agy_wsl_models_payload(
    distro: &str,
    gemini_root: &str,
    profile_dir: Option<&str>,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<(String, WslEnvCarrier), String> {
    let dir = match profile_dir {
        Some(path) => crate::config::path_into_wsl(path, distro).ok_or_else(|| {
            format!("Cannot use this Antigravity profile inside WSL distro {distro}")
        })?,
        None => crate::config::path_into_wsl(gemini_root, distro).ok_or_else(|| {
            format!("Antigravity config directory is not inside WSL distro {distro}")
        })?,
    };
    let mut carrier = WslEnvCarrier::default();
    let dir_var = carrier.var_for(&dir);
    let mut steps = Vec::new();
    if let Some(key) = api_key {
        let key_var = carrier.var_for(key);
        steps.push(format!("export GEMINI_API_KEY=\"${key_var}\""));
        if let Some(url) = base_url {
            let url_var = carrier.var_for(url);
            steps.push(format!("export GOOGLE_GEMINI_BASE_URL=\"${url_var}\""));
        } else {
            steps.push("unset GOOGLE_GEMINI_BASE_URL".to_string());
        }
    } else {
        steps.push("unset GEMINI_API_KEY GOOGLE_GEMINI_BASE_URL".to_string());
    }
    let payload_var = carrier.finish_exec(
        steps,
        &[format!("\"--gemini_dir=${dir_var}\""), "models".to_string()],
    );
    Ok((payload_var, carrier))
}

#[cfg(test)]
fn agy_wsl_models_command(
    distro: &str,
    gemini_root: &str,
    profile_dir: Option<&str>,
    api_key: Option<&str>,
    base_url: Option<&str>,
) -> Result<String, String> {
    let (payload_var, _) =
        agy_wsl_models_payload(distro, gemini_root, profile_dir, api_key, base_url)?;
    wsl_login_command_line(distro, &payload_var)
}

#[cfg(target_os = "windows")]
fn run_wsl_login_payload(
    distro: &str,
    payload_var: &str,
    carrier: &WslEnvCarrier,
    timeout: Option<std::time::Duration>,
) -> Result<std::process::Output, String> {
    use std::process::{Command, Stdio};

    if !is_valid_wsl_distro_name(distro) {
        return Err(format!("[WSL:{distro}] invalid distro name"));
    }
    let inner = wsl_login_inner(payload_var);
    let deadline = CommandDeadline::from_timeout(timeout);
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-d", distro, "--", "sh", "-c", &inner])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in &carrier.entries {
        cmd.env(name, value);
    }
    let spec = carrier.wslenv_spec();
    if !spec.is_empty() {
        let wslenv = match std::env::var("WSLENV") {
            Ok(existing) if !existing.trim().is_empty() => format!("{existing}:{spec}"),
            _ => spec,
        };
        cmd.env("WSLENV", wslenv);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("[WSL:{distro}] failed to run agy: {e}"))?;
    wait_child_output(child, deadline)
}

#[cfg(target_os = "windows")]
pub(crate) fn run_agy_wsl_usage(
    timeout: Option<std::time::Duration>,
) -> Result<std::process::Output, String> {
    let distro = wsl_distro_for_tool("agy")
        .ok_or_else(|| "agy is not configured to run in WSL".to_string())?;
    let root = crate::agy_config::agy_gemini_root();
    let (payload_var, carrier) = agy_wsl_usage_payload(&distro, &root.to_string_lossy())?;
    run_wsl_login_payload(&distro, &payload_var, &carrier, timeout)
}

#[cfg(target_os = "windows")]
pub(crate) fn run_agy_wsl_models(
    profile_dir: Option<&str>,
    api_key: Option<&str>,
    base_url: Option<&str>,
    timeout: Option<std::time::Duration>,
) -> Result<std::process::Output, String> {
    let distro = wsl_distro_for_tool("agy")
        .ok_or_else(|| "agy is not configured to run in WSL".to_string())?;
    let root = crate::agy_config::agy_gemini_root();
    let (payload_var, carrier) = agy_wsl_models_payload(
        &distro,
        &root.to_string_lossy(),
        profile_dir,
        api_key,
        base_url,
    )?;
    run_wsl_login_payload(&distro, &payload_var, &carrier, timeout)
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

    #[test]
    fn agy_wsl_override_uses_the_distro_and_wsl_exe() {
        let root = r"\\wsl.localhost\Ubuntu-26.04\home\alice\.gemini";
        assert_eq!(
            wsl_distro_for_override("agy", root).as_deref(),
            Some("Ubuntu-26.04")
        );
        assert_eq!(
            wsl_distro_for_override("agy", r"\\wsl$\Ubuntu-26.04\home\alice\.gemini").as_deref(),
            Some("Ubuntu-26.04")
        );
        assert!(wsl_distro_for_override("agy", r"C:\Users\alice\.gemini").is_none());
        assert!(wsl_distro_for_override(
            "not-a-tool",
            r"\\wsl.localhost\Ubuntu-26.04\home\alice\.gemini"
        )
        .is_none());

        let version = wsl_version_probe_command_line("agy", "Ubuntu-26.04").unwrap();
        assert!(
            version.starts_with("wsl.exe -d Ubuntu-26.04 -- sh -c "),
            "{version}"
        );
        assert!(version.contains("agy --version"), "{version}");

        let install =
            agy_wsl_lifecycle_command("Ubuntu-26.04", ToolLifecycleAction::Install).unwrap();
        assert!(
            install.starts_with("wsl.exe -d Ubuntu-26.04 -- "),
            "{install}"
        );
        assert!(
            install.contains("https://antigravity.google/cli/install.sh"),
            "{install}"
        );
        assert!(
            !install.contains("powershell") && !install.contains(".ps1"),
            "{install}"
        );

        let update =
            agy_wsl_lifecycle_command("Ubuntu-26.04", ToolLifecycleAction::Update).unwrap();
        assert!(
            update.starts_with("wsl.exe -d Ubuntu-26.04 -- "),
            "{update}"
        );
        assert!(update.contains("agy update"), "{update}");

        let (usage_var, usage_carrier) = agy_wsl_usage_payload("Ubuntu-26.04", root).unwrap();
        let usage = agy_wsl_usage_command("Ubuntu-26.04", root).unwrap();
        assert!(
            usage.starts_with("wsl.exe -d Ubuntu-26.04 -- sh -c "),
            "{usage}"
        );
        assert!(!usage.contains("/home/alice/.gemini"), "{usage}");
        let usage_payload = &usage_carrier
            .entries
            .iter()
            .find(|(name, _)| name == &usage_var)
            .unwrap()
            .1;
        assert!(
            usage_payload.contains(crate::services::subscription_agy::USAGE_OUTPUT_SENTINEL),
            "{usage_payload}"
        );
        assert!(
            usage_carrier
                .entries
                .iter()
                .any(|(_, val)| val == "/home/alice/.gemini"),
            "{:?}",
            usage_carrier.entries
        );

        let (models_var, models_carrier) = agy_wsl_models_payload(
            "Ubuntu-26.04",
            root,
            None,
            Some(r#"sk-secret-"&calc"#),
            Some("https://example.test/v1?a=1&b=2"),
        )
        .unwrap();
        let models = agy_wsl_models_command(
            "Ubuntu-26.04",
            root,
            None,
            Some(r#"sk-secret-"&calc"#),
            Some("https://example.test/v1?a=1&b=2"),
        )
        .unwrap();
        assert!(
            models.starts_with("wsl.exe -d Ubuntu-26.04 -- sh -c "),
            "{models}"
        );
        assert!(!models.contains("sk-secret"), "{models}");
        assert!(!models.contains("example.test"), "{models}");
        assert!(!models.contains("/home/alice/.gemini"), "{models}");
        let models_payload = &models_carrier
            .entries
            .iter()
            .find(|(name, _)| name == &models_var)
            .unwrap()
            .1;
        assert!(
            models_payload.contains("export GEMINI_API_KEY="),
            "{models_payload}"
        );
        assert!(!models_payload.contains("sk-secret"), "{models_payload}");
        assert!(!models_payload.contains("example.test"), "{models_payload}");
        assert!(
            models_carrier
                .entries
                .iter()
                .any(|(_, val)| val == r#"sk-secret-"&calc"#),
            "{:?}",
            models_carrier.entries
        );
    }
}
