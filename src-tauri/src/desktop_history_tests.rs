use super::*;
use crate::desktop_data::{root, RootKind};
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use tempfile::TempDir;
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
            projects: temp.path().join("projects"),
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
    fn session(&self, id: &str, archived: bool) {
        fs::write(self.dir(&self.source).join(format!("{id}.json")), serde_json::to_vec(&json!({"sessionId":id,"isArchived":archived,"title":"Fictional history","cliSessionId":"cli-current","priorCliSessionIds":["cli-prior"],"preClearCliSessionId":"cli-before-clear","unarchivedCliSessionId":"cli-current"})).unwrap()).unwrap();
    }
}
#[test]
fn inventory_preserves_profile_and_archive_identity_and_links_every_lineage_id() {
    let f = Fixture::new();
    f.session("local_active", false);
    f.session("local_archived", true);
    f.session("local_deleted", false);
    fs::write(f.dir(&f.source).join("deleted_deleted"), "1").unwrap();
    for project in ["project-a", "project-b"] {
        fs::create_dir_all(f.store.projects.join(project)).unwrap();
    }
    for (project, id) in [
        ("project-a", "cli-current"),
        ("project-a", "cli-before-clear"),
        ("project-b", "cli-before-clear"),
    ] {
        fs::write(
            f.store.projects.join(project).join(format!("{id}.jsonl")),
            "{}\n",
        )
        .unwrap();
    }
    let result = f.store.inventory().unwrap();
    assert_eq!(result.profiles.len(), 2);
    let p = &result.profiles[0];
    assert_eq!(p.sessions.len(), 2);
    assert!(p.sessions.iter().any(|s| s.archived));
    let links = &p.sessions[0].transcripts;
    assert_eq!(links.len(), 3);
    assert!(matches!(&links[0],TranscriptLink::Missing{id} if id=="cli-prior"));
    assert!(matches!(&links[1],TranscriptLink::Ambiguous{id} if id=="cli-before-clear"));
    assert!(matches!(&links[2],TranscriptLink::Available{id,..} if id=="cli-current"));
}

#[test]
fn legacy_archives_and_account_tombstones_control_visibility() {
    let f = Fixture::new();
    f.session("local_archived", false);
    f.session("local_deleted", false);
    fs::write(
        f.dir(&f.source).join("archived-sessions.idx"),
        r#"{"v":1,"archived":["local_archived"]}"#,
    )
    .unwrap();
    let other = f.dir(&f.source).parent().unwrap().join("other-org");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("deleted_deleted"), "1").unwrap();
    let inventory = f.store.inventory().unwrap();
    let profile = inventory
        .profiles
        .iter()
        .find(|p| p.key == f.source)
        .unwrap();
    assert_eq!(profile.sessions.len(), 1);
    assert!(profile.sessions[0].archived);
}
#[test]
fn linked_transcripts_require_an_undeleted_owner_and_have_no_resume_command() {
    let f = Fixture::new();
    f.session("local_one", false);
    fs::create_dir_all(f.store.projects.join("fictional")).unwrap();
    fs::write(f.store.projects.join("fictional/cli-current.jsonl"), "{}\n").unwrap();
    let session = f
        .store
        .open_transcript(&f.source, "local_one", "cli-current")
        .unwrap();
    assert!(session.resume_command.is_none());
    assert!(f
        .store
        .open_transcript(&f.target, "local_one", "cli-current")
        .is_err());
    fs::write(f.dir(&f.source).join("deleted_cli-current"), "1").unwrap();
    assert!(f
        .store
        .open_transcript(&f.source, "local_one", "cli-current")
        .is_err());
}
#[test]
fn malformed_state_and_linked_records_are_not_exposed() {
    let f = Fixture::new();
    f.session("local_one", false);
    fs::write(
        f.dir(&f.source).join("archived-sessions.idx"),
        r#"{"v":9,"archived":[]}"#,
    )
    .unwrap();
    let result = f.store.inventory().unwrap();
    assert!(result.profiles[0].sessions.is_empty());
    assert!(!result.profiles[0].warnings.is_empty());
    fs::remove_file(f.dir(&f.source).join("archived-sessions.idx")).unwrap();
    fs::remove_file(f.dir(&f.source).join("local_one.json")).unwrap();
    let outside = f.temp.path().join("outside.json");
    fs::write(&outside, "private fixture").unwrap();
    symlink(&outside, f.dir(&f.source).join("local_one.json")).unwrap();
    assert!(f.store.inventory().unwrap().profiles[0].sessions.is_empty());
    assert_eq!(fs::read_to_string(outside).unwrap(), "private fixture");
}
