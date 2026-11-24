use std::collections::HashMap;

use secret_service::{
    blocking::{Collection, Item, SecretService},
    EncryptionType, Error,
};

use crate::error::AppError;

const LOGIN_COLLECTION: &str = "/org/freedesktop/secrets/collection/login";
const CONTENT_TYPE: &str = "text/plain; charset=utf-8";

/// Antigravity CLI（go-keyring）优先用 `login` 集合，Desktop 用 Secret Service 默认集合；
/// 两边指向同一路径时去重为 1 个，不同时按 `[login, default]` 顺序一并处理。
fn candidate_collections<'a>(secrets: &'a SecretService<'a>) -> Result<Vec<Collection<'a>>, Error> {
    let mut collections = Vec::with_capacity(2);
    if let Some(login) = secrets
        .get_all_collections()?
        .into_iter()
        .find(|collection| collection.collection_path.as_str() == LOGIN_COLLECTION)
    {
        collections.push(login);
    }
    match secrets.get_default_collection() {
        Ok(default) => {
            let duplicate = collections.iter().any(|existing| {
                existing.collection_path.as_str() == default.collection_path.as_str()
            });
            if !duplicate {
                collections.push(default);
            }
        }
        Err(_) if !collections.is_empty() => {}
        Err(error) => return Err(error),
    }
    Ok(collections)
}

fn unlock_collection(collection: &Collection<'_>, allow_unlock: bool) -> Result<(), Error> {
    if collection.is_locked()? {
        if !allow_unlock {
            return Err(Error::Locked);
        }
        collection.unlock()?;
    }
    Ok(())
}

fn attributes<'a>(service: &'a str, account: &'a str) -> HashMap<&'a str, &'a str> {
    HashMap::from([("service", service), ("username", account)])
}

fn ensure_unlocked(item: &Item<'_>, allow_unlock: bool) -> Result<(), Error> {
    if item.is_locked()? {
        if !allow_unlock {
            return Err(Error::Locked);
        }
        item.unlock()?;
    }
    Ok(())
}

pub(super) fn read(
    service: &str,
    account: &str,
    allow_unlock: bool,
) -> Result<Option<String>, AppError> {
    let read_secret = || {
        let secrets = SecretService::connect(EncryptionType::Dh)?;
        let collections = candidate_collections(&secrets)?;
        let attrs = attributes(service, account);
        let mut locked = false;
        for collection in collections {
            match unlock_collection(&collection, allow_unlock) {
                Ok(()) => {}
                Err(Error::Locked) => {
                    locked = true;
                    continue;
                }
                Err(error) => return Err(error),
            }
            if let Some(item) = collection.search_items(attrs.clone())?.into_iter().next() {
                ensure_unlocked(&item, allow_unlock)?;
                return item.get_secret().map(Some);
            }
        }
        if locked {
            Err(Error::Locked)
        } else {
            Ok(None)
        }
    };
    read_secret()
        .map_err(|error: Error| secret_service_error("读取", &error))?
        .map(|bytes| {
            String::from_utf8(bytes)
                .map_err(|_| AppError::Config("Antigravity 登录数据不是 UTF-8".into()))
        })
        .transpose()
}

pub(super) fn write(service: &str, account: &str, value: &str) -> Result<(), AppError> {
    let write_secret = || {
        let secrets = SecretService::connect(EncryptionType::Dh)?;
        let collections = candidate_collections(&secrets)?;
        let attrs = attributes(service, account);
        let mut updated = false;
        for collection in &collections {
            unlock_collection(collection, true)?;
            if let Some(item) = collection.search_items(attrs.clone())?.first() {
                ensure_unlocked(item, true)?;
                item.set_secret(value.as_bytes(), CONTENT_TYPE)?;
                updated = true;
            }
        }
        if !updated {
            let label = format!("Password for '{account}' on '{service}'");
            for collection in &collections {
                collection.create_item(
                    &label,
                    attrs.clone(),
                    value.as_bytes(),
                    true,
                    CONTENT_TYPE,
                )?;
            }
        }
        Ok(())
    };
    write_secret().map_err(|error| secret_service_error("写入", &error))
}

fn secret_service_error(operation: &str, error: &Error) -> AppError {
    AppError::Config(format!(
        "{operation} Antigravity 登录失败（Linux Secret Service）：{error}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires an unlocked desktop Secret Service"]
    fn linux_secret_service_round_trip() {
        let service = format!("cc-switch-agy-selftest-{}", uuid::Uuid::new_v4());
        let account = "probe";
        let first = r#"{"test":"one"}"#;
        let second = r#"{"test":"two"}"#;
        let result = (|| {
            let missing = read(&service, account, false)?;
            write(&service, account, first)?;
            let created = read(&service, account, false)?;
            write(&service, account, second)?;
            let updated = read(&service, account, true)?;
            Ok::<_, AppError>((missing, created, updated))
        })();

        let secrets = SecretService::connect(EncryptionType::Dh).unwrap();
        for collection in candidate_collections(&secrets).unwrap() {
            unlock_collection(&collection, true).unwrap();
            for item in collection
                .search_items(attributes(&service, account))
                .unwrap()
            {
                item.delete().unwrap();
            }
        }
        let (missing, created, updated) = result.unwrap();
        assert!(missing.is_none());
        assert_eq!(created.as_deref(), Some(first));
        assert_eq!(updated.as_deref(), Some(second));
    }
}
