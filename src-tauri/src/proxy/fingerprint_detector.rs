//! 指纹检测集成
//!
//! 集成 llm-fingerprint-detector 进行异步模型身份验证

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 指纹检测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintVerificationResult {
    /// 是否匹配（后端模型与声称的模型一致）
    pub is_match: bool,

    /// Jensen-Shannon 散度（不匹配程度）
    pub jsd: f64,

    /// 判定等级
    pub verdict: String,

    /// 检测到的实际模型（如果检测到）
    pub detected_model: Option<String>,

    /// 错误信息（如果检测失败）
    pub error: Option<String>,
}

impl FingerprintVerificationResult {
    /// 是否需要触发故障转移
    pub fn should_trigger_failover(&self) -> bool {
        match self.verdict.as_str() {
            "match" => false,
            "uncertain" | "mismatch" => true,
            _ => true, // 默认触发
        }
    }

    /// 是否成功验证
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

/// 指纹检测器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerprintDetectorConfig {
    /// 是否启用指纹检测
    pub enabled: bool,

    /// npx llm-fingerprint-detector 命令路径
    pub command_path: String,

    /// 检测超时（秒）
    pub timeout_secs: u64,

    /// 抽样检测率（0.0-1.0，1.0 = 全部检测）
    pub sampling_rate: f32,

    /// 只在怀疑拒绝时检测（true）或定期检测（false）
    pub detect_on_refusal_only: bool,
}

impl Default for FingerprintDetectorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command_path: "llm-fingerprint-detector".to_string(),
            timeout_secs: 30,
            sampling_rate: 0.1, // 10% 抽样
            detect_on_refusal_only: true,
        }
    }
}

/// 指纹检测器
pub struct FingerprintDetector {
    config: FingerprintDetectorConfig,
    /// 已知的良好指纹（模型名 -> 指纹数据）
    known_fingerprints: Arc<RwLock<std::collections::HashMap<String, serde_json::Value>>>,
}

