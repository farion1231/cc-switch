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
            let start = find_subslice(&lower_chars(&message.content), needle)?;
            let chars: Vec<char> = message.content.chars().collect();
            Some(SessionSearchSnippet {
                message_index,
                role: message.role.clone(),
                text: snippet_around(&chars, start, needle.len()),
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

/// Lowercase one char at a time so the result keeps a 1:1 index mapping with the
/// source text and match offsets stay valid in the original.
fn lower_chars(text: &str) -> Vec<char> {
    text.chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
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
