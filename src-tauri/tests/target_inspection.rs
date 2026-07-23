use std::collections::BTreeMap;
use std::path::Path;

use cc_switch_lib::{
    AppType, ConfigLocation, ManagedTarget, ManagementState, TargetArtifactState, TargetKind,
    WindowsTargetInspector, WslTargetAdapter,
};

fn snapshot_tree(root: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, entries: &mut BTreeMap<String, Option<Vec<u8>>>) {
        let mut children = std::fs::read_dir(path)
            .expect("read snapshot directory")
            .map(|entry| entry.expect("snapshot entry").path())
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            let relative = child
                .strip_prefix(root)
                .expect("relative path")
                .to_string_lossy()
                .replace('\\', "/");
            if child.is_dir() {
                entries.insert(relative, None);
                visit(root, &child, entries);
            } else {
                entries.insert(
                    relative,
                    Some(std::fs::read(&child).expect("snapshot file")),
                );
            }
        }
    }

    let mut entries = BTreeMap::new();
    visit(root, root, &mut entries);
    entries
}

#[cfg(unix)]
#[test]
fn wsl_discovery_reports_online_and_offline_distros_from_utf16_output() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("fake wsl directory");
    let executable = temp.path().join("wsl.exe");
    let log = temp.path().join("discovery-arguments.log");
    let script = format!(
        r#"#!/bin/sh
{{
  printf 'CALL\n'
  for arg in "$@"; do printf 'ARG:%s\n' "$arg"; done
}} >> '{}'
if [ "$1" = "--list" ]; then
  printf 'U\000b\000u\000n\000t\000u\000\n\000D\000e\000b\000i\000a\000n\000\n\000'
  exit 0
fi
distro="$2"
while [ "$1" != "--" ]; do shift; done
shift
command="$1"
shift
case "$distro:$command:$1" in
  "Ubuntu:id:-un") printf 'mikasa\n' ;;
  "Ubuntu:printenv:HOME") printf '/home/mikasa\n' ;;
  "Ubuntu:test:-d") [ "$2" = "/home/mikasa/.codex" ] ;;
  "Debian:id:-un") exit 1 ;;
  *) exit 2 ;;
esac
"#,
        log.display()
    );
    std::fs::write(&executable, script).expect("write fake wsl executable");
    let mut permissions = std::fs::metadata(&executable)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).expect("make fake wsl executable");

    let discovered = WslTargetAdapter::with_executable(&executable)
        .discover()
        .expect("discover WSL targets");

    assert_eq!(discovered.len(), 2);
    assert_eq!(discovered[0].distro, "Ubuntu");
    assert_eq!(discovered[0].user.as_deref(), Some("mikasa"));
    assert_eq!(
        discovered[0].config_path.as_deref(),
        Some("/home/mikasa/.codex")
    );
    assert!(discovered[0].reachable);
    assert!(discovered[0].codex_config_present);
    assert_eq!(discovered[1].distro, "Debian");
    assert!(!discovered[1].reachable);
    assert!(discovered[1].user.is_none());

    let arguments = std::fs::read_to_string(log).expect("read argument log");
    assert!(arguments.contains("ARG:--list\nARG:--quiet\n"));
    assert!(!arguments.contains("ARG:sh\n"));
    assert!(!arguments.contains("ARG:-c\n"));
}

