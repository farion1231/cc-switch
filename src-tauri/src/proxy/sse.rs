#[inline]
pub(crate) fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let value = line.strip_prefix(field)?.strip_prefix(':')?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

/// Incremental SSE framing. Keep both a read cursor and a search cursor so a
/// batch of small events and one large, fragmented event both take O(bytes).
#[derive(Debug, Default)]
pub(crate) struct SseBuffer {
    data: String,
    start: usize,
    search_from: usize,
}

impl std::ops::Deref for SseBuffer {
    type Target = str;

    fn deref(&self) -> &str {
        &self.data[self.start..]
    }
}

impl From<String> for SseBuffer {
    fn from(data: String) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }
}

impl SseBuffer {
    pub(crate) fn clear(&mut self) {
        self.data.clear();
        self.start = 0;
        self.search_from = 0;
    }

    fn compact(&mut self) {
        // Move at most as many bytes as we consumed. Do not shift the entire
        // remaining batch after every event, or copy a growing partial event
        // after every network chunk.
        if self.start > 0 && self.start >= self.data.len() / 2 {
            self.data.drain(..self.start);
            self.search_from -= self.start;
            self.start = 0;
        }
    }

    pub(crate) fn push_str(&mut self, value: &str) {
        self.compact();
        self.data.push_str(value);
    }

    pub(crate) fn push_bytes(&mut self, remainder: &mut Vec<u8>, bytes: &[u8]) {
        self.compact();
        append_utf8_safe(&mut self.data, remainder, bytes);
    }
}

