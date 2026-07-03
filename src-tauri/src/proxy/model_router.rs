//! Model-based provider routing.
//!
//! This layer runs before normal app-level provider selection. When no enabled
//! model route matches, callers fall back to `ProviderRouter`.

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use std::sync::Arc;

pub struct ModelRouter {
    db: Arc<Database>,
}

impl ModelRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn select_provider(
        &self,
        app_type: &str,
        model: &str,
    ) -> Result<Option<Provider>, AppError> {
        if model.trim().is_empty() || model == "unknown" {
            return Ok(None);
        }

        for route in self.db.get_model_routes(app_type)? {
            if !route.enabled || !model_pattern_matches(&route.pattern, model) {
                continue;
            }

            match self.db.get_provider_by_id(&route.provider_id, app_type)? {
                Some(provider) => return Ok(Some(provider)),
                None => {
                    log::warn!(
                        "[{app_type}] Model route '{}' matched '{}' but provider '{}' is missing; falling back",
                        route.pattern,
                        model,
                        route.provider_id
                    );
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }
}

fn model_pattern_matches(pattern: &str, model: &str) -> bool {
    let pattern = pattern.trim();
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == model;
    }

    let mut remainder = model;
    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');
    let parts: Vec<&str> = pattern.split('*').filter(|part| !part.is_empty()).collect();

    if parts.is_empty() {
        return true;
    }

    if !starts_with_wildcard {
        let Some(first) = parts.first() else {
            return true;
        };
        let Some(stripped) = remainder.strip_prefix(first) else {
            return false;
        };
        remainder = stripped;
    }

    let start_index = usize::from(!starts_with_wildcard);
    let end_index = if ends_with_wildcard {
        parts.len()
    } else {
        parts.len().saturating_sub(1)
    };

    for part in &parts[start_index..end_index] {
        let Some(index) = remainder.find(part) else {
            return false;
        };
        remainder = &remainder[index + part.len()..];
    }

    if !ends_with_wildcard {
        let Some(last) = parts.last() else {
            return true;
        };
        return remainder.ends_with(last);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ModelRouteInput, Provider};
    use serde_json::json;

    fn test_provider(id: &str, name: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: name.to_string(),
            settings_config: json!({ "env": { "ANTHROPIC_BASE_URL": "https://example.com" } }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn wildcard_model_patterns_match_expected_models() {
        assert!(model_pattern_matches("*opus*", "claude-opus-4-8"));
        assert!(model_pattern_matches("claude-*", "claude-sonnet-4-5"));
        assert!(model_pattern_matches("*-4-5", "claude-sonnet-4-5"));
        assert!(model_pattern_matches(
            "claude-*sonnet*",
            "claude-4-sonnet-latest"
        ));
        assert!(model_pattern_matches("claude-opus-4-8", "claude-opus-4-8"));
        assert!(!model_pattern_matches(
            "claude-opus-4-8",
            "claude-sonnet-4-5"
        ));
        assert!(!model_pattern_matches("opus*", "claude-opus-4-8"));
    }

    #[tokio::test]
    async fn model_router_selects_highest_priority_matching_provider() {
        let db = Arc::new(crate::database::Database::memory().expect("create memory db"));
        db.save_provider("claude", &test_provider("low", "Low"))
            .expect("save low");
        db.save_provider("claude", &test_provider("high", "High"))
            .expect("save high");

        db.create_model_route(ModelRouteInput {
            app_type: "claude".to_string(),
            pattern: "*opus*".to_string(),
            provider_id: "low".to_string(),
            priority: 10,
            enabled: true,
        })
        .expect("create low route");
        db.create_model_route(ModelRouteInput {
            app_type: "claude".to_string(),
            pattern: "*opus*".to_string(),
            provider_id: "high".to_string(),
            priority: 100,
            enabled: true,
        })
        .expect("create high route");

        let router = ModelRouter::new(db);
        let provider = router
            .select_provider("claude", "claude-opus-4-8")
            .await
            .expect("select provider")
            .expect("route should match");

        assert_eq!(provider.id, "high");
    }

    #[tokio::test]
    async fn model_router_ignores_disabled_and_missing_routes() {
        let db = Arc::new(crate::database::Database::memory().expect("create memory db"));
        db.save_provider("claude", &test_provider("disabled", "Disabled"))
            .expect("save disabled");

        db.create_model_route(ModelRouteInput {
            app_type: "claude".to_string(),
            pattern: "*sonnet*".to_string(),
            provider_id: "disabled".to_string(),
            priority: 100,
            enabled: false,
        })
        .expect("create disabled route");

        let router = ModelRouter::new(db);
        let provider = router
            .select_provider("claude", "claude-sonnet-4-5")
            .await
            .expect("select provider");

        assert!(provider.is_none());
    }
}
