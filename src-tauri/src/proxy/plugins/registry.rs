//! 插件注册表与 fail-open 管线执行
//!
//! `PluginRegistry` 负责插件的注册/排序/过滤与运行时覆盖（enabled/priority），
//! 挂点只调用 [`run_request_pipeline`] 一个函数，保证 fail-open 语义集中实现。

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use super::types::{PluginError, PluginInfo, PluginOverride, PluginRequestContext, PluginStage};
use super::ProxyPlugin;

struct RegistryInner {
    /// 所有插件，按生效优先级升序排列（同优先级保持注册顺序）
    plugins: Vec<Arc<dyn ProxyPlugin>>,
    /// 每个插件的 enabled/priority 运行时覆盖，key = plugin id
    overrides: HashMap<String, PluginOverride>,
    /// 加载失败的插件条目（list 时一并展示，供前端提示）
    failed: Vec<PluginInfo>,
}

/// 插件注册表
pub struct PluginRegistry {
    inner: RwLock<RegistryInner>,
    /// 全局开关（PluginsConfig.enabled 的内存态）：关闭时所有插件均不参与管线，
    /// list() 中所有条目的 enabled 也展示为 false
    global_enabled: AtomicBool,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(RegistryInner {
                plugins: Vec::new(),
                overrides: HashMap::new(),
                failed: Vec::new(),
            }),
            global_enabled: AtomicBool::new(true),
        }
    }

    /// 设置全局开关（运行时切换，持久化由 DAO 层负责）
    pub fn set_global_enabled(&self, enabled: bool) {
        self.global_enabled.store(enabled, Ordering::SeqCst);
    }

    /// 读取全局开关状态
    pub fn global_enabled(&self) -> bool {
        self.global_enabled.load(Ordering::SeqCst)
    }

    /// 注册插件（幂等：同 id 后注册者覆盖先注册者），并应用已有 override
    pub fn register(&self, plugin: Arc<dyn ProxyPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.plugins.retain(|p| p.id() != plugin.id());
        inner.plugins.push(plugin);
        let RegistryInner {
            plugins, overrides, ..
        } = &mut *inner;
        Self::sort_plugins(plugins, overrides);
    }

    /// 清除所有非内置插件及其失败条目（重载用户插件前调用）
    pub fn clear_user_plugins(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.plugins.retain(|p| p.is_builtin());
        inner.failed.clear();
    }

    /// 追加一条"加载失败"的插件条目（契约 2.9：list 也要展示失败条目）
    pub fn add_failed_entry(&self, info: PluginInfo) {
        self.inner.write().unwrap().failed.push(info);
    }

    /// 列出全部插件元信息（供 Tauri 命令/前端），按生效优先级排序；
    /// 加载失败条目追加在末尾
    pub fn list(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().unwrap();
        let global = self.global_enabled.load(Ordering::SeqCst);
        let mut result: Vec<PluginInfo> = inner
            .plugins
            .iter()
            .map(|plugin| PluginInfo {
                id: plugin.id().to_string(),
                display_name: plugin.display_name().to_string(),
                description: plugin.description().to_string(),
                is_builtin: plugin.is_builtin(),
                stages: plugin.stages().to_vec(),
                priority: effective_priority(plugin.as_ref(), &inner.overrides),
                // 全局开关关闭时所有条目均展示为禁用
                enabled: global && is_effectively_enabled(plugin.as_ref(), &inner.overrides),
                version: plugin.version(),
                source: plugin.source(),
                has_config: !plugin.config_schema().is_empty(),
                config_title: plugin.config_title().map(str::to_string),
                error: None,
            })
            .collect();
        result.extend(inner.failed.iter().cloned());
        result
    }

    /// 按 id 取插件实例（配置读写命令用；失败条目不在此列）
    pub fn plugin_by_id(&self, id: &str) -> Option<Arc<dyn ProxyPlugin>> {
        self.inner
            .read()
            .unwrap()
            .plugins
            .iter()
            .find(|plugin| plugin.id() == id)
            .cloned()
    }

    /// 按 stage 取出启用的插件（只返回启用且声明该 stage 的插件，按生效优先级升序）；
    /// 全局开关关闭时返回空 vec
    pub fn plugins_for_stage(&self, stage: PluginStage) -> Vec<Arc<dyn ProxyPlugin>> {
        if !self.global_enabled.load(Ordering::SeqCst) {
            return Vec::new();
        }
        let inner = self.inner.read().unwrap();
        // inner.plugins 本身已按生效优先级升序，这里只需过滤
        inner
            .plugins
            .iter()
            .filter(|p| {
                is_effectively_enabled(p.as_ref(), &inner.overrides) && p.stages().contains(&stage)
            })
            .cloned()
            .collect()
    }

    /// 设置运行时覆盖（enabled/priority）。
    /// 两个参数均为 None 时移除该插件的覆盖；持久化由 DAO 层负责。
    pub fn set_override(&self, plugin_id: &str, enabled: Option<bool>, priority: Option<i32>) {
        let mut inner = self.inner.write().unwrap();
        if enabled.is_none() && priority.is_none() {
            inner.overrides.remove(plugin_id);
        } else {
            inner
                .overrides
                .insert(plugin_id.to_string(), PluginOverride { enabled, priority });
        }
        let RegistryInner {
            plugins, overrides, ..
        } = &mut *inner;
        // 禁用钩子：外部常驻插件借此终止子进程（重新启用后按需重新拉起，
        // Codex 审查 P2——否则被禁用的第三方进程会一直存活到重载/退出）
        if enabled == Some(false) {
            if let Some(plugin) = plugins.iter().find(|p| p.id() == plugin_id) {
                plugin.on_disabled();
            }
        }
        Self::sort_plugins(plugins, overrides);
    }

    /// 返回所有运行时覆盖的快照
    pub fn overrides(&self) -> HashMap<String, PluginOverride> {
        self.inner.read().unwrap().overrides.clone()
    }

    /// 按生效优先级升序重排（稳定排序，同优先级保持注册顺序）
    fn sort_plugins(
        plugins: &mut [Arc<dyn ProxyPlugin>],
        overrides: &HashMap<String, PluginOverride>,
    ) {
        // sort_by_key 无法对借用 dyn 的 key 排序，改用 sort_by
        plugins.sort_by(|a, b| {
            effective_priority(a.as_ref(), overrides)
                .cmp(&effective_priority(b.as_ref(), overrides))
        });
    }
}

