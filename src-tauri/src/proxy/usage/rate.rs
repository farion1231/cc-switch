//! 实时模型输出速率监控（滑动窗口采样）+ 会话用量 / 上下文统计。
//!
//! 采样粒度是「一次请求」：请求完成时记录其输出 token 总数与流式耗时，
//! 速率 = 窗口内输出 token / 活跃时长（各请求耗时之和与首尾跨度取大者）。
//! 这样单请求不会再因 span=0 被按 1 秒估算出荒谬的瞬时速率，
//! 并发请求也能正确累加吞吐。
//!
//! 除窗口速率外，另维护：
//! - 最近一次请求的输入（上下文长度）与输出、真实耗时 → 展示「上下文 Xk」与「上次 Y tok/s」
//!   （只认正式请求，后台小请求不覆盖）；
//! - 会话累计的输入 / 输出 / 缓存用量 → 展示「当前的用量」；
//! - 流式在途的实时输出（宽字符加权估算）→ 生成中显示实时速率。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use serde::Serialize;

use super::parser::TokenUsage;

/// 采样窗口时长（秒）
const WINDOW_SECS: u64 = 60;
/// 实时「生成中」判定：超过该时长没有新文本增量视为已结束
const GENERATING_IDLE_MS: u64 = 3000;
/// 「上次请求」展示的最小输出 token：低于该值的多为后台小请求
/// （Claude Code 的 haiku 主题判定、配额探测等，输出只有几个 token），
/// 不覆盖「上次耗时 / 上下文 / 速率」展示，避免主请求刚结束就被顶掉。
/// 仍计入窗口速率与会话用量。
const LAST_REQUEST_MIN_OUTPUT: u64 = 50;

struct RateSample {
    at_ms: u64,
    /// 该请求自身的流式耗时（扣除首字等待；首个样本被用于推算窗口起点）
    duration_ms: u64,
    output_tokens: u64,
}

static SAMPLES: Lazy<Mutex<VecDeque<RateSample>>> = Lazy::new(|| Mutex::new(VecDeque::new()));
static LAST_MODEL: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
/// 累计 token 消耗（input + output + cache，进程生命周期内）
static TOTAL_TOKENS: AtomicU64 = AtomicU64::new(0);
/// 最近一次请求的耗时（毫秒）
static LAST_DURATION_MS: AtomicU64 = AtomicU64::new(0);
/// 最近一次请求的纯流式耗时（总耗时扣除首字等待；供速率分母使用）
static LAST_STREAM_MS: AtomicU64 = AtomicU64::new(0);
/// 已经出现过「正式请求」（用于小请求兜底：会话里只有小请求时也展示其数值）
static LAST_REQUEST_SEEN: AtomicBool = AtomicBool::new(false);
/// 最近一次请求的输入（= 上下文长度）
static LAST_INPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
/// 最近一次请求的输出
static LAST_OUTPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
/// 会话累计用量（进程启动以来，分输入 / 输出 / 缓存）
static SESSION_INPUT: AtomicU64 = AtomicU64::new(0);
static SESSION_OUTPUT: AtomicU64 = AtomicU64::new(0);
static SESSION_CACHE: AtomicU64 = AtomicU64::new(0);
/// 流式实时输出（毫 token，宽字符加权估算；仅用于生成中实时速率展示）
static LIVE_OUTPUT_MILLI: AtomicU64 = AtomicU64::new(0);
/// 最近一次实时增量的时间（epoch ms；0 = 无在途输出）
static LIVE_LAST_AT: AtomicU64 = AtomicU64::new(0);
/// 当前突发（一次流式输出）的起点
static LIVE_BURST_START: Mutex<Option<Instant>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 宽字符（CJK / 谚文 / 全角）判定：这类字符约 1 字符 = 1 token，
/// 而拉丁文本约 4 字符 = 1 token。统一按「毫 token」加权累计，
/// 避免中文输出把实时速率低估约 4 倍。
fn is_wide_char(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x11FF   // Hangul Jamo
        | 0x2E80..=0x303F // CJK 部首 / 标点
        | 0x3040..=0x30FF // 平假名 / 片假名
        | 0x3130..=0x318F // Hangul Compatibility Jamo
        | 0x3400..=0x4DBF // CJK 扩展 A
        | 0x4E00..=0x9FFF // CJK 统一表意文字
        | 0xA960..=0xA97F // Hangul Jamo Extended-A
        | 0xAC00..=0xD7FF // Hangul 音节
        | 0xF900..=0xFAFF // CJK 兼容表意文字
        | 0xFF00..=0xFFEF // 全角 / 半角形式
        | 0x20000..=0x2FA1F // CJK 扩展 B~F
    )
}

