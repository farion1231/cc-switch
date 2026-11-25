//! Gemini Native shadow state
//!
//! Keeps provider/session-scoped assistant content snapshots and tool call metadata
//! so Gemini thought signatures and tool turns can be replayed without bloating
//! the main proxy files.

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{RwLock, RwLockWriteGuard};

/// Composite key for a Gemini shadow session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GeminiShadowKey {
    pub provider_id: String,
    pub session_id: String,
}

impl GeminiShadowKey {
    pub fn new(provider_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            session_id: session_id.into(),
        }
    }
}

/// Gemini function call metadata captured from an assistant turn.
#[derive(Debug, Clone, PartialEq)]
pub struct GeminiToolCallMeta {
    pub id: Option<String>,
    pub name: String,
    pub args: Value,
    pub thought_signature: Option<String>,
}

impl GeminiToolCallMeta {
    pub fn new(
        id: Option<impl Into<String>>,
        name: impl Into<String>,
        args: Value,
        thought_signature: Option<impl Into<String>>,
    ) -> Self {
        Self {
            id: id.map(Into::into),
            name: name.into(),
            args,
            thought_signature: thought_signature.map(Into::into),
        }
    }
}

/// Stored assistant turn snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct GeminiAssistantTurn {
    pub assistant_content: Value,
    pub tool_calls: Vec<GeminiToolCallMeta>,
}

impl GeminiAssistantTurn {
    pub fn new(assistant_content: Value, tool_calls: Vec<GeminiToolCallMeta>) -> Self {
        Self {
            assistant_content,
            tool_calls,
        }
    }
}

/// Session snapshot returned by read APIs.
#[derive(Debug, Clone, PartialEq)]
pub struct GeminiShadowSessionSnapshot {
    pub provider_id: String,
    pub session_id: String,
    pub turns: Vec<GeminiAssistantTurn>,
    /// Lightweight signatures outlive the bounded full-content turn window.
    pub thought_signatures: HashMap<String, String>,
}

const MAX_CACHED_SIGNATURES: usize = 4096;
const MAX_CACHED_SIGNATURE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GeminiSignatureKey {
    session: GeminiShadowKey,
    tool_call_id: String,
}

#[derive(Debug, Clone)]
struct GeminiShadowSession {
    turns: VecDeque<GeminiAssistantTurn>,
}

impl GeminiShadowSession {
    fn new() -> Self {
        Self {
            turns: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct GeminiShadowInner {
    sessions: HashMap<GeminiShadowKey, GeminiShadowSession>,
    session_order: VecDeque<GeminiShadowKey>,
    signatures: HashMap<GeminiSignatureKey, String>,
    signature_order: VecDeque<GeminiSignatureKey>,
    signature_bytes: usize,
}

impl GeminiShadowInner {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            session_order: VecDeque::new(),
            signatures: HashMap::new(),
            signature_order: VecDeque::new(),
            signature_bytes: 0,
        }
    }
}

/// Thread-safe shadow store for Gemini Native replay state.
///
/// The store is intentionally small and explicit:
/// - sessions are keyed by `(provider_id, session_id)`
/// - each session keeps only a bounded number of recent assistant turns
/// - the oldest session is evicted first when the store is full
/// - a separately bounded cache retains signatures by exact provider/session/call id
#[derive(Debug)]
pub struct GeminiShadowStore {
    max_sessions: usize,
    max_turns_per_session: usize,
    max_signatures: usize,
    max_signature_bytes: usize,
    inner: RwLock<GeminiShadowInner>,
}

impl Default for GeminiShadowStore {
    fn default() -> Self {
        Self::with_limits(200, 64)
    }
}

impl GeminiShadowStore {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(max_sessions: usize, max_turns_per_session: usize) -> Self {
        Self::with_signature_limits(
            max_sessions,
            max_turns_per_session,
            MAX_CACHED_SIGNATURES,
            MAX_CACHED_SIGNATURE_BYTES,
        )
    }

    fn with_signature_limits(
        max_sessions: usize,
        max_turns_per_session: usize,
        max_signatures: usize,
        max_signature_bytes: usize,
    ) -> Self {
        Self {
            max_sessions: max_sessions.max(1),
            max_turns_per_session: max_turns_per_session.max(1),
            max_signatures: max_signatures.max(1),
            max_signature_bytes: max_signature_bytes.max(1),
            inner: RwLock::new(GeminiShadowInner::new()),
        }
    }

