use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_mini::notify;
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use serde_json::Value;

use crate::error::AppError;
use crate::provider::Provider;

/// watcher 更新 autoSyncState 后把 provider 配置持久化到 DB 的回调。
pub(crate) type PersistSettingsCallback = Arc<dyn Fn(Value) -> Result<(), String> + Send + Sync>;

/// 监听 settings.json 后决定的当前激活模型窗口
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveModelWindow {
    pub model: String,
    pub window: u64,
}

/// watcher 防循环用的文件快照：model 或 ACW/MAX 任一变化都视为需要处理。
#[derive(Debug, Clone, PartialEq, Eq)]
struct WatcherSnapshot {
    model: Option<String>,
    acw: Option<String>,
    max: Option<String>,
}

/// 根据 settings.json 顶层 model 字段和 provider 配置，
/// 决定要写入 ACW/MAX 的窗口值。
///
/// 返回 None 表示"不写"（model 字段无效 / 角色无可用窗口 / 无固定兜底窗口）。
pub(crate) fn resolve_active_model_window(
    settings: &Value,
    provider: &Provider,
) -> Option<ActiveModelWindow> {
    // 1. 读顶层 model 字段
    let model = settings.get("model").and_then(Value::as_str)?;
    // 2. 映射到 env 字段名
    let env_key = match model {
        "sonnet" => "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "opus" => "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "fable" => "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "haiku" => "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "subagent" => "CLAUDE_CODE_SUBAGENT_MODEL",
        _ => return None,
    };
    // 3. 优先读取 contextWindows 显式窗口，其次回退到 env 模型名后缀；
    // 两者都没有时，保留 Codex OAuth / Kimi 的固定兜底窗口。
    let window =
        crate::claude_desktop_config::resolve_context_window(&provider.settings_config, env_key)
            .or_else(|| {
                crate::services::provider::static_context_window_fallback(provider)
                    .and_then(|(acw, _max)| acw.parse().ok())
            });
    window.map(|w| ActiveModelWindow {
        model: model.to_string(),
        window: w,
    })
}

/// 读取 provider 的自动压缩比例，缺失或非法时默认 0.95。
pub(crate) fn provider_compact_ratio(provider: &Provider) -> f64 {
    provider
        .settings_config
        .get("autoSyncCompactRatio")
        .and_then(Value::as_f64)
        .filter(|ratio| ratio.is_finite() && (0.2..=0.95).contains(ratio))
        .unwrap_or(0.95)
}

/// 根据窗口值和压缩比例生成要写入 settings.json.env 的两个 env 项。
/// ACW = 窗口 × ratio（向下取整），MAX = 窗口本身。
pub(crate) fn build_env_writes(window: u64, ratio: f64) -> Vec<(&'static str, String)> {
    let ratio = if ratio.is_finite() && (0.2..=0.95).contains(&ratio) {
        ratio
    } else {
        0.95
    };
    let acw = ((window as f64) * ratio).floor() as u64;
    vec![
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw.to_string()),
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", window.to_string()),
    ]
}

/// 检查新事件的 model + ACW/MAX 是否与上次快照不同。
/// 这里只声明候选，不修改 state；只有写入/持久化成功路径才提交快照。
fn should_process(
    state: &Mutex<Option<WatcherSnapshot>>,
    new_model: Option<&str>,
    new_acw: Option<&str>,
    new_max: Option<&str>,
) -> bool {
    let guard = state.lock().expect("settings watcher mutex poisoned");
    let next = WatcherSnapshot {
        model: new_model.map(|s| s.to_string()),
        acw: new_acw.map(|s| s.to_string()),
        max: new_max.map(|s| s.to_string()),
    };
    *guard != Some(next)
}

fn record_processed_state(
    state: &Mutex<Option<WatcherSnapshot>>,
    model: Option<&str>,
    acw: Option<&str>,
    max: Option<&str>,
) {
    *state.lock().expect("settings watcher mutex poisoned") = Some(WatcherSnapshot {
        model: model.map(|s| s.to_string()),
        acw: acw.map(|s| s.to_string()),
        max: max.map(|s| s.to_string()),
    });
}

/// Claude Code settings.json 监听器
///
/// 后台线程监听文件变化，根据顶层 model 字段值变化自动同步 ACW/MAX。
pub struct ClaudeSettingsWatcher {
    /// 防循环用的"上次见到的 model + ACW/MAX 快照"
    #[allow(dead_code)]
    state: Arc<Mutex<Option<WatcherSnapshot>>>,
    /// 关闭信号
    shutdown: Arc<AtomicBool>,
    /// notify debouncer handle（Drop 时自动停止监听）
    _debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
    /// 测试专用：保留传入的 provider 快照，便于断言内部同步字段。
    #[cfg(test)]
    provider: Arc<Mutex<Provider>>,
}

impl Drop for ClaudeSettingsWatcher {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }
}

