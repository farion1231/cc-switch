//! macOS 菜单栏图标右侧的数据标题与本地路由实时速度。

use crate::app_config::AppType;
use crate::services::usage_stats::UsageSummary;
use crate::settings::{AppSettings, UsageDisplaySettings};
use crate::store::AppState;
use chrono::{Local, TimeZone};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const SPEED_WINDOW: Duration = Duration::from_secs(5);
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const SPEED_REFRESH: Duration = Duration::from_millis(500);
const SPEED_MIN_DENOMINATOR: Duration = Duration::from_millis(100);
// Reserve one stable numeric column so status-item updates do not move neighboring icons.
const SPEED_VALUE_WIDTH: usize = 3;
const SPEED_PADDING: char = '\u{2007}';

const DISPLAY_CURRENCIES: [&str; 6] = ["USD", "CNY", "EUR", "JPY", "TWD", "TRY"];
const DEFAULT_CNY_RATE: f64 = 6.8;
const DEFAULT_EUR_RATE: f64 = 0.87;
const DEFAULT_JPY_RATE: f64 = 158.0;
const DEFAULT_TWD_RATE: f64 = 32.0;
const DEFAULT_TRY_RATE: f64 = 48.0;

static RUNTIME: OnceLock<Arc<MenuBarRuntime>> = OnceLock::new();

struct MenuBarRuntime {
    settings: Mutex<AppSettings>,
    speed: Arc<LiveSpeedRegistry>,
    usage_logging_enabled: AtomicBool,
    daily_generation: AtomicU64,
    daily: Mutex<Option<DailySnapshot>>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    day_key: Mutex<String>,
    applied_title: Mutex<Option<Option<String>>>,
}

#[derive(Debug, Clone, Default)]
struct DailySnapshot {
    total_tokens: u64,
    total_requests: u64,
    total_cost_usd: String,
    cache_hit_rate: f64,
}

impl From<UsageSummary> for DailySnapshot {
    fn from(summary: UsageSummary) -> Self {
        Self {
            total_tokens: summary.real_total_tokens,
            total_requests: summary.total_requests,
            total_cost_usd: summary.total_cost,
            cache_hit_rate: summary.cache_hit_rate,
        }
    }
}

/// 初始化菜单栏运行时。必须放在托盘和 AppState 都已创建之后。
pub fn init(app: AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let settings = crate::settings::get_settings();
        let runtime = Arc::new(MenuBarRuntime {
            speed: Arc::new(LiveSpeedRegistry::new(false)),
            settings: Mutex::new(settings),
            usage_logging_enabled: AtomicBool::new(false),
            daily_generation: AtomicU64::new(0),
            daily: Mutex::new(None),
            day_key: Mutex::new(local_day_key()),
            applied_title: Mutex::new(None),
        });
        if RUNTIME.set(runtime.clone()).is_err() {
            log::debug!("菜单栏运行时重复初始化，已忽略");
            return;
        }

        tauri::async_runtime::spawn(async move {
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(config) = state.proxy_service.get_config().await {
                    runtime
                        .usage_logging_enabled
                        .store(config.enable_logging, Ordering::Release);
                }
            }
            sync_speed_enabled(&runtime);
            refresh_daily(app.clone()).await;
            runtime.apply_title(&app);
            run_ticker(app).await;
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

/// 设置保存后立即应用新的显示项和数据范围。
pub fn settings_changed(app: &AppHandle, settings: &AppSettings) {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    if let Ok(mut current) = runtime.settings.lock() {
        *current = settings.clone();
    }
    sync_speed_enabled(runtime);
    runtime.apply_title(app);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        refresh_daily(app).await;
    });
}

/// 用量日志开关变化后清空实时请求，避免菜单栏残留旧速度。
pub fn usage_logging_changed(app: &AppHandle, enabled: bool) {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    runtime
        .usage_logging_enabled
        .store(enabled, Ordering::Release);
    sync_speed_enabled(runtime);
    if !enabled {
        runtime.apply_title(app);
        return;
    }
    runtime.apply_title(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        refresh_daily(app).await;
    });
}

