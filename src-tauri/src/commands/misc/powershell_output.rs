use once_cell::sync::Lazy;
use quick_xml::{events::Event, Reader};
use regex::Regex;

/// EncodedCommand can serialize stderr as CLIXML even with OutputFormat Text.
/// Keep error records, discard progress/host metadata (Write-Host is already on
/// stdout), and preserve any ordinary text before or after the XML document.
pub(super) fn readable_output(text: &str) -> String {
    let mut remaining = text;
    let mut result = String::new();
    while let Some((before, after)) = remaining.split_once("#< CLIXML") {
        let Some(end) = after.find("</Objs>").map(|index| index + "</Objs>".len()) else {
            break;
        };
        let Some(errors) = error_records(after[..end].trim()) else {
            break;
        };
        result.push_str(before);
        result.push_str(&errors);
        remaining = &after[end..];
    }
    result.push_str(remaining);
    result
}

fn error_records(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut errors = Vec::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(tag) if tag.name().as_ref() == b"S" => {
                let is_error = tag.attributes().any(|attr| {
                    attr.is_ok_and(|attr| {
                        attr.key.as_ref() == b"S" && attr.value.eq_ignore_ascii_case(b"error")
                    })
                });
                if is_error {
                    let raw = reader.read_text(tag.name()).ok()?;
                    let unescaped = quick_xml::escape::unescape(&raw).ok()?;
                    let decoded = decode_string(&unescaped);
                    let line = decoded.trim_end();
                    if !line.is_empty() {
                        errors.push(line.to_string());
                    }
                }
            }
            Event::Eof => return Some(errors.join("\n")),
            _ => {}
        }
    }
}

/// PowerShell escapes CR/LF and UTF-16 code units as _xHHHH_. Decode once so
/// escaped literals such as _x005F_x000A_ stay literal, and surrogate pairs work.
fn decode_string(text: &str) -> String {
    static ESCAPE: Lazy<Regex> = Lazy::new(|| Regex::new(r"_x[0-9a-fA-F]{4}_").unwrap());
    let mut units = Vec::new();
    let mut start = 0;
    for escape in ESCAPE.find_iter(text) {
        units.extend(text[start..escape.start()].encode_utf16());
        units.push(u16::from_str_radix(&text[escape.start() + 2..escape.end() - 1], 16).unwrap());
        start = escape.end();
    }
    units.extend(text[start..].encode_utf16());
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_error_records_without_host_metadata() {
        let xml = concat!(
            "plain stderr\n#< CLIXML\r\n",
            "<Objs><Obj S=\"information\"><ToString>Verifying checksum...</ToString>",
            "<Props><S N=\"User\">private-user</S></Props></Obj>",
            "<S S=\"Error\">Get-FileHash: &lt;module&gt; &amp; &#39;utility&#39;_x000D__x000A_</S>",
            "<S S=\"Error\">  at installer_x000D__x000A_</S></Objs>\ntail stderr"
        );
        assert_eq!(
            readable_output(xml),
            "plain stderr\nGet-FileHash: <module> & 'utility'\n  at installer\ntail stderr"
        );
    }

    #[test]
    fn discards_information_and_progress_to_allow_stdout_fallback() {
        assert_eq!(
            readable_output("#< CLIXML\n<Objs><Obj S=\"progress\"/><Obj S=\"information\"><ToString>Error: Checksum mismatch</ToString></Obj></Objs>"),
            ""
        );
    }

    #[test]
    fn decodes_unicode_and_escaped_literals_once() {
        assert_eq!(
            decode_string("中文_xD83D__xDE80_ _x005F_x000A_"),
            "中文🚀 _x000A_"
        );
    }

    #[test]
    fn preserves_plain_text_and_incomplete_xml() {
        for text in [
            "npm failed\nexit 1",
            "#< CLIXML\n<Objs><S S=\"Error\">incomplete",
        ] {
            assert_eq!(readable_output(text), text);
        }
    }
}
