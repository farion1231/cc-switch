//! Provider service - business logic for provider management

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, UniversalProvider};
use crate::store::AppState;

/// Provider sort update request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderSortUpdate {
    pub id: String,
    #[serde(rename = "sortIndex")]
    pub sort_index: usize,
}

/// Provider business logic service
pub struct ProviderService;

impl ProviderService {
    /// List all providers for an app type
    pub fn list(
        state: &AppState,
        app_type: AppType,
    ) -> Result<IndexMap<String, Provider>, AppError> {
        state.db.get_all_providers(app_type.as_str())
    }

    /// Get current provider ID
    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        if matches!(app_type, AppType::OpenCode) {
            return Ok(String::new());
        }
        state
            .db
            .get_current_provider(app_type.as_str())
            .map(|opt| opt.unwrap_or_default())
    }

    /// Add a new provider
    pub fn add(state: &AppState, app_type: AppType, provider: Provider) -> Result<bool, AppError> {
        state.db.save_provider(app_type.as_str(), &provider)?;

        if matches!(app_type, AppType::OpenCode) {
            return Ok(true);
        }

        let current = state.db.get_current_provider(app_type.as_str())?;
        if current.is_none() {
            state
                .db
                .set_current_provider(app_type.as_str(), &provider.id)?;
        }

        Ok(true)
    }

    /// Update a provider
    pub fn update(
        state: &AppState,
        app_type: AppType,
        provider: Provider,
    ) -> Result<bool, AppError> {
        state.db.save_provider(app_type.as_str(), &provider)
    }

    /// Delete a provider
    pub fn delete(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        if matches!(app_type, AppType::OpenCode) {
            state.db.delete_provider(app_type.as_str(), id)?;
            return Ok(());
        }

        let current = state.db.get_current_provider(app_type.as_str())?;
        if current.as_deref() == Some(id) {
            return Err(AppError::Message(
                "Cannot delete the currently active provider".to_string(),
            ));
        }

        state.db.delete_provider(app_type.as_str(), id)
    }

    /// Switch to a provider
    pub fn switch(state: &AppState, app_type: AppType, id: &str) -> Result<(), AppError> {
        let providers = state.db.get_all_providers(app_type.as_str())?;
        let _provider = providers
            .get(id)
            .ok_or_else(|| AppError::Message(format!("Provider {} not found", id)))?;

        if !matches!(app_type, AppType::OpenCode) {
            state.db.set_current_provider(app_type.as_str(), id)?;
        }

        Ok(())
    }

    /// Update provider sort order
    pub fn update_sort_order(
        state: &AppState,
        app_type: AppType,
        updates: Vec<ProviderSortUpdate>,
    ) -> Result<bool, AppError> {
        let mut providers = state.db.get_all_providers(app_type.as_str())?;

        for update in updates {
            if let Some(provider) = providers.get_mut(&update.id) {
                let mut p = provider.clone();
                p.sort_index = Some(update.sort_index);
                state.db.save_provider(app_type.as_str(), &p)?;
            }
        }

        Ok(true)
    }

    // ========== Universal Provider Methods ==========

    /// Get all universal providers
    pub fn list_universal(
        state: &AppState,
    ) -> Result<HashMap<String, UniversalProvider>, AppError> {
        state.db.get_all_universal_providers()
    }

    /// Get a single universal provider
    pub fn get_universal(
        state: &AppState,
        id: &str,
    ) -> Result<Option<UniversalProvider>, AppError> {
        state.db.get_universal_provider(id)
    }

    /// Add or update a universal provider
    pub fn upsert_universal(
        state: &AppState,
        provider: UniversalProvider,
    ) -> Result<bool, AppError> {
        state.db.save_universal_provider(&provider)
    }

    /// Sync universal provider to apps
    pub fn sync_universal_to_apps(state: &AppState, id: &str) -> Result<(), AppError> {
        let provider = state
            .db
            .get_universal_provider(id)?
            .ok_or_else(|| AppError::Message(format!("Universal provider {} not found", id)))?;

        if provider.apps.claude {
            if let Some(claude_provider) = provider.to_claude_provider() {
                state.db.save_provider("claude", &claude_provider)?;
            }
        }

        if provider.apps.codex {
            if let Some(codex_provider) = provider.to_codex_provider() {
                state.db.save_provider("codex", &codex_provider)?;
            }
        }

        if provider.apps.gemini {
            if let Some(gemini_provider) = provider.to_gemini_provider() {
                state.db.save_provider("gemini", &gemini_provider)?;
            }
        }

        Ok(())
    }

    /// Delete a universal provider
    pub fn delete_universal(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_universal_provider(id)
    }
}

/// Endpoint latency result for speed test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointLatency {
    pub url: String,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}
