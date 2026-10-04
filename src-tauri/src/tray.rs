//! 托盘菜单（v7）
//!
//! 结构：问题区（出问题才有）→ 反馈行（托盘里刚做完要重启才生效的操作）→ 打开 CC Switch →
//! 切换式应用的子菜单（Claude Code、Claude Desktop、Codex、Gemini CLI、Grok Build；在「应用」页
//! 隐藏的不列）→ 轻量模式 → 打开官方网站 / 退出 CC Switch。累加式应用（OpenCode / OpenClaw /
//! Hermes / Pi / MiniMax Code）不进托盘；托盘里不切模式、不启停路由服务。
//!
//! 分三层：`collect_*` 从数据库和设备状态读出快照，`build_menu_model` 把快照变成纯数据的菜单
//! 模型（单测覆盖这一层），`attach_*` 把模型挂到 Tauri 原生菜单上。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItem, Menu, MenuBuilder, MenuItem, MenuItemKind, Submenu, SubmenuBuilder,
};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::usage_cache::UsageCache;
use crate::store::AppState;

const TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION: &str = "official_subscription";
/// Copilot 用量结果的单位（`commands::provider` 的 `COPILOT_UNIT_PREMIUM`）：高级请求次数。
const COPILOT_UNIT_PREMIUM: &str = "requests";

pub const TRAY_ID: &str = "cc-switch";

/// 进托盘的应用，顺序和侧栏一致。累加式应用没有「当前供应商」，不进托盘。
pub const TRAY_APPS: [AppType; 5] = [
    AppType::Claude,
    AppType::ClaudeDesktop,
    AppType::Codex,
    AppType::Gemini,
    AppType::GrokBuild,
];

/// 应用全称（产品名，不翻译）。用全称是为了把 Claude Code 和 Claude Desktop 分开。
fn app_display_name(app: &AppType) -> &'static str {
    match app {
        AppType::Claude => "Claude Code",
        AppType::ClaudeDesktop => "Claude Desktop",
        AppType::Codex => "Codex",
        AppType::Gemini => "Gemini CLI",
        AppType::GrokBuild => "Grok Build",
        AppType::OpenCode => "OpenCode",
        AppType::OpenClaw => "OpenClaw",
        AppType::Hermes => "Hermes",
        AppType::Pi => "Pi",
        AppType::Mcode => "MiniMax Code",
    }
}

/// 问题区最多几行，再多写「还有 N 个问题」。
const MAX_PROBLEM_ROWS: usize = 2;
/// 原生菜单不会自己截断，名字太长会把整个菜单撑宽。
const MAX_NAME_CHARS: usize = 32;
/// 问题区里切换失败的原因最多几个字。
const MAX_REASON_CHARS: usize = 60;
/// 额度剩余不到这个百分比算「快用完」（和前端 `quotaRules.WARN_BELOW_PERCENT` 一致）。
const WARN_BELOW_PERCENT: f64 = 10.0;

/// 每个应用行的子菜单句柄，额度更新时就地改标题而不是整菜单重建（整建会关掉 macOS 上
/// 正开着的菜单）。`create_tray_menu` 每次重建都整表覆盖写入。
static TRAY_SECTION_SUBMENUS: Lazy<Mutex<HashMap<AppType, Submenu<tauri::Wry>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ─── 文案 ────────────────────────────────────────────────────────────────────

/// 托盘菜单文本（四语）。托盘文案不走前端的 i18n JSON；和界面同义的词照抄前端的译法。
///
/// 模板占位：`{app}` `{name}` `{port}` `{mode}` `{reason}` `{count}` `{value}` `{when}`；
/// 档名用 `{label}`，中文模板用 `{labelSp}`（档名以字母数字结尾时自动补一个空格）。
#[derive(Clone, Copy)]
pub struct TrayTexts {
    pub show_main: &'static str,
    pub open_website: &'static str,
    pub lightweight_mode: &'static str,
    pub quit: &'static str,
    /// 有应用在用路由服务时的「退出」：写明后果（客户端指回直连、路由服务停止，下次打开自动接回）。
    pub quit_stops_routing: &'static str,
    pub projects_label: &'static str,
    pub no_project_label: &'static str,
    pub header_direct: &'static str,
    pub header_route: &'static str,
    pub header_failover: &'static str,
    pub header_stack: &'static str,
    /// 聚合子菜单标题，写出默认那家：`{name}`
    pub header_stack_default: &'static str,
    pub failover_note: &'static str,
    pub mode_route: &'static str,
    pub mode_stack: &'static str,
    pub mode_mapping: &'static str,
    pub needs_routing_suffix: &'static str,
    pub official_blocked_suffix: &'static str,
    pub mapping_suffix: &'static str,
    pub open_app_page: &'static str,
    pub add_provider: &'static str,
    pub almost_out: &'static str,
    pub needs_attention: &'static str,
    pub problem_service_down: &'static str,
    pub problem_attach_failed: &'static str,
    pub problem_switch_failed: &'static str,
    pub problem_more: &'static str,
    pub desktop_unavailable: &'static str,
    /// 出问题时托盘图标的悬停提示：`{problem}` 是问题区第一条
    pub tooltip_problem: &'static str,
    /// 反馈行（灰字）：直连切换 / Claude Desktop 切换，`{app}` `{name}`
    pub feedback_switched: &'static str,
    /// 反馈行：聚合换默认
    pub feedback_stack_default: &'static str,
    /// 反馈行：应用项目
    pub feedback_profile: &'static str,
    pub tier_five_hour: &'static str,
    pub tier_weekly: &'static str,
    pub tier_fable: &'static str,
    pub tier_monthly: &'static str,
    pub tier_thirty_day: &'static str,
    pub tier_credits: &'static str,
    pub tier_gemini_pro: &'static str,
    pub tier_gemini_flash: &'static str,
    pub tier_gemini_flash_lite: &'static str,
    pub tier_premium: &'static str,
    pub tier_left: &'static str,
    pub tier_used_up: &'static str,
    pub balance: &'static str,
    pub balance_used_up: &'static str,
    pub plan_expired: &'static str,
    pub used_amount: &'static str,
    pub quota_failed: &'static str,
    pub quota_failed_login_expired: &'static str,
    pub quota_failed_token_refresh_pending: &'static str,
    pub reset_on_date: &'static str,
    pub reset_at_time: &'static str,
    /// chrono 格式串：重置日期
    pub date_format: &'static str,
}

/// 将系统区域标识映射为托盘支持的语言码。
///
/// 镜像前端 `i18n/getInitialLanguage` 的判定顺序，确保首次安装
/// （`settings.language` 尚未写入）时托盘语言与界面语言一致：
/// 繁中系统（zh-TW/HK/MO/Hant）→ `zh-TW`，其余 zh → `zh`，
/// 日文 → `ja`，英文 → `en`，未知区域回退到 `zh`（与前端默认一致）。
fn map_locale_to_tray_language(locale: &str) -> &'static str {
    let locale = locale.to_lowercase();
    if locale == "zh" {
        "zh"
    } else if locale.starts_with("zh-tw")
        || locale.starts_with("zh-hk")
        || locale.starts_with("zh-mo")
        || locale.starts_with("zh-hant")
    {
        "zh-TW"
    } else if locale.starts_with("zh") {
        "zh"
    } else if locale.starts_with("ja") {
        "ja"
    } else if locale.starts_with("en") {
        "en"
    } else {
        "zh"
    }
}

/// 读取系统区域并映射为托盘语言码；取不到区域时回退到 `zh`。
fn detect_system_tray_language() -> &'static str {
    sys_locale::get_locale()
        .as_deref()
        .map(map_locale_to_tray_language)
        .unwrap_or("zh")
}

impl TrayTexts {
    pub fn from_language(language: &str) -> Self {
        match language {
            "en" => Self {
                show_main: "Open CC Switch",
                open_website: "Open official website",
                lightweight_mode: "Lightweight mode",
                quit: "Quit CC Switch",
                quit_stops_routing: "Quit CC Switch (stops the routing service)",
                projects_label: "Projects",
                no_project_label: "No project",
                header_direct: "Direct",
                header_route: "Routing",
                header_failover: "Routing · Failover on",
                header_stack: "Aggregation · Default provider",
                header_stack_default: "Aggregation · Default: {name}",
                failover_note: "Picked from the queue automatically; change it on the app page",
                mode_route: "Routing",
                mode_stack: "Aggregation",
                mode_mapping: "Model mapping",
                needs_routing_suffix: " (needs routing)…",
                official_blocked_suffix: " (official plans don't go through routing)",
                mapping_suffix: " · Model mapping",
                open_app_page: "Open {app} page",
                add_provider: "Add provider…",
                almost_out: "almost out",
                needs_attention: "needs attention",
                problem_service_down: "The routing service isn't running (port {port})",
                problem_attach_failed:
                    "{app}: {mode} couldn't reconnect at startup; back to direct",
                problem_switch_failed: "{app} didn't switch: {reason}",
                problem_more: "{count} more issues — open CC Switch to see them",
                desktop_unavailable:
                    "The routing service isn't running; {name} is unavailable for now",
                tooltip_problem: "CC Switch · Needs attention: {problem}",
                feedback_switched: "{app} switched to {name}. Restart {app} to apply",
                feedback_stack_default:
                    "{app}'s default provider is now {name}. Restart {app} to apply",
                feedback_profile: "Applied project \u{201c}{name}\u{201d} to {app}",
                tier_five_hour: "5-hour",
                tier_weekly: "Weekly",
                tier_fable: "Fable",
                tier_monthly: "Monthly",
                tier_thirty_day: "30-day",
                tier_credits: "Credits",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "Premium",
                tier_left: "{label} {value}% left",
                tier_used_up: "{label} used up",
                balance: "Balance {value}",
                balance_used_up: "Balance used up",
                plan_expired: "Plan expired",
                used_amount: "Used {value}",
                quota_failed: "Quota unavailable",
                quota_failed_login_expired: "Quota unavailable: sign-in expired",
                quota_failed_token_refresh_pending: "Quota unavailable: token refresh pending",
                reset_on_date: "{label} quota resets {when}",
                reset_at_time: "{label} quota resets at {when}",
                date_format: "%b %-d",
            },
            "ja" => Self {
                show_main: "CC Switch を開く",
                open_website: "公式サイトを開く",
                lightweight_mode: "軽量モード",
                quit: "CC Switch を終了",
                quit_stops_routing: "CC Switch を終了（ルーティングサービスが停止します）",
                projects_label: "プロジェクト",
                no_project_label: "プロジェクトを使用しない",
                header_direct: "直接接続",
                header_route: "ルーティング",
                header_failover: "ルーティング · フェイルオーバー有効",
                header_stack: "集約 · デフォルトのプロバイダー",
                header_stack_default: "集約 · デフォルト：{name}",
                failover_note: "キューの順に自動で選ばれます（変更はアプリのページで）",
                mode_route: "ルーティング",
                mode_stack: "集約",
                mode_mapping: "モデルマッピング",
                needs_routing_suffix: "（ルーティングが必要）…",
                official_blocked_suffix: "（公式サブスクリプションはルーティングを通りません）",
                mapping_suffix: " · モデルマッピング",
                open_app_page: "{app} のページを開く",
                add_provider: "プロバイダーを追加…",
                almost_out: "残りわずか",
                needs_attention: "対応が必要",
                problem_service_down: "ルーティングサービスが動いていません（ポート {port}）",
                problem_attach_failed: "{app}：前回の{mode}に再接続できず、直接接続に戻りました",
                problem_switch_failed: "{app} を切り替えられませんでした：{reason}",
                problem_more:
                    "ほかに {count} 件の問題があります。CC Switch を開いて確認してください",
                desktop_unavailable:
                    "ルーティングサービスが動いていないため、{name} は今使えません",
                tooltip_problem: "CC Switch · 対応が必要：{problem}",
                feedback_switched:
                    "{app} を {name} に切り替えました。反映するには {app} を再起動してください",
                feedback_stack_default: "{app} のデフォルトのプロバイダーを {name} に変更しました。反映するには {app} を再起動してください",
                feedback_profile: "プロジェクト「{name}」を {app} に適用しました",
                tier_five_hour: "5時間",
                tier_weekly: "週間",
                tier_fable: "Fable",
                tier_monthly: "月間",
                tier_thirty_day: "30日間",
                tier_credits: "クレジット",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "プレミアム",
                tier_left: "{label} 残り {value}%",
                tier_used_up: "{label} 使い切り",
                balance: "残高 {value}",
                balance_used_up: "残高なし",
                plan_expired: "プラン期限切れ",
                used_amount: "使用 {value}",
                quota_failed: "残量を取得できません",
                quota_failed_login_expired: "残量を取得できません：ログインの期限切れ",
                quota_failed_token_refresh_pending: "残量を取得できません：トークンの更新待ち",
                reset_on_date: "{label}の枠は {when} にリセット",
                reset_at_time: "{label}の枠は {when} にリセット",
                date_format: "%-m月%-d日",
            },
            "zh-TW" => Self {
                show_main: "開啟 CC Switch",
                open_website: "開啟官方網站",
                lightweight_mode: "輕量模式",
                quit: "退出 CC Switch",
                quit_stops_routing: "退出 CC Switch（路由服務會停止）",
                projects_label: "專案",
                no_project_label: "不使用專案",
                header_direct: "直連",
                header_route: "路由",
                header_failover: "路由 · 故障轉移開啟中",
                header_stack: "聚合 · 預設供應商",
                header_stack_default: "聚合 · 預設 {name}",
                failover_note: "依佇列自動選擇，要調整請到應用頁",
                mode_route: "路由",
                mode_stack: "聚合",
                mode_mapping: "模型映射",
                needs_routing_suffix: "（需要路由）…",
                official_blocked_suffix: "（官方訂閱不經過路由）",
                mapping_suffix: " · 模型映射",
                open_app_page: "開啟 {app} 頁面",
                add_provider: "新增供應商…",
                almost_out: "快用完",
                needs_attention: "需要處理",
                problem_service_down: "路由服務沒在執行（連接埠 {port}）",
                problem_attach_failed: "{app}：上次的{mode}沒能接上，已回到直連",
                problem_switch_failed: "{app} 沒切換成功：{reason}",
                problem_more: "還有 {count} 個問題，開啟 CC Switch 查看",
                desktop_unavailable: "路由服務沒在執行，{name} 暫時無法使用",
                tooltip_problem: "CC Switch · 需要處理：{problem}",
                feedback_switched: "{app} 已切換到 {name}，重新啟動 {app} 後生效",
                feedback_stack_default: "{app} 的預設供應商改為 {name}，重新啟動 {app} 後生效",
                feedback_profile: "已把專案「{name}」套用到 {app}",
                tier_five_hour: "5 小時",
                tier_weekly: "每週",
                tier_fable: "Fable",
                tier_monthly: "每月",
                tier_thirty_day: "30 天",
                tier_credits: "額度",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "進階請求",
                tier_left: "{labelSp}剩餘 {value}%",
                tier_used_up: "{labelSp}已用完",
                balance: "餘額 {value}",
                balance_used_up: "餘額已用完",
                plan_expired: "方案已過期",
                used_amount: "已使用 {value}",
                quota_failed: "額度沒查到",
                quota_failed_login_expired: "額度沒查到：登入已過期",
                quota_failed_token_refresh_pending: "額度沒查到：權杖待重新整理",
                reset_on_date: "{labelSp}額度 {when}重置",
                reset_at_time: "{labelSp}額度 {when} 重置",
                date_format: "%-m 月 %-d 日",
            },
            _ => Self {
                show_main: "打开 CC Switch",
                open_website: "打开官方网站",
                lightweight_mode: "轻量模式",
                quit: "退出 CC Switch",
                quit_stops_routing: "退出 CC Switch（路由服务会停止）",
                projects_label: "项目",
                no_project_label: "不使用项目",
                header_direct: "直连",
                header_route: "路由",
                header_failover: "路由 · 故障转移开启中",
                header_stack: "聚合 · 默认供应商",
                header_stack_default: "聚合 · 默认 {name}",
                failover_note: "按队列自动选择，要调整请到应用页",
                mode_route: "路由",
                mode_stack: "聚合",
                mode_mapping: "模型映射",
                needs_routing_suffix: "（需要路由）…",
                official_blocked_suffix: "（官方订阅不经过路由）",
                mapping_suffix: " · 模型映射",
                open_app_page: "打开 {app} 页面",
                add_provider: "添加供应商…",
                almost_out: "快用完",
                needs_attention: "需要处理",
                problem_service_down: "路由服务没在运行（端口 {port}）",
                problem_attach_failed: "{app}：上次的{mode}没能接上，已回到直连",
                problem_switch_failed: "{app} 没切换成功：{reason}",
                problem_more: "还有 {count} 个问题，打开 CC Switch 查看",
                desktop_unavailable: "路由服务没在运行，{name} 暂时不可用",
                tooltip_problem: "CC Switch · 需要处理：{problem}",
                feedback_switched: "{app} 已切换到 {name}，重启 {app} 后生效",
                feedback_stack_default: "{app} 的默认供应商改为 {name}，重启 {app} 后生效",
                feedback_profile: "已把项目「{name}」用到 {app}",
                tier_five_hour: "5 小时",
                tier_weekly: "每周",
                tier_fable: "Fable",
                tier_monthly: "每月",
                tier_thirty_day: "30 天",
                tier_credits: "额度",
                tier_gemini_pro: "Pro",
                tier_gemini_flash: "Flash",
                tier_gemini_flash_lite: "Flash Lite",
                tier_premium: "高级请求",
                tier_left: "{labelSp}剩余 {value}%",
                tier_used_up: "{labelSp}已用完",
                balance: "余额 {value}",
                balance_used_up: "余额已用完",
                plan_expired: "套餐已过期",
                used_amount: "已使用 {value}",
                quota_failed: "额度没查到",
                quota_failed_login_expired: "额度没查到：登录已过期",
                quota_failed_token_refresh_pending: "额度没查到：令牌待刷新",
                reset_on_date: "{labelSp}额度 {when}重置",
                reset_at_time: "{labelSp}额度 {when} 重置",
                date_format: "%-m 月 %-d 日",
            },
        }
    }

