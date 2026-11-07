use cc_switch_lib::{
    get_prompt_file_protection, set_prompt_file_protection, update_settings, AppSettings, AppState,
    AppType, Database, MultiAppConfig, ProfileScope, ProfileService, Prompt, PromptService,
    Provider,
};
use serde_json::json;
use std::{ffi::OsString, fs, path::PathBuf, sync::Arc};

struct TestHome {
    _dir: tempfile::TempDir,
    previous: Option<OsString>,
}
impl TestHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
        update_settings(AppSettings {
            hermes_config_dir: Some(dir.path().join(".hermes").to_string_lossy().into_owned()),
            opencode_config_dir: Some(
                dir.path()
                    .join(".config/opencode")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..Default::default()
        })
        .unwrap();
        Self {
            _dir: dir,
            previous,
        }
    }
}
impl Drop for TestHome {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
            None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
        }
    }
}

#[test]
#[serial_test::serial]
fn local_protection_preserves_rules_across_restores_and_prompt_operations() {
    let test_home = TestHome::new();
    let root = test_home._dir.path();
    let policy = root.join(".cc-switch/prompt-file-protection.json");
    if policy.exists() {
        fs::remove_file(&policy).unwrap();
    }
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let mut results = Vec::new();
    for (app, relative) in [
        (AppType::Claude, ".claude/CLAUDE.md"),
        (AppType::Codex, ".codex/AGENTS.md"),
    ] {
        let key = app.as_str().to_string();
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let local = "# Team rules\nB: locally maintained instructions\n";
        let remote = Prompt {
            id: "remote".into(),
            name: "Synced team rules".into(),
            content: "A: remote snapshot\n".into(),
            description: None,
            enabled: true,
            created_at: None,
            updated_at: None,
        };
        state.db.save_prompt(&key, &remote).unwrap();
        assert!(!get_prompt_file_protection(key.clone()).unwrap());
        fs::write(&path, local).unwrap();
        PromptService::sync_to_live(&state, app.clone()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), remote.content);
        fs::write(&path, local).unwrap();

        set_prompt_file_protection(key.clone(), true).unwrap();
        assert!(get_prompt_file_protection(key.clone()).unwrap());
        PromptService::sync_to_live(&state, app.clone()).unwrap();
        let restored = fs::read_to_string(&path).unwrap();
        results.push(json!({"app":key,"protectionOff":remote.content,"beforeProtectedRestore":local,"afterProtectedRestore":restored}));
        if let Some(output) = std::env::var_os("CC_SWITCH_PROMPT_TEST_EVIDENCE_DIR") {
            fs::write(
                PathBuf::from(output).join("protection-results.json"),
                serde_json::to_vec_pretty(&results).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(
            restored, local,
            "protected restore must retain the local rules"
        );

        for (id, enabled) in [("draft", false), ("remote", false), ("remote", true)] {
            let modified = Prompt {
                id: id.into(),
                enabled,
                content: "edited template".into(),
                ..remote.clone()
            };
            assert!(PromptService::upsert_prompt(&state, app.clone(), id, modified).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), local);
        }
        assert!(PromptService::enable_prompt(&state, app.clone(), "remote").is_err());
        assert!(PromptService::import_from_file(&state, app.clone()).is_err());
        let listed = PromptService::get_prompts(&state, app.clone()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed["remote"].content, remote.content,
            "protection must also prevent live backfill into the restored snapshot"
        );
        let fresh = AppState::new(Arc::new(Database::memory().unwrap()));
        assert_eq!(
            PromptService::import_from_file_on_first_launch(&fresh, app.clone()).unwrap(),
            0
        );
        assert!(
            fresh.db.get_prompts(&key).unwrap().is_empty(),
            "first launch must not adopt independently managed rules"
        );

        // Ordinary device-preference saves and a replaced prompt row cannot reset protection.
        update_settings(AppSettings::default()).unwrap();
        let snapshot = Database::memory().unwrap();
        snapshot
            .save_prompt(
                &key,
                &Prompt {
                    content: "new snapshot".into(),
                    ..remote.clone()
                },
            )
            .unwrap();
        let provider = Provider::with_id(
            "team-api".into(),
            "Team gateway".into(),
            json!({"env":{"ANTHROPIC_BASE_URL":"https://gateway.example.test","ANTHROPIC_AUTH_TOKEN":"public-fake-key"}}),
            None,
        );
        snapshot.save_provider("claude", &provider).unwrap();
        let sql = snapshot.export_sql_string_for_sync().unwrap();
        assert!(!sql.contains("prompt-file-protection"));
        state.db.import_sql_string(&sql).unwrap();
        assert!(
            state
                .db
                .get_provider_by_id("team-api", "claude")
                .unwrap()
                .is_some(),
            "provider configuration still restores"
        );
        PromptService::sync_to_live(&state, app.clone()).unwrap();
        assert!(get_prompt_file_protection(key.clone()).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), local);

        set_prompt_file_protection(key.clone(), false).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            local,
            "toggling protection itself does not project a prompt"
        );
        PromptService::sync_to_live(&state, app.clone()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new snapshot");
    }
    set_prompt_file_protection("claude".into(), true).unwrap();
    assert!(!get_prompt_file_protection("codex".into()).unwrap());
    assert!(set_prompt_file_protection("gemini".into(), true).is_err());
    fs::write(&policy, "{broken").unwrap();
    let claude = root.join(".claude/CLAUDE.md");
    let before = fs::read(&claude).unwrap();
    assert!(
        PromptService::sync_to_live(&state, AppType::Claude).is_err(),
        "unreadable policy must fail closed"
    );
    assert_eq!(fs::read(&claude).unwrap(), before);
    fs::remove_file(&policy).unwrap();
    println!("Claude/Codex: protected restores and edits preserved local rules; unprotected restores still work; device-policy persistence, independent app switches and malformed-policy safety passed.");
}

