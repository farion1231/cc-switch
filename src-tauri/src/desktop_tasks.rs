//! Local Desktop scheduled-task inventory and explicit cross-profile recovery.
use crate::desktop_data::{
    component, discover, error, field, live_roots, parse, profile_dirs, Dir, ProfileKey,
};
use crate::desktop_recovery::{ensure_stopped, write, RECOVERY_LOCK};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    process::Command,
};
const REGISTRY_LIMIT: u64 = 16 * 1024 * 1024;
const PROMPT_LIMIT: u64 = 1024 * 1024;
const REGISTRY: &str = "scheduled-tasks.json";
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTask {
    pub id: String,
    pub title: String,
    pub enabled: bool,
    pub schedule: TaskSchedule,
    pub cwd: String,
    pub prompt_available: bool,
    pub problem: Option<Problem>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(not(unix), allow(dead_code))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskSchedule {
    Cron { expression: String },
    Once { at: i64 },
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    MissingPrompt,
    MissingDirectory,
    UnsupportedTask,
    Expired,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub key: ProfileKey,
    pub last_known_account: bool,
    pub tasks: Vec<DesktopTask>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub profiles: Vec<Profile>,
    pub orphan_prompts: Vec<String>,
    pub warnings: Vec<String>,
    pub writes_supported: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub source: ProfileKey,
    pub target: ProfileKey,
    pub task_ids: Vec<String>,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Disposition {
    Ready,
    Exists,
    Blocked { reason: Problem },
}
#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub disposition: Disposition,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub token: String,
    pub items: Vec<Item>,
    pub can_apply: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub restored_tasks: Vec<String>,
    pub skipped: Vec<String>,
    pub backup_path: Option<String>,
    pub error: Option<String>,
}
struct Prepared {
    preview: Preview,
    target: Dir,
    registry: Option<Vec<u8>>,
    tasks: Vec<Value>,
}
pub(crate) struct Store {
    roots: [PathBuf; 2],
    tasks: PathBuf,
}
impl Store {
    pub fn live() -> Result<Self, String> {
        Ok(Self {
            roots: live_roots()?,
            tasks: crate::config::get_claude_config_dir().join("scheduled-tasks"),
        })
    }
    pub fn inventory(&self) -> Result<Inventory, String> {
        let (locations, mut warnings) = discover(&self.roots)?;
        let mut complete = warnings.is_empty();
        let mut referenced = BTreeSet::new();
        let mut profiles = vec![];
        for location in locations {
            let mut profile = Profile {
                key: location.key,
                last_known_account: location.last_known_account,
                tasks: vec![],
                warnings: vec![],
            };
            let records = (|| -> Result<Vec<Value>, String> {
                let (_, dir) = profile_dirs(&self.roots, &profile.key)?;
                task_records(
                    dir.optional(REGISTRY, REGISTRY_LIMIT)
                        .map_err(error)?
                        .as_deref(),
                )
            })();
            match records {
                Ok(records) => {
                    for task in records {
                        let id = field(&task, "id").to_owned();
                        referenced.insert(id.clone());
                        let prompt_available = self.prompt(&id).is_ok();
                        let schedule = match (
                            task.get("cronExpression").and_then(Value::as_str),
                            task.get("fireAt").and_then(Value::as_i64),
                        ) {
                            (Some(expression), _) => TaskSchedule::Cron {
                                expression: expression.into(),
                            },
                            (_, Some(at)) => TaskSchedule::Once { at },
                            _ => TaskSchedule::Manual,
                        };
                        profile.tasks.push(DesktopTask {
                            id,
                            title: field(&task, "displayName").into(),
                            enabled: task["enabled"] == true,
                            schedule,
                            cwd: field(&task, "cwd").into(),
                            prompt_available,
                            problem: task_problem(
                                &task,
                                prompt_available,
                                Utc::now().timestamp_millis(),
                            ),
                        });
                    }
                }
                Err(e) => {
                    complete = false;
                    profile.warnings.push(e);
                }
            }
            profiles.push(profile);
        }
        let mut orphan_prompts = vec![];
        if complete {
            match Dir::open(&self.tasks) {
                Ok(root) => {
                    for id in root.entries().map_err(error)? {
                        if component(&id).is_ok()
                            && !referenced.contains(&id)
                            && self.prompt(&id).is_ok()
                        {
                            orphan_prompts.push(id);
                        }
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => warnings.push(error(e)),
            }
        } else {
            warnings.push("Some profiles could not be read; unreferenced prompts cannot be classified reliably.".into());
        }
        Ok(Inventory {
            profiles,
            orphan_prompts,
            warnings,
            writes_supported: cfg!(target_os = "macos"),
        })
    }
    pub fn prompt(&self, id: &str) -> Result<String, String> {
        component(id)?;
        let bytes = Dir::open(&self.tasks)
            .map_err(error)?
            .child(id)
            .map_err(error)?
            .read("SKILL.md", PROMPT_LIMIT)
            .map_err(error)?;
        let text = String::from_utf8(bytes).map_err(error)?;
        if text.trim().is_empty() {
            return Err("The saved task prompt is empty".into());
        }
        Ok(text)
    }

    pub fn preview(&self, request: &Request) -> Result<Preview, String> {
        Ok(self
            .prepare(request, Utc::now().timestamp_millis())?
            .preview)
    }
    fn prepare(&self, request: &Request, now: i64) -> Result<Prepared, String> {
        if request.source == request.target {
            return Err("Choose a different target account or profile".into());
        }
        if request.task_ids.len() > 10_000 {
            return Err("Too many selected tasks".into());
        }
        let mut seen = BTreeSet::new();
        for id in &request.task_ids {
            component(id)?;
            if !seen.insert(id) {
                return Err("Duplicate selected identifier".into());
            }
        }
        let (_, source) = profile_dirs(&self.roots, &request.source)?;
        let (_, target) = profile_dirs(&self.roots, &request.target)?;
        let source_registry = source.optional(REGISTRY, REGISTRY_LIMIT).map_err(error)?;
        let registry = target.optional(REGISTRY, REGISTRY_LIMIT).map_err(error)?;
        let source_tasks = task_records(source_registry.as_deref())?;
        let target_tasks = task_records(registry.as_deref())?;
        let mut digest = Sha256::new();
        digest.update(serde_json::to_vec(&(request, &source_registry, &registry)).map_err(error)?);
        let mut items = vec![];
        let mut tasks = vec![];
        for id in &request.task_ids {
            let task = source_tasks
                .iter()
                .find(|t| field(t, "id") == id)
                .ok_or("Selected source task disappeared; refresh the inventory")?;
            let prompt = self.prompt(id);
            if let Ok(text) = &prompt {
                digest.update(text.as_bytes());
            }
            let disposition = if target_tasks.iter().any(|t| field(t, "id") == id) {
                Disposition::Exists
            } else if let Some(reason) = task_problem(task, prompt.is_ok(), now) {
                Disposition::Blocked { reason }
            } else {
                tasks.push(task.clone());
                Disposition::Ready
            };
            items.push(Item {
                id: id.clone(),
                disposition,
            });
        }
        let can_apply = items
            .iter()
            .any(|i| matches!(i.disposition, Disposition::Ready))
            && !items
                .iter()
                .any(|i| matches!(i.disposition, Disposition::Blocked { .. }));
        Ok(Prepared {
            preview: Preview {
                token: format!("{:x}", digest.finalize()),
                items,
                can_apply,
            },
            target,
            registry,
            tasks,
        })
    }
    fn apply_at(
        &self,
        request: &Request,
        token: &str,
        now: i64,
        check_stopped: impl Fn() -> Result<(), String>,
    ) -> Result<Receipt, String> {
        check_stopped()?;
        let prepared = self.prepare(request, now)?;
        if prepared.preview.token != token {
            return Err("Source, target, or prompt changed; preview again".into());
        }
        if !prepared.preview.can_apply {
            return Err("No recoverable selection, or a selected entry is blocked".into());
        }
        let mut receipt = Receipt {
            restored_tasks: vec![],
            skipped: prepared
                .preview
                .items
                .iter()
                .filter(|i| matches!(i.disposition, Disposition::Exists))
                .map(|i| i.id.clone())
                .collect(),
            backup_path: None,
            error: None,
        };
        let result = (|| -> Result<(), String> {
            check_stopped()?;
            let mut registry = match &prepared.registry {
                Some(bytes) => parse(bytes)?,
                None => {
                    json!({"scheduledTasks":[],"sundayAliasBoundaryStamped":true,"dayFieldsOrBoundaryStamped":true})
                }
            };
            let entries = registry
                .get_mut("scheduledTasks")
                .and_then(Value::as_array_mut)
                .ok_or("Invalid target registry")?;
            let mut restored = vec![];
            for task in prepared.tasks {
                let id = field(&task, "id").to_owned();
                entries.push(recovered_task(task, &self.tasks, now)?);
                restored.push(id);
            }
            if let Some(retries) = registry
                .get_mut("runRetries")
                .and_then(Value::as_object_mut)
            {
                for id in &restored {
                    retries.remove(id);
                }
            }
            let bytes = serde_json::to_vec_pretty(&registry).map_err(error)?;
            if let Some(before) = &prepared.registry {
                let backup = format!(
                    ".cc-switch-scheduled-tasks-backup-{}.json",
                    uuid::Uuid::new_v4()
                );
                let result = write(&prepared.target, &backup, before, None);
                if result.is_ok() || result.as_ref().is_err_and(|e| e.published) {
                    receipt.backup_path = Some(
                        prepared
                            .target
                            .path
                            .join(&backup)
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
                result.map_err(error)?;
            }
            check_stopped()?;
            let result = write(
                &prepared.target,
                REGISTRY,
                &bytes,
                prepared.registry.as_deref(),
            );
            if result.is_ok() || result.as_ref().is_err_and(|e| e.published) {
                receipt.restored_tasks = restored;
            }
            // An exclusive destination appearing between preview and publication is a conflict,
            // not a reason to overwrite another program's newly saved registry.
            result.map_err(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    "Desktop registry appeared; preview again".into()
                } else {
                    error(e)
                }
            })?;
            if prepared
                .target
                .read(REGISTRY, REGISTRY_LIMIT)
                .map_err(error)?
                != bytes
            {
                return Err("Task registry read-back did not match".into());
            }
            Ok(())
        })();
        receipt.error = result.err();
        Ok(receipt)
    }
}
fn task_records(bytes: Option<&[u8]>) -> Result<Vec<Value>, String> {
    let Some(bytes) = bytes else {
        return Ok(vec![]);
    };
    let value = parse(bytes)?;
    let tasks = value
        .get("scheduledTasks")
        .and_then(Value::as_array)
        .ok_or("Unrecognized Desktop scheduled-task registry")?;
    let mut ids = BTreeSet::new();
    for task in tasks {
        let id = field(task, "id");
        component(id)?;
        if !ids.insert(id) || !valid_task_shape(task) {
            return Err("Duplicate task identifier or unrecognized scheduled-task fields".into());
        }
    }
    Ok(tasks.clone())
}

fn valid_task_shape(task: &Value) -> bool {
    if !task.get("enabled").is_some_and(Value::is_boolean)
        || task
            .get("createdAt")
            .and_then(Value::as_i64)
            .is_none_or(|n| n < 0)
        || !task.get("filePath").is_some_and(Value::is_string)
    {
        return false;
    }
    for key in [
        "displayName",
        "model",
        "cwd",
        "sourceBranch",
        "cronExpression",
        "notifySessionId",
        "permissionMode",
        "chromePermissionMode",
    ] {
        if task.get(key).is_some_and(|v| !v.is_string()) {
            return false;
        }
    }
    for key in ["lastRunAt", "lastScheduledFor", "missedRunScanFloor"] {
        if task.get(key).is_some_and(|v| {
            v.as_str()
                .is_none_or(|s| DateTime::parse_from_rfc3339(s).is_err())
        }) {
            return false;
        }
    }
    for key in [
        "useWorktree",
        "disableJitter",
        "dispatchSubscribed",
        "migratedFromRemote",
    ] {
        if task.get(key).is_some_and(|v| !v.is_boolean()) {
            return false;
        }
    }
    for key in [
        "userSelectedFolders",
        "userSelectedFiles",
        "userSelectedProjectUuids",
        "chromeAllowedDomains",
    ] {
        if task.get(key).is_some_and(|v| {
            v.as_array()
                .is_none_or(|a| a.iter().any(|v| !v.is_string()))
        }) {
            return false;
        }
    }
    if task.get("fireAt").is_some_and(|v| v.as_i64().is_none()) {
        return false;
    }
    if task
        .get("permissionMode")
        .and_then(Value::as_str)
        .is_some_and(|v| {
            ![
                "default",
                "acceptEdits",
                "plan",
                "bypassPermissions",
                "dontAsk",
                "auto",
            ]
            .contains(&v)
        })
    {
        return false;
    }
    if task
        .get("chromePermissionMode")
        .and_then(Value::as_str)
        .is_some_and(|v| !["ask", "skip_all_permission_checks", "follow_a_plan"].contains(&v))
    {
        return false;
    }
    if task.get("approvedPermissions").is_some_and(|v| {
        v.as_array().is_none_or(|a| {
            a.iter().any(|rule| {
                !rule.get("toolName").is_some_and(Value::is_string)
                    || rule.get("ruleContent").is_some_and(|v| !v.is_string())
            })
        })
    }) {
        return false;
    }
    true
}

fn valid_cron(value: &str) -> bool {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    fields
        .iter()
        .zip([(0, 59), (0, 23), (1, 31), (1, 12), (0, 6)])
        .all(|(field, (min, max))| {
            field.split(',').all(|part| {
                let (range, step) = match part.split_once('/') {
                    Some((a, b)) => (a, Some(b)),
                    None => (part, None),
                };
                if step.is_some_and(|v| {
                    v.parse::<u32>()
                        .map_or(true, |n| n == 0 || n > max - min + 1)
                }) {
                    return false;
                }
                let number = |s: &str| s.parse::<u32>().ok().filter(|n| *n >= min && *n <= max);
                if range == "*" {
                    true
                } else if let Some((a, b)) = range.split_once('-') {
                    matches!((number(a),number(b)), (Some(a),Some(b)) if a <= b)
                } else {
                    number(range).is_some()
                }
            })
        })
}

fn task_problem(task: &Value, prompt_available: bool, now: i64) -> Option<Problem> {
    if !prompt_available {
        return Some(Problem::MissingPrompt);
    }
    if !task.get("createdAt").is_some_and(Value::is_number)
        || ["watcher", "migratedToRemoteAt", "spaceId"]
            .iter()
            .any(|k| task.get(*k).is_some_and(|v| !v.is_null()))
    {
        return Some(Problem::UnsupportedTask);
    }
    let cron = task.get("cronExpression");
    let fire = task.get("fireAt");
    if cron.is_some() && fire.is_some() {
        return Some(Problem::UnsupportedTask);
    }
    if let Some(cron) = cron {
        if !cron.as_str().is_some_and(valid_cron) {
            return Some(Problem::UnsupportedTask);
        }
    }
    if let Some(fire) = fire {
        match fire.as_i64() {
            Some(at) if at > now => {}
            Some(_) => return Some(Problem::Expired),
            None => return Some(Problem::UnsupportedTask),
        }
    }
    let cwd = Path::new(field(task, "cwd"));
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Some(Problem::MissingDirectory);
    }
    None
}

fn recovered_task(mut task: Value, tasks_root: &Path, now: i64) -> Result<Value, String> {
    let id = field(&task, "id").to_owned();
    let object = task.as_object_mut().ok_or("Invalid task")?;
    object.insert(
        "filePath".into(),
        tasks_root
            .join(&id)
            .join("SKILL.md")
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    if object.contains_key("cronExpression") {
        let previous_floor = object
            .get("missedRunScanFloor")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis())
            .unwrap_or(now);
        let floor = DateTime::<Utc>::from_timestamp_millis(now.max(previous_floor))
            .ok_or("Invalid recovery time")?;
        object.insert("missedRunScanFloor".into(), floor.to_rfc3339().into());
    }
    // Notifications/subscriptions refer to a session in the source account.
    object.remove("notifySessionId");
    object.remove("dispatchSubscribed");
    Ok(task)
}

pub(crate) fn apply_live(request: Request, token: String) -> Result<Receipt, String> {
    let _lock = RECOVERY_LOCK
        .lock()
        .map_err(|_| "Desktop recovery lock is unavailable")?;
    ensure_stopped()?;
    if !request.task_ids.is_empty() {
        // missedRunScanFloor is an internal format. Do not silently enable schedules
        // on an older/unknown build that might ignore this recovery-time boundary.
        let output = Command::new("/usr/libexec/PlistBuddy")
            .args([
                "-c",
                "Print :CFBundleShortVersionString",
                "/Applications/Claude.app/Contents/Info.plist",
            ])
            .output()
            .map_err(error)?;
        if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != "1.46388.4"
        {
            return Err("Scheduled-task recovery requires the verified Claude Desktop build 1.46388.4; inventory remains available".into());
        }
    }
    Store::live()?.apply_at(
        &request,
        &token,
        Utc::now().timestamp_millis(),
        ensure_stopped,
    )
}

#[cfg(test)]
#[path = "desktop_tasks_tests.rs"]
mod tests;
