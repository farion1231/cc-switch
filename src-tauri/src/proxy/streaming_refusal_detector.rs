//! 流式响应围栏检测
//!
//! 逐块检测流式响应中的拒绝模式

use crate::proxy::guardrail_detector::{RefusalVerdict, GuardrailDetector};
use bytes::Bytes;
use futures::StreamExt;

/// 流式响应拒绝检测结果
#[derive(Debug, Clone)]
pub struct StreamingRefusalResult {
    /// 是否检测到拒绝
    pub is_refusal: bool,
    /// 拒绝原因（如果检测到）
    pub reason: Option<String>,
    /// 检测到的拒绝类型
    pub refusal_type: Option<String>,
    /// 检测拒绝时已接收的字符数
    pub chars_received: usize,
}

/// 流式响应缓冲区
///
/// 累积流式响应块，定期检测拒绝模式
pub struct StreamingRefusalBuffer {
    /// 累积的文本内容
    buffer: String,
    /// 最大缓冲区大小（字符数）
    max_buffer_size: usize,
    /// 检测间隔（字符数）
    detection_interval: usize,
    /// 已接收字符计数
    chars_received: usize,
    /// 是否已经检测到拒绝
    refusal_detected: bool,
}

impl StreamingRefusalBuffer {
    /// 创建新的缓冲区
    pub fn new(max_buffer_size: usize, detection_interval: usize) -> Self {
        Self {
            buffer: String::new(),
            max_buffer_size,
            detection_interval,
            chars_received: 0,
            refusal_detected: false,
        }
    }

    /// 添加新的数据块
    ///
    /// 返回：是否应该检测拒绝（达到检测间隔）
    pub fn add_chunk(&mut self, chunk: &str) -> bool {
        if self.refusal_detected {
            return false; // 已经检测到拒绝，不再继续
        }

        self.buffer.push_str(chunk);
        self.chars_received += chunk.len();

        // 检查是否应该触发检测
        self.chars_received % self.detection_interval == 0
    }

    /// 检测拒绝（使用提供的检测器）
    pub fn detect_refusal(&self, detector: &GuardrailDetector) -> StreamingRefusalResult {
        // 检查工具调用（流式响应通常不包含完整工具调用信息）
        let has_tool_calls = false;

        let verdict = detector.detect_refusal(
            &self.buffer,
            self.chars_received,
            has_tool_calls,
        );

        StreamingRefusalResult {
            is_refusal: verdict.is_refusal(),
            reason: if verdict.is_refusal() {
                Some(verdict.reason().to_string())
            } else {
                None
            },
            refusal_type: if verdict.is_refusal() {
                Some(format!("{:?}", verdict))
            } else {
                None
            },
            chars_received: self.chars_received,
        }
    }

    /// 标记拒绝已检测（停止后续检测）
    pub fn mark_refusal_detected(&mut self) {
        self.refusal_detected = true;
    }

    /// 获取当前缓冲区内容
    pub fn get_buffer(&self) -> &str {
        &self.buffer
    }

    /// 获取已接收字符数
    pub fn chars_received(&self) -> usize {
        self.chars_received
    }

    /// 清空缓冲区
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.chars_received = 0;
        self.refusal_detected = false;
    }
}

/// 流式响应拒绝检测器
pub struct StreamingRefusalDetector {
    detector: GuardrailDetector,
    /// 缓冲区配置
    max_buffer_size: usize,
    detection_interval: usize,
}

impl StreamingRefusalDetector {
    /// 创建新的流式检测器
    pub fn new(detector: GuardrailDetector, max_buffer_size: usize, detection_interval: usize) -> Self {
        Self {
            detector,
            max_buffer_size,
            detection_interval,
        }
    }

    /// 检测流式响应中的拒绝
    ///
    /// 参数：
    /// - `buffer`: 当前累积的响应文本
    /// - `has_tool_calls`: 是否包含工具调用
    ///
    /// 返回：拒绝检测结果
    pub fn detect_refusal(&self, buffer: &str, has_tool_calls: bool) -> RefusalVerdict {
        self.detector.detect_refusal(buffer, buffer.len(), has_tool_calls)
    }

    /// 处理流式响应块
    ///
    /// 返回：(是否应该停止流, 拒绝原因)
    pub fn process_chunk(
        &self,
        chunk: &str,
        accumulated_buffer: &str,
        has_tool_calls: bool,
    ) -> (bool, Option<String>) {
        let verdict = self.detect_refusal(accumulated_buffer, has_tool_calls);

        if self.detector.should_trigger_failover(&verdict) {
            return (true, Some(verdict.reason().to_string()));
        }

        (false, None)
    }

    /// 快速检测（用于早期拒绝识别）
    ///
    /// 在收到的前 N 个字符中检测明显的拒绝模式
    pub fn quick_detect(&self, prefix: &str) -> Option<RefusalVerdict> {
        // 只检测强拒绝模式（性能优先）
        let verdict = self.detector.detect_refusal(prefix, prefix.len(), false);

        if matches!(verdict, RefusalVerdict::Strong(_)) {
            Some(verdict)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::types::{GuardrailConfig, GuardrailMode, ConfidenceThreshold};

    fn create_test_detector() -> GuardrailDetector {
        let config = GuardrailConfig {
            enabled: true,
            mode: GuardrailMode::Loose,
            confidence_threshold: ConfidenceThreshold::Medium,
            custom_rules: Vec::new(),
        };
        GuardrailDetector::new(config).unwrap()
    }

    #[test]
    fn test_buffer_add_chunk() {
        let mut buffer = StreamingRefusalBuffer::new(1000, 100);

        // 添加少于检测间隔的块
        assert!(!buffer.add_chunk("Hello "));
        assert!(!buffer.add_chunk("world"));

        // 添加到检测间隔
        assert!(buffer.add_chunk(&"x".repeat(95))); // 总共 100 字符

        assert_eq!(buffer.chars_received(), 102);
    }

    #[test]
    fn test_buffer_detection() {
        let detector = create_test_detector();
        let mut buffer = StreamingRefusalBuffer::new(1000, 50);

        // 添加正常文本
        buffer.add_chunk("This is a normal response");

        let result = buffer.detect_refusal(&detector);
        assert!(!result.is_refusal);

        // 添加拒绝文本
        buffer.clear();
        buffer.add_chunk("I'm sorry, I can't help with that");

        let result = buffer.detect_refusal(&detector);
        assert!(result.is_refusal);
    }

    #[test]
    fn test_streaming_detector() {
        let detector = create_test_detector();
        let streaming_detector = StreamingRefusalDetector::new(detector, 1000, 50);

        // 测试快速检测
        let quick_result = streaming_detector.quick_detect("I'm sorry, I can't");
        assert!(quick_result.is_some());
        assert!(quick_result.unwrap().is_refusal());

        // 测试正常前缀
        let quick_result = streaming_detector.quick_detect("Sure, I can help");
        assert!(quick_result.is_none());
    }

    #[test]
    fn test_process_chunk() {
        let detector = create_test_detector();
        let streaming_detector = StreamingRefusalDetector::new(detector, 1000, 50);

        // 正常响应
        let (should_stop, reason) = streaming_detector.process_chunk(
            " with that",
            "Sure, I can help",
            false,
        );
        assert!(!should_stop);
        assert!(reason.is_none());

        // 拒绝响应
        let (should_stop, reason) = streaming_detector.process_chunk(
            " with that",
            "I'm sorry, I can't help",
            false,
        );
        assert!(should_stop);
        assert!(reason.is_some());
    }
}
