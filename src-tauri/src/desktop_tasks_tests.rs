use super::*;
use crate::desktop_data::{root, RootKind};
use std::fs;
use std::os::unix::fs::symlink;
use tempfile::TempDir;
const NOW: i64 = 1_800_000_000_000;
impl Store {
    fn root(&self, kind: RootKind) -> &std::path::Path {
        root(&self.roots, kind)
    }
}
struct Fixture {
    temp: TempDir,
    store: Store,
    source: ProfileKey,
    target: ProfileKey,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let store = Store {
            roots: [temp.path().join("Claude"), temp.path().join("Claude-3p")],
            tasks: temp.path().join("scheduled-tasks"),
        };
        let source = ProfileKey {
            root: RootKind::Default,
            account: "account-a".into(),
            org: "org-a".into(),
        };
        let target = ProfileKey {
            root: RootKind::Managed,
            account: "account-b".into(),
            org: "org-b".into(),
        };
        for key in [&source, &target] {
            fs::create_dir_all(
                store
                    .root(key.root)
                    .join("claude-code-sessions")
                    .join(&key.account)
                    .join(&key.org),
            )
            .unwrap();
        }
        Self {
            temp,
            store,
            source,
            target,
        }
    }
    fn dir(&self, key: &ProfileKey) -> PathBuf {
        self.store
            .root(key.root)
            .join("claude-code-sessions")
            .join(&key.account)
            .join(&key.org)
    }
    fn task(&self, id: &str, enabled: bool) -> Value {
        fs::create_dir_all(self.store.tasks.join(id)).unwrap();
        fs::write(
            self.store.tasks.join(id).join("SKILL.md"),
            "---\nname: fixture\n---\nInspect fictional fixtures.\n",
        )
        .unwrap();
        json!({"id":id,"displayName":"Fictional task","enabled":enabled,"cronExpression":"0 9 * * *","filePath":"/old/recorded/path/SKILL.md","cwd":self.temp.path(),"createdAt":NOW-86_400_000,"lastRunAt":"2026-11-01T09:00:00Z","lastScheduledFor":"2026-11-01T09:00:00Z","permissionMode":"default","unknownFutureSetting":{"keep":true}})
    }
    fn registry(&self, key: &ProfileKey, tasks: Vec<Value>) -> Vec<u8> {
        let bytes=serde_json::to_vec_pretty(&json!({"scheduledTasks":tasks,"recordedSkips":{"untouched":[{"at":1,"reason":"sleep"}]},"sundayAliasBoundaryStamped":true,"dayFieldsOrBoundaryStamped":true,"unknownRootSetting":42})).unwrap();
        fs::write(self.dir(key).join(REGISTRY), &bytes).unwrap();
        bytes
    }
    fn request(&self, _sessions: &[&str], tasks: &[&str]) -> Request {
        Request {
            source: self.source.clone(),
            target: self.target.clone(),
            task_ids: tasks.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn apply(&self, request: &Request) -> Receipt {
        let preview = self.store.prepare(request, NOW).unwrap().preview;
        self.store
            .apply_at(request, &preview.token, NOW, || Ok(()))
            .unwrap()
    }
}

#[test]
fn recovery_preserves_enabled_state_and_last_run_but_sets_the_catchup_floor() {
    let f = Fixture::new();
    let active = f.task("active", true);
    let paused = f.task("paused", false);
    let existing = f.task("existing", true);
    let source_before = f.registry(&f.source, vec![active.clone(), paused.clone()]);
    let target_before = f.registry(&f.target, vec![existing.clone()]);
    let prompt_before = fs::read(f.store.tasks.join("active/SKILL.md")).unwrap();
    let request = f.request(&[], &["active", "paused"]);
    let receipt = f.apply(&request);
    assert!(receipt.error.is_none());
    assert_eq!(receipt.restored_tasks, ["active", "paused"]);
    assert_eq!(
        fs::read(receipt.backup_path.unwrap()).unwrap(),
        target_before
    );
    assert_eq!(
        fs::read(f.dir(&f.source).join(REGISTRY)).unwrap(),
        source_before
    );
    assert_eq!(
        fs::read(f.store.tasks.join("active/SKILL.md")).unwrap(),
        prompt_before
    );
    let target = parse(&fs::read(f.dir(&f.target).join(REGISTRY)).unwrap()).unwrap();
    assert_eq!(target["scheduledTasks"][0], existing);
    assert_eq!(target["unknownRootSetting"], 42);
    for (i, original) in [active, paused].into_iter().enumerate() {
        let recovered = &target["scheduledTasks"][i + 1];
        assert_eq!(recovered["enabled"], original["enabled"]);
        assert_eq!(recovered["lastRunAt"], original["lastRunAt"]);
        assert_eq!(recovered["lastScheduledFor"], original["lastScheduledFor"]);
        assert_eq!(
            recovered["unknownFutureSetting"],
            original["unknownFutureSetting"]
        );
        assert_eq!(
            DateTime::parse_from_rfc3339(recovered["missedRunScanFloor"].as_str().unwrap())
                .unwrap()
                .timestamp_millis(),
            NOW
        );
    }
    assert!(!f.store.prepare(&request, NOW).unwrap().preview.can_apply);
}

#[test]
fn recovery_discards_pending_retries_only_for_restored_tasks() {
    let f = Fixture::new();
    let mut task = f.task("restored", true);
    task["notifySessionId"] = "source-session".into();
    task["dispatchSubscribed"] = true.into();
    let future_floor = DateTime::<Utc>::from_timestamp_millis(NOW + 60_000)
        .unwrap()
        .to_rfc3339();
    task["missedRunScanFloor"] = future_floor.clone().into();
    f.registry(&f.source, vec![task]);
    let existing = f.task("existing", true);
    let mut target = parse(&f.registry(&f.target, vec![existing])).unwrap();
    let pending = json!({"slot":"2026-11-01T09:00:00Z", "attempts":1, "notBefore":null});
    target["runRetries"] = json!({"restored":pending, "existing":pending});
    fs::write(
        f.dir(&f.target).join(REGISTRY),
        serde_json::to_vec(&target).unwrap(),
    )
    .unwrap();
    let receipt = f.apply(&f.request(&[], &["restored"]));
    assert!(receipt.error.is_none());
    let after = parse(&fs::read(f.dir(&f.target).join(REGISTRY)).unwrap()).unwrap();
    assert_eq!(after["runRetries"], json!({"existing":pending}));
    assert_eq!(after["recordedSkips"], target["recordedSkips"]);
    let restored = &after["scheduledTasks"][1];
    assert_eq!(restored["missedRunScanFloor"], future_floor);
    assert!(restored.get("notifySessionId").is_none());
    assert!(restored.get("dispatchSubscribed").is_none());
}

#[test]
fn overdue_one_time_tasks_cannot_be_restored_even_if_previously_paused() {
    let f = Fixture::new();
    for enabled in [false, true] {
        let mut task = f.task("once", enabled);
        task.as_object_mut().unwrap().remove("cronExpression");
        task["fireAt"] = (NOW - 1).into();
        f.registry(&f.source, vec![task]);
        let preview = f
            .store
            .prepare(&f.request(&[], &["once"]), NOW)
            .unwrap()
            .preview;
        assert!(matches!(
            preview.items[0].disposition,
            Disposition::Blocked {
                reason: Problem::Expired
            }
        ));
        assert!(!preview.can_apply);
    }
}

#[test]
fn future_one_time_and_manual_tasks_keep_their_schedules() {
    let f = Fixture::new();
    let mut once = f.task("once", true);
    once.as_object_mut().unwrap().remove("cronExpression");
    once["fireAt"] = (NOW + 3600_000).into();
    let mut manual = f.task("manual", false);
    manual.as_object_mut().unwrap().remove("cronExpression");
    f.registry(&f.source, vec![once, manual]);
    let receipt = f.apply(&f.request(&[], &["once", "manual"]));
    assert!(receipt.error.is_none());
    let tasks = task_records(Some(&fs::read(f.dir(&f.target).join(REGISTRY)).unwrap())).unwrap();
    assert_eq!(tasks[0]["fireAt"], NOW + 3600_000);
    assert!(tasks[0].get("missedRunScanFloor").is_none());
    assert_eq!(tasks[1]["enabled"], false);
    assert!(tasks[1].get("fireAt").is_none());
}

#[test]
fn changed_prompt_or_registry_invalidates_preview_without_writes() {
    for mutate_prompt in [true, false] {
        let f = Fixture::new();
        let task = f.task("one", true);
        f.registry(&f.source, vec![task]);
        let request = f.request(&[], &["one"]);
        let token = f.store.prepare(&request, NOW).unwrap().preview.token;
        if mutate_prompt {
            fs::write(f.store.tasks.join("one/SKILL.md"), "Changed fixture").unwrap();
        } else {
            f.registry(&f.target, vec![]);
        }
        assert!(f.store.apply_at(&request, &token, NOW, || Ok(())).is_err());
        assert!(!f.dir(&f.target).read_dir().unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("backup")));
    }
}