/// 字符数 → 毫 token（千分之一 token）：宽字符 1000/字，其余 250/字。
fn estimate_milli_tokens(text: &str) -> u64 {
    let mut milli = 0u64;
    for ch in text.chars() {
        milli += if is_wide_char(ch) { 1000 } else { 250 };
    }
    milli
}

/// 流式事件到达时记录文本增量（宽字符加权的毫 token 估算；权威计数仍以
/// 请求结束时的 usage 为准）。在透传流的每个 SSE 事件上调用，
/// 用于生成中显示实时速率。
pub fn record_live_delta(event: &serde_json::Value) {
    let mut milli = 0u64;
    // Claude 系：{"type":"content_block_delta","delta":{"type":"text_delta","text":"…"}}
    if let Some(text) = event.pointer("/delta/text").and_then(|v| v.as_str()) {
        milli += estimate_milli_tokens(text);
    }
    // OpenAI / opencode 系：{"choices":[{"delta":{"content":"…"}}]}
    if let Some(choices) = event.get("choices").and_then(|v| v.as_array()) {
        for choice in choices {
            if let Some(text) = choice.pointer("/delta/content").and_then(|v| v.as_str()) {
                milli += estimate_milli_tokens(text);
            }
        }
    }
    // Gemini 系：{"candidates":[{"content":{"parts":[{"text":"…"}]}}]}
    if let Some(candidates) = event.get("candidates").and_then(|v| v.as_array()) {
        for cand in candidates {
            if let Some(parts) = cand.pointer("/content/parts").and_then(|v| v.as_array()) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        milli += estimate_milli_tokens(text);
                    }
                }
            }
        }
    }
    if milli == 0 {
        return;
    }
    let now = now_ms();
    // swap 出上一次增量的时间：若间隔超过 GENERATING_IDLE_MS，说明旧流已死
    // （可能未走到 record_output 就断流），本次增量视为新突发的开始，
    // 避免残留计数把下一次生成的实时速率拖低
    let prev_at = LIVE_LAST_AT.swap(now, Ordering::Relaxed);
    let burst_expired = prev_at > 0 && now.saturating_sub(prev_at) > GENERATING_IDLE_MS;
    LIVE_OUTPUT_MILLI.fetch_add(milli, Ordering::Relaxed);
    let mut start = LIVE_BURST_START.lock().unwrap();
    if start.is_none() || burst_expired {
        if burst_expired {
            LIVE_OUTPUT_MILLI.store(milli, Ordering::Relaxed);
        }
        *start = Some(Instant::now());
    }
}