/// 代理服务停止时只清空实时请求，不改变“是否记录用量”的设置。
pub fn clear_live_speed(app: &AppHandle) {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    runtime.speed.clear();
    runtime.apply_title(app);
}

/// 供 200ms 防抖后的用量事件调用。
pub fn notify_usage_logged(app: &AppHandle) {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    let daily_enabled = runtime
        .settings
        .lock()
        .map(|settings| menu_bar_daily_enabled(&settings))
        .unwrap_or(false);
    if !daily_enabled {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        refresh_daily(app).await;
    });
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
async fn run_ticker(app: AppHandle) {
    loop {
        tokio::time::sleep(SPEED_REFRESH).await;
        let Some(runtime) = RUNTIME.get() else {
            return;
        };
        let current_key = local_day_key();
        let day_changed = runtime
            .day_key
            .lock()
            .map(|key| *key != current_key)
            .unwrap_or(false);
        if day_changed {
            if let Ok(mut key) = runtime.day_key.lock() {
                *key = current_key;
            }
            if let Ok(mut daily) = runtime.daily.lock() {
                *daily = None;
            }
            runtime.apply_title(&app);
            refresh_daily(app.clone()).await;
        }
        runtime.apply_title(&app);
    }
}

async fn refresh_daily(app: AppHandle) {
    let Some(runtime) = RUNTIME.get() else {
        return;
    };
    let settings = match runtime.settings.lock() {
        Ok(settings) => settings.clone(),
        Err(_) => return,
    };
    if !menu_bar_daily_enabled(&settings) {
        if let Ok(mut daily) = runtime.daily.lock() {
            *daily = None;
        }
        runtime.apply_title(&app);
        return;
    }

    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let generation = runtime
        .daily_generation
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let db = state.db.clone();
    let start_date = local_day_start_timestamp();
    let end_date = Local::now().timestamp();
    let app_types = visible_app_types(&settings);
    let result = tauri::async_runtime::spawn_blocking(move || {
        db.get_usage_summary_for_app_types(Some(start_date), Some(end_date), &app_types)
    })
    .await;

    match result {
        Ok(Ok(summary)) => {
            if runtime.daily_generation.load(Ordering::Acquire) != generation {
                return;
            }
            if let Ok(mut daily) = runtime.daily.lock() {
                *daily = Some(DailySnapshot::from(summary));
            }
        }
        Ok(Err(error)) => {
            log::warn!("[MenuBar] 读取当天用量失败: {error}");
        }
        Err(error) => {
            log::warn!("[MenuBar] 当天用量查询任务失败: {error}");
        }
    }
    runtime.apply_title(&app);
}

impl MenuBarRuntime {
    fn apply_title(&self, app: &AppHandle) {
        let settings = match self.settings.lock() {
            Ok(settings) => settings.clone(),
            Err(_) => return,
        };
        let daily = self
            .daily
            .lock()
            .ok()
            .and_then(|daily| daily.clone())
            .unwrap_or_default();
        let title = build_title(&settings, self.speed.speed_at(Instant::now()), &daily);
        apply_tray_title(app, &self.applied_title, title);
    }
}

fn apply_tray_title(
    app: &AppHandle,
    applied_title: &Mutex<Option<Option<String>>>,
    title: Option<String>,
) {
    #[cfg(target_os = "macos")]
    {
        let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) else {
            return;
        };
        let unchanged = applied_title
            .lock()
            .map(|applied| applied.as_ref() == Some(&title))
            .unwrap_or(false);
        if unchanged {
            return;
        }
        // tray-icon 在 macOS 上把 None 当作“不修改标题”，必须显式写空串才能清空。
        let text = title.as_deref().unwrap_or("");
        if let Err(error) = tray.set_title(Some(text)) {
            log::warn!("[MenuBar] 更新菜单栏标题失败: {error}");
            return;
        }
        if let Ok(mut applied) = applied_title.lock() {
            *applied = Some(title);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, applied_title, title);
    }
}

