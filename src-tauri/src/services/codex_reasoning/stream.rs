//! Stream helpers for multi-round reasoning continuation.
//!
//! T13 provides pure helpers; the full multi-round orchestrator lands in T14
//! (`LogicalCodexRequestResult` + forwarder loop).

use bytes::Bytes;
use serde_json::Value;

/// Extract terminal `output` array from a Responses API completed payload.
pub fn extract_terminal_output(terminal: &Value) -> Vec<Value> {
    terminal
        .get("output")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
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
}
