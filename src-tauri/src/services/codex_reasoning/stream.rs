//! Stream helpers for multi-round reasoning continuation.
//!
//! T13 provides pure helpers; the full multi-round orchestrator lands in T14
//! (`LogicalCodexRequestResult` + forwarder loop).

use bytes::Bytes;
use serde_json::Value;

use crate::error::AppError;
use crate::proxy::usage::parser::TokenUsage;

use super::usage::{ContinuationRoundResult, RoundUsage};

/// Extract terminal `output` array from a Responses API completed payload.
pub fn extract_terminal_output(terminal: &Value) -> Vec<Value> {
    terminal
        .get("output")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}


/// Parse a full Responses-API SSE buffer into a ContinuationRoundResult.
///
/// Scans events for the last `response.completed` (or any event carrying
/// `response.usage` / top-level `usage`) and extracts output + usage.
pub fn parse_sse_to_round(
    sse: Bytes,
    round_index: u8,
    duration_ms: u64,
) -> Result<ContinuationRoundResult, AppError> {
    let text = std::str::from_utf8(&sse).map_err(|e| {
        AppError::InvalidInput(format!("SSE is not valid UTF-8: {e}"))
    })?;

    let mut events: Vec<Value> = Vec::new();
    let mut data_lines: Vec<String> = Vec::new();

    let flush = |data_lines: &mut Vec<String>, events: &mut Vec<Value>| {
        if data_lines.is_empty() {
            return;
        }
        let payload = data_lines.join("\n");
        data_lines.clear();
        if payload.is_empty() || payload == "[DONE]" {
            return;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&payload) {
            events.push(v);
        }
    };

    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            flush(&mut data_lines, &mut events);
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
        // ignore event:/id:/retry: lines
    }
    flush(&mut data_lines, &mut events);

    // Prefer last response.completed; else last event with usage; else last event with output.
    let mut terminal: Option<Value> = None;
    for ev in events.iter().rev() {
        let ty = ev.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "response.completed" {
            // payload may be nested under "response"
            if let Some(resp) = ev.get("response") {
                terminal = Some(resp.clone());
            } else {
                terminal = Some(ev.clone());
            }
            break;
        }
    }
    if terminal.is_none() {
        for ev in events.iter().rev() {
            if ev.get("usage").is_some()
                || ev.pointer("/response/usage").is_some()
                || ev.get("output").is_some()
                || ev.pointer("/response/output").is_some()
            {
                if let Some(resp) = ev.get("response") {
                    terminal = Some(resp.clone());
                } else {
                    terminal = Some(ev.clone());
                }
                break;
            }
        }
    }
    let terminal = terminal.unwrap_or_else(|| serde_json::json!({ "output": [] }));

    let terminal_output = extract_terminal_output(&terminal);

    let reasoning_tokens = terminal
        .pointer("/usage/output_tokens_details/reasoning_tokens")
        .and_then(|v| v.as_u64())
        .or_else(|| {
            terminal
                .pointer("/usage/completion_tokens_details/reasoning_tokens")
                .and_then(|v| v.as_u64())
        })
        .or_else(|| terminal.pointer("/usage/reasoning_tokens").and_then(|v| v.as_u64()))
        .map(|n| n as u32);

    let usage = if let Some(tu) = TokenUsage::from_codex_response(&terminal) {
        RoundUsage {
            input_tokens: tu.input_tokens,
            output_tokens: tu.output_tokens,
            cache_read_tokens: tu.cache_read_tokens,
            cache_creation_tokens: tu.cache_creation_tokens,
        }
    } else {
        let u = terminal.get("usage");
        RoundUsage {
            input_tokens: u
                .and_then(|x| x.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            output_tokens: u
                .and_then(|x| x.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_read_tokens: u
                .and_then(|x| {
                    x.pointer("/input_tokens_details/cached_tokens")
                        .or_else(|| x.get("cache_read_input_tokens"))
                })
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            cache_creation_tokens: 0,
        }
    };

    Ok(ContinuationRoundResult {
        round_index,
        sse,
        usage,
        reasoning_tokens,
        duration_ms,
        terminal_output,
    })
}


/// Concatenate multiple SSE byte buffers into a single client-facing stream.
///
/// Intermediate rounds keep all events; a blank line is inserted between rounds
/// so clients that frame on double-newline remain stable.
pub fn concat_sse_rounds(rounds: &[Bytes]) -> Bytes {
    if rounds.is_empty() {
        return Bytes::new();
    }
    if rounds.len() == 1 {
        return rounds[0].clone();
    }
    let mut out: Vec<u8> = Vec::new();
    for (i, chunk) in rounds.iter().enumerate() {
        if i > 0 {
            // Ensure previous chunk ends with a blank line separator
            if !out.ends_with(b"\n\n") {
                if out.ends_with(b"\n") {
                    out.push(b'\n');
                } else {
                    out.extend_from_slice(b"\n\n");
                }
            }
        }
        out.extend_from_slice(chunk);
    }
    Bytes::from(out)
}

/// Strip intermediate `response.completed` events from non-final rounds so the
/// client only sees one logical completion (final round).
///
/// Conservative: removes SSE events whose `event:` line is `response.completed`
/// or whose data JSON has `"type":"response.completed"`.
pub fn strip_intermediate_completed(sse: &Bytes, is_final_round: bool) -> Bytes {
    if is_final_round {
        return sse.clone();
    }
    let text = match std::str::from_utf8(sse) {
        Ok(s) => s,
        Err(_) => return sse.clone(),
    };
    let mut out = String::with_capacity(text.len());
    let mut block = String::new();
    for line in text.split_inclusive('\n') {
        block.push_str(line);
        if line == "\n" || line.ends_with("\n\n") || line == "\r\n" {
            // end of block handled below when blank
        }
        if line == "\n" || line == "\r\n" {
            // blank line → flush block
            if !block_is_completed_event(&block) {
                out.push_str(&block);
            }
            block.clear();
        }
    }
    if !block.is_empty() && !block_is_completed_event(&block) {
        out.push_str(&block);
    }
    Bytes::from(out)
}

fn block_is_completed_event(block: &str) -> bool {
    let lower = block.to_ascii_lowercase();
    if lower.contains("event: response.completed") || lower.contains("event:response.completed") {
        return true;
    }
    // data line type check
    for line in block.lines() {
        let trimmed = line.trim();
        if let Some(data) = trimmed.strip_prefix("data:") {
            let data = data.trim();
            if data.contains("\"response.completed\"")
                || data.contains("\"type\":\"response.completed\"")
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_output_items() {
        let t = json!({"output": [{"type": "message"}, {"type": "reasoning"}]});
        assert_eq!(extract_terminal_output(&t).len(), 2);
        assert!(extract_terminal_output(&json!({})).is_empty());
    }

    #[test]
    fn concat_two_rounds() {
        let a = Bytes::from_static(b"data: {\"x\":1}\n\n");
        let b = Bytes::from_static(b"data: {\"x\":2}\n\n");
        let c = concat_sse_rounds(&[a, b]);
        let s = std::str::from_utf8(&c).unwrap();
        assert!(s.contains("\"x\":1"));
        assert!(s.contains("\"x\":2"));
    }

    #[test]
    fn strip_completed_from_intermediate() {
        let sse = Bytes::from_static(
            b"event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\"}\n\n",
        );
        let stripped = strip_intermediate_completed(&sse, false);
        let s = std::str::from_utf8(&stripped).unwrap();
        assert!(s.contains("output_item.done"));
        assert!(!s.contains("response.completed"));
        let kept = strip_intermediate_completed(&sse, true);
        assert!(std::str::from_utf8(&kept).unwrap().contains("response.completed"));
    }

    #[test]
    fn parse_sse_to_round_extracts_completed() {
        let raw = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"id\":\"m1\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"output_tokens_details\":{\"reasoning_tokens\":516}}}}\n\n",
        );
        let sse = Bytes::from(raw);
        let r = parse_sse_to_round(sse, 0, 42).unwrap();
        assert_eq!(r.round_index, 0);
        assert_eq!(r.duration_ms, 42);
        assert_eq!(r.reasoning_tokens, Some(516));
        assert_eq!(r.usage.input_tokens, 10);
        assert_eq!(r.usage.output_tokens, 20);
        assert_eq!(r.terminal_output.len(), 1);
    }
}