fn build_title(settings: &AppSettings, speed: f64, daily: &DailySnapshot) -> Option<String> {
    let data = &settings.menu_bar_data;
    if !data.speed && !data.total_tokens && !data.requests && !data.cost && !data.cache_hit_rate {
        return None;
    }

    let language = settings.language.as_deref().unwrap_or("zh");
    let mut parts = Vec::new();
    if data.speed {
        parts.push(format_speed(speed, language));
    }
    if data.total_tokens {
        parts.push(format_tokens_compact(daily.total_tokens, language));
    }
    if data.requests {
        parts.push(format_grouped_u64(daily.total_requests, language));
    }
    if data.cost {
        parts.push(format_money_usd(
            &daily.total_cost_usd,
            2,
            settings.usage_display.as_ref(),
        ));
    }
    if data.cache_hit_rate {
        parts.push(format_hit_rate(daily.cache_hit_rate));
    }
    Some(parts.join(" · "))
}

fn format_speed(speed: f64, language: &str) -> String {
    if !speed.is_finite() || speed < 0.05 {
        return format_speed_value("0");
    }
    let rounded = speed.round().min(999.0);
    let value = if speed < 1.0 {
        format!("{speed:.1}")
    } else {
        format!("{rounded:.0}")
    };
    let value = if language.eq_ignore_ascii_case("tr") {
        value.replace('.', ",")
    } else {
        value
    };
    format_speed_value(&value)
}

fn format_speed_value(value: &str) -> String {
    let padding = SPEED_VALUE_WIDTH.saturating_sub(value.chars().count());
    format!(
        "{}{value} tok/s",
        std::iter::repeat_n(SPEED_PADDING, padding).collect::<String>()
    )
}

fn format_hit_rate(rate: f64) -> String {
    let percent = (rate * 100.0).clamp(0.0, 100.0);
    if percent >= 99.95 {
        "100%".to_string()
    } else {
        format!("{percent:.1}%")
    }
}

