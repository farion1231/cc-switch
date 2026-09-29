use super::*;
use crate::app_config::{AppType, SkillApps};
use crate::services::skill::{SkillService, SkillStorageLocation, SyncMethod};
use std::fs;

fn with_home(test: impl FnOnce(&Arc<Database>, &VpsService)) {
    struct Guard {
        env: Vec<(&'static str, Option<std::ffi::OsString>)>,
        settings: crate::settings::AppSettings,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = crate::settings::update_settings(self.settings.clone());
            for (key, value) in &self.env {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let home = tempfile::tempdir().unwrap();
    let previous_env = [
        "CC_SWITCH_TEST_HOME",
        "HERMES_HOME",
        "MINIMAX_DATA_DIR",
        "MAVIS_DATA_DIR",
    ]
    .into_iter()
    .map(|key| (key, std::env::var_os(key)))
    .collect();
    std::env::set_var("CC_SWITCH_TEST_HOME", home.path());
    std::env::set_var("HERMES_HOME", home.path().join(".hermes"));
    std::env::set_var("MINIMAX_DATA_DIR", home.path().join(".minimax"));
    std::env::set_var("MAVIS_DATA_DIR", home.path().join(".minimax"));
    let guard = Guard {
        env: previous_env,
        settings: crate::settings::get_settings(),
    };
    assert_eq!(
        crate::config::get_app_config_dir(),
        home.path().join(".cc-switch")
    );
    crate::settings::update_settings(crate::settings::AppSettings {
        skill_sync_method: SyncMethod::Copy,
        skill_storage_location: SkillStorageLocation::CcSwitch,
        ..Default::default()
    })
    .unwrap();
    let _pi = crate::pi_config::test_support::TestAgentDir::at(&home.path().join(".pi/agent"));
    for app in AppType::all().filter(|app| !SkillApps::only(app).is_empty()) {
        let path = SkillService::get_app_skills_dir(&app).unwrap();
        assert!(
            path.starts_with(home.path()),
            "test client escaped isolation: {app:?}: {}",
            path.display()
        );
    }
    let db = Arc::new(Database::memory().unwrap());
    let service = VpsService::new();
    assert!(service.root().starts_with(home.path()));
    test(&db, &service);
    drop(guard);
}

fn server(apps: &[AppType]) -> VpsServer {
    let mut server = VpsServer::new("Test host".into(), "192.0.2.1".into(), "test".into());
    for app in apps {
        server.apps.set_enabled_for(app, true);
    }
    server
}

fn deployment(app: &AppType) -> std::path::PathBuf {
    SkillService::get_app_skills_dir(app)
        .unwrap()
        .join("cc-switch-vps")
}

fn manual_skill(id: &str, directory: &str) -> crate::app_config::InstalledSkill {
    serde_json::from_value(serde_json::json!({
        "id": id, "name": "manual", "directory": directory,
        "apps": {"claude": true}, "installedAt": 0, "updatedAt": 0,
    }))
    .unwrap()
}

#[test]
#[serial_test::serial]
fn opening_empty_vps_data_does_not_create_a_skill_or_files() {
    with_home(|db, service| {
        assert!(service.get_servers_with_skills(db).unwrap().is_empty());
        assert!(db.get_all_installed_skills().unwrap().is_empty());
        assert!(!service.root().exists());
    });
}

#[test]
#[serial_test::serial]
fn managed_registration_rejects_user_id_and_directory_collisions() {
    for (id, directory) in [
        ("internal:vps", "manual"),
        ("local:manual", "CC-SWITCH-VPS"),
    ] {
        with_home(|db, service| {
            db.save_skill(&manual_skill(id, directory)).unwrap();
            assert!(service
                .save_server_with_skills(db, server(&[AppType::Claude]))
                .is_err());
            assert!(service.load().unwrap().is_empty());
            assert!(db
                .get_installed_skill(id)
                .unwrap()
                .unwrap()
                .is_user_managed());
            assert!(!deployment(&AppType::Claude).exists());
        });
    }
}

#[test]
#[serial_test::serial]
fn client_alias_conflicts_are_rejected_before_saving_bindings() {
    with_home(|db, service| {
        let mut settings = crate::settings::get_settings();
        let shared = service
            .root()
            .parent()
            .unwrap()
            .join("shared-client")
            .display()
            .to_string();
        settings.claude_config_dir = Some(shared.clone());
        settings.codex_config_dir = Some(shared);
        crate::settings::update_settings(settings).unwrap();
        assert!(service
            .save_server_with_skills(db, server(&[AppType::Claude, AppType::Codex]))
            .is_err());
        assert!(service.load().unwrap().is_empty());
        assert!(!deployment(&AppType::Claude).exists());
    });
}

#[test]
#[serial_test::serial]
fn app_directory_changes_move_only_the_owned_deployment() {
    with_home(|db, service| {
        let host = server(&[AppType::Claude]);
        service.save_server_with_skills(db, host).unwrap();
        let old = deployment(&AppType::Claude);
        let mut settings = crate::settings::get_settings();
        settings.claude_config_dir = Some(
            service
                .root()
                .parent()
                .unwrap()
                .join("new-client")
                .display()
                .to_string(),
        );
        crate::settings::update_settings(settings).unwrap();
        service.get_servers_with_skills(db).unwrap();
        assert!(!old.exists());
        assert!(deployment(&AppType::Claude).join("SKILL.md").exists());
    });
}

#[test]
#[serial_test::serial]
fn source_edits_block_regeneration_without_overwriting_user_changes() {
    with_home(|db, service| {
        let mut host = server(&[AppType::Claude]);
        service.save_server_with_skills(db, host.clone()).unwrap();
        let source = service.root().join("skill-projections/claude/SKILL.md");
        fs::write(&source, b"user changed generated source").unwrap();
        host.name = "new name".into();
        assert!(service.save_server_with_skills(db, host).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"user changed generated source");
        assert_eq!(service.load().unwrap()[0].name, "Test host");
    });
}

#[test]
#[serial_test::serial]
fn missing_generated_source_is_recreated_without_reinstalling_the_record() {
    with_home(|db, service| {
        service
            .save_server_with_skills(db, server(&[AppType::Pi]))
            .unwrap();
        fs::remove_dir_all(service.root().join("skill-projections/pi")).unwrap();
        service.get_servers_with_skills(db).unwrap();
        assert!(service
            .root()
            .join("skill-projections/pi/SKILL.md")
            .exists());
        assert!(deployment(&AppType::Pi).join("SKILL.md").exists());
        assert_eq!(db.get_all_installed_skills().unwrap().len(), 1);
        assert!(
            !db.get_installed_skill("internal:vps")
                .unwrap()
                .unwrap()
                .apps
                .pi
        );
    });
}

#[test]
#[serial_test::serial]
fn vps_conflicts_do_not_prevent_ordinary_skills_from_resyncing() {
    with_home(|db, service| {
        service
            .save_server_with_skills(db, server(&[AppType::Claude]))
            .unwrap();
        fs::write(deployment(&AppType::Claude).join("SKILL.md"), b"edited").unwrap();
        let source = SkillService::get_ssot_dir().unwrap().join("ordinary");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), b"ordinary skill").unwrap();
        db.save_skill(&manual_skill("local:ordinary", "ordinary"))
            .unwrap();
        assert!(SkillService::sync_to_app(db, &AppType::Claude).is_err());
        assert_eq!(
            fs::read(
                SkillService::get_app_skills_dir(&AppType::Claude)
                    .unwrap()
                    .join("ordinary/SKILL.md")
            )
            .unwrap(),
            b"ordinary skill"
        );
        assert_eq!(
            fs::read(deployment(&AppType::Claude).join("SKILL.md")).unwrap(),
            b"edited"
        );
    });
}

#[test]
#[serial_test::serial]
fn simultaneous_host_saves_keep_both_hosts_and_one_managed_record() {
    with_home(|db, service| {
        std::thread::scope(|scope| {
            for app in [AppType::Claude, AppType::Codex] {
                scope.spawn(move || service.save_server_with_skills(db, server(&[app])).unwrap());
            }
        });
        assert_eq!(service.load().unwrap().len(), 2);
        assert_eq!(db.get_all_installed_skills().unwrap().len(), 1);
        assert!(deployment(&AppType::Claude).join("SKILL.md").exists());
        assert!(deployment(&AppType::Codex).join("SKILL.md").exists());
    });
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn symlink_deployments_keep_per_client_sources_and_clean_up_only_the_link() {
    with_home(|db, service| {
        let mut settings = crate::settings::get_settings();
        settings.skill_sync_method = SyncMethod::Symlink;
        crate::settings::update_settings(settings).unwrap();
        let host = server(&[AppType::Claude, AppType::Pi]);
        service.save_server_with_skills(db, host.clone()).unwrap();
        assert_eq!(
            fs::read_link(deployment(&AppType::Claude)).unwrap(),
            service.root().join("skill-projections/claude")
        );
        assert_eq!(
            fs::read_link(deployment(&AppType::Pi)).unwrap(),
            service.root().join("skill-projections/pi")
        );
        service.delete_server_with_skills(db, &host.id).unwrap();
        assert!(!deployment(&AppType::Claude).exists());
        assert!(!deployment(&AppType::Pi).exists());
    });
}

#[test]
#[serial_test::serial]
fn selected_clients_receive_one_hidden_skill_with_distinct_catalogs() {
    with_home(|db, service| {
        let apps: Vec<_> = AppType::all()
            .filter(|app| !SkillApps::only(app).is_empty())
            .collect();
        let host = server(&apps);
        service.save_server_with_skills(db, host.clone()).unwrap();
        let skills = db.get_all_installed_skills().unwrap();
        assert_eq!(skills.len(), 1);
        let skill = skills.get("internal:vps").unwrap();
        assert_eq!(skill.managed_by.as_deref(), Some("vps"));
        assert!(skill.repo_owner.is_none());
        assert!(SkillService::get_all_installed(db).unwrap().is_empty());
        assert!(!SkillService::get_ssot_dir()
            .unwrap()
            .join("cc-switch-vps")
            .exists());
        for app in &apps {
            let content = fs::read_to_string(deployment(app).join("SKILL.md")).unwrap();
            let catalog = service
                .root()
                .join(format!("clients/{}.json", app.as_str()));
            assert!(
                content.contains(&serde_json::to_string(&catalog).unwrap()),
                "{app:?}"
            );
            assert!(content.contains("cc-switch-generated: vps"));
        }
        service.save_server_with_skills(db, host).unwrap();
        assert_eq!(db.get_all_installed_skills().unwrap().len(), 1);
        assert_eq!(
            db.get_installed_skill("internal:vps")
                .unwrap()
                .unwrap()
                .installed_at,
            skill.installed_at
        );
    });
}

#[test]
#[serial_test::serial]
fn shared_binding_keeps_skill_until_last_host_is_removed() {
    with_home(|db, service| {
        let first = server(&[AppType::Claude, AppType::Pi]);
        let second = server(&[AppType::Claude, AppType::Mcode]);
        service.save_server_with_skills(db, first.clone()).unwrap();
        service.save_server_with_skills(db, second.clone()).unwrap();
        service.delete_server_with_skills(db, &first.id).unwrap();
        assert!(deployment(&AppType::Claude).join("SKILL.md").exists());
        assert!(deployment(&AppType::Mcode).join("SKILL.md").exists());
        assert!(!deployment(&AppType::Pi).exists());
        service.delete_server_with_skills(db, &second.id).unwrap();
        assert!(!deployment(&AppType::Claude).exists());
        assert!(!deployment(&AppType::Mcode).exists());
        assert!(db.get_installed_skill("internal:vps").unwrap().is_none());
    });
}

#[test]
#[serial_test::serial]
fn initial_deployment_never_adopts_an_existing_same_name_directory() {
    with_home(|db, service| {
        let dest = deployment(&AppType::Claude);
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), b"user skill").unwrap();
        assert!(service
            .save_server_with_skills(db, server(&[AppType::Claude]))
            .is_err());
        assert_eq!(fs::read(dest.join("SKILL.md")).unwrap(), b"user skill");
        assert!(service.load().unwrap().is_empty());
        assert!(db.get_all_installed_skills().unwrap().is_empty());
    });
}