fn team_prompt() -> Prompt {
    Prompt {
        id: "team".into(),
        name: "Team template".into(),
        content: "A: shared rules".into(),
        enabled: true,
        description: None,
        created_at: None,
        updated_at: None,
    }
}

#[test]
#[serial_test::serial]
#[cfg(feature = "test-hooks")]
fn policy_survives_data_directory_changes_and_fallback() {
    use cc_switch_lib::with_app_config_dir_override_for_test;
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let alternative = home._dir.path().join("other-database");
    fs::create_dir_all(&alternative).unwrap();
    fs::write(
        alternative.join("prompt-file-protection.json"),
        r#"{"claude":false,"codex":false}"#,
    )
    .unwrap();
    for (app, file) in [
        (AppType::Claude, ".claude/CLAUDE.md"),
        (AppType::Codex, ".codex/AGENTS.md"),
    ] {
        let path = home._dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "B: independently maintained").unwrap();
        state.db.save_prompt(app.as_str(), &team_prompt()).unwrap();
        set_prompt_file_protection(app.as_str().into(), true).unwrap();
        for directory in [
            Some(alternative.clone()),
            Some(home._dir.path().join("missing-database")),
            None,
        ] {
            with_app_config_dir_override_for_test(directory, || {
                assert!(
                    get_prompt_file_protection(app.as_str().into()).unwrap(),
                    "changing the data directory must not reset device policy"
                );
                PromptService::sync_to_live(&state, app.clone()).unwrap();
                assert_eq!(
                    fs::read_to_string(&path).unwrap(),
                    "B: independently maintained"
                );
            });
        }
    }
}

#[test]
#[serial_test::serial]
fn legacy_json_migration_does_not_adopt_protected_local_rules() {
    let home = TestHome::new();
    for (app, file) in [
        ("claude", ".claude/CLAUDE.md"),
        ("codex", ".codex/AGENTS.md"),
    ] {
        let path = home._dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "B: private local rules").unwrap();
        set_prompt_file_protection(app.into(), true).unwrap();
    }
    MultiAppConfig::default().save().unwrap();
    let migrated = MultiAppConfig::load().unwrap();
    assert!(
        migrated.prompts.claude.prompts.is_empty(),
        "legacy load must not import protected CLAUDE.md"
    );
    assert!(
        migrated.prompts.codex.prompts.is_empty(),
        "legacy load must not import protected AGENTS.md"
    );
    let db = Database::memory().unwrap();
    db.migrate_from_json(&migrated).unwrap();
    assert!(!db
        .export_sql_string()
        .unwrap()
        .contains("B: private local rules"));
}