fn format_tokens_compact(value: u64, language: &str) -> String {
    if value < 10_000 {
        return format_grouped_u64(value, language);
    }
    let (size, suffix) = if value >= 1_000_000_000 {
        (1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (1_000_000.0, "M")
    } else {
        (1_000.0, "K")
    };
    let scaled = value as f64 / size;
    let precision = if scaled >= 100.0 {
        0
    } else if scaled >= 10.0 {
        1
    } else {
        2
    };
    let mut number = format!("{scaled:.precision$}");
    while number.contains('.') && number.ends_with('0') {
        number.pop();
    }
    if number.ends_with('.') {
        number.pop();
    }
    if language.eq_ignore_ascii_case("tr") {
        number = number.replace('.', ",");
    }
    format!("{number}{suffix}")
}

fn format_grouped_u64(value: u64, language: &str) -> String {
    let separator = if language.eq_ignore_ascii_case("tr") {
        '.'
    } else {
        ','
    };
    group_integer(&value.to_string(), separator)
}

fn group_integer(value: &str, separator: char) -> String {
    let mut grouped = String::with_capacity(value.len() + value.len() / 3);
    for (index, ch) in value.chars().enumerate() {
        if index > 0 && (value.len() - index) % 3 == 0 {
            grouped.push(separator);
        }
        grouped.push(ch);
    }
    grouped
}

fn format_money_usd(usd: &str, digits: usize, settings: Option<&UsageDisplaySettings>) -> String {
    let value = usd.parse::<f64>().unwrap_or(0.0);
    let (currency, rate) = resolve_currency(settings);
    let converted = value * rate;
    let symbol = match currency {
        "CNY" => "¥",
        "EUR" => "€",
        "JPY" => "¥",
        "TWD" => "NT$",
        "TRY" => "₺",
        _ => "$",
    };
    let fewer = if currency == "JPY" { 2 } else { 0 };
    let shown = digits.saturating_sub(fewer).min(2);
    let fixed = format!("{converted:.shown$}");
    if shown < digits && converted != 0.0 && fixed.parse::<f64>().unwrap_or(1.0) == 0.0 {
        return format!("{symbol}{}", format_significant(converted, 2));
    }
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let grouped = group_integer(whole, ',');
    if fraction.is_empty() {
        format!("{symbol}{grouped}")
    } else {
        format!("{symbol}{grouped}.{fraction}")
    }
}

fn format_significant(value: f64, significant: usize) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    let decimals = (significant as i32 - 1 - exponent).max(0) as usize;
    let mut text = format!("{value:.decimals$}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn resolve_currency(settings: Option<&UsageDisplaySettings>) -> (&'static str, f64) {
    let Some(settings) = settings else {
        return ("USD", 1.0);
    };
    let currency = settings
        .currency
        .as_deref()
        .filter(|currency| DISPLAY_CURRENCIES.contains(currency))
        .unwrap_or("USD");
    if currency == "USD" {
        return ("USD", 1.0);
    }
    let fallback = match currency {
        "CNY" => DEFAULT_CNY_RATE,
        "EUR" => DEFAULT_EUR_RATE,
        "JPY" => DEFAULT_JPY_RATE,
        "TWD" => DEFAULT_TWD_RATE,
        "TRY" => DEFAULT_TRY_RATE,
        _ => 1.0,
    };
    let rate = settings
        .exchange_rates
        .get(currency)
        .copied()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .unwrap_or(fallback);
    (
        DISPLAY_CURRENCIES
            .into_iter()
            .find(|known| *known == currency)
            .unwrap_or("USD"),
        rate,
    )
}

fn menu_bar_daily_enabled(settings: &AppSettings) -> bool {
    let data = &settings.menu_bar_data;
    data.total_tokens || data.requests || data.cost || data.cache_hit_rate
}

fn sync_speed_enabled(runtime: &MenuBarRuntime) {
    let menu_enabled = runtime
        .settings
        .lock()
        .map(|settings| settings.menu_bar_data.speed)
        .unwrap_or(false);
    runtime.speed.set_enabled(
        cfg!(target_os = "macos")
            && menu_enabled
            && runtime.usage_logging_enabled.load(Ordering::Acquire),
    );
}

fn visible_app_types(settings: &AppSettings) -> Vec<String> {
    // 前端未保存过 visibleApps 时默认所有应用可见；后端 VisibleApps::default()
    // 为了旧版迁移让 Hermes 默认关闭，菜单栏需要跟当前 Apps 页面保持一致。
    let visible = settings
        .visible_apps
        .clone()
        .unwrap_or(crate::settings::VisibleApps {
            claude: true,
            claude_desktop: true,
            codex: true,
            gemini: true,
            grokbuild: true,
            opencode: true,
            openclaw: true,
            hermes: true,
            pi: true,
            mcode: true,
        });
    AppType::all()
        .filter(|app| visible.is_visible(app))
        .map(|app| app.as_str().to_string())
        .collect()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn local_day_key() -> String {
    Local::now().format("%Y-%m-%d%:z").to_string()
}

fn local_day_start_timestamp() -> i64 {
    let now = Local::now();
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .unwrap_or(now)
        .timestamp()
}

#[derive(Debug, Clone)]
struct SpeedSample {
    at: Instant,
    tokens: f64,
}

#[derive(Debug)]
struct RequestSpeed {
    samples: VecDeque<SpeedSample>,
    first_output_at: Option<Instant>,
    count_mode: Option<SpeedCountMode>,
    last_exact_output: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpeedCountMode {
    Approximate,
    Exact,
}

impl RequestSpeed {
    fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            first_output_at: None,
            count_mode: None,
            last_exact_output: 0,
        }
    }

    fn add_approximate(&mut self, at: Instant, tokens: f64) {
        if tokens <= 0.0 {
            return;
        }
        if self.count_mode == Some(SpeedCountMode::Exact) {
            return;
        }
        self.count_mode = Some(SpeedCountMode::Approximate);
        self.first_output_at.get_or_insert(at);
        self.samples.push_back(SpeedSample { at, tokens });
    }

    fn add_exact(&mut self, at: Instant, output_tokens: u64) {
        if output_tokens == 0 {
            return;
        }
        if self.count_mode == Some(SpeedCountMode::Approximate) {
            return;
        }
        self.count_mode = Some(SpeedCountMode::Exact);
        self.first_output_at.get_or_insert(at);
        if output_tokens <= self.last_exact_output {
            return;
        }
        self.samples.push_back(SpeedSample {
            at,
            tokens: (output_tokens - self.last_exact_output) as f64,
        });
        self.last_exact_output = output_tokens;
    }

    fn prune(&mut self, cutoff: Instant) {
        while self
            .samples
            .front()
            .is_some_and(|sample| sample.at < cutoff)
        {
            self.samples.pop_front();
        }
    }

    fn token_sum(&self) -> f64 {
        self.samples.iter().map(|sample| sample.tokens).sum()
    }
}

#[derive(Debug, Default)]
struct LiveSpeedState {
    requests: HashMap<u64, RequestSpeed>,
    active_since: Option<Instant>,
}

#[derive(Debug)]
struct LiveSpeedRegistry {
    enabled: AtomicBool,
    next_id: AtomicU64,
    state: Mutex<LiveSpeedState>,
}

impl LiveSpeedRegistry {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            next_id: AtomicU64::new(1),
            state: Mutex::new(LiveSpeedState::default()),
        }
    }

    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            self.clear();
        }
    }

    fn clear(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.requests.clear();
            state.active_since = None;
        }
    }

    fn start_request(self: &Arc<Self>) -> Option<LiveSpeedGuard> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            state.requests.insert(id, RequestSpeed::new());
        } else {
            return None;
        }
        Some(LiveSpeedGuard {
            registry: self.clone(),
            id,
            active: true,
        })
    }

    fn observe(&self, id: u64, event: &Value, at: Instant) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let delta = extract_live_delta(event);
        if delta.text_tokens == 0.0 && delta.exact_output_tokens.is_none() {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let became_active = {
            let request = state.requests.entry(id).or_insert_with(RequestSpeed::new);
            let had_output = request.first_output_at.is_some();
            if delta.text_tokens > 0.0 {
                request.add_approximate(at, delta.text_tokens);
            }
            if let Some(exact) = delta.exact_output_tokens {
                request.add_exact(at, exact);
            }
            !had_output && request.first_output_at.is_some()
        };
        if became_active {
            state.active_since.get_or_insert(at);
        }
    }

    fn finish(&self, id: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.requests.remove(&id);
            if state.requests.is_empty() {
                state.active_since = None;
            }
        }
    }

    fn speed_at(&self, now: Instant) -> f64 {
        if !self.enabled.load(Ordering::Acquire) {
            return 0.0;
        }
        let Ok(mut state) = self.state.lock() else {
            return 0.0;
        };
        if state.requests.is_empty() {
            return 0.0;
        }
        let cutoff = now.checked_sub(SPEED_WINDOW).unwrap_or(now);
        let mut total = 0.0;
        for request in state.requests.values_mut() {
            request.prune(cutoff);
            total += request.token_sum();
        }
        let Some(active_since) = state.active_since else {
            return 0.0;
        };
        let elapsed = now
            .saturating_duration_since(active_since)
            .min(SPEED_WINDOW)
            .max(SPEED_MIN_DENOMINATOR);
        (total.max(0.0) / elapsed.as_secs_f64()).max(0.0)
    }

    #[cfg(test)]
    fn add_approximate_for_test(&self, id: u64, at: Instant, text: &str) {
        if let Ok(mut state) = self.state.lock() {
            let became_active = state.requests.get_mut(&id).is_some_and(|request| {
                let had_output = request.first_output_at.is_some();
                request.add_approximate(at, estimate_text_tokens(text));
                !had_output && request.first_output_at.is_some()
            });
            if became_active {
                state.active_since.get_or_insert(at);
            }
        }
    }

    #[cfg(test)]
    fn add_exact_for_test(&self, id: u64, at: Instant, tokens: u64) {
        if let Ok(mut state) = self.state.lock() {
            let became_active = state.requests.get_mut(&id).is_some_and(|request| {
                let had_output = request.first_output_at.is_some();
                request.add_exact(at, tokens);
                !had_output && request.first_output_at.is_some()
            });
            if became_active {
                state.active_since.get_or_insert(at);
            }
        }
    }
}

