use serde::Serialize;
use std::sync::{OnceLock, RwLock};

#[derive(Debug, Clone, Serialize)]
pub struct InitErrorPayload {
    pub path: String,
    pub error: String,
}

static INIT_ERROR: OnceLock<RwLock<Option<InitErrorPayload>>> = OnceLock::new();

fn cell() -> &'static RwLock<Option<InitErrorPayload>> {
    INIT_ERROR.get_or_init(|| RwLock::new(None))
}

#[allow(dead_code)]
pub fn set_init_error(payload: InitErrorPayload) {
    #[allow(clippy::unwrap_used)]
    if let Ok(mut guard) = cell().write() {
        *guard = Some(payload);
    }
}

pub fn get_init_error() -> Option<InitErrorPayload> {
    cell().read().ok()?.clone()
}

// ============================================================
// 迁移结果状态
// ============================================================

static MIGRATION_SUCCESS: OnceLock<RwLock<bool>> = OnceLock::new();

fn migration_cell() -> &'static RwLock<bool> {
    MIGRATION_SUCCESS.get_or_init(|| RwLock::new(false))
}

pub fn set_migration_success() {
    if let Ok(mut guard) = migration_cell().write() {
        *guard = true;
    }
}

/// 获取并消费迁移成功状态（只返回一次 true，之后返回 false）
pub fn take_migration_success() -> bool {
    if let Ok(mut guard) = migration_cell().write() {
        let val = *guard;
        *guard = false;
        val
    } else {
        false
    }
}

// ============================================================
// Skills SSOT 迁移结果状态
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct SkillsMigrationPayload {
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

static SKILLS_MIGRATION_RESULT: OnceLock<RwLock<Option<SkillsMigrationPayload>>> = OnceLock::new();

fn skills_migration_cell() -> &'static RwLock<Option<SkillsMigrationPayload>> {
    SKILLS_MIGRATION_RESULT.get_or_init(|| RwLock::new(None))
}

pub fn set_skills_migration_result(count: usize) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload { count, error: None });
    }
}

pub fn set_skills_migration_error(error: String) {
    if let Ok(mut guard) = skills_migration_cell().write() {
        *guard = Some(SkillsMigrationPayload {
            count: 0,
            error: Some(error),
        });
    }
}

/// 获取并消费 Skills 迁移结果（只返回一次 Some，之后返回 None）
pub fn take_skills_migration_result() -> Option<SkillsMigrationPayload> {
    if let Ok(mut guard) = skills_migration_cell().write() {
        guard.take()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_error_roundtrip() {
        let payload = InitErrorPayload {
            path: "/tmp/config.json".into(),
            error: "broken json".into(),
        };
        set_init_error(payload.clone());
        let got = get_init_error().expect("should get payload back");
        assert_eq!(got.path, payload.path);
        assert_eq!(got.error, payload.error);
    }
}
