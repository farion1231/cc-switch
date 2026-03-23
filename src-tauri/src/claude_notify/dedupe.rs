use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::types::ClaudeNotifyEventType;

const DEDUPE_WINDOW: Duration = Duration::from_secs(8);

#[derive(Default)]
pub struct ClaudeNotifyDedupe {
    events: HashMap<String, Instant>,
}

impl ClaudeNotifyDedupe {
    pub fn should_emit(&mut self, session_id: &str, event_type: &ClaudeNotifyEventType) -> bool {
        let now = Instant::now();
        self.events
            .retain(|_, ts| now.duration_since(*ts) <= DEDUPE_WINDOW);

        let key = format!("{session_id}:{:?}", event_type);
        if self.events.contains_key(&key) {
            return false;
        }

        match event_type {
            ClaudeNotifyEventType::PermissionPrompt => {
                self.events.retain(|key, _| {
                    !key.starts_with(&format!("{session_id}:IdlePrompt"))
                        && !key.starts_with(&format!("{session_id}:Stop"))
                });
            }
            ClaudeNotifyEventType::IdlePrompt => {
                self.events
                    .retain(|key, _| !key.starts_with(&format!("{session_id}:Stop")));
            }
            ClaudeNotifyEventType::Stop => {
                if self
                    .events
                    .contains_key(&format!("{session_id}:IdlePrompt"))
                    || self
                        .events
                        .contains_key(&format!("{session_id}:PermissionPrompt"))
                {
                    return false;
                }
            }
        }

        self.events.insert(key, now);
        true
    }
}