pub(crate) struct LiveSpeedGuard {
    registry: Arc<LiveSpeedRegistry>,
    id: u64,
    active: bool,
}

impl LiveSpeedGuard {
    pub(crate) fn is_enabled(&self) -> bool {
        self.active && self.registry.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn observe_json(&self, event: &Value) {
        if self.is_enabled() {
            self.registry.observe(self.id, event, Instant::now());
        }
    }

    pub(crate) fn finish(mut self) {
        self.active = false;
        self.registry.finish(self.id);
    }
}

impl Drop for LiveSpeedGuard {
    fn drop(&mut self) {
        if self.active {
            self.registry.finish(self.id);
        }
    }
}

pub(crate) fn start_live_request() -> Option<LiveSpeedGuard> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    RUNTIME.get()?.speed.start_request()
}

#[derive(Default)]
struct LiveDelta {
    text_tokens: f64,
    exact_output_tokens: Option<u64>,
}

impl LiveDelta {
    fn add_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text_tokens += estimate_text_tokens(text);
    }
}

fn estimate_text_tokens(text: &str) -> f64 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 0.25 } else { 1.0 })
        .sum()
}

fn extract_live_delta(event: &Value) -> LiveDelta {
    let mut delta = LiveDelta::default();
    let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");

    if event_type == "content_block_delta" {
        if let Some(block_delta) = event.get("delta") {
            add_string_field(&mut delta, block_delta.get("text"));
            add_string_field(&mut delta, block_delta.get("thinking"));
            add_string_field(&mut delta, block_delta.get("partial_json"));
        }
    }

    if matches!(
        event_type,
        "response.output_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning.delta"
            | "response.function_call_arguments.delta"
            | "response.custom_tool_call_input.delta"
    ) {
        add_string_field(&mut delta, event.get("delta"));
    }

    if let Some(choices) = event.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let Some(choice_delta) = choice.get("delta") else {
                continue;
            };
            add_string_field(&mut delta, choice_delta.get("content"));
            add_string_field(&mut delta, choice_delta.get("reasoning_content"));
            add_string_field(&mut delta, choice_delta.get("reasoning"));
            if let Some(details) = choice_delta
                .get("reasoning_details")
                .and_then(Value::as_array)
            {
                for detail in details {
                    add_string_field(&mut delta, detail.get("text"));
                }
            }
            if let Some(tool_calls) = choice_delta.get("tool_calls").and_then(Value::as_array) {
                for call in tool_calls {
                    add_string_field(&mut delta, call.pointer("/function/arguments"));
                }
            }
            add_string_field(&mut delta, choice_delta.pointer("/function_call/arguments"));
        }
    }

    if let Some(candidates) = event.get("candidates").and_then(Value::as_array) {
        for candidate in candidates {
            let Some(parts) = candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
            else {
                continue;
            };
            for part in parts {
                add_string_field(&mut delta, part.get("text"));
                if let Some(args) = part.pointer("/functionCall/args") {
                    if let Ok(serialized) = serde_json::to_string(args) {
                        delta.add_text(&serialized);
                    }
                }
            }
        }
    }

    // Anthropic's message_start carries an initial output_tokens value (often 1)
    // before any output delta. Treating it as a cumulative total locks the
    // request to exact mode and ignores the streamed text that follows.
    if event_type != "message_start" {
        delta.exact_output_tokens = event
            .pointer("/message/usage/output_tokens")
            .and_then(Value::as_u64)
            .or_else(|| {
                event
                    .pointer("/response/usage/output_tokens")
                    .and_then(Value::as_u64)
            })
            .or_else(|| {
                event
                    .pointer("/usage/output_tokens")
                    .and_then(Value::as_u64)
            })
            .or_else(|| {
                event
                    .pointer("/usage/completion_tokens")
                    .and_then(Value::as_u64)
            })
            .or_else(|| gemini_output_tokens(event));
    }

    delta
}

