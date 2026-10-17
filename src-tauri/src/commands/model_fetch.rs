//! 模型列表获取命令
//!
//! 提供 Tauri 命令，供前端在供应商表单中获取可用模型列表。

use crate::services::model_fetch::{self, FetchedModel};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCodeModelRef {
    pub provider_id: String,
    pub model_id: String,
}

/// Read the current runtime's model list. No shell and no credential values in errors.
#[tauri::command]
pub async fn get_agy_models() -> Result<Vec<FetchedModel>, String> {
    let settings = crate::agy_config::read_live_settings().map_err(|e| e.to_string())?;
    let path = tokio::task::spawn_blocking(|| {
        crate::agy_config::real_agy_bin(&crate::agy_config::agy_dir().join("bin/agy"))
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Antigravity CLI not found; install agy first".to_string())?;
    let mut command = tokio::process::Command::new(path);
    command
        .arg("models")
        .kill_on_drop(true)
        .env_remove("GEMINI_API_KEY")
        .env_remove("GOOGLE_GEMINI_BASE_URL");
    if let Some(env) = settings["env"].as_object() {
        for name in ["GEMINI_API_KEY", "GOOGLE_GEMINI_BASE_URL"] {
            if let Some(value) = env.get(name).and_then(serde_json::Value::as_str) {
                command.env(name, value);
            }
        }
    }
    let output = tokio::time::timeout(std::time::Duration::from_secs(20), command.output())
        .await
        .map_err(|_| "Antigravity model discovery timed out".to_string())?
        .map_err(|_| "Antigravity CLI not found; install agy first".to_string())?;
    if !output.status.success() {
        return Err("Antigravity could not list models. Check the active provider and sign in with agy for official models.".into());
    }
    Ok(parse_agy_models(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_agy_models(output: &str) -> Vec<FetchedModel> {
    let mut names = BTreeMap::new();
    for line in output.lines() {
        let Some((id, name)) = line.trim().split_once('\t') else {
            continue;
        };
        let name = name.trim();
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
            || name.is_empty()
            || name.chars().any(char::is_control)
        {
            continue;
        }
        names
            .entry(id.to_string())
            .or_insert_with(|| name.to_string());
    }
    // Native settings.json stores the display name, unlike the --model flag
    // which accepts the first-column CLI alias. Persisting that alias silently
    // falls back to the runtime's default model in account-login mode.
    names
        .into_values()
        .map(|id| FetchedModel { id, owned_by: None })
        .collect()
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
        let extra_env = [
            ("OPENCODE_CONFIG_DIR", config_dir_env),
            ("OPENCODE_DISABLE_PROJECT_CONFIG", "true".to_string()),
        ];
        let output = super::misc::run_detected_tool_command_with_timeout(
            "opencode",
            &["models"],
            Some(OPENCODE_MODELS_TIMEOUT),
            &extra_env,
            &config_dir,
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
                || model_id
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control())
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
    api_format: Option<String>,
    request_headers: Option<BTreeMap<String, String>>,
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
        api_format.as_deref(),
        request_headers.as_ref(),
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
            "notice: loading models\n/model\nprovider/\nbad provider/model\nprovider/bad model\nprovider/bad\u{1b}[0m\n"
        )
        .is_empty());
    }
}

#[cfg(test)]
mod agy_tests {
    #[test]
    fn agy_model_list_returns_native_setting_names_and_deduplicates_aliases() {
        let models = super::parse_agy_models("Fetching available models...\ngemini-test\tGemini Test\nclaude-test\tClaude Test\ngemini-test\tDuplicate\n");
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["Claude Test", "Gemini Test"]
        );
    }

    #[test]
    fn agy_model_list_skips_malformed_rows_and_empty_or_control_character_names() {
        let models = super::parse_agy_models(
            "notice\n\tNo alias\nbad alias\tName\nempty\t\nspaces\t   \ncontrol\tBad\u{1b}Name\nextra\tName\tColumn\nvalid\t Gemini Test \n",
        );
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "Gemini Test");
    }
}
