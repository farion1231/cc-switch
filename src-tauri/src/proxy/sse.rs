#[inline]
pub(crate) fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let value = line.strip_prefix(field)?.strip_prefix(':')?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

/// Locate the first complete SSE event whose delimiter starts at or after
/// `from` (a char boundary). Returns `(block_end, next_block_start)`.
#[inline]
fn find_sse_block_from(buffer: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = buffer.as_bytes();
    let mut offset = from;
    while let Some(pos) = buffer[offset..].find('\n') {
        let newline = offset + pos;
        let after = &bytes[newline + 1..];
        let end = if after.starts_with(b"\n") {
            newline + 2
        } else if after.starts_with(b"\r\n") {
            newline + 3
        } else {
            offset = newline + 1;
            continue;
        };
        // Both lines may use LF or CRLF, independently.
        let start = if newline > 0 && bytes[newline - 1] == b'\r' {
            newline - 1
        } else {
            newline
        };
        return Some((start, end));
    }
    None
}

/// Earliest position a later scan has to revisit: a newline in the last two
/// bytes may still turn into a delimiter once more data arrives.
#[inline]
fn sse_resume_point(pending: &str) -> usize {
    let mut at = pending.len().saturating_sub(2);
    while !pending.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// SSE events of a body that is already complete, borrowed in place.
///
/// [`Self::remainder`] is the trailing text after the last delimiter, which
/// callers treat as a final event when the upstream omitted the blank line.
pub(crate) struct SseBlocks<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> SseBlocks<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self { text, pos: 0 }
    }

    pub(crate) fn remainder(&self) -> &'a str {
        &self.text[self.pos..]
    }
}

impl<'a> Iterator for SseBlocks<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = &self.text[self.pos..];
        let (end, next) = find_sse_block_from(rest, 0)?;
        self.pos += next;
        Some(&rest[..end])
    }
}

/// Incremental SSE framer for a streamed body.
///
/// Events are handed out as slices of the internal buffer; consumed text is
/// dropped once per incoming chunk instead of once per event, and a scan that
/// found no delimiter resumes near the end instead of rescanning a long
/// partial event.
#[derive(Debug, Default)]
pub(crate) struct SseBuffer {
    text: String,
    utf8_remainder: Vec<u8>,
    /// Start of the text not yet returned by [`Self::next_block`].
    consumed: usize,
    /// Where the next delimiter scan starts; always `>= consumed`.
    scan_from: usize,
}

