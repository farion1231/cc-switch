use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde::Serialize;
use uuid::Uuid;

use crate::AppError;

use super::domain::{NewTask, Project, TandemDomainError, TandemTask, TaskStatus, TimestampMs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedgerItem {
    pub task: TandemTask,
    pub project: Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskLedger {
    pub needs_attention: Vec<TaskLedgerItem>,
    pub awaiting_acceptance: Vec<TaskLedgerItem>,
    pub active: Vec<TaskLedgerItem>,
    pub recent_resumable: Vec<TaskLedgerItem>,
}

pub(super) fn create_task(
    connection: &mut rusqlite::Connection,
    input: NewTask,
    now: TimestampMs,
) -> Result<TaskLedgerItem, AppError> {
    let input = input.validate().map_err(domain_error)?;
    let transaction = connection.transaction()?;

    let project = match select_project_by_root(&transaction, &input.project_root_path)? {
        Some(mut project) => {
            transaction.execute(
                "UPDATE tandem_projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![input.project_name, now, project.id],
            )?;
            project.name = input.project_name.clone();
            project.updated_at = now;
            project
        }
        None => {
            let project = Project {
                id: Uuid::new_v4().to_string(),
                name: input.project_name.clone(),
                root_path: input.project_root_path.clone(),
                created_at: now,
                updated_at: now,
            };
            transaction.execute(
                "INSERT INTO tandem_projects (id, name, root_path, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project.id,
                    project.name,
                    project.root_path,
                    project.created_at,
                    project.updated_at
                ],
            )?;
            project
        }
    };

    let task = TandemTask::new(Uuid::new_v4().to_string(), project.id.clone(), input, now);
    transaction.execute(
            "INSERT INTO tandem_tasks
             (id, project_id, title, original_instruction, status, created_at, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, NULL)",
            params![
                task.id,
                task.project_id,
                task.title,
                task.original_instruction,
                task.created_at,
                task.updated_at
            ],
        )?;
    transaction.commit()?;
    Ok(TaskLedgerItem { task, project })
}

pub(super) fn list_ledger(connection: &rusqlite::Connection) -> Result<TaskLedger, AppError> {
    let mut statement = connection.prepare(
        "SELECT
                t.id, t.project_id, t.title, t.original_instruction, t.status,
                t.created_at, t.updated_at, t.completed_at,
                p.id, p.name, p.root_path, p.created_at, p.updated_at
             FROM tandem_tasks t
             JOIN tandem_projects p ON p.id = t.project_id
             WHERE t.status != 'completed'
             ORDER BY t.updated_at DESC, t.id ASC",
    )?;
    let items = statement
        .query_map([], row_to_ledger_item)?
        .collect::<Result<Vec<_>, _>>()?;

    let mut ledger = TaskLedger {
        needs_attention: Vec::new(),
        awaiting_acceptance: Vec::new(),
        active: Vec::new(),
        recent_resumable: Vec::new(),
    };
    for item in items {
        match item.task.status {
            TaskStatus::NeedsAttention => ledger.needs_attention.push(item),
            TaskStatus::AwaitingAcceptance => ledger.awaiting_acceptance.push(item),
            TaskStatus::Active => ledger.active.push(item),
            TaskStatus::Paused if ledger.recent_resumable.len() < 10 => {
                ledger.recent_resumable.push(item);
            }
            TaskStatus::Paused | TaskStatus::Completed => {}
        }
    }
    Ok(ledger)
}

pub(super) fn confirm_task_completed(
    connection: &mut rusqlite::Connection,
    task_id: &str,
    now: TimestampMs,
) -> Result<TaskLedgerItem, AppError> {
    let transaction = connection.transaction()?;
    let (mut task, project) = select_task_with_project(&transaction, task_id)?
        .ok_or_else(|| AppError::Message(format!("Tandem task not found: {task_id}")))?;
    task.confirm_completed(now).map_err(domain_error)?;
    transaction.execute(
        "UPDATE tandem_tasks SET status = 'completed', updated_at = ?1, completed_at = ?2
             WHERE id = ?3",
        params![task.updated_at, task.completed_at, task.id],
    )?;
    transaction.commit()?;
    Ok(TaskLedgerItem { task, project })
}

fn select_project_by_root(
    transaction: &Transaction<'_>,
    root_path: &str,
) -> Result<Option<Project>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT id, name, root_path, created_at, updated_at
             FROM tandem_projects WHERE root_path = ?1",
            params![root_path],
            |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    root_path: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .optional()
}

fn select_task_with_project(
    transaction: &Transaction<'_>,
    task_id: &str,
) -> Result<Option<(TandemTask, Project)>, AppError> {
    let item = transaction
        .query_row(
            "SELECT
                t.id, t.project_id, t.title, t.original_instruction, t.status,
                t.created_at, t.updated_at, t.completed_at,
                p.id, p.name, p.root_path, p.created_at, p.updated_at
             FROM tandem_tasks t
             JOIN tandem_projects p ON p.id = t.project_id
             WHERE t.id = ?1",
            params![task_id],
            row_to_ledger_item,
        )
        .optional()?;
    Ok(item.map(|item| (item.task, item.project)))
}

fn row_to_ledger_item(row: &Row<'_>) -> Result<TaskLedgerItem, rusqlite::Error> {
    let status: String = row.get(4)?;
    let status = TaskStatus::try_from(status.as_str()).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(TaskLedgerItem {
        task: TandemTask {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            original_instruction: row.get(3)?,
            status,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            completed_at: row.get(7)?,
        },
        project: Project {
            id: row.get(8)?,
            name: row.get(9)?,
            root_path: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        },
    })
}

