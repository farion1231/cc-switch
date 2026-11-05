use super::*;
use tempfile::{tempdir, TempDir};

/// 每个测试用独立的内存库和临时目录；目录显式传入服务，不依赖进程级 HOME，
/// 测试可以并行跑。
struct Fixture {
    db: Database,
    temp: TempDir,
    dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempdir().expect("tempdir");
        let dir = temp.path().join(".claude").join("agents");
        Self {
            db: Database::memory().expect("memory db"),
            temp,
            dir,
        }
    }

    fn path(&self, file_name: &str) -> PathBuf {
        self.dir.join(format!("{file_name}.md"))
    }

    fn read(&self, file_name: &str) -> String {
        std::fs::read_to_string(self.path(file_name)).expect("read subagent file")
    }

    fn write_disk(&self, file_name: &str, content: &str) {
        std::fs::create_dir_all(&self.dir).unwrap();
        std::fs::write(self.path(file_name), content).unwrap();
    }

    fn create(&self, content: &str) -> Subagent {
        SubagentService::upsert(&self.db, &self.dir, input(None, None, content, true), false)
            .expect("create subagent")
    }

    fn status(&self, id: &str) -> SubagentStatus {
        SubagentService::list(&self.db, &self.dir)
            .unwrap()
            .into_iter()
            .find(|s| s.id == id)
            .expect("subagent listed")
            .status
    }
}

fn input(id: Option<&str>, file_name: Option<&str>, content: &str, enabled: bool) -> SubagentInput {
    SubagentInput {
        id: id.map(str::to_string),
        file_name: file_name.map(str::to_string),
        content: content.to_string(),
        enabled,
    }
}

fn agent(name: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: Reviews {name} code\n---\n{body}\n")
}

fn assert_err_prefix<T: std::fmt::Debug>(result: Result<T, AppError>, prefix: &str) {
    let err = result.expect_err("expected an error").to_string();
    assert!(
        err.starts_with(prefix),
        "error should start with {prefix}, got: {err}"
    );
}

#[test]
fn create_writes_file_and_reports_synced() {
    let fx = Fixture::new();
    let content = agent("code-reviewer", "You review code.");
    let created = fx.create(&content);

    assert_eq!(created.name, "code-reviewer");
    assert_eq!(created.file_name, "code-reviewer");
    assert_eq!(
        created.description.as_deref(),
        Some("Reviews code-reviewer code")
    );
    assert_eq!(created.source, "local");
    assert_eq!(created.status, SubagentStatus::Synced);
    assert_eq!(fx.read("code-reviewer"), content);
    assert_eq!(PathBuf::from(&created.file_path), fx.path("code-reviewer"));
}

#[test]
fn create_disabled_does_not_write_file() {
    let fx = Fixture::new();
    let created = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(None, Some("custom-file"), &agent("tester", "x"), false),
        false,
    )
    .unwrap();
    assert_eq!(created.file_name, "custom-file");
    assert_eq!(created.status, SubagentStatus::Disabled);
    assert!(!fx.path("custom-file").exists());
}

#[test]
fn update_rewrites_file_and_refreshes_metadata() {
    let fx = Fixture::new();
    let created = fx.create(&agent("helper", "v1"));
    let v2 = "---\nname: helper\ndescription: New description\n---\nv2\n";
    let updated = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(Some(&created.id), None, v2, true),
        false,
    )
    .unwrap();

    assert_eq!(updated.description.as_deref(), Some("New description"));
    assert_eq!(updated.status, SubagentStatus::Synced);
    assert_eq!(fx.read("helper"), v2);
}

#[test]
fn rename_writes_new_file_and_removes_old_one() {
    let fx = Fixture::new();
    let created = fx.create(&agent("old-name", "body"));
    let content = agent("new-name", "body");
    let renamed = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(Some(&created.id), Some("new-name"), &content, true),
        false,
    )
    .unwrap();

    assert_eq!(renamed.file_name, "new-name");
    assert_eq!(renamed.status, SubagentStatus::Synced);
    assert!(!fx.path("old-name").exists());
    assert_eq!(fx.read("new-name"), content);
}