#[test]
#[serial_test::serial]
fn edited_deployment_is_preserved_on_unbind() {
    with_home(|db, service| {
        let mut host = server(&[AppType::Claude]);
        service.save_server_with_skills(db, host.clone()).unwrap();
        let path = deployment(&AppType::Claude).join("SKILL.md");
        fs::write(&path, b"user edited managed deployment").unwrap();
        host.apps.claude = false;
        assert!(service.save_server_with_skills(db, host).is_err());
        assert_eq!(fs::read(path).unwrap(), b"user edited managed deployment");
        assert!(service.load().unwrap()[0].apps.claude);
        assert!(db.get_installed_skill("internal:vps").unwrap().is_some());
    });
}

#[test]
#[serial_test::serial]
fn failed_final_database_update_is_not_success_and_retry_recovers() {
    with_home(|db, service| {
        db.conn.lock().unwrap().execute_batch(
            "CREATE TRIGGER fail_vps_apps BEFORE UPDATE ON skills BEGIN SELECT RAISE(FAIL, 'injected app update failure'); END;"
        ).unwrap();
        let host = server(&[AppType::Claude]);
        assert!(service.save_server_with_skills(db, host.clone()).is_err());
        assert_eq!(service.load().unwrap(), vec![host.clone()]);
        db.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_vps_apps;")
            .unwrap();
        service.get_servers_with_skills(db).unwrap();
        assert!(deployment(&AppType::Claude).join("SKILL.md").exists());
        assert_eq!(db.get_all_installed_skills().unwrap().len(), 1);
        assert!(
            db.get_installed_skill("internal:vps")
                .unwrap()
                .unwrap()
                .apps
                .claude
        );
    });
}

#[test]
#[serial_test::serial]
fn skill_resync_and_storage_migration_keep_generated_client_references() {
    with_home(|db, service| {
        let host = server(&[AppType::Claude, AppType::Pi, AppType::Mcode]);
        service.save_server_with_skills(db, host).unwrap();
        let expected = fs::read(deployment(&AppType::Claude).join("SKILL.md")).unwrap();
        fs::remove_dir_all(deployment(&AppType::Claude)).unwrap();
        SkillService::sync_to_app(db, &AppType::Claude).unwrap();
        assert_eq!(
            fs::read(deployment(&AppType::Claude).join("SKILL.md")).unwrap(),
            expected
        );
        let migration = SkillService::migrate_storage(db, SkillStorageLocation::Unified).unwrap();
        assert!(migration.errors.is_empty(), "{:?}", migration.errors);
        for app in [AppType::Claude, AppType::Pi, AppType::Mcode] {
            let content = fs::read_to_string(deployment(&app).join("SKILL.md")).unwrap();
            assert!(content.contains(
                &serde_json::to_string(
                    &service
                        .root()
                        .join(format!("clients/{}.json", app.as_str()))
                )
                .unwrap()
            ));
        }
    });
}
