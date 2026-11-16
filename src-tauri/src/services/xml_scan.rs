//! Minimal, tolerant XML scanner for remote storage listings.
//!
//! WebDAV `PROPFIND` multistatus bodies and S3 `ListObjectsV2` results only
//! need element names and text content. Servers disagree on namespace prefixes
//! (`D:`, `d:`, `lp1:`, none), so the scanner reports local names only and
//! leaves interpretation to the caller. It is deliberately not a validating
//! parser: malformed input yields a best-effort event list, never a panic.

/// One structural event in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum XmlEvent {
    /// Opening tag, local name only (`<D:href>` → `href`).
    Start(String),
    /// Closing tag, local name only. Self-closing tags emit `Start` + `End`.
    End(String),
    /// Unescaped character data (entity references and CDATA resolved).
    Text(String),
}

fn local_name(raw: &str) -> String {
    let name = raw
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or_default();
    let local = name.rsplit(':').next().unwrap_or(name);
    local.to_ascii_lowercase()
}

/// Resolve the five predefined entities plus numeric character references.
/// Unknown entities are kept verbatim.
pub(crate) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let Some(end) = after.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..end];
        let resolved = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                u32::from_str_radix(&entity[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if entity.starts_with('#') => {
                entity[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        match resolved {
            Some(ch) => {
                out.push(ch);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Scan `xml` into a flat event list. Whitespace-only text is dropped.
pub(crate) fn scan(xml: &str) -> Vec<XmlEvent> {
    let mut events = Vec::new();
    let bytes = xml.as_bytes();
    let mut i = 0;
    let mut text_start = 0;

    let flush_text = |events: &mut Vec<XmlEvent>, raw: &str| {
        if !raw.trim().is_empty() {
            events.push(XmlEvent::Text(unescape(raw.trim())));
        }
    };

    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        flush_text(&mut events, &xml[text_start..i]);
        let rest = &xml[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|p| p + 3).unwrap_or(rest.len());
        } else if rest.starts_with("<![CDATA[") {
            let body_end = rest.find("]]>").unwrap_or(rest.len());
            let body = &rest[9..body_end.max(9)];
            if !body.is_empty() {
                events.push(XmlEvent::Text(body.to_string()));
            }
            i += (body_end + 3).min(rest.len());
        } else if rest.starts_with("<?") {
            i += rest.find("?>").map(|p| p + 2).unwrap_or(rest.len());
        } else if rest.starts_with("<!") {
            i += rest.find('>').map(|p| p + 1).unwrap_or(rest.len());
        } else {
            // Find the closing '>' while respecting quoted attribute values.
            let mut j = 1;
            let mut quote: Option<u8> = None;
            let tag_bytes = rest.as_bytes();
            while j < tag_bytes.len() {
                let b = tag_bytes[j];
                match quote {
                    Some(q) if b == q => quote = None,
                    Some(_) => {}
                    None if b == b'"' || b == b'\'' => quote = Some(b),
                    None if b == b'>' => break,
                    None => {}
                }
                j += 1;
            }
            let inner = &rest[1..j.min(rest.len())];
            if let Some(closing) = inner.strip_prefix('/') {
                events.push(XmlEvent::End(local_name(closing.trim())));
            } else {
                let name = local_name(inner.trim());
                if !name.is_empty() {
                    events.push(XmlEvent::Start(name.clone()));
                    if inner.trim_end().ends_with('/') {
                        events.push(XmlEvent::End(name));
                    }
                }
            }
            i += (j + 1).min(rest.len());
        }
        text_start = i;
    }
    if text_start < xml.len() {
        flush_text(&mut events, &xml[text_start..]);
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_reports_local_names_and_unescaped_text() {
        let events = scan(
            r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:"><D:href>/a%20b/&amp;x</D:href><D:collection/></D:multistatus>"#,
        );
        assert_eq!(
            events,
            vec![
                XmlEvent::Start("multistatus".into()),
                XmlEvent::Start("href".into()),
                XmlEvent::Text("/a%20b/&x".into()),
                XmlEvent::End("href".into()),
                XmlEvent::Start("collection".into()),
                XmlEvent::End("collection".into()),
                XmlEvent::End("multistatus".into()),
            ]
        );
    }

    #[test]
    fn scan_handles_quoted_gt_comments_and_cdata() {
        let events = scan(r#"<a attr="x>y"><!-- <b> --><![CDATA[<raw>]]></a>"#);
        assert_eq!(
            events,
            vec![
                XmlEvent::Start("a".into()),
                XmlEvent::Text("<raw>".into()),
                XmlEvent::End("a".into()),
            ]
        );
    }

    #[test]
    fn unescape_numeric_references_and_unknown_entities() {
        assert_eq!(unescape("&#65;&#x42;&unknown;&"), "AB&unknown;&");
    }

    #[test]
    fn scan_never_panics_on_truncated_input() {
        for input in ["<", "<a", "<a attr=\"", "<![CDATA[", "<!--", "text<", "</"] {
            let _ = scan(input);
        }
    }
}
