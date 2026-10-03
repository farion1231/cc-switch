//! 安全围栏拒绝检测器
//!
//! 检测 LLM 响应中的拒绝模式，触发自动故障转移

use crate::proxy::types::{CustomRule, GuardrailConfig, GuardrailMode, RuleAction, ConfidenceThreshold};
use regex::Regex;
use std::sync::Arc;

/// 拒绝检测结果
#[derive(Debug, Clone, PartialEq)]
pub enum RefusalVerdict {
    /// 无拒绝
    None,
    /// 强拒绝（高置信度）
    Strong(String),
    /// 中等拒绝
    Medium(String),
    /// 弱拒绝（需要额外上下文判断）
    Weak(String),
    /// 启发式检测（异常短响应等）
    Heuristic(String),
}

impl RefusalVerdict {
    /// 是否为拒绝（任何非 None 都算）
    pub fn is_refusal(&self) -> bool {
        !matches!(self, RefusalVerdict::None)
    }

    /// 获取置信度等级（用于优先级排序）
    pub fn confidence_level(&self) -> u8 {
        match self {
            RefusalVerdict::None => 0,
            RefusalVerdict::Heuristic(_) => 1,
            RefusalVerdict::Weak(_) => 2,
            RefusalVerdict::Medium(_) => 3,
            RefusalVerdict::Strong(_) => 4,
        }
    }

    /// 获取拒绝原因描述
    pub fn reason(&self) -> &str {
        match self {
            RefusalVerdict::None => "无拒绝",
            RefusalVerdict::Strong(reason) => reason,
            RefusalVerdict::Medium(reason) => reason,
            RefusalVerdict::Weak(reason) => reason,
            RefusalVerdict::Heuristic(reason) => reason,
        }
    }
}

/// 围栏检测器
pub struct GuardrailDetector {
    config: GuardrailConfig,
    /// 编译后的正则表达式缓存（name -> regex）
    compiled_patterns: Vec<(String, Regex)>,
    /// 默认拒绝模式（强拒绝）
    default_strong_patterns: Vec<Regex>,
    /// 默认拒绝模式（中等拒绝）
    default_medium_patterns: Vec<Regex>,
    /// 默认拒绝模式（弱拒绝）
    default_weak_patterns: Vec<Regex>,
}

impl GuardrailDetector {
    /// 创建新的检测器
    pub fn new(config: GuardrailConfig) -> Result<Self, String> {
        // 编译默认模式
        let default_strong_patterns = Self::compile_default_strong_patterns()?;
        let default_medium_patterns = Self::compile_default_medium_patterns()?;
        let default_weak_patterns = Self::compile_default_weak_patterns()?;

        // 编译自定义规则
        let mut compiled_patterns = Vec::new();
        for rule in &config.custom_rules {
            let flags = if rule.case_insensitive {
                regex::RegexBuilder::new(&rule.pattern)
                    .case_insensitive(true)
            } else {
                regex::RegexBuilder::new(&rule.pattern)
            };

            match flags.build() {
                Ok(re) => {
                    compiled_patterns.push((rule.name.clone(), re));
                }
                Err(e) => {
                    log::warn!("[Guardrail] Failed to compile custom rule '{}': {}", rule.name, e);
                }
            }
        }

        Ok(Self {
            config,
            compiled_patterns,
            default_strong_patterns,
            default_medium_patterns,
            default_weak_patterns,
        })
    }

    /// 检测响应是否包含拒绝内容
    ///
    /// # 参数
    /// - `response_text`: 响应文本内容
    /// - `response_length`: 响应长度（用于启发式检测）
    /// - `has_tool_calls`: 是否包含工具调用（用于启发式检测）
    ///
    /// # 返回
    /// - `RefusalVerdict`: 拒绝检测结果
    pub fn detect_refusal(
        &self,
        response_text: &str,
        response_length: usize,
        has_tool_calls: bool,
    ) -> RefusalVerdict {
        // 如果未启用，返回 None
        if !self.config.enabled {
            return RefusalVerdict::None;
        }

        // 先检测自定义规则（Custom 模式下只用自定义规则）
        if matches!(self.config.mode, GuardrailMode::Custom) {
            if let Some(verdict) = self.check_custom_rules(response_text) {
                return verdict;
            }
            return RefusalVerdict::None;
        }

        // 按模式检测默认模式
        let verdict = match self.config.mode {
            GuardrailMode::Strict => self.check_strict_patterns(response_text),
            GuardrailMode::Loose => self.check_loose_patterns(response_text),
            GuardrailMode::Custom => unreachable!(), // 已在上面处理
        };

        // 如果有拒绝，直接返回
        if verdict.is_refusal() {
            return verdict;
        }

        // 启发式检测（作为兜底）
        self.check_heuristics(response_text, response_length, has_tool_calls)
    }