impl SseBuffer {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Append raw bytes; multi-byte characters split across chunks are held
    /// back until complete (see [`append_utf8_safe`]).
    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) {
        self.compact();
        append_utf8_safe(&mut self.text, &mut self.utf8_remainder, bytes);
    }

    pub(crate) fn push_str(&mut self, text: &str) {
        self.compact();
        self.text.push_str(text);
    }

    /// Next complete event (without its delimiter), or `None` until more data
    /// arrives.
    pub(crate) fn next_block(&mut self) -> Option<&str> {
        let pending = &self.text[self.consumed..];
        match find_sse_block_from(pending, self.scan_from - self.consumed) {
            Some((end, next)) => {
                let start = self.consumed;
                self.consumed += next;
                self.scan_from = self.consumed;
                Some(&self.text[start..start + end])
            }
            None => {
                self.scan_from = self.consumed + sse_resume_point(pending);
                None
            }
        }
    }

    /// Text received but not yet returned as an event.
    pub(crate) fn pending(&self) -> &str {
        &self.text[self.consumed..]
    }

    /// Discard the pending text (an incomplete UTF-8 tail is kept).
    pub(crate) fn clear(&mut self) {
        self.text.clear();
        self.consumed = 0;
        self.scan_from = 0;
    }

    /// End of stream: the pending text plus any incomplete UTF-8 tail,
    /// decoded lossily.
    pub(crate) fn take_tail(&mut self) -> String {
        self.compact();
        if !self.utf8_remainder.is_empty() {
            let remainder = std::mem::take(&mut self.utf8_remainder);
            self.text.push_str(&String::from_utf8_lossy(&remainder));
        }
        self.scan_from = 0;
        std::mem::take(&mut self.text)
    }

    fn compact(&mut self) {
        if self.consumed > 0 {
            self.text.drain(..self.consumed);
            self.scan_from -= self.consumed;
            self.consumed = 0;
        }
    }
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
    use super::{append_utf8_safe, strip_sse_field, SseBlocks, SseBuffer};

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

    fn drain_blocks(sse: &mut SseBuffer) -> Vec<String> {
        let mut blocks = Vec::new();
        while let Some(block) = sse.next_block() {
            blocks.push(block.to_string());
        }
        blocks
    }

    #[test]
    fn sse_buffer_supports_lf_and_crlf_delimiters() {
        for input in [
            "data: {\"ok\":true}\n\nrest",
            "data: {\"ok\":true}\r\n\r\nrest",
        ] {
            let mut sse = SseBuffer::new();
            sse.push_str(input);

            assert_eq!(drain_blocks(&mut sse), ["data: {\"ok\":true}"]);
            assert_eq!(sse.pending(), "rest");
        }
    }

    #[test]
    fn sse_framing_handles_mixed_line_endings_across_chunks() {
        for delimiter in ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"] {
            let first = "event: message\r\ndata: first";
            let input = format!("{first}{delimiter}data: second\n\npartial");

            // Exercise every possible transport split, including inside CRLF.
            for split in 0..=input.len() {
                let mut sse = SseBuffer::new();
                let mut blocks = Vec::new();
                for chunk in [&input[..split], &input[split..]] {
                    sse.push_bytes(chunk.as_bytes());
                    blocks.extend(drain_blocks(&mut sse));
                }
                assert_eq!(
                    blocks,
                    [first, "data: second"],
                    "{delimiter:?}, split {split}"
                );
                assert_eq!(sse.pending(), "partial");
            }

            // A complete mixed delimiter must dispatch without another event.
            let mut sse = SseBuffer::new();
            sse.push_str(&format!("{first}{delimiter}"));
            assert_eq!(drain_blocks(&mut sse), [first]);
            assert!(sse.pending().is_empty());

            let mut blocks = SseBlocks::new(&input);
            assert_eq!(blocks.by_ref().collect::<Vec<_>>(), [first, "data: second"]);
            assert_eq!(blocks.remainder(), "partial");
        }
    }

    #[test]
    fn sse_buffer_matches_whole_body_framing_when_fed_byte_by_byte() {
        // A long event, CRLF and mixed delimiters, an empty event and multi-byte
        // text: feeding one byte at a time exercises every scan resume point.
        let long = format!("data: {}", "中文\\n".repeat(200));
        let input = format!("{long}\r\n\r\nevent: a\r\ndata: 1\n\r\n\n\ndata: 😀\r\n\ndata: tail");
        let mut whole = SseBlocks::new(&input);
        let expected = whole.by_ref().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            expected,
            [long.as_str(), "event: a\r\ndata: 1", "", "data: 😀"]
        );
        assert_eq!(whole.remainder(), "data: tail");

        let mut sse = SseBuffer::new();
        let mut blocks = Vec::new();
        for byte in input.as_bytes() {
            sse.push_bytes(std::slice::from_ref(byte));
            blocks.extend(drain_blocks(&mut sse));
        }
        assert_eq!(blocks, expected);
        assert_eq!(sse.pending(), "data: tail");
    }

    #[test]
    fn sse_buffer_tail_and_clear() {
        let mut sse = SseBuffer::new();
        sse.push_bytes(b"data: a\n\ndata: b");
        assert_eq!(drain_blocks(&mut sse), ["data: a"]);
        // An incomplete multi-byte tail is held back until the stream ends.
        sse.push_bytes(&"中".as_bytes()[..2]);
        assert_eq!(sse.pending(), "data: b");
        assert_eq!(sse.take_tail(), "data: b\u{FFFD}");
        assert!(sse.pending().is_empty());

        // `clear` drops pending text but keeps the incomplete character.
        sse.push_bytes(b"{\"partial\"");
        sse.push_bytes(&"中".as_bytes()[..1]);
        sse.clear();
        assert!(sse.pending().is_empty());
        sse.push_bytes(&"中".as_bytes()[1..]);
        sse.push_str("\n\n");
        assert_eq!(drain_blocks(&mut sse), ["中"]);
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