    /// 按设置里的语言取文案；没设过语言（首次安装）时按系统区域，而不是固定简体。
    fn current() -> Self {
        let settings = crate::settings::get_settings();
        let language = match settings.language.as_deref() {
            Some(lang) => lang,
            None => detect_system_tray_language(),
        };
        Self::from_language(language)
    }
}

/// 按占位符填模板。
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut text = template.to_string();
    for (key, value) in values {
        text = text.replace(&format!("{{{key}}}"), value);
    }
    text
}

/// 中文里档名以字母数字结尾（「Pro」「每周 Opus」）时，和后面的「剩余」隔一个空格。
fn fill_label(template: &str, label: &str, extra: &[(&str, &str)]) -> String {
    let label_sp = if label.ends_with(|c: char| c.is_ascii_alphanumeric()) {
        format!("{label} ")
    } else {
        label.to_string()
    };
    let mut values = vec![("labelSp", label_sp.as_str()), ("label", label)];
    values.extend_from_slice(extra);
    fill(template, &values)
}

fn truncate_chars(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ─── 额度文字（和供应商卡片同一套：一律写剩余，快用完 / 已用完才多说一句）────────────

/// 托盘里合并的档：周限额的几个别名取最高利用率，Fable 单列；月窗口里 Codex 免费版的 30 天
/// 窗口也算（#3651）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TierGroup {
    FiveHour,
    Weekly,
    Fable,
    Monthly,
    Credits,
    GeminiPro,
    GeminiFlash,
    GeminiFlashLite,
    Premium,
}

const TIER_GROUPS: &[(TierGroup, &[&str])] = {
    use crate::services::subscription as s;
    &[
        (TierGroup::FiveHour, &[s::TIER_FIVE_HOUR]),
        (
            TierGroup::Weekly,
            &[
                s::TIER_WEEKLY_LIMIT,
                s::TIER_SEVEN_DAY,
                s::TIER_SEVEN_DAY_OPUS,
                s::TIER_SEVEN_DAY_SONNET,
            ],
        ),
        (TierGroup::Fable, &[s::TIER_SEVEN_DAY_FABLE]),
        (TierGroup::Monthly, &[s::TIER_MONTHLY, s::TIER_THIRTY_DAY]),
        (TierGroup::Credits, &[s::TIER_CREDITS]),
        (TierGroup::GeminiPro, &[s::TIER_GEMINI_PRO]),
        (TierGroup::GeminiFlash, &[s::TIER_GEMINI_FLASH]),
        (TierGroup::GeminiFlashLite, &[s::TIER_GEMINI_FLASH_LITE]),
        (TierGroup::Premium, &["premium"]),
    ]
};

fn tier_label(texts: &TrayTexts, group: TierGroup, tier_name: &str) -> &'static str {
    match group {
        TierGroup::FiveHour => texts.tier_five_hour,
        TierGroup::Weekly => texts.tier_weekly,
        TierGroup::Fable => texts.tier_fable,
        TierGroup::Monthly if tier_name == crate::services::subscription::TIER_THIRTY_DAY => {
            texts.tier_thirty_day
        }
        TierGroup::Monthly => texts.tier_monthly,
        TierGroup::Credits => texts.tier_credits,
        TierGroup::GeminiPro => texts.tier_gemini_pro,
        TierGroup::GeminiFlash => texts.tier_gemini_flash,
        TierGroup::GeminiFlashLite => texts.tier_gemini_flash_lite,
        TierGroup::Premium => texts.tier_premium,
    }
}

#[derive(Debug, Clone, PartialEq)]
struct QuotaLine {
    text: String,
    /// 剩余百分比；余额没有总额时是 `INFINITY`，套餐过期是负数。
    left: f64,
    /// 档名（重置说明用；余额等没有档名）。
    label: Option<&'static str>,
    resets_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaFailure {
    /// 没有更具体的原因（脚本结果只有成功 / 失败）。
    Other,
    LoginExpired,
    /// 访问令牌过期、刷新令牌还在：客户端下次运行时自己会换新的。
    TokenRefreshPending,
}

#[derive(Debug, Clone, PartialEq)]
enum QuotaView {
    Lines(Vec<QuotaLine>),
    Failed(QuotaFailure),
}

struct TierEntry<'a> {
    name: &'a str,
    utilization: f64,
    resets_at: Option<String>,
}

fn tier_line(
    texts: &TrayTexts,
    label: &'static str,
    utilization: f64,
    resets_at: Option<String>,
) -> QuotaLine {
    let left = (100.0 - utilization).round().max(0.0);
    let text = if left <= 0.0 {
        fill_label(texts.tier_used_up, label, &[])
    } else {
        let value = format!("{}", left as i64);
        fill_label(texts.tier_left, label, &[("value", value.as_str())])
    };
    QuotaLine {
        text,
        left,
        label: Some(label),
        resets_at,
    }
}

