use super::*;
use serde_json::{json, Value};
use std::fs;
use tempfile::TempDir;

fn service() -> (TempDir, VpsService) {
    let home = tempfile::tempdir().unwrap();
    let service = VpsService {
        root: home.path().join("vps"),
        fail_write: None,
    };
    (home, service)
}

fn server(apps: &[AppType]) -> VpsServer {
    let mut server = VpsServer::new("Test VPS".into(), "192.0.2.10".into(), "deploy".into());
    server.purpose = "Test only, no connection".into();
    for app in apps {
        server.apps.set_enabled_for(app, true);
    }
    server
}

fn read_json(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn catalog(service: &VpsService, app: &str) -> Value {
    read_json(service.root.join("clients").join(format!("{app}.json")))
}

#[test]
fn server_identity_and_alias_are_generated_once_and_survive_edits() {
    let mut one = server(&[]);
    let two = server(&[]);
    let id = one.id.clone();
    let alias = one.ssh_alias();
    assert_ne!(id, two.id);
    assert!(uuid::Uuid::parse_str(&id).is_ok());
    assert_eq!(alias, format!("cc-switch-vps-{id}"));
    one.name = "重命名した VPS".into();
    one.host = "vps.example.com".into();
    one.validate().unwrap();
    assert_eq!(one.id, id);
    assert_eq!(one.ssh_alias(), alias);
    let restored: VpsServer = serde_json::from_value(serde_json::to_value(&one).unwrap()).unwrap();
    assert_eq!(restored, one);
}

#[test]
fn validates_hosts_users_ports_and_control_characters() {
    for host in [
        "example.com",
        "localhost",
        "vps-1.example.com.",
        "192.0.2.1",
        "2001:db8::1",
        "::1",
    ] {
        let mut item = server(&[]);
        item.host = host.into();
        item.validate().unwrap();
    }
    for host in [
        "",
        "-oProxyCommand=anything",
        "a\nProxyCommand anything",
        "a\rb",
        "a b",
        "a\tb",
        "a#b",
        "user@host",
        "a/b",
        "a\\b",
        "*.example.com",
        "a..b",
        "a%h",
        "[::1]",
        "$(anything)",
    ] {
        let mut item = server(&[]);
        item.host = host.into();
        assert!(item.validate().is_err(), "must reject host {host:?}");
    }
    for user in ["", "-option", "a\nb", "a b", "a@b", "a\"b", "$(anything)"] {
        let mut item = server(&[]);
        item.user = user.into();
        assert!(item.validate().is_err(), "must reject user {user:?}");
    }
    for field in ["name", "purpose", "id"] {
        let mut item = server(&[]);
        match field {
            "name" => item.name = "bad\0name".into(),
            "purpose" => item.purpose = "bad\r\npurpose".into(),
            _ => item.id = "../escape".into(),
        }
        assert!(item.validate().is_err(), "must reject {field}");
    }
    let mut item = server(&[]);
    item.port = 0;
    assert!(item.validate().is_err());
    item.port = u16::MAX;
    item.validate().unwrap();
}

#[test]
fn key_references_allow_spaces_unicode_but_not_ssh_expansion_or_relative_paths() {
    let home = tempfile::tempdir().unwrap();
    let mut item = server(&[]);
    item.identity_file = Some(home.path().join("密钥 with spaces"));
    item.validate().unwrap();
    assert!(
        !item.identity_file.as_ref().unwrap().exists(),
        "validation must not read keys"
    );
    for path in ["relative-key", "-i-key", "~/key"] {
        item.identity_file = Some(PathBuf::from(path));
        assert!(item.validate().is_err(), "must reject {path}");
    }
    for name in ["bad\nHost *", "bad\"key", "key%h", "${HOME}", "a\0b"] {
        item.identity_file = Some(home.path().join(name));
        assert!(item.validate().is_err(), "must reject {name:?}");
    }
    item.identity_file = Some(home.path().join("folder").join("..").join("key"));
    assert!(item.validate().is_err());
}

#[test]
fn ssh_config_is_deterministic_strict_and_contains_no_description_as_commands() {
    let home = tempfile::tempdir().unwrap();
    let mut one = server(&[]);
    one.identity_file = Some(home.path().join("密钥 with spaces"));
    one.purpose = "$(touch never-run); ProxyCommand is data".into();
    let mut two = server(&[]);
    two.host = "2001:db8::2".into();
    let config = render_ssh_config(&[one.clone(), two.clone()]).unwrap();
    assert_eq!(
        config,
        render_ssh_config(&[two.clone(), one.clone()]).unwrap()
    );
    for item in [&one, &two] {
        assert!(config.contains(&format!("Host {}\n", item.ssh_alias())));
    }
    assert!(config.contains("StrictHostKeyChecking yes"));
    assert!(config.contains("BatchMode yes"));
    assert!(config.contains("IdentityFile none"));
    assert!(config.contains(&format!(
        "IdentityFile \"{}\"",
        one.identity_file
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/")
    )));
    assert!(!config.contains("ProxyCommand"));
    assert!(!config.contains(&one.purpose));
    assert!(!config.contains("StrictHostKeyChecking no"));
}

#[test]
fn ssh_rendering_rejects_invalid_records_and_duplicate_ids() {
    let one = server(&[]);
    assert!(render_ssh_config(&[one.clone(), one]).is_err());
    let mut invalid = server(&[]);
    invalid.user = "deploy\nLocalCommand anything".into();
    assert!(render_ssh_config(&[invalid]).is_err());
}

#[test]
fn aggregation_reuses_skill_capabilities_and_ignores_unsupported_apps() {
    let servers: Vec<_> = AppType::all().map(|app| server(&[app])).collect();
    let required = required_apps(&servers);
    for app in AppType::all() {
        assert_eq!(
            required.is_enabled_for(&app),
            !SkillApps::only(&app).is_empty()
        );
    }
    assert!(!required.is_enabled_for(&AppType::ClaudeDesktop));
    assert!(!required.is_enabled_for(&AppType::OpenClaw));
    assert!(required_apps(&[]).is_empty());
}

#[test]
fn client_plan_covers_shared_references_last_unbind_and_noop() {
    let one = server(&[AppType::Claude, AppType::Codex]);
    let two = server(&[AppType::Claude]);
    assert_eq!(
        plan_client_changes(&[], &[one.clone()]),
        vec![
            VpsClientChange {
                app: AppType::Claude,
                action: VpsClientAction::Install
            },
            VpsClientChange {
                app: AppType::Codex,
                action: VpsClientAction::Install
            },
        ]
    );
    assert_eq!(
        plan_client_changes(&[one.clone(), two.clone()], &[two.clone()]),
        vec![
            VpsClientChange {
                app: AppType::Claude,
                action: VpsClientAction::Update
            },
            VpsClientChange {
                app: AppType::Codex,
                action: VpsClientAction::Remove
            },
        ]
    );
    assert_eq!(
        plan_client_changes(&[two.clone()], &[]),
        vec![VpsClientChange {
            app: AppType::Claude,
            action: VpsClientAction::Remove
        },]
    );
    assert!(plan_client_changes(&[one.clone(), two.clone()], &[two, one]).is_empty());
}

#[test]
fn missing_data_is_empty_and_reading_does_not_create_files() {
    let (_home, service) = service();
    assert!(service.load().unwrap().is_empty());
    assert!(!service.root.exists());
}

#[test]
fn saves_local_data_and_only_catalogs_for_bound_clients() {
    let (home, service) = service();
    let ssh_config = home.path().join(".ssh/config");
    fs::create_dir_all(ssh_config.parent().unwrap()).unwrap();
    fs::write(&ssh_config, b"User-owned SSH config").unwrap();
    let mut one = server(&[AppType::Claude, AppType::Pi, AppType::Mcode]);
    one.identity_file = Some(home.path().join("secret-key-reference"));
    service.save_server(one.clone()).unwrap();
    let two = server(&[AppType::Codex]);
    service.save_server(two.clone()).unwrap();
    assert_eq!(service.load().unwrap().len(), 2);
    for app in ["claude", "pi", "mcode"] {
        let catalog = catalog(&service, app);
        assert_eq!(catalog["app"], app);
        assert_eq!(catalog["servers"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["servers"][0]["id"], one.id);
        assert_eq!(catalog["servers"][0]["sshAlias"], one.ssh_alias());
        assert_eq!(
            catalog["sshConfig"],
            service.root.join("ssh_config").to_str().unwrap()
        );
        assert!(!catalog.to_string().contains("secret-key-reference"));
        assert!(!catalog.to_string().contains(&one.host));
    }
    assert_eq!(catalog(&service, "codex")["servers"][0]["id"], two.id);
    assert!(!service.root.join("clients/claude-desktop.json").exists());
    assert!(!service.root.join("clients/openclaw.json").exists());
    assert_eq!(fs::read(&ssh_config).unwrap(), b"User-owned SSH config");
}

#[test]
fn removing_one_shared_host_updates_catalog_and_last_unbind_removes_it() {
    let (_home, service) = service();
    let one = server(&[AppType::Claude, AppType::Codex]);
    let mut two = server(&[AppType::Claude]);
    service.save_server(one.clone()).unwrap();
    service.save_server(two.clone()).unwrap();
    service.delete_server(&one.id).unwrap();
    assert_eq!(
        catalog(&service, "claude")["servers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(catalog(&service, "claude")["servers"][0]["id"], two.id);
    assert!(!service.root.join("clients/codex.json").exists());
    two.apps = SkillApps::default();
    service.save_server(two.clone()).unwrap();
    assert!(!service.root.join("clients/claude.json").exists());
    assert_eq!(service.load().unwrap(), vec![two.clone()]);
    assert!(fs::read_to_string(service.root.join("ssh_config"))
        .unwrap()
        .contains(&two.ssh_alias()));
}

#[test]
fn repeated_saves_and_reconciliation_do_not_rewrite_files_or_duplicate_hosts() {
    let (_home, service) = service();
    let one = server(&[AppType::Claude]);
    service.save_server(one.clone()).unwrap();
    let no_writes = VpsService {
        fail_write: Some(0),
        ..service.clone()
    };
    no_writes.save_server(one.clone()).unwrap();
    no_writes.reconcile().unwrap();
    assert_eq!(service.load().unwrap(), vec![one]);
}

#[test]
fn manual_server_edits_regenerate_catalogs_without_changing_identity() {
    let (_home, service) = service();
    let one = server(&[AppType::Gemini]);
    service.save_server(one.clone()).unwrap();
    let path = service.root.join("servers.json");
    let mut data = read_json(&path);
    data["servers"][0]["name"] = json!("Edited manually");
    data["servers"][0]["apps"]["grokbuild"] = json!(true);
    fs::write(&path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    service.reconcile().unwrap();
    for app in ["gemini", "grokbuild"] {
        assert_eq!(
            catalog(&service, app)["servers"][0]["name"],
            "Edited manually"
        );
        assert_eq!(catalog(&service, app)["servers"][0]["id"], one.id);
    }
}

#[test]
fn malformed_or_invalid_saved_data_is_not_replaced() {
    for bytes in [
        b"not json".to_vec(),
        serde_json::to_vec(&json!({ "version": 99, "servers": [] })).unwrap(),
        serde_json::to_vec(
            &json!({ "version": 1, "servers": [], "generatedFiles": { "../foreign": ["abc"] } }),
        )
        .unwrap(),
    ] {
        let (_home, service) = service();
        fs::create_dir_all(&service.root).unwrap();
        let path = service.root.join("servers.json");
        fs::write(&path, &bytes).unwrap();
        assert!(service.load().is_err());
        assert!(service.save_server(server(&[AppType::Claude])).is_err());
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert!(!service.root.join("ssh_config").exists());
    }
}

#[test]
fn invalid_or_duplicate_hosts_on_disk_are_rejected_before_generation() {
    let (_home, service) = service();
    fs::create_dir_all(&service.root).unwrap();
    let one = server(&[]);
    let mut invalid = one.clone();
    invalid.host = "host\nProxyCommand anything".into();
    for servers in [vec![one.clone(), one], vec![invalid]] {
        let data = json!({ "version": 1, "servers": servers });
        fs::write(
            service.root.join("servers.json"),
            serde_json::to_vec(&data).unwrap(),
        )
        .unwrap();
        assert!(service.reconcile().is_err());
        assert!(!service.root.join("ssh_config").exists());
    }
}

#[test]
fn unowned_or_user_modified_generated_files_are_preserved() {
    let (_home, service) = service();
    fs::create_dir_all(&service.root).unwrap();
    let output = service.root.join("ssh_config");
    fs::write(&output, b"User-owned file").unwrap();
    let one = server(&[AppType::Claude]);
    assert!(service.save_server(one.clone()).is_err());
    assert!(!service.root.join("servers.json").exists());
    assert_eq!(fs::read(&output).unwrap(), b"User-owned file");

    fs::remove_file(&output).unwrap();
    service.save_server(one.clone()).unwrap();
    let before = fs::read(service.root.join("servers.json")).unwrap();
    fs::write(service.root.join("clients/claude.json"), b"manually edited").unwrap();
    assert!(service.delete_server(&one.id).is_err());
    assert_eq!(fs::read(service.root.join("servers.json")).unwrap(), before);
    assert_eq!(
        fs::read(service.root.join("clients/claude.json")).unwrap(),
        b"manually edited"
    );
}

#[test]
fn foreign_files_in_the_feature_directory_are_not_removed() {
    let (_home, service) = service();
    let one = server(&[AppType::Hermes]);
    service.save_server(one.clone()).unwrap();
    let foreign = service.root.join("clients/notes.txt");
    fs::write(&foreign, b"keep me").unwrap();
    service.delete_server(&one.id).unwrap();
    assert_eq!(fs::read(&foreign).unwrap(), b"keep me");
    assert!(!service.root.join("clients/hermes.json").exists());
}

#[test]
fn write_failures_are_diagnostic_and_recoverable_after_reopening() {
    for fail_write in 0..5 {
        let (_home, service) = service();
        let mut one = server(&[AppType::Claude, AppType::Codex]);
        service.save_server(one.clone()).unwrap();
        one.name = "Updated".into();
        one.host = "192.0.2.20".into();
        let failing = VpsService {
            fail_write: Some(fail_write),
            ..service.clone()
        };
        let error = failing
            .save_server(one.clone())
            .expect_err("injected file write must fail");
        let message = format!("{error:#}");
        assert!(message.contains("injected"), "{message}");
        if fail_write > 0 {
            assert!(
                message.contains("saved"),
                "must distinguish saved data from generated files: {message}"
            );
            assert_eq!(service.load().unwrap(), vec![one.clone()]);
            service.reconcile().unwrap();
        } else {
            assert_ne!(service.load().unwrap()[0].name, one.name);
            service.save_server(one.clone()).unwrap();
        }
        assert_eq!(catalog(&service, "claude")["servers"][0]["name"], one.name);
        assert_eq!(catalog(&service, "codex")["servers"][0]["name"], one.name);
        assert!(fs::read_to_string(service.root.join("ssh_config"))
            .unwrap()
            .contains(&one.host));
        assert_eq!(service.load().unwrap().len(), 1);
        VpsService {
            fail_write: Some(0),
            ..service.clone()
        }
        .reconcile()
        .unwrap();
    }
}

#[test]
fn interrupted_deletion_can_be_retried_without_stale_catalogs() {
    let (_home, service) = service();
    let one = server(&[AppType::Claude, AppType::Codex]);
    service.save_server(one.clone()).unwrap();
    let failing = VpsService {
        fail_write: Some(2),
        ..service.clone()
    };
    assert!(failing.delete_server(&one.id).is_err());
    assert!(service.load().unwrap().is_empty());
    service.delete_server(&one.id).unwrap();
    assert!(!service.root.join("clients/claude.json").exists());
    assert!(!service.root.join("clients/codex.json").exists());
    assert!(!fs::read_to_string(service.root.join("ssh_config"))
        .unwrap()
        .contains(&one.ssh_alias()));
}

#[test]
fn concurrent_saves_do_not_lose_other_hosts() {
    let (_home, service) = service();
    let servers: Vec<_> = (0..8).map(|_| server(&[AppType::Claude])).collect();
    std::thread::scope(|scope| {
        for one in &servers {
            let service = service.clone();
            scope.spawn(move || service.save_server(one.clone()).unwrap());
        }
    });
    assert_eq!(service.load().unwrap().len(), servers.len());
    assert_eq!(
        catalog(&service, "claude")["servers"]
            .as_array()
            .unwrap()
            .len(),
        servers.len()
    );
}

#[test]
fn first_generation_failure_preserves_one_identity_and_can_resume() {
    let (_home, service) = service();
    let one = server(&[AppType::Claude, AppType::Codex]);
    let failing = VpsService {
        fail_write: Some(2),
        ..service.clone()
    };
    assert!(failing.save_server(one.clone()).is_err());
    assert_eq!(service.load().unwrap(), vec![one.clone()]);
    service.reconcile().unwrap();
    service.save_server(one.clone()).unwrap();
    assert_eq!(service.load().unwrap(), vec![one.clone()]);
    for app in ["claude", "codex"] {
        assert_eq!(catalog(&service, app)["servers"][0]["id"], one.id);
    }
}

#[test]
fn missing_generated_files_are_recreated_from_the_local_source() {
    let (_home, service) = service();
    let one = server(&[AppType::Pi]);
    service.save_server(one.clone()).unwrap();
    fs::remove_file(service.root.join("ssh_config")).unwrap();
    fs::remove_file(service.root.join("clients/pi.json")).unwrap();
    service.reconcile().unwrap();
    assert_eq!(catalog(&service, "pi")["servers"][0]["id"], one.id);
    assert!(fs::read_to_string(service.root.join("ssh_config"))
        .unwrap()
        .contains(&one.ssh_alias()));
}

#[test]
fn unsupported_client_bindings_cannot_be_silently_saved_as_disabled() {
    for app in ["openclaw", "claude-desktop", "unknown-client"] {
        let mut value = serde_json::to_value(server(&[])).unwrap();
        value["apps"][app] = json!(true);
        assert!(serde_json::from_value::<VpsServer>(value).is_err(), "{app}");
    }
}

#[test]
fn non_regular_storage_paths_fail_before_saving_data() {
    for relative in ["servers.json", "ssh_config", "clients/claude.json"] {
        let (_home, service) = service();
        fs::create_dir_all(service.root.join(relative)).unwrap();
        assert!(service.save_server(server(&[AppType::Claude])).is_err());
        assert!(!service.root.join("servers.json").is_file());
    }
    let (_home, service) = service();
    fs::create_dir_all(&service.root).unwrap();
    fs::write(service.root.join("clients"), b"not a directory").unwrap();
    assert!(service.save_server(server(&[AppType::Claude])).is_err());
    assert!(!service.root.join("servers.json").exists());
}

#[test]
fn changes_after_a_snapshot_are_not_overwritten() {
    let (_home, service) = service();
    fs::create_dir_all(&service.root).unwrap();
    let path = service.root.join("ssh_config");
    fs::write(&path, b"changed after preflight").unwrap();
    let mut writes = 0;
    assert!(service
        .replace_checked("ssh_config", Some(b"before"), Some(b"next"), &mut writes)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), b"changed after preflight");
    assert_eq!(writes, 0);
}

#[test]
fn changing_desired_data_after_interruption_still_recognizes_owned_outputs() {
    let (_home, service) = service();
    let mut one = server(&[AppType::Claude, AppType::Codex]);
    service.save_server(one.clone()).unwrap();
    one.name = "interrupted edit".into();
    let failing = VpsService {
        fail_write: Some(2),
        ..service.clone()
    };
    assert!(failing.save_server(one.clone()).is_err());
    one.name = "final edit".into();
    service.save_server(one.clone()).unwrap();
    assert_eq!(service.load().unwrap(), vec![one]);
    assert_eq!(
        catalog(&service, "claude")["servers"][0]["name"],
        "final edit"
    );
    assert_eq!(
        catalog(&service, "codex")["servers"][0]["name"],
        "final edit"
    );
}

#[cfg(windows)]
#[test]
fn junctioned_storage_is_rejected_without_touching_the_target() {
    let (home, service) = service();
    let external = home.path().join("external");
    fs::create_dir_all(&external).unwrap();
    // Directory junctions in our temporary directory do not require symlink privileges.
    let junction = |link: &Path| {
        let result = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(&external)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction creation failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    junction(&service.root);
    assert!(service.save_server(server(&[AppType::Claude])).is_err());
    assert!(fs::read_dir(&external).unwrap().next().is_none());
    fs::remove_dir(&service.root).unwrap();
    fs::create_dir_all(&service.root).unwrap();
    junction(&service.root.join("clients"));
    assert!(service.save_server(server(&[AppType::Claude])).is_err());
    assert!(fs::read_dir(&external).unwrap().next().is_none());
    fs::remove_dir(service.root.join("clients")).unwrap();
}

#[cfg(unix)]
#[test]
fn symlinked_storage_or_output_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;
    let (home, service) = service();
    let external = home.path().join("external");
    fs::create_dir_all(&external).unwrap();
    symlink(&external, &service.root).unwrap();
    assert!(service.save_server(server(&[])).is_err());
    assert!(fs::read_dir(&external).unwrap().next().is_none());
    fs::remove_file(&service.root).unwrap();
    fs::create_dir_all(&service.root).unwrap();
    let target = external.join("ssh");
    fs::write(&target, b"untouched").unwrap();
    symlink(&target, service.root.join("ssh_config")).unwrap();
    assert!(service.save_server(server(&[])).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"untouched");
}
