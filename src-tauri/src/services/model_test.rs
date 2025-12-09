//! 模型测试服务
//!
//! 提供独立的模型可用性测试功能，复用现有 Provider 适配器逻辑，
//! 但不影响正常代理数据流程。测试结果记录到独立的日志表。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::providers::{get_adapter, AuthInfo, ProviderAdapter};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// 模型测试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestConfig {
    /// 默认测试模型（Claude）
    pub claude_model: String,
    /// 默认测试模型（Codex/OpenAI）
    pub codex_model: String,
    /// 默认测试模型（Gemini）
    pub gemini_model: String,
    /// 测试提示词
    pub test_prompt: String,
    /// 超时时间（秒）
    pub timeout_secs: u64,
}

impl Default for ModelTestConfig {
    fn default() -> Self {
        Self {
            claude_model: "claude-haiku-4-5-20251001".to_string(),
            codex_model: "gpt-5.1-low".to_string(),
            gemini_model: "gemini-3-pro-low".to_string(),
            test_prompt: "ping".to_string(),
            timeout_secs: 15,
        }
    }
}

/// 模型测试结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult {
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub model_used: String,
    pub tested_at: i64,
}

/// 模型测试日志记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestLog {
    pub id: i64,
    pub provider_id: String,
    pub provider_name: String,
    pub app_type: String,
    pub model: String,
    pub prompt: String,
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<i64>,
    pub http_status: Option<i64>,
    pub tested_at: i64,
}

/// 模型测试服务
pub struct ModelTestService;

impl ModelTestService {
    /// 测试单个供应商的模型可用性
    pub async fn test_provider(
        app_type: &AppType,
        provider: &Provider,
        config: &ModelTestConfig,
    ) -> Result<ModelTestResult, AppError> {
        let start = Instant::now();
        let adapter = get_adapter(app_type);

        // 构建 HTTP 客户端（独立于代理服务）
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| AppError::Message(format!("创建 HTTP 客户端失败: {e}")))?;

        // 根据 AppType 选择测试模型
        let model = match app_type {
            AppType::Claude => &config.claude_model,
            AppType::Codex => &config.codex_model,
            AppType::Gemini => &config.gemini_model,
        };

        let result = match app_type {
            AppType::Claude => {
                Self::test_claude(
                    &client,
                    provider,
                    adapter.as_ref(),
                    model,
                    &config.test_prompt,
                )
                .await
            }
            AppType::Codex => {
                Self::test_codex(
                    &client,
                    provider,
                    adapter.as_ref(),
                    model,
                    &config.test_prompt,
                )
                .await
            }
            AppType::Gemini => {
                Self::test_gemini(
                    &client,
                    provider,
                    adapter.as_ref(),
                    model,
                    &config.test_prompt,
                )
                .await
            }
        };

        let response_time = start.elapsed().as_millis() as u64;
        let tested_at = chrono::Utc::now().timestamp();

