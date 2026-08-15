use cc_switch_lib::{
    tandem::{
        database::TandemDatabase,
        domain::{NewTask, TaskStatus},
    },
    AppError,
};

fn new_task(project_name: &str, root: &str, title: &str, instruction: &str) -> NewTask {
    NewTask {
        project_name: project_name.into(),
        project_root_path: root.into(),
        title: title.into(),
        original_instruction: instruction.into(),
    }
}

#[test]
fn tandem_repository_reuses_trimmed_project_and_round_trips_unicode_fields() {
    let database = TandemDatabase::memory().unwrap();
    let first = database
        .create_task(
            new_task(
                "  项目 Ω  ",
                "  /tmp/项目 Ω  ",
                "  修复 café  ",
                "  保留 Unicode 内容 🚀  ",
            ),
            1_000,
        )
        .unwrap();
    let second = database
        .create_task(
            new_task(
                "  Renamed 项目  ",
                "/tmp/项目 Ω",
                "Second task",
                "No secret content",
            ),
            2_000,
        )
        .unwrap();

    assert_eq!(first.project.id, second.project.id);
    assert_eq!(first.project.root_path, "/tmp/项目 Ω");
    assert_eq!(second.project.name, "Renamed 项目");
    assert_eq!(second.project.created_at, 1_000);
    assert_eq!(second.project.updated_at, 2_000);
    assert_eq!(first.task.title, "修复 café");
    assert_eq!(first.task.original_instruction, "保留 Unicode 内容 🚀");
    assert_eq!(first.task.status, TaskStatus::Active);

    let ledger = database.list_ledger().unwrap();
    assert_eq!(ledger.active.len(), 2);
    assert_eq!(ledger.active[0], second);
    assert_eq!(ledger.active[1].task, first.task);
    assert_eq!(ledger.active[1].project.name, "Renamed 项目");
    assert!(ledger.needs_attention.is_empty());
    assert!(ledger.awaiting_acceptance.is_empty());
    assert!(ledger.recent_resumable.is_empty());
}

#[test]
fn tandem_repository_completion_persists_and_removes_task_from_ledger() {
    let database = TandemDatabase::memory().unwrap();
    let created = database
        .create_task(
            new_task("Project", "/tmp/project", "Task", "Safe instruction"),
            1_000,
        )
        .unwrap();

    let completed = database
        .confirm_task_completed(&created.task.id, 2_000)
        .unwrap();
    assert_eq!(completed.task.status, TaskStatus::Completed);
    assert_eq!(completed.task.updated_at, 2_000);
    assert_eq!(completed.task.completed_at, Some(2_000));

    let ledger = database.list_ledger().unwrap();
    assert!(ledger.active.is_empty());
    assert!(ledger.needs_attention.is_empty());
    assert!(ledger.awaiting_acceptance.is_empty());
    assert!(ledger.recent_resumable.is_empty());
}

#[test]
fn tandem_repository_missing_completion_is_not_found() {
    let database = TandemDatabase::memory().unwrap();
    let error = database
        .confirm_task_completed("missing", 2_000)
        .unwrap_err();

    assert!(matches!(
        error,
        AppError::Message(message) if message == "Tandem task not found: missing"
    ));
}

#[test]
fn tandem_repository_repeated_completion_preserves_first_timestamp() {
    let database = TandemDatabase::memory().unwrap();
    let created = database
        .create_task(
            new_task("Project", "/tmp/project", "Task", "Safe instruction"),
            1_000,
        )
        .unwrap();
    let completed = database
        .confirm_task_completed(&created.task.id, 2_000)
        .unwrap();
    assert_eq!(completed.task.updated_at, 2_000);
    assert_eq!(completed.task.completed_at, Some(2_000));

    let error = database
        .confirm_task_completed(&created.task.id, 3_000)
        .unwrap_err();
    assert!(error.to_string().contains("already completed"));
    assert!(!error.to_string().contains("Safe instruction"));

    let persisted = database
        .confirm_task_completed(&created.task.id, 4_000)
        .unwrap_err();
    assert!(persisted.to_string().contains("already completed"));
}