#[test]
#[serial_test::serial]
fn releasing_protection_does_not_backfill_until_explicit_adoption() {
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    for (app, file, scope) in [
        (AppType::Claude, ".claude/CLAUDE.md", ProfileScope::Claude),
        (AppType::Codex, ".codex/AGENTS.md", ProfileScope::Codex),
    ] {
        let path = home._dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "B: private local rules").unwrap();
        state.db.save_prompt(app.as_str(), &team_prompt()).unwrap();
        set_prompt_file_protection(app.as_str().into(), true).unwrap();
        set_prompt_file_protection(app.as_str().into(), false).unwrap();
        let prompts = PromptService::get_prompts(&state, app.clone()).unwrap();
        assert_eq!(
            prompts["team"].content, "A: shared rules",
            "reading after release must not replace the shared template with private rules"
        );
        assert!(
            !prompts["team"].enabled,
            "the file is not managed until an explicit write"
        );
        assert_eq!(
            state.db.get_prompts(app.as_str()).unwrap()["team"].content,
            "A: shared rules"
        );
        assert!(PromptService::enable_prompt(&state, app.clone(), "missing").is_err());
        assert!(
            PromptService::upsert_prompt(
                &state,
                app.clone(),
                "team",
                Prompt {
                    content: "stale enabled editor".into(),
                    ..team_prompt()
                }
            )
            .is_err(),
            "an enabled bit in stale edit data is not explicit adoption"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "B: private local rules");
        assert_eq!(
            state.db.get_prompts(app.as_str()).unwrap()["team"].content,
            "A: shared rules"
        );
        assert!(
            !state
                .db
                .get_prompts(app.as_str())
                .unwrap()
                .values()
                .any(|p| p.content == "B: private local rules"),
            "an invalid target must not adopt the local file"
        );
        PromptService::upsert_prompt(
            &state,
            app.clone(),
            "team",
            Prompt {
                enabled: false,
                content: "edited draft".into(),
                ..team_prompt()
            },
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "B: private local rules",
            "editing a detached inactive template must not clear the independent file"
        );
        state.db.save_prompt(app.as_str(), &team_prompt()).unwrap();
        let profile = ProfileService::create(&state, "Team project", scope).unwrap();
        let warnings = ProfileService::apply(&state, &profile.id, scope).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "A: shared rules",
            "explicit project apply must not skip adoption just because the DB row is enabled"
        );
        let prompts = PromptService::get_prompts(&state, app.clone()).unwrap();
        assert!(prompts["team"].enabled);
        assert!(
            prompts
                .values()
                .any(|p| !p.enabled && p.content == "B: private local rules"),
            "explicit activation preserves a separate backup of local rules"
        );
        fs::write(&path, "C: later managed edit").unwrap();
        assert_eq!(
            PromptService::get_prompts(&state, app).unwrap()["team"].content,
            "C: later managed edit"
        );
    }
}

#[test]
#[serial_test::serial]
#[cfg(unix)]
fn dangling_policy_link_is_not_treated_as_an_unconfigured_device() {
    let home = TestHome::new();
    let policy = home
        ._dir
        .path()
        .join(".cc-switch/prompt-file-protection.json");
    std::os::unix::fs::symlink(home._dir.path().join("missing-policy.json"), &policy).unwrap();
    assert!(
        get_prompt_file_protection("claude".into()).is_err(),
        "an unreadable existing policy must fail closed"
    );
}

#[test]
#[serial_test::serial]
#[cfg(unix)]
fn failed_ownership_commit_is_reported_and_does_not_allow_backfill() {
    use std::os::unix::fs::PermissionsExt;
    // Root can bypass filesystem permissions; other cases still cover malformed
    // policies. Local and normal CI user accounts exercise this I/O failure.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let file = home._dir.path().join(".claude/CLAUDE.md");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "B: independent").unwrap();
    state.db.save_prompt("claude", &team_prompt()).unwrap();
    set_prompt_file_protection("claude".into(), true).unwrap();
    set_prompt_file_protection("claude".into(), false).unwrap();
    let policy_dir = home._dir.path().join(".cc-switch");
    let permissions = fs::metadata(&policy_dir).unwrap().permissions();
    fs::set_permissions(&policy_dir, fs::Permissions::from_mode(0o500)).unwrap();
    let result = PromptService::enable_prompt(&state, AppType::Claude, "team");
    fs::set_permissions(&policy_dir, permissions).unwrap();
    assert!(
        result.is_err(),
        "file success plus ownership commit failure must not report full success"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "A: shared rules");
    fs::write(&file, "C: independently edited after the failed operation").unwrap();
    let prompts = PromptService::get_prompts(&state, AppType::Claude).unwrap();
    assert!(!prompts["team"].enabled);
    assert_eq!(prompts["team"].content, "A: shared rules");
}