        match result {
            Ok((status, msg)) => Ok(ModelTestResult {
                success: true,
                message: msg,
                response_time_ms: Some(response_time),
                http_status: Some(status),
                model_used: model.clone(),
                tested_at,
            }),
            Err(e) => Ok(ModelTestResult {
                success: false,
                message: e.to_string(),
                response_time_ms: Some(response_time),
                http_status: None,
                model_used: model.clone(),
                tested_at,
            }),
        }
    }

    /// 测试 Claude (Anthropic Messages API)
    async fn test_claude(
        client: &Client,
        provider: &Provider,
        adapter: &dyn ProviderAdapter,
        model: &str,
        prompt: &str,
    ) -> Result<(u16, String), AppError> {
        let base_url = adapter
            .extract_base_url(provider)
            .map_err(|e| AppError::Message(format!("提取 base_url 失败: {e}")))?;

        let auth = adapter
            .extract_auth(provider)
            .ok_or_else(|| AppError::Message("未找到 API Key".to_string()))?;

        // 智能拼接 URL，避免重复 /v1
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        };

        let body = json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let mut request = client.post(&url).json(&body);
        request = Self::add_claude_auth(request, &auth);

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::Message("请求超时".to_string())
            } else if e.is_connect() {
                AppError::Message(format!("连接失败: {e}"))
            } else {
                AppError::Message(e.to_string())
            }
        })?;

        let status = response.status().as_u16();

        if response.status().is_success() {
            // 先获取文本，再尝试解析 JSON（兼容流式响应）
            let text = response.text().await.unwrap_or_default();

            // 尝试解析 JSON
            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                if data.get("type").is_some()
                    || data.get("content").is_some()
                    || data.get("id").is_some()
                {
                    return Ok((status, "模型测试成功".to_string()));
                }
            }

            // 即使无法解析 JSON，只要状态码是 200 就认为成功
            Ok((status, "模型测试成功".to_string()))
        } else {
            let error_text = response.text().await.unwrap_or_default();
            Err(AppError::Message(format!("HTTP {status}: {error_text}")))
        }
    }

    /// 测试 Codex (OpenAI Chat Completions API)
    async fn test_codex(
        client: &Client,
        provider: &Provider,
        adapter: &dyn ProviderAdapter,
        model: &str,
        prompt: &str,
    ) -> Result<(u16, String), AppError> {
        let base_url = adapter
            .extract_base_url(provider)
            .map_err(|e| AppError::Message(format!("提取 base_url 失败: {e}")))?;

        let auth = adapter
            .extract_auth(provider)
            .ok_or_else(|| AppError::Message("未找到 API Key".to_string()))?;

        // 智能拼接 URL，避免重复 /v1
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        };

        let body = json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "max_tokens": 1,
            "stream": false
        });

        let request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", auth.api_key))
            .header("Content-Type", "application/json")
            .json(&body);

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::Message("请求超时".to_string())
            } else if e.is_connect() {
                AppError::Message(format!("连接失败: {e}"))
            } else {
                AppError::Message(e.to_string())
            }
        })?;

        let status = response.status().as_u16();

        if response.status().is_success() {
            // 先获取文本，再尝试解析 JSON
            let text = response.text().await.unwrap_or_default();

            if let Ok(data) = serde_json::from_str::<Value>(&text) {
                if data.get("choices").is_some() || data.get("id").is_some() {
                    return Ok((status, "模型测试成功".to_string()));
                }
            }

            // 即使无法解析 JSON，只要状态码是 200 就认为成功
            Ok((status, "模型测试成功".to_string()))
        } else {
            let error_text = response.text().await.unwrap_or_default();
            Err(AppError::Message(format!("HTTP {status}: {error_text}")))
        }
    }

    /// 测试 Gemini (Google Generative AI API)
    async fn test_gemini(
        client: &Client,
        provider: &Provider,
        adapter: &dyn ProviderAdapter,
        model: &str,
        prompt: &str,
    ) -> Result<(u16, String), AppError> {
        let base_url = adapter
            .extract_base_url(provider)
            .map_err(|e| AppError::Message(format!("提取 base_url 失败: {e}")))?;

        let auth = adapter
            .extract_auth(provider)
            .ok_or_else(|| AppError::Message("未找到 API Key".to_string()))?;

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            base_url.trim_end_matches('/'),
            model,
            auth.api_key
        );

        let body = json!({
            "contents": [{
                "parts": [{
                    "text": prompt
                }]
            }],
            "generationConfig": {
                "maxOutputTokens": 1
            }
        });

        let request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body);

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                AppError::Message("请求超时".to_string())
            } else if e.is_connect() {
                AppError::Message(format!("连接失败: {e}"))
            } else {
                AppError::Message(e.to_string())
            }
        })?;

        let status = response.status().as_u16();

        if response.status().is_success() {
            let data: Value = response
                .json()
                .await
                .map_err(|e| AppError::Message(format!("解析响应失败: {e}")))?;

            if data.get("candidates").is_some() {
                Ok((status, "模型测试成功".to_string()))
            } else {
                Err(AppError::Message("响应格式异常".to_string()))
            }
        } else {
            let error_text = response.text().await.unwrap_or_default();
            Err(AppError::Message(format!("HTTP {status}: {error_text}")))
        }
    }

    /// 添加 Claude 认证头
    fn add_claude_auth(
        request: reqwest::RequestBuilder,
        auth: &AuthInfo,
    ) -> reqwest::RequestBuilder {
        request
            .header("x-api-key", &auth.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
    }
}