    /// 检查自定义规则
    fn check_custom_rules(&self, text: &str) -> Option<RefusalVerdict> {
        let mut best_verdict: Option<(RefusalVerdict, i32)> = None;

        for (name, regex) in &self.compiled_patterns {
            if regex.is_match(text) {
                // 找到对应规则获取 action
                if let Some(rule) = self.config.custom_rules.iter().find(|r| &r.name == name) {
                    let verdict = match rule.action {
                        RuleAction::SwitchProvider => RefusalVerdict::Strong(format!(
                            "自定义规则 '{}' 命中",
                            name
                        )),
                        RuleAction::LogOnly => RefusalVerdict::Weak(format!(
                            "自定义规则 '{}' 命中（仅记录）",
                            name
                        )),
                        RuleAction::RejectImmediately => RefusalVerdict::Strong(format!(
                            "自定义规则 '{}' 命中（立即拒绝）",
                            name
                        )),
                    };

                    // 按优先级排序
                    if best_verdict.is_none()
                        || rule.priority > best_verdict.as_ref().map(|(_, p)| p).unwrap_or(&0)
                    {
                        best_verdict = Some((verdict, rule.priority));
                    }
                }
            }
        }

        best_verdict.map(|(verdict, _)| verdict)
    }

    /// 检查严格模式模式（只有强拒绝才算）
    fn check_strict_patterns(&self, text: &str) -> RefusalVerdict {
        for pattern in &self.default_strong_patterns {
            if pattern.is_match(text) {
                return RefusalVerdict::Strong("强拒绝模式匹配".to_string());
            }
        }
        RefusalVerdict::None
    }

    /// 检查宽松模式（强 + 中等拒绝都算）
    fn check_loose_patterns(&self, text: &str) -> RefusalVerdict {
        // 先检查强拒绝
        for pattern in &self.default_strong_patterns {
            if pattern.is_match(text) {
                return RefusalVerdict::Strong("强拒绝模式匹配".to_string());
            }
        }

        // 再检查中等拒绝
        for pattern in &self.default_medium_patterns {
            if pattern.is_match(text) {
                return RefusalVerdict::Medium("中等拒绝模式匹配".to_string());
            }
        }

        RefusalVerdict::None
    }

    /// 启发式检测（作为兜底）
    fn check_heuristics(
        &self,
        text: &str,
        response_length: usize,
        has_tool_calls: bool,
    ) -> RefusalVerdict {
        // 异常短响应 + 无工具调用 → 可能为拒绝
        if response_length < 50 && !has_tool_calls {
            // 进一步检查是否包含典型拒绝词汇
            if text.to_lowercase().contains("can't")
                || text.to_lowercase().contains("cannot")
                || text.contains("无法")
                || text.contains("不能")
            {
                return RefusalVerdict::Heuristic("超短响应且含拒绝词汇".to_string());
            }
        }

        // 重复自我介绍 + 拒绝词汇
        let lower = text.to_lowercase();
        if (lower.contains("as an ai")
            || lower.contains("作为.*?ai")
            || lower.contains("我是.*?ai"))
            && (lower.contains("can't") || lower.contains("cannot") || lower.contains("无法"))
        {
            return RefusalVerdict::Heuristic("AI身份声明且含拒绝词汇".to_string());
        }

        RefusalVerdict::None
    }

    /// 编译默认强拒绝模式
    fn compile_default_strong_patterns() -> Result<Vec<Regex>, String> {
        let patterns = vec![
            r"(?i)I'm\s+sorry.*?I\s+can't",
            r"(?i)I'm\s+afraid\s+I\s+can't",
            r"(?i)I\s+cannot\s+assist",
            r"(?i)I\s+cannot\s+help\s+with",
            r"(?i)I\s+can't\s+help\s+with",
            r"(?i)I'm\s+not\s+able\s+to",
            r"(?i)I\s+apologize.*?but\s+I\s+can't",
            r"抱歉.*?无法",
            r"我无法协助",
            r"我无法帮助",
            r"我不能帮助",
        ];

        patterns
            .into_iter()
            .map(|p| {
                Regex::new(p).map_err(|e| format!("Invalid pattern '{}': {}", p, e))
            })
            .collect()
    }

    /// 编译默认中等拒绝模式
    fn compile_default_medium_patterns() -> Result<Vec<Regex>, String> {
        let patterns = vec![
            r"(?i)I\s+don't\s+feel\s+comfortable",
            r"(?i)This\s+is\s+(illegal|unethical)",
            r"(?i)against.*?guidelines",
            r"(?i)violates.*?policy",
            r"作为.*?AI.*?不能",
            r"这违反.*?政策",
            r"这不符合.*?准则",
        ];

        patterns
            .into_iter()
            .map(|p| {
                Regex::new(p).map_err(|e| format!("Invalid pattern '{}': {}", p, e))
            })
            .collect()
    }