#[test]
#[serial_test::serial]
#[cfg(unix)]
fn dangling_policy_directory_is_not_treated_as_an_unconfigured_device() {
    let home = TestHome::new();
    set_prompt_file_protection("claude".into(), true).unwrap();
    let directory = home._dir.path().join(".cc-switch");
    fs::rename(
        &directory,
        home._dir.path().join("retained-policy-directory"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        home._dir.path().join("offline-policy-directory"),
        &directory,
    )
    .unwrap();
    assert!(
        get_prompt_file_protection("claude".into()).is_err(),
        "an unreachable existing policy directory must fail closed"
    );
}

#[test]
#[serial_test::serial]
fn detached_adoption_requires_a_readable_existing_file() {
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    for (app, relative) in [
        (AppType::Claude, ".claude/CLAUDE.md"),
        (AppType::Codex, ".codex/AGENTS.md"),
    ] {
        let path = home._dir.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, [0xff, 0xfe, 0x41]).unwrap();
        state.db.save_prompt(app.as_str(), &team_prompt()).unwrap();
        set_prompt_file_protection(app.as_str().into(), true).unwrap();
        set_prompt_file_protection(app.as_str().into(), false).unwrap();
        assert!(
            PromptService::enable_prompt(&state, app.clone(), "team").is_err(),
            "a failed backup read must not be silently discarded"
        );
        assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe, 0x41]);
        assert_eq!(state.db.get_prompts(app.as_str()).unwrap().len(), 1);
        assert_eq!(
            state.db.get_prompts(app.as_str()).unwrap()["team"].content,
            "A: shared rules"
        );
        fs::remove_file(&path).unwrap();
        PromptService::enable_prompt(&state, app.clone(), "team").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "A: shared rules",
            "a genuinely absent file can be created after explicit activation"
        );
    }
}

#[test]
#[serial_test::serial]
fn detached_adoption_keeps_exact_separate_backups() {
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let file = home._dir.path().join(".claude/CLAUDE.md");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    state.db.save_prompt("claude", &team_prompt()).unwrap();
    state
        .db
        .save_prompt(
            "claude",
            &Prompt {
                id: "similar".into(),
                content: "B: local rules".into(),
                enabled: false,
                ..team_prompt()
            },
        )
        .unwrap();
    let originals = [
        "  B: local rules\n\n",
        "D: later independent rules",
        "E: latest independent rules",
    ];
    for content in originals {
        set_prompt_file_protection("claude".into(), true).unwrap();
        fs::write(&file, content).unwrap();
        set_prompt_file_protection("claude".into(), false).unwrap();
        PromptService::enable_prompt(&state, AppType::Claude, "team").unwrap();
    }
    let prompts = state.db.get_prompts("claude").unwrap();
    for original in originals {
        assert!(
            prompts
                .values()
                .any(|p| !p.enabled && p.content == original),
            "missing exact backup for {original:?}"
        );
    }
    assert_eq!(prompts["similar"].content, "B: local rules");
}

#[test]
#[serial_test::serial]
#[cfg(unix)] // This fixture relies on POSIX resolution of a missing prefix before `..`.
fn unresolved_parent_components_cannot_turn_an_existing_file_into_a_new_file() {
    let home = TestHome::new();
    let state = AppState::new(Arc::new(Database::memory().unwrap()));
    let existing = home._dir.path().join("existing");
    fs::create_dir_all(&existing).unwrap();
    fs::write(existing.join("CLAUDE.md"), "B: independent original").unwrap();
    state.db.save_prompt("claude", &team_prompt()).unwrap();
    update_settings(AppSettings {
        claude_config_dir: Some(
            home._dir
                .path()
                .join("missing/../existing")
                .to_string_lossy()
                .into_owned(),
        ),
        ..Default::default()
    })
    .unwrap();
    set_prompt_file_protection("claude".into(), true).unwrap();
    set_prompt_file_protection("claude".into(), false).unwrap();
    assert!(
        PromptService::enable_prompt(&state, AppType::Claude, "team").is_err(),
        "an unresolved parent traversal must not skip backup of an existing file"
    );
    assert_eq!(
        fs::read_to_string(existing.join("CLAUDE.md")).unwrap(),
        "B: independent original"
    );
    assert!(!home._dir.path().join("missing").exists());
    // A parent traversal that already resolves is safe; only the genuinely new
    // normal suffix will be created, without normalizing across symlinks.
    update_settings(AppSettings {
        claude_config_dir: Some(
            home._dir
                .path()
                .join("existing/../new-empty")
                .to_string_lossy()
                .into_owned(),
        ),
        ..Default::default()
    })
    .unwrap();
    PromptService::enable_prompt(&state, AppType::Claude, "team").unwrap();
    assert_eq!(
        fs::read_to_string(home._dir.path().join("new-empty/CLAUDE.md")).unwrap(),
        "A: shared rules"
    );
}
