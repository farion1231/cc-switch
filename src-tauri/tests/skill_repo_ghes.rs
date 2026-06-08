use std::sync::Arc;

use cc_switch_lib::{Database, SkillRepo};

#[path = "support.rs"]
mod support;
use support::{ensure_test_home, reset_test_fs, test_mutex};

// ── DAO: host + token 存取 ────────────────────────────────────────────────────

#[test]
fn save_and_get_skill_repo_preserves_host_and_token() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    db.save_skill_repo(&SkillRepo {
        host: "ghes.example.com".to_string(),
        owner: "my-org".to_string(),
        name: "my-skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: Some("ghp_secret123".to_string()),
    })
    .expect("save skill repo");

    let repos = db.get_skill_repos().expect("get skill repos");
    // memory() 会 init_default_skill_repos，过滤出我们插入的那条
    let saved = repos
        .iter()
        .find(|r| r.host == "ghes.example.com")
        .expect("should find GHES repo");

    assert_eq!(saved.owner, "my-org");
    assert_eq!(saved.name, "my-skills");
    assert_eq!(saved.token.as_deref(), Some("ghp_secret123"));
}

#[test]
fn save_skill_repo_without_token_stores_none() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    db.save_skill_repo(&SkillRepo {
        host: "github.com".to_string(),
        owner: "public-org".to_string(),
        name: "public-skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: None,
    })
    .expect("save skill repo");

    let repos = db.get_skill_repos().expect("get skill repos");
    let saved = repos
        .iter()
        .find(|r| r.owner == "public-org" && r.name == "public-skills")
        .expect("should find repo");

    assert!(
        saved.token.is_none(),
        "token should be None for public repo"
    );
}

// ── DAO: 多 host 共存 ─────────────────────────────────────────────────────────

#[test]
fn same_owner_name_different_hosts_coexist() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    let github = SkillRepo {
        host: "github.com".to_string(),
        owner: "acme".to_string(),
        name: "skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: None,
    };
    let ghes = SkillRepo {
        host: "ghes.internal.corp".to_string(),
        owner: "acme".to_string(),
        name: "skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: Some("pat-internal".to_string()),
    };

    db.save_skill_repo(&github).expect("save github repo");
    db.save_skill_repo(&ghes).expect("save ghes repo");

    let repos = db.get_skill_repos().expect("get skill repos");
    let acme_repos: Vec<_> = repos
        .iter()
        .filter(|r| r.owner == "acme" && r.name == "skills")
        .collect();

    assert_eq!(
        acme_repos.len(),
        2,
        "acme/skills on two different hosts should both exist"
    );

    let has_github = acme_repos.iter().any(|r| r.host == "github.com");
    let has_ghes = acme_repos.iter().any(|r| r.host == "ghes.internal.corp");
    assert!(has_github, "github.com entry should exist");
    assert!(has_ghes, "ghes.internal.corp entry should exist");
}

#[test]
fn insert_or_replace_only_replaces_same_host() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    db.save_skill_repo(&SkillRepo {
        host: "github.com".to_string(),
        owner: "org".to_string(),
        name: "repo".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: None,
    })
    .expect("save github repo");

    db.save_skill_repo(&SkillRepo {
        host: "ghes.example.com".to_string(),
        owner: "org".to_string(),
        name: "repo".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: Some("secret".to_string()),
    })
    .expect("save ghes repo");

    // Re-save github.com with a branch update
    db.save_skill_repo(&SkillRepo {
        host: "github.com".to_string(),
        owner: "org".to_string(),
        name: "repo".to_string(),
        branch: "develop".to_string(),
        enabled: true,
        token: None,
    })
    .expect("update github repo");

    let repos = db.get_skill_repos().expect("get skill repos");
    let org_repos: Vec<_> = repos
        .iter()
        .filter(|r| r.owner == "org" && r.name == "repo")
        .collect();

    assert_eq!(org_repos.len(), 2, "should still have two entries");

    let github_entry = org_repos
        .iter()
        .find(|r| r.host == "github.com")
        .expect("github entry");
    assert_eq!(
        github_entry.branch, "develop",
        "github.com entry should be updated"
    );

    let ghes_entry = org_repos
        .iter()
        .find(|r| r.host == "ghes.example.com")
        .expect("ghes entry");
    assert_eq!(
        ghes_entry.token.as_deref(),
        Some("secret"),
        "ghes entry should be untouched"
    );
}

// ── DAO: 三元主键删除 ─────────────────────────────────────────────────────────