/// 已知档位按组合并成额度行（每组取利用率最高的那档）。
fn grouped_tier_lines(texts: &TrayTexts, entries: &[TierEntry<'_>]) -> Vec<QuotaLine> {
    let mut lines = Vec::new();
    for &(group, names) in TIER_GROUPS {
        let worst = entries
            .iter()
            .filter(|entry| names.contains(&entry.name) && entry.utilization.is_finite())
            .max_by(|a, b| {
                a.utilization
                    .partial_cmp(&b.utilization)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(entry) = worst {
            lines.push(tier_line(
                texts,
                tier_label(texts, group, entry.name),
                entry.utilization,
                entry.resets_at.clone(),
            ));
        }
    }
    lines
}

fn is_known_tier(name: &str) -> bool {
    TIER_GROUPS.iter().any(|(_, names)| names.contains(&name))
}

fn format_subscription_quota(
    texts: &TrayTexts,
    quota: &crate::services::subscription::SubscriptionQuota,
) -> Option<QuotaView> {
    use crate::services::subscription::CredentialStatus;
    if !quota.success {
        // 没有凭据 / 凭据读不懂时不说话（和卡片一致）。
        return match quota.credential_status {
            CredentialStatus::NotFound | CredentialStatus::ParseError => None,
            CredentialStatus::Expired => Some(QuotaView::Failed(QuotaFailure::LoginExpired)),
            CredentialStatus::RefreshPending => {
                Some(QuotaView::Failed(QuotaFailure::TokenRefreshPending))
            }
            CredentialStatus::Valid => Some(QuotaView::Failed(QuotaFailure::Other)),
        };
    }
    let entries: Vec<TierEntry<'_>> = quota
        .tiers
        .iter()
        .map(|tier| TierEntry {
            name: tier.name.as_str(),
            utilization: tier.utilization,
            resets_at: tier.resets_at.clone(),
        })
        .collect();
    let lines = grouped_tier_lines(texts, &entries);
    (!lines.is_empty()).then_some(QuotaView::Lines(lines))
}

fn tier_pct(data: &crate::provider::UsageData) -> Option<f64> {
    match (data.used, data.total) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

/// 脚本结果里 `extra` 带的重置时间：Token Plan 是 JSON 的 `resetsAt`，官方订阅是原样的时间串。
fn resets_at_from_extra(extra: Option<&str>) -> Option<String> {
    let extra = extra?.trim();
    if extra.starts_with('{') {
        return serde_json::from_str::<serde_json::Value>(extra)
            .ok()?
            .get("resetsAt")?
            .as_str()
            .map(str::to_string);
    }
    chrono::DateTime::parse_from_rfc3339(extra)
        .ok()
        .map(|_| extra.to_string())
}

fn amount(value: f64, unit: Option<&str>) -> String {
    match unit.map(str::trim).filter(|unit| !unit.is_empty()) {
        Some(unit) => format!("{value:.2} {unit}"),
        None => format!("{value:.2}"),
    }
}

/// 不认识档名的一条脚本结果（余额、Copilot、自定义脚本），照卡片 `UsageFooter.planLine`。
fn plan_line(texts: &TrayTexts, data: &crate::provider::UsageData) -> Option<QuotaLine> {
    if data.is_valid == Some(false) {
        return Some(QuotaLine {
            text: texts.plan_expired.to_string(),
            left: -1.0,
            label: None,
            resets_at: None,
        });
    }
    let unit = data.unit.as_deref();
    let total = data.total.filter(|total| *total != -1.0);
    if unit == Some(COPILOT_UNIT_PREMIUM) {
        if let (Some(remaining), Some(total)) = (data.remaining, total) {
            if total > 0.0 {
                let utilization = (total - remaining) / total * 100.0;
                return Some(tier_line(texts, texts.tier_premium, utilization, None));
            }
        }
    }
    if let Some(remaining) = data.remaining {
        let has_total = total.is_some_and(|total| total > 0.0);
        let left = if remaining <= 0.0 {
            0.0
        } else if has_total {
            remaining / total.unwrap_or(1.0) * 100.0
        } else {
            f64::INFINITY
        };
        let text = if remaining <= 0.0 {
            texts.balance_used_up.to_string()
        } else {
            fill(texts.balance, &[("value", &amount(remaining, unit))])
        };
        return Some(QuotaLine {
            text,
            left,
            label: None,
            resets_at: None,
        });
    }
    data.used.map(|used| QuotaLine {
        text: fill(texts.used_amount, &[("value", &amount(used, unit))]),
        left: f64::INFINITY,
        label: None,
        resets_at: None,
    })
}

fn format_script_result(
    texts: &TrayTexts,
    result: &crate::provider::UsageResult,
) -> Option<QuotaView> {
    if !result.success {
        return Some(QuotaView::Failed(QuotaFailure::Other));
    }
    let data = result.data.as_ref()?;
    // commands::provider 的 token_plan / official_subscription 分支把每档扁平化成一条
    // UsageData（plan_name 是档名），按档名恢复成档位；其余（余额、Copilot、自定义脚本）一条一行。
    let entries: Vec<TierEntry<'_>> = data
        .iter()
        .filter_map(|d| {
            let name = d.plan_name.as_deref()?;
            if !is_known_tier(name) {
                return None;
            }
            Some(TierEntry {
                name,
                utilization: tier_pct(d)?,
                resets_at: resets_at_from_extra(d.extra.as_deref()),
            })
        })
        .collect();
    let mut lines = grouped_tier_lines(texts, &entries);
    lines.extend(
        data.iter()
            .filter(|d| !d.plan_name.as_deref().is_some_and(is_known_tier))
            .filter_map(|d| plan_line(texts, d)),
    );
    (!lines.is_empty()).then_some(QuotaView::Lines(lines))
}

/// 标题里最多留两行：留剩余最少的，再按原顺序排回去（同卡片 `pickLines`）。
fn pick_lines(lines: &[QuotaLine], max: usize) -> Vec<&QuotaLine> {
    let mut order: Vec<usize> = (0..lines.len()).collect();
    if lines.len() > max {
        order.sort_by(|a, b| {
            lines[*a]
                .left
                .partial_cmp(&lines[*b].left)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        order.truncate(max);
        order.sort_unstable();
    }
    order.into_iter().map(|index| &lines[index]).collect()
}

fn worst_left(lines: &[QuotaLine]) -> f64 {
    lines
        .iter()
        .map(|line| line.left)
        .fold(f64::INFINITY, f64::min)
}

/// 应用行标题里的额度：`(文字, 快用完)`。查询失败时标题不写额度，原因写在子菜单里。
fn quota_title(view: &QuotaView) -> Option<(String, bool)> {
    let QuotaView::Lines(lines) = view else {
        return None;
    };
    let text = pick_lines(lines, 2)
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join(" · ");
    let worst = worst_left(lines);
    // 用完时额度本身写「已用完」，不再加「快用完」。
    Some((text, worst > 0.0 && worst < WARN_BELOW_PERCENT))
}

/// 子菜单里的额度说明行：没查到写原因；快用完 / 已用完且知道重置时间时写什么时候重置。
fn quota_note(
    texts: &TrayTexts,
    view: &QuotaView,
    now: chrono::DateTime<chrono::Local>,
) -> Option<String> {
    let lines = match view {
        QuotaView::Failed(reason) => {
            return Some(
                match reason {
                    QuotaFailure::LoginExpired => texts.quota_failed_login_expired,
                    QuotaFailure::TokenRefreshPending => texts.quota_failed_token_refresh_pending,
                    QuotaFailure::Other => texts.quota_failed,
                }
                .to_string(),
            )
        }
        QuotaView::Lines(lines) => lines,
    };
    let worst = lines
        .iter()
        .filter(|line| line.left < WARN_BELOW_PERCENT)
        .min_by(|a, b| {
            a.left
                .partial_cmp(&b.left)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    let label = worst.label?;
    let resets = chrono::DateTime::parse_from_rfc3339(worst.resets_at.as_deref()?)
        .ok()?
        .with_timezone(&chrono::Local);
    if resets <= now {
        return None;
    }
    if resets.date_naive() == now.date_naive() {
        let when = resets.format("%H:%M").to_string();
        Some(fill_label(texts.reset_at_time, label, &[("when", &when)]))
    } else {
        let when = resets.format(texts.date_format).to_string();
        Some(fill_label(texts.reset_on_date, label, &[("when", &when)]))
    }
}

fn managed_codex_account_id(provider: &Provider) -> Option<String> {
    if crate::proxy::providers::is_codex_official_provider(provider) {
        return provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
    }
    None
}

fn provider_uses_official_subscription(provider: &Provider) -> bool {
    // Managed Codex uses the account-scoped path in tray_usage_source instead
    // of the CLI's app-wide subscription cache.
    if managed_codex_account_id(provider).is_some() {
        return false;
    }

    provider
        .meta
        .as_ref()
        .and_then(|m| m.usage_script.as_ref())
        .map(|script| {
            script.enabled
                && script.template_type.as_deref() == Some(TEMPLATE_TYPE_OFFICIAL_SUBSCRIPTION)
        })
        .unwrap_or(false)
}

#[derive(Debug, PartialEq, Eq)]
enum TrayUsageSource {
    ManagedCodex(String),
    /// 客户端自己登录的官方订阅：和供应商卡片读写同一份应用级订阅缓存。
    Subscription,
    Script,
}

/// Keep the tray's refresh and display paths on the same credentials and toggle.
fn tray_usage_source(app_type: &AppType, provider: &Provider) -> Option<TrayUsageSource> {
    if *app_type == AppType::Codex {
        if let Some(account_id) = managed_codex_account_id(provider) {
            // Match ProviderCard: managed accounts query by default until the
            // user explicitly disables usage, including older saved providers.
            let enabled = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .map(|script| script.enabled)
                .unwrap_or(true);
            return enabled.then_some(TrayUsageSource::ManagedCodex(account_id));
        }
    }
    // xAI OAuth 的额度属于绑定的 SuperGrok 账号，不是所在应用的客户端登录，仍走脚本路径。
    if provider_uses_official_subscription(provider) && !provider.is_xai_oauth() {
        return Some(TrayUsageSource::Subscription);
    }
    (provider.has_usage_script_enabled()
        && (provider.category.as_deref() != Some("official")
            || provider_uses_official_subscription(provider)))
    .then_some(TrayUsageSource::Script)
}

/// 在用的那家的额度。🔴 #7267：托管 Codex 账号卡按账号取，缓存缺失时**不能**回落到应用级
/// 订阅缓存或供应商级脚本缓存。
fn usage_view(
    usage_cache: &UsageCache,
    texts: &TrayTexts,
    app_type: &AppType,
    provider: &Provider,
    provider_id: &str,
) -> Option<QuotaView> {
    // 当前脚本是否启用：禁用/删除时不再沿用旧 UsageCache 结果，
    // 并顺手 invalidate，防止后续重建继续命中过期数据。
    let source = tray_usage_source(app_type, provider);
    if let Some(TrayUsageSource::ManagedCodex(account_id)) = &source {
        // No fallback: a missing account snapshot must not display another
        // account's quota from a provider cache or the CLI subscription cache.
        return usage_cache
            .with_codex_oauth(account_id, |quota| format_subscription_quota(texts, quota))
            .flatten();
    }
    if source == Some(TrayUsageSource::Subscription) {
        // 只读订阅缓存：卡片查到的和托盘悬停查到的都写在这里，两边不会各说各的。
        return usage_cache
            .with_subscription(app_type, |quota| format_subscription_quota(texts, quota))
            .flatten();
    }
    // 在用的不是客户端自己的订阅：应用级订阅缓存是别家留下的，不能沿用。
    usage_cache.invalidate_subscription(app_type);
    if source.is_none() {
        usage_cache.invalidate_script(app_type, provider_id);
        return None;
    }
    // 脚本缓存（Copilot/coding_plan/balance/自定义脚本），借用访问避免克隆整条 UsageResult。
    usage_cache
        .with_script(app_type, provider_id, |result| {
            format_script_result(texts, result)
        })
        .flatten()
}

// ─── 「需要路由」判定（镜像前端 `providerNeedsRouting`）──────────────────────────────

const MANAGED_OAUTH_PROVIDER_TYPES: &[&str] = &["github_copilot", "codex_oauth", "xai_oauth"];

/// 官方账号卡（同前端 `isOfficialAccount`）：Codex 早期绑定托管账号的官方卡没有 category，按身份认。
fn is_official_account(app: &AppType, provider: &Provider) -> bool {
    provider.category.as_deref() == Some("official")
        || (*app == AppType::Codex && crate::proxy::providers::is_codex_official_provider(provider))
}

/// 官方订阅不经过路由（Codex 官方卡除外）：路由 / 聚合下不能选。
fn blocked_from_routing(app: &AppType, provider: &Provider) -> bool {
    is_official_account(app, provider)
        && !crate::services::provider::official_provider_supports_proxy_takeover(app, provider)
}

/// Codex / Grok Build 配置里当前供应商的 `wire_api`；TOML 读不懂时退回唯一的一条赋值。
fn codex_wire_api(config_text: &str) -> Option<String> {
    if let Ok(doc) = config_text.parse::<toml::Value>() {
        if let Some(active) = doc.get("model_provider").and_then(|v| v.as_str()) {
            if let Some(wire_api) = doc
                .get("model_providers")
                .and_then(|providers| providers.get(active))
                .and_then(|provider| provider.get("wire_api"))
                .and_then(|v| v.as_str())
            {
                return Some(wire_api.to_string());
            }
        }
        return doc
            .get("wire_api")
            .and_then(|v| v.as_str())
            .map(str::to_string);
    }
    let mut found = config_text.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "wire_api").then(|| {
            value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string()
        })
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

fn is_chat_or_anthropic_wire_api(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "chat"
            | "chat_completions"
            | "chat-completions"
            | "openai_chat"
            | "openai-chat"
            | "openai_chat_completions"
            | "anthropic"
            | "anthropic_messages"
            | "anthropic-messages"
            | "messages"
            | "claude"
    )
}

/// 这家在这个应用下必须经过路由服务才能用（直连写进去也用不了）。和前端
/// `providerNeedsRouting`（`src/utils/providerCapabilities.ts`）是同一条规则，改一边要改另一边。
pub(crate) fn provider_needs_routing(app: &AppType, provider: &Provider) -> bool {
    if is_official_account(app, provider) {
        return false;
    }
    let meta = provider.meta.as_ref();
    let managed_oauth = meta
        .and_then(|meta| meta.provider_type.as_deref())
        .is_some_and(|kind| MANAGED_OAUTH_PROVIDER_TYPES.contains(&kind));
    let full_url = meta.and_then(|meta| meta.is_full_url) == Some(true);
    let api_format = meta
        .and_then(|meta| meta.api_format.as_deref())
        .filter(|fmt| !fmt.is_empty());
    match app {
        AppType::ClaudeDesktop => {
            managed_oauth
                || meta
                    .and_then(|meta| meta.claude_desktop_mode.as_ref())
                    .is_some_and(|mode| *mode == crate::provider::ClaudeDesktopMode::Proxy)
        }
        AppType::Claude => {
            managed_oauth || full_url || api_format.is_some_and(|f| f != "anthropic")
        }
        AppType::Codex | AppType::GrokBuild => {
            managed_oauth
                || full_url
                || matches!(api_format, Some("openai_chat" | "anthropic"))
                || provider
                    .settings_config
                    .get("config")
                    .and_then(|config| config.as_str())
                    .and_then(codex_wire_api)
                    .is_some_and(|wire_api| is_chat_or_anthropic_wire_api(&wire_api))
        }
        _ => false,
    }
}

// ─── 快照 ────────────────────────────────────────────────────────────────────

/// 应用子菜单的样子。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrayMode {
    Direct,
    Route,
    /// 路由 + 自动故障转移：子菜单只读（#6022）。
    Failover,
    Stack,
    /// Claude Desktop 没有模式 tab。
    Desktop,
}

#[derive(Debug, Clone, PartialEq)]
struct ProviderEntry {
    id: String,
    name: String,
    /// 同名时补在名字后面的区分词（备注或网址）。
    hint: Option<String>,
    needs_routing: bool,
    official: bool,
    blocked_from_routing: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ProfileSection {
    scope: &'static str,
    /// (id, 名字)
    items: Vec<(String, String)>,
    current: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct AppSnapshot {
    app: AppType,
    mode: TrayMode,
    /// 按供应商页的顺序（sort_index → created_at → name）。
    providers: Vec<ProviderEntry>,
    /// 在用的那家（`provider_for(InUse)`）。
    current_id: Option<String>,
    /// 故障转移队列（按优先级）。
    queue: Vec<String>,
    /// 聚合名单。
    stack_members: Vec<String>,
    quota: Option<QuotaView>,
    /// 路由服务该在跑却没在跑。
    service_down: bool,
    needs_attention: bool,
    profiles: Option<ProfileSection>,
}

impl AppSnapshot {
    fn current(&self) -> Option<&ProviderEntry> {
        let id = self.current_id.as_deref()?;
        self.providers.iter().find(|p| p.id == id)
    }

    /// 这个应用现在靠路由服务：路由 / 聚合中，或 Desktop 在用模型映射卡。
    fn uses_service(&self) -> bool {
        match self.mode {
            TrayMode::Route | TrayMode::Failover | TrayMode::Stack => true,
            TrayMode::Desktop => self.current().is_some_and(|p| p.needs_routing),
            TrayMode::Direct => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum TrayProblem {
    /// 路由服务没在跑，但有应用在用它。
    ServiceDown { port: u16 },
    /// 启动时没能接上路由 / 聚合，已退回直连。
    AttachFailed { app: AppType, stack: bool },
    /// 托盘里的切换没成功。
    SwitchFailed { app: AppType, reason: String },
}

/// 对供应商列表排序：sort_index → created_at → name
fn sort_providers(providers: &indexmap::IndexMap<String, Provider>) -> Vec<(&String, &Provider)> {
    let mut sorted: Vec<_> = providers.iter().collect();
    sorted.sort_by(|(_, a), (_, b)| {
        match (a.sort_index, b.sort_index) {
            (Some(idx_a), Some(idx_b)) => return idx_a.cmp(&idx_b),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            _ => {}
        }

        match (a.created_at, b.created_at) {
            (Some(time_a), Some(time_b)) => return time_a.cmp(&time_b),
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            _ => {}
        }

        a.name.cmp(&b.name)
    });
    sorted
}

fn provider_hint(provider: &Provider) -> Option<String> {
    if let Some(notes) = provider
        .notes
        .as_deref()
        .and_then(|notes| notes.lines().next())
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
    {
        return Some(truncate_chars(notes, 24));
    }
    let url = provider.website_url.as_deref()?.trim();
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()?
        .trim();
    (!host.is_empty()).then(|| host.to_string())
}

fn provider_entry(app: &AppType, provider: &Provider) -> ProviderEntry {
    ProviderEntry {
        id: provider.id.clone(),
        name: provider.name.clone(),
        hint: provider_hint(provider),
        needs_routing: provider_needs_routing(app, provider),
        official: is_official_account(app, provider),
        blocked_from_routing: blocked_from_routing(app, provider),
    }
}

fn tray_mode(app_state: &AppState, app: &AppType) -> TrayMode {
    if *app == AppType::ClaudeDesktop {
        return TrayMode::Desktop;
    }
    if !crate::mode::current::is_proxy(app) {
        return TrayMode::Direct;
    }
    if crate::mode::stack::stack_mode_now(app) {
        return TrayMode::Stack;
    }
    let (_, auto_failover) = app_state.db.get_proxy_flags_sync(app.as_str());
    if auto_failover {
        TrayMode::Failover
    } else {
        TrayMode::Route
    }
}

fn collect_app_snapshot(
    app_state: &AppState,
    texts: &TrayTexts,
    settings: &crate::settings::AppSettings,
    app: &AppType,
    service_running: Option<bool>,
    profiles: &[crate::database::Profile],
) -> Result<AppSnapshot, AppError> {
    let rows = app_state.db.get_all_providers(app.as_str())?;
    let providers: Vec<ProviderEntry> = sort_providers(&rows)
        .into_iter()
        .map(|(_, provider)| provider_entry(app, provider))
        .collect();
    // 代理模式下是代理路由到的那家（含故障转移切过去的）。
    let current_id = crate::mode::current::provider_for(
        &app_state.db,
        app,
        crate::mode::current::Purpose::InUse,
    )?
    .filter(|id| rows.contains_key(id));
    let mode = tray_mode(app_state, app);

    let queue = if mode == TrayMode::Failover {
        app_state
            .db
            .get_failover_queue(app.as_str())?
            .into_iter()
            .map(|item| item.provider_id)
            .filter(|id| rows.contains_key(id))
            .collect()
    } else {
        Vec::new()
    };
    let stack_members = if mode == TrayMode::Stack {
        crate::mode::state::stack(
            &crate::live::engine::DeviceStore::for_device(),
            app.as_str(),
        )?
        .members
    } else {
        Vec::new()
    };

    let quota = current_id.as_deref().and_then(|id| {
        let provider = rows.get(id)?;
        usage_view(&app_state.usage_cache, texts, app, provider, id)
    });

    let profiles = crate::services::profile::ProfileScope::for_app(app)
        .filter(|_| settings.show_profile_switcher && !profiles.is_empty())
        .map(|scope| -> Result<ProfileSection, AppError> {
            Ok(ProfileSection {
                scope: scope.as_str(),
                items: profiles
                    .iter()
                    .map(|profile| (profile.id.clone(), profile.name.clone()))
                    .collect(),
                current: app_state
                    .db
                    .get_current_profile_id(scope.as_str())?
                    .filter(|id| !id.is_empty()),
            })
        })
        .transpose()?;

    let mut snapshot = AppSnapshot {
        app: app.clone(),
        mode,
        providers,
        current_id,
        queue,
        stack_members,
        quota,
        service_down: false,
        needs_attention: false,
        profiles,
    };
    snapshot.service_down = service_running == Some(false) && snapshot.uses_service();
    Ok(snapshot)
}

// ─── 问题区的状态 ─────────────────────────────────────────────────────────────

/// 启动流程（接上路由、拉起 Desktop 映射服务）走完之前不报「路由服务没在跑」：那时它本来就还没起来。
static STARTUP_SETTLED: AtomicBool = AtomicBool::new(false);
/// 启动时没能接上、已退回直连的应用（托盘自己留一份，界面照样取走它的那份）。
static ATTACH_FAILURES: Lazy<Mutex<Vec<(AppType, bool)>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// 托盘里切换失败的应用。规格 5.5：处理掉才消失（同一应用之后切换成功、或打开过它的页面），
/// 不按时间过期。
struct SwitchFailure {
    app: AppType,
    reason: String,
}

static SWITCH_FAILURES: Lazy<Mutex<Vec<SwitchFailure>>> = Lazy::new(|| Mutex::new(Vec::new()));
/// 上一次建菜单时菜单上会自己过时的那部分，悬停图标 / 路由服务启停时比一下，变了才重建。
static LAST_STATUS: Lazy<Mutex<MenuStatus>> = Lazy::new(|| Mutex::new(MenuStatus::default()));

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 启动流程走完：记下启动时退回直连的应用，重建一次菜单。
pub fn mark_startup_settled(app: &tauri::AppHandle) {
    {
        let mut failures = lock(&ATTACH_FAILURES);
        for failure in crate::mode::controller::startup_attach_failures_snapshot() {
            if let Ok(app_type) = failure.app_type.parse::<AppType>() {
                if !failures.iter().any(|(existing, _)| *existing == app_type) {
                    failures.push((app_type, failure.stack));
                }
            }
        }
    }
    STARTUP_SETTLED.store(true, Ordering::Release);
    refresh_tray_menu(app);
}

/// 清掉某个应用的问题行，返回有没有清掉东西。
fn clear_app_problems(app: &AppType) -> bool {
    let mut attach = lock(&ATTACH_FAILURES);
    let mut switch = lock(&SWITCH_FAILURES);
    let before = attach.len() + switch.len();
    attach.retain(|(existing, _)| existing != app);
    switch.retain(|failure| failure.app != *app);
    attach.len() + switch.len() != before
}

/// 主界面打开了某个应用的页面：这个应用的问题行算处理过了（规格 5.5「打开过对应页面」）。
#[tauri::command]
pub fn tray_app_page_seen(app: tauri::AppHandle, app_type: String) {
    let Ok(app_type) = app_type.parse::<AppType>() else {
        return;
    };
    if clear_app_problems(&app_type) {
        schedule_tray_status_check(&app);
    }
}

fn record_switch_failure(app: &AppType, reason: String) {
    let mut failures = lock(&SWITCH_FAILURES);
    failures.retain(|failure| failure.app != *app);
    failures.push(SwitchFailure {
        app: app.clone(),
        reason,
    });
}

fn service_running(app_state: &AppState) -> Option<bool> {
    if !STARTUP_SETTLED.load(Ordering::Acquire) {
        return None;
    }
    app_state.proxy_service.running_now()
}

/// 问题列表。`modes` 是可见应用现在的模式（重新进入路由的应用不再报「退回直连」）。
fn collect_problems(
    service_down_port: Option<u16>,
    visible: &[(AppType, bool)],
) -> Vec<TrayProblem> {
    let mut problems = Vec::new();
    if let Some(port) = service_down_port {
        problems.push(TrayProblem::ServiceDown { port });
    }
    {
        let mut failures = lock(&ATTACH_FAILURES);
        // 已经重新进入路由 / 聚合的应用不再报。
        failures.retain(|(app, _)| {
            visible
                .iter()
                .find(|(visible_app, _)| visible_app == app)
                .is_none_or(|(_, direct)| *direct)
        });
        for (app, stack) in failures.iter() {
            if visible.iter().any(|(visible_app, _)| visible_app == app) {
                problems.push(TrayProblem::AttachFailed {
                    app: app.clone(),
                    stack: *stack,
                });
            }
        }
    }
    {
        let failures = lock(&SWITCH_FAILURES);
        for failure in failures.iter() {
            if visible.iter().any(|(app, _)| *app == failure.app) {
                problems.push(TrayProblem::SwitchFailed {
                    app: failure.app.clone(),
                    reason: failure.reason.clone(),
                });
            }
        }
    }
    problems
}

/// 受影响的应用行尾写「需要处理」（这时不写额度，保持短）。
fn mark_attention(snapshots: &mut [AppSnapshot], problems: &[TrayProblem]) {
    for snapshot in snapshots.iter_mut() {
        snapshot.needs_attention = snapshot.service_down
            || problems.iter().any(|problem| {
                matches!(problem, TrayProblem::AttachFailed { app, .. } if *app == snapshot.app)
            });
    }
}

// ─── 反馈行 ───────────────────────────────────────────────────────────────────

/// 反馈行显示多久（Linux 没有点击事件，只按时间）。
const FEEDBACK_TTL: std::time::Duration = std::time::Duration::from_secs(2 * 60);

/// 托盘里做完一个要重启才生效（或应用了项目）的操作后，下次打开菜单时最上面那行灰字。
/// 存结构不存文字：语言改了照样按新语言写。
#[derive(Debug, Clone, PartialEq)]
enum TrayFeedback {
    /// Codex / Gemini CLI / Grok Build 直连切换，或 Claude Desktop 切换。
    Switched { app: AppType, name: String },
    /// 聚合换默认：发布给客户端的模型列表跟着变。
    StackDefault { app: AppType, name: String },
    /// 应用项目：同时改了供应商、MCP、Skills、提示词。
    ProfileApplied { app: AppType, name: String },
}

struct FeedbackRecord {
    feedback: TrayFeedback,
    at: std::time::Instant,
    /// 已经在打开的菜单里显示过一次（点过托盘图标）。
    seen: bool,
}

static FEEDBACK: Lazy<Mutex<Option<FeedbackRecord>>> = Lazy::new(|| Mutex::new(None));

/// 切换成功后要不要说一句：客户端只在启动时读配置的才说；Claude Code 直连、路由 / 故障转移
/// 换一家立即生效，不说。
fn switch_feedback(app: &AppType, mode: TrayMode, name: String) -> Option<TrayFeedback> {
    match mode {
        TrayMode::Desktop => Some(TrayFeedback::Switched {
            app: app.clone(),
            name,
        }),
        TrayMode::Direct
            if matches!(app, AppType::Codex | AppType::Gemini | AppType::GrokBuild) =>
        {
            Some(TrayFeedback::Switched {
                app: app.clone(),
                name,
            })
        }
        TrayMode::Stack => Some(TrayFeedback::StackDefault {
            app: app.clone(),
            name,
        }),
        _ => None,
    }
}

/// 记下最近一次托盘操作的结果（覆盖上一条）。调用方随后会重建菜单。
fn record_feedback(app: &tauri::AppHandle, feedback: TrayFeedback) {
    *lock(&FEEDBACK) = Some(FeedbackRecord {
        feedback,
        at: std::time::Instant::now(),
        seen: false,
    });
    // Linux 没有悬停 / 点击事件，到点了自己重建一次把它拿掉。其它平台在悬停图标时就会重建，
    // 不在这里定时重建，免得把正开着的菜单关掉。
    #[cfg(target_os = "linux")]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FEEDBACK_TTL + std::time::Duration::from_secs(1));
            refresh_tray_if_status_changed(&app);
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

/// 现在该显示的反馈：没过期、也还没在打开的菜单里显示过。
fn current_feedback() -> Option<TrayFeedback> {
    let mut record = lock(&FEEDBACK);
    if record
        .as_ref()
        .is_some_and(|r| r.seen || r.at.elapsed() >= FEEDBACK_TTL)
    {
        *record = None;
    }
    record.as_ref().map(|r| r.feedback.clone())
}

/// 点了托盘图标：这次弹出的菜单里已经有反馈行了，算显示过；下次重建（悬停图标时）就拿掉。
/// 只算会弹出菜单的点击：Windows 左键是打开主界面，不算。
pub fn note_tray_click(button: tauri::tray::MouseButton) {
    let opens_menu = match button {
        tauri::tray::MouseButton::Right => true,
        tauri::tray::MouseButton::Left => !cfg!(target_os = "windows"),
        _ => false,
    };
    if opens_menu {
        if let Some(record) = lock(&FEEDBACK).as_mut() {
            record.seen = true;
        }
    }
}

fn feedback_text(texts: &TrayTexts, feedback: &TrayFeedback) -> String {
    let (template, app, name) = match feedback {
        TrayFeedback::Switched { app, name } => (texts.feedback_switched, app, name),
        TrayFeedback::StackDefault { app, name } => (texts.feedback_stack_default, app, name),
        TrayFeedback::ProfileApplied { app, name } => (texts.feedback_profile, app, name),
    };
    fill(
        template,
        &[
            ("app", app_display_name(app)),
            ("name", &truncate_chars(name, MAX_NAME_CHARS)),
        ],
    )
}

/// 菜单上会自己过时的那部分：问题区、反馈行、「退出」写不写后果。
#[derive(Debug, Clone, Default, PartialEq)]
struct MenuStatus {
    problems: Vec<TrayProblem>,
    feedback: Option<TrayFeedback>,
    routing_in_use: bool,
}

// ─── 菜单模型（纯数据）────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum TrayEntry {
    Item {
        id: String,
        text: String,
        enabled: bool,
    },
    Check {
        id: String,
        text: String,
        enabled: bool,
        checked: bool,
    },
    Submenu {
        id: String,
        text: String,
        /// 应用行：额度更新时按它就地改标题。
        app: Option<AppType>,
        children: Vec<TrayEntry>,
    },
    Separator,
}

impl TrayEntry {
    fn item(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            text: text.into(),
            enabled: true,
        }
    }

    fn label(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            text: text.into(),
            enabled: false,
        }
    }

    fn check(id: impl Into<String>, text: impl Into<String>, enabled: bool, checked: bool) -> Self {
        Self::Check {
            id: id.into(),
            text: text.into(),
            enabled,
            checked,
        }
    }
}

fn provider_id_for(app: &AppType, provider_id: &str) -> String {
    format!("prov:{}:{provider_id}", app.as_str())
}

fn problem_text(texts: &TrayTexts, problem: &TrayProblem) -> String {
    match problem {
        TrayProblem::ServiceDown { port } => {
            fill(texts.problem_service_down, &[("port", &port.to_string())])
        }
        TrayProblem::AttachFailed { app, stack } => fill(
            texts.problem_attach_failed,
            &[
                ("app", app_display_name(app)),
                (
                    "mode",
                    if *stack {
                        texts.mode_stack
                    } else {
                        texts.mode_route
                    },
                ),
            ],
        ),
        TrayProblem::SwitchFailed { app, reason } => fill(
            texts.problem_switch_failed,
            &[
                ("app", app_display_name(app)),
                ("reason", &truncate_chars(reason, MAX_REASON_CHARS)),
            ],
        ),
    }
}

fn problem_id(problem: &TrayProblem) -> String {
    match problem {
        TrayProblem::ServiceDown { .. } => "nav:settings:routing".to_string(),
        TrayProblem::AttachFailed { app, .. } => format!("nav:app:{}:attach", app.as_str()),
        TrayProblem::SwitchFailed { app, .. } => format!("nav:app:{}:failed", app.as_str()),
    }
}

/// 应用行标题：`<应用全称> · [模式词 · ]<在用供应商>[ · <额度>][ · 快用完][ · 需要处理]`。
fn app_row_title(texts: &TrayTexts, snapshot: &AppSnapshot) -> String {
    let mut parts: Vec<String> = vec![app_display_name(&snapshot.app).to_string()];
    let current = snapshot.current();
    let mode_word = match snapshot.mode {
        TrayMode::Route | TrayMode::Failover => Some(texts.mode_route),
        TrayMode::Stack => Some(texts.mode_stack),
        TrayMode::Desktop if current.is_some_and(|p| p.needs_routing) => Some(texts.mode_mapping),
        _ => None,
    };
    if let Some(word) = mode_word {
        parts.push(word.to_string());
    }
    if let Some(provider) = current {
        parts.push(truncate_chars(&provider.name, MAX_NAME_CHARS));
    }
    if snapshot.needs_attention {
        parts.push(texts.needs_attention.to_string());
    } else if let Some((quota, almost_out)) = snapshot.quota.as_ref().and_then(quota_title) {
        if !quota.is_empty() {
            parts.push(quota);
        }
        if almost_out {
            parts.push(texts.almost_out.to_string());
        }
    }
    parts.join(" · ")
}

/// 列出来的几家的显示名：截断，同名的补备注 / 网址，再不行补序号。
fn display_names(providers: &[&ProviderEntry]) -> Vec<String> {
    providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            let name = truncate_chars(&provider.name, MAX_NAME_CHARS);
            let same: Vec<usize> = providers
                .iter()
                .enumerate()
                .filter(|(_, other)| other.name.trim() == provider.name.trim())
                .map(|(i, _)| i)
                .collect();
            if same.len() < 2 {
                return name;
            }
            match provider.hint.as_deref() {
                Some(hint) => format!("{name} · {hint}"),
                None => {
                    let ordinal = same.iter().position(|i| *i == index).unwrap_or(0) + 1;
                    format!("{name} #{ordinal}")
                }
            }
        })
        .collect()
}

fn provider_rows(texts: &TrayTexts, snapshot: &AppSnapshot) -> Vec<TrayEntry> {
    let app = &snapshot.app;
    let current = snapshot.current_id.as_deref();
    let is_current = |p: &ProviderEntry| current == Some(p.id.as_str());
    match snapshot.mode {
        TrayMode::Direct => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    if p.needs_routing {
                        // 直连下不能直接切：打开应用页，弹和主界面同一个「需要路由」对话框。
                        TrayEntry::item(
                            format!("nav:needs:{}:{}", app.as_str(), p.id),
                            format!("{name}{}", texts.needs_routing_suffix),
                        )
                    } else {
                        TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                    }
                })
                .collect()
        }
        TrayMode::Route => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    if p.blocked_from_routing {
                        TrayEntry::label(
                            format!("info:{}:blocked:{}", app.as_str(), p.id),
                            format!("{name}{}", texts.official_blocked_suffix),
                        )
                    } else {
                        TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                    }
                })
                .collect()
        }
        TrayMode::Failover => {
            // 只读：只列队列，勾 = 现在路由到的那家；托盘不再悄悄关掉故障转移（#6022）。
            let listed: Vec<&ProviderEntry> = snapshot
                .queue
                .iter()
                .filter_map(|id| snapshot.providers.iter().find(|p| p.id == *id))
                .collect();
            let names = display_names(&listed);
            let mut rows = vec![TrayEntry::label(
                format!("info:{}:failover", app.as_str()),
                texts.failover_note,
            )];
            rows.extend(
                listed
                    .iter()
                    .zip(names)
                    .enumerate()
                    .map(|(index, (p, name))| {
                        TrayEntry::check(
                            provider_id_for(app, &p.id),
                            format!("{}. {name}", index + 1),
                            false,
                            is_current(p),
                        )
                    }),
            );
            rows
        }
        TrayMode::Stack => {
            // 只列名单里的成员和能做默认的官方卡（Codex 官方卡不能加入聚合，但能做默认）；点一家 = 设为默认。
            let listed: Vec<&ProviderEntry> = snapshot
                .providers
                .iter()
                .filter(|p| {
                    is_current(p)
                        || (!p.blocked_from_routing
                            && (snapshot.stack_members.contains(&p.id) || p.official))
                })
                .collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    TrayEntry::check(provider_id_for(app, &p.id), name, true, is_current(p))
                })
                .collect()
        }
        TrayMode::Desktop => {
            let listed: Vec<&ProviderEntry> = snapshot.providers.iter().collect();
            let names = display_names(&listed);
            listed
                .iter()
                .zip(names)
                .map(|(p, name)| {
                    let text = if p.needs_routing {
                        format!("{name}{}", texts.mapping_suffix)
                    } else {
                        name
                    };
                    TrayEntry::check(provider_id_for(app, &p.id), text, true, is_current(p))
                })
                .collect()
        }
    }
}

