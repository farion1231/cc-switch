use bytes::{Bytes, BytesMut};
use std::borrow::Cow;

#[inline]
pub(crate) fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(field)?;
    if rest.is_empty() {
        return Some("");
    }
    let value = rest.strip_prefix(':')?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

/// Iterate SSE lines, retaining their original LF, CRLF or CR terminator.
/// Unlike `str::lines`, a lone CR also ends a line.
pub(crate) fn sse_lines(mut text: &str) -> impl Iterator<Item = (&str, &str)> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let end = text
            .as_bytes()
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
            .unwrap_or(text.len());
        let ending_len = if text[end..].starts_with("\r\n") {
            2
        } else {
            usize::from(end < text.len())
        };
        let line = &text[..end];
        let ending = &text[end..end + ending_len];
        text = &text[end + ending_len..];
        Some((line, ending))
    })
}

/// SSE joins successive `data` fields with LF. The common single-line case
/// borrows the input; only multiline data needs a new string.
pub(crate) fn sse_data(block: &str) -> Option<Cow<'_, str>> {
    let mut data: Option<Cow<'_, str>> = None;
    for (index, (line, _)) in sse_lines(block).enumerate() {
        let line = if index == 0 {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        };
        if let Some(value) = strip_sse_field(line, "data") {
            match &mut data {
                None => data = Some(Cow::Borrowed(value)),
                Some(data) => {
                    let data = data.to_mut();
                    data.push('\n');
                    data.push_str(value);
                }
            }
        }
    }
    data
}

pub(crate) enum SseFrame {
    /// A complete event containing at least one data field, including its
    /// terminating blank line. Non-data fields before the data may already
    /// have been emitted as passthrough bytes.
    Event(Bytes),
    /// Leading comments (with any preceding metadata) / empty lines can be forwarded immediately,
    /// without waiting for an event's blank line (notably heartbeat comments).
    Passthrough(Bytes),
}

/// Incremental, byte-preserving SSE framing for response rewriting and usage
/// inspection. Each input byte is scanned once. Complete frames within one
/// chunk are `Bytes` slices; only frames spanning chunks need an owned buffer.
/// Call `next_frame` until it returns None before pushing another chunk.
#[derive(Default)]
pub(crate) struct SseDecoder {
    chunk: Bytes,
    cursor: usize,
    start: usize,
    pending: BytesMut,
    line_len: usize,
    line_prefix: [u8; 8],
    seen_line: bool,
    has_data: bool,
    skip_lf: bool,
}

impl SseDecoder {
    pub(crate) fn push(&mut self, chunk: Bytes) {
        debug_assert_eq!(self.cursor, self.chunk.len());
        debug_assert_eq!(self.start, self.cursor);
        self.chunk = chunk;
        self.cursor = 0;
        self.start = 0;
    }

    fn take_bytes(&mut self) -> Bytes {
        let bytes = self.chunk.slice(self.start..self.cursor);
        self.start = self.cursor;
        if self.pending.is_empty() {
            bytes
        } else {
            self.pending.extend_from_slice(&bytes);
            std::mem::take(&mut self.pending).freeze()
        }
    }

