use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::Utc;

use database::TandemDatabase;

pub mod database;
pub mod domain;
pub mod repository;
pub mod tray_summary;

pub trait Clock: Send + Sync {
    fn now_ms(&self) -> i64;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        Utc::now().timestamp_millis()
    }
}

pub struct TandemState {
    pub db: Option<Arc<TandemDatabase>>,
    pub init_error: Option<String>,
    clock: Arc<dyn Clock>,
}

impl TandemState {
    pub fn available(db: Arc<TandemDatabase>, clock: Arc<dyn Clock>) -> Self {
        Self {
            db: Some(db),
            init_error: None,
            clock,
        }
    }

    pub fn unavailable(init_error: String, clock: Arc<dyn Clock>) -> Self {
        Self {
            db: None,
            init_error: Some(init_error),
            clock,
        }
    }

    pub fn initialize(app_data_dir: &Path) -> Self {
        let path = tandem_database_path(app_data_dir);
        let result = path
            .parent()
            .ok_or_else(|| "Tandem database path has no parent directory".to_string())
            .and_then(|directory| {
                std::fs::create_dir_all(directory).map_err(|error| error.to_string())
            })
            .and_then(|()| {
                TandemDatabase::init(&path)
                    .map(Arc::new)
                    .map_err(|error| error.to_string())
            });

        match result {
            Ok(db) => Self::available(db, Arc::new(SystemClock)),
            Err(error) => Self::unavailable(error, Arc::new(SystemClock)),
        }
    }

    pub fn database(&self) -> Result<Arc<TandemDatabase>, String> {
        self.db.clone().ok_or_else(|| {
            format!(
                "Tandem unavailable: {}",
                self.init_error
                    .as_deref()
                    .unwrap_or("unknown initialization error")
            )
        })
    }

    pub fn now_ms(&self) -> i64 {
        self.clock.now_ms()
    }
}

pub fn tandem_database_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("tandem").join("tandem.db")
}
