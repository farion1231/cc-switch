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

/// 监听 settings.json 后决定的当前激活模型窗口
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveModelWindow {
    pub model: String,
    pub window: u64,
}

/// 根据 settings.json 顶层 model 字段和 provider env 配置，
/// 决定要写入 ACW/MAX 的窗口值。
///
/// 返回 None 表示"不写"（model 字段无效 / 角色对应 env 不存在 / env 无后缀）。
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
    // 3. 读 provider env 里对应字段
    let env_value = provider
        .settings_config
        .get("env")
        .and_then(|e| e.get(env_key))
        .and_then(Value::as_str)?;
    // 4. 解析后缀得到窗口
    let (_, window) = crate::claude_desktop_config::parse_context_window_suffix(env_value);
    window.map(|w| ActiveModelWindow {
        model: model.to_string(),
        window: w,
    })
}

/// 根据窗口值生成要写入 settings.json.env 的两个 env 项。
/// ACW = 窗口 × 0.8（向下取整），MAX = 窗口本身。
pub(crate) fn build_env_writes(window: u64) -> Vec<(&'static str, String)> {
    let acw = (window * 80) / 100;
    vec![
        ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", acw.to_string()),
        ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", window.to_string()),
    ]
}

/// 检查新事件的 model 字段是否需要处理（与上次不同则处理）。
/// 这是监听器防循环的核心逻辑：自己写 env 不改 model 字段 → 自动跳过。
pub(crate) fn should_process(state: &Mutex<Option<String>>, new_model: Option<&str>) -> bool {
    let mut guard = state.lock().expect("settings watcher mutex poisoned");
    let new_model_owned = new_model.map(|s| s.to_string());
    if *guard == new_model_owned {
        return false;
    }
    *guard = new_model_owned;
    true
}