#[test]
fn rename_refuses_when_old_file_drifted_and_touches_nothing() {
    let fx = Fixture::new();
    let created = fx.create(&agent("old-name", "body"));
    fx.write_disk("old-name", "user edit");

    let result = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(
            Some(&created.id),
            Some("new-name"),
            &agent("new-name", "b"),
            true,
        ),
        false,
    );
    assert_err_prefix(result, "SUBAGENT_DRIFTED:");
    assert_eq!(fx.read("old-name"), "user edit");
    assert!(!fx.path("new-name").exists());
    assert_eq!(
        fx.db.get_subagent(&created.id).unwrap().unwrap().file_name,
        "old-name"
    );
}

#[test]
fn rename_onto_foreign_file_reports_file_exists() {
    let fx = Fixture::new();
    let created = fx.create(&agent("mine", "body"));
    fx.write_disk("theirs", "foreign");

    let result = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(
            Some(&created.id),
            Some("theirs"),
            &agent("mine", "body"),
            true,
        ),
        false,
    );
    assert_err_prefix(result, "SUBAGENT_FILE_EXISTS:");
    assert_eq!(fx.read("theirs"), "foreign");
    assert!(fx.path("mine").exists());
}

#[test]
fn file_name_taken_by_another_record_is_rejected() {
    let fx = Fixture::new();
    fx.create(&agent("first", "a"));
    let second = fx.create(&agent("second", "b"));

    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(None, None, &agent("first", "c"), true),
            false,
        ),
        "SUBAGENT_NAME_TAKEN:",
    );
    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(Some(&second.id), Some("first"), &agent("second", "b"), true),
            false,
        ),
        "SUBAGENT_NAME_TAKEN:",
    );
    assert_eq!(fx.read("second"), agent("second", "b"));
}

#[test]
fn create_over_foreign_file_needs_force() {
    let fx = Fixture::new();
    fx.write_disk("reviewer", "hand written");

    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(None, None, &agent("reviewer", "managed"), true),
            false,
        ),
        "SUBAGENT_FILE_EXISTS:",
    );
    assert_eq!(fx.read("reviewer"), "hand written");
    assert!(fx.db.list_subagents().unwrap().is_empty());

    let created = SubagentService::upsert(
        &fx.db,
        &fx.dir,
        input(None, None, &agent("reviewer", "managed"), true),
        true,
    )
    .unwrap();
    assert_eq!(created.status, SubagentStatus::Synced);
    assert_eq!(fx.read("reviewer"), agent("reviewer", "managed"));
}

#[test]
fn disable_removes_file_and_enable_rewrites_it() {
    let fx = Fixture::new();
    let content = agent("toggler", "body");
    let created = fx.create(&content);

    let disabled = SubagentService::toggle(&fx.db, &fx.dir, &created.id, false, false).unwrap();
    assert_eq!(disabled.status, SubagentStatus::Disabled);
    assert!(!fx.path("toggler").exists());
    assert!(fx.db.get_subagent(&created.id).unwrap().is_some());

    let enabled = SubagentService::toggle(&fx.db, &fx.dir, &created.id, true, false).unwrap();
    assert_eq!(enabled.status, SubagentStatus::Synced);
    assert_eq!(fx.read("toggler"), content);
}

#[test]
fn enabling_onto_foreign_file_reports_file_exists() {
    let fx = Fixture::new();
    let created = fx.create(&agent("toggler", "body"));
    SubagentService::toggle(&fx.db, &fx.dir, &created.id, false, false).unwrap();
    // 停用期间用户自己在同名位置放了文件
    fx.write_disk("toggler", "user file");

    assert_err_prefix(
        SubagentService::toggle(&fx.db, &fx.dir, &created.id, true, false),
        "SUBAGENT_FILE_EXISTS:",
    );
    assert_eq!(fx.read("toggler"), "user file");
    assert_eq!(fx.status(&created.id), SubagentStatus::Disabled);

    SubagentService::toggle(&fx.db, &fx.dir, &created.id, true, true).unwrap();
    assert_eq!(fx.read("toggler"), agent("toggler", "body"));
}

#[test]
fn disabling_drifted_file_needs_force() {
    let fx = Fixture::new();
    let created = fx.create(&agent("toggler", "body"));
    fx.write_disk("toggler", "edited in claude");

    assert_err_prefix(
        SubagentService::toggle(&fx.db, &fx.dir, &created.id, false, false),
        "SUBAGENT_DRIFTED:",
    );
    assert_eq!(fx.read("toggler"), "edited in claude");
    assert!(fx.db.get_subagent(&created.id).unwrap().unwrap().enabled);

    SubagentService::toggle(&fx.db, &fx.dir, &created.id, false, true).unwrap();
    assert!(!fx.path("toggler").exists());
}