#[test]
fn missing_prompts_unsupported_settings_and_invalid_cron_block_recovery() {
    let f = Fixture::new();
    let mut task = f.task("one", true);
    for cron in [
        "garbage",
        "* * * * 7",
        "0 9 * * MON",
        "*/0 * * * *",
        "60 9 * * *",
        "10-1 * * * *",
    ] {
        task["cronExpression"] = cron.into();
        assert_eq!(
            task_problem(&task, true, NOW),
            Some(Problem::UnsupportedTask)
        );
    }
    task["cronExpression"] = "1-10/3 9 * * 1-5".into();
    assert_eq!(task_problem(&task, true, NOW), None);
    assert_eq!(
        task_problem(&task, false, NOW),
        Some(Problem::MissingPrompt)
    );
    task["watcher"] = json!({"scriptPath":"/fixture"});
    assert_eq!(
        task_problem(&task, true, NOW),
        Some(Problem::UnsupportedTask)
    );
}

#[test]
fn task_prompts_cannot_escape_the_selected_root() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("SKILL.md"), "private fixture").unwrap();
    fs::create_dir(&f.store.tasks).unwrap();
    symlink(&outside, f.store.tasks.join("linked")).unwrap();
    assert!(f.store.prompt("linked").is_err());
    assert!(f.store.prompt("../outside").is_err());
}