fn domain_error(error: TandemDomainError) -> AppError {
    AppError::Message(error.to_string())
}

#[cfg(test)]
mod tests {
    use crate::tandem::{database::TandemDatabase, domain::TaskStatus};
    use rusqlite::params;

    fn insert_project(database: &TandemDatabase) {
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO tandem_projects VALUES ('project','Project','/project',1,1)",
                [],
            )
            .unwrap();
    }

    fn insert_task(database: &TandemDatabase, id: &str, status: &str, updated_at: i64) {
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO tandem_tasks VALUES (?1,'project',?1,?1,?2,1,?3,NULL)",
                params![id, status, updated_at],
            )
            .unwrap();
    }

    #[test]
    fn ledger_groups_orders_limits_and_omits_completed_tasks() {
        let database = TandemDatabase::memory().unwrap();
        insert_project(&database);
        insert_task(&database, "attention-old", "needs_attention", 10);
        insert_task(&database, "attention-new", "needs_attention", 20);
        insert_task(&database, "acceptance", "awaiting_acceptance", 30);
        insert_task(&database, "active", "active", 40);
        for index in 0..12 {
            insert_task(
                &database,
                &format!("paused-{index:02}"),
                "paused",
                100 + index,
            );
        }
        insert_task(&database, "completed", "completed", 1_000);

        let ledger = database.list_ledger().unwrap();
        assert_eq!(
            ledger
                .needs_attention
                .iter()
                .map(|item| item.task.id.as_str())
                .collect::<Vec<_>>(),
            ["attention-new", "attention-old"]
        );
        assert_eq!(ledger.awaiting_acceptance[0].task.id, "acceptance");
        assert_eq!(ledger.active[0].task.id, "active");
        assert_eq!(ledger.recent_resumable.len(), 10);
        assert_eq!(ledger.recent_resumable[0].task.id, "paused-11");
        assert_eq!(ledger.recent_resumable[9].task.id, "paused-02");
        assert!(ledger
            .needs_attention
            .iter()
            .chain(&ledger.awaiting_acceptance)
            .chain(&ledger.active)
            .chain(&ledger.recent_resumable)
            .all(|item| item.task.status != TaskStatus::Completed));
    }

    #[test]
    fn invalid_persisted_task_enum_returns_typed_database_error() {
        let database = TandemDatabase::memory().unwrap();
        insert_project(&database);
        database.connection.lock().unwrap().execute_batch(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO tandem_tasks VALUES ('invalid','project','Invalid','Never leak this instruction','corrupt',1,1,NULL);
             PRAGMA ignore_check_constraints = OFF;",
        ).unwrap();

        let error = database.list_ledger().unwrap_err();
        assert!(matches!(error, crate::AppError::Database(_)));
        assert!(error
            .to_string()
            .contains("unknown Tandem task status: corrupt"));
        assert!(!error.to_string().contains("Never leak this instruction"));
    }

    #[test]
    fn create_task_rolls_back_project_after_induced_foreign_key_failure() {
        let database = TandemDatabase::memory().unwrap();
        database
            .connection
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER break_task_project BEFORE INSERT ON tandem_tasks
             BEGIN SELECT RAISE(ABORT, 'FOREIGN KEY constraint failed'); END;",
            )
            .unwrap();

        let result = database.create_task(
            crate::tandem::domain::NewTask {
                project_name: "Rollback".into(),
                project_root_path: "/rollback".into(),
                title: "Task".into(),
                original_instruction: "Safe instruction".into(),
            },
            10,
        );
        assert!(result.is_err());
        let count: i64 = database
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM tandem_projects WHERE root_path = '/rollback'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn repeated_completion_preserves_persisted_timestamps() {
        let database = TandemDatabase::memory().unwrap();
        let created = database
            .create_task(
                crate::tandem::domain::NewTask {
                    project_name: "Project".into(),
                    project_root_path: "/project".into(),
                    title: "Task".into(),
                    original_instruction: "Safe instruction".into(),
                },
                1_000,
            )
            .unwrap();
        database
            .confirm_task_completed(&created.task.id, 2_000)
            .unwrap();

        let error = database
            .confirm_task_completed(&created.task.id, 3_000)
            .unwrap_err();
        assert!(error.to_string().contains("already completed"));

        let connection = database.connection.lock().unwrap();
        let timestamps: (i64, Option<i64>) = connection
            .query_row(
                "SELECT updated_at, completed_at FROM tandem_tasks WHERE id = ?1",
                params![created.task.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(timestamps, (2_000, Some(2_000)));
    }

    #[test]
    fn deleting_project_cascades_to_tasks_and_runs() {
        let database = TandemDatabase::memory().unwrap();
        insert_project(&database);
        insert_task(&database, "task", "active", 1);
        database
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO tandem_runs VALUES ('run','task','codex','active',NULL,NULL,1,1,NULL)",
                [],
            )
            .unwrap();

        database
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM tandem_projects WHERE id = 'project'", [])
            .unwrap();
        let connection = database.connection.lock().unwrap();
        let task_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM tandem_tasks", [], |row| row.get(0))
            .unwrap();
        let run_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM tandem_runs", [], |row| row.get(0))
            .unwrap();
        assert_eq!((task_count, run_count), (0, 0));
    }
}