fn app_children(
    texts: &TrayTexts,
    snapshot: &AppSnapshot,
    now: chrono::DateTime<chrono::Local>,
) -> Vec<TrayEntry> {
    let app = snapshot.app.as_str();
    let mut children = Vec::new();

    let header = match snapshot.mode {
        TrayMode::Direct => Some(texts.header_direct.to_string()),
        TrayMode::Route => Some(texts.header_route.to_string()),
        TrayMode::Failover => Some(texts.header_failover.to_string()),
        // 聚合：标题写出默认那家（勾着的就是它）；还没有默认时退回泛称。
        TrayMode::Stack => Some(match snapshot.current() {
            Some(current) => fill(
                texts.header_stack_default,
                &[("name", &truncate_chars(&current.name, MAX_NAME_CHARS))],
            ),
            None => texts.header_stack.to_string(),
        }),
        TrayMode::Desktop => None,
    };
    if let Some(header) = header {
        children.push(TrayEntry::label(format!("info:{app}:header"), header));
    }
    if snapshot.mode == TrayMode::Desktop && snapshot.service_down {
        if let Some(current) = snapshot.current() {
            children.push(TrayEntry::item(
                format!("nav:app:{app}:service"),
                fill(
                    texts.desktop_unavailable,
                    &[("name", &truncate_chars(&current.name, MAX_NAME_CHARS))],
                ),
            ));
            children.push(TrayEntry::Separator);
        }
    }

    children.extend(provider_rows(texts, snapshot));

    // 额度说明行：可点（打开应用页），文字才是正常对比度。
    if let Some(note) = snapshot
        .quota
        .as_ref()
        .and_then(|quota| quota_note(texts, quota, now))
    {
        children.push(TrayEntry::item(format!("nav:app:{app}:quota"), note));
    }

    if let Some(profiles) = &snapshot.profiles {
        children.push(TrayEntry::Separator);
        children.push(TrayEntry::label(
            format!("info:{app}:projects"),
            texts.projects_label,
        ));
        children.push(TrayEntry::check(
            format!("profile_none_{}", profiles.scope),
            texts.no_project_label,
            true,
            profiles.current.is_none(),
        ));
        for (id, name) in &profiles.items {
            children.push(TrayEntry::check(
                format!("profile_{}_{id}", profiles.scope),
                truncate_chars(name, MAX_NAME_CHARS),
                true,
                profiles.current.as_deref() == Some(id.as_str()),
            ));
        }
    }

    children.push(TrayEntry::Separator);
    children.push(TrayEntry::item(
        format!("nav:app:{app}"),
        fill(
            texts.open_app_page,
            &[("app", app_display_name(&snapshot.app))],
        ),
    ));
    children
}

fn app_entry(
    texts: &TrayTexts,
    snapshot: &AppSnapshot,
    now: chrono::DateTime<chrono::Local>,
) -> TrayEntry {
    let app = snapshot.app.as_str();
    if snapshot.providers.is_empty() {
        return TrayEntry::item(
            format!("nav:add:{app}"),
            format!(
                "{} · {}",
                app_display_name(&snapshot.app),
                texts.add_provider
            ),
        );
    }
    TrayEntry::Submenu {
        id: format!("submenu_{app}"),
        text: app_row_title(texts, snapshot),
        app: Some(snapshot.app.clone()),
        children: app_children(texts, snapshot, now),
    }
}

fn build_menu_model(
    texts: &TrayTexts,
    status: &MenuStatus,
    apps: &[AppSnapshot],
    lightweight: bool,
    now: chrono::DateTime<chrono::Local>,
) -> Vec<TrayEntry> {
    let mut menu = Vec::new();
    let problems = &status.problems;

    if !problems.is_empty() {
        for problem in problems.iter().take(MAX_PROBLEM_ROWS) {
            menu.push(TrayEntry::item(
                problem_id(problem),
                problem_text(texts, problem),
            ));
        }
        if problems.len() > MAX_PROBLEM_ROWS {
            let count = (problems.len() - MAX_PROBLEM_ROWS).to_string();
            menu.push(TrayEntry::item(
                "nav:main",
                fill(texts.problem_more, &[("count", &count)]),
            ));
        }
    }
    // 反馈行：灰字、不可点，只说一句结果（文案同主界面 toast）。
    if let Some(feedback) = &status.feedback {
        menu.push(TrayEntry::label(
            "info:feedback",
            feedback_text(texts, feedback),
        ));
    }
    if !problems.is_empty() || status.feedback.is_some() {
        menu.push(TrayEntry::Separator);
    }

    // 窗口找不回来时托盘是唯一入口，「打开 CC Switch」一直放在最上面。
    menu.push(TrayEntry::item("show_main", texts.show_main));
    menu.push(TrayEntry::Separator);

    if !apps.is_empty() {
        menu.extend(apps.iter().map(|snapshot| app_entry(texts, snapshot, now)));
        menu.push(TrayEntry::Separator);
    }

    menu.push(TrayEntry::check(
        "lightweight_mode",
        texts.lightweight_mode,
        true,
        lightweight,
    ));
    menu.push(TrayEntry::Separator);
    menu.push(TrayEntry::item("open_website", texts.open_website));
    // 不换成系统自带的 quit：它不经过 app.exit(0) 的退出流程，客户端指回直连、停服务只能在
    // RunEvent::Exit 里限时补做（见 lib.rs 的 cleanup_before_system_exit）。
    // 有应用在用路由服务时把这个后果写在「退出」上（后果不进说明，写在按钮文字里）。
    menu.push(TrayEntry::item(
        "quit",
        if status.routing_in_use {
            texts.quit_stops_routing
        } else {
            texts.quit
        },
    ));
    menu
}

// ─── 读状态 → 模型 ─────────────────────────────────────────────────────────────

struct TrayModel {
    entries: Vec<TrayEntry>,
    status: MenuStatus,
}

fn collect_snapshots(
    app_state: &AppState,
    texts: &TrayTexts,
) -> Result<(Vec<AppSnapshot>, Vec<TrayProblem>), AppError> {
    let settings = crate::settings::get_settings();
    let visible_apps = settings.visible_apps.clone().unwrap_or_default();
    let running = service_running(app_state);
    let profiles = if settings.show_profile_switcher {
        app_state.db.get_all_profiles()?
    } else {
        Vec::new()
    };

    let mut snapshots = Vec::new();
    for app in TRAY_APPS.iter().filter(|app| visible_apps.is_visible(app)) {
        snapshots.push(collect_app_snapshot(
            app_state, texts, &settings, app, running, &profiles,
        )?);
    }

    let port = snapshots
        .iter()
        .any(|snapshot| snapshot.service_down)
        .then(|| app_state.db.get_proxy_listen_sync().1);
    let visible: Vec<(AppType, bool)> = snapshots
        .iter()
        .map(|snapshot| (snapshot.app.clone(), snapshot.mode == TrayMode::Direct))
        .collect();
    let problems = collect_problems(port, &visible);
    mark_attention(&mut snapshots, &problems);
    Ok((snapshots, problems))
}

fn collect_status(
    app_state: &AppState,
    texts: &TrayTexts,
) -> Result<(Vec<AppSnapshot>, MenuStatus), AppError> {
    let (snapshots, problems) = collect_snapshots(app_state, texts)?;
    let status = MenuStatus {
        problems,
        feedback: current_feedback(),
        routing_in_use: routing_in_use(app_state),
    };
    Ok((snapshots, status))
}

fn collect_model(app_state: &AppState, texts: &TrayTexts) -> Result<TrayModel, AppError> {
    let (snapshots, status) = collect_status(app_state, texts)?;
    let entries = build_menu_model(
        texts,
        &status,
        &snapshots,
        crate::lightweight::is_lightweight_mode(),
        chrono::Local::now(),
    );
    Ok(TrayModel { entries, status })
}

/// 退出会让它们断开的情况：路由服务在跑，且有应用在路由 / 聚合模式，或 Claude Desktop 在用
/// 模型映射卡（和 `controller::stop_server_if_unused` 的「在用」同一口径，隐藏的应用也算）。
fn routing_in_use(app_state: &AppState) -> bool {
    app_state.proxy_service.running_now() == Some(true)
        && (crate::mode::current::proxy_flags(crate::mode::controller::PROXY_APPS).contains(&true)
            || crate::claude_desktop_config::current_provider_uses_proxy(&app_state.db))
}

// ─── 模型 → Tauri 菜单 ─────────────────────────────────────────────────────────

fn menu_error(e: impl std::fmt::Display) -> AppError {
    AppError::Message(format!("创建托盘菜单失败: {e}"))
}

fn attach_entry(
    app: &tauri::AppHandle,
    entry: &TrayEntry,
    handles: &mut HashMap<AppType, Submenu<tauri::Wry>>,
) -> Result<Option<MenuItemKind<tauri::Wry>>, AppError> {
    Ok(Some(match entry {
        TrayEntry::Separator => return Ok(None),
        TrayEntry::Item { id, text, enabled } => MenuItemKind::MenuItem(
            MenuItem::with_id(app, id.as_str(), text, *enabled, None::<&str>)
                .map_err(menu_error)?,
        ),
        TrayEntry::Check {
            id,
            text,
            enabled,
            checked,
        } => MenuItemKind::Check(
            CheckMenuItem::with_id(app, id.as_str(), text, *enabled, *checked, None::<&str>)
                .map_err(menu_error)?,
        ),
        TrayEntry::Submenu {
            id,
            text,
            app: app_type,
            children,
        } => {
            let mut builder = SubmenuBuilder::with_id(app, id.as_str(), text);
            for child in children {
                builder = match attach_entry(app, child, handles)? {
                    Some(item) => builder.item(&item),
                    None => builder.separator(),
                };
            }
            let submenu = builder.build().map_err(menu_error)?;
            if let Some(app_type) = app_type {
                handles.insert(app_type.clone(), submenu.clone());
            }
            MenuItemKind::Submenu(submenu)
        }
    }))
}

/// 创建动态托盘菜单
pub fn create_tray_menu(
    app: &tauri::AppHandle,
    app_state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    let texts = TrayTexts::current();
    let model = collect_model(app_state, &texts)?;

    let mut handles = HashMap::new();
    let mut builder = MenuBuilder::new(app);
    for entry in &model.entries {
        builder = match attach_entry(app, entry, &mut handles)? {
            Some(item) => builder.item(&item),
            None => builder.separator(),
        };
    }
    let menu = builder.build().map_err(menu_error)?;

    *lock(&TRAY_SECTION_SUBMENUS) = handles;
    update_problem_indicator(app, &texts, &model.status.problems);
    *lock(&LAST_STATUS) = model.status;
    Ok(menu)
}

// ─── 图标圆点和悬停提示 ───────────────────────────────────────────────────────

/// 悬停提示最多几个字（Windows 的提示缓冲区是 128 个 UTF-16 单元，超了会被截掉结尾的 0）。
const MAX_TOOLTIP_CHARS: usize = 100;
/// 圆点颜色（彩色图标用，同画板 `--sys-dot`）。
const PROBLEM_DOT_RGB: [u8; 3] = [0xF7, 0x63, 0x0C];
/// 圆点半径、连同外圈透明边的半径（占图标边长的比例，照画板 24px 图标上 3.3 / 5.1 的圆）。
const DOT_RADIUS: f32 = 0.15;
const DOT_RING_RADIUS: f32 = 0.21;

