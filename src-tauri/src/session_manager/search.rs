use serde::Serialize;
use std::thread;

use super::{load_messages, scan_sessions, SessionMeta};

/// Characters of surrounding context kept on each side of a match.
const SNIPPET_CONTEXT_CHARS: usize = 60;
/// Snippets reported per session.
const MAX_SNIPPETS_PER_SESSION: usize = 3;
/// Sessions reported per query, keeping the most recently active ones.
const MAX_HITS: usize = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchSnippet {
    /// Index into the message list returned by `load_messages`, so the UI can
    /// scroll straight to the match.
    pub message_index: usize,
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    pub provider_id: String,
    pub session_id: String,
    pub source_path: String,
    pub snippets: Vec<SessionSearchSnippet>,
}

/// Search the full transcript of every session, not just the metadata the list
/// view indexes.
///
/// Matching runs over the same messages the detail pane renders, so every hit
/// corresponds to text the user can actually see. Deliberately no storage-level
/// prefilter: SQLite `LIKE` folds case for ASCII only and would silently drop
/// `Éclair` for the query `éclair`, and a raw substring scan of the JSONL would
/// miss content behind JSON escapes.
pub fn search_sessions(query: &str, provider_id: Option<&str>) -> Vec<SessionSearchHit> {
    let needle = lower_chars(query.trim());
    if needle.is_empty() {
        return Vec::new();
    }
    let needle = needle.as_slice();

    let sessions: Vec<SessionMeta> = scan_sessions()
        .into_iter()
        .filter(|meta| provider_id.is_none_or(|id| meta.provider_id == id))
        .collect();

    let workers = thread::available_parallelism().map_or(4, |count| count.get());
    let chunk_size = sessions.len().div_ceil(workers).max(1);

    // Chunks are joined in order, so hits stay sorted by recency like `scan_sessions`.
    let mut hits: Vec<SessionSearchHit> = thread::scope(|scope| {
        let handles: Vec<_> = sessions
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|meta| search_session(meta, needle))
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    });

    hits.truncate(MAX_HITS);
    hits
}

fn search_session(meta: &SessionMeta, needle: &[char]) -> Option<SessionSearchHit> {
    let source_path = meta.source_path.as_deref()?;
    let messages = load_messages(&meta.provider_id, source_path).ok()?;

    let snippets: Vec<SessionSearchSnippet> = messages
        .iter()
        .enumerate()
        .filter_map(|(message_index, message)| {
            let (lowered, origin) = lower_with_origin(&message.content);
            let start = find_subslice(&lowered, needle)?;
            // Map the match back through the fold, which may have expanded chars.
            let first = origin[start];
            let last = origin[start + needle.len() - 1];
            let chars: Vec<char> = message.content.chars().collect();
            Some(SessionSearchSnippet {
                message_index,
                role: message.role.clone(),
                text: snippet_around(&chars, first, last - first + 1),
            })
        })
        .take(MAX_SNIPPETS_PER_SESSION)
        .collect();

    if snippets.is_empty() {
        return None;
    }

    Some(SessionSearchHit {
        provider_id: meta.provider_id.clone(),
        session_id: meta.session_id.clone(),
        source_path: source_path.to_string(),
        snippets,
    })
}

/// Lowercase `text` for matching. `char::to_lowercase` can expand one char into
/// several — `İ` (U+0130) is the only such char in Unicode — so this is not a 1:1
/// mapping and offsets into the result are not offsets into the source.
fn lower_chars(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).collect()
}

/// Same fold as [`lower_chars`], plus the source char index each folded char came
/// from, so a match found in folded space can be sliced out of the original.
fn lower_with_origin(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut lowered = Vec::new();
    let mut origin = Vec::new();
    for (index, c) in text.chars().enumerate() {
        for folded in c.to_lowercase() {
            lowered.push(folded);
            origin.push(index);
        }
    }
    (lowered, origin)
}