/// 插件实际生效优先级：override.priority 优先，否则 default_priority
pub(crate) fn effective_priority(
    plugin: &dyn ProxyPlugin,
    overrides: &HashMap<String, PluginOverride>,
) -> i32 {
    overrides
        .get(plugin.id())
        .and_then(|o| o.priority)
        .unwrap_or_else(|| plugin.default_priority())
}

/// 插件实际是否启用：默认启用 且 override.enabled 不为 false
pub(crate) fn is_effectively_enabled(
    plugin: &dyn ProxyPlugin,
    overrides: &HashMap<String, PluginOverride>,
) -> bool {
    plugin.default_enabled() && overrides.get(plugin.id()).and_then(|o| o.enabled) != Some(false)
}

/// 执行某个 stage 的插件管线。
///
/// 任何插件错误或 panic 仅 `log::warn` 并跳过该插件（fail-open），请求继续。
/// 返回是否有插件修改了 body。
pub fn run_request_pipeline(
    registry: &PluginRegistry,
    stage: PluginStage,
    ctx: &PluginRequestContext,
    body: &mut serde_json::Value,
    apply: impl Fn(
        &dyn ProxyPlugin,
        &PluginRequestContext,
        &mut serde_json::Value,
    ) -> Result<bool, PluginError>,
) -> bool {
    let mut changed = false;
    for plugin in registry.plugins_for_stage(stage) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apply(plugin.as_ref(), ctx, body)
        }));
        match result {
            Ok(Ok(true)) => changed = true,
            Ok(Ok(false)) => {}
            Ok(Err(err)) => {
                log::warn!(
                    "[PLUGIN] 插件 {} 执行失败，已跳过(fail-open): {err}",
                    plugin.id()
                );
            }
            Err(panic_payload) => {
                log::warn!(
                    "[PLUGIN] 插件 {} 发生 panic，已跳过(fail-open): {}",
                    plugin.id(),
                    panic_message(&panic_payload)
                );
            }
        }
    }
    changed
}