    /// Record a Gemini assistant turn for later replay.
    pub fn record_assistant_turn(
        &self,
        provider_id: impl Into<String>,
        session_id: impl Into<String>,
        assistant_content: Value,
        tool_calls: Vec<GeminiToolCallMeta>,
    ) {
        let key = GeminiShadowKey::new(provider_id, session_id);
        let turn = GeminiAssistantTurn::new(assistant_content, tool_calls);

        let mut inner = self.write_inner();
        Self::touch_session_order(&mut inner.session_order, &key);
        Self::cache_signatures(
            &mut inner,
            &key,
            &turn.tool_calls,
            self.max_signatures,
            self.max_signature_bytes,
        );

        let session = inner
            .sessions
            .entry(key)
            .or_insert_with(GeminiShadowSession::new);
        session.turns.push_back(turn);
        while session.turns.len() > self.max_turns_per_session {
            session.turns.pop_front();
        }
        Self::prune_sessions(&mut inner, self.max_sessions);
    }

    /// Get the latest assistant content for a provider/session pair.
    #[cfg(test)]
    pub fn latest_assistant_content(&self, provider_id: &str, session_id: &str) -> Option<Value> {
        self.get_session(provider_id, session_id, [])
            .and_then(|snapshot| {
                snapshot
                    .turns
                    .last()
                    .map(|turn| turn.assistant_content.clone())
            })
    }

    /// Get the latest tool calls for a provider/session pair.
    #[cfg(test)]
    pub fn latest_tool_calls(
        &self,
        provider_id: &str,
        session_id: &str,
    ) -> Option<Vec<GeminiToolCallMeta>> {
        self.get_session(provider_id, session_id, [])
            .and_then(|snapshot| snapshot.turns.last().map(|turn| turn.tool_calls.clone()))
    }

    /// Read recent turns and retained signatures for the requested tool calls.
    pub fn get_session<'a>(
        &self,
        provider_id: &str,
        session_id: &str,
        tool_call_ids: impl IntoIterator<Item = &'a str>,
    ) -> Option<GeminiShadowSessionSnapshot> {
        let key = GeminiShadowKey::new(provider_id, session_id);
        let mut inner = self.write_inner();
        let turns: Option<Vec<_>> = inner
            .sessions
            .get(&key)
            .map(|session| session.turns.iter().cloned().collect());
        if turns.is_some() {
            Self::touch_session_order(&mut inner.session_order, &key);
        }

        let mut signature_key = GeminiSignatureKey {
            session: key.clone(),
            tool_call_id: String::new(),
        };
        let thought_signatures: HashMap<_, _> = tool_call_ids
            .into_iter()
            .filter_map(|id| {
                id.clone_into(&mut signature_key.tool_call_id);
                inner
                    .signatures
                    .get(&signature_key)
                    .map(|signature| (id.to_owned(), signature.clone()))
            })
            .collect();
        if turns.is_none() && thought_signatures.is_empty() {
            return None;
        }

        Some(GeminiShadowSessionSnapshot {
            provider_id: key.provider_id,
            session_id: key.session_id,
            turns: turns.unwrap_or_default(),
            thought_signatures,
        })
    }

    /// Number of tracked sessions.
    #[cfg(test)]
    pub fn session_count(&self) -> usize {
        self.write_inner().sessions.len()
    }

