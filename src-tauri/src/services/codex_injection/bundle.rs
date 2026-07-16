//! Build the in-page bootstrap JS. Never embeds secrets like OPENAI_API_KEY.

use crate::settings::CodexWorkbenchSettings;

/// Produce a self-executing bootstrap that installs feature flags and a secure bridge client.
pub fn build_bootstrap_bundle(
    settings: &CodexWorkbenchSettings,
    instance_id: &str,
    bridge_port: u16,
    nonce: &str,
) -> String {
    let e = &settings.enhancements;
    format!(
        r#"(function(){{
  if (window.__ccSwitchCodexBootstrapped) return;
  window.__ccSwitchCodexBootstrapped = true;
  window.__ccSwitchCodex = {{
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
  }};
  try {{
    var auth = 'Bearer ' + window.__ccSwitchCodex.nonce;
    fetch('http://127.0.0.1:' + window.__ccSwitchCodex.bridgePort + '/health', {{
      headers: {{ Authorization: auth }}
    }}).catch(function(){{}});
  }} catch (e) {{}}
}})();"#,
        instance_id = serde_json::to_string(instance_id).unwrap_or_else(|_| "\"\"".into()),
        bridge_port = bridge_port,
        nonce = serde_json::to_string(nonce).unwrap_or_else(|_| "\"\"".into()),
        plugin_unlock = e.plugin_unlock,
        auto_expand = e.auto_expand,
        session_delete = e.session_delete,
        wide_conversation = e.wide_conversation,
        native_menu = e.native_menu,
        user_script_runtime = e.user_script_runtime,
        markdown_export = e.markdown_export,
        model_switcher = e.model_switcher,
        system_prompt = e.system_prompt,
        reasoning_resume = e.reasoning_resume,
        reasoning_token = e.reasoning_token,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CodexEnhancementSettings, CodexWorkbenchSettings};

    #[test]
    fn bootstrap_contains_feature_flags() {
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
        assert!(bundle.contains("wideConversation"));
        assert!(bundle.contains("true"));
        assert!(bundle.contains("instance-1"));
        assert!(!bundle.contains("OPENAI_API_KEY"));
        assert!(bundle.contains("Bearer"));
        assert!(bundle.contains("17890"));
    }
}
