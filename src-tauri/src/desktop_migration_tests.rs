use super::*;
use crate::desktop_data::{root, RootKind};
use serde_json::{json, Value};
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
    fn request(&self, sessions: &[&str], _tasks: &[&str]) -> Request {
        Request {
            source: self.source.clone(),
            target: self.target.clone(),
            session_ids: sessions.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn apply(&self, request: &Request) -> Receipt {
        let preview = self.store.prepare(request).unwrap().preview;
        self.store
            .apply_checked(request, &preview.token, || Ok(()))
            .unwrap()
    }
}

#[test]
fn archive_index_and_tombstones_across_orgs_are_respected() {
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
    let request = f.request(&["local_archived"], &[]);
    let receipt = f.apply(&request);
    assert!(receipt.error.is_none());
    let copied: Value =
        serde_json::from_slice(&fs::read(f.dir(&f.target).join("local_archived.json")).unwrap())
            .unwrap();
    assert_eq!(copied["isArchived"], true);
    let preview = f
        .store
        .prepare(&f.request(&["local_deleted"], &[]))
        .unwrap()
        .preview;
    assert!(!preview.can_apply);
    assert!(matches!(
        preview.items[0].disposition,
        Disposition::Blocked {
            reason: Reason::Deleted
        }
    ));
}

#[test]
fn session_copy_is_previewed_idempotent_and_does_not_require_a_transcript() {
    let f = Fixture::new();
    f.session("local_one", false);
    let before = fs::read(f.dir(&f.source).join("local_one.json")).unwrap();
    let request = f.request(&["local_one"], &[]);
    let preview = f.store.prepare(&request).unwrap().preview;
    assert!(preview.can_apply);
    assert!(!f.dir(&f.target).join("local_one.json").exists());
    let receipt = f.apply(&request);
    assert_eq!(receipt.copied_sessions, ["local_one"]);
    assert_eq!(
        fs::read(f.dir(&f.source).join("local_one.json")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(f.dir(&f.target).join("local_one.json")).unwrap(),
        before
    );
    let preview = f.store.prepare(&request).unwrap().preview;
    assert!(!preview.can_apply);
    assert!(matches!(preview.items[0].disposition, Disposition::Exists));
}

#[test]
fn target_tombstone_or_archive_index_never_gets_removed() {
    for marker in ["deleted_one", "archived-sessions.idx"] {
        let f = Fixture::new();
        f.session("local_one", false);
        let bytes = if marker.starts_with("deleted") {
            "1"
        } else {
            r#"{"v":1,"archived":["local_one"]}"#
        };
        fs::write(f.dir(&f.target).join(marker), bytes).unwrap();
        assert!(
            !f.store
                .prepare(&f.request(&["local_one"], &[]))
                .unwrap()
                .preview
                .can_apply
        );
        assert_eq!(
            fs::read_to_string(f.dir(&f.target).join(marker)).unwrap(),
            bytes
        );
    }
}

#[test]
fn path_traversal_and_symlinked_profiles_or_records_are_rejected() {
    let f = Fixture::new();
    f.session("local_one", false);
    let mut request = f.request(&["local_one"], &[]);
    request.source.account = "../outside".into();
    assert!(f.store.prepare(&request).is_err());
    let outside = f.temp.path().join("outside.json");
    fs::write(&outside, "private fixture").unwrap();
    fs::remove_file(f.dir(&f.source).join("local_one.json")).unwrap();
    symlink(&outside, f.dir(&f.source).join("local_one.json")).unwrap();
    assert!(f.store.prepare(&f.request(&["local_one"], &[])).is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "private fixture");
}

#[test]
fn exclusive_publication_refuses_a_dangling_symlink_and_concurrent_writers() {
    let f = Fixture::new();
    let dir = Dir::open(&f.dir(&f.target)).unwrap();
    let outside = f.temp.path().join("never-created");
    symlink(&outside, dir.path.join("local_one.json")).unwrap();
    assert_eq!(
        write(&dir, "local_one.json", b"one", None)
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(!outside.exists());
    std::thread::scope(|scope| {
        let a = scope.spawn(|| write(&dir, "local_race.json", b"a", None));
        let b = scope.spawn(|| write(&dir, "local_race.json", b"b", None));
        let results = [a.join().unwrap(), b.join().unwrap()];
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| r
                    .as_ref()
                    .is_err_and(|e| e.kind() == io::ErrorKind::AlreadyExists))
                .count(),
            1
        );
    });
    assert!(!dir.entries().unwrap().iter().any(|n| n.ends_with(".tmp")));
}

#[test]
fn held_directory_cannot_be_redirected_by_a_symlink_swap() {
    let f = Fixture::new();
    let path = f.dir(&f.target);
    let dir = Dir::open(&path).unwrap();
    let moved = path.with_file_name("moved");
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::rename(&path, &moved).unwrap();
    symlink(&outside, &path).unwrap();
    write(&dir, "local_one.json", b"fixture", None).unwrap();
    assert!(moved.join("local_one.json").exists());
    assert!(!outside.join("local_one.json").exists());
}

#[test]
fn unverified_archive_index_and_malformed_record_fail_closed() {
    let f = Fixture::new();
    f.session("local_one", false);
    fs::write(f.dir(&f.source).join("local_one.json"), "broken JSON").unwrap();
    assert!(
        !f.store
            .prepare(&f.request(&["local_one"], &[]))
            .unwrap()
            .preview
            .can_apply
    );
    fs::write(
        f.dir(&f.source).join("archived-sessions.idx"),
        r#"{"v":9,"archived":[]}"#,
    )
    .unwrap();
    assert!(f.store.prepare(&f.request(&["local_one"], &[])).is_err());
}

#[test]
fn changed_source_or_new_tombstone_invalidates_session_preview() {
    for tombstone in [true, false] {
        let f = Fixture::new();
        f.session("local_one", false);
        let request = f.request(&["local_one"], &[]);
        let token = f.store.prepare(&request).unwrap().preview.token;
        if tombstone {
            fs::write(f.dir(&f.target).join("deleted_one"), "1").unwrap();
        } else {
            f.session("local_one", true);
        }
        assert!(f.store.apply_checked(&request, &token, || Ok(())).is_err());
        assert!(!f.dir(&f.target).join("local_one.json").exists());
    }
}

#[test]
fn a_running_desktop_blocks_writes_and_partial_failure_returns_receipts() {
    let f = Fixture::new();
    f.session("local_a", false);
    f.session("local_b", false);
    let request = f.request(&["local_a", "local_b"], &[]);
    let token = f.store.prepare(&request).unwrap().preview.token;
    assert!(f
        .store
        .apply_checked(&request, &token, || Err("Desktop running".into()))
        .is_err());
    assert!(!f.dir(&f.target).join("local_a.json").exists());
    let calls = std::cell::Cell::new(0);
    let receipt = f
        .store
        .apply_checked(&request, &token, || {
            calls.set(calls.get() + 1);
            if calls.get() > 2 {
                Err("Desktop started".into())
            } else {
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(receipt.copied_sessions, ["local_a"]);
    assert!(receipt.error.is_some());
    assert!(!f.dir(&f.target).join("local_b.json").exists());
}