#[test]
fn windows_target_inspection_reports_codex_artifacts_without_writing_target() {
    let temp = tempfile::tempdir().expect("target directory");
    std::fs::write(
        temp.path().join("config.toml"),
        "model_provider = \"openai\"\n",
    )
    .expect("config");
    std::fs::write(temp.path().join("auth.json"), "{\"tokens\":{}}\n").expect("auth");
    let active = temp.path().join("sessions/2026/07/21");
    let archived = temp.path().join("archived_sessions");
    std::fs::create_dir_all(&active).expect("active sessions");
    std::fs::create_dir_all(&archived).expect("archived sessions");
    std::fs::write(active.join("active.jsonl"), "{}\n").expect("active session");
    std::fs::write(archived.join("archived.jsonl"), "{}\n").expect("archived session");
    std::fs::write(temp.path().join("state_5.sqlite"), b"sqlite-bytes").expect("state DB");
    let before = snapshot_tree(temp.path());
    let target = ManagedTarget {
        id: "windows-codex".to_string(),
        app: AppType::Codex,
        name: "Windows Codex".to_string(),
        kind: TargetKind::LocalWindows,
        config_location: ConfigLocation {
            path: temp.path().display().to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Unmanaged,
        provider_overrides: Default::default(),
        last_viewed_at: None,
    };

    let inspection = WindowsTargetInspector::inspect(&target).expect("inspect target");

    assert!(inspection.reachable);
    assert_eq!(inspection.config, TargetArtifactState::Valid);
    assert_eq!(inspection.auth, TargetArtifactState::Valid);
    assert_eq!(inspection.active_session_count, 1);
    assert_eq!(inspection.archived_session_count, 1);
    assert!(inspection.state_db_present);
    assert_eq!(snapshot_tree(temp.path()), before);
}

#[cfg(unix)]
#[test]
fn wsl_target_inspection_uses_argument_safe_read_only_commands() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("fake wsl directory");
    let executable = temp.path().join("wsl.exe");
    let log = temp.path().join("arguments.log");
    let script = format!(
        r#"#!/bin/sh
{{
  printf 'CALL\n'
  for arg in "$@"; do printf 'ARG:%s\n' "$arg"; done
}} >> '{}'
while [ "$1" != "--" ]; do shift; done
shift
command="$1"
shift
case "$command" in
  test)
    flag="$1"
    path="$2"
    case "$flag:$path" in
      "-d:/home/mikasa/.codex"|"-d:/home/mikasa/.codex/sessions"|"-d:/home/mikasa/.codex/archived_sessions"|"-f:/home/mikasa/.codex/config.toml"|"-f:/home/mikasa/.codex/auth.json"|"-f:/home/mikasa/.codex/state_5.sqlite") exit 0 ;;
      *) exit 1 ;;
    esac
    ;;
  cat)
    path="$2"
    case "$path" in
      */config.toml) printf 'model_provider = "openai"\n' ;;
      */auth.json) printf '{{"tokens":{{}}}}\n' ;;
      *) exit 1 ;;
    esac
    ;;
  find)
    case "$1" in
      */sessions) printf 'one.jsonl\000two.jsonl\000' ;;
      */archived_sessions) printf 'old.jsonl\000' ;;
      *) exit 1 ;;
    esac
    ;;
  printenv)
    [ "$1" = "HOME" ] && printf '/home/mikasa\n'
    ;;
  *) exit 2 ;;
esac
"#,
        log.display()
    );
    std::fs::write(&executable, script).expect("write fake wsl executable");
    let mut permissions = std::fs::metadata(&executable)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).expect("make fake wsl executable");

    let target = ManagedTarget {
        id: "ubuntu-codex".to_string(),
        app: AppType::Codex,
        name: "Ubuntu Codex".to_string(),
        kind: TargetKind::Wsl {
            distro: "Ubuntu-24.04".to_string(),
            user: "mikasa".to_string(),
        },
        config_location: ConfigLocation {
            path: "/home/mikasa/.codex".to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Unmanaged,
        provider_overrides: Default::default(),
        last_viewed_at: None,
    };

    let inspection = WslTargetAdapter::with_executable(&executable)
        .inspect(&target)
        .expect("inspect WSL target");

    assert!(inspection.reachable);
    assert_eq!(inspection.config, TargetArtifactState::Valid);
    assert_eq!(inspection.auth, TargetArtifactState::Valid);
    assert_eq!(inspection.active_session_count, 2);
    assert_eq!(inspection.archived_session_count, 1);
    assert!(inspection.state_db_present);

    let arguments = std::fs::read_to_string(log).expect("read argument log");
    assert!(arguments.contains("ARG:-d\nARG:Ubuntu-24.04\n"));
    assert!(arguments.contains("ARG:-u\nARG:mikasa\n"));
    assert!(arguments.contains("ARG:/home/mikasa/.codex/config.toml\n"));
    assert!(!arguments.contains("ARG:sh\n"));
    assert!(!arguments.contains("ARG:-c\n"));
}