    pub(crate) fn next_frame(&mut self) -> Option<SseFrame> {
        while self.cursor < self.chunk.len() {
            let byte = self.chunk[self.cursor];
            self.cursor += 1;

            // A CR is a complete line ending on its own, so do not wait for
            // another chunk before dispatching a CR-terminated event/heartbeat.
            // If LF arrives later, retain it without treating it as a new line.
            if std::mem::take(&mut self.skip_lf) && byte == b'\n' {
                if !self.has_data {
                    return Some(SseFrame::Passthrough(self.take_bytes()));
                }
                continue;
            }

            if byte != b'\r' && byte != b'\n' {
                let remaining = &self.chunk[self.cursor - 1..];
                let len = remaining
                    .iter()
                    .position(|byte| matches!(byte, b'\r' | b'\n'))
                    .unwrap_or(remaining.len());
                let prefix_len = len.min(self.line_prefix.len().saturating_sub(self.line_len));
                if prefix_len > 0 {
                    self.line_prefix[self.line_len..self.line_len + prefix_len]
                        .copy_from_slice(&remaining[..prefix_len]);
                }
                self.cursor += len - 1;
                self.line_len += len;
                continue;
            }

            if byte == b'\r' {
                if self.chunk.get(self.cursor) == Some(&b'\n') {
                    self.cursor += 1;
                } else {
                    self.skip_lf = self.cursor == self.chunk.len();
                }
            }

            let mut prefix = &self.line_prefix[..self.line_len.min(self.line_prefix.len())];
            let mut line_len = self.line_len;
            if !self.seen_line && prefix.starts_with(b"\xef\xbb\xbf") {
                prefix = &prefix[3..];
                line_len -= 3;
            }
            self.has_data |= prefix.starts_with(b"data:") || (line_len == 4 && prefix == b"data");
            let is_comment = prefix.starts_with(b":");
            self.seen_line = true;
            self.line_len = 0;

            if line_len == 0 {
                let is_event = std::mem::take(&mut self.has_data);
                let bytes = self.take_bytes();
                return Some(if is_event {
                    SseFrame::Event(bytes)
                } else {
                    SseFrame::Passthrough(bytes)
                });
            }
            if !self.has_data && is_comment {
                return Some(SseFrame::Passthrough(self.take_bytes()));
            }
        }

        self.pending.extend_from_slice(&self.chunk[self.start..]);
        self.start = self.cursor;
        None
    }

    /// Preserve an incomplete tail on normal EOF. It is not a dispatched event
    /// and must not be rewritten or counted as usage.
    pub(crate) fn finish(self) -> Bytes {
        self.pending.freeze()
    }
}

#[inline]
pub(crate) fn take_sse_block(buffer: &mut String) -> Option<String> {
    let mut best: Option<(usize, usize)> = None;

    for (delimiter, len) in [("\r\n\r\n", 4usize), ("\n\n", 2usize)] {
        if let Some(pos) = buffer.find(delimiter) {
            if best.is_none_or(|(best_pos, _)| pos < best_pos) {
                best = Some((pos, len));
            }
        }
    }

    let (pos, len) = best?;
    let block = buffer[..pos].to_string();
    buffer.drain(..pos + len);
    Some(block)
}

