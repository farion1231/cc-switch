use serde::{Deserialize, Serialize};

pub type TimestampMs = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Cursor,
    Codex,
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Active,
    NeedsAttention,
    AwaitingAcceptance,
    Paused,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Active,
    AwaitingUser,
    AwaitingAcceptance,
    Paused,
    Ended,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("unknown Tandem {kind}: {value}")]
pub struct PersistedEnumError {
    kind: &'static str,
    value: String,
}

macro_rules! persisted_enum {
    ($type:ty, $kind:literal, {$($name:literal => $variant:path),+ $(,)?}) => {
        impl TryFrom<&str> for $type {
            type Error = PersistedEnumError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                match value {
                    $($name => Ok($variant),)+
                    _ => Err(PersistedEnumError {
                        kind: $kind,
                        value: value.to_owned(),
                    }),
                }
            }
        }
    };
}

persisted_enum!(TaskStatus, "task status", {
    "active" => TaskStatus::Active,
    "needs_attention" => TaskStatus::NeedsAttention,
    "awaiting_acceptance" => TaskStatus::AwaitingAcceptance,
    "paused" => TaskStatus::Paused,
    "completed" => TaskStatus::Completed,
});
persisted_enum!(RunStatus, "run status", {
    "active" => RunStatus::Active,
    "awaiting_user" => RunStatus::AwaitingUser,
    "awaiting_acceptance" => RunStatus::AwaitingAcceptance,
    "paused" => RunStatus::Paused,
    "ended" => RunStatus::Ended,
});
persisted_enum!(AgentKind, "agent", {
    "cursor" => AgentKind::Cursor,
    "codex" => AgentKind::Codex,
    "claude" => AgentKind::Claude,
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TandemTask {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub original_instruction: String,
    pub status: TaskStatus,
    pub created_at: TimestampMs,
    pub updated_at: TimestampMs,
    pub completed_at: Option<TimestampMs>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub id: String,
    pub task_id: String,
    pub agent: AgentKind,
    pub status: RunStatus,
    pub native_session_id: Option<String>,
    pub native_session_ref: Option<String>,
    pub started_at: TimestampMs,
    pub updated_at: TimestampMs,
    pub ended_at: Option<TimestampMs>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub project_name: String,
    pub project_root_path: String,
    pub title: String,
    pub original_instruction: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    PrivateKey,
    ApiToken,
    NamedSecret,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TandemDomainError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("{field} exceeds {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("{field} contains a structured plaintext credential ({kind:?})")]
    StructuredCredential {
        field: &'static str,
        kind: CredentialKind,
    },
    #[error("task is already completed")]
    AlreadyCompleted,
}

impl NewTask {
    pub fn validate(mut self) -> Result<Self, TandemDomainError> {
        self.project_name = self.project_name.trim().to_owned();
        self.project_root_path = self.project_root_path.trim().to_owned();
        self.title = self.title.trim().to_owned();
        self.original_instruction = self.original_instruction.trim().to_owned();

        validate_required("project_name", &self.project_name)?;
        validate_required("project_root_path", &self.project_root_path)?;
        validate_required("title", &self.title)?;
        validate_required("original_instruction", &self.original_instruction)?;
        validate_length("title", &self.title, 120)?;
        validate_length("original_instruction", &self.original_instruction, 20_000)?;
        validate_credential("title", &self.title)?;
        validate_credential("original_instruction", &self.original_instruction)?;

        Ok(self)
    }
}

impl TandemTask {
    pub fn new(id: String, project_id: String, input: NewTask, now: TimestampMs) -> Self {
        Self {
            id,
            project_id,
            title: input.title,
            original_instruction: input.original_instruction,
            status: TaskStatus::Active,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    pub fn mark_needs_attention(&mut self, now: TimestampMs) {
        self.set_status(TaskStatus::NeedsAttention, now);
    }

    pub fn mark_awaiting_acceptance(&mut self, now: TimestampMs) {
        self.set_status(TaskStatus::AwaitingAcceptance, now);
    }

    pub fn pause(&mut self, now: TimestampMs) {
        self.set_status(TaskStatus::Paused, now);
    }

    pub fn resume(&mut self, now: TimestampMs) {
        self.set_status(TaskStatus::Active, now);
    }

    pub fn confirm_completed(&mut self, now: TimestampMs) -> Result<(), TandemDomainError> {
        if self.status == TaskStatus::Completed {
            return Err(TandemDomainError::AlreadyCompleted);
        }

        self.status = TaskStatus::Completed;
        self.updated_at = now;
        self.completed_at = Some(now);
        Ok(())
    }

    fn set_status(&mut self, status: TaskStatus, now: TimestampMs) {
        if self.status == TaskStatus::Completed {
            return;
        }

        self.status = status;
        self.updated_at = now;
    }
}

pub fn detect_structured_credential(value: &str) -> Option<CredentialKind> {
    if contains_private_key_header(value) {
        return Some(CredentialKind::PrivateKey);
    }

    if contains_token(value, "sk_live_", 24)
        || contains_token(value, "sk-", 20)
        || contains_token(value, "ghp_", 36)
        || contains_token(value, "github_pat_", 82)
        || [b'b', b'a', b'p', b'r', b's']
            .iter()
            .any(|kind| contains_token(value, &format!("xox{}-", *kind as char), 10))
        || contains_token(value, "AKIA", 16)
    {
        return Some(CredentialKind::ApiToken);
    }

    contains_named_secret(value).then_some(CredentialKind::NamedSecret)
}

fn validate_required(field: &'static str, value: &str) -> Result<(), TandemDomainError> {
    if value.is_empty() {
        Err(TandemDomainError::EmptyField { field })
    } else {
        Ok(())
    }
}

fn validate_length(field: &'static str, value: &str, max: usize) -> Result<(), TandemDomainError> {
    if value.chars().count() > max {
        Err(TandemDomainError::TooLong { field, max })
    } else {
        Ok(())
    }
}

fn validate_credential(field: &'static str, value: &str) -> Result<(), TandemDomainError> {
    match detect_structured_credential(value) {
        Some(kind) => Err(TandemDomainError::StructuredCredential { field, kind }),
        None => Ok(()),
    }
}

fn contains_private_key_header(value: &str) -> bool {
    value.lines().any(|line| {
        let line = line.trim();
        line.starts_with("-----BEGIN ")
            && line.ends_with("PRIVATE KEY-----")
            && line["-----BEGIN ".len()..line.len() - "PRIVATE KEY-----".len()]
                .chars()
                .all(|character| character.is_ascii_uppercase() || character == ' ')
    })
}

fn contains_token(value: &str, prefix: &str, minimum_suffix_len: usize) -> bool {
    value.match_indices(prefix).any(|(start, _)| {
        let starts_at_boundary = value[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !(character.is_alphanumeric() || character == '_'));
        if !starts_at_boundary {
            return false;
        }

        let suffix = &value[start + prefix.len()..];
        suffix
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .count()
            >= minimum_suffix_len
    })
}

fn contains_named_secret(value: &str) -> bool {
    const NAMES: [&str; 5] = ["api_key", "apikey", "token", "secret", "password"];
    let lowercase = value.to_ascii_lowercase();

    NAMES.iter().any(|name| {
        lowercase.match_indices(name).any(|(start, _)| {
            let before_is_identifier = lowercase[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_alphanumeric() || character == '_');
            if before_is_identifier {
                return false;
            }

            let remainder = lowercase[start + name.len()..].trim_start();
            let Some(remainder) = remainder.strip_prefix([':', '=']) else {
                return false;
            };
            remainder
                .trim_start()
                .chars()
                .take_while(|character| !character.is_whitespace())
                .count()
                >= 12
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_input() -> NewTask {
        NewTask {
            project_name: " Project ".into(),
            project_root_path: " /tmp/project ".into(),
            title: " Build feature ".into(),
            original_instruction: " Implement the requested behavior. ".into(),
        }
    }

    #[test]
    fn validate_trims_all_values() {
        let input = valid_input().validate().unwrap();

        assert_eq!(input.project_name, "Project");
        assert_eq!(input.project_root_path, "/tmp/project");
        assert_eq!(input.title, "Build feature");
        assert_eq!(
            input.original_instruction,
            "Implement the requested behavior."
        );
    }

    #[test]
    fn validate_rejects_each_empty_field_by_name() {
        for (field, mut input) in [
            ("project_name", valid_input()),
            ("project_root_path", valid_input()),
            ("title", valid_input()),
            ("original_instruction", valid_input()),
        ] {
            match field {
                "project_name" => input.project_name = "  ".into(),
                "project_root_path" => input.project_root_path = "  ".into(),
                "title" => input.title = "  ".into(),
                "original_instruction" => input.original_instruction = "  ".into(),
                _ => unreachable!(),
            }

            assert_eq!(
                input.validate(),
                Err(TandemDomainError::EmptyField { field })
            );
        }
    }

    #[test]
    fn validate_rejects_values_over_character_limits() {
        let mut title = valid_input();
        title.title = "x".repeat(121);
        assert_eq!(
            title.validate(),
            Err(TandemDomainError::TooLong {
                field: "title",
                max: 120,
            })
        );

        let mut instruction = valid_input();
        instruction.original_instruction = "x".repeat(20_001);
        assert_eq!(
            instruction.validate(),
            Err(TandemDomainError::TooLong {
                field: "original_instruction",
                max: 20_000,
            })
        );
    }

    #[test]
    fn validate_counts_unicode_scalar_values_not_bytes() {
        let mut input = valid_input();
        input.title = "é".repeat(120);
        input.original_instruction = "界".repeat(20_000);

        assert!(input.validate().is_ok());
    }

    #[test]
    fn detects_each_fixed_structured_credential_form_without_echoing_it() {
        let cases = [
            ("-----BEGIN PRIVATE KEY-----", CredentialKind::PrivateKey),
            (
                "-----BEGIN RSA PRIVATE KEY-----",
                CredentialKind::PrivateKey,
            ),
            ("sk-12345678901234567890", CredentialKind::ApiToken),
            (concat!("sk_", "live_123456789012345678901234"), CredentialKind::ApiToken),
            (
                "ghp_123456789012345678901234567890123456",
                CredentialKind::ApiToken,
            ),
            (
                "github_pat_1234567890123456789012345678901234567890123456789012345678901234567890123456789012",
                CredentialKind::ApiToken,
            ),
            ("xoxb-1234567890", CredentialKind::ApiToken),
            ("xoxa-1234567890", CredentialKind::ApiToken),
            ("xoxp-1234567890", CredentialKind::ApiToken),
            ("xoxr-1234567890", CredentialKind::ApiToken),
            ("xoxs-1234567890", CredentialKind::ApiToken),
            ("AKIA1234567890123456", CredentialKind::ApiToken),
            ("API_KEY=123456789012", CredentialKind::NamedSecret),
            ("apikey:123456789012", CredentialKind::NamedSecret),
            ("Token = 123456789012", CredentialKind::NamedSecret),
            ("secret: 123456789012", CredentialKind::NamedSecret),
            ("PASSWORD=123456789012", CredentialKind::NamedSecret),
        ];

        for (value, expected_kind) in cases {
            assert_eq!(detect_structured_credential(value), Some(expected_kind));

            let mut input = valid_input();
            input.original_instruction = value.into();
            let error = input.validate().unwrap_err();
            assert_eq!(
                error,
                TandemDomainError::StructuredCredential {
                    field: "original_instruction",
                    kind: expected_kind,
                }
            );
            assert!(!error.to_string().contains(value));
        }
    }

    #[test]
    fn structured_credential_near_miss_prose_remains_valid() {
        for value in [
            "Rotate the api key before release.",
            "The token: should be stored outside this task.",
            "password = use the team vault",
            "Discuss sk- prefixes without including a token.",
            "AKIA is an AWS access-key prefix.",
            "sk-1234567890123456789",
            concat!("sk_", "live_12345678901234567890123"),
            "ghp_12345678901234567890123456789012345",
            "github_pat_123456789012345678901234567890123456789012345678901234567890123456789012345678901",
            "xoxb-123456789",
            "AKIA123456789012345",
            "api_key=12345678901",
            "ésk-12345678901234567890",
            "épassword=123456789012",
            "prefixsk-12345678901234567890",
            "prefixAKIA1234567890123456",
        ] {
            let mut input = valid_input();
            input.original_instruction = value.into();
            assert!(
                input.validate().is_ok(),
                "near-miss prose must remain valid"
            );
        }
    }

    #[test]
    fn new_task_starts_active_with_equal_timestamps_and_no_completion() {
        let task = TandemTask::new("task-1".into(), "project-1".into(), valid_input(), 1_000);

        assert_eq!(task.status, TaskStatus::Active);
        assert_eq!(task.created_at, 1_000);
        assert_eq!(task.updated_at, 1_000);
        assert_eq!(task.completed_at, None);
    }

    #[test]
    fn non_completion_transitions_set_exact_states_and_timestamps() {
        let mut task = TandemTask::new("task-1".into(), "project-1".into(), valid_input(), 1_000);

        task.mark_needs_attention(2_000);
        assert_eq!(
            (task.status, task.updated_at),
            (TaskStatus::NeedsAttention, 2_000)
        );
        assert_eq!(task.completed_at, None);

        task.mark_awaiting_acceptance(3_000);
        assert_eq!(
            (task.status, task.updated_at),
            (TaskStatus::AwaitingAcceptance, 3_000)
        );
        assert_eq!(task.completed_at, None);

        task.pause(4_000);
        assert_eq!((task.status, task.updated_at), (TaskStatus::Paused, 4_000));
        assert_eq!(task.completed_at, None);

        task.resume(5_000);
        assert_eq!((task.status, task.updated_at), (TaskStatus::Active, 5_000));
        assert_eq!(task.completed_at, None);
    }

    #[test]
    fn only_confirmation_sets_completed_and_completion_timestamp() {
        let mut task = TandemTask::new("task-1".into(), "project-1".into(), valid_input(), 1_000);

        task.confirm_completed(6_000).unwrap();

        assert_eq!(task.status, TaskStatus::Completed);
        assert_eq!(task.updated_at, 6_000);
        assert_eq!(task.completed_at, Some(6_000));
    }

    #[test]
    fn repeated_confirmation_preserves_first_completion_timestamp() {
        let mut task = TandemTask::new("task-1".into(), "project-1".into(), valid_input(), 1_000);
        task.confirm_completed(6_000).unwrap();

        assert_eq!(
            task.confirm_completed(7_000),
            Err(TandemDomainError::AlreadyCompleted)
        );
        assert_eq!(task.updated_at, 6_000);
        assert_eq!(task.completed_at, Some(6_000));
    }

    #[test]
    fn completed_task_is_terminal_and_preserves_first_completion_timestamp() {
        let transitions: [fn(&mut TandemTask, TimestampMs); 4] = [
            TandemTask::mark_needs_attention,
            TandemTask::mark_awaiting_acceptance,
            TandemTask::pause,
            TandemTask::resume,
        ];

        for transition in transitions {
            let mut task =
                TandemTask::new("task-1".into(), "project-1".into(), valid_input(), 1_000);
            task.confirm_completed(6_000).unwrap();

            transition(&mut task, 7_000);

            assert_eq!(task.status, TaskStatus::Completed);
            assert_eq!(task.updated_at, 6_000);
            assert_eq!(task.completed_at, Some(6_000));
            assert_eq!(
                task.confirm_completed(8_000),
                Err(TandemDomainError::AlreadyCompleted)
            );
            assert_eq!(task.updated_at, 6_000);
            assert_eq!(task.completed_at, Some(6_000));
        }
    }

    #[test]
    fn enum_serde_names_are_stable_snake_case() {
        let task_statuses = [
            (TaskStatus::Active, "active"),
            (TaskStatus::NeedsAttention, "needs_attention"),
            (TaskStatus::AwaitingAcceptance, "awaiting_acceptance"),
            (TaskStatus::Paused, "paused"),
            (TaskStatus::Completed, "completed"),
        ];
        let run_statuses = [
            (RunStatus::Active, "active"),
            (RunStatus::AwaitingUser, "awaiting_user"),
            (RunStatus::AwaitingAcceptance, "awaiting_acceptance"),
            (RunStatus::Paused, "paused"),
            (RunStatus::Ended, "ended"),
        ];
        let agents = [
            (AgentKind::Cursor, "cursor"),
            (AgentKind::Codex, "codex"),
            (AgentKind::Claude, "claude"),
        ];

        for (value, expected) in task_statuses {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in run_statuses {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
        for (value, expected) in agents {
            assert_eq!(serde_json::to_value(value).unwrap(), expected);
        }
    }
}
