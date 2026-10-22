//! Local account catalog containing Qoder's opaque, already encrypted credentials.
//! Native CLI owns encryption and refresh; snapshots never leave the backend.
use crate::{config::atomic_write_private, qoder_config, AppType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountProfile {
    pub username: Option<String>,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QoderAccount {
    pub id: String,
    #[serde(flatten)]
    pub(crate) profile: AccountProfile,
    pub is_current: bool,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Credential {
    machine_id: String,
    encrypted_user: String,
}

#[derive(Clone, Deserialize, Serialize)]
struct SavedAccount {
    id: String,
    profile: AccountProfile,
    credential: Credential,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct AccountStore {
    scopes: BTreeMap<String, Vec<SavedAccount>>,
}

pub(crate) fn absolute_root(app: &AppType) -> Result<PathBuf, String> {
    let root = qoder_config::data_dir(app);
    if root.is_absolute() {
        Ok(root)
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(root))
    }
}

fn environment(app: &AppType) -> String {
    let variable = if *app == AppType::QoderCn {
        "QODERCN_ENV"
    } else {
        "QODER_ENV"
    };
    std::env::var(variable)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("QODER_ENV").ok())
        .unwrap_or_else(|| "prod".into())
}

fn credential_path(root: &Path, app: &AppType) -> PathBuf {
    let environment = environment(app);
    root.join(".auth").join(match environment.as_str() {
        "daily" => "user.daily",
        "test" => "user.test",
        _ => "user",
    })
}

pub(crate) fn scope(app: &AppType, root: &Path) -> String {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let identity = format!("{}\0{}\0{}", app.as_str(), root.display(), environment(app));
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

fn store_path() -> PathBuf {
    crate::config::get_app_config_dir().join("qoder_accounts.json")
}

fn optional_text(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => {
            Err("Cannot read Qoder credentials; check the config directory permissions".into())
        }
    }
}

/// Cooperate with proper-lockfile's native transaction directory. Never remove
/// or reclaim another process's lock, and keep the critical section synchronous.
struct NativeLock(PathBuf);
impl NativeLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let auth = root.join(".auth");
        fs::create_dir_all(&auth).map_err(|e| e.to_string())?;
        let path = auth.join(".credential-transaction.lock");
        fs::create_dir(&path).map_err(|_| {
            "Another Qoder process is updating this login. Refresh and retry".to_string()
        })?;
        Ok(Self(path))
    }
}
impl Drop for NativeLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn read_unlocked(root: &Path, app: &AppType) -> Result<Option<Credential>, String> {
    let Some(encrypted_user) = optional_text(&credential_path(root, app))? else {
        return Ok(None);
    };
    let machine_id = optional_text(&root.join(".auth/machine_id"))?
        .filter(|v| !v.trim().is_empty())
        .ok_or("Qoder's machine identity is missing. Sign in again")?;
    Ok(Some(Credential {
        machine_id,
        encrypted_user,
    }))
}

pub(crate) fn capture(root: &Path, app: &AppType) -> Result<Option<Credential>, String> {
    if !root.join(".auth").exists() {
        return Ok(None);
    }
    let _guard = NativeLock::acquire(root)?;
    read_unlocked(root, app)
}

