use std::collections::HashMap;

use secret_service::{
    blocking::{Collection, Item, SecretService},
    EncryptionType, Error,
};

use crate::error::AppError;

const LOGIN_COLLECTION: &str = "/org/freedesktop/secrets/collection/login";
const CONTENT_TYPE: &str = "text/plain; charset=utf-8";

fn login_collection<'a>(
    secrets: &'a SecretService<'a>,
    allow_unlock: bool,
) -> Result<Collection<'a>, Error> {
    // Match go-keyring: prefer the collection at the login path, then the default alias.
    let collection = secrets
        .get_all_collections()?
        .into_iter()
        .find(|collection| collection.collection_path.as_str() == LOGIN_COLLECTION)
        .map_or_else(|| secrets.get_default_collection(), Ok)?;
    if collection.is_locked()? {
        if !allow_unlock {
            return Err(Error::Locked);
        }
        collection.unlock()?;
    }
    Ok(collection)
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
        let collection = login_collection(&secrets, allow_unlock)?;
        let Some(item) = collection
            .search_items(attributes(service, account))?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        ensure_unlocked(&item, allow_unlock)?;
        item.get_secret().map(Some)
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
        let collection = login_collection(&secrets, true)?;
        let attributes = attributes(service, account);
        if let Some(item) = collection.search_items(attributes.clone())?.first() {
            ensure_unlocked(item, true)?;
            return item.set_secret(value.as_bytes(), CONTENT_TYPE);
        }
        let label = format!("Password for '{account}' on '{service}'");
        collection.create_item(&label, attributes, value.as_bytes(), true, CONTENT_TYPE)?;
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
        let collection = login_collection(&secrets, true).unwrap();
        for item in collection
            .search_items(attributes(&service, account))
            .unwrap()
        {
            item.delete().unwrap();
        }
        let (missing, created, updated) = result.unwrap();
        assert!(missing.is_none());
        assert_eq!(created.as_deref(), Some(first));
        assert_eq!(updated.as_deref(), Some(second));
    }
}