pub(crate) fn take_sse_block(buffer: &mut SseBuffer) -> Option<String> {
    let bytes = buffer.data.as_bytes();
    while let Some(relative) = bytes[buffer.search_from..]
        .iter()
        .position(|byte| *byte == b'\n')
    {
        let newline = buffer.search_from + relative;
        let next = newline + 1;
        let end = if bytes.get(next) == Some(&b'\r') {
            next + 1
        } else {
            next
        };
        if end == bytes.len() {
            // A LF or LF+CR at the end may be half of a delimiter. Only this
            // suffix needs to be checked again when the next chunk arrives.
            buffer.search_from = newline;
            return None;
        }
        if bytes[end] == b'\n' {
            let delimiter = if newline > buffer.start && bytes[newline - 1] == b'\r' {
                newline - 1
            } else {
                newline
            };
            let block = buffer.data[buffer.start..delimiter].to_string();
            buffer.start = end + 1;
            buffer.search_from = buffer.start;
            if buffer.start == buffer.data.len() {
                buffer.clear();
            }
            return Some(block);
        }
        buffer.search_from = next;
    }
    buffer.search_from = bytes.len();
    None
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
    use super::{append_utf8_safe, strip_sse_field, take_sse_block, SseBuffer};

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
        let mut buffer = SseBuffer::from("data: {\"ok\":true}\n\nrest".to_string());

        assert_eq!(
            take_sse_block(&mut buffer),
            Some("data: {\"ok\":true}".to_string())
        );
        assert_eq!(&*buffer, "rest");
    }

    #[test]
    fn take_sse_block_supports_crlf_delimiters() {
        let mut buffer = SseBuffer::from("data: {\"ok\":true}\r\n\r\nrest".to_string());

        assert_eq!(
            take_sse_block(&mut buffer),
            Some("data: {\"ok\":true}".to_string())
        );
        assert_eq!(&*buffer, "rest");
    }

    #[test]
    fn take_sse_block_handles_mixed_line_endings_across_chunks() {
        for delimiter in ["\n\n", "\r\n\r\n", "\n\r\n", "\r\n\n"] {
            let first = "event: message\r\ndata: first";
            let input = format!("{first}{delimiter}data: second\n\npartial");

            // Exercise every possible transport split, including inside CRLF.
            for split in 0..=input.len() {
                let mut buffer = SseBuffer::default();
                let mut blocks = Vec::new();
                for chunk in [&input[..split], &input[split..]] {
                    buffer.push_str(chunk);
                    while let Some(block) = take_sse_block(&mut buffer) {
                        blocks.push(block);
                    }
                }
                assert_eq!(
                    blocks,
                    [first, "data: second"],
                    "{delimiter:?}, split {split}"
                );
                assert_eq!(&*buffer, "partial");
            }

            // A complete mixed delimiter must dispatch without another event.
            let mut buffer = SseBuffer::from(format!("{first}{delimiter}"));
            assert_eq!(take_sse_block(&mut buffer).as_deref(), Some(first));
            assert!(buffer.is_empty());
        }
    }

    // Keep the previous implementation as an oracle for framing compatibility.
    fn reference_block(buffer: &mut String) -> Option<String> {
        let (pos, len) = ["\r\n\r\n", "\r\n\n", "\n\r\n", "\n\n"]
            .iter()
            .filter_map(|delimiter| buffer.find(delimiter).map(|pos| (pos, delimiter.len())))
            .min_by_key(|(pos, _)| *pos)?;
        let block = buffer[..pos].to_string();
        buffer.drain(..pos + len);
        Some(block)
    }

    #[test]
    fn incremental_framing_matches_reference_for_all_short_inputs() {
        // Includes empty blocks, lone CRs, adjacent/mixed delimiters and every
        // transport split. Compare after each chunk, not only at EOF.
        for len in 0..=7_u32 {
            for mut encoded in 0..3_usize.pow(len) {
                let mut input = String::new();
                for _ in 0..len {
                    input.push(['x', '\r', '\n'][encoded % 3]);
                    encoded /= 3;
                }
                for split in 0..=input.len() {
                    let mut reference = String::new();
                    let mut buffer = SseBuffer::default();
                    for chunk in [&input[..split], &input[split..]] {
                        reference.push_str(chunk);
                        buffer.push_str(chunk);
                        loop {
                            let expected = reference_block(&mut reference);
                            let actual = take_sse_block(&mut buffer);
                            assert_eq!(actual, expected, "input={input:?}, split={split}");
                            if actual.is_none() {
                                break;
                            }
                        }
                        assert_eq!(&*buffer, reference, "input={input:?}, split={split}");
                    }
                }
            }
        }
    }

    #[test]
    fn incremental_framing_preserves_utf8_partial_events_and_reuse() {
        let input = format!("data: {}\r\n\r\ndata: 😀\n\n尾部", "你好".repeat(4096));
        let mut buffer = SseBuffer::default();
        let mut remainder = Vec::new();
        let mut blocks = Vec::new();
        for byte in input.as_bytes() {
            buffer.push_bytes(&mut remainder, &[*byte]);
            while let Some(block) = take_sse_block(&mut buffer) {
                blocks.push(block);
            }
        }
        assert_eq!(
            blocks,
            [format!("data: {}", "你好".repeat(4096)), "data: 😀".into()]
        );
        assert_eq!(&*buffer, "尾部");
        buffer.clear();
        buffer.push_str("data: reused\n\n");
        assert_eq!(take_sse_block(&mut buffer).as_deref(), Some("data: reused"));
        assert!(buffer.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn incremental_framing_compacts_without_losing_the_search_position() {
        let mut buffer = SseBuffer::from("data: first\n\npartial\r\n".to_string());
        assert_eq!(take_sse_block(&mut buffer).as_deref(), Some("data: first"));
        assert!(take_sse_block(&mut buffer).is_none());
        buffer.push_str("\r\n"); // compacts and completes a split delimiter
        assert_eq!(take_sse_block(&mut buffer).as_deref(), Some("partial"));
        assert!(buffer.is_empty());
    }

    #[test]
    #[ignore = "manual performance comparison; run with --ignored --nocapture"]
    fn benchmark_sse_framing() {
        use std::{hint::black_box, time::Instant};
        for count in [1_000, 4_000, 16_000] {
            let input = format!("data: {}\n\n", "x".repeat(128)).repeat(count);
            let started = Instant::now();
            let mut old = input.clone();
            let mut old_count = 0;
            while let Some(block) = reference_block(&mut old) {
                black_box(block);
                old_count += 1;
            }
            let old_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = Instant::now();
            let mut new = SseBuffer::from(input);
            let mut new_count = 0;
            while let Some(block) = take_sse_block(&mut new) {
                black_box(block);
                new_count += 1;
            }
            let new_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(old_count, count);
            assert_eq!(new_count, count);
            eprintln!("SSE batch events={count}: old={old_ms:.3}ms new={new_ms:.3}ms");
        }
        for size in [64 * 1024, 256 * 1024, 1024 * 1024] {
            let input = format!("data: {}\n\n", "x".repeat(size));
            let started = Instant::now();
            let mut old = String::new();
            for chunk in input.as_bytes().chunks(128) {
                old.push_str(std::str::from_utf8(chunk).unwrap());
                black_box(reference_block(&mut old));
            }
            let old_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = Instant::now();
            let mut new = SseBuffer::default();
            for chunk in input.as_bytes().chunks(128) {
                new.push_str(std::str::from_utf8(chunk).unwrap());
                black_box(take_sse_block(&mut new));
            }
            let new_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert!(old.is_empty() && new.is_empty());
            eprintln!("SSE fragmented bytes={size}: old={old_ms:.3}ms new={new_ms:.3}ms");
        }
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