/// 启动 settings.json 监听器
///
/// 返回的 watcher 在 Drop 时自动停止监听。
pub(crate) fn spawn_claude_settings_watcher(
    settings_path: PathBuf,
    provider: Arc<Mutex<Provider>>,
    persist: PersistSettingsCallback,
) -> Result<ClaudeSettingsWatcher, AppError> {
    let state = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(AtomicBool::new(false));

    // 启动时读一次 settings.json 初始化 state
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            *state.lock().unwrap() = Some(WatcherSnapshot {
                model: v.get("model").and_then(Value::as_str).map(String::from),
                acw: v
                    .pointer("/env/CLAUDE_CODE_AUTO_COMPACT_WINDOW")
                    .and_then(Value::as_str)
                    .map(String::from),
                max: v
                    .pointer("/env/CLAUDE_CODE_MAX_CONTEXT_TOKENS")
                    .and_then(Value::as_str)
                    .map(String::from),
            });
        }
    }

    let state_clone = state.clone();
    let shutdown_clone = shutdown.clone();
    let provider_clone = provider.clone();
    let path_clone = settings_path.clone();
    let persist_clone = persist.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        move |result: DebounceEventResult| {
            if shutdown_clone.load(Ordering::SeqCst) {
                return;
            }
            let events = match result {
                Ok(events) => events,
                Err(errors) => {
                    log::warn!("[ClaudeSettingsWatcher] debounce error: {errors}");
                    return;
                }
            };
            // watch 父目录会收到目录下所有文件事件。任意事件都读一次
            // settings.json 检查 model 字段，should_process 会跳过 model
            // 没变的情况，避免不必要的写入。不过滤 event.path 是因为某些
            // 平台（如 macOS FSEvents）可能报告目录级别事件，file_name
            // 过滤会漏掉。
            for _event in events {
                handle_settings_change(
                    &path_clone,
                    provider_clone.as_ref(),
                    &state_clone,
                    &persist_clone,
                );
            }
        },
    )
    .map_err(|e| AppError::Message(format!("failed to create settings watcher: {e}")))?;

    // watch 父目录而不是文件本身：atomic_write 用 rename 覆盖文件，
    // 在 inotify（Linux）上文件 watch 附加旧 inode，rename 后失效；
    // watch 父目录能持续观察文件替换 + 创建（fresh 安装时文件还不存在）。
    let watch_dir = settings_path
        .parent()
        .ok_or_else(|| AppError::Message("settings.json has no parent directory".to_string()))?;
    debouncer
        .watcher()
        .watch(watch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| AppError::Message(format!("failed to watch settings.json dir: {e}")))?;

    Ok(ClaudeSettingsWatcher {
        state,
        shutdown,
        _debouncer: Some(debouncer),
        #[cfg(test)]
        provider,
    })
}

/// 进程级单例 slot，持有当前存活的 watcher。
///
/// production 路径下 spawn 出来的 watcher 必须存到这里，否则函数返回时
/// 返回值被 Drop，notify 监听线程随之退出--这正是 dev 测试里 /model 切换
/// 不更新 ACW/MAX 的根因。
static WATCHER_SLOT: OnceLock<Mutex<Option<ClaudeSettingsWatcher>>> = OnceLock::new();

fn watcher_slot() -> &'static Mutex<Option<ClaudeSettingsWatcher>> {
    WATCHER_SLOT.get_or_init(|| Mutex::new(None))
}

/// 把新 watcher 存进进程级单例，旧的自动 Drop（停止监听）。
///
/// 调用方不需要持有返回的 watcher--这正是 production 路径需要的语义：
/// spawn_claude_settings_watcher 的 Ok 返回值交给 replace_watcher，
/// 由静态 slot 接管所有权，watcher 才能存活到进程退出或下次替换。
pub fn replace_watcher(new: ClaudeSettingsWatcher) {
    let mut guard = watcher_slot().lock().expect("watcher slot mutex poisoned");
    // 旧 watcher 在赋值时自动 Drop：Drop 设 shutdown=true 并 drop debouncer，
    // notify 监听线程随之停止。新 watcher 接管监听。
    *guard = Some(new);
}

#[cfg(test)]
pub(crate) fn clear_watcher_slot_for_tests() {
    *watcher_slot().lock().expect("watcher slot mutex poisoned") = None;
}

#[cfg(test)]
pub(crate) fn watcher_slot_is_empty_for_tests() -> bool {
    watcher_slot()
        .lock()
        .expect("watcher slot mutex poisoned")
        .is_none()
}

#[cfg(test)]
pub(crate) fn watcher_provider_settings_config_for_tests() -> Option<Value> {
    let slot = watcher_slot().lock().expect("watcher slot mutex poisoned");
    let watcher = slot.as_ref()?;
    let settings_config = watcher
        .provider
        .lock()
        .expect("watcher provider mutex poisoned")
        .settings_config
        .clone();
    Some(settings_config)
}