    /// 编译默认弱拒绝模式
    fn compile_default_weak_patterns() -> Result<Vec<Regex>, String> {
        let patterns = vec![
            r"(?i)That's\s+not\s+something\s+I\s+can\s+do",
            r"(?i)Have\s+you\s+considered.*?instead",
            r"我建议.*?替代方案",
            r"我推荐.*?其他",
        ];

        patterns
            .into_iter()
            .map(|p| {
                Regex::new(p).map_err(|e| format!("Invalid pattern '{}': {}", p, e))
            })
            .collect()
    }

    /// 提取响应文本（从 JSON 响应中）
    ///
    /// 尝试从各种可能的响应格式中提取文本内容
    pub fn extract_response_text(response_body: &serde_json::Value) -> String {
        // 尝试 Claude 格式
        if let Some(content) = response_body
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
        {
            return content.to_string();
        }

        // 尝试 OpenAI Chat 格式
        if let Some(choices) = response_body.get("choices").and_then(|c| c.as_array()) {
            if let Some(first) = choices.first() {
                if let Some(msg) = first.get("message") {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                        return content.to_string();
                    }
                }
            }
        }

        // 尝试直接获取 text 字段
        if let Some(text) = response_body.get("text").and_then(|t| t.as_str()) {
            return text.to_string();
        }

        // 兜底：序列化整个响应
        response_body.to_string()
    }

    /// 检查响应是否需要触发故障转移
    ///
    /// 综合考虑配置的置信度阈值和检测结果
    pub fn should_trigger_failover(&self, verdict: &RefusalVerdict) -> bool {
        if !self.config.enabled {
            return false;
        }

        let threshold = self.config.confidence_threshold.as_value();

        match verdict {
            RefusalVerdict::None => false,
            RefusalVerdict::Heuristic(_) => threshold <= 1,
            RefusalVerdict::Weak(_) => threshold <= 2,
            RefusalVerdict::Medium(_) => threshold <= 3,
            RefusalVerdict::Strong(_) => true, // 强拒绝总是触发
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strong_refusal_detection() {
        let config = GuardrailConfig {
            enabled: true,
            mode: GuardrailMode::Strict,
            confidence_threshold: ConfidenceThreshold::Medium,
            custom_rules: Vec::new(),
        };

        let detector = GuardrailDetector::new(config).unwrap();

        // 强拒绝案例
        assert!(detector
            .detect_refusal("I'm sorry, I can't help with that", 50, false)
            .is_refusal());

        assert!(detector
            .detect_refusal("我无法协助这个请求", 20, false)
            .is_refusal());
    }

    #[test]
    fn test_heuristic_detection() {
        let config = GuardrailConfig {
            enabled: true,
            mode: GuardrailMode::Loose,
            confidence_threshold: ConfidenceThreshold::Low,
            custom_rules: Vec::new(),
        };

        let detector = GuardrailDetector::new(config).unwrap();

        // 启发式：超短响应 + 拒绝词汇
        assert!(detector
            .detect_refusal("I can't do this.", 15, false)
            .is_refusal());

        // 正常响应（即使短但不含拒绝词汇）
        assert!(!detector
            .detect_refusal("OK", 2, false)
            .is_refusal());
    }

    #[test]
    fn test_disabled_detector() {
        let config = GuardrailConfig {
            enabled: false, // 未启用
            mode: GuardrailMode::Strict,
            confidence_threshold: ConfidenceThreshold::Medium,
            custom_rules: Vec::new(),
        };

        let detector = GuardrailDetector::new(config).unwrap();

        // 即使有明显拒绝，也不检测
        assert!(!detector
            .detect_refusal("I'm sorry, I can't help", 50, false)
            .is_refusal());
    }

    #[test]
    fn test_custom_rules() {
        let config = GuardrailConfig {
            enabled: true,
            mode: GuardrailMode::Custom,
            confidence_threshold: ConfidenceThreshold::Medium,
            custom_rules: vec![CustomRule {
                name: "cybersecurity_refusal".to_string(),
                pattern: r"(?i)(security testing|penetration test|unauthorized access)".to_string(),
                action: RuleAction::SwitchProvider,
                priority: 10,
                case_insensitive: true,
            }],
        };

        let detector = GuardrailDetector::new(config).unwrap();

        // 自定义规则应该匹配
        assert!(detector
            .detect_refusal("I cannot assist with penetration testing", 50, false)
            .is_refusal());
    }
}