fn add_string_field(delta: &mut LiveDelta, value: Option<&Value>) {
    if let Some(text) = value.and_then(Value::as_str) {
        delta.add_text(text);
    }
}

fn gemini_output_tokens(event: &Value) -> Option<u64> {
    let usage = event.get("usageMetadata")?;
    let prompt = usage
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if let Some(total) = usage.get("totalTokenCount").and_then(Value::as_u64) {
        return Some(total.saturating_sub(prompt));
    }
    let candidates = usage
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let thoughts = usage
        .get("thoughtsTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (candidates + thoughts > 0).then_some(candidates + thoughts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::MenuBarDataSettings;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn settings_with_data(data: MenuBarDataSettings) -> AppSettings {
        AppSettings {
            language: Some("en".to_string()),
            menu_bar_data: data,
            ..AppSettings::default()
        }
    }

    #[test]
    fn title_orders_enabled_values_and_omits_disabled_ones() {
        let settings = settings_with_data(MenuBarDataSettings {
            speed: true,
            total_tokens: true,
            requests: true,
            cost: true,
            cache_hit_rate: true,
        });
        let daily = DailySnapshot {
            total_tokens: 128_000,
            total_requests: 37,
            total_cost_usd: "0.42".to_string(),
            cache_hit_rate: 0.86,
        };
        assert_eq!(
            build_title(&settings, 42.4, &daily)
                .as_deref()
                .map(|title| title.replace(SPEED_PADDING, ""))
                .as_deref(),
            Some("42 tok/s · 128K · 37 · $0.42 · 86.0%")
        );
    }

    #[test]
    fn title_all_off_is_none_and_idle_speed_is_zero() {
        let settings = settings_with_data(MenuBarDataSettings {
            speed: false,
            total_tokens: false,
            requests: false,
            cost: false,
            cache_hit_rate: false,
        });
        assert_eq!(
            build_title(&settings, 10.0, &DailySnapshot::default()),
            None
        );

        let settings = settings_with_data(MenuBarDataSettings::default());
        assert_eq!(
            build_title(&settings, 0.0, &DailySnapshot::default())
                .as_deref()
                .map(|title| title.replace(SPEED_PADDING, ""))
                .as_deref(),
            Some("0 tok/s")
        );
    }

    #[test]
    fn low_speed_has_one_decimal() {
        assert_eq!(
            format_speed(0.42, "en").replace(SPEED_PADDING, ""),
            "0.4 tok/s"
        );
    }

    #[test]
    fn speed_value_width_stays_fixed_across_magnitudes() {
        for speed in [0.0, 0.4, 42.0, 999.0, 999.6, 12_400.0] {
            let title = format_speed(speed, "en");
            let value = title.strip_suffix(" tok/s").expect("speed title has unit");
            assert_eq!(value.chars().count(), SPEED_VALUE_WIDTH, "title={title:?}");
        }

        assert_eq!(
            format_speed(999.6, "en").replace(SPEED_PADDING, ""),
            "999 tok/s"
        );
        assert_eq!(
            format_speed(12_400.0, "en").replace(SPEED_PADDING, ""),
            "999 tok/s"
        );
    }

    #[test]
    fn live_window_uses_actual_elapsed_and_aggregates_requests() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let first = registry.start_request().expect("first request");
        let second = registry.start_request().expect("second request");
        let start = Instant::now();
        registry.add_approximate_for_test(first.id, start, "aaaa");
        registry.add_approximate_for_test(second.id, start, "bbbb");
        let speed = registry.speed_at(start + Duration::from_secs(1));
        assert!((speed - 2.0).abs() < 0.01, "speed={speed}");
        first.finish();
        second.finish();
        assert_eq!(registry.speed_at(start + Duration::from_secs(1)), 0.0);
    }

    #[test]
    fn exact_usage_is_used_when_the_stream_has_no_text_deltas() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let guard = registry.start_request().expect("request");
        let start = Instant::now();
        registry.add_exact_for_test(guard.id, start, 4);
        registry.add_exact_for_test(guard.id, start, 4);
        let speed = registry.speed_at(start + Duration::from_secs(1));
        assert!((speed - 4.0).abs() < 0.01, "speed={speed}");
    }

    #[test]
    fn text_estimate_and_cumulative_usage_do_not_double_count() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let guard = registry.start_request().expect("request");
        let start = Instant::now();
        registry.add_approximate_for_test(guard.id, start, "aaaaaaaa");
        registry.add_exact_for_test(guard.id, start, 99);
        let speed = registry.speed_at(start + Duration::from_secs(1));
        assert!((speed - 2.0).abs() < 0.01, "speed={speed}");
    }

    #[test]
    fn live_window_drops_samples_after_five_seconds() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let guard = registry.start_request().expect("request");
        let start = Instant::now();
        registry.add_approximate_for_test(guard.id, start, "aaaaaaaa");
        assert!(registry.speed_at(start + Duration::from_secs(1)) > 0.0);
        assert_eq!(registry.speed_at(start + Duration::from_secs(6)), 0.0);
    }

    #[test]
    fn cost_uses_usage_currency_and_exchange_rate() {
        let settings = UsageDisplaySettings {
            currency: Some("CNY".to_string()),
            exchange_rates: BTreeMap::from([("CNY".to_string(), 7.2)]),
            ..UsageDisplaySettings::default()
        };
        assert_eq!(format_money_usd("0.42", 2, Some(&settings)), "¥3.02");
    }

    #[test]
    fn extracts_reasoning_tool_and_exact_usage() {
        let anthropic = json!({
            "type": "content_block_delta",
            "delta": {"type": "thinking_delta", "thinking": "abc"}
        });
        let delta = extract_live_delta(&anthropic);
        assert!((delta.text_tokens - 0.75).abs() < f64::EPSILON);

        let chat = json!({
            "choices": [{"delta": {"tool_calls": [{"function": {"arguments": "{\"a\":1}"}}]}}],
            "usage": {"completion_tokens": 12}
        });
        let delta = extract_live_delta(&chat);
        assert!(delta.text_tokens > 0.0);
        assert_eq!(delta.exact_output_tokens, Some(12));
    }

    #[test]
    fn anthropic_message_start_does_not_block_live_text_speed() {
        let message_start = json!({
            "type": "message_start",
            "message": {
                "usage": {"output_tokens": 1}
            }
        });
        assert_eq!(extract_live_delta(&message_start).exact_output_tokens, None);

        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let guard = registry.start_request().expect("request");
        let start = Instant::now();
        for index in 0..40 {
            registry.add_approximate_for_test(
                guard.id,
                start + Duration::from_millis(index * 50),
                "hello hello",
            );
        }
        let speed = registry.speed_at(start + Duration::from_secs(2));
        assert!(speed > 10.0, "speed={speed}");
    }

    #[test]
    fn extracts_responses_and_gemini_output_without_counting_done_snapshots() {
        let responses = json!({
            "type": "response.output_text.delta",
            "delta": "hello"
        });
        assert_eq!(extract_live_delta(&responses).text_tokens, 1.25);

        let done = json!({
            "type": "response.output_text.done",
            "text": "hello hello hello"
        });
        assert_eq!(extract_live_delta(&done).text_tokens, 0.0);

        let gemini = json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "回答"}, {"functionCall": {"args": {"path": "a"}}}]
                }
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 7,
                "thoughtsTokenCount": 3
            }
        });
        let delta = extract_live_delta(&gemini);
        assert!(delta.text_tokens > 0.0);
        assert_eq!(delta.exact_output_tokens, Some(10));
    }

    #[test]
    fn disabling_speed_clears_active_requests() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let guard = registry.start_request().expect("request");
        registry.add_approximate_for_test(guard.id, Instant::now(), "aaaaaaaa");
        registry.set_enabled(false);
        assert_eq!(registry.speed_at(Instant::now()), 0.0);
    }

    #[test]
    fn finishing_an_old_request_does_not_shrink_the_active_window() {
        let registry = Arc::new(LiveSpeedRegistry::new(true));
        let first = registry.start_request().expect("first request");
        let second = registry.start_request().expect("second request");
        let start = Instant::now();
        registry.add_approximate_for_test(first.id, start, "aaaaaaaa");
        let second_start = start + Duration::from_secs(10);
        registry.add_approximate_for_test(second.id, second_start, "bbbbbbbb");
        first.finish();

        let speed = registry.speed_at(second_start + Duration::from_secs(1));
        assert!((speed - 0.4).abs() < 0.01, "speed={speed}");
    }

    #[test]
    fn reenabling_speed_resumes_an_existing_guard() {
        let registry = Arc::new(LiveSpeedRegistry::new(false));
        let guard = registry.start_request().expect("request");
        registry.set_enabled(true);
        guard.observe_json(&json!({
            "type": "response.output_text.delta",
            "delta": "aaaaaaaa"
        }));
        assert!(registry.speed_at(Instant::now()) > 0.0);
    }
}
