//! Gemini Native shadow state
//!
//! Keeps provider/session-scoped assistant content snapshots and tool call metadata
//! so Gemini thought signatures and tool turns can be replayed without bloating
//! the main proxy files.

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

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
}

impl GeminiShadowInner {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            session_order: VecDeque::new(),
        }
    }
}

/// Thread-safe shadow store for Gemini Native replay state.
///
/// The store is intentionally small and explicit:
/// - sessions are keyed by `(provider_id, session_id)`
/// - each session keeps only a bounded number of recent assistant turns
/// - the oldest session is evicted first when the store is full
#[derive(Debug)]
pub struct GeminiShadowStore {
    max_sessions: usize,
    max_turns_per_session: usize,
    inner: RwLock<GeminiShadowInner>,
}

impl Default for GeminiShadowStore {
    fn default() -> Self {
        Self::with_limits(200, 64)
    }
}

impl GeminiShadowStore {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_limits(max_sessions: usize, max_turns_per_session: usize) -> Self {
        Self {
            max_sessions: max_sessions.max(1),
            max_turns_per_session: max_turns_per_session.max(1),
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
    ) -> GeminiShadowSessionSnapshot {
        let key = GeminiShadowKey::new(provider_id, session_id);
        let turn = GeminiAssistantTurn::new(assistant_content, tool_calls);

        let mut inner = self.inner.write().expect("gemini shadow lock poisoned");
        Self::touch_session_order(&mut inner.session_order, &key);

        let snapshot = {
            let session = inner
                .sessions
                .entry(key.clone())
                .or_insert_with(GeminiShadowSession::new);
            session.turns.push_back(turn);
            while session.turns.len() > self.max_turns_per_session {
                session.turns.pop_front();
            }
            Self::snapshot_session(&key, session)
        };
        Self::prune_sessions(&mut inner, self.max_sessions);
        snapshot
    }

    /// Get the latest assistant content for a provider/session pair.
    #[allow(dead_code)]
    pub fn latest_assistant_content(&self, provider_id: &str, session_id: &str) -> Option<Value> {
        self.get_session(provider_id, session_id)
            .and_then(|snapshot| {
                snapshot
                    .turns
                    .last()
                    .map(|turn| turn.assistant_content.clone())
            })
    }

    /// Get the latest tool calls for a provider/session pair.
    #[allow(dead_code)]
    pub fn latest_tool_calls(
        &self,
        provider_id: &str,
        session_id: &str,
    ) -> Option<Vec<GeminiToolCallMeta>> {
        self.get_session(provider_id, session_id)
            .and_then(|snapshot| snapshot.turns.last().map(|turn| turn.tool_calls.clone()))
    }

    /// Read a full session snapshot.
    pub fn get_session(
        &self,
        provider_id: &str,
        session_id: &str,
    ) -> Option<GeminiShadowSessionSnapshot> {
        let key = GeminiShadowKey::new(provider_id, session_id);
        let mut inner = self.inner.write().expect("gemini shadow lock poisoned");
        let snapshot = inner
            .sessions
            .get(&key)
            .map(|session| Self::snapshot_session(&key, session));
        if snapshot.is_some() {
            Self::touch_session_order(&mut inner.session_order, &key);
        }
        snapshot
    }

    /// Remove a single session from the store.
    #[allow(dead_code)]
    pub fn clear_session(&self, provider_id: &str, session_id: &str) -> bool {
        let key = GeminiShadowKey::new(provider_id, session_id);
        let mut inner = self.inner.write().expect("gemini shadow lock poisoned");
        let removed = inner.sessions.remove(&key).is_some();
        if removed {
            Self::remove_key_from_order(&mut inner.session_order, &key);
        }
        removed
    }

    /// Remove all sessions for a provider.
    #[allow(dead_code)]
    pub fn clear_provider(&self, provider_id: &str) -> usize {
        let mut inner = self.inner.write().expect("gemini shadow lock poisoned");
        let keys: Vec<_> = inner
            .sessions
            .keys()
            .filter(|key| key.provider_id == provider_id)
            .cloned()
            .collect();
        for key in &keys {
            inner.sessions.remove(key);
            Self::remove_key_from_order(&mut inner.session_order, key);
        }
        keys.len()
    }

    /// Number of tracked sessions.
    #[allow(dead_code)]
    pub fn session_count(&self) -> usize {
        self.inner
            .read()
            .expect("gemini shadow lock poisoned")
            .sessions
            .len()
    }

    fn snapshot_session(
        key: &GeminiShadowKey,
        session: &GeminiShadowSession,
    ) -> GeminiShadowSessionSnapshot {
        GeminiShadowSessionSnapshot {
            provider_id: key.provider_id.clone(),
            session_id: key.session_id.clone(),
            turns: session.turns.iter().cloned().collect(),
        }
    }

    fn touch_session_order(order: &mut VecDeque<GeminiShadowKey>, key: &GeminiShadowKey) {
        if let Some(pos) = order.iter().position(|existing| existing == key) {
            order.remove(pos);
        }
        order.push_back(key.clone());
    }

    #[allow(dead_code)]
    fn remove_key_from_order(order: &mut VecDeque<GeminiShadowKey>, key: &GeminiShadowKey) {
        if let Some(pos) = order.iter().position(|existing| existing == key) {
            order.remove(pos);
        }
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
    fn record_and_read_latest_turn() {
        let store = GeminiShadowStore::with_limits(8, 4);
        let snapshot = store.record_assistant_turn(
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
            .get_session("provider-a", "session-1")
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

        assert!(store.get_session("provider-a", "session-1").is_none());
        assert!(store.get_session("provider-a", "session-2").is_some());
        assert!(store.get_session("provider-a", "session-3").is_some());
    }

    #[test]
    fn clear_session_and_provider_work() {
        let store = GeminiShadowStore::with_limits(8, 4);

        store.record_assistant_turn("provider-a", "session-1", json!({"idx": 1}), vec![]);
        store.record_assistant_turn("provider-a", "session-2", json!({"idx": 2}), vec![]);
        store.record_assistant_turn("provider-b", "session-3", json!({"idx": 3}), vec![]);

        assert!(store.clear_session("provider-a", "session-1"));
        assert!(store.get_session("provider-a", "session-1").is_none());

        let removed = store.clear_provider("provider-a");
        assert_eq!(removed, 1);
        assert!(store.get_session("provider-a", "session-2").is_none());
        assert!(store.get_session("provider-b", "session-3").is_some());
    }
}
