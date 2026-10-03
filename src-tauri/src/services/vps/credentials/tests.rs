use super::*;
use crate::services::vps::{managed::tests::with_home, VpsAuthMethod};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

#[derive(Default)]
pub(in crate::services::vps) struct MemoryCredentialStore {
    values: Mutex<BTreeMap<(String, String), String>>,
    pub(in crate::services::vps) fail_get: AtomicBool,
    fail_set: AtomicBool,
    fail_after_set: AtomicBool,
    fail_delete: AtomicBool,
    pub(in crate::services::vps) get_calls: AtomicUsize,
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, namespace: &str, id: &str) -> Result<Option<SecretString>> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_get.load(Ordering::SeqCst) {
            bail!("injected credential read failure");
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&(namespace.into(), id.into()))
            .cloned()
            .map(SecretString::new))
    }
    fn set(&self, namespace: &str, id: &str, password: &SecretString) -> Result<()> {
        if self.fail_set.swap(false, Ordering::SeqCst) {
            bail!("injected credential save failure");
        }
        self.values
            .lock()
            .unwrap()
            .insert((namespace.into(), id.into()), password.expose().into());
        if self.fail_after_set.swap(false, Ordering::SeqCst) {
            bail!("injected failure after credential write");
        }
        Ok(())
    }
    fn delete(&self, namespace: &str, id: &str) -> Result<()> {
        if self.fail_delete.load(Ordering::SeqCst) {
            bail!("injected credential delete failure");
        }
        self.values
            .lock()
            .unwrap()
            .remove(&(namespace.into(), id.into()));
        Ok(())
    }
}

fn password_host() -> VpsServer {
    let mut server = VpsServer::new("Password test".into(), "192.0.2.10".into(), "test".into());
    server.auth_method = Some(VpsAuthMethod::Password);
    server
}

fn saved(store: &MemoryCredentialStore, service: &VpsService, id: &str) -> Option<String> {
    let server = service
        .load()
        .unwrap()
        .into_iter()
        .find(|server| server.id == id)?;
    read_password(service.root(), &server, store, &AtomicBool::new(false))
        .unwrap()
        .map(|value| value.expose().into())
}

#[test]
#[serial_test::serial]
fn passwords_save_preserve_replace_and_never_enter_generated_files() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        host.apps.claude = true;
        let password = "  test secret with spaces  ";
        assert!(service.save_server(host.clone()).is_err());
        service
            .save_server_with_password(host.clone(), Some(password.into()))
            .unwrap();
        for supplied in [None, Some(String::new())] {
            host.name.push('x');
            service
                .save_server_with_password(host.clone(), supplied)
                .unwrap();
            assert_eq!(saved(&store, &service, &host.id).as_deref(), Some(password));
        }
        for relative in ["servers.json", "ssh_config", "clients/claude.json"] {
            let content = fs::read_to_string(service.root.join(relative)).unwrap();
            assert!(!content.contains(password));
            assert!(!content.contains("[REDACTED]"));
        }
        assert!(!format!("{:?}", SecretString::new(password.into())).contains(password));
        service
            .save_server_with_password(host.clone(), Some(" ".into()))
            .unwrap();
        assert_eq!(saved(&store, &service, &host.id).as_deref(), Some(" "));
        for invalid in ["line\nbreak", "null\0byte"] {
            assert!(service
                .save_server_with_password(host.clone(), Some(invalid.into()))
                .is_err());
            assert_eq!(saved(&store, &service, &host.id).as_deref(), Some(" "));
        }
    });
}

#[test]
#[serial_test::serial]
fn credential_namespace_is_canonical_root_and_stable_uuid_scoped() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let first = service.clone().with_credential_store(store.clone());
        let other = tempfile::tempdir().unwrap();
        let second = VpsService {
            root: other.path().join("vps"),
            credentials: store.clone(),
            fail_write: None,
        };
        let host = password_host();
        first
            .save_server_with_password(host.clone(), Some("first only".into()))
            .unwrap();
        assert_eq!(saved(&store, &second, &host.id), None);
        second
            .save_server_with_password(host.clone(), Some("second only".into()))
            .unwrap();
        first.delete_server(&host.id).unwrap();
        assert_eq!(
            saved(&store, &second, &host.id).as_deref(),
            Some("second only")
        );
        assert_eq!(
            namespace(&second.root).unwrap(),
            namespace(&second.root.join(".")).unwrap()
        );
    });
}

