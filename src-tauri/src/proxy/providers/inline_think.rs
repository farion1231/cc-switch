//! Chat 兼容上游把思考内容内联进 content 时的剥离逻辑。
//!
//! Claude 路径（Chat → Anthropic）与 Codex 路径（Chat → Responses）共用：
//! 流式走 [`InlineThinkSplitter`]，非流式走 [`split_leading_think_block`]。
//! 只识别正文开头的块。

/// 内联思考标签：`<think>`（MiniMax M3 等）与 `<thinking>`（DeepSeek 系等）。
const INLINE_THINK_TAG_PAIRS: [(&str, &str); 2] =
    [("<think>", "</think>"), ("<thinking>", "</thinking>")];

/// 标签与内容之间的分隔符（开标签之后、闭合标签前后）。
/// 只认换行：空格、制表符可能是正文的缩进。
const THINK_SEPARATORS: [char; 2] = ['\r', '\n'];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum InlineThinkMode {
    #[default]
    Detecting,
    Reasoning,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkPrefixDecision {
    NeedMore,
    Reasoning,
    Text,
}

/// 判断缓冲区开头是否最终会构成某个 think 开标签。
fn leading_think_prefix_decision(buffer: &str) -> ThinkPrefixDecision {
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return ThinkPrefixDecision::NeedMore;
    }

    if INLINE_THINK_TAG_PAIRS
        .iter()
        .any(|(open_tag, _)| trimmed.starts_with(open_tag))
    {
        return ThinkPrefixDecision::Reasoning;
    }

    if INLINE_THINK_TAG_PAIRS
        .iter()
        .any(|(open_tag, _)| open_tag.starts_with(trimmed))
    {
        return ThinkPrefixDecision::NeedMore;
    }

    ThinkPrefixDecision::Text
}

/// 剥掉开头的空白与开标签，返回标签之后的内容；开头不是开标签时返回 None。
fn strip_leading_think_open_tag(text: &str) -> Option<&str> {
    let after_ws = text.trim_start();
    INLINE_THINK_TAG_PAIRS
        .iter()
        .find_map(|(open_tag, _)| after_ws.strip_prefix(open_tag))
}

/// 在文本里找最早出现的闭合标签。容忍开闭不配对（如 `<think>` 开、`</thinking>` 闭）：
/// 只认配对闭合会让整段连正文一起被当成推理吞掉。
fn find_think_close_tag(text: &str) -> Option<(usize, &'static str)> {
    INLINE_THINK_TAG_PAIRS
        .iter()
        .filter_map(|(_, close_tag)| text.find(close_tag).map(|index| (index, *close_tag)))
        .min_by_key(|(index, _)| *index)
}

/// Reasoning 态下需扣住不下发的尾部长度：末尾可能是某个闭合标签真前缀的
/// 最长后缀，连同它前面的换行（闭合标签到了就是分隔符，没到再补发）。
fn reasoning_holdback_len(buffer: &str) -> usize {
    let partial_close_tag = INLINE_THINK_TAG_PAIRS
        .iter()
        .filter_map(|(_, close_tag)| {
            (1..close_tag.len())
                .rev()
                .find(|len| buffer.ends_with(&close_tag[..*len]))
        })
        .max()
        .unwrap_or(0);
    let body = &buffer[..buffer.len() - partial_close_tag];
    buffer.len() - body.trim_end_matches(THINK_SEPARATORS).len()
}