/// 记录一次成功请求的用量（供速率统计与用量展示）。
///
/// `duration_ms` 为该请求从发出到收到完整响应的耗时；`first_token_ms`
/// 为到上游首包的耗时（流式请求才有）：速率分母用扣除首字等待后的
/// 纯流式时长——等待首包期间没有 token 产出，计入分母会把速率拉低。
///
/// `model` 为实际发往上游的模型名（映射后的真值），用于「上次请求」展示。
pub fn record_output(
    usage: &TokenUsage,
    duration_ms: u64,
    model: &str,
    first_token_ms: Option<u64>,
) {
    // 模型名先于任何提前返回写入：即使 0 输出也更新「上次请求」展示
    *LAST_MODEL.lock().unwrap() = if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    };
    let total = usage.input_tokens as u64
        + usage.output_tokens as u64
        + usage.cache_read_tokens as u64
        + usage.cache_creation_tokens as u64;
    TOTAL_TOKENS.fetch_add(total, Ordering::Relaxed);
    // 请求结束：更新会话累计。放在 output==0 提前返回之前，
    // 保证缓存命中（0 输出）也计入用量。
    SESSION_INPUT.fetch_add(usage.input_tokens as u64, Ordering::Relaxed);
    SESSION_OUTPUT.fetch_add(usage.output_tokens as u64, Ordering::Relaxed);
    SESSION_CACHE.fetch_add(
        usage.cache_read_tokens as u64 + usage.cache_creation_tokens as u64,
        Ordering::Relaxed,
    );

    if usage.output_tokens == 0 {
        return;
    }

    // 纯流式时长：总耗时扣除首字等待（异常值时退回总耗时）
    let stream_ms = match first_token_ms {
        Some(ttfb) if ttfb > 0 && ttfb < duration_ms => duration_ms - ttfb,
        _ => duration_ms,
    };

    // 「上次请求」展示只认正式请求：后台小请求（haiku 判题、配额探测）
    // 不覆盖主请求留下的耗时 / 上下文 / 速率。会话里只有小请求时仍兜底展示。
    let significant = usage.output_tokens >= LAST_REQUEST_MIN_OUTPUT as u32
        || !LAST_REQUEST_SEEN.load(Ordering::Relaxed);
    if significant {
        LAST_REQUEST_SEEN.store(true, Ordering::Relaxed);
        LAST_DURATION_MS.store(duration_ms, Ordering::Relaxed);
        LAST_STREAM_MS.store(stream_ms, Ordering::Relaxed);
        LAST_INPUT_TOKENS.store(usage.input_tokens as u64, Ordering::Relaxed);
        LAST_OUTPUT_TOKENS.store(usage.output_tokens as u64, Ordering::Relaxed);
    }

    let now = now_ms();
    let sample = RateSample {
        at_ms: now,
        duration_ms: stream_ms,
        output_tokens: usage.output_tokens as u64,
    };
    let mut samples = SAMPLES.lock().unwrap();
    samples.push_back(sample);
    let cutoff = now.saturating_sub(WINDOW_SECS * 1000);
    while samples.front().map(|s| s.at_ms < cutoff).unwrap_or(false) {
        samples.pop_front();
    }
}

/// 实时速率 / 用量信息（返回给前端）。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelRateInfo {
    /// 窗口平均输出速率（60s 滑动窗口，token/s）
    pub tokens_per_second: f64,
    /// 窗口内累计输出 token
    pub output_tokens: u64,
    pub sample_seconds: u64,
    /// 累计 token 消耗（含输入 / 缓存，进程启动以来）
    pub total_tokens: u64,
    /// 最近一次请求耗时（毫秒；无请求时为 0）
    pub last_duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_model: Option<String>,
    /// 最近一次请求的输入 token（= 上下文长度；无请求时为 0）
    pub last_input_tokens: u64,
    /// 最近一次请求的输出 token
    pub last_output_tokens: u64,
    /// 最近一次请求的真实速率（token/s；last_output / last_duration）
    pub last_speed_tok_s: f64,
    /// 会话累计输入 / 输出 / 缓存用量（进程启动以来）
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub session_cache_tokens: u64,
    /// 当前是否有流式输出在途（近 3s 内有文本增量）
    pub generating: bool,
    /// 生成中的实时速率（宽字符加权估算：CJK 约 1 字符 ≈ 1 token，
    /// 其余约 4 字符 ≈ 1 token）
    pub live_speed_tok_s: f64,
}

