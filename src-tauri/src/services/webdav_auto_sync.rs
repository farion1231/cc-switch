//! WebDAV backend for the shared auto sync worker in [`super::auto_sync`].

use std::future::Future;

use serde_json::Value;

use super::auto_sync::{self, AutoSyncBackend, AutoSyncState};
use crate::database::Database;
use crate::error::AppError;
use crate::services::webdav_sync;
use crate::settings::{self, WebDavSyncSettings, WebDavSyncStatus};

pub(crate) struct WebDavAutoSync;

static STATE: AutoSyncState = AutoSyncState::new();

impl AutoSyncBackend for WebDavAutoSync {
    type Settings = WebDavSyncSettings;
    const NAME: &'static str = "WebDAV";
    const STATUS_EVENT: &'static str = "webdav-sync-status-updated";

    fn state() -> &'static AutoSyncState {
        &STATE
    }
    fn load() -> Option<WebDavSyncSettings> {
        settings::get_webdav_sync_settings()
    }
    fn wants_auto_sync(settings: &WebDavSyncSettings) -> bool {
        settings.enabled && settings.auto_sync
    }
    fn status_mut(settings: &mut WebDavSyncSettings) -> &mut WebDavSyncStatus {
        &mut settings.status
    }
    fn store_status(status: WebDavSyncStatus) -> Result<(), AppError> {
        settings::update_webdav_sync_status(status)
    }
    fn upload<'a>(
        db: &'a Database,
        settings: &'a mut WebDavSyncSettings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a {
        webdav_sync::upload(db, settings)
    }
}

pub(crate) type AutoSyncSuppressionGuard = auto_sync::SuppressionGuard<WebDavAutoSync>;

#[cfg(test)]
pub(crate) fn is_auto_sync_suppressed() -> bool {
    auto_sync::is_suppressed::<WebDavAutoSync>()
}