/// 托盘平时的图标和它是不是 macOS 模板图：macOS 用单色模板图（跟随菜单栏深浅色），读不到时
/// 和其它平台一样用应用图标。读一次缓存起来，出问题 / 恢复时在它上面加减圆点。
pub fn base_tray_icon(app: &tauri::AppHandle) -> Option<(Image<'static>, bool)> {
    static BASE: OnceLock<Option<(Image<'static>, bool)>> = OnceLock::new();
    BASE.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            const ICON_BYTES: &[u8] =
                include_bytes!("../icons/tray/macos/statusbar_template_3x.png");
            match Image::from_bytes(ICON_BYTES) {
                Ok(icon) => return Some((icon, true)),
                Err(err) => {
                    log::warn!("Failed to load macOS tray icon: {err}");
                    log::warn!("Falling back to default window icon for tray");
                }
            }
        }
        match app.default_window_icon() {
            Some(icon) => Some((icon.clone().to_owned(), false)),
            None => {
                log::warn!("Failed to get default window icon for tray");
                None
            }
        }
    })
    .clone()
}

/// 出问题时的图标：右下角一个实心圆点，圆点外挖一圈透明边和图形隔开。模板图只看不透明度，
/// 圆点画成黑色、由系统按菜单栏深浅色着色（单色，靠形状区分）；彩色图画橙点。
fn icon_with_problem_dot(icon: &Image<'_>, template: bool) -> Image<'static> {
    let (width, height) = (icon.width(), icon.height());
    let mut rgba = icon.rgba().to_vec();
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Image::new_owned(rgba, width, height);
    }
    let size = width.min(height) as f32;
    let dot_radius = size * DOT_RADIUS;
    let ring_radius = size * DOT_RING_RADIUS;
    let (cx, cy) = (width as f32 - ring_radius, height as f32 - ring_radius);
    let color = if template { [0, 0, 0] } else { PROBLEM_DOT_RGB };
    let x0 = (cx - ring_radius - 1.0).max(0.0) as u32;
    let y0 = (cy - ring_radius - 1.0).max(0.0) as u32;
    for y in y0..height {
        for x in x0..width {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let distance = (dx * dx + dy * dy).sqrt();
            // 边缘各留半个像素做抗锯齿。
            let cut = (ring_radius + 0.5 - distance).clamp(0.0, 1.0);
            if cut <= 0.0 {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            let fill = (dot_radius + 0.5 - distance).clamp(0.0, 1.0);
            if fill > 0.0 {
                // 圆点（含边缘）整个落在透明圈里，底下原本的像素已经挖掉了。
                rgba[i..i + 3].copy_from_slice(&color);
                rgba[i + 3] = (255.0 * fill).round() as u8;
            } else {
                rgba[i + 3] = (f32::from(rgba[i + 3]) * (1.0 - cut)).round() as u8;
            }
        }
    }
    Image::new_owned(rgba, width, height)
}

/// 悬停提示：平时 `CC Switch`，出问题时 `CC Switch · 需要处理：<问题区第一条>`。额度不算问题。
fn tray_tooltip(texts: &TrayTexts, problems: &[TrayProblem]) -> String {
    match problems.first() {
        None => "CC Switch".to_string(),
        Some(problem) => truncate_chars(
            &fill(
                texts.tooltip_problem,
                &[("problem", &problem_text(texts, problem))],
            ),
            MAX_TOOLTIP_CHARS,
        ),
    }
}

/// 现在托盘上的（有没有圆点，悬停提示）；`None` = 托盘图标还没建好，还是建图标时的样子。
static APPLIED_INDICATOR: Lazy<Mutex<Option<(bool, String)>>> = Lazy::new(|| Mutex::new(None));

/// 问题区有内容时托盘图标加圆点、悬停提示写第一条问题；问题都消失后换回原图标和提示。只在
/// 变了时才动（Linux 每次换图标都要写临时文件）。
fn update_problem_indicator(app: &tauri::AppHandle, texts: &TrayTexts, problems: &[TrayProblem]) {
    // 启动时第一次建菜单那会儿托盘图标还没建，等下一次重建。
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let has_dot = !problems.is_empty();
    let tooltip = tray_tooltip(texts, problems);
    // 只在锁里比对和记账：换图标要回主线程执行，不能拿着锁等。
    let icon_changed = {
        let mut applied = lock(&APPLIED_INDICATOR);
        if applied.as_ref() == Some(&(has_dot, tooltip.clone())) {
            return;
        }
        let changed = applied.as_ref().map_or(has_dot, |(dot, _)| *dot != has_dot);
        *applied = Some((has_dot, tooltip.clone()));
        changed
    };
    if icon_changed {
        if let Some((base, template)) = base_tray_icon(app) {
            let icon = if has_dot {
                icon_with_problem_dot(&base, template)
            } else {
                base
            };
            if let Err(e) = tray.set_icon(Some(icon)) {
                log::warn!("[Tray] 更新托盘图标失败: {e}");
            }
            // macOS 上 set_icon 会把模板标记清掉，要再设一次，否则深色菜单栏里是黑图标。
            if template {
                if let Err(e) = tray.set_icon_as_template(true) {
                    log::warn!("[Tray] 恢复模板图标标记失败: {e}");
                }
            }
        }
    }
    // Linux（AppIndicator）不显示悬停提示，设了也无害。
    if let Err(e) = tray.set_tooltip(Some(&tooltip)) {
        log::warn!("[Tray] 更新托盘悬停提示失败: {e}");
    }
}

/// 就地更新各应用行的标题（额度变化时走这条），避免 `set_menu` 关掉用户正开着的菜单。
/// 句柄由上一次 `create_tray_menu` 填充；为空（从未构建过菜单）时无事发生。
fn update_tray_usage_labels(app: &tauri::AppHandle) {
    // Linux（AppIndicator）上 `Submenu::set_text` 不稳，标题会变空（#3385）：一律整菜单重建。
    #[cfg(target_os = "linux")]
    {
        refresh_tray_menu(app);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let Some(app_state) = app.try_state::<AppState>() else {
            return;
        };
        let texts = TrayTexts::current();
        let Ok((snapshots, _)) = collect_snapshots(app_state.inner(), &texts) else {
            return;
        };
        // 先拷出句柄再改标题：`set_text` 要回主线程执行，不能拿着锁等。
        let handles = lock(&TRAY_SECTION_SUBMENUS).clone();
        for snapshot in &snapshots {
            let Some(submenu) = handles.get(&snapshot.app) else {
                continue;
            };
            if let Err(e) = submenu.set_text(app_row_title(&texts, snapshot)) {
                log::debug!(
                    "[Tray] 更新{}子菜单标题失败: {e}",
                    app_display_name(&snapshot.app)
                );
            }
        }
    }
}

pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        match create_tray_menu(app, state.inner()) {
            Ok(new_menu) => {
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    if let Err(e) = tray.set_menu(Some(new_menu)) {
                        log::error!("刷新托盘菜单失败: {e}");
                    }
                }
            }
            Err(e) => log::error!("创建托盘菜单失败: {e}"),
        }
    }
}

/// 悬停到托盘图标时：问题区该出现 / 该消失了（路由服务停了、又起来了）、反馈行显示过了或到点了，
/// 就重建菜单。只在变了时重建，菜单还没打开，不会被关掉。
pub fn refresh_tray_if_problems_changed(app: &tauri::AppHandle) {
    static LAST_CHECK: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    {
        let mut last = lock(&LAST_CHECK);
        if last.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    schedule_tray_status_check(app);
}

/// 路由服务启动 / 停止之后：圆点、问题区、「退出」的后果可能要跟着变。在后台线程比对，
/// 变了才重建（调用方可能在异步任务里，也可能正等着主线程）。
pub fn schedule_tray_status_check(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || refresh_tray_if_status_changed(&app));
}

fn refresh_tray_if_status_changed(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let texts = TrayTexts::current();
    let Ok((_, status)) = collect_status(state.inner(), &texts) else {
        return;
    };
    if *lock(&LAST_STATUS) != status {
        refresh_tray_menu(app);
    }
}

// ─── 点击 ────────────────────────────────────────────────────────────────────

/// 托盘让主界面去的地方（`tray-navigate` 事件 + `take_tray_navigation` 命令）。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayNavigation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// 设置分组（`routing`）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// `needsRoute`：弹「需要路由」对话框；`add`：开始添加供应商
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
}

impl TrayNavigation {
    fn is_empty(&self) -> bool {
        self.app.is_none() && self.section.is_none()
    }
}

/// 等主界面来取的导航：轻量模式下窗口是新建的，事件发出去时前端可能还没开始监听。
static PENDING_NAVIGATION: Lazy<Mutex<Option<TrayNavigation>>> = Lazy::new(|| Mutex::new(None));

/// 解析 `nav:…` 菜单 id：`nav:main`、`nav:settings:<分组>`、`nav:app:<应用>[:…]`、
/// `nav:add:<应用>`、`nav:needs:<应用>:<供应商 id>`。
fn parse_nav_id(id: &str) -> Option<TrayNavigation> {
    let rest = id.strip_prefix("nav:")?;
    let mut parts = rest.splitn(3, ':');
    let kind = parts.next()?;
    let target = parts.next();
    let tail = parts.next();
    let app = |value: &str| {
        value
            .parse::<AppType>()
            .ok()
            .map(|app| app.as_str().to_string())
    };
    Some(match kind {
        "main" => TrayNavigation::default(),
        "settings" => TrayNavigation {
            section: Some(target?.to_string()),
            ..Default::default()
        },
        "app" => TrayNavigation {
            app: Some(app(target?)?),
            ..Default::default()
        },
        "add" => TrayNavigation {
            app: Some(app(target?)?),
            intent: Some("add".to_string()),
            ..Default::default()
        },
        "needs" => TrayNavigation {
            app: Some(app(target?)?),
            intent: Some("needsRoute".to_string()),
            provider_id: Some(tail.filter(|id| !id.is_empty())?.to_string()),
            ..Default::default()
        },
        _ => return None,
    })
}

/// 主界面取走托盘留下的导航（取一次就清空）。
#[tauri::command]
pub fn take_tray_navigation() -> Option<TrayNavigation> {
    lock(&PENDING_NAVIGATION).take()
}

fn navigate(app: &tauri::AppHandle, navigation: TrayNavigation) {
    if let Some(app_type) = navigation
        .app
        .as_deref()
        .and_then(|value| value.parse::<AppType>().ok())
    {
        // 打开过对应页面，这个应用的问题行就算看过了。
        clear_app_problems(&app_type);
    }
    let pending = !navigation.is_empty();
    if pending {
        *lock(&PENDING_NAVIGATION) = Some(navigation);
    }
    show_main_window(app);
    if pending {
        if let Err(e) = app.emit("tray-navigate", ()) {
            log::error!("发射 tray-navigate 事件失败: {e}");
        }
    }
    refresh_tray_menu(app);
}

/// 显示并聚焦主窗口；轻量模式下重建窗口。
pub fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "windows")]
        {
            let _ = window.set_skip_taskbar(false);
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "linux")]
        {
            crate::linux_fix::nudge_main_window(window.clone());
        }
        #[cfg(target_os = "macos")]
        {
            apply_tray_policy(app, true);
        }
    } else if crate::lightweight::is_lightweight_mode() {
        if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
            log::error!("退出轻量模式重建窗口失败: {e}");
        }
    }
}

/// 处理项目 Profile 托盘事件，返回是否已处理
///
/// 事件 id 形如 `profile_<scope>_<uuid>`（同一项目在各应用子菜单里各有一项，
/// 应用时只作用于该分组）；`profile_none_<scope>` 表示某分组"不使用项目"
/// （只清该分组标记，不动配置）。
pub fn handle_profile_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    let Some(suffix) = event_id.strip_prefix("profile_") else {
        return false;
    };

    if let Some(scope_str) = suffix.strip_prefix("none_") {
        let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
            log::error!("未知的项目分组托盘事件: {event_id}");
            return true;
        };
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Err(e) = app_state.db.set_current_profile_id(scope.as_str(), None) {
                log::error!("清除当前项目失败: {e}");
            }
        }
        // 通知主窗口刷新（profileId=null 表示该分组已清除当前项目）
        if let Err(e) = app.emit(
            "profile-applied",
            serde_json::json!({ "profileId": null, "scope": scope.as_str() }),
        ) {
            log::error!("发射 profile-applied 事件失败: {e}");
        }
        refresh_tray_menu(app);
        return true;
    }

    // scope 是固定枚举字符串（不含下划线），uuid 只含连字符，首个下划线即分界
    let Some((scope_str, profile_id)) = suffix.split_once('_') else {
        log::error!("无法解析项目托盘事件: {event_id}");
        return true;
    };
    let Ok(scope) = crate::services::profile::ProfileScope::parse(scope_str) else {
        log::error!("未知的项目分组托盘事件: {event_id}");
        return true;
    };

    log::info!("应用项目: {profile_id}（{scope_str} 组）");
    let app_handle = app.clone();
    let profile_id = profile_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(app_state) = app_handle.try_state::<AppState>() else {
            return;
        };
        let desktop_was_mapping = crate::commands::desktop_uses_mapping(app_state.inner(), scope);
        match crate::services::profile::ProfileService::apply(app_state.inner(), &profile_id, scope)
        {
            Ok(warnings) => {
                for warning in &warnings {
                    log::warn!("[Profile] 应用项目 {profile_id} 警告: {warning}");
                }
                for app_type in scope.apps() {
                    clear_app_problems(app_type);
                }
                // 应用项目会同时改供应商、MCP、Skills、提示词，值得说一声。
                let profile_name = app_state
                    .db
                    .get_profile(&profile_id)
                    .ok()
                    .flatten()
                    .map(|profile| profile.name);
                if let (Some(name), Some(app_type)) = (profile_name, scope.apps().first()) {
                    record_feedback(
                        &app_handle,
                        TrayFeedback::ProfileApplied {
                            app: app_type.clone(),
                            name,
                        },
                    );
                }
                crate::commands::emit_profile_apply_events(
                    &app_handle,
                    app_state.inner(),
                    &profile_id,
                    scope,
                    desktop_was_mapping,
                );
            }
            Err(e) => {
                log::error!("应用项目 {profile_id} 失败: {e}");
                if let Some(app_type) = scope.apps().first() {
                    record_switch_failure(app_type, e.to_string());
                }
                refresh_tray_menu(&app_handle);
            }
        }
    });
    true
}

/// 处理供应商托盘事件（`prov:<应用>:<供应商 id>`）
pub fn handle_provider_tray_event(app: &tauri::AppHandle, event_id: &str) -> bool {
    let Some(rest) = event_id.strip_prefix("prov:") else {
        return false;
    };
    let Some((app_str, provider_id)) = rest.split_once(':') else {
        log::warn!("无法解析供应商托盘事件: {event_id}");
        return true;
    };
    let Ok(app_type) = app_str.parse::<AppType>() else {
        log::warn!("未知应用的供应商托盘事件: {event_id}");
        return true;
    };
    log::info!("托盘切换 {} 到 {provider_id}", app_display_name(&app_type));
    let app_handle = app.clone();
    let provider_id = provider_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let desktop_was_mapping = app_type == AppType::ClaudeDesktop
            && app_handle.try_state::<AppState>().is_some_and(|state| {
                crate::claude_desktop_config::current_provider_uses_proxy(&state.db)
            });
        match handle_provider_click(&app_handle, &app_type, &provider_id) {
            Ok(ClickOutcome::Switched { mode, name }) => {
                clear_app_problems(&app_type);
                if let Some(feedback) = switch_feedback(&app_type, mode, name) {
                    record_feedback(&app_handle, feedback);
                }
                emit_switched(&app_handle, &app_type, &provider_id);
                if app_type == AppType::ClaudeDesktop {
                    // 换上模型映射卡要把路由服务拉起来、换走时别人不用就停掉（同主界面
                    // `switch_provider`），对齐之后再重建一次。
                    let handle = app_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(state) = handle.try_state::<AppState>() {
                            crate::mode::controller::sync_desktop_mapping_service(
                                state.inner(),
                                desktop_was_mapping,
                            )
                            .await;
                        }
                        refresh_tray_menu(&handle);
                    });
                }
            }
            Ok(ClickOutcome::NeedsRoute) => {
                navigate(
                    &app_handle,
                    TrayNavigation {
                        app: Some(app_type.as_str().to_string()),
                        intent: Some("needsRoute".to_string()),
                        provider_id: Some(provider_id.clone()),
                        ..Default::default()
                    },
                );
                return;
            }
            Ok(ClickOutcome::Unchanged) => {}
            Err(e) => {
                log::error!("托盘切换{}失败: {e}", app_display_name(&app_type));
                record_switch_failure(&app_type, e.to_string());
            }
        }
        // 打勾项点了会自己翻转勾选：成功、没动、失败都要按实际状态重建。
        refresh_tray_menu(&app_handle);
    });
    true
}

enum ClickOutcome {
    /// 切过去了；`mode` 是点击时的模式，决定要不要出反馈行。
    Switched { mode: TrayMode, name: String },
    /// 已经在用 / 故障转移开着（只读）：什么都不做。
    Unchanged,
    /// 直连下点了需要路由的那家：不直接切。
    NeedsRoute,
}

/// 点一家供应商：切换照旧走 `ProviderService::switch`（里面先拿切换锁再看模式）。不再先关掉
/// 自动故障转移（#6022）；点已勾着的那家不再写一次客户端文件。
fn handle_provider_click(
    app: &tauri::AppHandle,
    app_type: &AppType,
    provider_id: &str,
) -> Result<ClickOutcome, AppError> {
    let Some(app_state) = app.try_state::<AppState>() else {
        return Ok(ClickOutcome::Unchanged);
    };
    let state = app_state.inner();
    let current = crate::mode::current::provider_for(
        &state.db,
        app_type,
        crate::mode::current::Purpose::InUse,
    )?;
    if current.as_deref() == Some(provider_id) {
        return Ok(ClickOutcome::Unchanged);
    }
    let mode = tray_mode(state, app_type);
    if mode == TrayMode::Failover {
        log::info!(
            "{} 的故障转移开着，托盘只读，不切换",
            app_display_name(app_type)
        );
        return Ok(ClickOutcome::Unchanged);
    }
    let provider = state
        .db
        .get_provider_by_id(provider_id, app_type.as_str())?
        .ok_or_else(|| AppError::Message(format!("供应商 {provider_id} 不存在")))?;
    if mode == TrayMode::Direct && provider_needs_routing(app_type, &provider) {
        return Ok(ClickOutcome::NeedsRoute);
    }
    crate::services::ProviderService::switch(state, app_type.clone(), provider_id)?;
    Ok(ClickOutcome::Switched {
        mode,
        name: provider.name,
    })
}

