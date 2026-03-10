//! h3 /feed endpoint for realtime agent events
//!
//! Mirrors Moltbook feed: agent posts, observations, security stack status.
//! Radio-aware - includes carrier state in events.

use axum::response::sse::Event;
use bytes::Bytes;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::interval;

pub use crate::openapi::{FeedEvent, FeedEventType};

/// Feed subscriber configuration
#[derive(Debug, Clone)]
pub struct FeedConfig {
    /// Maximum events to buffer
    pub buffer_size: usize,
    /// Heartbeat interval (seconds)
    pub heartbeat_secs: u64,
    /// Include radio state in heartbeats
    pub include_radio_state: bool,
}

impl Default for FeedConfig {
    fn default() -> Self {
        Self {
            buffer_size: 256,
            heartbeat_secs: 30,
            include_radio_state: true,
        }
    }
}

/// Feed broadcaster - shares events across all subscribers
pub struct FeedBroadcaster {
    sender: broadcast::Sender<FeedEvent>,
    config: FeedConfig,
}

impl FeedBroadcaster {
    pub fn new(config: FeedConfig) -> Self {
        let (sender, _) = broadcast::channel(config.buffer_size);
        Self { sender, config }
    }

    /// Publish an event to all subscribers
    pub fn publish(&self, event: FeedEvent) {
        // Ignore send errors (no subscribers)
        let _ = self.sender.send(event);
    }

    /// Publish an observation event
    pub fn observation(&self, agent: &str, content: &str) {
        self.publish(FeedEvent {
            event_type: FeedEventType::Observation,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "agent": agent,
                "content": content,
            }),
        });
    }

    /// Publish a tool call event
    pub fn tool_call(&self, tool: &str, status: &str, result: Option<&str>) {
        self.publish(FeedEvent {
            event_type: FeedEventType::ToolCall,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "tool": tool,
                "status": status,
                "result": result,
            }),
        });
    }

    /// Publish a radio state change
    pub fn radio_state(&self, interface: &str, state: &str, quality: f32) {
        self.publish(FeedEvent {
            event_type: FeedEventType::RadioState,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "interface": interface,
                "state": state,
                "quality": quality,
            }),
        });
    }

    /// Publish a carrier handoff event
    pub fn carrier_handoff(&self, from: &str, to: &str, reason: &str) {
        self.publish(FeedEvent {
            event_type: FeedEventType::CarrierHandoff,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "from_carrier": from,
                "to_carrier": to,
                "reason": reason,
            }),
        });
    }

    /// Publish a key rotation event
    pub fn key_rotation(&self, provider: &str, old_key_id: &str, new_key_id: &str) {
        self.publish(FeedEvent {
            event_type: FeedEventType::KeyRotation,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "provider": provider,
                "old_key_id": old_key_id,
                "new_key_id": new_key_id,
            }),
        });
    }

    /// Publish a failover event
    pub fn failover(&self, provider: &str, from_model: &str, to_model: &str, reason: &str) {
        self.publish(FeedEvent {
            event_type: FeedEventType::Failover,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "provider": provider,
                "from_model": from_model,
                "to_model": to_model,
                "reason": reason,
            }),
        });
    }

    /// Publish a security event
    pub fn security(&self, event: &str, severity: &str, details: &str) {
        self.publish(FeedEvent {
            event_type: FeedEventType::Security,
            timestamp: chrono::Utc::now().timestamp(),
            data: serde_json::json!({
                "event": event,
                "severity": severity,
                "details": details,
            }),
        });
    }

    /// Subscribe to the feed
    pub fn subscribe(&self) -> broadcast::Receiver<FeedEvent> {
        self.sender.subscribe()
    }

    /// Get the config
    pub fn config(&self) -> &FeedConfig {
        &self.config
    }
}

/// Create SSE stream from feed events
pub fn create_feed_stream(
    mut receiver: broadcast::Receiver<FeedEvent>,
    config: FeedConfig,
) -> impl Stream<Item = Result<Event, std::io::Error>> {
    let mut heartbeat = interval(Duration::from_secs(config.heartbeat_secs));

    async_stream::try_stream! {
        loop {
            tokio::select! {
                // Receive events
                Ok(event) = receiver.recv() => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Event::default().data(json);
                }

                // Heartbeat
                _ = heartbeat.tick() => {
                    let heartbeat_event = FeedEvent {
                        event_type: FeedEventType::RadioState,
                        timestamp: chrono::Utc::now().timestamp(),
                        data: if config.include_radio_state {
                            // TODO: Get actual radio state from carrier module
                            serde_json::json!({
                                "interface": "rmnet0",
                                "state": "connected",
                                "quality": 0.95,
                            })
                        } else {
                            serde_json::json!({ "ping": true })
                        },
                    };
                    let json = serde_json::to_string(&heartbeat_event).unwrap_or_default();
                    yield Event::default().data(json);
                }

                // Channel closed
                else => break,
            }
        }
    }
}

/// SSE content type helper
pub fn sse_content_type() -> axum::http::header::HeaderValue {
    axum::http::header::HeaderValue::from_static("text/event-stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feed_broadcaster() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.observation("test-agent", "Hello world");

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::Observation));
    }

    #[test]
    fn test_tool_call_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.tool_call("read_file", "completed", Some("file contents"));

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::ToolCall));
        assert!(event.data["tool"].as_str().unwrap() == "read_file");
    }

    #[test]
    fn test_radio_state_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.radio_state("rmnet0", "connected", 0.95);

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::RadioState));
    }

    #[test]
    fn test_carrier_handoff_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.carrier_handoff("cellular", "wifi", "better_latency");

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::CarrierHandoff));
    }

    #[test]
    fn test_key_rotation_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.key_rotation("anthropic", "key-1", "key-2");

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::KeyRotation));
    }

    #[test]
    fn test_failover_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.failover("openai", "gpt-4", "gpt-3.5-turbo", "rate_limit");

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::Failover));
    }

    #[test]
    fn test_security_event() {
        let broadcaster = FeedBroadcaster::new(FeedConfig::default());
        let mut receiver = broadcaster.subscribe();

        broadcaster.security("suspicious_request", "warning", "Detected malformed input");

        let event = receiver.try_recv().unwrap();
        assert!(matches!(event.event_type, FeedEventType::Security));
    }
}
