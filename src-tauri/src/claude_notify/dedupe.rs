use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::types::ClaudeNotifyEventType;

const DEDUPE_WINDOW: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct ClaudeNotifyDedupe {
    events: HashMap<String, Instant>,
}

impl ClaudeNotifyDedupe {
    pub fn should_emit(
        &mut self,
        session_id: &str,
        event_type: &ClaudeNotifyEventType,
        timestamp: Option<i64>,
    ) -> bool {
        let now = Instant::now();
        self.events
            .retain(|_, ts| now.duration_since(*ts) <= DEDUPE_WINDOW);

        let timestamp = timestamp.unwrap_or_default();
        let key = format!("{session_id}:{:?}:{timestamp}", event_type);
        if self.events.contains_key(&key) {
            return false;
        }

        match event_type {
            ClaudeNotifyEventType::PermissionPrompt => {
                let idle_prefix = format!("{session_id}:IdlePrompt:");
                let stop_prefix = format!("{session_id}:Stop:");
                self.events.retain(|key, _| {
                    !key.starts_with(&idle_prefix) && !key.starts_with(&stop_prefix)
                });
            }
            ClaudeNotifyEventType::IdlePrompt => {
                let stop_prefix = format!("{session_id}:Stop:");
                self.events.retain(|key, _| !key.starts_with(&stop_prefix));
            }
            ClaudeNotifyEventType::Stop => {
                let idle_prefix = format!("{session_id}:IdlePrompt:");
                let permission_prefix = format!("{session_id}:PermissionPrompt:");
                if self.events.keys().any(|key| {
                    key.starts_with(&idle_prefix) || key.starts_with(&permission_prefix)
                }) {
                    return false;
                }
            }
        }

        self.events.insert(key, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_allows_distinct_stop_events_in_same_session() {
        let mut dedupe = ClaudeNotifyDedupe::default();
        assert!(dedupe.should_emit("session-1", &ClaudeNotifyEventType::Stop, Some(1000)));
        assert!(dedupe.should_emit("session-1", &ClaudeNotifyEventType::Stop, Some(2000)));
    }

    #[test]
    fn dedupe_blocks_same_event_instance() {
        let mut dedupe = ClaudeNotifyDedupe::default();
        assert!(dedupe.should_emit("session-1", &ClaudeNotifyEventType::Stop, Some(1000)));
        assert!(!dedupe.should_emit("session-1", &ClaudeNotifyEventType::Stop, Some(1000)));
    }
}
