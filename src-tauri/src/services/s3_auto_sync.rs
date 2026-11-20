//! S3 backend for the shared auto sync worker in [`super::auto_sync`].

use std::future::Future;

use serde_json::Value;

use super::auto_sync::{self, AutoSyncBackend, AutoSyncState};
use crate::database::Database;
use crate::error::AppError;
use crate::services::s3_sync;
use crate::settings::{self, S3SyncSettings, WebDavSyncStatus};

pub(crate) struct S3AutoSync;

static STATE: AutoSyncState = AutoSyncState::new();

impl AutoSyncBackend for S3AutoSync {
    type Settings = S3SyncSettings;
    const NAME: &'static str = "S3";
    const STATUS_EVENT: &'static str = "s3-sync-status-updated";

    fn state() -> &'static AutoSyncState {
        &STATE
    }
    fn load() -> Option<S3SyncSettings> {
        settings::get_s3_sync_settings()
    }
    fn wants_auto_sync(settings: &S3SyncSettings) -> bool {
        settings.enabled && settings.auto_sync
    }
    fn status_mut(settings: &mut S3SyncSettings) -> &mut WebDavSyncStatus {
        &mut settings.status
    }
    fn store_status(status: WebDavSyncStatus) -> Result<(), AppError> {
        settings::update_s3_sync_status(status)
    }
    fn upload<'a>(
        db: &'a Database,
        settings: &'a mut S3SyncSettings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a {
        s3_sync::upload(db, settings)
    }
}

pub(crate) type AutoSyncSuppressionGuard = auto_sync::SuppressionGuard<S3AutoSync>;

#[cfg(test)]
pub(crate) fn is_auto_sync_suppressed() -> bool {
    auto_sync::is_suppressed::<S3AutoSync>()
}