#[test]
#[serial_test::serial]
fn credential_write_and_precommit_failures_preserve_previous_password() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        service
            .save_server_with_password(host.clone(), Some("old".into()))
            .unwrap();
        host.name = "changed".into();
        store.fail_set.store(true, Ordering::SeqCst);
        assert!(service
            .save_server_with_password(host.clone(), Some("new".into()))
            .is_err());
        assert_eq!(saved(&store, &service, &host.id).as_deref(), Some("old"));
        let failing = VpsService {
            fail_write: Some(0),
            ..service.clone()
        };
        assert!(failing
            .save_server_with_password(host.clone(), Some("new".into()))
            .is_err());
        assert_eq!(saved(&store, &service, &host.id).as_deref(), Some("old"));
        assert_ne!(service.load().unwrap()[0].name, host.name);
        let new_host = password_host();
        assert!(failing
            .save_server_with_password(new_host.clone(), Some("new".into()))
            .is_err());
        assert_eq!(saved(&store, &service, &new_host.id), None);
    });
}

#[test]
#[serial_test::serial]
fn committed_generation_failure_keeps_new_password_for_retry() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        service
            .save_server_with_password(host.clone(), Some("old".into()))
            .unwrap();
        host.host = "192.0.2.20".into();
        let failing = VpsService {
            fail_write: Some(1),
            ..service.clone()
        };
        assert!(failing
            .save_server_with_password(host.clone(), Some("new".into()))
            .is_err());
        assert_eq!(service.load().unwrap(), vec![host.clone()]);
        assert_eq!(saved(&store, &service, &host.id).as_deref(), Some("new"));
        service.save_server(host.clone()).unwrap();
        assert_eq!(saved(&store, &service, &host.id).as_deref(), Some("new"));
    });
}

#[test]
#[serial_test::serial]
fn skill_failure_after_commit_keeps_password_and_reports_failure() {
    with_home(|db, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        host.apps.claude = true;
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_vps_auth BEFORE UPDATE ON skills BEGIN SELECT RAISE(FAIL, 'injected deployment failure'); END;").unwrap();
        assert!(service
            .save_server_with_skills_and_password(db, host.clone(), Some("test secret".into()))
            .is_err());
        assert_eq!(service.load().unwrap(), vec![host.clone()]);
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("test secret")
        );
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_vps_auth;")
            .unwrap();
        service.save_server_with_skills(db, host.clone()).unwrap();
        let skill = crate::services::skill::SkillService::get_app_skills_dir(
            &crate::app_config::AppType::Claude,
        )
        .unwrap()
        .join("cc-switch-vps/SKILL.md");
        assert!(!fs::read_to_string(skill).unwrap().contains("test secret"));
    });
}

#[test]
#[serial_test::serial]
fn switching_and_deletion_retry_only_owned_pending_credentials() {
    with_home(|_, service| {
        for delete in [false, true] {
            let store = Arc::new(MemoryCredentialStore::default());
            let service = service.clone().with_credential_store(store.clone());
            let mut host = password_host();
            service
                .save_server_with_password(host.clone(), Some("keep until committed".into()))
                .unwrap();
            store.fail_delete.store(true, Ordering::SeqCst);
            let result = if delete {
                service.delete_server(&host.id)
            } else {
                host.auth_method = None;
                service.save_server(host.clone())
            };
            assert!(result.is_err());
            let document: serde_json::Value =
                serde_json::from_slice(&fs::read(service.root.join("servers.json")).unwrap())
                    .unwrap();
            assert!(document["pendingCredentialDeletions"]
                .get(&host.id)
                .is_some());
            assert!(!store.values.lock().unwrap().is_empty());
            assert!(
                saved(&store, &service, &host.id).is_none(),
                "retired credentials are not active"
            );
            store.fail_delete.store(false, Ordering::SeqCst);
            if delete {
                service.delete_server(&host.id).unwrap();
            } else {
                service.save_server(host.clone()).unwrap();
            }
            assert_eq!(saved(&store, &service, &host.id), None);
            assert!(service
                .read_document()
                .unwrap()
                .0
                .pending_credential_deletions
                .is_empty());
        }
    });
}

#[test]
#[serial_test::serial]
fn unavailable_vault_does_not_fallback_or_change_host_data() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        store.fail_set.store(true, Ordering::SeqCst);
        store.fail_get.store(true, Ordering::SeqCst);
        assert!(service
            .save_server_with_password(password_host(), Some("never written".into()))
            .is_err());
        assert!(service.load().unwrap().is_empty());
        assert!(store.values.lock().unwrap().is_empty());
        // Legacy key/agent hosts do not require an available credential backend.
        let mut host = password_host();
        host.auth_method = None;
        service.save_server(host).unwrap();
        assert_eq!(store.get_calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
#[serial_test::serial]
fn immutable_revisions_preserve_old_password_when_write_and_cleanup_both_fail() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        service
            .save_server_with_password(host.clone(), Some("old valid".into()))
            .unwrap();
        let original_revision =
            service.read_document().unwrap().0.password_revisions[&host.id].clone();
        host.name = "not committed".into();
        store.fail_after_set.store(true, Ordering::SeqCst);
        store.fail_delete.store(true, Ordering::SeqCst);
        let error = service
            .save_server_with_password(host.clone(), Some("staged only".into()))
            .unwrap_err();
        assert!(format!("{error:#}").contains("cleanup"));
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("old valid")
        );
        let document = service.read_document().unwrap().0;
        assert_eq!(document.password_revisions[&host.id], original_revision);
        assert!(!document.pending_credential_deletions[&host.id].contains(&original_revision));
        assert_eq!(store.values.lock().unwrap().len(), 2);
        store.fail_delete.store(false, Ordering::SeqCst);
        service.reconcile().unwrap();
        assert_eq!(store.values.lock().unwrap().len(), 1);
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("old valid")
        );
    });
}

