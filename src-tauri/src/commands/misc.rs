use crate::init_status::{InitErrorPayload, SkillsMigrationPayload};
use indexmap::IndexSet;
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::LazyLock;
use tauri::AppHandle;
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

    crate::host_open::open_url(&app, &url).map_err(|e| format!("打开链接失败: {e}"))?;

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
    crate::host_open::open_url(
        &handle,
        "https://github.com/farion1231/cc-switch/releases/latest",
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
    crate::host_open::open_url(&handle, &url).map_err(|e| format!("打开更新页面失败: {e}"))?;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    desktop_installation: Option<crate::codex_config::CodexDesktopInstallation>,
    desktop_managed: bool,
    /// 工具运行环境: "windows", "wsl", "macos", "linux", "unknown"
    env_type: String,
    /// 当 env_type 为 "wsl" 时，返回该工具绑定的 WSL distro（用于按 distro 探测 shells）
    wsl_distro: Option<String>,
    /// 有新版本、但新版本要求的 Node 比这处安装用的新：前端不给升级按钮，提示先升级 Node。
    update_blocked_by_node: Option<tool_node_requirement::NodeRequirementGap>,
    manual_update_only: bool,
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

const OPENCODE_V2_NPM_PACKAGE: &str = "@opencode/cli";

/// 顺序即「应用」页与批量安装 / 升级的顺序。
static TOOLS: [ToolSpec; 15] = [
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
    ToolSpec::npm("pi", "Pi", "@earendil-works/pi-coding-agent").official_update("update"),
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
    // `codebuddy update` 无交互确认：按自身安装方式升级（npm 会用 `--prefix` 钉在正在运行的
    // 那份上，pnpm/bun 各用自己的命令，原生版走官方 installer），装完核对版本；失败或认不出
    // 安装方式（会提示手动 `npm install -g`）时 exit 1，由 `|| <npm fallback>` 兜底。
    // 依赖没有 install 脚本，npm 12 不需要 `--allow-scripts`。
    ToolSpec::npm("codebuddy", "CodeBuddy", "@tencent-ai/codebuddy-code")
        .official_update("update"),
];

fn known_tool(id: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|tool| tool.id == id)
}

fn tool_spec(id: &str) -> &'static ToolSpec {
    known_tool(id).unwrap_or(&UNKNOWN_TOOL)
}

/// 一处安装归属的 npm 包，以及 `npm i -g` 它时要追加的参数（前导空格已含）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NpmPackage {
    pub(super) name: &'static str,
    pub(super) install_args: &'static str,
}

/// OpenCode 2 是与 1.x 并行发布的另一条线：npm 包 `@opencode/cli`、安装脚本
/// `opencode.ai/v2/install`、brew `anomalyco/tap/opencode-v2`，从 1.x 升上去要用户自己跑
/// `opencode upgrade --major`（首次启动会迁移共用的 opencode.db）。`TOOLS` 里登记的仍是 1.x
/// 的 `opencode-ai`，安装按钮也只装 1.x；应用页只跟着本地已装的那条线查版本、升级、卸载，
/// 从不替用户换线。两个包都提供 `opencode` 命令，pnpm 给 2.x 装 `opencode-ai` 会静默顶掉
/// 2.x 的入口（npm 则 EEXIST 失败），所以拼包管理器命令前必须先认对是哪条线。
pub(super) const OPENCODE_V2_NPM: NpmPackage = NpmPackage {
    name: "@opencode/cli",
    install_args: " --ignore-scripts=false --include=optional --allow-scripts=@opencode/cli",
};

/// 版本号是否属于 OpenCode 2 及以后（1.x 之外的线）。认不出的版本按 1.x 处理。
pub(super) fn is_opencode_v2_version(version: &str) -> bool {
    parse_semver(version).is_some_and(|([major, ..], _)| major >= 2)
}

/// 真身路径（或 Windows npm `.cmd` shim 的内容）是否落在 `@opencode/cli` 包里。
/// npm / pnpm / bun 的全局安装都形如 `.../node_modules/@opencode/cli/bin/opencode.exe`。
pub(super) fn mentions_opencode_v2_package(text: &str) -> bool {
    text.replace('\\', "/")
        .to_ascii_lowercase()
        .contains("node_modules/@opencode/cli/")
}

/// 看版本号挑 npm 包：版本探测、Node 要求检查用（此时只有 `--version` 的结果）。
pub(super) fn npm_package_for_version(tool: &str, version: Option<&str>) -> Option<NpmPackage> {
    if tool == "opencode" && version.is_some_and(is_opencode_v2_version) {
        return Some(OPENCODE_V2_NPM);
    }
    let spec = tool_spec(tool);
    spec.npm_package.map(|name| NpmPackage {
        name,
        install_args: spec.npm_install_args,
    })
}

/// 看安装位置挑 npm 包：锚定升级、卸载用。POSIX 只看真身路径（纯函数）；Windows 的
/// npm / pnpm 入口是 `.cmd` shim、不是软链，真身就是它自己，要读 shim 内容才知道指向哪个包。
/// Volta 的入口是它自己的 shim 程序，两边都看不出，按 1.x 处理。
pub(super) fn npm_package_for_install(
    tool: &str,
    bin_path: &str,
    real: &str,
) -> Option<NpmPackage> {
    if tool == "opencode"
        && (mentions_opencode_v2_package(real) || windows_shim_mentions_opencode_v2(bin_path))
    {
        return Some(OPENCODE_V2_NPM);
    }
    npm_package_for_version(tool, None)
}

#[cfg(target_os = "windows")]
fn windows_shim_mentions_opencode_v2(bin_path: &str) -> bool {
    bin_path.to_ascii_lowercase().ends_with(".cmd")
        && std::fs::read_to_string(bin_path).is_ok_and(|shim| mentions_opencode_v2_package(&shim))
}