#[cfg(unix)]
#[test]
fn wsl_adapter_projects_provider_fields_atomically_and_restores_exact_snapshot() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("fake wsl directory");
    let executable = temp.path().join("wsl.exe");
    let live = temp.path().join("live-config.toml");
    let pending = temp.path().join("pending-config.toml");
    let catalog = temp.path().join("model-catalog.json");
    let pending_catalog = temp.path().join("pending-model-catalog.json");
    let fail_next_config_replace = temp.path().join("fail-next-config-replace");
    let log = temp.path().join("write-arguments.log");
    let original = b"# WSL local config\napproval_policy = \"on-request\"\nmodel_provider = \"old\"\nmodel = \"old-model\"\nsqlite_home = \"/home/mikasa/state\"\n\n[mcp_servers.local]\ncommand = \"linux-tool\"\n\n[model_providers.old]\nbase_url = \"https://old.example/v1\"\n";
    let original_catalog = br#"{"models":[{"slug":"old-model"}]}"#;
    std::fs::write(&live, original).expect("seed live config");
    std::fs::write(&catalog, original_catalog).expect("seed live model catalog");
    std::fs::set_permissions(&live, std::fs::Permissions::from_mode(0o640))
        .expect("set live config permissions");
    std::fs::set_permissions(&catalog, std::fs::Permissions::from_mode(0o640))
        .expect("set model catalog permissions");
    let script = format!(
        r#"#!/bin/sh
{{
  printf 'CALL\n'
  for arg in "$@"; do printf 'ARG:%s\n' "$arg"; done
}} >> '{}'
while [ "$1" != "--" ] && [ "$1" != "--exec" ]; do shift; done
shift
command="$1"
shift
case "$command" in
  /bin/sh)
    case "$4" in
      *cc-switch-model-catalog.json)
        mapped_path='{}'
        mapped_temporary='{}'
        ;;
      *)
        if [ -f '{}' ]; then
          rm -f '{}'
          exit 55
        fi
        mapped_path='{}'
        mapped_temporary='{}'
        ;;
    esac
    exec /bin/sh -c "$2" "$3" "$mapped_path" "$mapped_temporary" "$6"
    ;;
  test)
    case "$2" in
      *cc-switch-model-catalog.json) [ -f '{}' ] ;;
      *) [ -f '{}' ] ;;
    esac
    ;;
  cat)
    case "$2" in
      *cc-switch-model-catalog.json) cat '{}' ;;
      *) cat '{}' ;;
    esac
    ;;
  tee)
    case "$2" in
      *cc-switch-model-catalog.json*) cat > '{}' ;;
      *) cat > '{}' ;;
    esac
    ;;
  stat)
    echo 640
    ;;
  chmod)
    case "$3" in
      *cc-switch-model-catalog.json*) chmod "$2" '{}' ;;
      *) chmod "$2" '{}' ;;
    esac
    ;;
  mv)
    case "$4" in
      *cc-switch-model-catalog.json) mv '{}' '{}' ;;
      *)
        if [ -f '{}' ]; then
          rm -f '{}'
          exit 55
        fi
        mv '{}' '{}'
        ;;
    esac
    ;;
  rm)
    for target in "$@"; do :; done
    case "$target" in
      *cc-switch-model-catalog.json) rm -f '{}' '{}' ;;
      *) rm -f '{}' '{}' ;;
    esac
    ;;
  *) exit 2 ;;