#[test]
fn delete_skill_repo_requires_matching_host() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    db.save_skill_repo(&SkillRepo {
        host: "github.com".to_string(),
        owner: "org".to_string(),
        name: "skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: None,
    })
    .expect("save github repo");

    db.save_skill_repo(&SkillRepo {
        host: "ghes.example.com".to_string(),
        owner: "org".to_string(),
        name: "skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: Some("tok".to_string()),
    })
    .expect("save ghes repo");

    // 只删除 ghes 那条
    db.delete_skill_repo("ghes.example.com", "org", "skills")
        .expect("delete ghes repo");

    let repos = db.get_skill_repos().expect("get repos");
    let remaining: Vec<_> = repos
        .iter()
        .filter(|r| r.owner == "org" && r.name == "skills")
        .collect();

    assert_eq!(remaining.len(), 1, "only one entry should remain");
    assert_eq!(
        remaining[0].host, "github.com",
        "github.com entry should survive"
    );
}

#[test]
fn delete_with_wrong_host_leaves_repo_intact() {
    let db = Arc::new(Database::memory().expect("create memory db"));

    db.save_skill_repo(&SkillRepo {
        host: "ghes.example.com".to_string(),
        owner: "org".to_string(),
        name: "skills".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: None,
    })
    .expect("save repo");

    // 用错误的 host 删
    db.delete_skill_repo("github.com", "org", "skills")
        .expect("delete with wrong host should not error");

    let repos = db.get_skill_repos().expect("get repos");
    let found = repos
        .iter()
        .any(|r| r.host == "ghes.example.com" && r.owner == "org");
    assert!(found, "repo should still exist when host did not match");
}

// ── DB 迁移：v10 → v11 ────────────────────────────────────────────────────────

#[test]
fn v10_to_v11_migration_preserves_existing_repos_as_github_com() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let db_path = home.join(".cc-switch").join("cc-switch.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).expect("create dir");

    // 手动建一个 v10 风格的数据库（skill_repos PK 为 (owner, name)）
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open v10 db");
        conn.execute_batch(
            "PRAGMA user_version = 10;
             CREATE TABLE IF NOT EXISTS skill_repos (
                 owner   TEXT NOT NULL,
                 name    TEXT NOT NULL,
                 branch  TEXT NOT NULL DEFAULT 'main',
                 enabled BOOLEAN NOT NULL DEFAULT 1,
                 PRIMARY KEY (owner, name)
             );
             INSERT INTO skill_repos (owner, name, branch, enabled)
             VALUES ('old-org', 'legacy-skills', 'main', 1);",
        )
        .expect("create v10 schema");
    }

    // Database::init() 将触发 v10→v11 迁移
    let db = Arc::new(Database::init().expect("init db runs migration"));

    let repos = db.get_skill_repos().expect("get repos after migration");
    let migrated = repos
        .iter()
        .find(|r| r.owner == "old-org" && r.name == "legacy-skills")
        .expect("legacy repo should survive migration");

    assert_eq!(
        migrated.host, "github.com",
        "migrated repo should default to github.com"
    );
    assert!(
        migrated.token.is_none(),
        "migrated repo should have no token"
    );
}

#[test]
fn v10_to_v11_migration_enforces_three_part_pk_after_upgrade() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let db_path = home.join(".cc-switch").join("cc-switch.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).expect("create dir");

    // v10 数据库，有一条 org/repo 记录
    {
        let conn = rusqlite::Connection::open(&db_path).expect("open v10 db");
        conn.execute_batch(
            "PRAGMA user_version = 10;
             CREATE TABLE IF NOT EXISTS skill_repos (
                 owner TEXT NOT NULL, name TEXT NOT NULL,
                 branch TEXT NOT NULL DEFAULT 'main',
                 enabled BOOLEAN NOT NULL DEFAULT 1,
                 PRIMARY KEY (owner, name)
             );
             INSERT INTO skill_repos (owner, name) VALUES ('org', 'repo');",
        )
        .expect("seed v10 db");
    }

    let db = Arc::new(Database::init().expect("migrate to v11"));

    // 迁移后，同一 owner/name 不同 host 应能共存
    db.save_skill_repo(&SkillRepo {
        host: "ghes.example.com".to_string(),
        owner: "org".to_string(),
        name: "repo".to_string(),
        branch: "main".to_string(),
        enabled: true,
        token: Some("pat".to_string()),
    })
    .expect("add GHES repo after migration");

    let repos = db.get_skill_repos().expect("get repos");
    let org_repos: Vec<_> = repos
        .iter()
        .filter(|r| r.owner == "org" && r.name == "repo")
        .collect();

    assert_eq!(
        org_repos.len(),
        2,
        "after migration, same owner/name on different hosts should coexist"
    );
}
