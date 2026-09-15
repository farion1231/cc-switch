//! 插件系统核心框架
//!
//! 把代理转发链路中的请求/响应改写抽象为统一的插件扩展点：
//! - [`ProxyPlugin`]：内置插件与外部插件的统一抽象；
//! - [`PluginRegistry`]：插件注册表（排序、stage 过滤、运行时覆盖）；
//! - [`run_request_pipeline`]：fail-open 管线执行函数（挂点只调这一个函数）；
//! - `builtin`：内置插件（cache 注入 / thinking 优化）；
//! - `external`：用户自定义外部脚本插件（plugin.json + stdin/stdout JSON 协议）；
//! - `init`：注册表初始化（内置 + 用户插件 + overrides）与运行时重载。
//!
//! **核心语义（不可妥协）**：插件永远不能破坏主链路。任何插件返回错误、超时、
//! panic，都只记录 `log::warn` 并跳过该插件，请求继续（fail-open）。
//!
//! 详见 docs/dev/plugin-system-contract.md。

pub mod builtin;
pub mod external;
pub mod init;
pub mod registry;
pub mod types;

// 供阶段 2b 的 Tauri 命令层使用（apply_overrides/plugins_dir 亦在 crate 内复用）
#[allow(unused_imports)]
pub use init::{apply_overrides, init_registry, plugins_dir, reload_user_plugins};
pub use registry::{run_request_pipeline, run_sse_pipeline, PluginRegistry};
#[allow(unused_imports)] // 部分重导出仅供外部（commands/services/前端序列化）使用
pub use types::{
    PluginError, PluginInfo, PluginManifest, PluginOverride, PluginProviderInfo,
    PluginRequestContext, PluginStage, PluginsConfig,
};

/// 插件 trait：内置插件与外部插件的统一抽象
pub trait ProxyPlugin: Send + Sync {
    /// 唯一 ID：内置为 "builtin:<name>"，用户插件为 "user:<manifest.id>"
    fn id(&self) -> &str;
    /// 显示名
    fn display_name(&self) -> &str;
    /// 描述
    fn description(&self) -> &str;
    fn is_builtin(&self) -> bool;
    /// 声明支持的 stage
    fn stages(&self) -> &'static [PluginStage];
    /// 默认优先级，数字越小越先执行。内置插件占用 100-899，用户插件默认 500
    fn default_priority(&self) -> i32;
    /// 默认启用状态（内置恒为 true；外部插件来自 manifest.enabled）
    fn default_enabled(&self) -> bool {
        true
    }
    /// 版本（用户插件来自 manifest；内置为 None）
    fn version(&self) -> Option<String> {
        None
    }
    /// 来源（用户插件 manifest 路径；内置为 None）
    fn source(&self) -> Option<String> {
        None
    }

    /// 请求变换（PreRequest / PreSend 阶段调用）
    /// 返回 Ok(true) 表示修改了 body；Ok(false) 表示未修改
    fn transform_request(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        let _ = (ctx, body);
        Ok(false)
    }

    /// 响应变换（PostResponse 阶段调用，仅非流式）
    fn transform_response(
        &self,
        ctx: &PluginRequestContext,
        body: &mut serde_json::Value,
    ) -> Result<bool, PluginError> {
        let _ = (ctx, body);
        Ok(false)
    }

    /// 每条流为每个插件创建一次的私有状态（跨 chunk 携带，如跨 delta 的半截标记缓冲）。
    /// 返回 None 表示该插件无状态（管线会传入 `&mut ()` 占位）。
    fn new_sse_state(&self) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }

    /// 对单个 SSE 事件的 data 负载做变换（SseChunk 阶段调用）。
    /// data 为原始字符串（不解析 JSON，子串替换即可，避免再序列化）。
    /// 返回 Ok(true) 表示修改了 data。
    fn transform_sse_event(
        &self,
        ctx: &PluginRequestContext,
        event_name: Option<&str>,
        data: &mut String,
        state: &mut dyn std::any::Any,
    ) -> Result<bool, PluginError> {
        let _ = (ctx, event_name, data, state);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证默认实现：不做任何修改
    #[test]
    fn test_default_trait_impls_noop() {
        struct NoopPlugin;
        impl ProxyPlugin for NoopPlugin {
            fn id(&self) -> &str {
                "builtin:noop"
            }
            fn display_name(&self) -> &str {
                "Noop"
            }
            fn description(&self) -> &str {
                ""
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
        }

        let plugin = NoopPlugin;
        let ctx = PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "s1".to_string(),
            request_model: "m".to_string(),
            stage: PluginStage::PreRequest,
            provider: None,
        };
        let mut body = serde_json::json!({"model": "m"});
        assert!(plugin.default_enabled());
        assert_eq!(plugin.version(), None);
        assert_eq!(plugin.source(), None);
        assert!(!plugin.transform_request(&ctx, &mut body).unwrap());
        assert!(!plugin.transform_response(&ctx, &mut body).unwrap());
        assert_eq!(body, serde_json::json!({"model": "m"}));
        // SseChunk 默认实现：无状态、不改 data
        assert!(plugin.new_sse_state().is_none());
        let mut data = "hello".to_string();
        let mut state: Box<dyn std::any::Any + Send> = Box::new(());
        assert!(!plugin
            .transform_sse_event(&ctx, Some("message"), &mut data, state.as_mut())
            .unwrap());
        assert_eq!(data, "hello");
    }
}