fn find_subslice(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Context window around a match, with whitespace flattened so the snippet reads
/// as a single line in the list.
fn snippet_around(chars: &[char], start: usize, len: usize) -> String {
    let from = start.saturating_sub(SNIPPET_CONTEXT_CHARS);
    let to = (start + len + SNIPPET_CONTEXT_CHARS).min(chars.len());

    let mut text = String::new();
    if from > 0 {
        text.push('…');
    }
    text.extend(
        chars[from..to]
            .iter()
            .map(|c| if c.is_whitespace() { ' ' } else { *c }),
    );
    if to < chars.len() {
        text.push('…');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn claude_meta(path: &Path) -> SessionMeta {
        SessionMeta {
            provider_id: "claude".to_string(),
            session_id: "s1".to_string(),
            title: None,
            summary: None,
            project_dir: None,
            created_at: None,
            last_active_at: None,
            source_path: Some(path.to_string_lossy().to_string()),
            resume_command: None,
        }
    }

    fn write_claude_session(path: &Path, contents: &[&str]) {
        let body = contents
            .iter()
            .map(|text| {
                format!(
                    "{{\"timestamp\":\"2026-03-06T21:50:13Z\",\"message\":{{\"role\":\"user\",\"content\":{}}}}}",
                    serde_json::to_string(text).unwrap()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, body).expect("write session");
    }

    fn search(path: &Path, query: &str) -> Option<SessionSearchHit> {
        search_session(&claude_meta(path), &lower_chars(query))
    }

    #[test]
    fn finds_keyword_in_the_middle_of_a_conversation() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        write_claude_session(
            &path,
            &[
                "opening question",
                "中间讨论了浙江移动的方案",
                "closing note",
            ],
        );

        let hit = search(&path, "浙江移动").expect("expected a content hit");
        assert_eq!(hit.snippets.len(), 1);
        assert_eq!(hit.snippets[0].message_index, 1);
        assert!(hit.snippets[0].text.contains("浙江移动"));
    }

    #[test]
    fn case_folding_covers_non_ascii() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        write_claude_session(&path, &["Ordered an Éclair"]);

        assert!(search(&path, "éclair").is_some());
        assert!(search(&path, "ÉCLAIR").is_some());
    }

    /// `İ` (U+0130) is the only char in Unicode whose lowercase expands to more
    /// than one char, so it is the only input that exercises the origin map.
    /// Without this the map could be reverted to a 1:1 fold and every other test
    /// would still pass.
    #[test]
    fn folds_expanding_chars_without_dropping_code_points() {
        assert_eq!(lower_chars("\u{0130}"), vec!['i', '\u{0307}']);

        let (lowered, origin) = lower_with_origin("a\u{0130}b");
        assert_eq!(lowered, vec!['a', 'i', '\u{0307}', 'b']);
        assert_eq!(origin, vec![0, 1, 1, 2]);
    }

    #[test]
    fn reports_at_most_three_snippets() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        write_claude_session(&path, &["hit", "hit", "hit", "hit", "hit"]);

        let hit = search(&path, "hit").expect("expected a content hit");
        assert_eq!(hit.snippets.len(), MAX_SNIPPETS_PER_SESSION);
    }

    #[test]
    fn misses_produce_no_hit() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        write_claude_session(&path, &["nothing to see"]);

        assert!(search(&path, "absent").is_none());
    }

    #[test]
    fn empty_query_matches_nothing() {
        assert!(search_sessions("   ", None).is_empty());
    }

    #[test]
    fn snippet_trims_context_on_char_boundaries() {
        let chars: Vec<char> = format!("{}目标{}", "前".repeat(200), "后".repeat(200))
            .chars()
            .collect();
        let text = snippet_around(&chars, 200, 2);

        assert!(text.starts_with('…') && text.ends_with('…'));
        assert!(text.contains("目标"));
        assert_eq!(text.chars().count(), 2 + SNIPPET_CONTEXT_CHARS * 2 + 2);
    }

    #[test]
    fn snippet_omits_ellipsis_when_whole_message_fits() {
        let chars: Vec<char> = "short and sweet".chars().collect();
        assert_eq!(snippet_around(&chars, 6, 3), "short and sweet");
    }

    #[test]
    fn snippet_flattens_newlines() {
        let chars: Vec<char> = "line one\nline two".chars().collect();
        assert_eq!(snippet_around(&chars, 0, 4), "line one line two");
    }

    #[test]
    fn find_subslice_locates_and_rejects() {
        let haystack: Vec<char> = "abcabd".chars().collect();
        assert_eq!(find_subslice(&haystack, &['a', 'b', 'd']), Some(3));
        assert_eq!(find_subslice(&haystack, &['x']), None);
        assert_eq!(find_subslice(&haystack, &[]), None);
        assert_eq!(find_subslice(&['a'], &['a', 'b']), None);
    }
}
