use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use cc_switch_lib::{
    commands::tandem::{
        confirm_tandem_task_completed_with_state, create_tandem_task_impl,
        create_tandem_task_with_state, list_tandem_ledger_impl, CreateTandemTaskInput,
    },
    tandem::{database::TandemDatabase, tandem_database_path, Clock, TandemState},
};
use serde_json::json;

struct FakeClock {
    now: i64,
    calls: AtomicUsize,
}

impl FakeClock {
    fn new(now: i64) -> Self {
        Self {
            now,
            calls: AtomicUsize::new(0),
        }
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.now
    }
}

fn input(instruction: &str) -> CreateTandemTaskInput {
    CreateTandemTaskInput {
        project_name: "Tandem".into(),
        project_root_path: "/tmp/tandem".into(),
        title: "Wire commands".into(),
        original_instruction: instruction.into(),
    }
}

#[test]
fn create_input_deserializes_camel_case() {
    let parsed: CreateTandemTaskInput = serde_json::from_value(json!({
        "projectName": "Tandem",
        "projectRootPath": "/tmp/tandem",
        "title": "Wire commands",
        "originalInstruction": "Implement command wiring"
    }))
    .unwrap();

    assert_eq!(parsed.project_name, "Tandem");
    assert_eq!(parsed.project_root_path, "/tmp/tandem");
    assert_eq!(parsed.original_instruction, "Implement command wiring");
}

#[tokio::test]
async fn create_validation_error_is_stable_and_does_not_echo_instruction() {
    let db = Arc::new(TandemDatabase::memory().unwrap());
    let secret_instruction = "instruction-that-must-not-be-echoed";
    let mut invalid = input(secret_instruction);
    invalid.title = " ".into();

    let error = create_tandem_task_impl(db, invalid, 10).await.unwrap_err();
    assert_eq!(error, "title must not be empty");
    assert!(!error.contains(secret_instruction));
}

#[tokio::test]
async fn create_list_complete_round_trip_forwards_fake_clock_timestamps() {
    let clock = Arc::new(FakeClock::new(1_700_000_000_123));
    let state = TandemState::available(Arc::new(TandemDatabase::memory().unwrap()), clock.clone());

    let created = create_tandem_task_with_state(&state, input("Implement command wiring"))
        .await
        .unwrap();
    assert_eq!(created.task.created_at, 1_700_000_000_123);
    assert_eq!(created.task.updated_at, 1_700_000_000_123);
    assert_eq!(clock.calls.load(Ordering::SeqCst), 1);

    let ledger = list_tandem_ledger_impl(state.db.clone().unwrap())
        .await
        .unwrap();
    assert_eq!(ledger.active, vec![created.clone()]);

    let completed = confirm_tandem_task_completed_with_state(&state, created.task.id.clone())
        .await
        .unwrap();
    assert_eq!(completed.task.completed_at, Some(1_700_000_000_123));
    assert_eq!(completed.task.updated_at, 1_700_000_000_123);
    assert_eq!(clock.calls.load(Ordering::SeqCst), 2);

    let ledger = list_tandem_ledger_impl(state.db.clone().unwrap())
        .await
        .unwrap();
    assert!(ledger.active.is_empty());
}

#[tokio::test]
async fn missing_task_id_returns_stable_error() {
    let db = Arc::new(TandemDatabase::memory().unwrap());
    let error = cc_switch_lib::commands::tandem::confirm_tandem_task_completed_impl(
        db,
        "missing-task".into(),
        20,
    )
    .await
    .unwrap_err();
    assert_eq!(error, "Tandem task not found: missing-task");
}

#[tokio::test]
async fn unavailable_state_returns_stored_reason_without_reading_clock() {
    let clock = Arc::new(FakeClock::new(99));
    let state = TandemState::unavailable("disk unavailable".into(), clock.clone());

    let error = create_tandem_task_with_state(&state, input("Do not leak this instruction"))
        .await
        .unwrap_err();
    assert_eq!(error, "Tandem unavailable: disk unavailable");
    assert_eq!(clock.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn tandem_database_path_is_isolated_in_config_directory() {
    let config_dir = tempfile::tempdir().unwrap();
    let path = tandem_database_path(config_dir.path());

    assert_eq!(path, config_dir.path().join("tandem.db"));
    assert_ne!(path, config_dir.path().join("cc-switch.db"));
    assert!(path.starts_with(Path::new(config_dir.path())));
}

#[test]
fn initialize_creates_tandem_database_in_config_directory() {
    let config_dir = tempfile::tempdir().unwrap();

    let state = TandemState::initialize(config_dir.path());

    state.database().unwrap();
    assert!(config_dir.path().join("tandem.db").is_file());
    assert!(!config_dir.path().join("cc-switch.db").exists());
    assert!(!config_dir.path().join("tandem").exists());
}