#[test]
fn duplicate_task_ids_are_not_silently_merged() {
    let f = Fixture::new();
    let task = f.task("one", true);
    f.registry(&f.source, vec![task.clone(), task]);
    assert!(f.store.prepare(&f.request(&[], &["one"]), NOW).is_err());
}

#[test]
fn orphan_prompts_are_inventory_only_and_profiles_stay_separate() {
    let f = Fixture::new();
    let active = f.task("active", true);
    let paused = f.task("paused", false);
    f.task("orphan", true);
    f.registry(&f.source, vec![active]);
    f.registry(&f.target, vec![paused]);
    let result = f.store.inventory().unwrap();
    assert_eq!(result.orphan_prompts, ["orphan"]);
    assert!(result.profiles[0].tasks[0].enabled);
    assert!(!result.profiles[1].tasks[0].enabled);
    assert!(f.store.prepare(&f.request(&[], &["orphan"]), NOW).is_err());
}

#[test]
fn a_running_desktop_blocks_task_publication() {
    let f = Fixture::new();
    let task = f.task("one", true);
    f.registry(&f.source, vec![task]);
    let request = f.request(&[], &["one"]);
    let preview = f.store.prepare(&request, NOW).unwrap().preview;
    assert!(f
        .store
        .apply_at(&request, &preview.token, NOW, || Err(
            "Desktop running".into()
        ))
        .is_err());
    assert!(!f.dir(&f.target).join(REGISTRY).exists());
}