/// 处理一次 settings.json 变化
fn handle_settings_change(
    path: &std::path::Path,
    provider: &Mutex<Provider>,
    state: &Mutex<Option<WatcherSnapshot>>,
    persist: &PersistSettingsCallback,
) {
    // 1. 读最新内容
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log::debug!("[ClaudeSettingsWatcher] read failed: {e}");
            return;
        }
    };
    let v: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] invalid JSON: {e}");
            return;
        }
    };

    // 直连与路由模式共用同一套自动同步：/model 切换后按激活角色写 MAX/ACW 并记
    // lastWritten（round-8 曾按路由模式门控，后经实机验证直连同样生效，已解冻）。
    let new_model = v.get("model").and_then(Value::as_str);
    let new_acw = v
        .pointer("/env/CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        .and_then(Value::as_str);
    let new_max = v
        .pointer("/env/CLAUDE_CODE_MAX_CONTEXT_TOKENS")
        .and_then(Value::as_str);

    // 3. 检查 model / ACW / MAX 任一变化（防循环）
    if !should_process(state, new_model, new_acw, new_max) {
        return;
    }

    let mut provider = provider
        .lock()
        .expect("settings watcher provider mutex poisoned");

    // 4. 检查 provider 的 autoSyncContextWindow 开关（字段缺失即关闭，spec：缺失该开关时默认关闭）
    if !effective_auto_sync_enabled(&provider) {
        log::debug!("[ClaudeSettingsWatcher] auto-sync disabled for provider, skip");
        return;
    }

    // 5. 只有静态注入实际生效的路径需要跳过：Kimi 固定注入，或 Codex OAuth 的
    // env 模型全部指向 gpt-5.6。其他 Codex OAuth 配置仍按 contextWindows 增强。
    let static_injection_active =
        crate::services::provider::static_context_window_fallback(&provider).is_some();
    if static_injection_active {
        log::debug!(
            "[ClaudeSettingsWatcher] static ACW/MAX for {}, skip watcher rewrite",
            provider.name
        );
        return;
    }

    // 6. 决定要写的窗口值
    let active = match resolve_active_model_window(&v, &provider) {
        Some(a) => a,
        None => {
            log::debug!("[ClaudeSettingsWatcher] no active model window to write");
            return;
        }
    };

    // 6. 生成 ACW/MAX 并做写前二次校验，避免覆盖并发修改。
    let writes = build_env_writes(active.window, provider_compact_ratio(&provider));
    let new_content = match update_env_fields(&content, &writes) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] update failed: {e}");
            return;
        }
    };
    if let Err(e) = verify_file_unchanged(path, &content) {
        log::warn!("[ClaudeSettingsWatcher] concurrent change, skip write: {e}");
        return;
    }

    // 使用原子写入（临时文件 + rename），避免 Claude Code 在写入期间
    // 读到截断后的空文件或残缺 JSON；写入成功后才更新 lastWritten。
    if let Err(e) = crate::config::atomic_write(path, new_content.as_bytes()) {
        log::warn!("[ClaudeSettingsWatcher] write failed: {e}");
    } else {
        record_last_written(&mut provider, &writes[0].1, &writes[1].1);
        record_processed_state(
            state,
            Some(active.model.as_str()),
            Some(&writes[0].1),
            Some(&writes[1].1),
        );
        // live 已写入后若 DB 持久化失败，不能回滚到与文件不一致的账本/state；
        // 保留新值等下次变化时重试，避免 rename 事件把自动写入误判成用户手改。
        if persist_settings(persist, &provider) {
            log::info!(
                "[ClaudeSettingsWatcher] wrote ACW/MAX for model={} window={}",
                active.model,
                active.window
            );
        } else {
            log::warn!(
                "[ClaudeSettingsWatcher] persist failed after live write; keeping ACW/MAX for retry"
            );
        }
    }
}

fn persist_settings(persist: &PersistSettingsCallback, provider: &Provider) -> bool {
    match persist(provider.settings_config.clone()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] failed to persist autoSyncState: {e}");
            false
        }
    }
}

fn record_last_written(provider: &mut Provider, acw: &str, max: &str) {
    if let Some(state) =
        crate::services::provider::auto_sync_state_mut(&mut provider.settings_config)
    {
        state.insert(
            "lastWritten".to_string(),
            serde_json::json!({ "ACW": acw, "MAX": max }),
        );
    }
}

/// autoSyncContextWindow 开关有效值：显式字段优先，缺失即关闭（spec：缺失该开关时默认关闭）。
pub(crate) fn effective_auto_sync_enabled(provider: &Provider) -> bool {
    provider
        .settings_config
        .get("autoSyncContextWindow")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn verify_file_unchanged(path: &std::path::Path, expected: &str) -> Result<(), String> {
    let current = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if current == expected {
        Ok(())
    } else {
        Err("settings.json changed between read and write".to_string())
    }
}

/// 原子地更新 settings.json 中 env 子对象的指定字段，其他字段全部保留
fn update_env_fields(content: &str, writes: &[(&'static str, String)]) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("top-level not object".to_string());
    }
    // cc-switch 内部字段（contextWindows / autoSyncState 等）不能写回 settings.json。
    v = crate::services::provider::sanitize_claude_settings_for_live(&v);
    let obj = v.as_object_mut().unwrap();
    let env = obj
        .entry("env".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !env.is_object() {
        return Err("env not object".to_string());
    }
    let env_obj = env.as_object_mut().unwrap();
    for (key, value) in writes {
        env_obj.insert((*key).to_string(), Value::String(value.clone()));
    }
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests;
