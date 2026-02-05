//! URL utilities shared across proxy modules.
//!
//! This module intentionally avoids full URL parsing to keep behavior aligned with
//! existing "stringly-typed" base_url configurations while fixing common edge cases
//! (query/fragment handling and safe path de-duplication).

/// Split a URL-like string into `(base, suffix)` where suffix starts with `?` or `#`.
///
/// Example:
/// - `https://x/v1?token=1` => (`https://x/v1`, `?token=1`)
/// - `https://x/v1#frag` => (`https://x/v1`, `#frag`)
pub(crate) fn split_url_suffix(input: &str) -> (&str, &str) {
    match input.find(['?', '#']) {
        Some(idx) => (&input[..idx], &input[idx..]),
        None => (input, ""),
    }
}

/// De-duplicate repeated `/v1/v1` only when it occurs on a segment boundary.
///
/// This avoids corrupting valid paths such as `/v1/v1beta/...`.
pub(crate) fn dedup_v1_v1_boundary_safe(mut url: String) -> String {
    const NEEDLE: &str = "/v1/v1";
    let mut search_start = 0usize;

    loop {
        let Some(rel_pos) = url[search_start..].find(NEEDLE) else {
            break;
        };
        let pos = search_start + rel_pos;
        let after = pos + NEEDLE.len();

        let boundary_ok = after == url.len()
            || matches!(
                url.as_bytes().get(after),
                Some(b'/') | Some(b'?') | Some(b'#')
            );

        if boundary_ok {
            url.replace_range(pos..after, "/v1");
            // Continue searching from the same position in case we created a new boundary match.
            search_start = pos;
        } else {
            // Skip forward to find later occurrences that might be valid boundary matches.
            search_start = pos + 1;
        }
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_url_suffix_handles_query() {
        let (base, suffix) = split_url_suffix("https://example.com/v1?token=1");
        assert_eq!(base, "https://example.com/v1");
        assert_eq!(suffix, "?token=1");
    }

    #[test]
    fn split_url_suffix_handles_fragment() {
        let (base, suffix) = split_url_suffix("https://example.com/v1#frag");
        assert_eq!(base, "https://example.com/v1");
        assert_eq!(suffix, "#frag");
    }

    #[test]
    fn dedup_v1_v1_only_on_boundary() {
        let url = "https://example.com/v1/v1/messages".to_string();
        assert_eq!(
            dedup_v1_v1_boundary_safe(url),
            "https://example.com/v1/messages"
        );
    }

    #[test]
    fn dedup_v1_v1_does_not_corrupt_v1beta() {
        let url = "https://example.com/v1/v1beta/models".to_string();
        assert_eq!(
            dedup_v1_v1_boundary_safe(url.clone()),
            "https://example.com/v1/v1beta/models"
        );
    }

    #[test]
    fn dedup_v1_v1_skips_v1beta_but_dedups_later_occurrence() {
        let url = "https://example.com/v1/v1beta/v1/v1/messages".to_string();
        assert_eq!(
            dedup_v1_v1_boundary_safe(url),
            "https://example.com/v1/v1beta/v1/messages"
        );
    }
}