/// Only replace the exact credential observed by the caller. Machine identity
/// stays stable so Qoder's other encrypted files and running sessions still work.
pub(crate) fn replace(
    root: &Path,
    app: &AppType,
    expected: Option<&Credential>,
    next: Option<&Credential>,
) -> Result<(), String> {
    let _guard = NativeLock::acquire(root)?;
    if read_unlocked(root, app)?.as_ref() != expected {
        return Err("Qoder login changed outside CC Switch. Refresh and retry".into());
    }
    let path = credential_path(root, app);
    if let Some(next) = next {
        let machine_path = root.join(".auth/machine_id");
        let previous_machine = optional_text(&machine_path)?;
        if previous_machine
            .as_ref()
            .is_some_and(|v| v.trim() != next.machine_id.trim())
        {
            return Err(
                "This account belongs to a different Qoder machine identity. Add the account again"
                    .into(),
            );
        }
        if previous_machine.is_none() {
            atomic_write_private(&machine_path, next.machine_id.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        if let Err(error) = atomic_write_private(&path, next.encrypted_user.as_bytes()) {
            if previous_machine.is_none() {
                let _ = fs::remove_file(machine_path);
            }
            return Err(error.to_string());
        }
    } else if expected.is_some() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn prepare_login(root: &Path) -> Result<tempfile::TempDir, String> {
    let staging = tempfile::Builder::new()
        .prefix("cc-switch-qoder-account-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    if let Some(settings) = optional_text(&root.join("settings.json"))? {
        atomic_write_private(&staging.path().join("settings.json"), settings.as_bytes())
            .map_err(|e| e.to_string())?;
    } else {
        atomic_write_private(
            &staging.path().join("settings.json"),
            br#"{"privacy":{"usageStatisticsEnabled":false}}"#,
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(machine_id) = optional_text(&root.join(".auth/machine_id"))? {
        atomic_write_private(
            &staging.path().join(".auth/machine_id"),
            machine_id.as_bytes(),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(staging)
}

impl AccountStore {
    pub(crate) fn load() -> Result<Self, String> {
        Self::load_from(&store_path())
    }
    fn load_from(path: &Path) -> Result<Self, String> {
        let Some(source) = optional_text(path)? else {
            return Ok(Self::default());
        };
        serde_json::from_str(&source)
            .map_err(|_| "Qoder account catalog is invalid; the existing file was preserved".into())
    }
    pub(crate) fn save(&self) -> Result<(), String> {
        self.save_to(&store_path())
    }
    fn save_to(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        atomic_write_private(path, &bytes).map_err(|e| e.to_string())
    }
    pub(crate) fn import(
        &mut self,
        scope: &str,
        credential: Credential,
        profile: AccountProfile,
        before: Option<&Credential>,
    ) -> String {
        let accounts = self.scopes.entry(scope.into()).or_default();
        // Exact matches also handle importing a login already managed here.
        // A refresh during status lookup can be linked to the known credential
        // observed before the command, even when only a display name is known.
        let existing = accounts
            .iter_mut()
            .find(|account| account.credential == credential);
        if let Some(account) = existing {
            account.profile = profile;
            return account.id.clone();
        }
        if let Some(account) = accounts.iter_mut().find(|account| {
            before == Some(&account.credential)
                && account.profile.username == profile.username
                && account.profile.email == profile.email
                && account.profile.organization_id == profile.organization_id
        }) {
            account.credential = credential;
            account.profile = profile;
            return account.id.clone();
        }
        // The CLI may already have rotated credentials before status lookup.
        // Require a unique identity within this scope: email, or username plus
        // organization. A display name or organization alone cannot identify a user.
        let nonempty = |value: &Option<String>| {
            value
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
        };
        if nonempty(&profile.email)
            || (nonempty(&profile.username) && nonempty(&profile.organization_id))
        {
            let mut matches = accounts.iter_mut().filter(|account| {
                account.profile.username == profile.username
                    && account.profile.email == profile.email
                    && account.profile.organization_id == profile.organization_id
            });
            if let Some(account) = matches.next() {
                if matches.next().is_none() {
                    account.credential = credential;
                    account.profile = profile;
                    return account.id.clone();
                }
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        accounts.push(SavedAccount {
            id: id.clone(),
            profile,
            credential,
        });
        id
    }
    pub(crate) fn accounts(&self, scope: &str, current: Option<&Credential>) -> Vec<QoderAccount> {
        self.scopes
            .get(scope)
            .into_iter()
            .flatten()
            .map(|account| QoderAccount {
                id: account.id.clone(),
                profile: account.profile.clone(),
                is_current: current == Some(&account.credential),
            })
            .collect()
    }
    pub(crate) fn credential(&self, scope: &str, id: &str) -> Result<Credential, String> {
        self.scopes
            .get(scope)
            .into_iter()
            .flatten()
            .find(|a| a.id == id)
            .map(|a| a.credential.clone())
            .ok_or_else(|| "Qoder account no longer exists. Refresh the account list".into())
    }
    pub(crate) fn remove(&mut self, scope: &str, id: &str) -> Result<(), String> {
        self.credential(scope, id)?;
        self.scopes
            .get_mut(scope)
            .unwrap()
            .retain(|account| account.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn credential(value: &str) -> Credential {
        Credential {
            machine_id: "fixture-machine".into(),
            encrypted_user: value.into(),
        }
    }

    #[test]
    fn qoder_accounts_keep_same_name_accounts_and_editions_separate() {
        let mut store = AccountStore::default();
        let profile = AccountProfile {
            username: Some("same name".into()),
            ..Default::default()
        };
        let a = store.import("global", credential("a"), profile.clone(), None);
        let b = store.import("global", credential("b"), profile.clone(), None);
        store.import("cn", credential("c"), profile.clone(), None);
        assert_ne!(a, b);
        let id = store.import(
            "global",
            credential("a-refreshed"),
            profile,
            Some(&credential("a")),
        );
        assert_eq!(id, a);
        assert_eq!(store.accounts("global", None).len(), 2);
        assert!(store.remove("cn", &a).is_err());
        store.remove("global", &a).unwrap();
        assert_eq!(store.accounts("cn", None).len(), 1);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("accounts.json");
        store.save_to(&path).unwrap();
        let loaded = AccountStore::load_from(&path).unwrap();
        assert!(loaded.credential("global", &b).unwrap() == credential("b"));
        let public = serde_json::to_string(&loaded.accounts("global", None)).unwrap();
        assert!(!public.contains("encryptedUser") && !public.contains("machineId"));
    }

    #[test]
    fn qoder_accounts_reuse_unique_identities_after_external_token_rotation() {
        for profile in [
            AccountProfile {
                username: Some("fixture".into()),
                email: Some("fixture@example.test".into()),
                ..Default::default()
            },
            AccountProfile {
                username: Some("fixture".into()),
                organization_id: Some("org".into()),
                ..Default::default()
            },
            AccountProfile {
                email: Some("fixture@example.test".into()),
                ..Default::default()
            },
        ] {
            let mut store = AccountStore::default();
            let id = store.import("global", credential("old"), profile.clone(), None);
            let cn = store.import("cn", credential("cn"), profile.clone(), None);
            let updated = AccountProfile {
                plan: Some("Pro".into()),
                ..profile
            };
            let current = credential("refreshed-before-status");
            assert_eq!(
                store.import("global", current.clone(), updated.clone(), Some(&current)),
                id
            );
            assert!(store.credential("global", &id).unwrap() == current);
            let accounts = store.accounts("global", Some(&current));
            assert_eq!(accounts.len(), 1);
            assert!(accounts[0].is_current);
            assert_eq!(accounts[0].profile.plan.as_deref(), Some("Pro"));
            assert_eq!(
                store.import("global", credential("refreshed-again"), updated, None),
                id
            );
            assert!(store.credential("global", &id).unwrap() == credential("refreshed-again"));
            assert_eq!(store.accounts("global", None).len(), 1);
            assert!(store.credential("cn", &cn).unwrap() == credential("cn"));
        }
    }

    #[test]
    fn qoder_accounts_do_not_merge_missing_or_conflicting_identities() {
        for profile in [
            AccountProfile::default(),
            AccountProfile {
                username: Some("same name".into()),
                ..Default::default()
            },
            AccountProfile {
                username: Some("same name".into()),
                email: Some(" ".into()),
                organization_id: Some("".into()),
                ..Default::default()
            },
            AccountProfile {
                organization_id: Some("shared-org".into()),
                ..Default::default()
            },
        ] {
            let mut store = AccountStore::default();
            let first = store.import("global", credential("a"), profile.clone(), None);
            let next = credential("b");
            assert_ne!(
                store.import("global", next.clone(), profile, Some(&next)),
                first
            );
            assert!(store.credential("global", &first).unwrap() == credential("a"));
        }
        let profile = AccountProfile {
            username: Some("fixture".into()),
            email: Some("fixture@example.test".into()),
            organization_id: Some("org".into()),
            ..Default::default()
        };
        for other in [
            AccountProfile {
                username: Some("other".into()),
                ..profile.clone()
            },
            AccountProfile {
                email: Some("other@example.test".into()),
                ..profile.clone()
            },
            AccountProfile {
                organization_id: Some("other-org".into()),
                ..profile.clone()
            },
        ] {
            let mut store = AccountStore::default();
            let first = store.import("global", credential("a"), profile.clone(), None);
            assert_ne!(store.import("global", credential("b"), other, None), first);
        }
    }

    #[test]
    fn qoder_accounts_do_not_choose_between_ambiguous_saved_identities() {
        let mut store = AccountStore::default();
        let profile = AccountProfile {
            username: Some("fixture".into()),
            email: Some("fixture@example.test".into()),
            ..Default::default()
        };
        let first = store.import("global", credential("a"), profile.clone(), None);
        // Older catalogs may already contain duplicates from native refreshes.
        let accounts = store.scopes.get_mut("global").unwrap();
        let mut duplicate = accounts[0].clone();
        duplicate.id = "duplicate".into();
        duplicate.credential = credential("b");
        accounts.push(duplicate);
        let next = store.import("global", credential("c"), profile, None);
        assert_ne!(next, first);
        assert_ne!(next, "duplicate");
        assert!(store.credential("global", &first).unwrap() == credential("a"));
        assert!(store.credential("global", "duplicate").unwrap() == credential("b"));
    }

    #[test]
    fn qoder_account_switch_and_delete_preserve_other_native_data() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("settings.json"), "keep settings").unwrap();
        replace(root, &AppType::QoderCn, None, Some(&credential("a"))).unwrap();
        assert!(replace(root, &AppType::QoderCn, None, Some(&credential("b"))).is_err());
        replace(
            root,
            &AppType::QoderCn,
            Some(&credential("a")),
            Some(&credential("b")),
        )
        .unwrap();
        let staged = prepare_login(root).unwrap();
        assert!(!staged.path().join(".auth/user").exists());
        assert_eq!(
            fs::read_to_string(staged.path().join(".auth/machine_id")).unwrap(),
            "fixture-machine"
        );
        replace(root, &AppType::QoderCn, Some(&credential("b")), None).unwrap();
        assert!(capture(root, &AppType::QoderCn).unwrap().is_none());
        assert_eq!(
            fs::read_to_string(root.join("settings.json")).unwrap(),
            "keep settings"
        );
        assert!(root.join(".auth/machine_id").exists());
    }

    #[test]
    fn qoder_accounts_refuse_foreign_machine_or_concurrent_native_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        replace(root, &AppType::Qoder, None, Some(&credential("a"))).unwrap();
        let foreign = Credential {
            machine_id: "foreign-machine".into(),
            encrypted_user: "foreign".into(),
        };
        assert!(replace(
            root,
            &AppType::Qoder,
            Some(&credential("a")),
            Some(&foreign)
        )
        .is_err());
        assert!(capture(root, &AppType::Qoder).unwrap().as_ref() == Some(&credential("a")));
        replace(
            root,
            &AppType::Qoder,
            Some(&credential("a")),
            Some(&credential("b")),
        )
        .unwrap();
        assert!(replace(root, &AppType::Qoder, Some(&credential("a")), None).is_err());
        assert!(capture(root, &AppType::Qoder).unwrap().as_ref() == Some(&credential("b")));
        let native_lock = root.join(".auth/.credential-transaction.lock");
        fs::create_dir(&native_lock).unwrap();
        assert!(replace(root, &AppType::Qoder, Some(&credential("b")), None).is_err());
        assert!(native_lock.exists());
    }

    #[test]
    fn qoder_accounts_preserve_invalid_catalogs() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("accounts.json");
        for original in ["broken JSON", "{}", "{\"scopes\":[]}"] {
            fs::write(&path, original).unwrap();
            assert!(AccountStore::load_from(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }
}