/// Append raw bytes to a UTF-8 `String` buffer, correctly handling multi-byte
/// characters that are split across chunk boundaries.
///
/// `remainder` accumulates trailing bytes from the previous chunk that form an
/// incomplete UTF-8 sequence (at most 3 bytes under normal operation). On each
/// call the remainder is prepended to `new_bytes`, the longest valid UTF-8
/// prefix is appended to `buffer`, and any trailing incomplete bytes are saved
/// back into `remainder` for the next call.
///
/// A defensive guard discards `remainder` via lossy conversion if it ever
/// exceeds 3 bytes, which cannot happen with well-formed UTF-8 streams.
pub(crate) fn append_utf8_safe(buffer: &mut String, remainder: &mut Vec<u8>, new_bytes: &[u8]) {
    // Build the byte slice to decode: prepend any leftover bytes from previous chunk.
    let (owned, bytes): (Option<Vec<u8>>, &[u8]) = if remainder.is_empty() {
        (None, new_bytes)
    } else {
        // Defensive guard: remainder should never exceed 3 bytes (max incomplete
        // UTF-8 sequence is 3 bytes: a 4-byte char missing its last byte). If it
        // does, the stream is producing genuinely invalid bytes; flush them lossy
        // and start fresh.
        if remainder.len() > 3 {
            buffer.push_str(&String::from_utf8_lossy(remainder));
            remainder.clear();
            (None, new_bytes)
        } else {
            let mut combined = std::mem::take(remainder);
            combined.extend_from_slice(new_bytes);
            (Some(combined), &[])
        }
    };
    let input = owned.as_deref().unwrap_or(bytes);

    // Decode loop: consume all valid UTF-8 and any genuinely invalid bytes,
    // only leaving a trailing incomplete sequence in remainder.
    let mut pos = 0;
    loop {
        match std::str::from_utf8(&input[pos..]) {
            Ok(s) => {
                buffer.push_str(s);
                // Everything consumed – remainder stays empty.
                return;
            }
            Err(e) => {
                let valid_up_to = pos + e.valid_up_to();
                let valid_slice = &input[pos..valid_up_to];
                match std::str::from_utf8(valid_slice) {
                    Ok(valid) => buffer.push_str(valid),
                    Err(_) => buffer.push_str(&String::from_utf8_lossy(valid_slice)),
                }
                if let Some(invalid_len) = e.error_len() {
                    // Genuinely invalid byte(s) – emit U+FFFD and continue.
                    buffer.push('\u{FFFD}');
                    pos = valid_up_to + invalid_len;
                } else {
                    // Incomplete trailing sequence – stash for next chunk.
                    *remainder = input[valid_up_to..].to_vec();
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_preserves_bytes_and_events_at_every_chunk_boundary() {
        for delimiter in ["\n\n", "\r\n\r\n", "\r\r", "\n\r\n", "\r\n\n", "\n\r"] {
            let input = format!(
                "\u{feff}: heartbeat\r\nid: 1\ndata: {{\"text\":\"你好😀\"}}{delimiter}data: [DONE]{delimiter}data: unfinished",
            );
            let bytes = Bytes::from(input);
            for size in [1, 2, 3, 7, 4096] {
                let mut decoder = SseDecoder::default();
                let mut output = Vec::new();
                let mut data = Vec::new();
                for start in (0..bytes.len()).step_by(size) {
                    decoder.push(bytes.slice(start..(start + size).min(bytes.len())));
                    while let Some(frame) = decoder.next_frame() {
                        let raw = match frame {
                            SseFrame::Event(raw) => {
                                data.push(
                                    sse_data(std::str::from_utf8(&raw).unwrap())
                                        .unwrap()
                                        .into_owned(),
                                );
                                raw
                            }
                            SseFrame::Passthrough(raw) => raw,
                        };
                        output.extend_from_slice(&raw);
                    }
                }
                output.extend_from_slice(&decoder.finish());
                assert_eq!(output, bytes, "delimiter={delimiter:?}, chunk={size}");
                assert_eq!(data, ["{\"text\":\"你好😀\"}", "[DONE]"]);
            }
        }
    }

    #[test]
    fn decoder_dispatches_cr_heartbeat_and_event_without_waiting_for_lf() {
        let mut decoder = SseDecoder::default();
        decoder.push(Bytes::from_static(b": ping\r"));
        assert!(
            matches!(decoder.next_frame(), Some(SseFrame::Passthrough(raw)) if raw == b": ping\r"[..])
        );
        assert!(decoder.next_frame().is_none());
        decoder.push(Bytes::from_static(b"\ndata: {}\r\r"));
        assert!(
            matches!(decoder.next_frame(), Some(SseFrame::Passthrough(raw)) if raw == b"\n"[..])
        );
        assert!(
            matches!(decoder.next_frame(), Some(SseFrame::Event(raw)) if raw == b"data: {}\r\r"[..])
        );
        assert!(decoder.next_frame().is_none());
    }

    #[test]
    fn data_joins_multiline_fields_and_borrows_single_line() {
        assert_eq!(
            sse_data("data\rdata: one\r\ndata:  two\n\n").as_deref(),
            Some("\none\n two")
        );
        assert!(matches!(
            sse_data("data: {}\n\n"),
            Some(Cow::Borrowed("{}"))
        ));
        assert_eq!(sse_data("\u{feff}data: {}\r\r").as_deref(), Some("{}"));
        assert_eq!(sse_data(": model\nmetadata: false\n\n"), None);
    }

    #[test]
    fn strip_sse_field_accepts_optional_space() {
        assert_eq!(
            strip_sse_field("data: {\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            strip_sse_field("data:{\"ok\":true}", "data"),
            Some("{\"ok\":true}")
        );
        assert_eq!(
            strip_sse_field("event: message_start", "event"),
            Some("message_start")
        );
        assert_eq!(
            strip_sse_field("event:message_start", "event"),
            Some("message_start")
        );
        assert_eq!(strip_sse_field("id:1", "data"), None);
    }

    #[test]
    fn take_sse_block_supports_lf_delimiters() {
        let mut buffer = "data: {\"ok\":true}\n\nrest".to_string();

        assert_eq!(
            take_sse_block(&mut buffer),
            Some("data: {\"ok\":true}".to_string())
        );
        assert_eq!(buffer, "rest");
    }

    #[test]
    fn take_sse_block_supports_crlf_delimiters() {
        let mut buffer = "data: {\"ok\":true}\r\n\r\nrest".to_string();

        assert_eq!(
            take_sse_block(&mut buffer),
            Some("data: {\"ok\":true}".to_string())
        );
        assert_eq!(buffer, "rest");
    }

    // ------------------------------------------------------------------
    // append_utf8_safe tests
    // ------------------------------------------------------------------

    #[test]
    fn ascii_passthrough() {
        let mut buf = String::new();
        let mut rem = Vec::new();
        append_utf8_safe(&mut buf, &mut rem, b"hello world");
        assert_eq!(buf, "hello world");
        assert!(rem.is_empty());
    }

    #[test]
    fn complete_multibyte_in_single_chunk() {
        let mut buf = String::new();
        let mut rem = Vec::new();
        append_utf8_safe(&mut buf, &mut rem, "你好世界".as_bytes());
        assert_eq!(buf, "你好世界");
        assert!(rem.is_empty());
    }

    #[test]
    fn split_multibyte_across_two_chunks() {
        // "你" = E4 BD A0 (3 bytes)
        let bytes = "你".as_bytes();
        assert_eq!(bytes.len(), 3);

        let mut buf = String::new();
        let mut rem = Vec::new();

        // Chunk 1: first 2 bytes (incomplete)
        append_utf8_safe(&mut buf, &mut rem, &bytes[..2]);
        assert_eq!(buf, "");
        assert_eq!(rem.len(), 2);

        // Chunk 2: last byte completes the character
        append_utf8_safe(&mut buf, &mut rem, &bytes[2..]);
        assert_eq!(buf, "你");
        assert!(rem.is_empty());
    }

    #[test]
    fn split_four_byte_char_across_chunks() {
        // 😀 = F0 9F 98 80 (4 bytes)
        let bytes = "😀".as_bytes();
        assert_eq!(bytes.len(), 4);

        let mut buf = String::new();
        let mut rem = Vec::new();

        // Send 1 byte at a time
        append_utf8_safe(&mut buf, &mut rem, &bytes[..1]);
        assert_eq!(buf, "");
        assert_eq!(rem.len(), 1);

        append_utf8_safe(&mut buf, &mut rem, &bytes[1..2]);
        assert_eq!(buf, "");
        assert_eq!(rem.len(), 2);

        append_utf8_safe(&mut buf, &mut rem, &bytes[2..3]);
        assert_eq!(buf, "");
        assert_eq!(rem.len(), 3);

        append_utf8_safe(&mut buf, &mut rem, &bytes[3..]);
        assert_eq!(buf, "😀");
        assert!(rem.is_empty());
    }

    #[test]
    fn mixed_ascii_and_split_multibyte() {
        // "hi你" = 68 69 E4 BD A0
        let all = "hi你".as_bytes();
        assert_eq!(all.len(), 5);

        let mut buf = String::new();
        let mut rem = Vec::new();

        // Chunk 1: "hi" + first byte of "你"
        append_utf8_safe(&mut buf, &mut rem, &all[..3]);
        assert_eq!(buf, "hi");
        assert_eq!(rem.len(), 1);

        // Chunk 2: remaining 2 bytes of "你"
        append_utf8_safe(&mut buf, &mut rem, &all[3..]);
        assert_eq!(buf, "hi你");
        assert!(rem.is_empty());
    }

    #[test]
    fn multiple_split_characters_in_sequence() {
        let text = "你好";
        let bytes = text.as_bytes(); // E4 BD A0 E5 A5 BD

        let mut buf = String::new();
        let mut rem = Vec::new();

        // Split in the middle: first char complete + 1 byte of second
        append_utf8_safe(&mut buf, &mut rem, &bytes[..4]);
        assert_eq!(buf, "你");
        assert_eq!(rem.len(), 1);

        // Remaining 2 bytes complete second char
        append_utf8_safe(&mut buf, &mut rem, &bytes[4..]);
        assert_eq!(buf, "你好");
        assert!(rem.is_empty());
    }

    #[test]
    fn empty_chunks_are_harmless() {
        let mut buf = String::new();
        let mut rem = Vec::new();

        append_utf8_safe(&mut buf, &mut rem, b"");
        assert_eq!(buf, "");
        assert!(rem.is_empty());

        append_utf8_safe(&mut buf, &mut rem, b"ok");
        assert_eq!(buf, "ok");

        append_utf8_safe(&mut buf, &mut rem, b"");
        assert_eq!(buf, "ok");
    }

    #[test]
    fn sse_json_with_chinese_split_at_boundary() {
        // Simulates an SSE data line with Chinese content split across chunks
        let json_line = "data: {\"text\":\"你好\"}\n\n";
        let bytes = json_line.as_bytes();

        // Find where "你" starts in the byte stream and split there
        let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
        let split_point = ni_start + 1; // split inside "你"

        let mut buf = String::new();
        let mut rem = Vec::new();

        append_utf8_safe(&mut buf, &mut rem, &bytes[..split_point]);
        append_utf8_safe(&mut buf, &mut rem, &bytes[split_point..]);

        assert_eq!(buf, json_line);
        assert!(rem.is_empty());

        // Verify the buffer can be parsed as SSE with valid JSON
        let data = strip_sse_field(buf.lines().next().unwrap(), "data").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(data).unwrap();
        assert_eq!(parsed["text"], "你好");
    }

    #[test]
    fn invalid_bytes_flushed_immediately_not_accumulated() {
        // 0xFF is never valid in UTF-8 – it should be replaced immediately,
        // not stashed in remainder.
        let mut buf = String::new();
        let mut rem = Vec::new();

        // "hi" + invalid byte + "ok"
        append_utf8_safe(&mut buf, &mut rem, b"hi\xFFok");
        assert!(
            rem.is_empty(),
            "remainder should be empty after invalid byte"
        );
        assert!(buf.contains("hi"), "valid prefix must be present");
        assert!(buf.contains("ok"), "valid suffix must be present");
        assert!(buf.contains('\u{FFFD}'), "invalid byte must produce U+FFFD");
    }

    #[test]
    fn invalid_byte_in_slow_path_flushed_immediately() {
        let mut buf = String::new();
        let mut rem = Vec::new();

        // Prime remainder with an incomplete sequence (first byte of "你")
        append_utf8_safe(&mut buf, &mut rem, &"你".as_bytes()[..1]);
        assert_eq!(rem.len(), 1);

        // Next chunk starts with an invalid byte – the stale remainder and the
        // invalid byte should both be flushed, not accumulated.
        append_utf8_safe(&mut buf, &mut rem, b"\xFFworld");
        assert!(rem.is_empty(), "remainder should be empty");
        assert!(
            buf.contains("world"),
            "valid data after invalid byte must appear"
        );
    }

    #[test]
    fn defensive_guard_flushes_oversized_remainder() {
        let mut buf = String::new();
        let mut rem = Vec::new();

        // Manually inject 4 invalid bytes into remainder to trigger the >3 guard.
        // This can't happen with well-formed UTF-8, but tests the safety net.
        rem.extend_from_slice(b"\x80\x80\x80\x80");
        assert_eq!(rem.len(), 4);

        append_utf8_safe(&mut buf, &mut rem, b"hello");
        // The 4 invalid bytes should have been flushed lossy, then "hello" decoded.
        assert!(rem.is_empty(), "remainder must be empty after guard flush");
        assert!(
            buf.contains("hello"),
            "valid data after guard flush must appear"
        );
        // The 4 invalid bytes each produce a U+FFFD
        let replacement_count = buf.chars().filter(|&c| c == '\u{FFFD}').count();
        assert_eq!(
            replacement_count, 4,
            "each invalid byte should produce one U+FFFD"
        );
    }
}