#[test]
fn status_reports_missing_and_drifted() {
    let fx = Fixture::new();
    let created = fx.create(&agent("watcher", "body"));
    assert_eq!(fx.status(&created.id), SubagentStatus::Synced);

    fx.write_disk("watcher", "changed");
    assert_eq!(fx.status(&created.id), SubagentStatus::Drifted);

    std::fs::remove_file(fx.path("watcher")).unwrap();
    assert_eq!(fx.status(&created.id), SubagentStatus::Missing);
}

#[test]
fn editing_a_drifted_file_needs_force() {
    let fx = Fixture::new();
    let created = fx.create(&agent("watcher", "body"));
    fx.write_disk("watcher", "changed outside");

    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(Some(&created.id), None, &agent("watcher", "new"), true),
            false,
        ),
        "SUBAGENT_DRIFTED:",
    );
    assert_eq!(fx.read("watcher"), "changed outside");
}

#[test]
fn delete_refuses_drifted_file_unless_forced() {
    let fx = Fixture::new();
    let created = fx.create(&agent("victim", "body"));
    fx.write_disk("victim", "precious user edits");

    assert_err_prefix(
        SubagentService::delete(&fx.db, &fx.dir, &created.id, false),
        "SUBAGENT_DRIFTED:",
    );
    assert_eq!(fx.read("victim"), "precious user edits");
    assert!(fx.db.get_subagent(&created.id).unwrap().is_some());

    SubagentService::delete(&fx.db, &fx.dir, &created.id, true).unwrap();
    assert!(!fx.path("victim").exists());
    assert!(fx.db.get_subagent(&created.id).unwrap().is_none());
}

#[test]
fn delete_removes_synced_file_and_tolerates_missing_file() {
    let fx = Fixture::new();
    let synced = fx.create(&agent("synced", "body"));
    SubagentService::delete(&fx.db, &fx.dir, &synced.id, false).unwrap();
    assert!(!fx.path("synced").exists());

    let missing = fx.create(&agent("missing", "body"));
    std::fs::remove_file(fx.path("missing")).unwrap();
    SubagentService::delete(&fx.db, &fx.dir, &missing.id, false).unwrap();
    assert!(fx.db.list_subagents().unwrap().is_empty());
}

#[test]
fn delete_of_disabled_record_keeps_foreign_file() {
    let fx = Fixture::new();
    let created = fx.create(&agent("ghost", "body"));
    SubagentService::toggle(&fx.db, &fx.dir, &created.id, false, false).unwrap();
    fx.write_disk("ghost", "someone else");

    SubagentService::delete(&fx.db, &fx.dir, &created.id, false).unwrap();
    assert_eq!(fx.read("ghost"), "someone else");
}

#[test]
fn unknown_ids_report_not_found() {
    let fx = Fixture::new();
    assert_err_prefix(
        SubagentService::delete(&fx.db, &fx.dir, "nope", false),
        "SUBAGENT_NOT_FOUND:",
    );
    assert_err_prefix(
        SubagentService::toggle(&fx.db, &fx.dir, "nope", true, false),
        "SUBAGENT_NOT_FOUND:",
    );
    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(Some("nope"), None, &agent("x", "y"), true),
            false,
        ),
        "SUBAGENT_NOT_FOUND:",
    );
}

#[test]
fn resolve_drift_adopt_disk_takes_disk_content() {
    let fx = Fixture::new();
    let created = fx.create(&agent("drifter", "original"));
    let disk = "---\nname: drifter\ndescription: Edited via /agents\ncolor: blue\n---\nnew body\n";
    fx.write_disk("drifter", disk);
    assert_eq!(
        SubagentService::disk_content(&fx.db, &fx.dir, &created.id)
            .unwrap()
            .as_deref(),
        Some(disk)
    );

    let resolved = SubagentService::resolve_drift(
        &fx.db,
        &fx.dir,
        &created.id,
        SubagentDriftStrategy::AdoptDisk,
    )
    .unwrap();
    assert_eq!(resolved.content, disk);
    assert_eq!(resolved.description.as_deref(), Some("Edited via /agents"));
    assert_eq!(resolved.status, SubagentStatus::Synced);
    assert_eq!(fx.read("drifter"), disk);
}

