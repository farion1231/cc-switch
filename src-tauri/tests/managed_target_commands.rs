use std::collections::BTreeMap;

use cc_switch_lib::{
    inspectManagedTarget, listManagedTargets, register_managed_target,
    set_managed_target_provider_link, update_settings, AppSettings, AppType, ConfigLocation,
    ManagedTarget, ManagementState, TargetArtifactState, TargetKind,
};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn managed_target_commands_resolve_registered_target_by_id() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_dir = home.join("windows-codex");
    std::fs::create_dir_all(&config_dir).expect("create target directory");
    std::fs::write(
        config_dir.join("config.toml"),
        "model_provider = \"openai\"\n",
    )
    .expect("seed config");

    update_settings(AppSettings {
        managed_targets: vec![ManagedTarget {
            id: "windows-codex".to_string(),
            app: AppType::Codex,
            name: "Windows Codex".to_string(),
            kind: TargetKind::LocalWindows,
            config_location: ConfigLocation {
                path: config_dir.display().to_string(),
            },
            current_provider_id: Some("codex-official".to_string()),
            management_state: ManagementState::Managed,
            provider_overrides: BTreeMap::new(),
            last_viewed_at: None,
        }],
        ..Default::default()
    })
    .expect("seed target registry");

    let targets = futures::executor::block_on(listManagedTargets()).expect("list targets");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "windows-codex");

    let inspection = futures::executor::block_on(inspectManagedTarget("windows-codex".to_string()))
        .expect("inspect registered target");
    assert_eq!(inspection.target_id, "windows-codex");
    assert!(inspection.reachable);
    assert_eq!(inspection.config, TargetArtifactState::Valid);

    let error = futures::executor::block_on(inspectManagedTarget("unknown".to_string()))
        .expect_err("unknown target must be rejected");
    assert!(error.contains("unknown"));
}

#[test]
fn registering_wsl_target_updates_only_local_registry_and_rejects_duplicates() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let simulated_wsl_live = home.join("simulated-wsl-live-config.toml");
    std::fs::write(&simulated_wsl_live, b"model_provider = \"openai\"\n")
        .expect("seed simulated WSL live config");
    let before = std::fs::read(&simulated_wsl_live).expect("snapshot WSL live config");
    let target = ManagedTarget {
        id: "wsl-ubuntu-mikasa".to_string(),
        app: AppType::Codex,
        name: "Ubuntu · mikasa".to_string(),
        kind: TargetKind::Wsl {
            distro: "Ubuntu".to_string(),
            user: "mikasa".to_string(),
        },
        config_location: ConfigLocation {
            path: "/home/mikasa/.codex".to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Unmanaged,
        provider_overrides: BTreeMap::new(),
        last_viewed_at: None,
    };

    let registered = register_managed_target(target.clone()).expect("register WSL target");

    assert_eq!(registered, target);
    assert_eq!(list_managed_targets_sync(), vec![target.clone()]);
    assert_eq!(
        std::fs::read(&simulated_wsl_live).expect("read WSL live after registration"),
        before
    );

    let error = register_managed_target(ManagedTarget {
        id: "another-id".to_string(),
        ..target
    })
    .expect_err("duplicate WSL config identity must be rejected");
    assert!(error.to_string().contains("already managed"));
    assert_eq!(list_managed_targets_sync().len(), 1);
}

fn list_managed_targets_sync() -> Vec<ManagedTarget> {
    futures::executor::block_on(listManagedTargets()).expect("list registered targets")
}

#[test]
fn linking_provider_keeps_target_unmanaged_and_does_not_write_live_files() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let simulated_live = home.join("linked-target-live.toml");
    std::fs::write(&simulated_live, b"model_provider = \"existing\"\n")
        .expect("seed simulated live config");
    let before = std::fs::read(&simulated_live).expect("snapshot live config");
    register_managed_target(ManagedTarget {
        id: "wsl-linked".to_string(),
        app: AppType::Codex,
        name: "Ubuntu · mikasa".to_string(),
        kind: TargetKind::Wsl {
            distro: "Ubuntu".to_string(),
            user: "mikasa".to_string(),
        },
        config_location: ConfigLocation {
            path: "/home/mikasa/.codex".to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Unmanaged,
        provider_overrides: BTreeMap::new(),
        last_viewed_at: None,
    })
    .expect("register target");

    let linked =
        set_managed_target_provider_link("wsl-linked", Some("provider-a")).expect("link provider");

    assert_eq!(linked.current_provider_id.as_deref(), Some("provider-a"));
    assert_eq!(linked.management_state, ManagementState::Unmanaged);
    assert_eq!(
        std::fs::read(&simulated_live).expect("read live config after link"),
        before
    );

    let cleared =
        set_managed_target_provider_link("wsl-linked", None).expect("clear provider link");
    assert!(cleared.current_provider_id.is_none());
    assert_eq!(cleared.management_state, ManagementState::Unmanaged);
}

#[test]
fn provider_link_cannot_change_a_managed_target_without_switching_live() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    register_managed_target(ManagedTarget {
        id: "managed-windows".to_string(),
        app: AppType::Codex,
        name: "Windows Codex".to_string(),
        kind: TargetKind::LocalWindows,
        config_location: ConfigLocation {
            path: r"C:\Users\Mikasa\.codex".to_string(),
        },
        current_provider_id: Some("provider-a".to_string()),
        management_state: ManagementState::Managed,
        provider_overrides: BTreeMap::new(),
        last_viewed_at: None,
    })
    .expect("register managed target");

    let error = set_managed_target_provider_link("managed-windows", Some("provider-b"))
        .expect_err("managed target must use the live switch transaction");

    assert!(error.to_string().contains("Unmanaged"));
    assert_eq!(
        list_managed_targets_sync()[0]
            .current_provider_id
            .as_deref(),
        Some("provider-a")
    );
}