#[cfg(not(target_os = "windows"))]
fn windows_shim_mentions_opencode_v2(_bin_path: &str) -> bool {
    false
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
    // 各工具互不依赖，并发探测：总耗时取最慢的那个，而不是逐个累加（启动后台检查一次
    // 传入全部工具）。join_all 保持输入顺序。
    let results = futures::future::join_all(requested.into_iter().map(|tool| {
        let pref = wsl_shell_by_tool.as_ref().and_then(|m| m.get(tool));
        let tool_wsl_shell = pref.and_then(|p| p.wsl_shell.as_deref());
        let tool_wsl_shell_flag = pref.and_then(|p| p.wsl_shell_flag.as_deref());
        get_single_tool_version_impl(tool, tool_wsl_shell, tool_wsl_shell_flag)
    }))
    .await;

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
                .map(|tool| tool.id)
                .chain(["claude-desktop"])
                .map(|id| (id, Arc::new(tokio::sync::Mutex::new(()))))
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolUpdatePlan {
    tool: String,
    path: String,
    real_path: String,
    version: Option<String>,
    source: String,
    command: String,
}

#[derive(serde::Serialize)]
pub struct ToolInstallationUpdate {
    installation: ToolInstallation,
    real_path: PathBuf,
    package_version: Option<String>,
    latest_version: Option<String>,
    update_blocked_by_node: Option<tool_node_requirement::NodeRequirementGap>,
    plan: Option<ToolUpdatePlan>,
}

#[tauri::command]
pub async fn get_tool_installation_updates(
    tool: String,
    path: Option<PathBuf>,
    include_latest: Option<bool>,
) -> Result<Vec<ToolInstallationUpdate>, String> {
    if known_tool(&tool).is_none() {
        return Err("Unsupported tool".into());
    }
    #[cfg(target_os = "windows")]
    if wsl_distro_for_tool(&tool).is_some() {
        return Ok(Vec::new());
    }
    let selected = tool.clone();
    let check_selected = path.is_some();
    let installs = tokio::task::spawn_blocking(move || {
        enumerate_tool_installations_at(&selected, path.as_deref())
            .into_iter()
            .map(|install| {
                let launcher = Path::new(&install.path);
                let package_version = (!install.runnable)
                    .then(|| global_node_package_version(&selected, launcher, &install.real))
                    .flatten();
                let plan = plan_selected_tool_update(&selected, &install);
                (install, package_version, plan)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| error.to_string())?;
    let client = crate::proxy::http_client::get();
    Ok(futures::future::join_all(
        installs
            .into_iter()
            .map(|(install, package_version, plan)| {
                let client = &client;
                let tool = &tool;
                async move {
                    let latest_version = if include_latest.unwrap_or(true)
                        && (check_selected || plan.is_some())
                        && install.runnable
                    {
                        latest_version_for_installation(
                            client,
                            tool,
                            Path::new(&install.path),
                            &install.real,
                            install.version.as_deref(),
                        )
                        .await
                    } else {
                        None
                    };
                    // The npm plan installs @latest. A newer prerelease shown by
                    // discovery does not authorize that plan to install stable.
                    let plan = plan.filter(|plan| {
                        plan.source != "npm"
                            || !latest_version
                                .as_deref()
                                .is_some_and(tool_version::version_is_prerelease)
                    });
                    let update_blocked_by_node = match (
                        plan.as_ref(),
                        install.version.as_deref(),
                        latest_version.as_deref(),
                    ) {
                        (Some(plan), Some(current), Some(target))
                            if plan.source == "npm"
                                && tool_version::compare_semver(current, target)
                                    == Some(std::cmp::Ordering::Less) =>
                        {
                            tool_node_requirement::selected_npm_update_node_gap(
                                client,
                                tool,
                                PathBuf::from(&install.path),
                                install.real.clone(),
                                target,
                            )
                            .await
                        }
                        _ => None,
                    };
                    ToolInstallationUpdate {
                        real_path: install.real.clone(),
                        installation: install,
                        package_version,
                        latest_version,
                        update_blocked_by_node,
                        plan,
                    }
                }
            }),
    )
    .await)
}

/// Rebuild a confirmed plan under the lifecycle lock. Never execute preview text
/// received through IPC or fall back to a different installation.
fn matching_tool_update_plan(
    tool: &str,
    installs: &[ToolInstallation],
    confirmed: &ToolUpdatePlan,
) -> Option<ToolUpdatePlan> {
    installs
        .iter()
        .find(|install| install.path == confirmed.path)
        .and_then(|install| plan_selected_tool_update(tool, install))
        .filter(|plan| plan == confirmed)
}

fn checked_tool_update_command(tool: &str, confirmed: &ToolUpdatePlan) -> Result<String, String> {
    let installs = enumerate_tool_installations(tool);
    let plan = matching_tool_update_plan(tool, &installs, confirmed).ok_or(
        "Installation changed since the update preview. Check installations and confirm again.",
    )?;
    #[cfg(target_os = "windows")]
    return Ok(format!(
        "@chcp 65001>nul\r\n@echo off\r\ncall {}\r\nexit /b %errorlevel%",
        plan.command
    ));
    #[cfg(not(target_os = "windows"))]
    Ok(format!("set -e\n{}", plan.command))
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallMethod {
    Curl,
    Homebrew,
    Npm,
    NpmLegacy,
    NpmModern,
    Pnpm,
    PnpmBuild,
    Yarn,
    Bun,
}

#[derive(serde::Deserialize)]
struct AppInstaller {
    platforms: Vec<String>,
    command: String,
}

fn app_install_command(
    app: &str,
    method: InstallMethod,
    platform: &str,
) -> Result<&'static str, String> {
    type InstallCatalogue = HashMap<String, HashMap<String, AppInstaller>>;
    static INSTALLERS: LazyLock<Result<InstallCatalogue, serde_json::Error>> =
        LazyLock::new(|| {
            serde_json::from_str(include_str!("../../../src/config/appInstallers.json"))
        });
    let method = match method {
        InstallMethod::Curl => "curl",
        InstallMethod::Homebrew => "homebrew",
        InstallMethod::Npm => "npm",
        InstallMethod::NpmLegacy => "npm-legacy",
        InstallMethod::NpmModern => "npm-modern",
        InstallMethod::Pnpm => "pnpm",
        InstallMethod::PnpmBuild => "pnpm-build",
        InstallMethod::Yarn => "yarn",
        InstallMethod::Bun => "bun",
    };
    INSTALLERS
        .as_ref()
        .map_err(|_| "Invalid bundled installation catalogue".to_string())?
        .get(app)
        .and_then(|channels| channels.get(method))
        .filter(|installer| installer.platforms.iter().any(|target| target == platform))
        .map(|installer| installer.command.as_str())
        .ok_or_else(|| {
            "This install method is unavailable for the selected app and platform.".into()
        })
}

/// Bind a bundled command to the manager resolved by PATH-only preflight.
#[cfg(any(target_os = "windows", test))]
fn bind_install_executable(
    command: &str,
    manager: &str,
    executable: &str,
) -> Result<String, String> {
    let arguments = command
        .strip_prefix(manager)
        .and_then(|tail| tail.strip_prefix(' '))
        .ok_or("The bundled command does not match the selected package manager.")?;
    Ok(format!("{executable} {arguments}"))
}

#[cfg(target_os = "windows")]
fn windows_manager_batch(command: &str, manager: &str, launcher: &Path) -> Result<String, String> {
    let launcher = windows_shell_compatible_path(launcher);
    // This is batch text, with no CALL reparse: percent needs one escaping pass.
    let executable = win_double_quote(&launcher.to_string_lossy().replace('%', "%%"));
    let bound = bind_install_executable(command, manager, &executable)?;
    // A .cmd manager tail-chains the batch; a native manager returns its exit
    // code at EOF. Neither needs CALL (which corrupts quoted caret paths).
    Ok(format!("@echo off\r\n@chcp 65001>nul\r\n{bound}\r\n"))
}

/// Check only known package managers in the same PATH used by the installer.
#[tauri::command]
pub async fn get_install_manager_availability(
    methods: Vec<InstallMethod>,
) -> Result<HashMap<String, bool>, String> {
    tokio::task::spawn_blocking(move || install_manager_availability(&methods, true))
        .await
        .map_err(|error| error.to_string())
}

fn install_manager_availability(methods: &[InstallMethod], passive: bool) -> HashMap<String, bool> {
    let managers = [
        ("npm", "npm"),
        ("pnpm", "pnpm"),
        ("yarn", "yarn"),
        ("bun", "bun"),
        ("node", "node"),
        ("curl", "curl"),
        ("homebrew", "brew"),
    ];
    #[cfg(not(target_os = "windows"))]
    let login_env = login_shell_env(CommandDeadline::from_timeout(Some(
        std::time::Duration::from_secs(10),
    )));
    #[cfg(not(target_os = "windows"))]
    let path = {
        let inherited = std::env::var("PATH").unwrap_or_default();
        login_env
            .as_ref()
            .and_then(|env| env.path.clone())
            .map_or_else(
                || inherited.clone(),
                |path| merge_path_segments(&path, &inherited),
            )
    };
    #[cfg(target_os = "windows")]
    let path = effective_path_string();
    #[cfg(not(target_os = "windows"))]
    let vars = login_env
        .as_ref()
        .map_or(&[][..], |env| env.vars.as_slice());
    #[cfg(target_os = "windows")]
    let vars = &[][..];
    let mut result: HashMap<String, bool> = managers
        .into_iter()
        .map(|(key, executable)| {
            #[cfg(not(target_os = "windows"))]
            let available = std::process::Command::new("/bin/sh")
                .args(["-c", &format!("command -v {executable}")])
                .env("PATH", &path)
                .output()
                .is_ok_and(|output| output.status.success());
            #[cfg(target_os = "windows")]
            let available = resolve_path_default_entry(
                executable,
                CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(5))),
            )
            .is_ok_and(|entry| entry.is_some());
            let available = available
                && (key != "yarn"
                    || !methods.contains(&InstallMethod::Yarn)
                    || manager_query(&path, vars, "yarn", passive)
                        .map_or(passive, |version| is_yarn_classic(&version)));
            let available = available
                && (key != "pnpm"
                    || !methods.iter().any(|method| {
                        matches!(method, InstallMethod::Pnpm | InstallMethod::PnpmBuild)
                    })
                    || manager_query(&path, vars, "pnpm", passive)
                        .map_or(passive, |output| Path::new(output.trim()).is_absolute()));
            (key.to_owned(), available)
        })
        .collect();
    if methods
        .iter()
        .any(|method| matches!(method, InstallMethod::NpmLegacy | InstallMethod::NpmModern))
    {
        let modern = if result.get("npm") == Some(&true) && result.get("node") == Some(&true) {
            manager_query(&path, vars, "npm", passive)
                .and_then(|version| npm_supports_script_allowlist(&version))
        } else {
            None
        };
        result.insert("npm-legacy".into(), modern == Some(false));
        result.insert("npm-modern".into(), modern == Some(true));
    }
    if methods.contains(&InstallMethod::PnpmBuild) {
        let available = result.get("pnpm") == Some(&true)
            && manager_query(&path, vars, "pnpm-version", passive)
                .map_or(passive, |version| pnpm_supports_allow_build(&version));
        result.insert("pnpm-build".into(), available);
    }
    result
}

fn manager_query(
    path: &str,
    vars: &[(String, String)],
    manager: &'static str,
    passive: bool,
) -> Option<String> {
    let (executable, args): (&str, &[&str]) = match manager {
        "npm" => ("npm", &["--version"]),
        "yarn" => ("yarn", &["--version"]),
        "pnpm" => ("pnpm", &["bin", "-g"]),
        "pnpm-version" => ("pnpm", &["--version"]),
        "npm-prefix" => ("npm", &["prefix", "-g"]),
        "bun-bin" => ("bun", &["pm", "bin", "-g"]),
        "yarn-bin" => ("yarn", &["global", "bin"]),
        "yarn-dir" => ("yarn", &["global", "dir"]),
        _ => return None,
    };
    use std::process::Stdio;
    #[cfg(not(target_os = "windows"))]
    let mut command = std::process::Command::new(executable);
    #[cfg(not(target_os = "windows"))]
    command.args(args);
    #[cfg(target_os = "windows")]
    let query_script_dir = tempfile::Builder::new()
        .prefix("cc_switch_manager_")
        .tempdir()
        .ok()?;
    #[cfg(target_os = "windows")]
    let mut command = {
        let resolved = resolve_path_default_entry(
            executable,
            CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(5))),
        )
        .ok()
        .flatten()?;
        let batch = query_script_dir.path().join("query.bat");
        let invocation = format!("{executable} {}", args.join(" "));
        std::fs::write(
            &batch,
            windows_manager_batch(&invocation, executable, &resolved.launcher).ok()?,
        )
        .ok()?;
        windows_lifecycle_command(&batch, path)
    };
    #[cfg(not(target_os = "windows"))]
    if executable == "yarn" {
        // Global metadata must not load a project-local .yarnrc / yarn-path.
        command.current_dir(crate::config::get_home_dir());
    }
    command
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(not(target_os = "windows"))]
    {
        apply_login_shell_vars(&mut command, vars);
        isolate_child_process_group(&mut command);
    }
    #[cfg(target_os = "windows")]
    let _ = vars;
    if passive {
        command.env("COREPACK_ENABLE_NETWORK", "0");
    }
    let child = command.spawn().ok()?;
    wait_child_output(
        child,
        CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(5))),
    )
    .ok()
    .filter(|output| output.status.success())
    .map(|output| decode_command_output(&output.stdout))
}

