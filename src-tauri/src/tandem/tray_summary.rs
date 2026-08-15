use super::repository::TaskLedger;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskSummary {
    pub needs_attention: usize,
    pub awaiting_acceptance: usize,
    pub active: usize,
}

pub fn summarize(ledger: &TaskLedger) -> TaskSummary {
    TaskSummary {
        needs_attention: ledger.needs_attention.len(),
        awaiting_acceptance: ledger.awaiting_acceptance.len(),
        active: ledger.active.len(),
    }
}

pub fn summary_label(summary: &TaskSummary, language: &str) -> String {
    match language {
        "zh" | "zh-TW" => format!(
            "任务 · {} 需处理 · {} 待验收 · {} 推进中",
            summary.needs_attention, summary.awaiting_acceptance, summary.active
        ),
        _ => format!(
            "Tasks · {} attention · {} review · {} active",
            summary.needs_attention, summary.awaiting_acceptance, summary.active
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tandem::{
        domain::{Project, TandemTask, TaskStatus},
        repository::TaskLedgerItem,
    };

    fn item(id: &str, status: TaskStatus) -> TaskLedgerItem {
        TaskLedgerItem {
            task: TandemTask {
                id: id.into(),
                project_id: "project".into(),
                title: format!("Task {id}"),
                original_instruction: "Safe instruction".into(),
                status,
                created_at: 1,
                updated_at: 1,
                completed_at: (status == TaskStatus::Completed).then_some(1),
            },
            project: Project {
                id: "project".into(),
                name: "Project".into(),
                root_path: "/project".into(),
                created_at: 1,
                updated_at: 1,
            },
        }
    }

    #[test]
    fn summarize_returns_zero_counts_for_empty_ledger() {
        let summary = summarize(&TaskLedger {
            needs_attention: vec![],
            awaiting_acceptance: vec![],
            active: vec![],
            recent_resumable: vec![],
        });

        assert_eq!(summary.needs_attention, 0);
        assert_eq!(summary.awaiting_acceptance, 0);
        assert_eq!(summary.active, 0);
    }

    #[test]
    fn summarize_counts_only_the_active_bucket_as_active() {
        let summary = summarize(&TaskLedger {
            needs_attention: vec![item("attention", TaskStatus::NeedsAttention)],
            awaiting_acceptance: vec![
                item("review-1", TaskStatus::AwaitingAcceptance),
                item("review-2", TaskStatus::AwaitingAcceptance),
            ],
            active: vec![item("active", TaskStatus::Active)],
            recent_resumable: vec![item("paused", TaskStatus::Paused)],
        });

        assert_eq!(summary.needs_attention, 1);
        assert_eq!(summary.awaiting_acceptance, 2);
        assert_eq!(summary.active, 1);
    }

    #[test]
    fn summary_label_uses_exact_supported_titles_and_counts() {
        let summary = TaskSummary {
            needs_attention: 1,
            awaiting_acceptance: 2,
            active: 3,
        };

        let chinese = "任务 · 1 需处理 · 2 待验收 · 3 推进中";
        assert_eq!(summary_label(&summary, "zh"), chinese);
        assert_eq!(summary_label(&summary, "zh-TW"), chinese);
        let fallback = "Tasks · 1 attention · 2 review · 3 active";
        assert_eq!(summary_label(&summary, "en"), fallback);
        assert_eq!(summary_label(&summary, "ja"), fallback);
    }
}