    fn write_inner(&self) -> RwLockWriteGuard<'_, GeminiShadowInner> {
        self.inner.write().unwrap_or_else(|poisoned| {
            log::warn!("[GeminiShadow] recovering poisoned write lock");
            poisoned.into_inner()
        })
    }

    fn signature_entry_size(key: &GeminiSignatureKey, signature: &str) -> usize {
        // Keys are stored in both the lookup map and the eviction queue.
        2 * (key.session.provider_id.len() + key.session.session_id.len() + key.tool_call_id.len())
            + signature.len()
    }

    fn remove_signature(inner: &mut GeminiShadowInner, key: &GeminiSignatureKey) {
        if let Some(signature) = inner.signatures.remove(key) {
            inner.signature_bytes -= Self::signature_entry_size(key, &signature);
        }
    }

    fn cache_signatures(
        inner: &mut GeminiShadowInner,
        session: &GeminiShadowKey,
        calls: &[GeminiToolCallMeta],
        max_count: usize,
        max_bytes: usize,
    ) {
        for call in calls {
            let Some(id) = call.id.as_deref().filter(|id| !id.is_empty()) else {
                continue;
            };
            let Some(signature) = call
                .thought_signature
                .as_deref()
                .filter(|sig| !sig.is_empty())
            else {
                continue;
            };
            let key = GeminiSignatureKey {
                session: session.clone(),
                tool_call_id: id.to_string(),
            };
            if inner.signatures.contains_key(&key) {
                Self::remove_signature(inner, &key);
                inner.signature_order.retain(|cached| cached != &key);
            }
            let entry_size = Self::signature_entry_size(&key, signature);
            if entry_size > max_bytes {
                continue;
            }
            inner.signature_bytes += entry_size;
            inner.signatures.insert(key.clone(), signature.to_string());
            inner.signature_order.push_back(key);
            while inner.signatures.len() > max_count || inner.signature_bytes > max_bytes {
                let Some(oldest) = inner.signature_order.pop_front() else {
                    break;
                };
                Self::remove_signature(inner, &oldest);
            }
        }
    }

    fn touch_session_order(order: &mut VecDeque<GeminiShadowKey>, key: &GeminiShadowKey) {
        if let Some(pos) = order.iter().position(|existing| existing == key) {
            order.remove(pos);
        }
        order.push_back(key.clone());
    }

    fn prune_sessions(inner: &mut GeminiShadowInner, max_sessions: usize) {
        while inner.sessions.len() > max_sessions {
            let Some(evicted_key) = inner.session_order.pop_front() else {
                break;
            };
            inner.sessions.remove(&evicted_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn signature_cache_survives_full_turn_and_session_eviction() {
        let store = GeminiShadowStore::with_signature_limits(1, 1, 8, 1024);
        store.record_assistant_turn(
            "p",
            "s",
            json!({"text": "call"}),
            vec![GeminiToolCallMeta::new(
                Some("call_1"),
                "Glob",
                json!({}),
                Some("real-signature"),
            )],
        );
        store.record_assistant_turn("p", "s", json!({"text": "later"}), vec![]);
        let snapshot = store.get_session("p", "s", ["call_1"]).unwrap();
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(
            snapshot
                .thought_signatures
                .get("call_1")
                .map(String::as_str),
            Some("real-signature")
        );
        store.record_assistant_turn("p", "another", json!({"text": "another session"}), vec![]);
        let retained = store.get_session("p", "s", ["call_1"]).unwrap();
        assert!(retained.turns.is_empty());
        assert_eq!(
            retained
                .thought_signatures
                .get("call_1")
                .map(String::as_str),
            Some("real-signature")
        );
        assert!(store.get_session("p", "another", []).is_some());
    }

    #[test]
    fn signature_cache_is_scoped_and_count_bounded() {
        let store = GeminiShadowStore::with_signature_limits(8, 1, 2, 1024);
        for (provider, session, signature) in [
            ("a", "s", "sig-a"),
            ("b", "s", "sig-b"),
            ("a", "other", "sig-other"),
        ] {
            store.record_assistant_turn(
                provider,
                session,
                json!({}),
                vec![GeminiToolCallMeta::new(
                    Some("same-id"),
                    "Glob",
                    json!({}),
                    Some(signature),
                )],
            );
        }
        assert!(store
            .get_session("a", "s", ["same-id"])
            .unwrap()
            .thought_signatures
            .is_empty());
        assert_eq!(
            store
                .get_session("b", "s", ["same-id"])
                .unwrap()
                .thought_signatures["same-id"],
            "sig-b"
        );
        assert_eq!(
            store
                .get_session("a", "other", ["same-id"])
                .unwrap()
                .thought_signatures["same-id"],
            "sig-other"
        );
    }

    #[test]
    fn signature_cache_enforces_byte_budget_and_updates_existing_ids() {
        let store = GeminiShadowStore::with_signature_limits(8, 1, 8, 128);
        for id in ["first", "second"] {
            store.record_assistant_turn(
                "p",
                "s",
                json!({}),
                vec![GeminiToolCallMeta::new(
                    Some(id),
                    "Glob",
                    json!({}),
                    Some("x".repeat(80)),
                )],
            );
        }
        let signatures = store
            .get_session("p", "s", ["first", "second"])
            .unwrap()
            .thought_signatures;
        assert!(!signatures.contains_key("first"));
        assert_eq!(signatures["second"], "x".repeat(80));
        store.record_assistant_turn(
            "p",
            "s",
            json!({}),
            vec![GeminiToolCallMeta::new(
                Some("second"),
                "Glob",
                json!({}),
                Some("updated"),
            )],
        );
        assert_eq!(
            store
                .get_session("p", "s", ["second"])
                .unwrap()
                .thought_signatures["second"],
            "updated"
        );
        store.record_assistant_turn(
            "p",
            "s",
            json!({}),
            vec![GeminiToolCallMeta::new(
                Some("oversize"),
                "Glob",
                json!({}),
                Some("x".repeat(200)),
            )],
        );
        let signatures = store
            .get_session("p", "s", ["oversize", "second"])
            .unwrap()
            .thought_signatures;
        assert!(!signatures.contains_key("oversize"));
        assert_eq!(signatures["second"], "updated");
    }

    #[test]
    fn record_and_read_latest_turn() {
        let store = GeminiShadowStore::with_limits(8, 4);
        store.record_assistant_turn(
            "provider-a",
            "session-1",
            json!({"parts": [{"text": "hello", "thoughtSignature": "sig-1"}]}),
            vec![GeminiToolCallMeta::new(
                Some("call-1"),
                "get_weather",
                json!({"location": "Tokyo"}),
                Some("sig-1"),
            )],
        );

        let snapshot = store.get_session("provider-a", "session-1", []).unwrap();
        assert!(snapshot.thought_signatures.is_empty());
        assert_eq!(snapshot.provider_id, "provider-a");
        assert_eq!(snapshot.session_id, "session-1");
        assert_eq!(snapshot.turns.len(), 1);

        let content = store
            .latest_assistant_content("provider-a", "session-1")
            .expect("content");
        assert_eq!(content["parts"][0]["text"], "hello");
        assert_eq!(content["parts"][0]["thoughtSignature"], "sig-1");

        let tool_calls = store
            .latest_tool_calls("provider-a", "session-1")
            .expect("tool calls");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id.as_deref(), Some("call-1"));
        assert_eq!(tool_calls[0].name, "get_weather");
        assert_eq!(tool_calls[0].args["location"], "Tokyo");
        assert_eq!(tool_calls[0].thought_signature.as_deref(), Some("sig-1"));
    }

    #[test]
    fn sessions_are_isolated_by_provider_and_session_id() {
        let store = GeminiShadowStore::with_limits(8, 4);

        store.record_assistant_turn("provider-a", "session-1", json!({"text": "a"}), vec![]);
        store.record_assistant_turn("provider-b", "session-1", json!({"text": "b"}), vec![]);
        store.record_assistant_turn("provider-a", "session-2", json!({"text": "c"}), vec![]);

        assert_eq!(store.session_count(), 3);
        assert_eq!(
            store.latest_assistant_content("provider-a", "session-1"),
            Some(json!({"text": "a"}))
        );
        assert_eq!(
            store.latest_assistant_content("provider-b", "session-1"),
            Some(json!({"text": "b"}))
        );
        assert_eq!(
            store.latest_assistant_content("provider-a", "session-2"),
            Some(json!({"text": "c"}))
        );
    }

    #[test]
    fn retains_only_latest_turns_per_session() {
        let store = GeminiShadowStore::with_limits(8, 2);

        store.record_assistant_turn("provider-a", "session-1", json!({"idx": 1}), vec![]);
        store.record_assistant_turn("provider-a", "session-1", json!({"idx": 2}), vec![]);
        store.record_assistant_turn("provider-a", "session-1", json!({"idx": 3}), vec![]);

        let snapshot = store
            .get_session("provider-a", "session-1", [])
            .expect("snapshot");
        assert_eq!(snapshot.turns.len(), 2);
        assert_eq!(snapshot.turns[0].assistant_content, json!({"idx": 2}));
        assert_eq!(snapshot.turns[1].assistant_content, json!({"idx": 3}));
    }

    #[test]
    fn evicts_oldest_session_when_capacity_is_exceeded() {
        let store = GeminiShadowStore::with_limits(2, 2);

        store.record_assistant_turn("provider-a", "session-1", json!({"idx": 1}), vec![]);
        store.record_assistant_turn("provider-a", "session-2", json!({"idx": 2}), vec![]);
        store.record_assistant_turn("provider-a", "session-3", json!({"idx": 3}), vec![]);

        assert!(store.get_session("provider-a", "session-1", []).is_none());
        assert!(store.get_session("provider-a", "session-2", []).is_some());
        assert!(store.get_session("provider-a", "session-3", []).is_some());
    }
}