impl FingerprintDetector {
    /// 创建新的指纹检测器
    pub fn new(config: FingerprintDetectorConfig) -> Self {
        Self {
            config,
            known_fingerprints: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// 注册已知良好的指纹
    pub async fn register_fingerprint(&self, model_name: String, fingerprint_data: serde_json::Value) {
        let mut fingerprints = self.known_fingerprints.write().await;
        fingerprints.insert(model_name, fingerprint_data);
        log::info!("[Fingerprint] Registered fingerprint for model");
    }

    /// 验证供应商模型身份
    ///
    /// 参数：
    /// - `claimed_model`: 声称的模型名称
    /// - `base_url`: 供应商 base URL
    /// - `api_key`: API 密钥
    pub async fn verify_provider(
        &self,
        claimed_model: &str,
        base_url: &str,
        api_key: &str,
    ) -> FingerprintVerificationResult {
        if !self.config.enabled {
            return FingerprintVerificationResult {
                is_match: true,
                jsd: 0.0,
                verdict: "match".to_string(),
                detected_model: Some(claimed_model.to_string()),
                error: None,
            };
        }

        // 抽样检查（如果不是 100% 检测）
        if !self.should_sample() {
            return FingerprintVerificationResult {
                is_match: true,
                jsd: 0.0,
                verdict: "match".to_string(),
                detected_model: Some(claimed_model.to_string()),
                error: None,
            };
        }

        // 使用 llm-fingerprint-detector 命令行工具进行检测
        let result = tokio::task::spawn_blocking(|| {
            Self::run_fingerprint_command(claimed_model, base_url, api_key)
        })
        .await
        .unwrap_or_else(|_| FingerprintVerificationResult {
            is_match: false,
            jsd: 1.0,
            verdict: "error".to_string(),
            detected_model: None,
            error: Some("Task execution failed".to_string()),
        });

        result
    }

    /// 执行指纹检测命令
    fn run_fingerprint_command(
        claimed_model: &str,
        base_url: &str,
        api_key: &str,
    ) -> FingerprintVerificationResult {
        // 构造命令
        let output = Command::new("npx")
            .arg(&self.config.command_path)
            .arg("verify")
            .arg("--base-url")
            .arg(base_url)
            .arg("--model")
            .arg(claimed_model)
            .arg("--apiKey")
            .arg(api_key)
            .output();

        match output {
            Ok(stdout) => {
                let json_str = String::from_utf8_lossy(&stdout);
                if let Ok(result) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    FingerprintVerificationResult {
                        is_match: result["is_match"].as_bool().unwrap_or(false),
                        jsd: result["meanJsd"].as_f64().unwrap_or(1.0),
                        verdict: result["verdict"].as_str().unwrap_or("unknown").to_string(),
                        detected_model: result["detected_model"].as_str().map(|s| s.to_string()),
                        error: None,
                    }
                } else {
                    // JSON 解析失败，可能是命令返回了错误信息
                    FingerprintVerificationResult {
                        is_match: false,
                        jsd: 1.0,
                        verdict: "error".to_string(),
                        detected_model: None,
                        error: Some("Failed to parse detector output".to_string()),
                    }
                }
            }
            Err(stderr) => {
                let error_msg = String::from_utf8_lossy(&stderr.to_bytes());
                FingerprintVerificationResult {
                    is_match: false,
                    jsd: 1.0,
                    verdict: "error".to_string(),
                    detected_model: None,
                    error: Some(error_msg),
                }
            }
        }
    }

    /// 采集供应商的指纹数据
    ///
    /// 用于建立基线指纹
    pub async fn collect_fingerprint(
        &self,
        model_name: &str,
        base_url: &str,
        api_key: &str,
    ) -> Result<serde_json::Value, String> {
        let output = Command::new("npx")
            .arg(&self.config.command_path)
            .arg("fingerprint")
            .arg("--base-url")
            .arg(base_url)
            .arg("--model")
            .arg(model_name)
            .arg("--apiKey")
            .arg(api_key)
            .output();

        match output {
            Ok(stdout) => {
                let json_str = String::from_utf8_lossy(&stdout);
                serde_json::from_str(&json_str).map_err(|e| format!("Failed to parse fingerprint: {}", e))
            }
            Err(stderr) => {
                Err(format!("Command failed: {}", String::from_utf8_lossy(&stderr.to_bytes())))
            }
        }
    }

    /// 是否应该进行抽样检测
    fn should_sample(&self) -> bool {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen::<f32>() < self.config.sampling_rate
    }

    /// 获取配置
    pub fn get_config(&self) -> &FingerprintDetectorConfig {
        &self.config
    }

    /// 更新配置
    pub fn update_config(&mut self, config: FingerprintDetectorConfig) {
        self.config = config;
    }
}

/// 异步指纹验证任务
///
/// 在后台验证供应商模型身份
pub struct AsyncFingerprintVerification {
    detector: Arc<FingerprintDetector>,
    /// 验证队列（待验证的供应商信息）
    verification_queue: Arc<RwLock<Vec<VerificationTask>>>,
}

/// 验证任务
#[derive(Debug, Clone)]
pub struct VerificationTask {
    pub claimed_model: String,
    pub base_url: String,
    pub api_key: String,
    pub priority: i32,
    pub reason: String,
}

impl AsyncFingerprintVerification {
    /// 创建新的异步验证器
    pub fn new(detector: Arc<FingerprintDetector>) -> Self {
        Self {
            detector,
            verification_queue: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 添加验证任务
    pub async fn add_verification_task(&self, task: VerificationTask) {
        let mut queue = self.verification_queue.write().await;
        queue.push(task);
        // 按优先级排序
        queue.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// 处理验证队列
    pub async fn process_queue(&self) -> Vec<FingerprintVerificationResult> {
        let mut results = Vec::new();

        let mut queue = self.verification_queue.write().await;
        let tasks = std::mem::take(&mut *queue);
        drop(queue);

        for task in tasks {
            log::info!(
                "[Fingerprint] Verifying provider: model={}, reason={}",
                task.claimed_model,
                task.reason
            );

            let result = self
                .detector
                .verify_provider(&task.claimed_model, &task.base_url, &task.api_key)
                .await;

            if !result.is_match {
                log::warn!(
                    "[Fingerprint] Provider model mismatch detected! claimed={}, detected={:?}",
                    task.claimed_model,
                    result.detected_model
                );
            }

            results.push(result);
        }

        results
    }

    /// 启动定期验证任务
    pub async fn start_periodic_verification(
        &self,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let verifier = self.clone();
        let interval = tokio::time::Duration::from_secs(interval_secs);

        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;

                let results = verifier.process_queue().await;

                if !results.is_empty() {
                    log::info!(
                        "[Fingerprint] Verified {} providers",
                        results.len()
                    );
                }
            }
        })
    }
}

impl Clone for AsyncFingerprintVerification {
    fn clone(&self) -> Self {
        Self {
            detector: self.detector.clone(),
            verification_queue: self.verification_queue.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verification_result() {
        let result = FingerprintVerificationResult {
            is_match: true,
            jsd: 0.1,
            verdict: "match".to_string(),
            detected_model: Some("gpt-4".to_string()),
            error: None,
        };

        assert!(!result.should_trigger_failover());
        assert!(result.is_success());
    }

    #[test]
    fn test_mismatch_trigger_failover() {
        let result = FingerprintVerificationResult {
            is_match: false,
            jsd: 0.5,
            verdict: "mismatch".to_string(),
            detected_model: Some("gpt-3.5".to_string()),
            error: None,
        };

        assert!(result.should_trigger_failover());
        assert!(result.is_success());
    }

    #[test]
    fn test_error_case() {
        let result = FingerprintVerificationResult {
            is_match: false,
            jsd: 1.0,
            verdict: "error".to_string(),
            detected_model: None,
            error: Some("Detection failed".to_string()),
        };

        assert!(result.should_trigger_failover());
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_fingerprint_detector() {
        let config = FingerprintDetectorConfig {
            enabled: false,
            ..Default::default()
        };

        let detector = FingerprintDetector::new(config);

        // 禁用状态下应该返回 match
        let result = detector
            .verify_provider("gpt-4", "https://api.openai.com", "sk-test")
            .await;

        assert!(result.is_match);
        assert_eq!(result.verdict, "match");
    }

    #[tokio::test]
    async fn test_verification_queue() {
        let config = FingerprintDetectorConfig::default();
        let detector = Arc::new(FingerprintDetector::new(config));
        let verifier = AsyncFingerprintVerification::new(detector);

        // 添加任务
        verifier.add_verification_task(VerificationTask {
            claimed_model: "gpt-4".to_string(),
            base_url: "https://api.openai.com".to_string(),
            api_key: "sk-test".to_string(),
            priority: 10,
            reason: "Routine check".to_string(),
        }).await;

        // 处理队列
        let results = verifier.process_queue().await;

        // 由于命令可能不存在，至少应该有结果
        assert!(!results.is_empty());
    }
}