esac
"#,
        log.display(),
        catalog.display(),
        pending_catalog.display(),
        fail_next_config_replace.display(),
        fail_next_config_replace.display(),
        live.display(),
        pending.display(),
        catalog.display(),
        live.display(),
        catalog.display(),
        live.display(),
        pending_catalog.display(),
        pending.display(),
        pending_catalog.display(),
        pending.display(),
        pending_catalog.display(),
        catalog.display(),
        fail_next_config_replace.display(),
        fail_next_config_replace.display(),
        pending.display(),
        live.display(),
        pending_catalog.display(),
        catalog.display(),
        pending.display(),
        live.display(),
    );
    std::fs::write(&executable, script).expect("write fake wsl executable");
    let mut permissions = std::fs::metadata(&executable)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).expect("make fake wsl executable");

    let target = ManagedTarget {
        id: "ubuntu-codex-write".to_string(),
        app: AppType::Codex,
        name: "Ubuntu Codex".to_string(),
        kind: TargetKind::Wsl {
            distro: "Ubuntu-24.04".to_string(),
            user: "mikasa".to_string(),
        },
        config_location: ConfigLocation {
            path: "/home/mikasa/.codex".to_string(),
        },
        current_provider_id: None,
        management_state: ManagementState::Unmanaged,
        provider_overrides: Default::default(),
        last_viewed_at: None,
    };
    let desired = r#"model_provider = "custom"
model = "gpt-new"
sqlite_home = "C:/Users/Mikasa/.codex"

[model_providers.custom]
base_url = "https://new.example/v1"
wire_api = "responses"
experimental_bearer_token = "secret"
"#;
    let adapter = WslTargetAdapter::with_executable(&executable);
    let desired_catalog = br#"{"models":[{"slug":"gpt-new"}]}"#;

    let snapshot = adapter
        .apply_provider_config(&target, desired, Some(desired_catalog))
        .expect("apply Provider projection");
    let apply_call_count = std::fs::read_to_string(&log)
        .expect("read WSL invocation log after apply")
        .lines()
        .filter(|line| *line == "CALL")
        .count();
    assert_eq!(
        apply_call_count, 4,
        "a two-file WSL projection should use two snapshot reads and two batched atomic writes"
    );
    let projected = std::fs::read_to_string(&live).expect("read projected config");
    let parsed = projected
        .parse::<toml::Table>()
        .expect("valid projected TOML");
    assert_eq!(parsed["model_provider"].as_str(), Some("custom"));
    assert_eq!(parsed["model"].as_str(), Some("gpt-new"));
    assert_eq!(parsed["sqlite_home"].as_str(), Some("/home/mikasa/state"));
    assert_eq!(
        parsed["mcp_servers"]["local"]["command"].as_str(),
        Some("linux-tool")
    );
    assert_eq!(
        std::fs::read(&catalog).expect("read projected catalog"),
        desired_catalog
    );
    assert_eq!(
        std::fs::metadata(&live)
            .expect("projected config metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );

    adapter
        .restore_provider_config(&target, &snapshot)
        .expect("restore exact snapshot");
    assert_eq!(
        std::fs::read(&live).expect("read restored config"),
        original
    );
    assert_eq!(
        std::fs::read(&catalog).expect("read restored catalog"),
        original_catalog
    );

    std::fs::write(&fail_next_config_replace, b"fail once")
        .expect("arm config replacement failure");
    let error = adapter
        .apply_provider_config(&target, desired, Some(desired_catalog))
        .expect_err("config replacement failure must abort the two-file transaction");
    assert!(error
        .to_string()
        .contains("atomically write and verify WSL managed file"));
    assert_eq!(
        std::fs::read(&live).expect("read config after automatic rollback"),
        original
    );
    assert_eq!(
        std::fs::read(&catalog).expect("read catalog after automatic rollback"),
        original_catalog
    );

    let arguments = std::fs::read_to_string(log).expect("read argument log");
    assert!(
        arguments.contains("ARG:--exec\nARG:/bin/sh\n"),
        "WSL shell scripts must use explicit exec mode so Windows preserves positional arguments"
    );
    assert!(arguments.contains("ARG:/bin/sh\n"));
    assert!(arguments.contains("ARG:-c\n"));
    assert!(!arguments.contains("ARG:-l\n"));
    assert!(!arguments.contains("ARG:bash\n"));
}
