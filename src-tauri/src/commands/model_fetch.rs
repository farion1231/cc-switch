//! 模型列表获取命令
//!
//! 提供 Tauri 命令，供前端在供应商表单中获取可用模型列表。

use crate::services::model_fetch::{self, FetchedModel};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeModelRef {
    pub provider_id: String,
    pub model_id: String,
}

const OPENCODE_MODELS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 获取 OpenCode 当前运行时可用的模型。
///
/// 复用工具更新页的 CLI 定位逻辑执行 `opencode models`，因此会包含 OpenCode
/// 已加载的 OAuth 模型与 Zen 免费模型，而不是只读取 opencode.json。
#[tauri::command]
pub async fn get_opencode_models() -> Result<Vec<OpenCodeModelRef>, String> {
    tokio::task::spawn_blocking(|| {
        // Align runtime discovery with the OpenCode config directory that
        // cc-switch already uses for live read/write (settings override included).
        let config_dir = crate::opencode_config::get_opencode_dir();
        let config_dir_env = config_dir.to_string_lossy().into_owned();
        let extra_env = [("OPENCODE_CONFIG_DIR", config_dir_env)];
        let output = super::misc::run_detected_tool_command_with_timeout(
            "opencode",
            &["models"],
            Some(OPENCODE_MODELS_TIMEOUT),
            &extra_env,
        )?;
        if !output.status.success() {
            let stderr = super::misc::decode_command_output(&output.stderr);
            let stdout = super::misc::decode_command_output(&output.stdout);
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            return Err(if detail.is_empty() {
                "Failed to load OpenCode models".to_string()
            } else {
                format!("Failed to load OpenCode models: {detail}")
            });
        }

        Ok(parse_opencode_models(&super::misc::decode_command_output(
            &output.stdout,
        )))
    })
    .await
    .map_err(|e| format!("OpenCode model discovery task failed: {e}"))?
}

fn parse_opencode_models(output: &str) -> Vec<OpenCodeModelRef> {
    output
        .lines()
        .filter_map(|line| {
            let (provider_id, model_id) = line.trim().split_once('/')?;
            if provider_id.is_empty()
                || model_id.is_empty()
                || !provider_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                || model_id.chars().any(char::is_whitespace)
            {
                return None;
            }
            Some((provider_id.to_string(), model_id.to_string()))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|(provider_id, model_id)| OpenCodeModelRef {
            provider_id,
            model_id,
        })
        .collect()
}

/// 获取供应商的可用模型列表
///
/// 使用 OpenAI 兼容的 GET /v1/models 端点。优先使用 `models_url` 精确覆写；
/// 否则对 baseURL 生成候选列表（含「剥离 Anthropic 兼容子路径」兜底），按序尝试。
#[tauri::command(rename_all = "camelCase")]
pub async fn fetch_models_for_config(
    base_url: String,
    api_key: String,
    is_full_url: Option<bool>,
    models_url: Option<String>,
    custom_user_agent: Option<String>,
) -> Result<Vec<FetchedModel>, String> {
    // 与转发 / 检测路径共用 parse_custom_user_agent：非法 UA 静默忽略（不阻断取模型）。
    let user_agent = crate::provider::parse_custom_user_agent(custom_user_agent.as_deref())
        .ok()
        .flatten();
    model_fetch::fetch_models(
        &base_url,
        &api_key,
        is_full_url.unwrap_or(false),
        models_url.as_deref(),
        user_agent,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{parse_opencode_models, OpenCodeModelRef};

    #[test]
    fn parses_sorts_and_deduplicates_models() {
        assert_eq!(
            parse_opencode_models(
                "openrouter/vendor/model\nopencode/free-model\ninvalid\nopencode/free-model\n"
            ),
            vec![
                OpenCodeModelRef {
                    provider_id: "opencode".to_string(),
                    model_id: "free-model".to_string(),
                },
                OpenCodeModelRef {
                    provider_id: "openrouter".to_string(),
                    model_id: "vendor/model".to_string(),
                },
            ]
        );
    }

    #[test]
    fn skips_malformed_output_lines() {
        assert!(parse_opencode_models(
            "notice: loading models\n/model\nprovider/\nbad provider/model\nprovider/bad model\n"
        )
        .is_empty());
    }
}