/// 执行 SseChunk 阶段的 SSE 事件管线（与 [`run_request_pipeline`] 同构的 fail-open 执行器）。
///
/// - 每次调用开头取一次 [`PluginRegistry::plugins_for_stage`]（SseChunk）并遍历同一份，
///   保证插件顺序与 `states` 下标对齐（注册表排序稳定，同一流内多次调用顺序一致）；
///   `states` 长度与插件数量不一致时自动补齐/截断（防御注册表热重载）。
/// - 每个插件使用 `states` 中与自己同下标的私有状态槽位：槽位为 None 时调用
///   [`ProxyPlugin::new_sse_state`] 初始化并放回；无状态插件（返回 None）传 `&mut ()` 占位。
/// - 任何插件错误或 panic 仅 `log::warn` 并跳过该插件（fail-open），事件继续。
///
/// 返回是否有插件修改了 data。
pub fn run_sse_pipeline(
    registry: &PluginRegistry,
    ctx: &PluginRequestContext,
    event_name: Option<&str>,
    data: &mut String,
    states: &mut Vec<Option<Box<dyn Any + Send>>>,
) -> bool {
    let plugins = registry.plugins_for_stage(PluginStage::SseChunk);
    if plugins.len() != states.len() {
        log::warn!(
            "[PLUGIN] SseChunk 状态槽数量与插件数量不一致（{} vs {}），已自动调整",
            states.len(),
            plugins.len()
        );
        states.resize_with(plugins.len(), || None);
    }
    let mut changed = false;
    for (index, plugin) in plugins.iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // 惰性初始化该插件的私有状态槽位；无状态插件用 () 占位
            if states[index].is_none() {
                states[index] = plugin.new_sse_state();
            }
            let mut anonymous_state: Box<dyn Any + Send> = Box::new(());
            let state: &mut dyn Any = match states[index].as_mut() {
                Some(any_state) => any_state.as_mut(),
                None => anonymous_state.as_mut(),
            };
            plugin.transform_sse_event(ctx, event_name, data, state)
        }));
        match result {
            Ok(Ok(true)) => changed = true,
            Ok(Ok(false)) => {}
            Ok(Err(err)) => {
                log::warn!(
                    "[PLUGIN] 插件 {} SseChunk 执行失败，已跳过(fail-open): {err}",
                    plugin.id()
                );
            }
            Err(panic_payload) => {
                log::warn!(
                    "[PLUGIN] 插件 {} SseChunk 发生 panic，已跳过(fail-open): {}",
                    plugin.id(),
                    panic_message(&panic_payload)
                );
            }
        }
    }
    changed
}