/// 当前实时速率。
pub fn current_rate() -> ModelRateInfo {
    let now = now_ms();
    let cutoff = now.saturating_sub(WINDOW_SECS * 1000);
    let mut info = ModelRateInfo::default();
    {
        let mut samples = SAMPLES.lock().unwrap();
        // 读取时按当前时间清理过期样本：空闲 60s 后速率归零，
        // 而不是冻结在最后一次请求的数值上
        while samples.front().map(|s| s.at_ms < cutoff).unwrap_or(false) {
            samples.pop_front();
        }
        if let Some(first) = samples.front() {
            let last = samples.back().unwrap();
            let output = samples.iter().map(|s| s.output_tokens).sum::<u64>();
            // 活跃时长：各请求自身流式耗时之和（并发请求正确累加），
            // 与「首个请求起点 → 最后一个样本」的窗口跨度取大者
            let duration_sum_ms = samples.iter().map(|s| s.duration_ms).sum::<u64>();
            let earliest_start = first.at_ms.saturating_sub(first.duration_ms);
            let span_ms = last.at_ms.saturating_sub(earliest_start);
            let active_secs = (duration_sum_ms.max(span_ms) as f64 / 1000.0).max(1.0);
            info.tokens_per_second = output as f64 / active_secs;
            info.output_tokens = output;
            info.sample_seconds = (span_ms / 1000).max(1);
        }
    }
    info.last_model = LAST_MODEL.lock().unwrap().clone();
    info.total_tokens = TOTAL_TOKENS.load(Ordering::Relaxed);
    info.last_duration_ms = LAST_DURATION_MS.load(Ordering::Relaxed);
    info.last_input_tokens = LAST_INPUT_TOKENS.load(Ordering::Relaxed);
    info.last_output_tokens = LAST_OUTPUT_TOKENS.load(Ordering::Relaxed);
    // 速率分母用纯流式时长（总耗时扣除首字等待）
    let last_stream = LAST_STREAM_MS.load(Ordering::Relaxed);
    info.last_speed_tok_s = if last_stream > 0 && info.last_output_tokens > 0 {
        info.last_output_tokens as f64 * 1000.0 / last_stream as f64
    } else {
        0.0
    };
    info.session_input_tokens = SESSION_INPUT.load(Ordering::Relaxed);
    info.session_output_tokens = SESSION_OUTPUT.load(Ordering::Relaxed);
    info.session_cache_tokens = SESSION_CACHE.load(Ordering::Relaxed);
    let live_milli = LIVE_OUTPUT_MILLI.load(Ordering::Relaxed);
    let live_at = LIVE_LAST_AT.load(Ordering::Relaxed);
    info.generating = live_at > 0 && now.saturating_sub(live_at) < GENERATING_IDLE_MS;
    if info.generating && live_milli > 0 {
        let start = *LIVE_BURST_START.lock().unwrap().get_or_insert_with(Instant::now);
        let secs = start.elapsed().as_secs_f64().max(0.5);
        // 毫 token → token（宽字符加权估算，仅用于生成中实时展示）
        info.live_speed_tok_s = live_milli as f64 / 1000.0 / secs;
    }
    info
}

#[tauri::command]
pub fn get_model_rate() -> ModelRateInfo {
    current_rate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_chars_count_as_full_tokens() {
        // 4 个汉字 ≈ 4 token；旧实现按 4 字符/token 只会估出 1
        assert_eq!(estimate_milli_tokens("你好世界"), 4000);
    }

    #[test]
    fn ascii_chars_count_as_quarter_tokens() {
        assert_eq!(estimate_milli_tokens("abcd"), 1000);
        assert_eq!(estimate_milli_tokens(""), 0);
    }

    #[test]
    fn mixed_text_weights_by_script() {
        // 2 个汉字 + 4 个字母 → 2000 + 1000 = 3000 毫 token
        assert_eq!(estimate_milli_tokens("两ab字cd"), 3000);
    }
}