struct ManagerBinEntry {
    started: std::time::Instant,
    published: std::time::Instant,
    path: Option<PathBuf>,
}
type ManagerBinCache = HashMap<&'static str, ManagerBinEntry>;
static MANAGER_BIN_CACHE: LazyLock<std::sync::Mutex<ManagerBinCache>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

fn manager_bin_queries(method: Option<InstallMethod>) -> &'static [&'static str] {
    match method {
        Some(InstallMethod::Npm | InstallMethod::NpmLegacy | InstallMethod::NpmModern) => {
            &["npm-prefix"]
        }
        Some(InstallMethod::Pnpm | InstallMethod::PnpmBuild) => &["pnpm"],
        Some(InstallMethod::Yarn) => &["yarn-bin", "yarn-dir"],
        Some(InstallMethod::Bun) => &["bun-bin"],
        Some(_) => &[],
        None => &["npm-prefix", "pnpm", "yarn-bin", "yarn-dir", "bun-bin"],
    }
}

pub(super) fn manager_bin_paths() -> Vec<PathBuf> {
    MANAGER_BIN_CACHE
        .lock()
        .ok()
        .map(|cache| {
            manager_bin_queries(None)
                .iter()
                .filter_map(|query| {
                    cache
                        .get(query)
                        .and_then(|entry| entry.path.clone())
                        .map(|path| {
                            if *query == "yarn-dir" {
                                path.join("node_modules/.bin")
                            } else {
                                path
                            }
                        })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn configured_yarn_global_dir() -> Option<PathBuf> {
    static QUERY_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));
    let _guard = QUERY_LOCK.lock().ok()?;
    let fresh = MANAGER_BIN_CACHE
        .lock()
        .ok()?
        .get("yarn-dir")
        .is_some_and(|entry| entry.published.elapsed().as_secs() < 2);
    if !fresh {
        refresh_manager_bins(Some(InstallMethod::Yarn));
    }
    MANAGER_BIN_CACHE.lock().ok()?.get("yarn-dir")?.path.clone()
}

fn refresh_manager_bins(method: Option<InstallMethod>) {
    let queries = manager_bin_queries(method);
    let queries: Vec<_> = if method.is_none() {
        let Ok(cache) = MANAGER_BIN_CACHE.lock() else {
            return;
        };
        queries
            .iter()
            .copied()
            .filter(|query| {
                cache
                    .get(query)
                    .is_none_or(|entry| entry.published.elapsed().as_secs() >= 2)
            })
            .collect()
    } else {
        queries.to_vec()
    };
    if queries.is_empty() {
        return;
    }
    let checked = std::time::Instant::now();
    let paths = discover_manager_bins(&queries);
    if let Ok(mut cache) = MANAGER_BIN_CACHE.lock() {
        for (query, path) in paths {
            if cache
                .get(query)
                .is_none_or(|entry| entry.started <= checked)
            {
                let path = path.or_else(|| cache.get(query).and_then(|entry| entry.path.clone()));
                cache.insert(
                    query,
                    ManagerBinEntry {
                        started: checked,
                        published: std::time::Instant::now(),
                        path,
                    },
                );
            }
        }
    }
}

fn discover_manager_bins(queries: &[&'static str]) -> Vec<(&'static str, Option<PathBuf>)> {
    #[cfg(not(target_os = "windows"))]
    let env = login_shell_env(CommandDeadline::from_timeout(Some(
        std::time::Duration::from_secs(10),
    )));
    #[cfg(not(target_os = "windows"))]
    let path = env
        .as_ref()
        .and_then(|env| env.path.as_deref())
        .map_or_else(
            || std::env::var("PATH").unwrap_or_default(),
            |path| merge_path_segments(path, &std::env::var("PATH").unwrap_or_default()),
        );
    #[cfg(target_os = "windows")]
    let path = effective_path_string();
    #[cfg(not(target_os = "windows"))]
    let vars = env.as_ref().map_or(&[][..], |env| env.vars.as_slice());
    #[cfg(target_os = "windows")]
    let vars = &[][..];
    std::thread::scope(|scope| {
        let handles: Vec<_> = queries
            .iter()
            .map(|&query| {
                let path = &path;
                scope.spawn(move || {
                    let directory = manager_query(path, vars, query, true)
                        .map(|output| PathBuf::from(output.trim()))
                        .filter(|directory| directory.is_absolute());
                    #[cfg(not(target_os = "windows"))]
                    let directory = directory.map(|directory| {
                        if query == "npm-prefix" {
                            directory.join("bin")
                        } else {
                            directory
                        }
                    });
                    (query, directory)
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .collect()
    })
}

fn npm_supports_script_allowlist(version: &str) -> Option<bool> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    let _patch = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor) >= (11, 16))
}

fn pnpm_supports_allow_build(version: &str) -> bool {
    let mut parts = version.trim().split('.');
    let parsed = parts
        .next()
        .and_then(|major| major.parse::<u32>().ok())
        .zip(parts.next().and_then(|minor| minor.parse::<u32>().ok()));
    parsed.is_some_and(|version| version >= (10, 4))
}

fn is_yarn_classic(version: &str) -> bool {
    let mut parts = version.trim().split('.');
    parts.next() == Some("1")
        && parts.next().is_some_and(|part| part.parse::<u32>().is_ok())
        && parts.next().is_some_and(|part| part.parse::<u32>().is_ok())
        && parts.next().is_none()
}

// Keep Tauri's optional IPC arguments compatible with existing callers.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn run_tool_lifecycle_action(
    app: AppHandle,
    tools: Vec<String>,
    action: String,
    wsl_shell_by_tool: Option<HashMap<String, WslShellPreferenceInput>>,
    uninstall_plan: Option<ToolUninstallPlan>,
    update_plan: Option<ToolUpdatePlan>,
    confirmed_update: Option<String>,
    install_method: Option<InstallMethod>,
) -> Result<(), String> {
    if let Some(method) = install_method {
        if action != "install"
            || tools.len() != 1
            || confirmed_update.is_some()
            || update_plan.is_some()
            || uninstall_plan.is_some()
        {
            return Err("Select one app and one installation method.".into());
        }
        let app_id = if tools[0] == "claude-desktop" {
            "claude-desktop"
        } else {
            normalize_requested_tools(&tools)
                .into_iter()
                .next()
                .ok_or("Unsupported installation target.")?
        };
        let command = app_install_command(app_id, method, std::env::consts::OS)?.to_owned();
        let requested = vec![app_id];
        let progress = ToolLifecycleProgress::from_app(app, &requested);
        let queued = progress.clone();
        return TOOL_LIFECYCLE
            .run_with_queue_notice(
                requested,
                move || queued.queued(),
                move |_| {
                    progress.running();
                    let available = install_manager_availability(&[method], false);
                    let manager = match method {
                        InstallMethod::Npm
                        | InstallMethod::NpmLegacy
                        | InstallMethod::NpmModern => "npm",
                        InstallMethod::Pnpm | InstallMethod::PnpmBuild => "pnpm",
                        InstallMethod::Yarn => "yarn",
                        InstallMethod::Bun => "bun",
                        InstallMethod::Curl => "curl",
                        InstallMethod::Homebrew => "homebrew",
                    };
                    if !available.get(manager).copied().unwrap_or(false) {
                        return Err(if manager == "yarn" {
                            "Yarn Classic (1.x) is required for global installs.".into()
                        } else if manager == "pnpm" {
                            "pnpm global bin directory is unavailable. Run pnpm setup in your terminal and restart CC Switch.".into()
                        } else {
                            format!("{manager} is not available in PATH.")
                        });
                    }
                    if matches!(method, InstallMethod::NpmLegacy | InstallMethod::NpmModern) {
                        let key = if method == InstallMethod::NpmLegacy { "npm-legacy" } else { "npm-modern" };
                        if available.get(key) != Some(&true) {
                            return Err("The selected installation command does not match the detected npm version.".into());
                        }
                    }
                    if manager == "npm" && !available.get("node").copied().unwrap_or(false) {
                        return Err("Node.js is not available in PATH.".into());
                    }
                    if method == InstallMethod::PnpmBuild && available.get("pnpm-build") != Some(&true) {
                        return Err("This installation requires pnpm 10.4 or newer.".into());
                    }
                    #[cfg(target_os = "windows")]
                    let command = {
                        let resolved = resolve_path_default_entry(
                            manager,
                            CommandDeadline::from_timeout(Some(std::time::Duration::from_secs(5))),
                        )?
                        .ok_or_else(|| format!("{manager} is no longer available in PATH."))?;
                        windows_manager_batch(&command, manager, &resolved.launcher)?
                    };
                    run_tool_lifecycle_silently(&command, "tool_install", &progress)
                },
            )
            .await;
    }
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }

    if action == "uninstall" {
        if update_plan.is_some() || confirmed_update.is_some() {
            return Err("Uninstall requires only an uninstall plan.".into());
        }
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
                        let removed_preference = plan
                            .native
                            .then(|| {
                                let launcher = Path::new(&plan.path);
                                let verified_real = launcher
                                    .canonicalize()
                                    .ok()
                                    .filter(|real| real == Path::new(&plan.real_path));
                                crate::settings::cli_installation_policy(tool)
                                    .0
                                    .filter(|saved| {
                                        crate::settings::matches_cli_installation(
                                            saved,
                                            launcher,
                                            verified_real.as_deref(),
                                        )
                                    })
                            })
                            .flatten();
                        tool_uninstall::uninstall(tool, pref, &plan, &progress)?;
                        if let Some(saved) = removed_preference {
                            if let Err(err) =
                                crate::settings::clear_cli_preference_if_unchanged(tool, &saved)
                            {
                                log::warn!("Could not clear CLI preference for {tool}: {err}");
                            }
                        }
                    }
                    Ok(())
                },
            )
            .await;
    }
    let action = ToolLifecycleAction::from_str(&action)?;
    if let Some(plan) = &update_plan {
        if !matches!(action, ToolLifecycleAction::Update)
            || requested.as_slice() != [plan.tool.as_str()]
        {
            return Err("The update plan does not match the selected tool.".into());
        }
    } else if matches!(action, ToolLifecycleAction::Update) {
        for tool in &requested {
            tool_node_requirement::ensure_update_node(tool).await?;
        }
    }

    if confirmed_update.is_some()
        && (update_plan.is_some()
            || requested.len() != 1
            || !matches!(action, ToolLifecycleAction::Update))
    {
        return Err("Update confirmation must match one requested update.".into());
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
                #[cfg(target_os = "windows")]
                let opencode_wsl =
                    if matches!(action, ToolLifecycleAction::Update) && tools == ["opencode"] {
                        wsl_distro_for_tool("opencode")
                    } else {
                        None
                    };
                #[cfg(target_os = "windows")]
                if let Some(distro) = opencode_wsl {
                    if confirmed_update.is_some() || update_plan.is_some() {
                        return Err(
                            "The installation environment changed. Confirm the update again."
                                .into(),
                        );
                    }
                    let pref = wsl_shell_by_tool
                        .as_ref()
                        .and_then(|prefs| prefs.get("opencode"));
                    let process = tool_uninstall::wsl_opencode_update_process(&distro, pref)?;
                    let output = tool_lifecycle_progress::run_with_progress(process, &progress)
                        .map_err(|e| format!("Cannot start the OpenCode WSL update: {e}"))?;
                    return finish_lifecycle_output(&output);
                }
                let command_line = if let Some(plan) = update_plan.as_ref() {
                    checked_tool_update_command(tools[0], plan)?
                } else {
                    build_tool_lifecycle_command(
                        tools,
                        action,
                        wsl_shell_by_tool.as_ref(),
                        confirmed_update.as_deref(),
                    )?
                };
                #[cfg(target_os = "windows")]
                if let Some(plan) = update_plan
                    .as_ref()
                    .filter(|plan| plan.tool == "omp" && plan.source == "curl")
                {
                    // checked_tool_update_command has just revalidated the exact copy.
                    // Direct argv avoids cmd's 8191-character limit and path expansion.
                    let process = omp_native_update_process(
                        Path::new(&plan.path),
                        &crate::config::get_home_dir(),
                        &effective_path_os().unwrap_or_default(),
                    )
                    .ok_or("Cannot construct the selected Oh My Pi update process.")?;
                    let output = tool_lifecycle_progress::run_with_progress(process, &progress)
                        .map_err(|error| format!("Cannot start the Oh My Pi update: {error}"))?;
                    return finish_lifecycle_output(&output);
                }
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
    cmd.arg("-c")
        .arg(command_line)
        // Install/update is a user-global operation. Do not load project-local
        // Yarn delegation or package-manager configuration from an inherited CWD.
        .current_dir(crate::config::get_home_dir());
    // GUI 启动时，安装脚本的 node/npm 子进程也需要登录 shell 的 PATH 和 Vite+ 目录。
    if let Some(login_env) = login_shell_env(None) {
        if let Some(login_path) = login_env.path.as_deref() {
            let inherited = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", merge_path_segments(login_path, &inherited));
        }
        apply_login_shell_vars(&mut cmd, &login_env.vars);
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
    let system_dir = PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows")),
    )
    .join("System32");
    let mut command = std::process::Command::new(system_dir.join("cmd.exe"));
    command
        .args(["/D", "/V:OFF", "/S", "/C"])
        // Expand the path from one child-local variable, inside quotes. CMD
        // must not parse literal carets/percent signs from the absolute TMP path.
        .raw_arg("\"\"%CC_SWITCH_LIFECYCLE_SCRIPT%\"\"")
        .env(
            "CC_SWITCH_LIFECYCLE_SCRIPT",
            windows_shell_compatible_path(bat_file),
        )
        // MSI 自动重启可能丢失用户 PATH（#8171）。执行与探测共用补全后的 PATH，
        // 让裸 npm 兜底及工具内部启动的 node/npm 都能找到；仅设置子进程环境。
        .env("PATH", effective_path)
        .env("NoDefaultCurrentDirectoryInExePath", "1")
        .current_dir(system_dir)
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

fn version_probe_error(detail: &str) -> String {
    // Node's exception line comes before the stack, so taking the tail often
    // hides the reason the launcher failed.
    if let Some(line) = detail.lines().map(str::trim).find(|line| {
        [
            "Error:",
            "TypeError:",
            "SyntaxError:",
            "ReferenceError:",
            "RangeError:",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    }) {
        return line.chars().take(400).collect();
    }
    last_lines(detail.trim(), 4)
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
        &crate::config::get_home_dir(),
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
    confirmation_token: Option<String>,
    /// 锚定后将执行的升级命令。执行时重新生成并核对已确认的安装。
    command: String,
    /// 是否成功锚定到某处具体安装。false = 退到裸 fallback 命令（无法确定命令行实际
    /// 命中哪处，或该处无同级 npm）；前端据此给出"默认入口无法确定"的诚实文案。
    anchored: bool,
    /// 默认那处是认不出安装渠道的原生可执行文件（见 `UpdateCommand::Unmanaged`）：
    /// 不会执行升级，此时 `command` 为空；前端跳过该工具并提示用原安装方式升级。
    unmanaged: bool,
    // The manifest version for the same default installation shown in Apps.
    #[serde(skip_serializing_if = "Option::is_none")]
    package_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    update_plan: Option<ToolUpdatePlan>,
}

/// 探测各工具的安装分布：枚举所有安装、标记冲突、生成锚定升级命令。只读、无副作用。
/// 诊断按钮、升级前确认、升级后补诊共用此命令，各取所需字段——避免对同一份枚举结果
/// 散落多套下游判定。
#[tauri::command]
pub async fn probe_tool_installations(
    tools: Vec<String>,
    native_only: Option<bool>,
    install_method: Option<InstallMethod>,
) -> Result<Vec<ToolInstallationReport>, String> {
    tool_installation_reports(tools, false, native_only.unwrap_or(false), install_method).await
}

async fn tool_installation_reports(
    tools: Vec<String>,
    apps_listing: bool,
    native_only: bool,
    install_method: Option<InstallMethod>,
) -> Result<Vec<ToolInstallationReport>, String> {
    let requested = normalize_requested_tools(&tools);
    if requested.is_empty() {
        return Err("No supported tools selected".to_string());
    }
    tokio::task::spawn_blocking(move || {
        if install_method.is_some() || apps_listing {
            refresh_manager_bins(
                if install_method == Some(InstallMethod::Curl) && requested.contains(&"openclaw") {
                    Some(InstallMethod::Npm)
                } else {
                    install_method
                },
            );
        }
        let tools = requested.into_iter();
        // The Apps row already has a signed desktop installation.
        // Independent CLI probes remain available via explicit diagnostics.
        #[cfg(target_os = "macos")]
        let tools = tools.filter(|&tool| {
            !(apps_listing
                && tool == "codex"
                && crate::codex_config::codex_macos_official_installation().is_some())
        });
        #[cfg(not(target_os = "macos"))]
        let _ = apps_listing;
        #[cfg(not(target_os = "windows"))]
        let _ = native_only;

        tools
            .map(|tool| {
                // WSL lifecycle actions resolve inside the configured distro. Windows
                // paths cannot describe that installation or confirm its update plan.
                #[cfg(target_os = "windows")]
                let uses_wsl = !native_only && wsl_distro_for_tool(tool).is_some();
                #[cfg(not(target_os = "windows"))]
                let uses_wsl = false;
                let installs = if uses_wsl {
                    Vec::new()
                } else {
                    enumerate_tool_installations(tool)
                };
                let package_version = default_install(&installs)
                    .filter(|install| !install.runnable)
                    .and_then(|install| {
                        global_node_package_version(tool, Path::new(&install.path), &install.real)
                    });
                let update_plan = default_install(&installs)
                    .and_then(|install| plan_selected_tool_update(tool, install));
                let (update, needs_confirmation) = if uses_wsl {
                    (UpdateCommand::Static(String::new()), false)
                } else {
                    plan_command_for(tool, &installs)
                };
                let (command, anchored, unmanaged) = if let Some(plan) = update_plan.as_ref() {
                    (plan.command.clone(), true, false)
                } else {
                    match update {
                        UpdateCommand::Anchored(command) => (command, true, false),
                        UpdateCommand::Static(command) => (command, false, false),
                        UpdateCommand::Unmanaged => (String::new(), false, true),
                    }
                };
                let is_conflict = is_conflicting(&installs);
                ToolInstallationReport {
                    tool: tool.to_string(),
                    confirmation_token: (!unmanaged
                        && (needs_confirmation
                            || (!apps_listing
                                && !uses_wsl
                                && default_install(&installs).is_some())))
                    .then(|| update_confirmation_token(tool, &command, &installs)),
                    installs,
                    is_conflict,
                    needs_confirmation,
                    command,
                    anchored,
                    unmanaged,
                    package_version,
                    update_plan,
                }
            })
            .collect()
    })
    .await
    .map_err(|e| format!("probe task join error: {e}"))
}

/// 「应用」页的安装概览：每个工具装在哪几处、来源和命令行默认那处。只定位、不跑命令，
/// 用来在版本探测回来之前先把工具分进「已安装 / 未安装」。
#[derive(Debug, serde::Serialize)]
pub struct ToolInstallationOverview {
    tool: &'static str,
    installs: Vec<ToolInstallLocation>,
    #[cfg(target_os = "macos")]
    desktop_installed: bool,
    /// 这个工具跑在 WSL 里：本机清单说明不了 WSL 里装没装，前端等版本探测再分组。
    is_wsl: bool,
}

/// 「应用」页展示路径、来源和多处安装。和升级前的预检是同一份定位，但不对每处安装跑
/// `--version`（那一步留给诊断和升级预检），打开页面不必等每个命令行工具启动一遍。
/// 各工具的定位各要开一个登录 shell / `where`，互不依赖，并发进行。
#[tauri::command]
pub async fn list_tool_installations(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationOverview>, String> {
    let requested = normalize_requested_tools(&tools);
    let tasks = requested.into_iter().map(|tool| {
        tokio::task::spawn_blocking(move || ToolInstallationOverview {
            tool,
            installs: list_tool_install_locations(tool),
            #[cfg(target_os = "macos")]
            desktop_installed: tool == "codex"
                && crate::codex_config::codex_macos_official_installation().is_some(),
            is_wsl: tool_runs_in_wsl(tool),
        })
    });
    futures::future::join_all(tasks)
        .await
        .into_iter()
        .map(|result| result.map_err(|e| format!("installation listing task join error: {e}")))
        .collect()
}

#[tauri::command]
pub async fn get_tool_installation_reports(
    tools: Vec<String>,
) -> Result<Vec<ToolInstallationReport>, String> {
    tool_installation_reports(tools, true, false, None).await
}

fn supports_cli_preference(tool: &str) -> bool {
    let supported = known_tool(tool).is_some();
    #[cfg(target_os = "windows")]
    return supported && wsl_distro_for_tool(tool).is_none();
    #[cfg(not(target_os = "windows"))]
    supported
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum CliSelection {
    Automatic,
    Explicit,
    Bundled,
    Unavailable,
    Disabled,
    Ambiguous,
    Unknown,
}

#[derive(Debug, serde::Serialize)]
pub struct PreferredToolInstallation {
    supported: bool,
    bundled_available: bool,
    path: Option<String>,
    disabled_paths: Vec<String>,
    disabled_real_paths: Vec<PathBuf>,
    selected_path: Option<String>,
    selected_real_path: Option<String>,
    selection: CliSelection,
}

fn preferred_tool_installation(tool: &str) -> PreferredToolInstallation {
    let (path, disabled_paths) = crate::settings::cli_installation_policy(tool);
    let supported = supports_cli_preference(tool);
    let bundled = supported.then(|| verified_bundled_cli(tool)).flatten();
    let bundled_available = bundled.is_some();
    // Explicit bundle preference is macOS-only; runtime fallback also exists on Windows.
    #[cfg(target_os = "windows")]
    let runtime_bundle = supported.then(|| runtime_bundled_cli(tool)).flatten();
    #[cfg(not(target_os = "windows"))]
    let runtime_bundle = bundled.clone();
    let bundled_real = runtime_bundle
        .as_ref()
        .and_then(|path| path.canonicalize().ok());
    let (selected_path, mut selection) = if supported {
        let deadline = CommandDeadline::from_timeout(Some(INSTALL_PROBE_TIMEOUT));
        match resolve_cli_installation(
            tool,
            deadline,
            path.as_deref(),
            &disabled_paths,
            || runtime_bundle,
            |path| cli_installation_is_runnable(path, deadline),
        ) {
            Ok((selected, selection)) => (Some(selected), selection),
            Err(CliLookupError::Missing | CliLookupError::Unrunnable) => {
                (None, CliSelection::Unavailable)
            }
            Err(CliLookupError::Disabled) => (None, CliSelection::Disabled),
            Err(CliLookupError::Ambiguous) => (None, CliSelection::Ambiguous),
            Err(CliLookupError::Probe(_)) => (None, CliSelection::Unknown),
        }
    } else {
        (None, CliSelection::Unknown)
    };
    let selected_real = selected_path
        .as_ref()
        .and_then(|path| path.canonicalize().ok());
    if matches!(selection, CliSelection::Automatic)
        && bundled_real.is_some()
        && bundled_real == selected_real
    {
        selection = CliSelection::Bundled;
    }
    let selected_real_path = selected_real.map(|path| path.to_string_lossy().into_owned());
    let disabled_real_paths = disabled_paths
        .iter()
        .map(Path::new)
        .filter(|path| path.is_absolute())
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    let selected_path = selected_path.map(|path| path.to_string_lossy().into_owned());
    PreferredToolInstallation {
        supported,
        bundled_available,
        path,
        disabled_paths,
        disabled_real_paths,
        selected_path,
        selected_real_path,
        selection,
    }
}

#[tauri::command]
pub async fn get_preferred_tool_installation(
    tool: String,
) -> Result<PreferredToolInstallation, String> {
    tokio::task::spawn_blocking(move || preferred_tool_installation(&tool))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_preferred_tool_installation(
    tool: String,
    path: Option<String>,
    enabled: Option<bool>,
) -> Result<PreferredToolInstallation, String> {
    if known_tool(&tool).is_none() {
        return Err("Unsupported tool".into());
    }
    if enabled.is_some() && path.is_none() {
        return Err("Select an installation to change its status.".into());
    }
    tokio::task::spawn_blocking(move || {
        if let Some(path) = &path {
            if !supports_cli_preference(&tool) {
                return Err("This environment does not support a CLI preference.".into());
            }
            let available = if path == BUNDLED_CLI_PREFERENCE {
                enabled.is_none() && verified_bundled_cli(&tool).is_some()
            } else {
                is_discovered_tool_launcher(&tool, Path::new(path))
                    && (enabled.is_some()
                        || enumerate_tool_installations(&tool)
                            .iter()
                            .any(|entry| entry.path == *path && entry.runnable))
            };
            if !available {
                return Err("The selected CLI is no longer available.".into());
            }
        }
        crate::settings::set_cli_installation_policy(tool.clone(), path, enabled)
            .map_err(|error| error.to_string())?;
        Ok(preferred_tool_installation(&tool))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Signed official desktop apps own their updates; installation is independent of configuration.
#[derive(serde::Serialize)]
pub struct OfficialDesktopInstallation {
    path: PathBuf,
    version: Option<String>,
}

#[cfg(target_os = "macos")]
fn official_desktop_installation(
    name: &str,
    identifier: &str,
    team: &str,
) -> Option<OfficialDesktopInstallation> {
    let paths = std::iter::once(PathBuf::from("/Applications").join(name))
        .chain(dirs::home_dir().map(|home| home.join("Applications").join(name)));
    for path in paths.filter(|path| path.is_dir()) {
        if !crate::config::macos_has_developer_signature(&path, identifier, team) {
            continue;
        }
        let version = plist::Value::from_file(path.join("Contents/Info.plist"))
            .ok()
            .and_then(|info| {
                info.as_dictionary()?
                    .get("CFBundleShortVersionString")?
                    .as_string()
                    .map(str::to_owned)
            });
        return Some(OfficialDesktopInstallation { path, version });
    }
    None
}

#[tauri::command]
pub async fn get_official_claude_desktop_installation() -> Option<OfficialDesktopInstallation> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(|| {
            official_desktop_installation(
                "Claude.app",
                "com.anthropic.claudefordesktop",
                "Q6L2SF6YDW",
            )
        })
        .await
        .ok()
        .flatten()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[tauri::command]
pub async fn get_official_workbuddy_installation() -> Option<OfficialDesktopInstallation> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(|| {
            official_desktop_installation(
                "WorkBuddy.app",
                "com.tencent.workbuddy.mac",
                "FN2V63AD2J",
            )
        })
        .await
        .ok()
        .flatten()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
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
    #[test]
    fn installer_binding_keeps_arguments_and_rejects_other_managers() {
        assert_eq!(
            bind_install_executable(
                "npm install -g @openai/codex@latest",
                "npm",
                r#""C:\node tools\npm.cmd""#
            )
            .unwrap(),
            r#""C:\node tools\npm.cmd" install -g @openai/codex@latest"#
        );
        assert!(bind_install_executable("npm-extra install x", "npm", "npm.cmd").is_err());
        assert!(bind_install_executable("yarn global add x", "npm", "npm.cmd").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn confirmed_homebrew_update_cannot_change_destination_or_command() {
        let dir = tempfile::tempdir().unwrap();
        let brew = dir.path().join("bin/brew");
        std::fs::create_dir_all(brew.parent().unwrap()).unwrap();
        std::fs::write(&brew, "").unwrap();
        let install = ToolInstallation {
            path: dir.path().join("bin/claude").display().to_string(),
            version: Some("2.1.323".into()),
            runnable: true,
            error: None,
            source: "homebrew".into(),
            is_path_default: false,
            real: dir.path().join("Caskroom/claude-code/2.1.323/claude"),
            exited_with_error: false,
        };
        let plan = plan_selected_tool_update("claude", &install).unwrap();
        assert_eq!(plan.source, "homebrew");
        assert!(plan.command.contains("upgrade --cask 'claude-code'"));
        assert!(
            matching_tool_update_plan("claude", std::slice::from_ref(&install), &plan).is_some()
        );

        let mut altered = plan.clone();
        altered.command.push_str(" && another-command");
        assert!(
            matching_tool_update_plan("claude", std::slice::from_ref(&install), &altered).is_none()
        );
        altered = plan;
        altered.real_path = "/elsewhere/claude".into();
        assert!(matching_tool_update_plan("claude", &[install], &altered).is_none());
    }

    use super::*;

    #[tokio::test]
    async fn preferred_cli_rejects_launchers_outside_discovery() {
        let dir = tempfile::tempdir().unwrap();
        for tool in ["codex", "dsh"] {
            assert!(supports_cli_preference(tool));
            let launcher = dir.path().join(tool);
            std::fs::write(&launcher, b"unsupported launcher").unwrap();
            let error = set_preferred_tool_installation(
                tool.into(),
                Some(launcher.to_str().unwrap().into()),
                Some(false),
            )
            .await
            .unwrap_err();
            assert_eq!(error, "The selected CLI is no longer available.");
        }
    }

    #[test]
    fn npm_channel_version_matches_script_allowlist_support() {
        assert_eq!(npm_supports_script_allowlist("10.9.4"), Some(false));
        assert_eq!(npm_supports_script_allowlist("11.15.0"), Some(false));
        assert_eq!(npm_supports_script_allowlist("11.16.0\n"), Some(true));
        assert_eq!(npm_supports_script_allowlist("12.0.0"), Some(true));
        assert_eq!(npm_supports_script_allowlist("unknown"), None);
    }

    #[test]
    fn yarn_global_installs_require_classic() {
        assert!(is_yarn_classic("1.22.22\n"));
        for version in ["2.0.0", "4.5.1", "unknown", ""] {
            assert!(!is_yarn_classic(version));
        }
    }

    #[test]
    fn direct_install_commands_match_the_selected_app_and_platform() {
        assert_eq!(
            app_install_command("codex", InstallMethod::Homebrew, "macos"),
            Ok("brew install --cask codex")
        );
        assert_eq!(
            app_install_command("opencode", InstallMethod::Homebrew, "linux"),
            Ok("brew install anomalyco/tap/opencode-v2")
        );
        assert!(app_install_command("codex", InstallMethod::Curl, "linux")
            .unwrap()
            .contains("https://chatgpt.com/codex/install.sh"));
        assert!(app_install_command("codex", InstallMethod::Homebrew, "linux").is_err());
        assert!(app_install_command("codex", InstallMethod::Curl, "windows").is_err());
        assert_eq!(
            app_install_command("codebuddy", InstallMethod::Npm, "windows"),
            Ok("npm install -g @tencent-ai/codebuddy-code")
        );
        assert_eq!(
            app_install_command("codebuddy", InstallMethod::Pnpm, "linux"),
            Ok("pnpm add -g @tencent-ai/codebuddy-code")
        );
        assert!(app_install_command("opencode", InstallMethod::NpmModern, "windows").is_err());
        assert!(!pnpm_supports_allow_build("10.3.0"));
        assert!(pnpm_supports_allow_build("10.4.0"));
        assert!(app_install_command("workbuddy", InstallMethod::Homebrew, "macos").is_err());
    }

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
    fn windows_manager_paths_with_carets_and_percent_keep_exit_status() {
        let temp = tempfile::tempdir().unwrap();
        let managers = temp.path().join("manager ^ literal%PATH% tools");
        let scripts = temp.path().join("scripts^literal%PATH%cache");
        std::fs::create_dir_all(&managers).unwrap();
        std::fs::create_dir_all(&scripts).unwrap();
        let system_dir = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let native = managers.join("bun.exe");
        std::fs::copy(system_dir.join("cmd.exe"), &native).unwrap();
        let shim = managers.join("npm.cmd");
        std::fs::write(&shim, "@exit /b 17\r\n").unwrap();
        for (manager, path, args, expected) in [
            ("bun", native, "/D /C exit 23", 23),
            ("npm", shim, "--version", 17),
        ] {
            let text = windows_manager_batch(&format!("{manager} {args}"), manager, &path).unwrap();
            assert!(!text.contains("call "));
            let batch = scripts.join(format!("{manager}.bat"));
            std::fs::write(&batch, text).unwrap();
            let output = windows_lifecycle_command(&batch, system_dir.to_str().unwrap())
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(expected), "{output:?}");
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_lifecycle_ignores_a_project_local_manager() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("untrusted-project");
        let managers = temp.path().join("trusted-managers");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&managers).unwrap();
        std::fs::write(project.join("npm.cmd"), "@echo wrong-project-manager\r\n").unwrap();
        std::fs::write(managers.join("npm.cmd"), "@echo verified-path-manager\r\n").unwrap();
        let batch = temp.path().join("install.bat");
        std::fs::write(&batch, "@echo off\r\ncall npm --version\r\n").unwrap();
        let system_dir = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let effective = format!("{};{}", managers.display(), system_dir.display());
        let mut command = windows_lifecycle_command(&batch, &effective);
        assert_eq!(
            command.get_program(),
            system_dir.join("cmd.exe").as_os_str()
        );
        assert_eq!(command.get_current_dir(), Some(system_dir.as_path()));
        // Even an explicitly overridden CWD must not become an executable lookup root.
        let output = command.current_dir(&project).output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            decode_command_output(&output.stdout).trim(),
            "verified-path-manager"
        );
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

    #[test]
    fn npm_bin_collision_detection_and_hint() {
        let raw = [
            "npm error code EEXIST",
            "npm error path /home/me/.local/bin/pi",
            "npm error EEXIST: file already exists",
            "npm error File exists: /home/me/.local/bin/pi",
            "npm error Remove the existing file and try again, or run npm",
            "npm error with --force to overwrite files recklessly.",
            "npm notice",
            "npm notice New major version of npm available! 10.9.2 -> 11.6.0",
            "npm error A complete log of this run can be found in: /tmp/debug-0.log",
        ]
        .join("\n");
        let detail = append_npm_bin_collision_hint("npm error code EEXIST".to_string(), &raw);
        assert!(detail.contains("/home/me/.local/bin/pi.bak"));
        assert!(detail.contains("Do not use npm --force"));
        assert_eq!(
            npm_bin_collision_detail(&raw).as_deref(),
            Some("npm error code EEXIST\nnpm error File exists: /home/me/.local/bin/pi")
        );

        let win_raw =
            "npm error code EEXIST\nnpm error File exists: C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd";
        assert_eq!(
            npm_bin_collision_path(win_raw),
            Some("C:\\Users\\me\\AppData\\Roaming\\npm\\pi.cmd")
        );

        // Non-collision errors are untouched.
        let other = "some tool failed with EEXIST while writing its cache";
        assert_eq!(
            append_npm_bin_collision_hint("cache write failed".to_string(), other),
            "cache write failed"
        );
        assert_eq!(npm_bin_collision_detail("npm error code E404"), None);
        assert_eq!(npm_bin_collision_detail("EEXIST without a path"), None);
    }

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
    fn mcode_receipt_validation() {
        // Claims custom POSIX/Windows dirs (including root named `bin` and UTF-8 BOM).
        for (root_name, layout, bom) in [
            ("mcode", McodeLayout::Posix, false),
            ("bin", McodeLayout::Posix, false),
            ("mcode", McodeLayout::Windows, true),
            ("bin", McodeLayout::Windows, false),
        ] {
            let (_dir, root, entry) = mcode_custom_install(root_name, layout);
            write_mcode_receipt(&root, "minimax-code", "npm-prefix", &root, bom);
            assert_eq!(
                mcode_receipt_install_root(&entry, layout).as_deref(),
                Some(root.as_str()),
                "{root_name} / {layout:?}"
            );
        }

        // End-to-end: single install reruns installer in place instead of global npm.
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

        // Missing, foreign, or copied-prefix receipts are rejected.
        let layout = McodeLayout::NATIVE;
        let (_dir, _root, no_receipt) = mcode_custom_install("mcode", layout);
        assert_eq!(mcode_receipt_install_root(&no_receipt, layout), None);
        for (product, owner) in [
            ("other", "npm-prefix"),
            ("minimax-code", "managed-installer"),
        ] {
            let (_dir, root, entry) = mcode_custom_install("mcode", layout);
            write_mcode_receipt(&root, product, owner, &root, false);
            assert_eq!(mcode_receipt_install_root(&entry, layout), None);
        }
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
    #[test]
    fn wsl_validation_and_path_helpers() {
        for ok in ["bash", "zsh", "sh", "fish", "dash", "/usr/bin/bash"] {
            assert!(is_valid_shell(ok), "{ok}");
        }
        for bad in ["powershell", "cmd", ""] {
            assert!(!is_valid_shell(bad), "{bad}");
        }
        assert!(
            is_valid_shell_flag("-c") && is_valid_shell_flag("-lc") && is_valid_shell_flag("-lic")
        );
        assert!(!is_valid_shell_flag("-x") && !is_valid_shell_flag("--login"));
        assert_eq!(default_flag_for_shell("/bin/dash"), "-c");
        assert_eq!(default_flag_for_shell("fish"), "-lc");
        assert_eq!(default_flag_for_shell("/usr/bin/zsh"), "-lic");

        assert!(is_valid_wsl_distro_name("Ubuntu-22.04") && is_valid_wsl_distro_name("my_distro"));
        assert!(
            !is_valid_wsl_distro_name("")
                && !is_valid_wsl_distro_name("distro with spaces")
                && !is_valid_wsl_distro_name(&"a".repeat(65))
        );

        assert_eq!(
            merge_path_segments_win(&[r"C:\a;C:\B;%SystemRoot%\system32", r"C:\b;C:\a", "",]),
            r"C:\a;C:\B;%SystemRoot%\system32"
        );
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
}