/// 从 panic payload 提取可读消息
pub(crate) fn panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::super::types::{PluginError, PluginRequestContext};
    use super::*;

    /// 测试用 mock 插件：可配置优先级/启用/stages/行为
    struct MockPlugin {
        id: String,
        stages: &'static [PluginStage],
        priority: i32,
        enabled: bool,
        builtin: bool,
        outcome: MockOutcome,
        panic: bool,
        calls: AtomicUsize,
    }

    #[derive(Clone, Copy)]
    enum MockOutcome {
        Noop,
        Modify,
        Fail,
    }

    impl MockPlugin {
        fn new(id: &str, stages: &'static [PluginStage], priority: i32) -> Self {
            Self {
                id: id.to_string(),
                stages,
                priority,
                enabled: true,
                builtin: false,
                outcome: MockOutcome::Noop,
                panic: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn with_outcome(mut self, outcome: MockOutcome) -> Self {
            self.outcome = outcome;
            self
        }

        fn with_panic(mut self) -> Self {
            self.panic = true;
            self
        }

        fn with_enabled(mut self, enabled: bool) -> Self {
            self.enabled = enabled;
            self
        }

        fn into_arc(self) -> Arc<dyn ProxyPlugin> {
            Arc::new(self)
        }
    }

    impl ProxyPlugin for MockPlugin {
        fn id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn description(&self) -> &str {
            "mock"
        }
        fn is_builtin(&self) -> bool {
            self.builtin
        }
        fn stages(&self) -> &'static [PluginStage] {
            self.stages
        }
        fn default_priority(&self) -> i32 {
            self.priority
        }
        fn default_enabled(&self) -> bool {
            self.enabled
        }
        fn transform_request(
            &self,
            _ctx: &PluginRequestContext,
            body: &mut serde_json::Value,
        ) -> Result<bool, PluginError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.panic {
                panic!("mock plugin panicked");
            }
            match self.outcome {
                MockOutcome::Noop => Ok(false),
                MockOutcome::Modify => {
                    body["touched"] = json!(self.id.clone());
                    Ok(true)
                }
                MockOutcome::Fail => Err(PluginError::Execution {
                    plugin_id: self.id.clone(),
                    message: "boom".to_string(),
                }),
            }
        }
    }

    fn ctx(stage: PluginStage) -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage,
            provider: None,
        }
    }

    #[test]
    fn test_register_sorts_by_priority() {
        let registry = PluginRegistry::new();
        registry.register(MockPlugin::new("p300", &[PluginStage::PreRequest], 300).into_arc());
        registry.register(MockPlugin::new("p100", &[PluginStage::PreRequest], 100).into_arc());
        registry.register(MockPlugin::new("p200", &[PluginStage::PreRequest], 200).into_arc());

        let ordered: Vec<String> = registry
            .plugins_for_stage(PluginStage::PreRequest)
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        assert_eq!(ordered, vec!["p100", "p200", "p300"]);
    }

    #[test]
    fn test_register_idempotent_override() {
        let registry = PluginRegistry::new();
        registry.register(MockPlugin::new("same-id", &[PluginStage::PreRequest], 100).into_arc());
        registry.register(MockPlugin::new("same-id", &[PluginStage::PreSend], 900).into_arc());

        let all = registry.list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].stages, vec![PluginStage::PreSend]);
        assert_eq!(all[0].priority, 900);
    }

    #[test]
    fn test_plugins_for_stage_filters_disabled_and_stage() {
        let registry = PluginRegistry::new();
        registry.register(
            MockPlugin::new("pre-request-only", &[PluginStage::PreRequest], 100).into_arc(),
        );
        registry.register(
            MockPlugin::new("disabled", &[PluginStage::PreRequest], 200)
                .with_enabled(false)
                .into_arc(),
        );
        registry.register(MockPlugin::new("pre-send", &[PluginStage::PreSend], 300).into_arc());

        let pre_request_ids: Vec<String> = registry
            .plugins_for_stage(PluginStage::PreRequest)
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        assert_eq!(pre_request_ids, vec!["pre-request-only"]);

        let pre_send_ids: Vec<String> = registry
            .plugins_for_stage(PluginStage::PreSend)
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        assert_eq!(pre_send_ids, vec!["pre-send"]);
    }

    #[test]
    fn test_set_override_enabled_and_priority() {
        let registry = PluginRegistry::new();
        registry.register(MockPlugin::new("a", &[PluginStage::PreRequest], 100).into_arc());
        registry.register(MockPlugin::new("b", &[PluginStage::PreRequest], 200).into_arc());

        // a 被禁用后不再参与管线
        registry.set_override("a", Some(false), None);
        let ids: Vec<String> = registry
            .plugins_for_stage(PluginStage::PreRequest)
            .iter()
            .map(|p| p.id().to_string())
            .collect();
        assert_eq!(ids, vec!["b"]);
        let infos = registry.list();
        assert_eq!(infos[0].id, "a");
        assert!(!infos[0].enabled);

        // b 提优先级：排序变化
        registry.set_override("b", None, Some(-50));
        let infos = registry.list();
        assert_eq!(infos[0].id, "b");
        assert_eq!(infos[0].priority, -50);

        // overrides 快照
        let overrides = registry.overrides();
        assert_eq!(overrides.get("a").unwrap().enabled, Some(false));
        assert_eq!(overrides.get("b").unwrap().priority, Some(-50));

        // 双 None 清除覆盖
        registry.set_override("b", None, None);
        assert!(!registry.overrides().contains_key("b"));
    }

    #[test]
    fn test_clear_user_plugins_keeps_builtin() {
        let registry = PluginRegistry::new();
        let mut builtin = MockPlugin::new("builtin:x", &[PluginStage::PreRequest], 100);
        builtin.builtin = true;
        registry.register(builtin.into_arc());
        registry.register(MockPlugin::new("user:y", &[PluginStage::PreRequest], 200).into_arc());
        registry.add_failed_entry(PluginInfo {
            id: "user:bad".to_string(),
            display_name: "bad".to_string(),
            description: String::new(),
            is_builtin: false,
            stages: vec![],
            priority: 500,
            enabled: false,
            version: None,
            source: None,
            has_config: false,
            config_title: None,
            error: Some("清单无效".to_string()),
        });

        registry.clear_user_plugins();

        let ids: Vec<String> = registry.list().iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids, vec!["builtin:x"]);
    }

    #[test]
    fn test_list_contains_failed_entry_with_error() {
        let registry = PluginRegistry::new();
        registry.add_failed_entry(PluginInfo {
            id: "user:broken".to_string(),
            display_name: "broken".to_string(),
            description: String::new(),
            is_builtin: false,
            stages: vec![],
            priority: 500,
            enabled: false,
            version: None,
            source: Some("/tmp/user/broken/plugin.json".to_string()),
            has_config: false,
            config_title: None,
            error: Some("JSON 错误".to_string()),
        });

        let all = registry.list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].error.as_deref(), Some("JSON 错误"));
    }

    #[test]
    fn test_pipeline_fail_open_skips_error_plugin() {
        let registry = PluginRegistry::new();
        registry.register(
            MockPlugin::new("err", &[PluginStage::PreRequest], 100)
                .with_outcome(MockOutcome::Fail)
                .into_arc(),
        );
        let ok_plugin = Arc::new(
            MockPlugin::new("ok", &[PluginStage::PreRequest], 200)
                .with_outcome(MockOutcome::Modify),
        );
        registry.register(ok_plugin.clone() as Arc<dyn ProxyPlugin>);

        let mut body = json!({"model": "m"});
        let changed = run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &ctx(PluginStage::PreRequest),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );

        assert!(changed);
        // 错误插件没有阻断后续插件
        assert_eq!(ok_plugin.calls.load(Ordering::SeqCst), 1);
        assert_eq!(body["touched"], json!("ok"));
    }

    #[test]
    fn test_pipeline_fail_open_skips_panicking_plugin() {
        let registry = PluginRegistry::new();
        registry.register(
            MockPlugin::new("panic", &[PluginStage::PreRequest], 100)
                .with_panic()
                .into_arc(),
        );
        let ok_plugin = Arc::new(
            MockPlugin::new("ok", &[PluginStage::PreRequest], 200)
                .with_outcome(MockOutcome::Modify),
        );
        registry.register(ok_plugin.clone() as Arc<dyn ProxyPlugin>);

        let mut body = json!({"model": "m"});
        let changed = run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &ctx(PluginStage::PreRequest),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );

        assert!(changed);
        assert_eq!(ok_plugin.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_pipeline_no_plugins_returns_false() {
        let registry = PluginRegistry::new();
        let mut body = json!({"model": "m"});
        let changed = run_request_pipeline(
            &registry,
            PluginStage::PreSend,
            &ctx(PluginStage::PreSend),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );
        assert!(!changed);
        assert_eq!(body, json!({"model": "m"}));
    }

    #[test]
    fn test_global_switch_disables_all_plugins() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("a", &[PluginStage::PreRequest], 100));
        registry.register(plugin.clone() as Arc<dyn ProxyPlugin>);

        // 默认开启
        assert!(registry.global_enabled());
        assert_eq!(registry.plugins_for_stage(PluginStage::PreRequest).len(), 1);
        assert!(registry.list()[0].enabled);

        // 全局关闭：管线为空，list 条目展示为禁用，overrides 不受影响
        registry.set_global_enabled(false);
        assert!(!registry.global_enabled());
        assert!(registry
            .plugins_for_stage(PluginStage::PreRequest)
            .is_empty());
        let infos = registry.list();
        assert_eq!(infos.len(), 1);
        assert!(!infos[0].enabled);
        assert!(registry.overrides().is_empty());

        let mut body = json!({});
        let changed = run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &ctx(PluginStage::PreRequest),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );
        assert!(!changed);
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);

        // 重新开启后恢复
        registry.set_global_enabled(true);
        assert_eq!(registry.plugins_for_stage(PluginStage::PreRequest).len(), 1);
        assert!(registry.list()[0].enabled);
    }

    #[test]
    fn test_pipeline_respects_disabled_override() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("a", &[PluginStage::PreRequest], 100));
        registry.register(plugin.clone() as Arc<dyn ProxyPlugin>);
        registry.set_override("a", Some(false), None);

        let mut body = json!({});
        let changed = run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &ctx(PluginStage::PreRequest),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        );
        assert!(!changed);
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);
    }

    // ------------------------------------------------------------------
    // run_sse_pipeline tests
    // ------------------------------------------------------------------

    /// SSE 测试插件：
    /// - Stateful：用私有状态（计数器）跨事件累加，把计数追加到 data 尾部
    /// - Stateless：断言收到 `&mut ()` 占位，可配置是否修改 data / 返回错误 / panic
    struct SseMockPlugin {
        id: String,
        priority: i32,
        stateful: bool,
        modify: bool,
        fail: bool,
        panic: bool,
        calls: AtomicUsize,
    }

    impl SseMockPlugin {
        fn stateful(id: &str, priority: i32) -> Self {
            Self {
                id: id.to_string(),
                priority,
                stateful: true,
                modify: true,
                fail: false,
                panic: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn stateless(id: &str, priority: i32) -> Self {
            Self {
                id: id.to_string(),
                priority,
                stateful: false,
                modify: true,
                fail: false,
                panic: false,
                calls: AtomicUsize::new(0),
            }
        }

        fn with_fail(mut self) -> Self {
            self.fail = true;
            self
        }

        fn with_panic(mut self) -> Self {
            self.panic = true;
            self
        }
    }

    impl ProxyPlugin for SseMockPlugin {
        fn id(&self) -> &str {
            &self.id
        }
        fn display_name(&self) -> &str {
            &self.id
        }
        fn description(&self) -> &str {
            "sse mock"
        }
        fn is_builtin(&self) -> bool {
            false
        }
        fn stages(&self) -> &'static [PluginStage] {
            &[PluginStage::SseChunk]
        }
        fn default_priority(&self) -> i32 {
            self.priority
        }
        fn new_sse_state(&self) -> Option<Box<dyn Any + Send>> {
            if self.stateful {
                Some(Box::new(0usize))
            } else {
                None
            }
        }
        fn transform_sse_event(
            &self,
            _ctx: &PluginRequestContext,
            _event_name: Option<&str>,
            data: &mut String,
            state: &mut dyn Any,
        ) -> Result<bool, PluginError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.panic {
                panic!("sse mock plugin panicked");
            }
            if self.fail {
                return Err(PluginError::Execution {
                    plugin_id: self.id.clone(),
                    message: "boom".to_string(),
                });
            }
            if !self.modify {
                return Ok(false);
            }
            if self.stateful {
                let counter = state
                    .downcast_mut::<usize>()
                    .expect("stateful plugin should receive its own counter state");
                *counter += 1;
                data.push_str(&format!("#{counter}"));
            } else {
                // 无状态插件收到的应是 `&mut ()` 占位
                assert!(
                    state.downcast_ref::<()>().is_some(),
                    "stateless plugin should receive unit state"
                );
                data.push_str("+s");
            }
            Ok(true)
        }
    }

    fn sse_ctx() -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage: PluginStage::SseChunk,
            provider: None,
        }
    }

    #[test]
    fn test_run_sse_pipeline_state_accumulates_across_events() {
        let registry = PluginRegistry::new();
        registry.register(Arc::new(SseMockPlugin::stateful("sse-a", 100)));

        let mut states: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
        let mut data = String::from("delta");
        assert!(run_sse_pipeline(
            &registry,
            &sse_ctx(),
            Some("content_block_delta"),
            &mut data,
            &mut states
        ));
        assert_eq!(data, "delta#1");

        // 第二次事件：同一 states 传入，状态跨事件累加
        let mut data2 = String::from("delta2");
        assert!(run_sse_pipeline(
            &registry,
            &sse_ctx(),
            Some("content_block_delta"),
            &mut data2,
            &mut states
        ));
        assert_eq!(data2, "delta2#2");
    }

    #[test]
    fn test_run_sse_pipeline_fail_open_skips_error_and_panic() {
        let registry = PluginRegistry::new();
        registry.register(Arc::new(
            SseMockPlugin::stateful("bad", 100).with_fail().with_panic(),
        ));
        // fail 在 panic 之前返回，这里拆成两个插件分别覆盖两条 fail-open 路径
        registry.register(Arc::new(
            SseMockPlugin::stateful("panicky", 110).with_panic(),
        ));
        let ok = Arc::new(SseMockPlugin::stateful("ok", 120));
        registry.register(ok.clone());

        let mut states: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
        let mut data = String::from("x");
        assert!(run_sse_pipeline(
            &registry,
            &sse_ctx(),
            None,
            &mut data,
            &mut states
        ));
        assert_eq!(data, "x#1");
        assert_eq!(ok.calls.load(Ordering::SeqCst), 1);

        // 失败/panic 插件的槽位保持未初始化，不影响后续插件继续工作
        let mut data2 = String::from("y");
        assert!(run_sse_pipeline(
            &registry,
            &sse_ctx(),
            None,
            &mut data2,
            &mut states
        ));
        assert_eq!(data2, "y#2");
    }

    #[test]
    fn test_run_sse_pipeline_stateless_plugin_gets_unit_state() {
        let registry = PluginRegistry::new();
        registry.register(Arc::new(SseMockPlugin::stateless("no-state", 100)));

        let mut states: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
        let mut data = String::from("v");
        assert!(run_sse_pipeline(
            &registry,
            &sse_ctx(),
            None,
            &mut data,
            &mut states
        ));
        assert_eq!(data, "v+s");
        // 无状态插件的槽位保持 None
        assert!(states[0].is_none());
    }

    #[test]
    fn test_run_sse_pipeline_no_plugins_returns_false() {
        let registry = PluginRegistry::new();
        let mut states: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
        let mut data = String::from("keep");
        assert!(!run_sse_pipeline(
            &registry,
            &sse_ctx(),
            None,
            &mut data,
            &mut states
        ));
        assert_eq!(data, "keep");
    }

    #[test]
    fn test_run_sse_pipeline_global_switch_disables() {
        let registry = PluginRegistry::new();
        let plugin = Arc::new(SseMockPlugin::stateful("sse-g", 100));
        registry.register(plugin.clone());
        registry.set_global_enabled(false);

        let mut states: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
        let mut data = String::from("z");
        assert!(!run_sse_pipeline(
            &registry,
            &sse_ctx(),
            None,
            &mut data,
            &mut states
        ));
        assert_eq!(data, "z");
        assert_eq!(plugin.calls.load(Ordering::SeqCst), 0);
    }

    /// 运行时禁用钩子探针：记录 on_disabled 触发次数
    struct DisableProbePlugin {
        disable_count: AtomicUsize,
    }

    impl ProxyPlugin for DisableProbePlugin {
        fn id(&self) -> &str {
            "builtin:disable-probe"
        }
        fn display_name(&self) -> &str {
            "DisableProbe"
        }
        fn description(&self) -> &str {
            "test-only"
        }
        fn is_builtin(&self) -> bool {
            true
        }
        fn stages(&self) -> &'static [PluginStage] {
            &[PluginStage::PreRequest]
        }
        fn default_priority(&self) -> i32 {
            500
        }
        fn on_disabled(&self) {
            self.disable_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_set_override_false_triggers_on_disabled() {
        // Codex 审查 P2：禁用插件必须触发 on_disabled（外部常驻插件借此终止子进程）
        let registry = PluginRegistry::new();
        let probe = Arc::new(DisableProbePlugin {
            disable_count: AtomicUsize::new(0),
        });
        registry.register(probe.clone());

        registry.set_override("builtin:disable-probe", Some(false), None);
        assert_eq!(
            probe.disable_count.load(Ordering::SeqCst),
            1,
            "禁用应触发 on_disabled 钩子"
        );

        registry.set_override("builtin:disable-probe", Some(true), None);
        assert_eq!(
            probe.disable_count.load(Ordering::SeqCst),
            1,
            "启用不触发钩子"
        );

        // 调整优先级（enabled=None）不触发
        registry.set_override("builtin:disable-probe", None, Some(10));
        assert_eq!(
            probe.disable_count.load(Ordering::SeqCst),
            1,
            "优先级调整不应触发钩子"
        );
    }
}
