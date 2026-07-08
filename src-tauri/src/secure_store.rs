use crate::error::AppError;

#[cfg(not(test))]
const SERVICE_NAME: &str = "cc-switch";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    WebDavPassword,
    S3SecretAccessKey,
}

impl SecretKey {
    fn username(self) -> &'static str {
        match self {
            Self::WebDavPassword => "webdav-sync-password",
            Self::S3SecretAccessKey => "s3-sync-secret-access-key",
        }
    }

    #[cfg(not(test))]
    fn label(self) -> &'static str {
        match self {
            Self::WebDavPassword => "WebDAV password",
            Self::S3SecretAccessKey => "S3 secret access key",
        }
    }
}

#[cfg(not(test))]
fn secure_store_error(action: &str, key: SecretKey, error: impl std::fmt::Display) -> AppError {
    AppError::Config(format!(
        "无法{action}系统安全存储中的 {}: {error}",
        key.label()
    ))
}

#[cfg(not(test))]
mod imp {
    use super::{secure_store_error, SecretKey, SERVICE_NAME};
    use crate::error::AppError;
    use keyring::v1::{Entry, Error};

    fn entry(key: SecretKey) -> Result<Entry, AppError> {
        Entry::new(SERVICE_NAME, key.username())
            .map_err(|error| secure_store_error("访问", key, error))
    }

    pub fn get_secret(key: SecretKey) -> Result<Option<String>, AppError> {
        let entry = entry(key)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(secure_store_error("读取", key, error)),
        }
    }

    pub fn set_secret(key: SecretKey, value: &str) -> Result<(), AppError> {
        let entry = entry(key)?;
        entry
            .set_password(value)
            .map_err(|error| secure_store_error("写入", key, error))
    }

    pub fn delete_secret(key: SecretKey) -> Result<(), AppError> {
        let entry = entry(key)?;
        match entry.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => Ok(()),
            Err(error) => Err(secure_store_error("删除", key, error)),
        }
    }
}

#[cfg(test)]
mod imp {
    use super::SecretKey;
    use crate::error::AppError;
    use std::collections::HashMap;
    use std::sync::{OnceLock, RwLock};

    fn store() -> &'static RwLock<HashMap<&'static str, String>> {
        static STORE: OnceLock<RwLock<HashMap<&'static str, String>>> = OnceLock::new();
        STORE.get_or_init(|| RwLock::new(HashMap::new()))
    }

    pub fn get_secret(key: SecretKey) -> Result<Option<String>, AppError> {
        Ok(store().read()?.get(key.username()).cloned())
    }

    pub fn set_secret(key: SecretKey, value: &str) -> Result<(), AppError> {
        store().write()?.insert(key.username(), value.to_string());
        Ok(())
    }

    pub fn delete_secret(key: SecretKey) -> Result<(), AppError> {
        store().write()?.remove(key.username());
        Ok(())
    }

    pub fn clear_all() -> Result<(), AppError> {
        store().write()?.clear();
        Ok(())
    }
}

pub fn get_secret(key: SecretKey) -> Result<Option<String>, AppError> {
    imp::get_secret(key)
}

pub fn set_secret(key: SecretKey, value: &str) -> Result<(), AppError> {
    imp::set_secret(key, value)
}

pub fn delete_secret(key: SecretKey) -> Result<(), AppError> {
    imp::delete_secret(key)
}

#[cfg(test)]
pub fn clear_all_test_secrets() -> Result<(), AppError> {
    imp::clear_all()
}