#[test]
fn resolve_drift_adopt_disk_rejects_invalid_disk_content() {
    let fx = Fixture::new();
    let created = fx.create(&agent("drifter", "original"));
    fx.write_disk("drifter", "no frontmatter at all");

    assert_err_prefix(
        SubagentService::resolve_drift(
            &fx.db,
            &fx.dir,
            &created.id,
            SubagentDriftStrategy::AdoptDisk,
        ),
        "SUBAGENT_INVALID:",
    );
    assert_eq!(fx.status(&created.id), SubagentStatus::Drifted);
}

#[test]
fn resolve_drift_overwrite_disk_restores_record_content() {
    let fx = Fixture::new();
    let content = agent("drifter", "original");
    let created = fx.create(&content);
    fx.write_disk("drifter", "garbage");

    let resolved = SubagentService::resolve_drift(
        &fx.db,
        &fx.dir,
        &created.id,
        SubagentDriftStrategy::OverwriteDisk,
    )
    .unwrap();
    assert_eq!(resolved.status, SubagentStatus::Synced);
    assert_eq!(fx.read("drifter"), content);
}

#[test]
fn scan_lists_unmanaged_files_including_parse_failures() {
    let fx = Fixture::new();
    fx.create(&agent("managed", "body"));
    fx.write_disk("hand-made", &agent("hand-made", "body"));
    fx.write_disk("broken", "just text, no frontmatter");
    fx.write_disk("Bad_Name", &agent("bad-name", "body"));
    std::fs::write(fx.dir.join("notes.txt"), "ignored").unwrap();
    std::fs::create_dir_all(fx.dir.join("nested")).unwrap();
    std::fs::write(fx.dir.join("nested").join("deep.md"), agent("deep", "x")).unwrap();

    let found = SubagentService::scan_unmanaged(&fx.db, &fx.dir).unwrap();
    let names: Vec<&str> = found.iter().map(|u| u.file_name.as_str()).collect();
    assert_eq!(names, vec!["Bad_Name", "broken", "hand-made"]);

    let hand = found.iter().find(|u| u.file_name == "hand-made").unwrap();
    assert_eq!(hand.name.as_deref(), Some("hand-made"));
    assert!(hand.parse_error.is_none());
    assert_eq!(hand.content, agent("hand-made", "body"));

    let broken = found.iter().find(|u| u.file_name == "broken").unwrap();
    assert!(broken.parse_error.is_some());
    assert_eq!(broken.content, "just text, no frontmatter");

    // 内容能解析但文件名不合规，导入会失败，所以同样带 parseError
    let bad = found.iter().find(|u| u.file_name == "Bad_Name").unwrap();
    assert!(bad.parse_error.is_some());
}

#[test]
fn scan_of_missing_directory_is_empty() {
    let fx = Fixture::new();
    assert!(SubagentService::scan_unmanaged(&fx.db, &fx.dir)
        .unwrap()
        .is_empty());
    assert!(!SubagentService::dir_info(&fx.dir).exists);
}