fn emit_switched(app: &tauri::AppHandle, app_type: &AppType, provider_id: &str) {
    let proxy_enabled = crate::mode::current::is_proxy(app_type);
    let auto_failover = app
        .try_state::<AppState>()
        .map(|state| state.db.get_proxy_flags_sync(app_type.as_str()).1)
        .unwrap_or(false);
    let event_data = serde_json::json!({
        "appType": app_type.as_str(),
        "proxyEnabled": proxy_enabled,
        "autoFailoverEnabled": auto_failover,
        "providerId": provider_id
    });
    if let Err(e) = app.emit("proxy-flags-changed", event_data.clone()) {
        log::error!("发射 proxy-flags-changed 事件失败: {e}");
    }
    if let Err(e) = app.emit("provider-switched", event_data) {
        log::error!("发射 provider-switched 事件失败: {e}");
    }
}

#[cfg(target_os = "macos")]
pub fn apply_tray_policy(app: &tauri::AppHandle, dock_visible: bool) {
    use tauri::ActivationPolicy;

    let desired_policy = if dock_visible {
        ActivationPolicy::Regular
    } else {
        ActivationPolicy::Accessory
    };

    if let Err(err) = app.set_dock_visibility(dock_visible) {
        log::warn!("设置 Dock 显示状态失败: {err}");
    }

    if let Err(err) = app.set_activation_policy(desired_policy) {
        log::warn!("设置激活策略失败: {err}");
    }
}

/// 处理托盘菜单事件
pub fn handle_tray_menu_event(app: &tauri::AppHandle, event_id: &str) {
    log::info!("处理托盘菜单事件: {event_id}");

    match event_id {
        "show_main" => show_main_window(app),
        "open_website" => {
            if let Err(e) = app.opener().open_url("https://ccswitch.io", None::<String>) {
                log::error!("打开官方网站失败: {e}");
            }
        }
        "lightweight_mode" => {
            if crate::lightweight::is_lightweight_mode() {
                if let Err(e) = crate::lightweight::exit_lightweight_mode(app) {
                    log::error!("退出轻量模式失败: {e}");
                }
            } else if let Err(e) = crate::lightweight::enter_lightweight_mode(app) {
                log::error!("进入轻量模式失败: {e}");
            }
        }
        "quit" => {
            log::info!("退出应用");
            app.exit(0);
        }
        _ => {
            if let Some(navigation) = parse_nav_id(event_id) {
                navigate(app, navigation);
                return;
            }
            if handle_profile_tray_event(app, event_id) {
                return;
            }
            if handle_provider_tray_event(app, event_id) {
                return;
            }
            log::warn!("未处理的菜单事件: {event_id}");
        }
    }
}

// ─── 额度刷新 ─────────────────────────────────────────────────────────────────

static LAST_TRAY_USAGE_REFRESH: Mutex<Option<std::time::Instant>> = Mutex::new(None);
const MIN_TRAY_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// 合并多次快速触发的"usage 标题软更新"：批量刷新期间多个 usage 命令
/// 同时成功时，只会产生一次就地 `set_text` 批量调用。走软更新而不是
/// `refresh_tray_menu` 整建，避免用户打开中的菜单被 macOS 系统关闭。
static TRAY_REBUILD_SCHEDULED: AtomicBool = AtomicBool::new(false);

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    if TRAY_REBUILD_SCHEDULED.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // 50ms 合窗：让同一轮 React Query / 托盘批量刷新触发的多个写入
        // 共享一次标题更新。
        std::thread::sleep(std::time::Duration::from_millis(50));
        TRAY_REBUILD_SCHEDULED.store(false, Ordering::Release);
        update_tray_usage_labels(&app);
    });
}