fn non_empty(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 非流式：把开头的 think 块拆成 (思考, 正文)。开头不是 think 块或块未闭合时返回 None。
pub(crate) fn split_leading_think_block(text: &str) -> Option<(String, String)> {
    let body = strip_leading_think_open_tag(text)?;
    let (close_start, close_tag) = find_think_close_tag(body)?;
    let answer = body[close_start + close_tag.len()..].trim_start_matches(THINK_SEPARATORS);

    let thinking = body[..close_start].trim_matches(THINK_SEPARATORS);

    Some((thinking.to_string(), answer.to_string()))
}

/// 流式：跨 chunk 剥离正文开头的 think 块。
///
/// Detecting 扣住可能构成开标签的前缀；Reasoning 即时下发思考内容、只扣住
/// 可能是闭合标签前缀的短尾；Text 透传正文。紧挨标签的换行是分隔符，不下发。
/// 思考内容不能整段缓冲到闭合标签才下发：外层的静默超时量的是转换后的流，
/// 长思考期间没有输出会被判成上游卡死。
#[derive(Debug, Default)]
pub(crate) struct InlineThinkSplitter {
    mode: InlineThinkMode,
    /// Detecting: 流首前缀缓冲；Reasoning: 扣住的短尾。
    buffer: String,
    /// 标签刚结束，后续增量开头的换行仍是分隔符。
    /// 跨增量保持，保证同一响应不随 SSE 分块方式产生不同输出。
    strip_separator: bool,
}

impl InlineThinkSplitter {
    /// 喂入一个 content 增量，返回 (thinking 增量, 正文增量)。
    pub(crate) fn push(&mut self, delta: &str) -> (Option<String>, Option<String>) {
        match self.mode {
            InlineThinkMode::Text => (None, self.push_text(delta)),
            InlineThinkMode::Detecting => {
                self.buffer.push_str(delta);
                match leading_think_prefix_decision(&self.buffer) {
                    ThinkPrefixDecision::NeedMore => (None, None),
                    ThinkPrefixDecision::Reasoning => {
                        self.mode = InlineThinkMode::Reasoning;
                        self.strip_separator = true;
                        let rest = strip_leading_think_open_tag(&self.buffer)
                            .unwrap_or_default()
                            .to_string();
                        self.buffer = rest;
                        self.drain_reasoning()
                    }
                    ThinkPrefixDecision::Text => {
                        self.mode = InlineThinkMode::Text;
                        let text = std::mem::take(&mut self.buffer);
                        (None, non_empty(&text))
                    }
                }
            }
            InlineThinkMode::Reasoning => {
                self.buffer.push_str(delta);
                self.drain_reasoning()
            }
        }
    }

    fn push_text(&mut self, delta: &str) -> Option<String> {
        if !self.strip_separator {
            return non_empty(delta);
        }
        let rest = delta.trim_start_matches(THINK_SEPARATORS);
        if rest.is_empty() {
            // 整个增量都是分隔符，正文还没开始
            return None;
        }
        self.strip_separator = false;
        Some(rest.to_string())
    }

    /// Reasoning 态推进：闭合标签完整出现 → 立即拆块转 Text；
    /// 否则除扣住的短尾外，思考内容全部即时下发。
    fn drain_reasoning(&mut self) -> (Option<String>, Option<String>) {
        if self.strip_separator {
            let separators =
                self.buffer.len() - self.buffer.trim_start_matches(THINK_SEPARATORS).len();
            self.buffer.drain(..separators);
            if self.buffer.is_empty() {
                return (None, None);
            }
            self.strip_separator = false;
        }

        if let Some((close_start, close_tag)) = find_think_close_tag(&self.buffer) {
            let buffered = std::mem::take(&mut self.buffer);
            self.mode = InlineThinkMode::Text;
            self.strip_separator = true;
            let thinking = non_empty(buffered[..close_start].trim_end_matches(THINK_SEPARATORS));
            let text = self.push_text(&buffered[close_start + close_tag.len()..]);
            (thinking, text)
        } else {
            let holdback = reasoning_holdback_len(&self.buffer);
            let split_at = self.buffer.len() - holdback;
            let thinking: String = self.buffer.drain(..split_at).collect();
            (non_empty(&thinking), None)
        }
    }

    /// 边界（工具调用 / finish_reason / [DONE] / 截断 EOF / 错误）冲刷残留，幂等：
    /// 未闭合的 think 块按思考内容原样下发（保住已收到的载荷），
    /// Detecting 缓冲按正文下发。
    pub(crate) fn flush(&mut self) -> (Option<String>, Option<String>) {
        let buffered = std::mem::take(&mut self.buffer);
        let was_reasoning = self.mode == InlineThinkMode::Reasoning;
        self.mode = InlineThinkMode::Text;
        self.strip_separator = false;
        if was_reasoning {
            // push 已即时拆块，此处残留不含完整闭合标签
            (non_empty(&buffered), None)
        } else {
            (None, non_empty(&buffered))
        }
    }

    /// 是否还扣着未下发的非空白内容。
    pub(crate) fn has_pending_content(&self) -> bool {
        !self.buffer.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把一组增量喂完并冲刷，返回拼接后的 (思考, 正文)。
    fn run(deltas: &[&str]) -> (String, String) {
        let mut splitter = InlineThinkSplitter::default();
        let mut thinking = String::new();
        let mut text = String::new();
        let mut collect = |(t, x): (Option<String>, Option<String>)| {
            thinking.push_str(&t.unwrap_or_default());
            text.push_str(&x.unwrap_or_default());
        };
        for delta in deltas {
            collect(splitter.push(delta));
        }
        collect(splitter.flush());
        (thinking, text)
    }

    #[test]
    fn splits_both_tag_variants() {
        assert_eq!(
            run(&["<think>a</think>b"]),
            ("a".to_string(), "b".to_string())
        );
        assert_eq!(
            run(&["<thinking>a</thinking>b"]),
            ("a".to_string(), "b".to_string())
        );
    }

    #[test]
    fn tolerates_mismatched_tag_pairs() {
        assert_eq!(
            run(&["<think>a</thinking>b"]),
            ("a".to_string(), "b".to_string())
        );
    }

    #[test]
    fn reasoning_is_emitted_before_the_close_tag_arrives() {
        let mut splitter = InlineThinkSplitter::default();
        assert_eq!(splitter.push("<think>"), (None, None));
        assert_eq!(splitter.push("step one"), (Some("step one".into()), None));
        // 末尾可能是闭合标签的开头，先扣住
        assert_eq!(splitter.push(" two</th"), (Some(" two".into()), None));
        assert_eq!(splitter.push("ink>done"), (None, Some("done".into())));
    }

    #[test]
    fn a_held_back_tail_that_is_not_a_close_tag_is_released() {
        assert_eq!(
            run(&["<think>a </th", "ree> b</think>c"]),
            ("a </three> b".to_string(), "c".to_string())
        );
    }

    #[test]
    fn output_does_not_depend_on_how_the_stream_is_chunked() {
        // 紧挨标签的换行不下发，思考内部的换行和正文的缩进原样保留
        let response = "  \n<think>\r\nrea\nson\n</think>\r\n\n    return 42\n";
        let expected = ("rea\nson".to_string(), "    return 42\n".to_string());

        assert_eq!(run(&[response]), expected);
        for chunk_len in 1..response.len() {
            let chars: Vec<char> = response.chars().collect();
            let parts: Vec<String> = chars
                .chunks(chunk_len)
                .map(|chunk| chunk.iter().collect())
                .collect();
            let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
            assert_eq!(run(&parts), expected, "chunk_len {chunk_len}");
        }
    }

    #[test]
    fn newlines_held_back_before_a_possible_close_tag_are_released() {
        assert_eq!(
            run(&["<think>a\n", "\n</th", "ree>b\n", "</think>c"]),
            ("a\n\n</three>b".to_string(), "c".to_string())
        );
    }

    #[test]
    fn text_that_does_not_start_with_a_think_tag_is_untouched() {
        assert_eq!(
            run(&["<", "div>hi <think>x</think>"]),
            (String::new(), "<div>hi <think>x</think>".to_string())
        );
        assert_eq!(run(&["\n\n", "hi"]), (String::new(), "\n\nhi".to_string()));
    }

    #[test]
    fn flush_releases_whatever_is_held_back() {
        // 半截开标签按正文
        assert_eq!(run(&["<thi"]), (String::new(), "<thi".to_string()));
        // 未闭合的块按思考，连同扣住的短尾
        assert_eq!(
            run(&["<think>partial</th"]),
            ("partial</th".to_string(), String::new())
        );
    }

    #[test]
    fn flush_is_idempotent() {
        let mut splitter = InlineThinkSplitter::default();
        splitter.push("<think>partial");
        splitter.push("</th");
        assert_eq!(splitter.flush(), (Some("</th".into()), None));
        assert_eq!(splitter.flush(), (None, None));
        assert!(!splitter.has_pending_content());
    }

    #[test]
    fn non_streaming_split_matches_the_streaming_result() {
        assert_eq!(
            split_leading_think_block(" <thinking>\nrea\nson\n</thinking>\n\n    answer"),
            Some(("rea\nson".to_string(), "    answer".to_string()))
        );
        assert_eq!(
            split_leading_think_block("<think>a</thinking>b"),
            Some(("a".to_string(), "b".to_string()))
        );
        assert_eq!(split_leading_think_block("<think>unclosed"), None);
        assert_eq!(split_leading_think_block("hi <think>a</think>"), None);
    }
}