/// Claude Code settings.json 监听器
///
/// 后台线程监听文件变化，根据顶层 model 字段值变化自动同步 ACW/MAX。
pub struct ClaudeSettingsWatcher {
    /// 防循环用的"上次见到的 model 字段值"
    #[allow(dead_code)]
    state: Arc<Mutex<Option<String>>>,
    /// 关闭信号
    shutdown: Arc<AtomicBool>,
    /// notify debouncer handle（Drop 时自动停止监听）
    _debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
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
    provider: Arc<Provider>,
) -> Result<ClaudeSettingsWatcher, AppError> {
    let state = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(AtomicBool::new(false));

    // 启动时读一次 settings.json 初始化 state
    if let Ok(content) = std::fs::read_to_string(&settings_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&content) {
            *state.lock().unwrap() = v.get("model").and_then(Value::as_str).map(String::from);
        }
    }

    let state_clone = state.clone();
    let shutdown_clone = shutdown.clone();
    let provider_clone = provider.clone();
    let path_clone = settings_path.clone();

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
            // 任意一个事件触发就处理
            for _event in events {
                handle_settings_change(&path_clone, &provider_clone, &state_clone);
            }
        },
    )
    .map_err(|e| AppError::Message(format!("failed to create settings watcher: {e}")))?;

    debouncer
        .watcher()
        .watch(&settings_path, RecursiveMode::NonRecursive)
        .map_err(|e| AppError::Message(format!("failed to watch settings.json: {e}")))?;

    Ok(ClaudeSettingsWatcher {
        state,
        shutdown,
        _debouncer: Some(debouncer),
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

/// 处理一次 settings.json 变化
fn handle_settings_change(
    path: &std::path::Path,
    provider: &Provider,
    state: &Mutex<Option<String>>,
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

    // 2. 读 model 字段
    let new_model = v.get("model").and_then(Value::as_str);

    // 3. 检查 model 字段是否变化（防循环）
    if !should_process(state, new_model) {
        return;
    }

    // 4. 检查 provider 的 autoSyncContextWindow 开关
    let auto_sync = provider
        .settings_config
        .get("autoSyncContextWindow")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !auto_sync {
        log::debug!("[ClaudeSettingsWatcher] auto-sync disabled for provider, skip");
        return;
    }

    // 5. 决定要写的窗口值
    let active = match resolve_active_model_window(&v, provider) {
        Some(a) => a,
        None => {
            log::debug!("[ClaudeSettingsWatcher] no active model window to write");
            return;
        }
    };

    // 6. 写 ACW/MAX
    let writes = build_env_writes(active.window);
    let new_content = match update_env_fields(&content, &writes) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[ClaudeSettingsWatcher] update failed: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(path, new_content) {
        log::warn!("[ClaudeSettingsWatcher] write failed: {e}");
    } else {
        log::info!(
            "[ClaudeSettingsWatcher] wrote ACW/MAX for model={} window={}",
            active.model,
            active.window
        );
    }
}

/// 原子地更新 settings.json 中 env 子对象的指定字段，其他字段全部保留
fn update_env_fields(content: &str, writes: &[(&'static str, String)]) -> Result<String, String> {
    let mut v: Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if !v.is_object() {
        return Err("top-level not object".to_string());
    }
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
    serde_json::to_string(&v).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_provider(env: Value) -> Provider {
        Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({ "env": env }),
            None,
        )
    }

    // ========== Task 2: 角色映射测试 ==========

    #[test]
    fn resolve_maps_haiku_to_anthropic_default_haiku_model() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "haiku");
        assert_eq!(result.window, 30000);
    }

    #[test]
    fn resolve_maps_sonnet_to_anthropic_default_sonnet_model() {
        let settings = json!({ "model": "sonnet" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "sonnet");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_opus_to_anthropic_default_opus_model() {
        let settings = json!({ "model": "opus" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "deepseek-v4-pro[1000000]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "opus");
        assert_eq!(result.window, 1000000);
    }

    #[test]
    fn resolve_maps_fable_to_anthropic_default_fable_model() {
        let settings = json!({ "model": "fable" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_FABLE_MODEL": "GLM-5.2[200k]"
        }));
        let result = resolve_active_model_window(&settings, &provider).unwrap();
        assert_eq!(result.model, "fable");
        assert_eq!(result.window, 200000);
    }

    #[test]
    fn resolve_maps_subagent_to_claude_code_subagent_model() {
        let settings = json!({ "model": "subagent" });
        let provider = make_provider(json!({
            "CLAUDE_CODE_SUBAGENT_MODEL": "deepseek-v4-flash"
        }));
        // subagent 没后缀 → 期望 None（无窗口可写）
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    // ========== Task 3: build_env_writes 测试 ==========

    #[test]
    fn build_writes_for_30k_window() {
        let writes = build_env_writes(30000);
        assert_eq!(
            writes,
            vec![
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "24000".to_string()),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "30000".to_string()),
            ]
        );
    }

    #[test]
    fn build_writes_for_1m_window() {
        let writes = build_env_writes(1000000);
        assert_eq!(
            writes,
            vec![
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "800000".to_string()),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1000000".to_string()),
            ]
        );
    }

    #[test]
    fn build_writes_for_200k_window() {
        let writes = build_env_writes(200000);
        assert_eq!(
            writes,
            vec![
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "160000".to_string()),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "200000".to_string()),
            ]
        );
    }

    #[test]
    fn build_writes_for_1_token_boundary() {
        // 最小边界：1 token → ACW=0（×0.8 = 0.8，向下取整 = 0）
        let writes = build_env_writes(1);
        assert_eq!(
            writes,
            vec![
                ("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "0".to_string()),
                ("CLAUDE_CODE_MAX_CONTEXT_TOKENS", "1".to_string()),
            ]
        );
    }

    // ========== Task 4: 无效输入处理测试 ==========

    #[test]
    fn resolve_returns_none_when_model_field_missing() {
        let settings = json!({});
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi[30k]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_value_unknown() {
        let settings = json!({ "model": "custom-alias" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_not_string() {
        let settings = json!({ "model": 123 });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_model_is_null() {
        let settings = json!({ "model": null });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_role_env_field_missing() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({}));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_env_value_not_string() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": { "name": "weird" }
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_invalid() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[invalid]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_zero() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[0]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_unsupported_unit() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1G]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_suffix_decimal() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "model[1.5m]"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    #[test]
    fn resolve_returns_none_when_no_suffix_at_all() {
        let settings = json!({ "model": "haiku" });
        let provider = make_provider(json!({
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "gpt-5.6"
        }));
        assert!(resolve_active_model_window(&settings, &provider).is_none());
    }

    // ========== Task 5: 防循环测试 ==========

    #[test]
    fn loop_same_model_consecutive_triggers() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(!should_process(&state, Some("haiku")));
        assert!(!should_process(&state, Some("haiku")));
    }

    #[test]
    fn loop_two_models_alternating() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, Some("sonnet")));
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, Some("sonnet")));
    }

    #[test]
    fn loop_model_to_none_to_same() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, None)); // model 被删
        assert!(should_process(&state, Some("haiku"))); // 重新出现
    }

    #[test]
    fn loop_model_to_none_stays_none() {
        let state = Mutex::new(None);
        assert!(should_process(&state, Some("haiku")));
        assert!(should_process(&state, None));
        // 后续 None 事件都算"无变化" → 跳过
        assert!(!should_process(&state, None));
        assert!(!should_process(&state, None));
    }

    #[test]
    fn loop_initial_state_with_existing_model() {
        // 启动时 model 已经是 "haiku"（如上次会话留下的）
        let state = Mutex::new(Some("haiku".to_string()));
        // 第一次触发就是 haiku → 跳过（不算变化）
        assert!(!should_process(&state, Some("haiku")));
        // 切到别的 → 处理
        assert!(should_process(&state, Some("sonnet")));
    }

    // ========== Task 6: 文件系统集成测试 ==========

    #[test]
    fn fs_update_env_fields_writes_only_env_keys() {
        let original = r#"{"model":"sonnet","effortLevel":"xhigh","env":{"ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]"}}"#;
        let writes = build_env_writes(1000000);
        let result = update_env_fields(original, &writes).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        // 顶层字段不动
        assert_eq!(v["model"], "sonnet");
        assert_eq!(v["effortLevel"], "xhigh");
        // env 只加了 ACW/MAX
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "800000");
        assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
        // 原有 env 字段保留
        assert_eq!(v["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"], "MiniMax-M3[1M]");
    }

    #[test]
    fn fs_update_env_fields_creates_env_if_missing() {
        let original = r#"{"model":"haiku","effortLevel":"max"}"#;
        let writes = build_env_writes(30000);
        let result = update_env_fields(original, &writes).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["model"], "haiku");
        assert_eq!(v["effortLevel"], "max");
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
        assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
    }

    #[test]
    fn fs_update_env_fields_preserves_existing_env_fields() {
        let original = r#"{"model":"haiku","env":{"ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi[30k]","CLAUDE_CODE_SUBAGENT_MODEL":"deepseek"}}"#;
        let writes = build_env_writes(30000);
        let result = update_env_fields(original, &writes).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "Kimi[30k]");
        assert_eq!(v["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "deepseek");
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
    }

    #[test]
    fn fs_update_env_fields_overwrites_existing_acw_max() {
        let original = r#"{"model":"haiku","env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"999","CLAUDE_CODE_MAX_CONTEXT_TOKENS":"888"}}"#;
        let writes = build_env_writes(30000);
        let result = update_env_fields(original, &writes).unwrap();
        let v: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
        assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
    }

    /// 用 tempfile 创建临时目录，验证真实 fs 事件的 watcher 行为
    #[test]
    fn fs_real_watcher_external_model_change() {
        use std::fs;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let initial = json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, initial.to_string()).unwrap();

        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        })));

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();

        // 模拟外部程序修改 model 字段
        let new_content = json!({
            "model": "haiku",
            "env": {
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "Kimi-K2.7-Code[30k]",
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, new_content.to_string()).unwrap();

        // 等待 debouncer + 文件写入生效
        thread::sleep(Duration::from_millis(800));

        // 验证 ACW/MAX 已被写入
        let content = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["model"], "haiku");
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
        assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");

        drop(watcher);
    }

    /// 只改 effortLevel 不应该触发 ACW/MAX 写入
    #[test]
    fn fs_real_watcher_effort_change_no_trigger() {
        use std::fs;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let initial = json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, initial.to_string()).unwrap();

        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        })));

        let watcher = spawn_claude_settings_watcher(path.clone(), provider).unwrap();

        // 只改 effortLevel
        let new_content = json!({
            "model": "sonnet",
            "effortLevel": "max",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, new_content.to_string()).unwrap();

        thread::sleep(Duration::from_millis(800));

        // ACW/MAX 不应该被写入（model 没变）
        let content = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert!(v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none());
        assert!(v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none());

        drop(watcher);
    }

    /// 设置不存在时的行为
    #[test]
    fn fs_real_watcher_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]","ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        })));

        let result = spawn_claude_settings_watcher(path, provider);
        // 文件不存在 → 应该出错（watch 失败）
        assert!(result.is_err());
    }
    /// 回归测试：production 路径下 spawn 的 watcher 必须靠 replace_watcher 存活，
    /// 不能因为返回值没绑定到局部变量就被 Drop。
    ///
    /// 修复前（直接 if-let-Err 丢弃 Ok 返回值）：watcher 构造完立即 Drop，
    /// notify 线程退出，改文件后 ACW/MAX 不会被写入。
    /// 修复后（spawn 的 Ok 交给 replace_watcher 存进进程单例）：watcher 存活，
    /// 改 model 字段后 ACW/MAX 正确写入。
    #[test]
    fn fs_watcher_survives_via_replace_watcher_without_local_binding() {
        use std::fs;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let initial = json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, initial.to_string()).unwrap();

        let provider = Arc::new(make_provider(json!({
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
        })));

        // 模拟 production 调用：spawn 后立即存进进程单例，不保留局部绑定。
        // spawned 在这里 move 进 replace_watcher，没有局部变量持有 watcher。
        let spawned = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        replace_watcher(spawned);

        // 改 model 字段，模拟 Claude Code /model 切换 sonnet -> haiku
        let new_content = json!({
            "model": "haiku",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, new_content.to_string()).unwrap();

        thread::sleep(Duration::from_millis(800));

        let content = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(v["model"], "haiku");
        assert_eq!(v["env"]["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "24000");
        assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "30000");
    }

    /// 回归测试 #4：autoSyncContextWindow=false 时，model 字段变化不写 ACW/MAX。
    /// 验证开关关闭后终端切模型不会同步（开关行为链路：toggle OFF -> save ->
    /// update -> write_live -> replace_watcher(新 provider 快照) -> watcher 读到 false -> skip）。
    #[test]
    fn fs_watcher_auto_sync_disabled_skips_writes() {
        use std::fs;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let initial = json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, initial.to_string()).unwrap();

        // provider 显式关闭 autoSyncContextWindow
        let provider = Arc::new(Provider::with_id(
            "p".to_string(),
            "P".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
                },
                "autoSyncContextWindow": false
            }),
            None,
        ));

        let spawned = spawn_claude_settings_watcher(path.clone(), provider).unwrap();
        replace_watcher(spawned);

        // 改 model 字段 sonnet -> haiku
        let new_content = json!({
            "model": "haiku",
            "env": {
                "ANTHROPIC_DEFAULT_SONNET_MODEL":"MiniMax-M3[1M]",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL":"Kimi-K2.7-Code[30k]"
            }
        });
        fs::write(&path, new_content.to_string()).unwrap();

        thread::sleep(Duration::from_millis(800));

        let content = fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&content).unwrap();
        // model 字段确实变了（说明事件被收到），但 ACW/MAX 不应该被写
        assert_eq!(v["model"], "haiku");
        assert!(
            v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none(),
            "autoSync OFF 时不应写 ACW，但实际写入了: {:?}",
            v["env"].get("CLAUDE_CODE_AUTO_COMPACT_WINDOW")
        );
        assert!(
            v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS").is_none(),
            "autoSync OFF 时不应写 MAX，但实际写入了: {:?}",
            v["env"].get("CLAUDE_CODE_MAX_CONTEXT_TOKENS")
        );
    }
}
