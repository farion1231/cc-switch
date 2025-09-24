use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::cloud_sync::models::{SyncType, OperationStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOperation {
    pub operation_id: String,
    pub operation_type: SyncType,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: OperationStatus,
    pub progress_percentage: u8,
    pub error_message: Option<String>,
    pub files_affected: Vec<PathBuf>,
    pub gist_id: Option<String>,
    pub bytes_transferred: u64,
}

impl SyncOperation {
    pub fn new(operation_id: String, operation_type: SyncType) -> Self {
        Self {
            operation_id,
            operation_type,
            started_at: Utc::now(),
            completed_at: None,
            status: OperationStatus::Pending,
            progress_percentage: 0,
            error_message: None,
            files_affected: Vec::new(),
            gist_id: None,
            bytes_transferred: 0,
        }
    }

    pub fn start(&mut self) {
        self.status = OperationStatus::InProgress;
        self.progress_percentage = 0;
    }

    pub fn update_progress(&mut self, percentage: u8) {
        self.progress_percentage = percentage.min(100);
    }

    pub fn complete_success(&mut self) {
        self.status = OperationStatus::Success;
        self.progress_percentage = 100;
        self.completed_at = Some(Utc::now());
    }

    pub fn complete_failure(&mut self, error: String) {
        self.status = OperationStatus::Failed;
        self.error_message = Some(error);
        self.completed_at = Some(Utc::now());
    }

    pub fn cancel(&mut self) {
        self.status = OperationStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }
}