#[test]
fn import_adopts_files_without_rewriting_them() {
    let fx = Fixture::new();
    // CRLF 与 BOM 都要原样保留，导入不能改动磁盘上的任何字节
    let raw = "\u{feff}---\r\nname: imported-one\r\ndescription: From disk\r\n---\r\nBody\r\n";
    fx.write_disk("imported-one", raw);
    let before = std::fs::metadata(fx.path("imported-one"))
        .unwrap()
        .modified()
        .unwrap();

    let imported =
        SubagentService::import(&fx.db, &fx.dir, vec!["imported-one".to_string()]).unwrap();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0].source, "imported");
    assert!(imported[0].enabled);
    assert_eq!(imported[0].status, SubagentStatus::Synced);
    assert_eq!(imported[0].content, raw);
    assert_eq!(fx.read("imported-one"), raw);
    assert_eq!(
        std::fs::metadata(fx.path("imported-one"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
    assert!(SubagentService::scan_unmanaged(&fx.db, &fx.dir)
        .unwrap()
        .is_empty());
}

#[test]
fn import_rejects_whole_batch_when_any_file_is_invalid() {
    let fx = Fixture::new();
    fx.write_disk("good", &agent("good", "body"));
    fx.write_disk("broken", "no frontmatter");

    assert_err_prefix(
        SubagentService::import(
            &fx.db,
            &fx.dir,
            vec!["good".to_string(), "broken".to_string()],
        ),
        "SUBAGENT_INVALID:",
    );
    assert!(fx.db.list_subagents().unwrap().is_empty());

    assert_err_prefix(
        SubagentService::import(&fx.db, &fx.dir, vec!["../escape".to_string()]),
        "SUBAGENT_INVALID:",
    );
}

#[test]
fn unknown_frontmatter_fields_are_preserved_verbatim() {
    let fx = Fixture::new();
    let content = "---\n\
name: fancy\n\
description: \"Quoted: with colon\"\n\
tools: Read, Grep, Glob\n\
model: sonnet\n\
color: purple\n\
future-field:\n  nested: [1, 2, 3]\n  # comment kept\n\
permissionMode: plan\n\
---\n\
\n\
System prompt with *markdown*.\n";
    let created = fx.create(content);
    assert_eq!(created.content, content);
    assert_eq!(created.description.as_deref(), Some("Quoted: with colon"));
    assert_eq!(fx.read("fancy"), content);
    let stored = fx.db.get_subagent(&created.id).unwrap().unwrap();
    assert_eq!(stored.content, content);
}

#[test]
fn claude_generated_descriptions_that_break_strict_yaml_still_parse() {
    // /agents 生成的典型写法：普通标量里夹着 "Context: ..."，严格 YAML 会报错
    let content = "---\nname: test-runner\ndescription: Use this agent after code changes. Examples: <example>Context: user wrote code. user: \"done\" assistant: \"running tests\"</example>\nmodel: inherit\n---\nRun tests.\n";
    let meta = parse_subagent_meta(content).expect("loose parse");
    assert_eq!(meta.name, "test-runner");
    assert!(meta
        .description
        .starts_with("Use this agent after code changes."));
    assert_eq!(meta.model.as_deref(), Some("inherit"));
}

#[test]
fn invalid_names_and_missing_fields_are_rejected() {
    let fx = Fixture::new();
    for bad in [
        "---\nname: Bad Name\ndescription: x\n---\n",
        "---\nname: -leading\ndescription: x\n---\n",
        "---\nname: under_score\ndescription: x\n---\n",
        "---\nname: ../evil\ndescription: x\n---\n",
        "---\ndescription: missing name\n---\n",
        "---\nname: no-desc\n---\n",
        "---\nname: empty-desc\ndescription: \"  \"\n---\n",
        "no frontmatter",
        "---\nname: unterminated\ndescription: x\n",
    ] {
        assert_err_prefix(
            SubagentService::upsert(&fx.db, &fx.dir, input(None, None, bad, true), false),
            "SUBAGENT_INVALID:",
        );
    }
    let long_name = "a".repeat(65);
    assert_err_prefix(
        SubagentService::upsert(
            &fx.db,
            &fx.dir,
            input(None, None, &agent(&long_name, "x"), true),
            false,
        ),
        "SUBAGENT_INVALID:",
    );
    assert!(fx.db.list_subagents().unwrap().is_empty());
}

#[test]
fn path_traversal_file_names_are_rejected() {
    let fx = Fixture::new();
    let content = agent("safe", "body");
    for bad in [
        "../escape",
        "..",
        "a/b",
        "a\\b",
        "/abs",
        "C:evil",
        "dot.name",
        "",
    ] {
        // 空串等同于未提供，回落到 name；其余都必须被拒
        if bad.is_empty() {
            continue;
        }
        assert_err_prefix(
            SubagentService::upsert(
                &fx.db,
                &fx.dir,
                input(None, Some(bad), &content, true),
                false,
            ),
            "SUBAGENT_INVALID:",
        );
    }
    assert!(fx.db.list_subagents().unwrap().is_empty());
    assert!(!fx.dir.exists() || std::fs::read_dir(&fx.dir).unwrap().next().is_none());
    assert!(!fx.temp.path().join(".claude").join("escape.md").exists());
}

#[test]
fn sync_after_restore_writes_missing_files_and_keeps_drifted_ones() {
    let fx = Fixture::new();
    let missing = fx.create(&agent("restored", "remote body"));
    let drifted = fx.create(&agent("local-edit", "remote body"));
    let disabled = fx.create(&agent("off", "body"));
    SubagentService::toggle(&fx.db, &fx.dir, &disabled.id, false, false).unwrap();

    std::fs::remove_file(fx.path("restored")).unwrap();
    fx.write_disk("local-edit", "local only changes");

    SubagentService::sync_after_restore(&fx.db, &fx.dir).unwrap();
    assert_eq!(fx.read("restored"), agent("restored", "remote body"));
    assert_eq!(fx.status(&missing.id), SubagentStatus::Synced);
    assert_eq!(fx.read("local-edit"), "local only changes");
    assert_eq!(fx.status(&drifted.id), SubagentStatus::Drifted);
    assert!(!fx.path("off").exists());
}

#[test]
fn plugin_subagents_are_read_from_installed_plugins_manifest() {
    let temp = tempdir().unwrap();
    let root = temp.path().join(".claude");
    let install = root
        .join("plugins")
        .join("cache")
        .join("mkt")
        .join("my-plugin")
        .join("1.0.0");
    std::fs::create_dir_all(install.join("agents")).unwrap();
    std::fs::write(
        install.join("agents").join("planner.md"),
        "---\nname: planner\ndescription: Plans work\nmodel: opus\n---\nPlan.\n",
    )
    .unwrap();
    std::fs::write(install.join("agents").join("raw.md"), "no frontmatter").unwrap();
    let manifest = serde_json::json!({
        "version": 2,
        "plugins": {
            "my-plugin@mkt": [
                { "scope": "user", "installPath": install.to_string_lossy() },
                { "scope": "project", "installPath": install.to_string_lossy() }
            ],
            "legacy@mkt": { "installPath": temp.path().join("missing").to_string_lossy() }
        }
    });
    std::fs::write(
        root.join("plugins").join("installed_plugins.json"),
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let found = SubagentService::list_plugin_subagents(&root);
    assert_eq!(
        found.len(),
        2,
        "duplicate install paths are listed once: {found:?}"
    );
    let planner = found.iter().find(|p| p.name == "planner").unwrap();
    assert_eq!(planner.plugin_name, "my-plugin");
    assert_eq!(planner.model.as_deref(), Some("opus"));
    assert_eq!(planner.description.as_deref(), Some("Plans work"));
    assert!(found
        .iter()
        .any(|p| p.name == "raw" && p.description.is_none()));

    assert!(SubagentService::list_plugin_subagents(&temp.path().join("nowhere")).is_empty());
}

#[test]
fn serialized_shape_matches_frontend_contract() {
    let fx = Fixture::new();
    let created = fx.create(&agent("shape", "body"));
    let value = serde_json::to_value(&created).unwrap();
    for key in [
        "id",
        "name",
        "fileName",
        "description",
        "content",
        "enabled",
        "source",
        "status",
        "filePath",
        "createdAt",
        "updatedAt",
    ] {
        assert!(value.get(key).is_some(), "missing {key} in {value}");
    }
    assert_eq!(value["status"], "synced");
    assert_eq!(
        serde_json::to_value(SubagentStatus::Drifted).unwrap(),
        "drifted"
    );
    let strategy: SubagentDriftStrategy = serde_json::from_str("\"adopt_disk\"").unwrap();
    assert_eq!(strategy, SubagentDriftStrategy::AdoptDisk);
    let parsed: SubagentInput =
        serde_json::from_str(r#"{"content":"x","enabled":true,"fileName":"f"}"#).unwrap();
    assert_eq!(parsed.file_name.as_deref(), Some("f"));
    assert!(parsed.id.is_none());
}

#[test]
#[serial_test::serial]
fn agents_dir_honors_test_home_override() {
    struct EnvGuard(Option<std::ffi::OsString>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }
    let temp = tempdir().unwrap();
    let _guard = EnvGuard(std::env::var_os("CC_SWITCH_TEST_HOME"));
    std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

    if crate::settings::get_claude_override_dir().is_none() {
        assert_eq!(
            SubagentService::agents_dir(),
            temp.path().join(".claude").join("agents")
        );
        let info = SubagentService::dir_info(&SubagentService::agents_dir());
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(info.display_path, format!("~{sep}.claude{sep}agents"));
    }
}
