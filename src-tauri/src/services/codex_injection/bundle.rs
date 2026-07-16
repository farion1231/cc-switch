//! Build the in-page bootstrap JS. Never embeds secrets like OPENAI_API_KEY.

use crate::settings::CodexWorkbenchSettings;

const RENDERER_FEATURES: &str =
    include_str!("../../../resources/codex-workbench/renderer-features.js");
const RENDERER_INJECT: &str =
    include_str!("../../../resources/codex-workbench/renderer-inject.js");

fn js_bool(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn js_string(s: &str) -> String {
    // Minimal JSON string escape for embedding in JS object literal.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Produce a self-executing bootstrap that installs feature flags and a secure bridge client.
pub fn build_bootstrap_bundle(
    settings: &CodexWorkbenchSettings,
    instance_id: &str,
    bridge_port: u16,
    nonce: &str,
) -> String {
    let e = &settings.enhancements;
    let config = format!(
        r#"{{
  instanceId: {instance_id},
  bridgePort: {bridge_port},
  nonce: {nonce},
  features: {{
    pluginUnlock: {plugin_unlock},
    autoExpand: {auto_expand},
    sessionDelete: {session_delete},
    wideConversation: {wide_conversation},
    nativeMenu: {native_menu},
    userScriptRuntime: {user_script_runtime},
    markdownExport: {markdown_export},
    modelSwitcher: {model_switcher},
    systemPrompt: {system_prompt},
    reasoningResume: {reasoning_resume},
    reasoningToken: {reasoning_token}
  }}
}}"#,
        instance_id = js_string(instance_id),
        bridge_port = bridge_port,
        nonce = js_string(nonce),
        plugin_unlock = js_bool(e.plugin_unlock),
        auto_expand = js_bool(e.auto_expand),
        session_delete = js_bool(e.session_delete),
        wide_conversation = js_bool(e.wide_conversation),
        native_menu = js_bool(e.native_menu),
        user_script_runtime = js_bool(e.user_script_runtime),
        markdown_export = js_bool(e.markdown_export),
        model_switcher = js_bool(e.model_switcher),
        system_prompt = js_bool(e.system_prompt),
        reasoning_resume = js_bool(e.reasoning_resume),
        reasoning_token = js_bool(e.reasoning_token),
    );

    // Order: features first, then inject runtime, then bootstrap call.
    // Idempotent: runtime configure() if same instanceId.
    format!(
        r#"(function(){{
{features}
{inject}
if (typeof window !== "undefined" && typeof window.__ccSwitchCodexBootstrap === "function") {{
  window.__ccSwitchCodexBootstrap({config});
}}
}})();"#,
        features = RENDERER_FEATURES,
        inject = RENDERER_INJECT,
        config = config
    )
}

/// Alias used by plan wording.
pub fn build_injection_bundle(
    settings: &CodexWorkbenchSettings,
    instance_id: &str,
    bridge_port: u16,
    nonce: &str,
) -> String {
    build_bootstrap_bundle(settings, instance_id, bridge_port, nonce)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CodexEnhancementSettings, CodexWorkbenchSettings};

    #[test]
    fn bootstrap_contains_feature_flags_but_no_secrets() {
        let settings = CodexWorkbenchSettings {
            enhancements: CodexEnhancementSettings {
                plugin_unlock: true,
                auto_expand: true,
                session_delete: true,
                wide_conversation: true,
                native_menu: true,
                user_script_runtime: true,
                markdown_export: false,
                model_switcher: true,
                system_prompt: false,
                reasoning_resume: false,
                reasoning_token: false,
            },
            ..CodexWorkbenchSettings::default()
        };
        let bundle = build_bootstrap_bundle(&settings, "instance-1", 17890, "nonce-abc");
        assert!(bundle.contains("pluginUnlock"));
        assert!(bundle.contains("wideConversation"));
        assert!(bundle.contains("true"));
        assert!(bundle.contains("instance-1"));
        assert!(!bundle.contains("OPENAI_API_KEY"));
        assert!(bundle.contains("Bearer"));
        assert!(bundle.contains("17890"));
        assert!(bundle.contains("__ccSwitchCodexBootstrap"));
        assert!(bundle.contains("configure"));
        assert!(bundle.contains("dispose"));
        // features source embedded
        assert!(bundle.contains("__ccSwitchCodexFeatures"));
    }

    #[test]
    fn bootstrap_is_idempotent_marker_present() {
        let settings = CodexWorkbenchSettings::default();
        let bundle = build_injection_bundle(&settings, "id-a", 1, "n");
        assert!(bundle.contains("instanceId"));
        assert!(bundle.contains("__ccSwitchCodex"));
    }
}