#[test]
#[serial_test::serial]
fn failed_host_commit_and_cleanup_leave_old_revision_available() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let mut host = password_host();
        service
            .save_server_with_password(host.clone(), Some("old valid".into()))
            .unwrap();
        let config = service.root.join("ssh_config");
        let original_config = fs::read(&config).unwrap();
        fs::write(&config, "unowned user edit").unwrap();
        host.name = "not committed".into();
        store.fail_delete.store(true, Ordering::SeqCst);
        assert!(service
            .save_server_with_password(host.clone(), Some("new uncommitted".into()))
            .is_err());
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("old valid")
        );
        assert_ne!(service.load().unwrap()[0].name, host.name);
        fs::write(config, original_config).unwrap();
        store.fail_delete.store(false, Ordering::SeqCst);
        service.reconcile().unwrap();
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("old valid")
        );
    });
}

#[test]
#[serial_test::serial]
fn rebinding_same_server_never_cleans_its_new_active_revision() {
    with_home(|_, service| {
        let store = Arc::new(MemoryCredentialStore::default());
        let service = service.clone().with_credential_store(store.clone());
        let host = password_host();
        service
            .save_server_with_password(host.clone(), Some("retired".into()))
            .unwrap();
        store.fail_delete.store(true, Ordering::SeqCst);
        assert!(service.delete_server(&host.id).is_err());
        assert!(service
            .save_server_with_password(host.clone(), Some("active replacement".into()))
            .is_err());
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("active replacement")
        );
        let mut document = service.read_document().unwrap().0;
        let active = document.password_revisions[&host.id].clone();
        assert!(!document.pending_credential_deletions[&host.id].contains(&active));
        document
            .pending_credential_deletions
            .entry(host.id.clone())
            .or_default()
            .insert(active);
        assert!(
            document.validate().is_err(),
            "even malformed receipts cannot delete active credentials"
        );
        store.fail_delete.store(false, Ordering::SeqCst);
        service.reconcile().unwrap();
        assert_eq!(store.values.lock().unwrap().len(), 1);
        assert_eq!(
            saved(&store, &service, &host.id).as_deref(),
            Some("active replacement")
        );
    });
}

#[test]
#[serial_test::serial]
fn old_documents_and_certificate_paths_preserve_ssh_first_match_semantics() {
    with_home(|_, service| {
        let legacy = VpsServer::new("old".into(), "192.0.2.1".into(), "test".into());
        let json = serde_json::to_value(&legacy).unwrap();
        assert!(json.get("authMethod").is_none());
        assert!(json.get("certificateFile").is_none());
        let decoded: VpsServer = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.auth_method, None);
        let mut private_key = legacy.clone();
        private_key.auth_method = Some(VpsAuthMethod::PrivateKey);
        assert!(private_key.validate().is_err());
        private_key.identity_file = Some(service.root.join("private-key"));
        private_key.validate().unwrap();
        let mut cert = legacy.clone();
        cert.id = uuid::Uuid::new_v4().to_string();
        cert.auth_method = Some(VpsAuthMethod::Certificate);
        assert!(cert.validate().is_err());
        cert.identity_file = Some(service.root.join("key with spaces 日本語"));
        assert!(cert.validate().is_err());
        cert.certificate_file = Some(service.root.join("user-cert.pub"));
        cert.validate().unwrap();
        let password = password_host();
        let config =
            super::super::render_ssh_config(&[legacy, cert.clone(), password.clone()]).unwrap();
        assert!(config.contains("CertificateFile \""));
        let password_block = config
            .split(&format!("Host {}\n", password.ssh_alias()))
            .nth(1)
            .unwrap()
            .split("\nHost ")
            .next()
            .unwrap();
        assert!(password_block.contains("BatchMode no"));
        assert!(password_block.contains("PreferredAuthentications password"));
        assert!(
            config
                .find(&format!("Host {}", password.ssh_alias()))
                .unwrap()
                < config.find("Host *").unwrap()
        );
        for path in [
            service.root.join("bad\nCertificateFile injected"),
            service.root.join("bad%h"),
            "relative-cert.pub".into(),
        ] {
            cert.certificate_file = Some(path);
            assert!(cert.validate().is_err());
        }
    });
}
