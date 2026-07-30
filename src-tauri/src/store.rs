use crate::database::Database;
use crate::deeplink::ProviderSwitchRequest;
use crate::error::AppError;
use crate::services::{ProxyService, UsageCache};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROVIDER_SWITCH_REVIEW_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_PROVIDER_SWITCH_REVIEWS: usize = 64;

#[derive(Clone, PartialEq)]
pub(crate) struct ProviderSwitchStateSnapshot {
    pub target_provider: Value,
    pub device_current_provider_id: Option<String>,
    pub device_current_provider: Option<Value>,
    pub database_current_provider_id: Option<String>,
    pub database_current_provider: Option<Value>,
    pub effective_current_provider_id: Option<String>,
    pub live_settings: Value,
    pub live_auth_bytes: Vec<u8>,
}

pub(crate) struct ProviderSwitchReview {
    pub request: ProviderSwitchRequest,
    pub snapshot: ProviderSwitchStateSnapshot,
    created_at: Instant,
}

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    provider_switch_reviews: Mutex<HashMap<String, ProviderSwitchReview>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            provider_switch_reviews: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn issue_provider_switch_review(
        &self,
        request: ProviderSwitchRequest,
        snapshot: ProviderSwitchStateSnapshot,
    ) -> Result<String, AppError> {
        let now = Instant::now();
        let mut reviews = self.provider_switch_reviews.lock()?;
        reviews.retain(|_, review| {
            now.duration_since(review.created_at) <= PROVIDER_SWITCH_REVIEW_TTL
        });
        if reviews.len() >= MAX_PROVIDER_SWITCH_REVIEWS {
            if let Some(oldest) = reviews
                .iter()
                .min_by_key(|(_, review)| review.created_at)
                .map(|(token, _)| token.clone())
            {
                reviews.remove(&oldest);
            }
        }

        let token = loop {
            let candidate = Uuid::new_v4().to_string();
            if !reviews.contains_key(&candidate) {
                break candidate;
            }
        };
        reviews.insert(
            token.clone(),
            ProviderSwitchReview {
                request,
                snapshot,
                created_at: now,
            },
        );
        Ok(token)
    }

    pub(crate) fn has_active_provider_switch_reviews(&self) -> Result<bool, AppError> {
        let now = Instant::now();
        let mut reviews = self.provider_switch_reviews.lock()?;
        reviews.retain(|_, review| {
            now.duration_since(review.created_at) <= PROVIDER_SWITCH_REVIEW_TTL
        });
        Ok(!reviews.is_empty())
    }

    pub(crate) fn cancel_provider_switch_review(&self, token: &str) -> Result<(), AppError> {
        self.provider_switch_reviews.lock()?.remove(token);
        Ok(())
    }

    pub(crate) fn take_provider_switch_review(
        &self,
        token: &str,
    ) -> Result<ProviderSwitchReview, AppError> {
        let mut reviews = self.provider_switch_reviews.lock()?;
        let review = reviews.remove(token).ok_or_else(|| {
            AppError::InvalidInput(
                "Provider switch review is missing or expired; review the request again"
                    .to_string(),
            )
        })?;
        if review.created_at.elapsed() > PROVIDER_SWITCH_REVIEW_TTL {
            return Err(AppError::InvalidInput(
                "Provider switch review is missing or expired; review the request again"
                    .to_string(),
            ));
        }
        Ok(review)
    }
}