// ===== 数据库操作 =====

impl Database {
    /// 保存模型测试日志
    pub fn save_model_test_log(
        &self,
        provider_id: &str,
        provider_name: &str,
        app_type: &str,
        model: &str,
        prompt: &str,
        result: &ModelTestResult,
    ) -> Result<i64, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Database(format!("获取数据库连接失败: {e}")))?;

        conn.execute(
            "INSERT INTO model_test_logs 
             (provider_id, provider_name, app_type, model, prompt, success, message, response_time_ms, http_status, tested_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                provider_id,
                provider_name,
                app_type,
                model,
                prompt,
                result.success,
                result.message,
                result.response_time_ms.map(|t| t as i64),
                result.http_status.map(|s| s as i64),
                result.tested_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(conn.last_insert_rowid())
    }

    /// 获取模型测试日志
    pub fn get_model_test_logs(
        &self,
        app_type: Option<&str>,
        provider_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<ModelTestLog>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Database(format!("获取数据库连接失败: {e}")))?;

        let mut sql = String::from(
            "SELECT id, provider_id, provider_name, app_type, model, prompt, success, message, response_time_ms, http_status, tested_at
             FROM model_test_logs WHERE 1=1"
        );

        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(at) = app_type {
            sql.push_str(" AND app_type = ?");
            params.push(Box::new(at.to_string()));
        }

        if let Some(pid) = provider_id {
            sql.push_str(" AND provider_id = ?");
            params.push(Box::new(pid.to_string()));
        }

        sql.push_str(" ORDER BY tested_at DESC LIMIT ?");
        params.push(Box::new(limit as i64));

        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| AppError::Database(e.to_string()))?;

        let logs = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(ModelTestLog {
                    id: row.get(0)?,
                    provider_id: row.get(1)?,
                    provider_name: row.get(2)?,
                    app_type: row.get(3)?,
                    model: row.get(4)?,
                    prompt: row.get(5)?,
                    success: row.get(6)?,
                    message: row.get(7)?,
                    response_time_ms: row.get(8)?,
                    http_status: row.get(9)?,
                    tested_at: row.get(10)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(logs)
    }

    /// 获取模型测试配置
    pub fn get_model_test_config(&self) -> Result<ModelTestConfig, AppError> {
        match self.get_setting("model_test_config")? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| AppError::Message(format!("解析模型测试配置失败: {e}"))),
            None => Ok(ModelTestConfig::default()),
        }
    }

    /// 保存模型测试配置
    pub fn save_model_test_config(&self, config: &ModelTestConfig) -> Result<(), AppError> {
        let json = serde_json::to_string(config)
            .map_err(|e| AppError::Message(format!("序列化模型测试配置失败: {e}")))?;
        self.set_setting("model_test_config", &json)
    }

    /// 清理旧的测试日志（保留最近 N 条）
    pub fn cleanup_model_test_logs(&self, keep_count: u32) -> Result<u64, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| AppError::Database(format!("获取数据库连接失败: {e}")))?;

        let deleted = conn
            .execute(
                "DELETE FROM model_test_logs WHERE id NOT IN (
                SELECT id FROM model_test_logs ORDER BY tested_at DESC LIMIT ?
            )",
                rusqlite::params![keep_count as i64],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(deleted as u64)
    }
}