/// 并行刷新每个可见应用"在用的那家"的用量；成功 / 失败结果都通过各
/// command 的 write-through 逻辑写入 `UsageCache`，单次标题更新由
/// `schedule_tray_refresh` 做合并。内部 10 秒节流防止鼠标悬停反复进出时
/// 雪崩请求；互斥锁被毒化时以上次状态为准继续推进，不会永久阻塞。
///
/// 刷新面与 `usage_view` 的展示面严格对齐 —— 每次悬停最多发
/// `TRAY_APPS.len()` 个用量查询；按供应商用量开关查询，Codex 托管账号
/// 未保存开关时与卡片一致默认启用。
pub(crate) async fn refresh_all_usage_in_tray(app: &tauri::AppHandle) {
    use crate::commands::CopilotAuthState;
    use futures::future::join_all;

    {
        let mut guard = lock(&LAST_TRAY_USAGE_REFRESH);
        let now = std::time::Instant::now();
        if let Some(last) = *guard {
            if now.duration_since(last) < MIN_TRAY_USAGE_REFRESH_INTERVAL {
                return;
            }
        }
        *guard = Some(now);
    }

    let Some(app_state) = app.try_state::<AppState>() else {
        return;
    };

    // 与 `create_tray_menu` 保持一致：用户隐藏的 app 不参与外部 API 查询，
    // 避免在未使用的 app 上浪费请求、撞 rate limit 或反复触发鉴权失败日志。
    let visible_apps = crate::settings::get_settings()
        .visible_apps
        .unwrap_or_default();

    let mut usage_futures = Vec::new();

    for app_type in TRAY_APPS.iter() {
        if !visible_apps.is_visible(app_type) {
            continue;
        }

        let app_type_str = app_type.as_str();
        let log_name = app_display_name(app_type);

        // 解析在用的那家；未设置 / 出错都静默跳过，与 create_tray_menu 的行为保持一致。
        let current_id = match crate::mode::current::provider_for(
            &app_state.db,
            app_type,
            crate::mode::current::Purpose::InUse,
        ) {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                continue;
            }
        };
        // 只需当前 provider —— by-id 查询避免把整个 app 的 provider 列表加载
        // 进内存（每次悬停的热路径）。
        let current = match app_state.db.get_provider_by_id(&current_id, app_type_str) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                log::warn!("[Tray] 读取{log_name}当前供应商失败: {e}");
                continue;
            }
        };

        if let Some(source) = tray_usage_source(app_type, &current) {
            let app_clone = app.clone();
            let state = app.state::<AppState>();
            let copilot_state = app.state::<CopilotAuthState>();
            let xai_state = app.state::<crate::commands::XaiOAuthState>();
            let provider_id = current_id.clone();
            let app_str = app_type_str.to_string();
            usage_futures.push(async move {
                let result = match source {
                    TrayUsageSource::ManagedCodex(account_id) => {
                        let codex_state = app.state::<crate::commands::CodexOAuthState>();
                        crate::commands::get_codex_oauth_quota(
                            app_clone,
                            state,
                            Some(account_id),
                            codex_state,
                        )
                        .await
                        .map(|_| ())
                    }
                    TrayUsageSource::Subscription => {
                        crate::commands::get_subscription_quota(app_clone, state, app_str)
                            .await
                            .map(|_| ())
                    }
                    TrayUsageSource::Script => crate::commands::queryProviderUsage(
                        app_clone,
                        state,
                        copilot_state,
                        xai_state,
                        provider_id.clone(),
                        app_str,
                    )
                    .await
                    .map(|_| ()),
                };
                if let Err(e) = result {
                    log::debug!("[Tray] 刷新{log_name}供应商 {provider_id} 用量失败: {e}");
                }
            });
        }
    }

    join_all(usage_futures).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{UsageData, UsageResult};
    use crate::services::subscription::{
        CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_GEMINI_FLASH,
        TIER_GEMINI_FLASH_LITE, TIER_GEMINI_PRO, TIER_MONTHLY, TIER_SEVEN_DAY,
        TIER_SEVEN_DAY_FABLE, TIER_SEVEN_DAY_OPUS, TIER_SEVEN_DAY_SONNET, TIER_THIRTY_DAY,
        TIER_WEEKLY_LIMIT,
    };

    fn en() -> TrayTexts {
        TrayTexts::from_language("en")
    }

    fn zh() -> TrayTexts {
        TrayTexts::from_language("zh")
    }

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local::now()
    }

    // ─── 基础 ───

    #[test]
    fn tray_id_is_unique_to_app() {
        assert_eq!(TRAY_ID, "cc-switch");
        assert_ne!(TRAY_ID, "main");
    }

    #[test]
    fn tray_lists_switch_apps_in_sidebar_order_without_additive_apps() {
        let names: Vec<&str> = TRAY_APPS.iter().map(app_display_name).collect();
        assert_eq!(
            names,
            [
                "Claude Code",
                "Claude Desktop",
                "Codex",
                "Gemini CLI",
                "Grok Build"
            ]
        );
        for app in TRAY_APPS {
            assert!(!app.is_additive_mode(), "{app:?} 是累加式应用，不进托盘");
        }
    }

    #[test]
    fn locale_maps_traditional_chinese_variants_to_zh_tw() {
        for locale in [
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-Hant",
            "zh-Hant-TW",
            "zh-hant-hk",
        ] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh-TW",
                "expected {locale} -> zh-TW"
            );
        }
    }

    #[test]
    fn locale_maps_simplified_chinese_variants_to_zh() {
        for locale in ["zh", "zh-CN", "zh-SG", "zh-Hans", "zh-Hans-CN"] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh",
                "expected {locale} -> zh"
            );
        }
    }

    #[test]
    fn locale_maps_japanese_and_english() {
        assert_eq!(map_locale_to_tray_language("ja-JP"), "ja");
        assert_eq!(map_locale_to_tray_language("ja"), "ja");
        assert_eq!(map_locale_to_tray_language("en-US"), "en");
        assert_eq!(map_locale_to_tray_language("en"), "en");
    }

    #[test]
    fn locale_unknown_falls_back_to_zh() {
        // 与前端 getInitialLanguage 的默认值保持一致。
        for locale in ["de-DE", "fr", "ko-KR", ""] {
            assert_eq!(
                map_locale_to_tray_language(locale),
                "zh",
                "expected {locale} -> zh (default)"
            );
        }
    }

    #[test]
    fn mode_names_follow_the_v7_terms_in_every_language() {
        let zh = zh();
        assert_eq!(
            (zh.header_direct, zh.mode_route, zh.mode_stack),
            ("直连", "路由", "聚合")
        );
        assert_eq!(zh.header_failover, "路由 · 故障转移开启中");
        let tw = TrayTexts::from_language("zh-TW");
        assert_eq!((tw.header_direct, tw.mode_stack), ("直連", "聚合"));
        let en = en();
        assert_eq!(
            (en.header_direct, en.mode_route, en.mode_stack),
            ("Direct", "Routing", "Aggregation")
        );
        let ja = TrayTexts::from_language("ja");
        assert_eq!(ja.mode_route, "ルーティング");
        for texts in [zh, tw, en, ja] {
            // 托盘不用 emoji 表达状态（Windows 菜单里彩色 emoji 会变成单色轮廓）。
            for text in [
                texts.almost_out,
                texts.needs_attention,
                texts.official_blocked_suffix,
            ] {
                assert!(text.chars().all(|c| (c as u32) < 0x1F000), "{text}");
            }
        }
    }

    // ─── 额度文字 ───

    fn make_quota(tool: &str, success: bool, tiers: Vec<QuotaTier>) -> SubscriptionQuota {
        SubscriptionQuota {
            tool: tool.to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success,
            tiers,
            extra_usage: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn tier(name: &str, utilization: f64) -> QuotaTier {
        QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at: None,
            used_value_usd: None,
            max_value_usd: None,
        }
    }

    fn usage_data(plan_name: Option<&str>, utilization: f64) -> UsageData {
        UsageData {
            plan_name: plan_name.map(String::from),
            extra: None,
            is_valid: Some(true),
            invalid_message: None,
            total: Some(100.0),
            used: Some(utilization),
            remaining: Some(100.0 - utilization),
            unit: Some("%".to_string()),
        }
    }

    fn usage_result(success: bool, data: Vec<UsageData>) -> UsageResult {
        UsageResult {
            success,
            data: if data.is_empty() { None } else { Some(data) },
            error: None,
        }
    }

    fn title(view: Option<QuotaView>) -> Option<(String, bool)> {
        view.as_ref().and_then(quota_title)
    }

    fn sub_title(texts: &TrayTexts, quota: &SubscriptionQuota) -> Option<String> {
        title(format_subscription_quota(texts, quota)).map(|(text, _)| text)
    }

    #[test]
    fn subscription_quota_is_written_as_what_is_left() {
        let quota = make_quota(
            "claude",
            true,
            vec![tier(TIER_FIVE_HOUR, 31.0), tier(TIER_SEVEN_DAY, 95.0)],
        );
        assert_eq!(
            title(format_subscription_quota(&zh(), &quota)),
            Some(("5 小时剩余 69% · 每周剩余 5%".to_string(), true))
        );
        assert_eq!(
            title(format_subscription_quota(&en(), &quota)),
            Some(("5-hour 69% left · Weekly 5% left".to_string(), true))
        );
        let tw = TrayTexts::from_language("zh-TW");
        assert_eq!(
            sub_title(&tw, &quota).as_deref(),
            Some("5 小時剩餘 69% · 每週剩餘 5%")
        );
    }

    #[test]
    fn almost_out_only_below_ten_percent_left_and_not_when_used_up() {
        let at = |used: f64| {
            title(format_subscription_quota(
                &zh(),
                &make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, used)]),
            ))
            .unwrap()
        };
        assert_eq!(at(85.0), ("5 小时剩余 15%".to_string(), false));
        assert_eq!(at(91.0), ("5 小时剩余 9%".to_string(), true));
        // 用完时额度本身写「已用完」，不再加「快用完」。
        assert_eq!(at(100.0), ("5 小时已用完".to_string(), false));
    }

    #[test]
    fn latin_labels_get_a_space_before_the_chinese_word() {
        let quota = make_quota(
            "gemini",
            true,
            vec![tier(TIER_GEMINI_PRO, 15.0), tier(TIER_GEMINI_FLASH, 42.0)],
        );
        assert_eq!(
            sub_title(&zh(), &quota).as_deref(),
            Some("Pro 剩余 85% · Flash 剩余 58%")
        );
    }

    #[test]
    fn title_keeps_the_two_tiers_with_least_left_in_original_order() {
        let quota = make_quota(
            "gemini",
            true,
            vec![
                tier(TIER_GEMINI_PRO, 5.0),
                tier(TIER_GEMINI_FLASH, 42.0),
                tier(TIER_GEMINI_FLASH_LITE, 80.0),
            ],
        );
        assert_eq!(
            sub_title(&en(), &quota).as_deref(),
            Some("Flash 58% left · Flash Lite 20% left")
        );
    }

    #[test]
    fn fable_stays_separate_and_weekly_aliases_use_the_highest() {
        let quota = make_quota(
            "claude",
            true,
            vec![
                tier(TIER_FIVE_HOUR, 12.0),
                tier(TIER_SEVEN_DAY_OPUS, 20.0),
                tier(TIER_SEVEN_DAY_SONNET, 95.0),
                tier(TIER_SEVEN_DAY_FABLE, 30.0),
            ],
        );
        let Some(QuotaView::Lines(lines)) = format_subscription_quota(&en(), &quota) else {
            panic!("expected lines");
        };
        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(
            texts,
            ["5-hour 88% left", "Weekly 5% left", "Fable 70% left"]
        );
    }

    #[test]
    fn monthly_and_thirty_day_windows_keep_their_own_words() {
        let codex_free = make_quota("codex", true, vec![tier(TIER_THIRTY_DAY, 85.0)]);
        assert_eq!(
            sub_title(&zh(), &codex_free).as_deref(),
            Some("30 天剩余 15%")
        );
        let volcengine = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 25.0),
                usage_data(Some(TIER_WEEKLY_LIMIT), 30.0),
                usage_data(Some(TIER_MONTHLY), 42.0),
            ],
        );
        let Some(QuotaView::Lines(lines)) = format_script_result(&zh(), &volcengine) else {
            panic!("expected lines");
        };
        assert_eq!(lines[2].text, "每月剩余 58%");
        assert!(!lines.iter().any(|line| line.text.contains("monthly")));
    }

    #[test]
    fn flattened_subscription_tiers_match_the_subscription_text() {
        let quota = make_quota(
            "claude",
            true,
            vec![tier(TIER_FIVE_HOUR, 12.0), tier(TIER_SEVEN_DAY, 25.0)],
        );
        let result = usage_result(
            true,
            vec![
                usage_data(Some(TIER_FIVE_HOUR), 12.0),
                usage_data(Some(TIER_SEVEN_DAY), 25.0),
            ],
        );
        assert_eq!(
            format_script_result(&en(), &result),
            format_subscription_quota(&en(), &quota)
        );
    }

    #[test]
    fn balance_is_shown_and_warns_below_ten_percent_of_total() {
        let balance = |remaining: f64, total: Option<f64>| UsageData {
            plan_name: Some("CNY".to_string()),
            extra: None,
            is_valid: Some(true),
            invalid_message: None,
            total,
            used: None,
            remaining: Some(remaining),
            unit: Some("CNY".to_string()),
        };
        let one = |data| title(format_script_result(&zh(), &usage_result(true, vec![data])));
        assert_eq!(
            one(balance(82.1, None)),
            Some(("余额 82.10 CNY".to_string(), false))
        );
        assert_eq!(
            one(balance(5.0, Some(100.0))),
            Some(("余额 5.00 CNY".to_string(), true))
        );
        assert_eq!(
            one(balance(0.0, Some(100.0))),
            Some(("余额已用完".to_string(), false))
        );
    }

    #[test]
    fn copilot_premium_requests_use_the_card_word() {
        let data = UsageData {
            plan_name: Some("copilot_pro".to_string()),
            extra: Some("Reset: 2026-11-01".to_string()),
            is_valid: Some(true),
            invalid_message: None,
            total: Some(300.0),
            used: Some(120.0),
            remaining: Some(180.0),
            unit: Some(COPILOT_UNIT_PREMIUM.to_string()),
        };
        assert_eq!(
            title(format_script_result(&zh(), &usage_result(true, vec![data]))),
            Some(("高级请求剩余 60%".to_string(), false))
        );
    }

    #[test]
    fn expired_plan_and_failed_queries() {
        let mut expired = usage_data(None, 10.0);
        expired.is_valid = Some(false);
        assert_eq!(
            title(format_script_result(
                &zh(),
                &usage_result(true, vec![expired])
            )),
            Some(("套餐已过期".to_string(), false))
        );

        let failed = format_script_result(&zh(), &usage_result(false, vec![]));
        assert_eq!(failed, Some(QuotaView::Failed(QuotaFailure::Other)));
        // 查询失败时标题不写额度，原因写在子菜单里。
        assert_eq!(title(failed.clone()), None);
        assert_eq!(
            quota_note(&zh(), &failed.unwrap(), now()).as_deref(),
            Some("额度没查到")
        );

        let login =
            SubscriptionQuota::error("claude", CredentialStatus::Expired, "expired".to_string());
        let view = format_subscription_quota(&zh(), &login).unwrap();
        assert_eq!(
            quota_note(&zh(), &view, now()).as_deref(),
            Some("额度没查到：登录已过期")
        );
        let pending = SubscriptionQuota::error(
            "claude",
            CredentialStatus::RefreshPending,
            "pending".to_string(),
        );
        let view = format_subscription_quota(&zh(), &pending).unwrap();
        assert_eq!(
            quota_note(&zh(), &view, now()).as_deref(),
            Some("额度没查到：令牌待刷新")
        );
        // 没有凭据时不说话。
        let missing =
            SubscriptionQuota::error("claude", CredentialStatus::NotFound, "missing".to_string());
        assert_eq!(format_subscription_quota(&zh(), &missing), None);
    }

    #[test]
    fn unknown_tiers_alone_show_nothing() {
        let quota = make_quota("claude", true, vec![tier("one_hour", 80.0)]);
        assert_eq!(format_subscription_quota(&en(), &quota), None);
    }

    #[test]
    fn reset_note_appears_only_when_running_out() {
        let later = (now() + chrono::Duration::days(3)).to_rfc3339();
        let mut weekly = tier(TIER_SEVEN_DAY, 95.0);
        weekly.resets_at = Some(later.clone());
        let quota = make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 10.0), weekly]);
        let view = format_subscription_quota(&zh(), &quota).unwrap();
        let note = quota_note(&zh(), &view, now()).expect("note");
        assert!(note.starts_with("每周额度 "), "{note}");
        assert!(note.ends_with("日重置"), "{note}");
        let en_view = format_subscription_quota(&en(), &quota).unwrap();
        let en_note = quota_note(&en(), &en_view, now()).unwrap();
        assert!(en_note.starts_with("Weekly quota resets "), "{en_note}");

        let mut calm = tier(TIER_SEVEN_DAY, 50.0);
        calm.resets_at = Some(later);
        let calm = format_subscription_quota(&zh(), &make_quota("claude", true, vec![calm]));
        assert_eq!(quota_note(&zh(), &calm.unwrap(), now()), None);
    }

    // ─── 🔴 #7267：托管 Codex 账号卡按账号取额度 ───

    fn codex_provider(account_id: Option<&str>, enabled: Option<bool>) -> Provider {
        serde_json::from_value(serde_json::json!({
            "id": "managed-codex",
            "name": "Managed Codex",
            "settingsConfig": {"auth": {}, "config": ""},
            "category": "official",
            "meta": {
                "authBinding": account_id.map(|id| serde_json::json!({
                    "source": "managed_account",
                    "authProvider": "codex_oauth",
                    "accountId": id
                })),
                "usage_script": enabled.map(|enabled| serde_json::json!({
                    "enabled": enabled,
                    "language": "javascript",
                    "code": "",
                    "templateType": "official_subscription"
                }))
            }
        }))
        .unwrap()
    }

    #[test]
    fn managed_codex_quota_stays_out_of_the_app_wide_tray_cache() {
        let provider = |id| codex_provider(id, Some(true));
        assert!(!provider_uses_official_subscription(&provider(Some(
            "account-1"
        ))));
        assert!(provider_uses_official_subscription(&provider(None)));
    }

    #[test]
    fn managed_codex_tray_refresh_respects_binding_and_usage_toggle() {
        for enabled in [Some(true), None] {
            let provider = codex_provider(Some("account-1"), enabled);
            assert_eq!(
                tray_usage_source(&AppType::Codex, &provider),
                Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
            );
        }
        let disabled = codex_provider(Some("account-1"), Some(false));
        assert_eq!(tray_usage_source(&AppType::Codex, &disabled), None);

        let mut fixed = codex_provider(Some("account-1"), Some(true));
        fixed.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        assert_eq!(
            tray_usage_source(&AppType::Codex, &fixed),
            Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
        );
        assert_eq!(
            tray_usage_source(&AppType::Codex, &codex_provider(None, Some(true))),
            Some(TrayUsageSource::Subscription)
        );
        let mut legacy = codex_provider(Some(" account-1 "), None);
        legacy.category = None;
        assert_eq!(
            tray_usage_source(&AppType::Codex, &legacy),
            Some(TrayUsageSource::ManagedCodex("account-1".to_string()))
        );
    }

    #[test]
    fn managed_codex_tray_uses_bound_account_after_switch_or_rebind() {
        let cache = UsageCache::new();
        let texts = en();
        let first = codex_provider(Some("account-1"), Some(true));
        let mut second = codex_provider(Some("account-2"), Some(true));
        second.id = "second-provider".to_string();
        let label = |provider: &Provider| {
            title(usage_view(
                &cache,
                &texts,
                &AppType::Codex,
                provider,
                &provider.id,
            ))
            .map(|(text, _)| text)
        };
        cache.put_subscription(
            AppType::Codex,
            make_quota("codex", true, vec![tier(TIER_FIVE_HOUR, 99.0)]),
        );
        cache.put_script(
            AppType::Codex,
            first.id.clone(),
            usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 99.0)]),
        );
        // Neither CLI nor stale provider-scoped data may fill an account miss.
        assert_eq!(label(&first), None);
        cache.put_codex_oauth(
            "account-1".to_string(),
            make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 12.0)]),
        );
        assert_eq!(label(&first).as_deref(), Some("5-hour 88% left"));
        assert_eq!(label(&second), None);
        cache.put_codex_oauth(
            "account-2".to_string(),
            make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
        );
        assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
        // Rebinding the same provider must select the new account's snapshot.
        second.id = first.id.clone();
        assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
        // A late response for the previous account cannot overwrite this label.
        cache.put_codex_oauth(
            "account-1".to_string(),
            make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 40.0)]),
        );
        assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
        assert_eq!(label(&first).as_deref(), Some("5-hour 60% left"));

        let disabled = codex_provider(Some("account-2"), Some(false));
        assert_eq!(label(&disabled), None);
        assert_eq!(label(&second).as_deref(), Some("5-hour 75% left"));
        cache.put_codex_oauth(
            "account-2".to_string(),
            SubscriptionQuota::error(
                "codex_oauth",
                CredentialStatus::Expired,
                "expired".to_string(),
            ),
        );
        assert_eq!(label(&second), None);
        assert_eq!(
            usage_view(&cache, &texts, &AppType::Codex, &second, &second.id),
            Some(QuotaView::Failed(QuotaFailure::LoginExpired))
        );
        assert_eq!(label(&first).as_deref(), Some("5-hour 60% left"));
    }

    #[test]
    fn native_codex_tray_still_uses_cli_subscription() {
        let cache = UsageCache::new();
        let mut native = codex_provider(None, Some(true));
        native.id = crate::database::CODEX_OFFICIAL_PROVIDER_ID.to_string();
        cache.put_codex_oauth(
            "account-1".to_string(),
            make_quota("codex_oauth", true, vec![tier(TIER_FIVE_HOUR, 12.0)]),
        );
        cache.put_subscription(
            AppType::Codex,
            make_quota("codex", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
        );
        assert_eq!(
            title(usage_view(
                &cache,
                &en(),
                &AppType::Codex,
                &native,
                &native.id
            ))
            .map(|(text, _)| text)
            .as_deref(),
            Some("5-hour 75% left")
        );
    }

    #[test]
    fn cli_subscription_tray_reads_the_cache_the_card_writes() {
        let cache = UsageCache::new();
        let native = codex_provider(None, Some(true));
        assert_eq!(
            tray_usage_source(&AppType::Claude, &native),
            Some(TrayUsageSource::Subscription)
        );
        let view = || usage_view(&cache, &en(), &AppType::Claude, &native, &native.id);

        // 脚本缓存里的同供应商结果不能盖住订阅缓存。
        cache.put_script(
            AppType::Claude,
            native.id.clone(),
            usage_result(true, vec![usage_data(Some(TIER_FIVE_HOUR), 99.0)]),
        );
        assert_eq!(view(), None);
        cache.put_subscription(
            AppType::Claude,
            SubscriptionQuota::error(
                "claude",
                CredentialStatus::RefreshPending,
                "pending".to_string(),
            ),
        );
        assert_eq!(
            view(),
            Some(QuotaView::Failed(QuotaFailure::TokenRefreshPending))
        );
        cache.put_subscription(
            AppType::Claude,
            make_quota("claude", true, vec![tier(TIER_FIVE_HOUR, 25.0)]),
        );
        assert_eq!(
            title(view()).map(|(text, _)| text).as_deref(),
            Some("5-hour 75% left")
        );

        // xAI OAuth 的额度属于绑定的 SuperGrok 账号：仍走脚本路径，也不沿用应用级订阅缓存。
        let mut xai = codex_provider(None, Some(true));
        xai.meta.as_mut().unwrap().provider_type = Some("xai_oauth".to_string());
        assert_eq!(
            tray_usage_source(&AppType::Claude, &xai),
            Some(TrayUsageSource::Script)
        );
        assert_eq!(
            title(usage_view(&cache, &en(), &AppType::Claude, &xai, "xai")),
            None
        );
    }

    // ─── 「需要路由」判定（和前端 providerNeedsRouting 对照）───

    fn provider(meta: serde_json::Value, settings: serde_json::Value) -> Provider {
        serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "P1",
            "settingsConfig": settings,
            "meta": meta,
        }))
        .unwrap()
    }

    #[test]
    fn needs_routing_mirrors_the_frontend_rule() {
        let plain = provider(serde_json::json!({}), serde_json::json!({}));
        for app in TRAY_APPS {
            assert!(!provider_needs_routing(&app, &plain), "{app:?}");
        }

        let copilot = provider(
            serde_json::json!({"providerType": "github_copilot"}),
            serde_json::json!({}),
        );
        for app in [
            AppType::Claude,
            AppType::ClaudeDesktop,
            AppType::Codex,
            AppType::GrokBuild,
        ] {
            assert!(provider_needs_routing(&app, &copilot), "{app:?}");
        }
        assert!(!provider_needs_routing(&AppType::Gemini, &copilot));

        let openai_chat = provider(
            serde_json::json!({"apiFormat": "openai_chat"}),
            serde_json::json!({}),
        );
        assert!(provider_needs_routing(&AppType::Claude, &openai_chat));
        assert!(provider_needs_routing(&AppType::Codex, &openai_chat));
        let anthropic = provider(
            serde_json::json!({"apiFormat": "anthropic"}),
            serde_json::json!({}),
        );
        assert!(!provider_needs_routing(&AppType::Claude, &anthropic));
        assert!(provider_needs_routing(&AppType::Codex, &anthropic));
        let full_url = provider(
            serde_json::json!({"isFullUrl": true}),
            serde_json::json!({}),
        );
        assert!(provider_needs_routing(&AppType::Claude, &full_url));

        let codex_config = |wire_api: &str| {
            serde_json::json!({
                "auth": {"OPENAI_API_KEY": "sk"},
                "config": format!(
                    "model_provider = \"x\"\n[model_providers.x]\nbase_url = \"https://x.example/v1\"\nwire_api = \"{wire_api}\"\n"
                )
            })
        };
        let chat_wire = provider(serde_json::json!({}), codex_config("chat"));
        assert!(provider_needs_routing(&AppType::Codex, &chat_wire));
        let responses_wire = provider(serde_json::json!({}), codex_config("responses"));
        assert!(!provider_needs_routing(&AppType::Codex, &responses_wire));

        let desktop_mapping = provider(
            serde_json::json!({"claudeDesktopMode": "proxy"}),
            serde_json::json!({}),
        );
        assert!(provider_needs_routing(
            &AppType::ClaudeDesktop,
            &desktop_mapping
        ));

        // 官方卡永远不算。
        let mut official = copilot.clone();
        official.category = Some("official".to_string());
        assert!(!provider_needs_routing(&AppType::Claude, &official));
    }

    #[test]
    fn codex_wire_api_falls_back_to_a_single_assignment_when_toml_is_broken() {
        assert_eq!(
            codex_wire_api("wire_api = \"chat\"\nbroken = [").as_deref(),
            Some("chat")
        );
        assert_eq!(
            codex_wire_api("wire_api = \"chat\"\nwire_api = \"responses\"\nx = ["),
            None
        );
    }

    // ─── 菜单模型 ───

    fn entry(id: &str, name: &str) -> ProviderEntry {
        ProviderEntry {
            id: id.to_string(),
            name: name.to_string(),
            hint: None,
            needs_routing: false,
            official: false,
            blocked_from_routing: false,
        }
    }

    fn snapshot(app: AppType, mode: TrayMode, providers: Vec<ProviderEntry>) -> AppSnapshot {
        AppSnapshot {
            app,
            mode,
            current_id: providers.first().map(|p| p.id.clone()),
            providers,
            queue: Vec::new(),
            stack_members: Vec::new(),
            quota: None,
            service_down: false,
            needs_attention: false,
            profiles: None,
        }
    }

    fn model(problems: &[TrayProblem], apps: &[AppSnapshot]) -> Vec<TrayEntry> {
        let status = MenuStatus {
            problems: problems.to_vec(),
            ..Default::default()
        };
        build_menu_model(&zh(), &status, apps, false, now())
    }

    fn text_of(entry: &TrayEntry) -> String {
        match entry {
            TrayEntry::Item { text, .. }
            | TrayEntry::Check { text, .. }
            | TrayEntry::Submenu { text, .. } => text.clone(),
            TrayEntry::Separator => "---".to_string(),
        }
    }

    fn texts_of(entries: &[TrayEntry]) -> Vec<String> {
        entries.iter().map(text_of).collect()
    }

    fn children_of(menu: &[TrayEntry], app: &AppType) -> Vec<TrayEntry> {
        menu.iter()
            .find_map(|entry| match entry {
                TrayEntry::Submenu {
                    app: Some(a),
                    children,
                    ..
                } if a == app => Some(children.clone()),
                _ => None,
            })
            .expect("submenu")
    }

    #[test]
    fn top_level_order_is_open_apps_lightweight_website_quit() {
        let apps = [
            snapshot(
                AppType::Claude,
                TrayMode::Direct,
                vec![entry("kimi", "Kimi For Coding")],
            ),
            snapshot(AppType::GrokBuild, TrayMode::Direct, vec![]),
        ];
        let menu = model(&[], &apps);
        assert_eq!(
            texts_of(&menu),
            [
                "打开 CC Switch",
                "---",
                "Claude Code · Kimi For Coding",
                "Grok Build · 添加供应商…",
                "---",
                "轻量模式",
                "---",
                "打开官方网站",
                "退出 CC Switch",
            ]
        );
        assert!(matches!(&menu[3], TrayEntry::Item { id, .. } if id == "nav:add:grokbuild"));
        assert!(matches!(&menu[8], TrayEntry::Item { id, .. } if id == "quit"));
    }

    #[test]
    fn direct_submenu_sends_needs_routing_providers_to_the_app_page() {
        let mut copilot = entry("copilot", "GitHub Copilot");
        copilot.needs_routing = true;
        let app = snapshot(
            AppType::Claude,
            TrayMode::Direct,
            vec![entry("kimi", "Kimi For Coding"), copilot],
        );
        let children = children_of(&model(&[], &[app]), &AppType::Claude);
        assert_eq!(
            texts_of(&children),
            [
                "直连",
                "Kimi For Coding",
                "GitHub Copilot（需要路由）…",
                "---",
                "打开 Claude Code 页面"
            ]
        );
        assert!(matches!(
            &children[0],
            TrayEntry::Item { enabled: false, .. }
        ));
        assert!(matches!(
            &children[1],
            TrayEntry::Check { id, checked: true, enabled: true, .. } if id == "prov:claude:kimi"
        ));
        assert!(matches!(
            &children[2],
            TrayEntry::Item { id, enabled: true, .. } if id == "nav:needs:claude:copilot"
        ));
        assert!(matches!(&children[4], TrayEntry::Item { id, .. } if id == "nav:app:claude"));
    }

    #[test]
    fn route_submenu_disables_official_plans_with_the_reason() {
        let mut official = entry("xai", "xAI Official");
        official.official = true;
        official.blocked_from_routing = true;
        let app = snapshot(
            AppType::GrokBuild,
            TrayMode::Route,
            vec![entry("or", "OpenRouter"), official],
        );
        let menu = model(&[], &[app]);
        assert_eq!(text_of(&menu[2]), "Grok Build · 路由 · OpenRouter");
        let children = children_of(&menu, &AppType::GrokBuild);
        assert_eq!(text_of(&children[0]), "路由");
        assert!(
            matches!(&children[2], TrayEntry::Item { enabled: false, text, .. }
            if text == "xAI Official（官方订阅不经过路由）")
        );
    }

    #[test]
    fn failover_submenu_is_read_only_and_lists_only_the_queue() {
        let mut app = snapshot(
            AppType::Claude,
            TrayMode::Failover,
            vec![
                entry("a", "DeepSeek"),
                entry("b", "智谱 GLM Coding Plan"),
                entry("c", "OpenRouter"),
            ],
        );
        app.queue = vec!["a".to_string(), "b".to_string()];
        let children = children_of(&model(&[], &[app]), &AppType::Claude);
        assert_eq!(
            texts_of(&children),
            [
                "路由 · 故障转移开启中",
                "按队列自动选择，要调整请到应用页",
                "1. DeepSeek",
                "2. 智谱 GLM Coding Plan",
                "---",
                "打开 Claude Code 页面"
            ]
        );
        for row in &children[1..4] {
            assert!(
                matches!(
                    row,
                    TrayEntry::Item { enabled: false, .. }
                        | TrayEntry::Check { enabled: false, .. }
                ),
                "{row:?}"
            );
        }
        assert!(matches!(
            &children[2],
            TrayEntry::Check { checked: true, .. }
        ));
    }

    #[test]
    fn stack_submenu_lists_members_and_the_codex_official_card() {
        let mut official = entry("official", "OpenAI Official");
        official.official = true;
        let mut app = snapshot(
            AppType::Codex,
            TrayMode::Stack,
            vec![
                entry("deepseek", "DeepSeek"),
                entry("kimi", "Kimi For Coding"),
                entry("other", "Not In Stack"),
                official,
            ],
        );
        app.stack_members = vec!["deepseek".to_string(), "kimi".to_string()];
        let menu = model(&[], &[app]);
        assert_eq!(text_of(&menu[2]), "Codex · 聚合 · DeepSeek");
        let children = children_of(&menu, &AppType::Codex);
        assert_eq!(
            texts_of(&children),
            [
                "聚合 · 默认 DeepSeek",
                "DeepSeek",
                "Kimi For Coding",
                "OpenAI Official",
                "---",
                "打开 Codex 页面"
            ]
        );
    }

    #[test]
    fn desktop_submenu_marks_mapping_cards_without_a_mode_header() {
        let mut mapping = entry("kimi", "Kimi For Coding");
        mapping.needs_routing = true;
        let app = snapshot(
            AppType::ClaudeDesktop,
            TrayMode::Desktop,
            vec![mapping, entry("official", "Claude Desktop Official")],
        );
        let menu = model(&[], &[app]);
        assert_eq!(
            text_of(&menu[2]),
            "Claude Desktop · 模型映射 · Kimi For Coding"
        );
        let children = children_of(&menu, &AppType::ClaudeDesktop);
        assert_eq!(
            texts_of(&children),
            [
                "Kimi For Coding · 模型映射",
                "Claude Desktop Official",
                "---",
                "打开 Claude Desktop 页面"
            ]
        );
        assert!(
            matches!(&children[0], TrayEntry::Check { id, .. } if id == "prov:claude-desktop:kimi")
        );
    }

    #[test]
    fn service_down_shows_the_problem_row_and_needs_attention_instead_of_quota() {
        let mut mapping = entry("kimi", "Kimi For Coding");
        mapping.needs_routing = true;
        let mut desktop = snapshot(AppType::ClaudeDesktop, TrayMode::Desktop, vec![mapping]);
        desktop.service_down = true;
        desktop.quota = Some(QuotaView::Lines(vec![tier_line(
            &zh(),
            zh().tier_weekly,
            20.0,
            None,
        )]));
        let mut apps = [desktop];
        let problems = [TrayProblem::ServiceDown { port: 15721 }];
        mark_attention(&mut apps, &problems);
        let menu = model(&problems, &apps);
        assert_eq!(text_of(&menu[0]), "路由服务没在运行（端口 15721）");
        assert!(
            matches!(&menu[0], TrayEntry::Item { id, enabled: true, .. } if id == "nav:settings:routing")
        );
        assert_eq!(text_of(&menu[1]), "---");
        assert_eq!(text_of(&menu[2]), "打开 CC Switch");
        assert_eq!(
            text_of(&menu[4]),
            "Claude Desktop · 模型映射 · Kimi For Coding · 需要处理"
        );
        let children = children_of(&menu, &AppType::ClaudeDesktop);
        assert_eq!(
            text_of(&children[0]),
            "路由服务没在运行，Kimi For Coding 暂时不可用"
        );
    }

    #[test]
    fn problem_area_keeps_two_rows_then_says_how_many_more() {
        let problems = [
            TrayProblem::ServiceDown { port: 15721 },
            TrayProblem::AttachFailed {
                app: AppType::Claude,
                stack: false,
            },
            TrayProblem::SwitchFailed {
                app: AppType::Codex,
                reason: "config.toml 格式有误".to_string(),
            },
        ];
        let menu = model(&problems, &[]);
        assert_eq!(
            texts_of(&menu[..4]),
            [
                "路由服务没在运行（端口 15721）",
                "Claude Code：上次的路由没能接上，已回到直连",
                "还有 1 个问题，打开 CC Switch 查看",
                "---"
            ]
        );
        let switch = model(&problems[2..], &[]);
        assert_eq!(
            text_of(&switch[0]),
            "Codex 没切换成功：config.toml 格式有误"
        );
        assert!(matches!(&switch[0], TrayEntry::Item { id, .. } if id == "nav:app:codex:failed"));
    }

    #[test]
    fn quota_title_and_almost_out_suffix_on_the_app_row() {
        let mut app = snapshot(
            AppType::Claude,
            TrayMode::Direct,
            vec![entry("official", "Claude Official")],
        );
        let quota = make_quota(
            "claude",
            true,
            vec![tier(TIER_FIVE_HOUR, 31.0), tier(TIER_SEVEN_DAY, 95.0)],
        );
        app.quota = format_subscription_quota(&zh(), &quota);
        assert_eq!(
            app_row_title(&zh(), &app),
            "Claude Code · Claude Official · 5 小时剩余 69% · 每周剩余 5% · 快用完"
        );
    }

    #[test]
    fn projects_live_inside_the_app_submenu() {
        let mut app = snapshot(
            AppType::Codex,
            TrayMode::Direct,
            vec![entry("deepseek", "DeepSeek")],
        );
        app.profiles = Some(ProfileSection {
            scope: "codex",
            items: vec![
                ("p1".to_string(), "个人项目".to_string()),
                ("p2".to_string(), "公司项目".to_string()),
            ],
            current: Some("p2".to_string()),
        });
        let children = children_of(&model(&[], &[app]), &AppType::Codex);
        assert_eq!(
            texts_of(&children),
            [
                "直连",
                "DeepSeek",
                "---",
                "项目",
                "不使用项目",
                "个人项目",
                "公司项目",
                "---",
                "打开 Codex 页面"
            ]
        );
        assert!(
            matches!(&children[4], TrayEntry::Check { id, checked: false, .. } if id == "profile_none_codex")
        );
        assert!(
            matches!(&children[6], TrayEntry::Check { id, checked: true, .. } if id == "profile_codex_p2")
        );
    }

    #[test]
    fn long_and_duplicate_names_stay_readable() {
        let long = "An Extremely Long Provider Name That Keeps Going On";
        let mut first = entry("a", "OpenAI Official");
        first.hint = Some("me@example.com".to_string());
        let second = entry("b", "OpenAI Official");
        let third = entry("c", long);
        let names = display_names(&[&first, &second, &third]);
        assert_eq!(names[0], "OpenAI Official · me@example.com");
        assert_eq!(names[1], "OpenAI Official #2");
        assert_eq!(names[2].chars().count(), MAX_NAME_CHARS);
        assert!(names[2].ends_with('…'));
    }

    #[test]
    fn lightweight_mode_is_a_check_item() {
        let menu = build_menu_model(&en(), &MenuStatus::default(), &[], true, now());
        assert!(menu.iter().any(|entry| matches!(entry,
            TrayEntry::Check { id, checked: true, text, .. }
                if id == "lightweight_mode" && text == "Lightweight mode")));
    }

    #[test]
    fn stack_header_falls_back_to_the_generic_word_without_a_default() {
        let mut app = snapshot(
            AppType::Claude,
            TrayMode::Stack,
            vec![entry("deepseek", "DeepSeek")],
        );
        app.current_id = None;
        app.stack_members = vec!["deepseek".to_string()];
        let children = children_of(&model(&[], &[app.clone()]), &AppType::Claude);
        assert_eq!(text_of(&children[0]), "聚合 · 默认供应商");
        app.current_id = Some("deepseek".to_string());
        let menu = build_menu_model(&en(), &MenuStatus::default(), &[app], false, now());
        let children = children_of(&menu, &AppType::Claude);
        assert_eq!(text_of(&children[0]), "Aggregation · Default: DeepSeek");
    }

    #[test]
    fn quit_says_the_routing_service_stops_only_while_it_is_in_use() {
        let quit_text = |texts: &TrayTexts, routing: bool| {
            let status = MenuStatus {
                routing_in_use: routing,
                ..Default::default()
            };
            let menu = build_menu_model(texts, &status, &[], false, now());
            match menu.last() {
                Some(TrayEntry::Item { id, text, .. }) if id == "quit" => text.clone(),
                other => panic!("last entry is not quit: {other:?}"),
            }
        };
        assert_eq!(quit_text(&zh(), false), "退出 CC Switch");
        assert_eq!(quit_text(&zh(), true), "退出 CC Switch（路由服务会停止）");
        assert_eq!(
            quit_text(&en(), true),
            "Quit CC Switch (stops the routing service)"
        );
        for language in ["zh", "zh-TW", "en", "ja"] {
            let texts = TrayTexts::from_language(language);
            assert_ne!(texts.quit, texts.quit_stops_routing, "{language}");
            assert!(
                texts.quit_stops_routing.starts_with(texts.quit),
                "{language}"
            );
            assert!(texts.header_stack_default.contains("{name}"), "{language}");
            assert!(texts.tooltip_problem.contains("{problem}"), "{language}");
            for template in [
                texts.feedback_switched,
                texts.feedback_stack_default,
                texts.feedback_profile,
            ] {
                assert!(
                    template.contains("{app}") && template.contains("{name}"),
                    "{language}: {template}"
                );
            }
        }
    }

    #[test]
    fn tooltip_names_the_first_problem_and_is_plain_otherwise() {
        assert_eq!(tray_tooltip(&zh(), &[]), "CC Switch");
        let problems = [
            TrayProblem::ServiceDown { port: 15721 },
            TrayProblem::SwitchFailed {
                app: AppType::Codex,
                reason: "boom".to_string(),
            },
        ];
        assert_eq!(
            tray_tooltip(&zh(), &problems),
            "CC Switch · 需要处理：路由服务没在运行（端口 15721）"
        );
        let long = [TrayProblem::SwitchFailed {
            app: AppType::Codex,
            reason: "x".repeat(500),
        }];
        assert!(tray_tooltip(&en(), &long).chars().count() <= MAX_TOOLTIP_CHARS);
    }

    #[test]
    fn problem_dot_sits_bottom_right_with_a_clear_ring() {
        let size = 72u32;
        let pixel = |image: &Image<'_>, x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            image.rgba()[i..i + 4].to_vec()
        };
        // 整张图都是不透明白色，方便看哪里被挖掉、哪里画了点。
        let base = Image::new_owned(vec![255; (size * size * 4) as usize], size, size);

        let colored = icon_with_problem_dot(&base, false);
        let dot_center = size - (size as f32 * DOT_RING_RADIUS) as u32;
        assert_eq!(
            pixel(&colored, dot_center, dot_center),
            vec![0xF7, 0x63, 0x0C, 255]
        );
        // 圆点和圈之间是透明的。
        let ring = dot_center - (size as f32 * (DOT_RADIUS + DOT_RING_RADIUS) / 2.0) as u32;
        assert_eq!(pixel(&colored, ring, dot_center)[3], 0);
        // 左上角不动。
        assert_eq!(pixel(&colored, 2, 2), vec![255, 255, 255, 255]);

        let template = icon_with_problem_dot(&base, true);
        assert_eq!(pixel(&template, dot_center, dot_center), vec![0, 0, 0, 255]);
        assert_eq!(pixel(&template, 2, 2), vec![255, 255, 255, 255]);

        // 尺寸对不上的图原样返回，不越界。
        let broken = Image::new_owned(vec![1, 2, 3], 4, 4);
        assert_eq!(icon_with_problem_dot(&broken, false).rgba(), &[1, 2, 3]);
    }

    #[test]
    fn feedback_only_for_switches_that_need_a_client_restart() {
        let name = || "DeepSeek".to_string();
        for app in [AppType::Codex, AppType::Gemini, AppType::GrokBuild] {
            assert_eq!(
                switch_feedback(&app, TrayMode::Direct, name()),
                Some(TrayFeedback::Switched {
                    app: app.clone(),
                    name: name()
                })
            );
            // 路由 / 故障转移换一家立即生效。
            assert_eq!(switch_feedback(&app, TrayMode::Route, name()), None);
            assert_eq!(switch_feedback(&app, TrayMode::Failover, name()), None);
        }
        assert_eq!(
            switch_feedback(&AppType::Claude, TrayMode::Direct, name()),
            None
        );
        assert!(matches!(
            switch_feedback(&AppType::ClaudeDesktop, TrayMode::Desktop, name()),
            Some(TrayFeedback::Switched { .. })
        ));
        for app in [AppType::Claude, AppType::Codex] {
            assert!(matches!(
                switch_feedback(&app, TrayMode::Stack, name()),
                Some(TrayFeedback::StackDefault { .. })
            ));
        }
    }

    #[test]
    fn feedback_row_is_a_grey_line_after_the_problems() {
        let status = MenuStatus {
            problems: vec![TrayProblem::ServiceDown { port: 15721 }],
            feedback: Some(TrayFeedback::Switched {
                app: AppType::Codex,
                name: "DeepSeek".to_string(),
            }),
            routing_in_use: false,
        };
        let menu = build_menu_model(&zh(), &status, &[], false, now());
        assert_eq!(
            texts_of(&menu[..4]),
            [
                "路由服务没在运行（端口 15721）",
                "Codex 已切换到 DeepSeek，重启 Codex 后生效",
                "---",
                "打开 CC Switch",
            ]
        );
        assert!(matches!(&menu[1], TrayEntry::Item { enabled: false, .. }));

        let only_feedback = MenuStatus {
            feedback: Some(TrayFeedback::StackDefault {
                app: AppType::Codex,
                name: "Kimi For Coding".to_string(),
            }),
            ..Default::default()
        };
        let menu = build_menu_model(&zh(), &only_feedback, &[], false, now());
        assert_eq!(
            texts_of(&menu[..3]),
            [
                "Codex 的默认供应商改为 Kimi For Coding，重启 Codex 后生效",
                "---",
                "打开 CC Switch",
            ]
        );
        assert_eq!(
            feedback_text(
                &zh(),
                &TrayFeedback::ProfileApplied {
                    app: AppType::Claude,
                    name: "公司项目".to_string()
                }
            ),
            "已把项目「公司项目」用到 Claude Code"
        );
        assert_eq!(
            feedback_text(
                &en(),
                &TrayFeedback::Switched {
                    app: AppType::ClaudeDesktop,
                    name: "DeepSeek".to_string()
                }
            ),
            "Claude Desktop switched to DeepSeek. Restart Claude Desktop to apply"
        );
    }

    #[test]
    fn feedback_goes_away_after_the_menu_showed_it_or_two_minutes() {
        let record = |at: std::time::Instant| {
            *lock(&FEEDBACK) = Some(FeedbackRecord {
                feedback: TrayFeedback::Switched {
                    app: AppType::Codex,
                    name: "DeepSeek".to_string(),
                },
                at,
                seen: false,
            });
        };
        record(std::time::Instant::now());
        assert!(current_feedback().is_some());
        // 中键不弹菜单；右键弹菜单 = 这次菜单里已经显示过。
        note_tray_click(tauri::tray::MouseButton::Middle);
        assert!(current_feedback().is_some());
        note_tray_click(tauri::tray::MouseButton::Right);
        assert!(current_feedback().is_none());
        assert!(lock(&FEEDBACK).is_none());

        if let Some(at) = std::time::Instant::now().checked_sub(FEEDBACK_TTL) {
            record(at);
            assert!(current_feedback().is_none());
        }
    }

    // ─── 托盘 → 主界面 ───

    #[test]
    fn nav_ids_parse_into_navigation_requests() {
        assert_eq!(
            parse_nav_id("nav:needs:claude:pkg:with:colons"),
            Some(TrayNavigation {
                app: Some("claude".to_string()),
                intent: Some("needsRoute".to_string()),
                provider_id: Some("pkg:with:colons".to_string()),
                ..Default::default()
            })
        );
        assert_eq!(
            parse_nav_id("nav:app:claude-desktop:quota"),
            Some(TrayNavigation {
                app: Some("claude-desktop".to_string()),
                ..Default::default()
            })
        );
        assert_eq!(
            parse_nav_id("nav:add:grokbuild"),
            Some(TrayNavigation {
                app: Some("grokbuild".to_string()),
                intent: Some("add".to_string()),
                ..Default::default()
            })
        );
        assert_eq!(
            parse_nav_id("nav:settings:routing"),
            Some(TrayNavigation {
                section: Some("routing".to_string()),
                ..Default::default()
            })
        );
        assert_eq!(parse_nav_id("nav:main"), Some(TrayNavigation::default()));
        assert_eq!(parse_nav_id("nav:needs:claude"), None);
        assert_eq!(parse_nav_id("nav:app:nope"), None);
        assert_eq!(parse_nav_id("prov:claude:x"), None);
        let json = serde_json::to_value(parse_nav_id("nav:needs:codex:p1").unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"app": "codex", "intent": "needsRoute", "providerId": "p1"})
        );
    }

    #[test]
    fn problems_drop_attach_failures_once_the_app_routes_again() {
        lock(&ATTACH_FAILURES).clear();
        lock(&SWITCH_FAILURES).clear();
        lock(&ATTACH_FAILURES).push((AppType::Claude, false));
        record_switch_failure(&AppType::Codex, "boom".to_string());
        let visible = [(AppType::Claude, true), (AppType::Codex, true)];
        assert_eq!(collect_problems(None, &visible).len(), 2);
        // Claude Code 重新进入路由：「退回直连」的问题行消失。
        let rerouted = [(AppType::Claude, false), (AppType::Codex, true)];
        assert_eq!(
            collect_problems(Some(15721), &rerouted),
            [
                TrayProblem::ServiceDown { port: 15721 },
                TrayProblem::SwitchFailed {
                    app: AppType::Codex,
                    reason: "boom".to_string()
                }
            ]
        );
        assert!(clear_app_problems(&AppType::Codex));
        assert!(!clear_app_problems(&AppType::Codex));
        assert!(collect_problems(None, &rerouted).is_empty());
    }
}
