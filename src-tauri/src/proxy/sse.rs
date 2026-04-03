#[inline]
pub(crate) fn strip_sse_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    line.strip_prefix(&format!("{field}: "))
        .or_else(|| line.strip_prefix(&format!("{field}:")))
}

pub(crate) fn take_sse_block(buffer: &mut String) -> Option<String> {
    let lf = buffer.find("\n\n");
    let crlf = buffer.find("\r\n\r\n");
    let (pos, delim_len) = match (lf, crlf) {
        (Some(lf_pos), Some(crlf_pos)) => {
            if lf_pos < crlf_pos {
                (lf_pos, 2)
            } else {
                (crlf_pos, 4)
            }
        }
        (Some(lf_pos), None) => (lf_pos, 2),
        (None, Some(crlf_pos)) => (crlf_pos, 4),
        (None, None) => return None,
    };

    let mut block = buffer[..pos].to_string();
    *buffer = buffer[pos + delim_len..].to_string();

    if block.contains('\r') {
        block = block.replace("\r\n", "\n");
        block = block.replace('\r', "\n");
    }

    Some(block)
}

#[cfg(test)]
mod tests {
    use super::{strip_sse_field, take_sse_block};

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
    fn take_sse_block_handles_crlf_delimiter() {
        let mut buffer = "data: {\"ok\":true}\r\n\r\nrest".to_string();
        let block = take_sse_block(&mut buffer).expect("block");
        assert_eq!(block, "data: {\"ok\":true}");
        assert_eq!(buffer, "rest");
    }

    #[test]
    fn take_sse_block_handles_lf_delimiter() {
        let mut buffer = "event: ping\n\ndata:1".to_string();
        let block = take_sse_block(&mut buffer).expect("block");
        assert_eq!(block, "event: ping");
        assert_eq!(buffer, "data:1");
    }
}
