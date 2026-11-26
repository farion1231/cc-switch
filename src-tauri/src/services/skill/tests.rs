use super::*;
use crate::test_support::EnvVars;
use tempfile::tempdir;

#[test]
fn skill_state_lock_allows_snapshots_but_excludes_writers() {
    let first_reader = skill_state_read_guard();
    let second_reader = skill_state_read_guard();
    assert!(
        skill_state_lock().try_write().is_err(),
        "a Skill mutation must wait for every snapshot reader"
    );

    drop(second_reader);
    drop(first_reader);
    assert!(skill_state_lock().try_write().is_ok());
}

#[test]
fn replace_dir_staged_keeps_installed_skill_when_copy_fails() {
    let root = tempdir().expect("tempdir");
    let dest = root.path().join("skills").join("demo");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "old").unwrap();

    let missing_source = root.path().join("missing");
    assert!(SkillService::replace_dir_staged(&missing_source, &dest).is_err());

    assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "old");
    let leftovers: Vec<_> = fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name != "demo")
        .collect();
    assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");
}

#[test]
fn replace_dir_staged_swaps_in_new_contents() {
    let root = tempdir().expect("tempdir");
    let source = root.path().join("source");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("SKILL.md"), "new").unwrap();
    fs::write(source.join("nested").join("a.txt"), "a").unwrap();
    let dest = root.path().join("skills").join("demo");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("SKILL.md"), "old").unwrap();
    fs::write(dest.join("stale.txt"), "stale").unwrap();

    SkillService::replace_dir_staged(&source, &dest).unwrap();

    assert_eq!(fs::read_to_string(dest.join("SKILL.md")).unwrap(), "new");
    assert_eq!(
        fs::read_to_string(dest.join("nested").join("a.txt")).unwrap(),
        "a"
    );
    assert!(!dest.join("stale.txt").exists());
    assert_eq!(fs::read_dir(dest.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn refresh_local_skill_metadata_updates_stale_snapshot() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let ssot = tempdir().expect("ssot tempdir");
    let dir_name = "demo-skill";
    let skill_dir = ssot.path().join(dir_name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: old snapshot\n---\nbody\n",
    )
    .unwrap();

    let installed = InstalledSkill {
        id: format!("local:{dir_name}"),
        name: "demo".to_string(),
        description: Some("old snapshot".to_string()),
        directory: dir_name.to_string(),
        repo_host: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps::only(&AppType::Claude),
        installed_at: 1,
        content_hash: Some("stale-hash".to_string()),
        updated_at: 0,
    };
    db.save_skill(&installed).unwrap();

    // 缓存哈希失配但文件未再变：回写真实哈希，description 保持原值
    let refreshed = SkillService::refresh_local_skill_metadata_in(ssot.path(), &db).unwrap();
    assert_eq!(refreshed, 1);
    let after = db.get_installed_skill(&installed.id).unwrap().unwrap();
    assert_eq!(after.description.as_deref(), Some("old snapshot"));
    assert_ne!(after.content_hash.as_deref(), Some("stale-hash"));
    let healed_hash = after.content_hash.clone();

    // 手动编辑 SKILL.md：description 随哈希一起刷新
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: edited externally\n---\nbody\n",
    )
    .unwrap();
    let refreshed = SkillService::refresh_local_skill_metadata_in(ssot.path(), &db).unwrap();
    assert_eq!(refreshed, 1);
    let after = db.get_installed_skill(&installed.id).unwrap().unwrap();
    assert_eq!(after.description.as_deref(), Some("edited externally"));
    assert_ne!(after.content_hash, healed_hash);

    // 幂等：无变化时不再刷新
    let refreshed = SkillService::refresh_local_skill_metadata_in(ssot.path(), &db).unwrap();
    assert_eq!(refreshed, 0);
}

#[test]
fn refresh_local_skill_metadata_skips_repo_skills() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let ssot = tempdir().expect("ssot tempdir");
    let dir_name = "repo-skill";
    let skill_dir = ssot.path().join(dir_name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: repo\ndescription: edited externally\n---\nbody\n",
    )
    .unwrap();

    let installed = InstalledSkill {
        id: format!("repo:{dir_name}"),
        name: "repo".to_string(),
        description: Some("old snapshot".to_string()),
        directory: dir_name.to_string(),
        repo_host: None,
        repo_owner: Some("owner".to_string()),
        repo_name: Some("repo".to_string()),
        repo_branch: Some("main".to_string()),
        readme_url: None,
        apps: SkillApps::only(&AppType::Claude),
        installed_at: 1,
        content_hash: Some("repo-hash".to_string()),
        updated_at: 0,
    };
    db.save_skill(&installed).unwrap();

    let refreshed = SkillService::refresh_local_skill_metadata_in(ssot.path(), &db).unwrap();
    assert_eq!(refreshed, 0);
    let after = db.get_installed_skill(&installed.id).unwrap().unwrap();
    assert_eq!(after.description.as_deref(), Some("old snapshot"));
    assert_eq!(after.content_hash.as_deref(), Some("repo-hash"));
}

#[test]
fn refresh_local_skill_metadata_discards_candidates_changed_during_scan() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let ssot = tempdir().expect("ssot tempdir");
    let dir_name = "demo-skill";
    let skill_dir = ssot.path().join(dir_name);
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo\ndescription: edited externally\n---\nbody\n",
    )
    .unwrap();

    let installed = InstalledSkill {
        id: format!("local:{dir_name}"),
        name: "demo".to_string(),
        description: Some("old snapshot".to_string()),
        directory: dir_name.to_string(),
        repo_host: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps::only(&AppType::Claude),
        installed_at: 1,
        content_hash: Some("stale-hash".to_string()),
        updated_at: 0,
    };
    db.save_skill(&installed).unwrap();

    let candidates =
        SkillService::collect_local_skill_metadata_candidates(ssot.path(), &db).unwrap();
    assert_eq!(candidates.len(), 1);

    // 扫描完成后、写回之前发生了重装/并发更新：旧快照必须被丢弃。
    let mut concurrent = db.get_installed_skill(&installed.id).unwrap().unwrap();
    concurrent.description = Some("newer value".to_string());
    concurrent.content_hash = Some("newer-hash".to_string());
    concurrent.installed_at = 2;
    db.save_skill(&concurrent).unwrap();

    let _state_guard = skill_state_write_guard();
    let refreshed = SkillService::apply_local_skill_metadata_candidates(&db, candidates).unwrap();
    assert_eq!(refreshed, 0);
    let after = db.get_installed_skill(&installed.id).unwrap().unwrap();
    assert_eq!(after.description.as_deref(), Some("newer value"));
    assert_eq!(after.content_hash.as_deref(), Some("newer-hash"));
    assert_eq!(after.installed_at, 2);
}

/// 构造一个模拟 GitHub 归档的 ZIP：带一层 `repo-main/` 根目录，
/// 其中掺入用 `../` 逃逸的恶意条目。
///
/// 两个恶意条目走的是**不同**的拦截层，缺一不可：
/// - 两级 `../../`：净深度为负，`enclosed_name()` 自己就会拒绝；
/// - 一级 `../`：净深度非负，`enclosed_name()` **放行**且原样保留 `..`，
///   只有剥掉 root_name 之后的组件校验才能拦住。
fn build_zip_with_traversal_entry() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();

        // 合法条目：会被正常解压
        zip.start_file("repo-main/SKILL.md", opts).unwrap();
        zip.write_all(b"---\nname: ok\n---\n").unwrap();

        // 恶意条目 A：被 enclosed_name() 拒绝
        zip.start_file("repo-main/../../escaped.txt", opts).unwrap();
        zip.write_all(b"pwned").unwrap();

        // 恶意条目 B：能通过 enclosed_name()，靠组件校验拦截
        zip.start_file("repo-main/../escaped-one-level.txt", opts)
            .unwrap();
        zip.write_all(b"pwned").unwrap();

        zip.finish().unwrap();
    }
    buf
}

#[test]
fn validate_repo_ref_accepts_real_world_coordinates() {
    // 合法分支名允许 `/`，不能因为防穿越就把它们一起禁掉
    for branch in [
        "main",
        "master",
        "HEAD",
        "feature/new-thing",
        "release/v1.2.3",
        "fix-123",
        "user.name/topic",
    ] {
        assert!(
            SkillService::validate_repo_ref("farion1231", "cc-switch", branch).is_ok(),
            "must accept branch: {branch:?}"
        );
    }
    assert!(SkillService::validate_repo_ref("a", "b.c_d-e", "main").is_ok());
}

#[test]
fn validate_repo_ref_accepts_the_empty_branch_sentinel() {
    // 空 branch 与 "HEAD" 在 download_repo 里是同一个哨兵：分支候选表跳过
    // 两者，改试 main / master，所以它们从不进 URL。校验若把空串当非法，
    // 存量 skill_repos 行（建表默认 'main'，但空串没被禁）会在 download_repo
    // 第一行就 INVALID_REPO_REF，整个技能面板列不出东西——前端两处
    // `repo.branch || "main"` 正是照着"空串可用"写的。
    assert!(
        SkillService::validate_repo_ref("farion1231", "cc-switch", "").is_ok(),
        "the empty-branch sentinel must stay usable"
    );
}

#[test]
fn validate_repo_ref_rejects_url_hijacking_branches() {
    // 这是核心用例：branch 被拼进 archive URL，URL 解析会消解点段，
    // 落点会从 /archive/refs/heads/ 改写成攻击者可上传的 release asset。
    for branch in [
        "../../../releases/download/v1/evil",
        "..",
        "../x",
        "a/../../b",
        "a/./b",
        "..\\..\\releases\\download\\v1\\evil",
        "/leading",
        "trailing/",
        "double//slash",
        "with space",
        "frag#ment",
        "pct%2e%2e",
        "ref@{0}",
        "seg.lock",
        ".hidden/x",
    ] {
        assert!(
            SkillService::validate_repo_ref("owner", "repo", branch).is_err(),
            "must reject branch: {branch:?}"
        );
    }
    for (owner, name) in [
        ("..", "repo"),
        ("own/er", "repo"),
        ("owner", ".."),
        ("owner", "re/po"),
        ("owner", "re po"),
        ("", "repo"),
        ("owner", ""),
    ] {
        assert!(
            SkillService::validate_repo_ref(owner, name, "main").is_err(),
            "must reject coordinates: {owner:?}/{name:?}"
        );
    }
}

fn branch_fallback_repo(branch: &str) -> SkillRepo {
    SkillRepo {
        host: default_repo_host(),
        owner: "acme".to_string(),
        name: "skills".to_string(),
        branch: branch.to_string(),
        enabled: true,
    }
}

fn http404_error() -> anyhow::Error {
    anyhow!(format_skill_error(
        "DOWNLOAD_FAILED",
        &[("status", "404")],
        Some("http404"),
    ))
}

/// 按分支名返回预设结果，并记下实际尝试过的分支。
async fn run_branch_fallback(
    repo: &SkillRepo,
    mut outcome: impl FnMut(&str) -> Result<()>,
) -> (Result<String>, Vec<String>) {
    let temp = tempdir().unwrap();
    let mut attempted = Vec::new();
    let platform = SkillService::default_platform(&repo.host);
    let result = SkillService::try_repo_branches(repo, platform, false, temp.path(), |url| {
        let branch = url
            .rsplit('/')
            .next()
            .and_then(|file| file.strip_suffix(".zip"))
            .unwrap()
            .to_string();
        let result = outcome(&branch);
        attempted.push(branch);
        async move { result }
    })
    .await;
    (result, attempted)
}

fn error_json(error: &anyhow::Error) -> serde_json::Value {
    serde_json::from_str(&error.to_string()).expect("structured skill error")
}

#[tokio::test]
async fn branch_fallback_keeps_non_404_error_from_main() {
    let repo = branch_fallback_repo("main");

    // 超过下载上限：不再去试 master，原始错误原样报出
    let (result, attempted) = run_branch_fallback(&repo, |branch| match branch {
        "main" => Err(anyhow!(format_skill_error(
            "ARCHIVE_DOWNLOAD_TOO_LARGE",
            &[("limit_mb", "512")],
            None,
        ))),
        _ => Err(http404_error()),
    })
    .await;
    assert_eq!(attempted, ["main"]);
    let err = error_json(&result.unwrap_err());
    assert_eq!(err["code"], "ARCHIVE_DOWNLOAD_TOO_LARGE");
    assert_eq!(err["context"]["limit_mb"], "512");

    // 超时（结构化与 reqwest 那种非结构化两种形状）同样不被 master 的 404 盖掉
    let (result, attempted) = run_branch_fallback(&repo, |branch| match branch {
        "main" => Err(anyhow!(format_skill_error(
            "DOWNLOAD_TIMEOUT",
            &[("timeout", "30")],
            Some("checkNetwork"),
        ))),
        _ => Err(http404_error()),
    })
    .await;
    assert_eq!(attempted, ["main"]);
    assert_eq!(error_json(&result.unwrap_err())["code"], "DOWNLOAD_TIMEOUT");

    let (result, attempted) = run_branch_fallback(&repo, |branch| match branch {
        "main" => Err(anyhow!("operation timed out")),
        _ => Err(http404_error()),
    })
    .await;
    assert_eq!(attempted, ["main"]);
    assert_eq!(result.unwrap_err().to_string(), "operation timed out");
}

#[tokio::test]
async fn stalled_download_stops_branch_fallback_and_carries_repo_coordinates() {
    // 下载停滞换个分支也不会好：不去试 master，停滞原因原样报出并补上坐标
    let (result, attempted) =
        run_branch_fallback(&branch_fallback_repo("main"), |branch| match branch {
            "main" => Err(SkillService::download_stalled_error()),
            _ => Err(http404_error()),
        })
        .await;
    assert_eq!(attempted, ["main"]);
    let err = error_json(&result.unwrap_err());
    assert_eq!(err["code"], "DOWNLOAD_STALLED");
    assert_eq!(err["context"]["owner"], "acme");
    assert_eq!(err["context"]["name"], "skills");
    assert_eq!(err["context"]["timeout"], "60");
    assert_eq!(err["suggestion"], "checkNetwork");
}

#[test]
fn total_download_timeout_reports_the_real_ceiling() {
    let err: serde_json::Value = serde_json::from_str(&SkillService::download_timeout_error(
        &branch_fallback_repo("main"),
    ))
    .expect("structured skill error");
    assert_eq!(err["code"], "DOWNLOAD_TIMEOUT");
    assert_eq!(
        err["context"]["timeout"],
        DOWNLOAD_TOTAL_TIMEOUT.as_secs().to_string()
    );
}

#[tokio::test]
async fn branch_fallback_moves_on_only_after_404() {
    // main 404、master 成功：装得上，且记下真实分支
    let (result, attempted) =
        run_branch_fallback(&branch_fallback_repo("main"), |branch| match branch {
            "master" => Ok(()),
            _ => Err(http404_error()),
        })
        .await;
    assert_eq!(result.unwrap(), "master");
    assert_eq!(attempted, ["main", "master"]);

    // 自定义分支 404 时照旧回落到 main
    let (result, attempted) =
        run_branch_fallback(&branch_fallback_repo("dev"), |branch| match branch {
            "main" => Ok(()),
            _ => Err(http404_error()),
        })
        .await;
    assert_eq!(result.unwrap(), "main");
    assert_eq!(attempted, ["dev", "main"]);
}

#[tokio::test]
async fn branch_fallback_error_carries_repo_coordinates() {
    // 只有 master 的仓库在 master 上出错：报的是 master 的错误，
    // 且补上前端文案插值要用的 owner / name
    let (result, attempted) =
        run_branch_fallback(&branch_fallback_repo("main"), |branch| match branch {
            "master" => Err(anyhow!(format_skill_error(
                "DOWNLOAD_TIMEOUT",
                &[("timeout", "30")],
                Some("checkNetwork"),
            ))),
            _ => Err(http404_error()),
        })
        .await;
    assert_eq!(attempted, ["main", "master"]);
    let err = error_json(&result.unwrap_err());
    assert_eq!(err["code"], "DOWNLOAD_TIMEOUT");
    assert_eq!(err["context"]["owner"], "acme");
    assert_eq!(err["context"]["name"], "skills");
    assert_eq!(err["context"]["timeout"], "30");
    assert_eq!(err["suggestion"], "checkNetwork");

    // 全部 404：报最后一个 404，同样带坐标
    let (result, attempted) =
        run_branch_fallback(&branch_fallback_repo("main"), |_| Err(http404_error())).await;
    assert_eq!(attempted, ["main", "master"]);
    let err = error_json(&result.unwrap_err());
    assert_eq!(err["code"], "DOWNLOAD_FAILED");
    assert_eq!(err["context"]["status"], "404");
    assert_eq!(err["context"]["owner"], "acme");
}

#[test]
fn assert_github_archive_url_pins_host_and_path() {
    let ok = "https://github.com/owner/repo/archive/refs/heads/main.zip";
    assert!(SkillService::assert_github_archive_url(ok, "owner", "repo").is_ok());

    // 出口断言必须挡住落点被改写到 release asset 的情况
    for bad in [
        "https://github.com/owner/repo/releases/download/v1/evil.zip",
        "https://evil.example/owner/repo/archive/refs/heads/main.zip",
        "http://github.com/owner/repo/archive/refs/heads/main.zip",
        "https://github.com/other/repo/archive/refs/heads/main.zip",
    ] {
        assert!(
            SkillService::assert_github_archive_url(bad, "owner", "repo").is_err(),
            "must reject url: {bad}"
        );
    }
}

fn gitlab_repo(host: &str, owner: &str, name: &str, branch: &str) -> SkillRepo {
    SkillRepo {
        host: host.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
        branch: branch.to_string(),
        enabled: true,
    }
}

#[test]
fn validate_repo_accepts_real_world_gitlab_coordinates() {
    for (host, owner, name) in [
        ("gitlab.com", "gitlab-org", "gitlab"),
        // 嵌套分组
        ("gitlab.com", "gitlab-org/api", "client-go"),
        // 自建实例：带端口、裸 IP、分组名含下划线和点（#8389 报告人的坐标）
        (
            "gitlab.example.com:8443",
            "team.platform/ai_tools",
            "skills",
        ),
        ("10.22.36.105:8081", "me_release", "SkillList"),
    ] {
        assert!(
            SkillService::validate_repo(&gitlab_repo(host, owner, name, "main")).is_ok(),
            "must accept {host}/{owner}/{name}"
        );
    }
}

#[test]
fn validate_repo_rejects_gitlab_host_and_path_injection() {
    for (host, owner, name, branch) in [
        // 主机里夹带路径、用户信息、查询、片段
        ("gitlab.example.com/evil", "group", "skills", "main"),
        ("user@gitlab.example.com", "group", "skills", "main"),
        ("gitlab.example.com?x=1", "group", "skills", "main"),
        ("gitlab.example.com#x", "group", "skills", "main"),
        ("https://gitlab.example.com", "group", "skills", "main"),
        ("", "group", "skills", "main"),
        // 入库前统一成小写，校验不认大写
        ("GitLab.example.com", "group", "skills", "main"),
        // 端口不是数字
        ("gitlab.example.com:8443x", "group", "skills", "main"),
        // 借端口绕开 GitHub 的规则
        ("github.com:443", "group/sub", "skills", "main"),
        // 分组路径
        ("gitlab.example.com", "../group", "skills", "main"),
        ("gitlab.example.com", "group//sub", "skills", "main"),
        ("gitlab.example.com", "group/", "skills", "main"),
        ("gitlab.example.com", "-group", "skills", "main"),
        ("gitlab.example.com", "group%2Fx", "skills", "main"),
        // 仓库名、分支沿用既有规则
        ("gitlab.example.com", "group", "..", "main"),
        ("gitlab.example.com", "group", "skills", "../../../issues"),
    ] {
        assert!(
            SkillService::validate_repo(&gitlab_repo(host, owner, name, branch)).is_err(),
            "must reject {host:?} {owner:?} {name:?} {branch:?}"
        );
    }
    // GitHub 的规则不因为多了 GitLab 而放宽：owner 里仍不许有 `/`
    assert!(
        SkillService::validate_repo(&gitlab_repo(GITHUB_HOST, "group/sub", "skills", "main"))
            .is_err()
    );
}

#[test]
fn build_archive_url_targets_the_gitlab_api_on_the_configured_host() {
    let repo = gitlab_repo(
        "gitlab.example.com:8443",
        "group/subgroup",
        "skills",
        "main",
    );
    // 项目路径是一段，`/` 编码成 %2F；分支在查询参数里，带 `/` 也不会混进路径
    assert_eq!(
        SkillService::build_archive_url(&repo, RepoPlatform::GitLab, "feature/env", false).unwrap(),
        "https://gitlab.example.com:8443/api/v4/projects/group%2Fsubgroup%2Fskills/repository/archive.zip?sha=feature%2Fenv"
    );
    // 分支名允许 `&` `=`，不能让它们变成额外的查询参数
    assert_eq!(
        SkillService::build_archive_url(&repo, RepoPlatform::GitLab, "a&ref=b", false).unwrap(),
        "https://gitlab.example.com:8443/api/v4/projects/group%2Fsubgroup%2Fskills/repository/archive.zip?sha=a%26ref%3Db"
    );
    // GitHub 的地址不变
    assert_eq!(
        SkillService::build_archive_url(
            &branch_fallback_repo("main"),
            RepoPlatform::GitHub,
            "main",
            false,
        )
        .unwrap(),
        "https://github.com/acme/skills/archive/refs/heads/main.zip"
    );
}

#[test]
fn authenticated_github_downloads_go_through_the_api_zipball() {
    let repo = branch_fallback_repo("main");
    assert_eq!(
        SkillService::build_archive_url(&repo, RepoPlatform::GitHub, "feature/x", true).unwrap(),
        "https://api.github.com/repos/acme/skills/zipball/refs/heads/feature/x"
    );
    // GitLab、Gitea 带不带令牌都是同一个 API 地址
    let gitlab = gitlab_repo("gitlab.example.com", "group", "skills", "main");
    assert_eq!(
        SkillService::build_archive_url(&gitlab, RepoPlatform::GitLab, "main", true).unwrap(),
        SkillService::build_archive_url(&gitlab, RepoPlatform::GitLab, "main", false).unwrap(),
    );

    let ok = "https://api.github.com/repos/acme/skills/zipball/refs/heads/main";
    assert!(SkillService::assert_github_api_archive_url(ok, "acme", "skills").is_ok());
    for bad in [
        "http://api.github.com/repos/acme/skills/zipball/main",
        "https://github.com/repos/acme/skills/zipball/main",
        "https://api.github.com:8443/repos/acme/skills/zipball/main",
        "https://evil@api.github.com/repos/acme/skills/zipball/main",
        "https://api.github.com/repos/acme/other/zipball/main",
        "https://api.github.com/repos/acme/skills/tarball/main",
        "https://api.github.com/repos/acme/skills/zipball/main?x=1",
        // 不带 refs/heads/ 的会命中同名 tag
        "https://api.github.com/repos/acme/skills/zipball/main",
    ] {
        assert!(
            SkillService::assert_github_api_archive_url(bad, "acme", "skills").is_err(),
            "{bad}"
        );
    }
}

#[test]
fn download_failure_hints_separate_missing_tokens_from_rejected_ones() {
    let hint = SkillService::download_failure_hint;
    // 不带令牌：404 提示私有仓库要设令牌，GitHub 的 403 仍按限流说
    assert_eq!(hint("404", false, true), "http404");
    assert_eq!(hint("404", false, false), "http404");
    assert_eq!(hint("403", false, true), "http403");
    assert_eq!(hint("403", false, false), "httpAuthRequired");
    assert_eq!(hint("401", false, false), "httpAuthRequired");
    // 带了令牌：问题在令牌
    assert_eq!(hint("401", true, false), "http401Token");
    assert_eq!(hint("403", true, true), "http403Token");
    assert_eq!(hint("404", true, false), "http404Token");
    assert_eq!(hint("429", true, true), "http429");
    assert_eq!(hint("500", false, true), "checkNetwork");
}

#[tokio::test]
async fn set_and_delete_repo_token_reprobe_the_platform() {
    let dir = tempfile::tempdir().unwrap();
    let (store, secrets) = crate::services::skill_repo_tokens::tests::memory_store(dir.path());
    let service = SkillService::new().with_repo_tokens(store);
    service
        .platform_cache
        .lock_recover()
        .insert("git.example.com".into(), RepoPlatform::GitLab);
    let cached = |host: &str| (host.to_string(), "o".into(), "n".into(), "main".into());
    for host in ["git.example.com", "other.example.com"] {
        service
            .discovery_cache
            .lock_recover()
            .insert(cached(host), (Instant::now(), Vec::new()));
    }
    // 匿名探测还在途中时保存了令牌
    let stale_epoch = service.credential_epoch.load(Ordering::SeqCst);

    let host = service
        .set_repo_token("Git.Example.com", "tok_123")
        .unwrap();
    assert_eq!(host, "git.example.com");
    assert!(!service.platform_cache.lock_recover().contains_key(&host));
    // 这台主机的发现缓存作废，别的主机不动
    {
        let discovery = service.discovery_cache.lock_recover();
        assert!(!discovery.contains_key(&cached("git.example.com")));
        assert!(discovery.contains_key(&cached("other.example.com")));
    }
    // 按旧身份得出的探测结论不入缓存，新的照常记
    service.remember_platform(&host, RepoPlatform::GitLab, stale_epoch);
    assert!(!service.platform_cache.lock_recover().contains_key(&host));
    let epoch = service.credential_epoch.load(Ordering::SeqCst);
    service.remember_platform(&host, RepoPlatform::Gitea, epoch);
    assert_eq!(
        service.platform_cache.lock_recover().get(&host).copied(),
        Some(RepoPlatform::Gitea)
    );
    assert_eq!(
        service.repo_token(&host).await.unwrap().as_deref(),
        Some("tok_123")
    );
    assert_eq!(service.repo_token("github.com").await.unwrap(), None);
    assert_eq!(
        service.repo_token_list().unwrap().hosts,
        vec!["git.example.com".to_string()]
    );

    service
        .platform_cache
        .lock_recover()
        .insert(host.clone(), RepoPlatform::Gitea);
    service.delete_repo_token(&host).unwrap();
    assert!(!service.platform_cache.lock_recover().contains_key(&host));
    assert_eq!(service.repo_token(&host).await.unwrap(), None);
    assert!(secrets.entries.lock_recover().is_empty());
}

#[tokio::test]
async fn branch_fallback_walks_gitlab_branches_on_404() {
    let repo = gitlab_repo("gitlab.com", "group/sub", "skills", "develop");
    let temp = tempdir().unwrap();
    let mut attempted = Vec::new();
    let result =
        SkillService::try_repo_branches(&repo, RepoPlatform::GitLab, false, temp.path(), |url| {
            let found = url.ends_with("?sha=master");
            attempted.push(url);
            async move {
                if found {
                    Ok(())
                } else {
                    Err(http404_error())
                }
            }
        })
        .await;
    assert_eq!(result.unwrap(), "master");
    let prefix = "https://gitlab.com/api/v4/projects/group%2Fsub%2Fskills/repository/archive.zip";
    assert_eq!(
        attempted,
        [
            format!("{prefix}?sha=develop"),
            format!("{prefix}?sha=main"),
            format!("{prefix}?sha=master"),
        ]
    );
}

#[test]
fn gitlab_doc_url_round_trips_through_extract_doc_path() {
    // 分组名恰好叫 blob：只找 `/blob/` 会切在分组路径上
    let url = SkillService::build_skill_doc_url(
        RepoPlatform::GitLab,
        "gitlab.example.com",
        "blob/tools",
        "skills",
        "main",
        "demo/SKILL.md",
    )
    .unwrap();
    assert_eq!(
        url,
        "https://gitlab.example.com/blob/tools/skills/-/blob/main/demo/SKILL.md"
    );
    assert_eq!(
        SkillService::extract_doc_path_from_url(&url).as_deref(),
        Some("demo/SKILL.md")
    );
    assert!(SkillService::build_skill_doc_url(
        RepoPlatform::GitLab,
        "gitlab.example.com/evil",
        "group",
        "skills",
        "main",
        "x"
    )
    .is_none());
}

#[test]
fn platform_probe_only_treats_a_version_json_as_gitea() {
    use RepoPlatform::{GitLab, Gitea};
    let classify =
        |status: u16, body: &[u8]| SkillService::classify_platform_probe(status, body, false);
    // Gitea 与 Forgejo（Codeberg）的真实回答
    assert_eq!(classify(200, br#"{"version":"1.22.3"}"#), Some(Gitea));
    assert_eq!(
        classify(200, br#"{"version":"9.0.3+gitea-1.22.0"}"#),
        Some(Gitea)
    );
    // GitLab 没有这个地址；自建实例前面的网关还可能对任何地址都回 200 的登录页
    assert_eq!(classify(404, br#"{"error":"404 Not Found"}"#), Some(GitLab));
    assert_eq!(classify(401, b""), Some(GitLab));
    assert_eq!(
        classify(200, b"<!DOCTYPE html><html>Sign in</html>"),
        Some(GitLab)
    );
    assert_eq!(classify(200, br#"{"version":17}"#), Some(GitLab));
    assert_eq!(classify(200, br#"{"message":"ok"}"#), Some(GitLab));
    // 没问到就不下结论：限流和服务端错误不能把一个 Gitea 主机记成 GitLab
    assert_eq!(classify(429, b""), None);
    assert_eq!(classify(502, br#"{"version":"1.22.3"}"#), None);
    // 超过读取上限的响应体不解析
    let mut oversized = br#"{"version":"1","pad":""#.to_vec();
    oversized.resize(PLATFORM_PROBE_MAX_BODY_BYTES + 1, b'a');
    oversized.extend_from_slice(br#""}"#);
    assert_eq!(classify(200, &oversized), Some(GitLab));

    // 带令牌（#9666）：401 只说明令牌被拒，不下结论；令牌有效时照常判
    let with_token =
        |status: u16, body: &[u8]| SkillService::classify_platform_probe(status, body, true);
    assert_eq!(with_token(401, b""), None);
    assert_eq!(with_token(200, br#"{"version":"1.22.3"}"#), Some(Gitea));
    assert_eq!(with_token(404, b""), Some(GitLab));
}

#[tokio::test]
async fn platform_lookup_never_probes_github_and_reuses_what_it_learned() {
    let service = SkillService::new();
    // github.com 不联网直接定；没探过的主机拼文档链接时按 GitLab
    assert_eq!(
        service.resolve_platform(GITHUB_HOST, None, 0).await,
        RepoPlatform::GitHub
    );
    assert_eq!(service.known_platform(GITHUB_HOST), RepoPlatform::GitHub);
    assert_eq!(
        service.known_platform("git.example.com"),
        RepoPlatform::GitLab
    );
    // 不合法的主机不发请求
    assert_eq!(
        service
            .resolve_platform("git.example.com/evil", None, 0)
            .await,
        RepoPlatform::GitLab
    );

    service
        .platform_cache
        .lock_recover()
        .insert("git.example.com".to_string(), RepoPlatform::Gitea);
    assert_eq!(
        service.resolve_platform("git.example.com", None, 0).await,
        RepoPlatform::Gitea
    );
    assert_eq!(
        service.known_platform("git.example.com"),
        RepoPlatform::Gitea
    );
}

#[test]
fn build_archive_url_targets_the_gitea_api() {
    let build = |repo: &SkillRepo, branch: &str| {
        SkillService::build_archive_url(repo, RepoPlatform::Gitea, branch, false)
    };
    let repo = gitlab_repo("git.example.com:3000", "team", "skills", "main");
    assert_eq!(
        build(&repo, "main").unwrap(),
        "https://git.example.com:3000/api/v1/repos/team/skills/archive/main.zip"
    );
    // 分支里的 `/` 是路径分隔；`&` `=` 留在路径里，变不成查询参数
    assert_eq!(
        build(&repo, "feature/env").unwrap(),
        "https://git.example.com:3000/api/v1/repos/team/skills/archive/feature/env.zip"
    );
    assert_eq!(
        build(&repo, "a&ref=b").unwrap(),
        "https://git.example.com:3000/api/v1/repos/team/skills/archive/a&ref=b.zip"
    );
    // 点段会被 URL 解析吃掉、把落点挪到别的接口上：出口断言拦住
    assert!(build(&repo, "../../../../user").is_err());
    // Gitea 的 owner 只有一段，GitLab 式的嵌套分组不是合法坐标
    let nested = gitlab_repo("git.example.com", "group/sub", "skills", "main");
    assert!(build(&nested, "main").is_err());
    // 主机才是准绳：github.com 不会因为传错平台就去打 Gitea 的接口
    assert_eq!(
        build(&branch_fallback_repo("main"), "main").unwrap(),
        "https://github.com/acme/skills/archive/refs/heads/main.zip"
    );
}

#[tokio::test]
async fn branch_fallback_walks_gitea_branches_on_404() {
    let repo = gitlab_repo("codeberg.org", "team", "skills", "develop");
    let temp = tempdir().unwrap();
    let mut attempted = Vec::new();
    let result =
        SkillService::try_repo_branches(&repo, RepoPlatform::Gitea, false, temp.path(), |url| {
            let found = url.ends_with("/master.zip");
            attempted.push(url);
            async move {
                if found {
                    Ok(())
                } else {
                    Err(http404_error())
                }
            }
        })
        .await;
    assert_eq!(result.unwrap(), "master");
    let prefix = "https://codeberg.org/api/v1/repos/team/skills/archive";
    assert_eq!(
        attempted,
        [
            format!("{prefix}/develop.zip"),
            format!("{prefix}/main.zip"),
            format!("{prefix}/master.zip"),
        ]
    );
}

#[test]
fn gitea_doc_url_round_trips_through_extract_doc_path() {
    // 仓库里恰好有个 blob 目录：标记要取最靠前的 `/src/branch/`，不是后面的 `/blob/`
    let url = SkillService::build_skill_doc_url(
        RepoPlatform::Gitea,
        "git.example.com:3000",
        "team",
        "skills",
        "main",
        "tools/blob/demo/SKILL.md",
    )
    .unwrap();
    assert_eq!(
        url,
        "https://git.example.com:3000/team/skills/src/branch/main/tools/blob/demo/SKILL.md"
    );
    assert_eq!(
        SkillService::extract_doc_path_from_url(&url).as_deref(),
        Some("tools/blob/demo/SKILL.md")
    );
    // 标记按位置认，不在整条地址里搜：GitHub 仓库里有 src/branch 目录、
    // 仓库名或 owner 恰好叫 tree / blob，都不能切错
    for (url, expected) in [
        (
            "https://github.com/o/r/blob/main/src/branch/demo/SKILL.md",
            "src/branch/demo/SKILL.md",
        ),
        (
            "https://github.com/acme/tree/blob/main/tools/demo/SKILL.md",
            "tools/demo/SKILL.md",
        ),
        (
            "https://github.com/blob/tree/tree/main/demo/SKILL.md",
            "demo/SKILL.md",
        ),
        (
            "https://git.example.com/team/blob/src/branch/main/demo/SKILL.md",
            "demo/SKILL.md",
        ),
        // 不带 `/-/` 的老式 GitLab 链接仍按原来的搜法
        (
            "https://gitlab.example.com/group/sub/skills/blob/main/demo/SKILL.md",
            "demo/SKILL.md",
        ),
    ] {
        assert_eq!(
            SkillService::extract_doc_path_from_url(url).as_deref(),
            Some(expected),
            "{url}"
        );
    }
    assert_eq!(
        SkillService::extract_doc_path_from_url("https://github.com/o/r"),
        None
    );
}

#[test]
fn doc_url_platform_falls_back_to_the_shape_of_the_discovered_link() {
    let skill = |host: &str, readme_url: &str| DiscoverableSkill {
        key: "team/skills:demo".to_string(),
        name: "demo".to_string(),
        description: String::new(),
        directory: "demo".to_string(),
        readme_url: Some(readme_url.to_string()),
        repo_host: host.to_string(),
        repo_owner: "team".to_string(),
        repo_name: "skills".to_string(),
        repo_branch: "main".to_string(),
    };
    let gitea_link = "https://git.example.com/team/skills/src/branch/main/demo/SKILL.md";
    let gitlab_link = "https://git.example.com/team/skills/-/blob/main/demo/SKILL.md";
    let service = SkillService::new();

    // 重启后还没探测过：安装跳过下载时，按发现阶段的链接形状定
    assert_eq!(
        service.platform_for_doc_url(&skill("git.example.com", gitea_link)),
        RepoPlatform::Gitea
    );
    assert_eq!(
        service.platform_for_doc_url(&skill("git.example.com", gitlab_link)),
        RepoPlatform::GitLab
    );
    // 链接指向别的仓库或别的主机，不算数
    assert_eq!(
        service.platform_for_doc_url(&skill(
            "git.example.com",
            "https://git.example.com/other/skills/src/branch/main/demo/SKILL.md"
        )),
        RepoPlatform::GitLab
    );
    assert_eq!(
        service.platform_for_doc_url(&skill(
            GITHUB_HOST,
            "https://github.com/team/skills/src/branch/main/demo/SKILL.md"
        )),
        RepoPlatform::GitHub
    );
    // 探测过的以探测结果为准
    service
        .platform_cache
        .lock_recover()
        .insert("git.example.com".to_string(), RepoPlatform::GitLab);
    assert_eq!(
        service.platform_for_doc_url(&skill("git.example.com", gitea_link)),
        RepoPlatform::GitLab
    );
}

#[test]
fn skill_repo_without_host_deserializes_as_github() {
    // 旧前端、旧 config.json、深链接都不带 host
    let repo: SkillRepo =
        serde_json::from_str(r#"{"owner":"o","name":"r","branch":"main","enabled":true}"#).unwrap();
    assert!(repo.is_github());
}

#[test]
fn build_skill_doc_url_drops_illegal_coordinates() {
    assert_eq!(
        SkillService::build_skill_doc_url(
            RepoPlatform::GitHub,
            GITHUB_HOST,
            "owner",
            "repo",
            "main",
            "a/SKILL.md"
        )
        .as_deref(),
        Some("https://github.com/owner/repo/blob/main/a/SKILL.md")
    );
    // readme_url 会被前端 openExternal 直接打开，非法坐标不得产出链接
    assert!(SkillService::build_skill_doc_url(
        RepoPlatform::GitHub,
        GITHUB_HOST,
        "owner",
        "repo",
        "../../../issues",
        "x"
    )
    .is_none());
}

#[test]
fn copy_entry_within_budget_stops_before_exceeding_the_limit() {
    // 预算逐块累加，超限时中止且不再继续写——压缩炸弹声明的 size 不可信，
    // 所以判断只能基于实际读到的字节。
    let mut total = MAX_ARCHIVE_TOTAL_BYTES - 8;
    let mut reader = std::io::Cursor::new(vec![7u8; 64]);
    let mut writer: Vec<u8> = Vec::new();

    let err = SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
        .expect_err("must reject once the budget is exhausted");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_LARGE"),
        "unexpected error: {err}"
    );
    assert!(
        writer.is_empty(),
        "nothing may be written once the chunk would exceed the budget"
    );

    // 预算充足时照常写完
    let mut total = 0u64;
    let mut reader = std::io::Cursor::new(vec![7u8; 64]);
    let mut writer: Vec<u8> = Vec::new();
    SkillService::copy_entry_within_budget(&mut reader, &mut writer, &mut total)
        .expect("within budget");
    assert_eq!(writer.len(), 64);
    assert_eq!(total, 64);
}

#[test]
fn downloaded_html_page_is_reported_as_invalid_archive() {
    let page = std::io::Cursor::new(b"<!DOCTYPE html><html><body>app</body></html>".to_vec());
    let err = SkillService::open_downloaded_archive(page).unwrap_err();
    let value: serde_json::Value = serde_json::from_str(&err.to_string()).unwrap();
    assert_eq!(value["code"], "ARCHIVE_INVALID");
    assert_eq!(value["suggestion"], "checkRepoUrl");
    // 不是 404，不能触发换分支重试
    assert!(!SkillService::is_branch_not_found(&err));
}

#[test]
fn extract_repo_archive_rejects_too_many_entries() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        for i in 0..(MAX_ARCHIVE_ENTRIES + 1) {
            zip.start_file(format!("repo-main/f{i}"), opts).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
    }

    let temp = tempdir().expect("tempdir");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");
    let err = SkillService::extract_repo_archive(archive, temp.path())
        .expect_err("entry count over the limit must be rejected");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_MANY_ENTRIES"),
        "unexpected error: {err}"
    );
}

#[test]
fn extract_repo_archive_accepts_real_world_sized_skill_repos() {
    // #7475：hugohe3/ppt-master 整仓归档 13_248 条目，旧上限 10_000 把它当成
    // 压缩炸弹拒掉。下载的是整个仓库而非单个技能目录，上限必须容得下这种规模。
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    const ENTRIES: usize = 13_248;
    // 9P 上逐个建文件太慢，13k 个文件超过 180 秒单测超时。生产环境解压到系统
    // TEMP，只有 WSL2 夜间测试才把 TEMP 指到 \\wsl.localhost，这里测的是条目上限。
    let temp_root = std::env::temp_dir();
    if crate::config::is_wsl_path(&temp_root) {
        eprintln!(
            "temp dir {} is a WSL share, too slow for 13k files; skipping",
            temp_root.display()
        );
        return;
    }
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.start_file("ppt-master-main/skills/ppt-master/SKILL.md", opts)
            .unwrap();
        zip.write_all(b"---\nname: ppt-master\n---\n").unwrap();
        for i in 1..ENTRIES {
            zip.start_file(
                format!("ppt-master-main/skills/ppt-master/templates/t{i}.md"),
                opts,
            )
            .unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
    }

    let temp = tempdir().expect("tempdir");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");
    SkillService::extract_repo_archive(archive, temp.path())
        .expect("a 13k-entry skill repo must extract");
    assert!(temp.path().join("skills/ppt-master/SKILL.md").is_file());
    assert!(temp
        .path()
        .join(format!("skills/ppt-master/templates/t{}.md", ENTRIES - 1))
        .is_file());
}

#[test]
fn file_entries_are_charged_at_least_one_block() {
    // 空文件与小文件照样占 inode 和磁盘块。只按内容字节计费时，全是空文件的
    // 归档预算读数一直是 0，条目上限提高后就没有东西兜底了。
    let temp = tempdir().expect("tempdir");

    let mut total_bytes = 0u64;
    let mut empty = std::io::Cursor::new(Vec::<u8>::new());
    SkillService::write_file_within_budget(
        &mut empty,
        &temp.path().join("empty"),
        &mut total_bytes,
    )
    .expect("within budget");
    assert_eq!(total_bytes, FILE_ENTRY_BUDGET_COST);

    let mut small = std::io::Cursor::new(vec![7u8; 64]);
    SkillService::write_file_within_budget(
        &mut small,
        &temp.path().join("small"),
        &mut total_bytes,
    )
    .expect("within budget");
    assert_eq!(total_bytes, 2 * FILE_ENTRY_BUDGET_COST);

    // 超过一块的文件按实际字节计，不重复加最小值
    let mut large = std::io::Cursor::new(vec![7u8; FILE_ENTRY_BUDGET_COST as usize + 1]);
    SkillService::write_file_within_budget(
        &mut large,
        &temp.path().join("large"),
        &mut total_bytes,
    )
    .expect("within budget");
    assert_eq!(total_bytes, 3 * FILE_ENTRY_BUDGET_COST + 1);

    // 预算只差一个字节就满，再写一个 1 字节文件也必须被拦下
    let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - FILE_ENTRY_BUDGET_COST + 1;
    let mut tiny = std::io::Cursor::new(vec![7u8; 1]);
    let err = SkillService::write_file_within_budget(
        &mut tiny,
        &temp.path().join("tiny"),
        &mut total_bytes,
    )
    .expect_err("a file that would exceed the block budget must be rejected");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_LARGE"),
        "unexpected error: {err}"
    );
}

#[test]
fn extract_repo_archive_rejects_path_traversal_entries() {
    let temp = tempdir().expect("tempdir");
    // dest 放在深一层，这样逃逸一层/两层都落在 temp 内、可被检出
    let dest = temp.path().join("nested").join("dest");
    fs::create_dir_all(&dest).expect("create dest");

    let bytes = build_zip_with_traversal_entry();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("archive parses");

    SkillService::extract_repo_archive(archive, &dest).expect("extract must not fail");

    // 合法条目正常落盘
    assert!(
        dest.join("SKILL.md").is_file(),
        "legitimate entry should be extracted"
    );
    // 两级逃逸：不得写到 dest 之外
    assert!(
        !temp.path().join("escaped.txt").exists(),
        "zip-slip entry must not escape dest (temp root)"
    );
    assert!(
        !temp.path().join("nested").join("escaped.txt").exists(),
        "zip-slip entry must not escape dest (parent dir)"
    );
    // 一级逃逸：enclosed_name() 放行的那一类，必须被组件校验拦住
    assert!(
        !temp
            .path()
            .join("nested")
            .join("escaped-one-level.txt")
            .exists(),
        "single-`..` entry must not escape dest (enclosed_name allows it)"
    );
}

#[test]
fn extract_repo_archive_skips_a_symlink_that_contains_itself() {
    // `dir/link -> ..` 解析后正是归档根：它**通过**「目标必须在 base 内」的
    // 检查，因为目标就是 base 本身。没有第二道自包含检查时，
    // copy_dir_recursive(base, base/dir/link) 会把根复制进自己的子目录，
    // 每递归一层都重新看到刚落盘的副本，直到 PATH_MAX 才以 IO 错误收场。
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.start_file("repo-main/SKILL.md", opts).unwrap();
        zip.write_all(b"---\nname: t\ndescription: d\n---\n")
            .unwrap();
        zip.add_directory("repo-main/dir/", opts).unwrap();
        zip.add_symlink("repo-main/dir/link", "..", opts).unwrap();
        zip.finish().unwrap();
    }

    let temp = tempdir().expect("tempdir");
    let dest = temp.path().join("dest");
    fs::create_dir_all(&dest).expect("create dest");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(buf)).expect("archive parses");

    SkillService::extract_repo_archive(archive, &dest)
        .expect("a self-containing symlink must be skipped, not blow up the extraction");

    assert!(
        dest.join("SKILL.md").is_file(),
        "legitimate entries must still be extracted"
    );
    assert!(
        !dest.join("dir").join("link").exists(),
        "a symlink whose target contains the link itself must not be materialized"
    );
}

#[test]
fn symlink_materialization_is_charged_to_the_archive_budget() {
    // symlink 的物化走第二遍、与解压循环不同的代码路径。若它不计入同一个
    // 预算，「一个大文件 + N 个指向它的 symlink」就能写下 N 倍字节而上限
    // 全程显示合规。这里把预算预置到接近上限来验证物化确实在计费。
    let temp = tempdir().expect("tempdir");
    let base = temp.path().join("base");
    fs::create_dir_all(base.join("payload")).expect("create payload dir");
    fs::write(base.join("payload").join("big.bin"), vec![b'x'; 4096]).expect("write payload");

    let symlinks = vec![(base.join("copy"), "payload".to_string())];
    let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 1024;

    let err = SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
        .expect_err("materializing 4 KiB with 1 KiB of budget left must fail");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_LARGE"),
        "unexpected error: {err}"
    );
}

#[test]
fn symlink_guard_sees_through_unnormalized_link_paths() {
    // `enclosed_name()` 不消解 `..`，所以 link_path 可能长成 `e/../d/self`。
    // 守卫若拿它按字面跟规范化过的目标比组件，会在第一段（`e` vs `d`）就判定
    // "不包含"——而这个位置物理上就在 `d` 里面，把 `d` 复制进去正是递归自复制。
    let temp = tempdir().expect("tempdir");
    let base = temp.path().join("base");
    fs::create_dir_all(base.join("d").join("sub")).expect("create d");
    fs::create_dir_all(base.join("e")).expect("create e");

    let link_path = base.join("e").join("..").join("d").join("self");
    let symlinks = vec![(link_path, ".".to_string())];
    let mut total_bytes = 0u64;

    SkillService::resolve_symlinks_in_dir(&base, &symlinks, &mut total_bytes)
        .expect("a self-containing symlink must be skipped, not blow up the extraction");

    assert!(
        !base.join("d").join("self").exists(),
        "a link that physically lives inside its own target must not be materialized"
    );
}

/// 记录实际被消耗了多少字节的 reader。
///
/// 直接断言返回值是不够的：函数末尾本就有一道长度检查，把 `take` 的上限拆掉
/// 之后它照样返回 `None`，断言仍然通过——而炸弹的危害全在读取过程里，不在
/// 返回值。只有观测消耗量才能真正钉住"读取是有界的"。
struct CountingReader {
    remaining: u64,
    consumed: u64,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Ok(0);
        }
        let n = buf.len().min(self.remaining as usize);
        buf[..n].fill(b'a');
        self.remaining -= n as u64;
        self.consumed += n as u64;
        Ok(n)
    }
}

#[test]
fn read_symlink_target_is_bounded_and_charged() {
    // 一个打着 symlink 标志、解压流却极大的条目：zip 2.4.2 的 make_reader
    // 不按声明的 uncompressed_size 截断，没有上限就会被整条读进内存。
    let mut oversized = CountingReader {
        remaining: 8 * 1024 * 1024,
        consumed: 0,
    };
    let mut total_bytes = 0u64;

    let target = SkillService::read_symlink_target(&mut oversized, &mut total_bytes)
        .expect("an oversized target must be skipped, not raise");
    assert!(
        target.is_none(),
        "a target longer than a path can plausibly be must be rejected"
    );
    assert_eq!(total_bytes, 0, "a rejected target must not be charged");
    assert!(
        oversized.consumed <= MAX_SYMLINK_TARGET_BYTES + 1,
        "the read must stop at the cap instead of draining the stream, consumed {}",
        oversized.consumed
    );

    // 正常目标照常读出来并计费
    let mut normal = std::io::Cursor::new(b"../shared".to_vec());
    let target = SkillService::read_symlink_target(&mut normal, &mut total_bytes)
        .expect("a normal target must be read");
    assert_eq!(target.as_deref(), Some("../shared"));
    assert_eq!(total_bytes, 9);
}

#[test]
fn directory_materialization_is_charged_to_the_archive_budget() {
    // 全是空目录的归档一个内容字节都不写。不给目录计费，第二遍的 symlink
    // 解析就能让目录数按层数指数增长，而预算读数一直停在 0。
    let temp = tempdir().expect("tempdir");
    let src = temp.path().join("src");
    fs::create_dir_all(src.join("a").join("b")).expect("create tree");

    let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - DIRECTORY_BUDGET_COST;
    let err =
        SkillService::copy_dir_within_budget(&src, &temp.path().join("dest"), &mut total_bytes)
            .expect_err("materializing directories past the limit must fail");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_LARGE"),
        "unexpected error: {err}"
    );
}

#[test]
fn create_dir_all_charges_every_directory_it_creates() {
    // `create_dir_all` 一次能把缺失的父目录全建出来，所以一个条目名
    // `a/a/…/a/f.txt` 可以隐式造出几百层。按调用次数计费会严重低估。
    let temp = tempdir().expect("tempdir");
    let deep = temp.path().join("a").join("b").join("c");

    // 预算只够两层，建三层必须被拦下
    let mut total_bytes = MAX_ARCHIVE_TOTAL_BYTES - 2 * DIRECTORY_BUDGET_COST;
    let err = SkillService::create_dir_all_within_budget(&deep, &mut total_bytes)
        .expect_err("creating more directories than the budget allows must fail");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_LARGE"),
        "unexpected error: {err}"
    );
    assert!(
        !deep.exists(),
        "nothing must be created once the budget is exceeded"
    );
}

#[test]
fn extract_local_zip_leaves_no_partial_directory_when_it_fails() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    // scratch 只喂给这一次解压：并发测试的临时目录不会落进来，
    // 所以"必须为空"的断言观测到的恰好就是这次解压的残留
    let holder = tempdir().expect("tempdir");
    let scratch = tempdir().expect("tempdir");

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        // `x` 先落成文件，再要求把它当目录用 —— create_dir_all 必然失败，
        // 而这个失败发生在临时目录已经建好、且已经写进去东西之后
        zip.start_file("x", opts).unwrap();
        zip.write_all(b"i am a file").unwrap();
        zip.start_file("x/y", opts).unwrap();
        zip.write_all(b"and my parent is not a directory").unwrap();
        zip.finish().unwrap();
    }
    let zip_path = holder.path().join("collide.zip");
    fs::write(&zip_path, &buf).expect("write zip");

    let result = SkillService::extract_local_zip_in(&zip_path, scratch.path());

    assert!(
        result.is_err(),
        "the fixture must actually fail after the temp dir exists"
    );
    let leftovers: Vec<_> = fs::read_dir(scratch.path())
        .expect("read scratch")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert!(
        leftovers.is_empty(),
        "a failed extraction must not leave a partial directory behind: {leftovers:?}"
    );
}

#[test]
fn extract_local_zip_hands_back_a_guard_that_owns_the_tree() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    // 解压成功之后调用方还要走扫描 / 取 SSOT / 复制 / 写库 / 同步一长串 `?`。
    // 之前返回裸 PathBuf，清理靠每条出口手写 remove_dir_all——实测漏了不止一条
    // （install_from_zip 的 copy_dir_recursive 与 save_skill、update_skill 的
    // copy_dir_recursive、fetch_repo_skills 的 scan_dir_recursive 都会漏）。
    let holder = tempdir().expect("tempdir");
    let scratch = tempdir().expect("tempdir");

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.start_file("s/SKILL.md", opts).unwrap();
        zip.write_all(b"# skill").unwrap();
        zip.finish().unwrap();
    }
    let zip_path = holder.path().join("ok.zip");
    fs::write(&zip_path, &buf).expect("write zip");

    let extracted = SkillService::extract_local_zip_in(&zip_path, scratch.path())
        .expect("extract must succeed");
    assert!(
        extracted.path().join("s").join("SKILL.md").exists(),
        "the fixture must actually extract something worth cleaning up"
    );

    // 模拟调用方在下游任意一个 `?` 上提前返回
    drop(extracted);

    let leftovers: Vec<_> = fs::read_dir(scratch.path())
        .expect("read scratch")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    assert!(
        leftovers.is_empty(),
        "dropping the extraction result must take the whole tree with it: {leftovers:?}"
    );
}

#[test]
fn extract_local_zip_rejects_dot_dot_entries() {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        zip.add_directory("d/", opts).unwrap();
        zip.add_directory("e/", opts).unwrap();
        // 净深度非负，enclosed_name() 放行；未消解的 `..` 会把它落到 d 里面
        zip.start_file("e/../d/leaked.txt", opts).unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
    }

    let temp = tempdir().expect("tempdir");
    let zip_path = temp.path().join("dots.zip");
    fs::write(&zip_path, &buf).expect("write zip");

    let extracted = SkillService::extract_local_zip(&zip_path).expect("extract must not fail");
    assert!(
        !extracted.path().join("d").join("leaked.txt").exists(),
        "an entry with an unresolved `..` must be skipped, not silently relocated"
    );
}

#[test]
fn extract_local_zip_rejects_too_many_entries() {
    // 本地 ZIP 走的是另一个解压器，条目上限曾只加在远端归档那条路径上。
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        for i in 0..(MAX_ARCHIVE_ENTRIES + 1) {
            zip.start_file(format!("f{i}"), opts).unwrap();
            zip.write_all(b"x").unwrap();
        }
        zip.finish().unwrap();
    }

    let temp = tempdir().expect("tempdir");
    let zip_path = temp.path().join("bomb.zip");
    fs::write(&zip_path, &buf).expect("write zip");

    let err = SkillService::extract_local_zip(&zip_path)
        .expect_err("entry count over the limit must be rejected for local ZIPs too");
    assert!(
        err.to_string().contains("ARCHIVE_TOO_MANY_ENTRIES"),
        "unexpected error: {err}"
    );
}

fn write_skill(dir: &Path, name: &str) {
    fs::create_dir_all(dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

#[test]
#[serial_test::serial]
fn pi_skill_state_follows_native_directory_presence() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
    skill.apps.pi = true;
    db.save_skill(&skill).expect("save skill");

    let installed = SkillService::get_all_installed(&db).expect("read skills");
    assert!(!installed[0].apps.pi, "a DB flag must not activate Pi");

    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "test",
    );
    SkillService::toggle_app(&db, &skill.id, &AppType::Pi, true).expect("enable Pi skill");
    assert!(SkillService::get_all_installed(&db).unwrap()[0].apps.pi);

    SkillService::toggle_app(&db, &skill.id, &AppType::Pi, false).expect("disable Pi skill");
    assert!(!SkillService::get_all_installed(&db).unwrap()[0].apps.pi);
}

#[test]
#[serial_test::serial]
fn pi_skill_toggle_preserves_a_same_name_external_directory() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "managed",
    );
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    write_skill(&pi_skill, "external");

    let enable = SkillService::toggle_app(&db, &skill.id, &AppType::Pi, true);
    assert!(
        enable.is_err(),
        "enable must not overwrite external content"
    );
    assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
        .unwrap()
        .contains("external"));

    let disable = SkillService::toggle_app(&db, &skill.id, &AppType::Pi, false);
    assert!(disable.is_err(), "disable must not delete external content");
    assert!(pi_skill.join("SKILL.md").exists());
}

#[test]
#[serial_test::serial]
fn pi_install_conflict_is_rejected_before_persisting() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "managed",
    );
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    write_skill(&pi_skill, "external");

    let result = SkillService::persist_and_sync_new_skill(&db, &skill, &AppType::Pi);

    assert!(result.is_err());
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("read skill")
        .is_none());
    assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
        .expect("read external skill")
        .contains("external"));
}

#[test]
#[serial_test::serial]
fn agy_and_kimi_skill_toggle_rolls_back_on_database_failure() {
    for app in [
        AppType::Agy,
        AppType::Kimi,
        AppType::Dsh,
        AppType::WorkBuddy,
        AppType::Codebuddy,
    ] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("local:test-skill", "test-skill");
        skill.apps.set_enabled_for(&app, true);
        db.save_skill(&skill).unwrap();
        let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
        write_skill(&source, "original");
        SkillService::sync_to_app_dir(&skill.directory, &app).unwrap();
        let native = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join(&skill.directory);
        let original = fs::read(native.join("SKILL.md")).unwrap();
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        assert!(SkillService::toggle_app(&db, &skill.id, &app, false).is_err());
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
        assert!(db
            .get_installed_skill(&skill.id)
            .unwrap()
            .unwrap()
            .apps
            .is_enabled_for(&app));
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=OFF")
            .unwrap();
        SkillService::toggle_app(&db, &skill.id, &app, false).unwrap();
        assert!(!native.exists());
        SkillService::toggle_app(&db, &skill.id, &app, true).unwrap();
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    }
}

#[test]
#[serial_test::serial]
fn agy_unmanaged_copy_survives_sync_migration_and_uninstall() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
    skill.apps.claude = true;
    db.save_skill(&skill).unwrap();
    let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
    let native = SkillService::get_app_skills_dir(&AppType::Agy)
        .unwrap()
        .join(&skill.directory);
    write_skill(&source, "same-content");
    SkillService::copy_dir_recursive(&source, &native).unwrap();
    let original = fs::read(native.join("SKILL.md")).unwrap();
    SkillService::sync_to_app(&db, &AppType::Agy).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    SkillService::migrate_storage(&db, SkillStorageLocation::Unified).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    SkillService::uninstall(&db, &skill.id).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
}

#[test]
#[serial_test::serial]
fn mcode_unmanaged_copy_survives_sync_migration_and_uninstall() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
    skill.apps.claude = true;
    db.save_skill(&skill).unwrap();
    let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
    let native = SkillService::get_app_skills_dir(&AppType::Mcode)
        .unwrap()
        .join(&skill.directory);
    write_skill(&source, "same-content");
    SkillService::copy_dir_recursive(&source, &native).unwrap();
    let original = fs::read(native.join("SKILL.md")).unwrap();
    SkillService::sync_to_app(&db, &AppType::Mcode).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    SkillService::migrate_storage(&db, SkillStorageLocation::Unified).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    SkillService::uninstall(&db, &skill.id).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
}

#[test]
#[serial_test::serial]
fn kimi_workbuddy_and_codebuddy_unmanaged_copy_survives_resync_migration_and_uninstall() {
    for (app, native_content) in [
        (AppType::Kimi, "same-content"),
        (AppType::Kimi, "external"),
        (AppType::WorkBuddy, "same-content"),
        (AppType::WorkBuddy, "external"),
        (AppType::Codebuddy, "same-content"),
        (AppType::Codebuddy, "external"),
    ] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
        skill.apps.claude = true;
        db.save_skill(&skill).unwrap();
        let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
        let native = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join(&skill.directory);
        write_skill(&source, "same-content");
        write_skill(&native, native_content);
        let original = fs::read(native.join("SKILL.md")).unwrap();
        let outcomes = SkillService::resync_all_apps(&db);
        assert!(
            outcomes
                .iter()
                .find(|result| result.app == app.as_str())
                .unwrap()
                .ok
        );
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
        SkillService::toggle_app(&db, &skill.id, &app, false).unwrap();
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
        SkillService::migrate_storage(&db, SkillStorageLocation::Unified).unwrap();
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
        SkillService::uninstall(&db, &skill.id).unwrap();
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
        assert!(db.get_installed_skill(&skill.id).unwrap().is_none());
    }
}

#[test]
#[serial_test::serial]
fn kimi_workbuddy_and_codebuddy_skill_enable_rejects_external_directory_without_changing_files_or_flags(
) {
    for app in [AppType::Kimi, AppType::WorkBuddy, AppType::Codebuddy] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("local:test-skill", "test-skill");
        let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
        let native = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join(&skill.directory);
        write_skill(&source, "managed");
        write_skill(&native, "external");
        let original = fs::read(native.join("SKILL.md")).unwrap();
        skill.apps.set_enabled_for(&app, true);
        assert!(SkillService::persist_and_sync_new_skill(&db, &skill, &app).is_err());
        assert!(db.get_installed_skill(&skill.id).unwrap().is_none());
        skill.apps.set_enabled_for(&app, false);
        db.save_skill(&skill).unwrap();
        assert!(SkillService::toggle_app(&db, &skill.id, &app, true).is_err());
        assert!(!db
            .get_installed_skill(&skill.id)
            .unwrap()
            .unwrap()
            .apps
            .is_enabled_for(&app));
        assert!(SkillService::remove_from_app(&skill.directory, &app).is_err());
        assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);
    }
}

#[test]
#[cfg(unix)]
#[serial_test::serial]
fn agy_imported_external_symlink_can_toggle_without_changing_its_source() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let external = temp.path().join("external-skill");
    write_skill(&external, "external");
    let original = fs::read(external.join("SKILL.md")).unwrap();
    let native = SkillService::get_app_skills_dir(&AppType::Agy)
        .unwrap()
        .join("test-skill");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    SkillService::create_symlink(&external, &native).unwrap();
    let imported = SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                agy: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert!(SkillService::paths_alias(
        &native,
        &SkillService::get_ssot_dir().unwrap().join("test-skill")
    ));
    SkillService::toggle_app(&db, &imported[0].id, &AppType::Agy, false).unwrap();
    assert!(!native.exists());
    assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), original);
    SkillService::toggle_app(&db, &imported[0].id, &AppType::Agy, true).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);

    SkillService::remove_path(&native).unwrap();
    SkillService::create_symlink(&external, &native).unwrap();
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "different",
    );
    let empty_db = Arc::new(Database::memory().unwrap());
    assert!(SkillService::import_from_apps(
        &empty_db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                agy: true,
                ..Default::default()
            },
        }],
    )
    .is_err());
    assert!(empty_db.get_all_installed_skills().unwrap().is_empty());
    assert!(SkillService::paths_alias(&native, &external));
    assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), original);
}

#[test]
#[cfg(unix)]
#[serial_test::serial]
fn mcode_imported_external_symlink_can_toggle_without_changing_its_source() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let external = temp.path().join("external-skill");
    write_skill(&external, "external");
    let original = fs::read(external.join("SKILL.md")).unwrap();
    let native = SkillService::get_app_skills_dir(&AppType::Mcode)
        .unwrap()
        .join("test-skill");
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    SkillService::create_symlink(&external, &native).unwrap();
    let imported = SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                mcode: true,
                kimi: false,
                dsh: false,
                ..Default::default()
            },
        }],
    )
    .unwrap();
    assert!(SkillService::paths_alias(
        &native,
        &SkillService::get_ssot_dir().unwrap().join("test-skill")
    ));
    SkillService::toggle_app(&db, &imported[0].id, &AppType::Mcode, false).unwrap();
    assert!(!native.exists());
    assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), original);
    SkillService::toggle_app(&db, &imported[0].id, &AppType::Mcode, true).unwrap();
    assert_eq!(fs::read(native.join("SKILL.md")).unwrap(), original);

    SkillService::remove_path(&native).unwrap();
    SkillService::create_symlink(&external, &native).unwrap();
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "different",
    );
    let empty_db = Arc::new(Database::memory().unwrap());
    assert!(SkillService::import_from_apps(
        &empty_db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                mcode: true,
                kimi: false,
                dsh: false,
                ..Default::default()
            },
        }],
    )
    .is_err());
    assert!(empty_db.get_all_installed_skills().unwrap().is_empty());
    assert!(SkillService::paths_alias(&native, &external));
    assert_eq!(fs::read(external.join("SKILL.md")).unwrap(), original);
}

#[test]
#[serial_test::serial]
fn mcode_import_checks_native_copies_and_deploys_selected_skills() {
    for native_content in [None, Some("shared"), Some("different")] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let claude = SkillService::get_app_skills_dir(&AppType::Claude)
            .unwrap()
            .join("test-skill");
        let native = SkillService::get_app_skills_dir(&AppType::Mcode)
            .unwrap()
            .join("test-skill");
        write_skill(&claude, "shared");
        if let Some(content) = native_content {
            write_skill(&native, content);
        }
        let imported = SkillService::import_from_apps(
            &db,
            vec![ImportSkillSelection {
                directory: "test-skill".into(),
                apps: SkillApps {
                    mcode: true,
                    kimi: false,
                    dsh: false,
                    ..Default::default()
                },
            }],
        );
        if native_content == Some("different") {
            assert!(imported.is_err());
            assert!(db.get_all_installed_skills().unwrap().is_empty());
            assert!(fs::read_to_string(native.join("SKILL.md"))
                .unwrap()
                .contains("different"));
        } else {
            let imported = imported.unwrap();
            assert!(imported[0].apps.mcode);
            assert_eq!(
                fs::read(native.join("SKILL.md")).unwrap(),
                fs::read(claude.join("SKILL.md")).unwrap()
            );
            SkillService::toggle_app(&db, &imported[0].id, &AppType::Mcode, false).unwrap();
            assert!(!native.exists());
        }
        assert!(fs::read_to_string(claude.join("SKILL.md"))
            .unwrap()
            .contains("shared"));
    }
}

#[test]
#[serial_test::serial]
fn mcode_skill_import_continues_after_a_conflicting_copy() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let native = SkillService::get_app_skills_dir(&AppType::Mcode).unwrap();
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("conflict"),
        "managed",
    );
    write_skill(&native.join("conflict"), "external");
    write_skill(&native.join("valid"), "valid");
    let selections = ["conflict", "valid"]
        .into_iter()
        .map(|directory| ImportSkillSelection {
            directory: directory.into(),
            apps: SkillApps {
                mcode: true,
                kimi: false,
                dsh: false,
                ..Default::default()
            },
        })
        .collect();
    let error = SkillService::import_from_apps(&db, selections)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Imported 1 Skills"));
    assert!(error.contains("conflict"));
    let installed = db.get_all_installed_skills().unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].directory, "valid");
    assert!(installed[0].apps.mcode);
    assert!(fs::read_to_string(native.join("conflict/SKILL.md"))
        .unwrap()
        .contains("external"));
}

#[test]
#[serial_test::serial]
fn import_deploys_the_skill_to_selected_apps_that_do_not_have_it() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let app_copy = |app: AppType| {
        SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join("test-skill")
    };
    // 只在 Claude 里发现，导入时另外勾了 Codex 和 Gemini。
    write_skill(&app_copy(AppType::Claude), "found-in-claude");

    let imported = SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                gemini: true,
                ..Default::default()
            },
        }],
    )
    .unwrap();

    assert!(imported[0].apps.codex && imported[0].apps.gemini);
    for app in [AppType::Codex, AppType::Gemini] {
        assert!(
            fs::read_to_string(app_copy(app.clone()).join("SKILL.md"))
                .unwrap()
                .contains("found-in-claude"),
            "{app:?} must have the skill right after import"
        );
    }
    // 没勾的应用不写；发现它的地方保持原样（仍是真实目录，不换成链接）。
    assert!(!app_copy(AppType::OpenCode).exists());
    assert!(!SkillService::is_symlink(&app_copy(AppType::Claude)));
}

#[test]
#[serial_test::serial]
fn import_does_not_record_an_app_the_skill_could_not_be_deployed_to() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    write_skill(
        &SkillService::get_app_skills_dir(&AppType::Claude)
            .unwrap()
            .join("test-skill"),
        "found-in-claude",
    );
    // Codex 的 skills 路径被一个文件占着，目录建不出来。
    let codex_skills = SkillService::get_app_skills_dir(&AppType::Codex).unwrap();
    fs::create_dir_all(codex_skills.parent().unwrap()).unwrap();
    fs::write(&codex_skills, "not a directory").unwrap();

    let error = SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "test-skill".into(),
            apps: SkillApps {
                claude: true,
                codex: true,
                ..Default::default()
            },
        }],
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("Imported 1 Skills"), "{error}");
    assert!(error.contains("test-skill (codex)"), "{error}");
    let installed = db.get_all_installed_skills().unwrap();
    assert_eq!(installed.len(), 1);
    assert!(installed[0].apps.claude);
    assert!(!installed[0].apps.codex);
}

#[tokio::test]
#[ignore = "downloads a real upstream Skill to verify the complete update workflow"]
#[serial_test::serial]
async fn mcode_copy_updates_and_disables_without_losing_external_changes() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let db = Arc::new(Database::memory().unwrap());
    let mut skill = poisoned_skill("anthropics/skills:algorithmic-art", "algorithmic-art");
    skill.repo_owner = Some("anthropics".into());
    skill.repo_name = Some("skills".into());
    skill.repo_branch = Some("main".into());
    skill.apps.mcode = true;
    db.save_skill(&skill).unwrap();
    let source = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
    let destination = SkillService::get_app_skills_dir(&AppType::Mcode)
        .unwrap()
        .join(&skill.directory);
    write_skill(&source, "old");
    SkillService::copy_dir_recursive(&source, &destination).unwrap();
    SkillService::new()
        .update_skill(&db, &skill.id)
        .await
        .unwrap();
    assert_eq!(
        SkillService::compute_dir_hash(&source).unwrap(),
        SkillService::compute_dir_hash(&destination).unwrap()
    );
    assert!(!fs::read_to_string(destination.join("SKILL.md"))
        .unwrap()
        .contains("name: old"));
    fs::write(destination.join("local.txt"), "user-owned").unwrap();
    assert!(SkillService::toggle_app(&db, &skill.id, &AppType::Mcode, false).is_err());
    fs::remove_file(destination.join("local.txt")).unwrap();
    SkillService::toggle_app(&db, &skill.id, &AppType::Mcode, false).unwrap();
    assert!(!destination.exists());
    assert!(
        !db.get_installed_skill(&skill.id)
            .unwrap()
            .unwrap()
            .apps
            .mcode
    );
}

#[test]
#[serial_test::serial]
fn mcode_skill_update_rolls_back_deployments_when_database_is_read_only() {
    for symlink in [false, true] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("test-skill", "test-skill");
        skill.apps.mcode = true;
        skill.apps.pi = true;
        db.save_skill(&skill).unwrap();
        let ssot = SkillService::get_ssot_dir().unwrap().join(&skill.directory);
        let source = temp.path().join("download");
        write_skill(&ssot, "old");
        write_skill(&source, "new");
        let mut deployments = Vec::new();
        for app in [AppType::Pi, AppType::Mcode] {
            let native = SkillService::get_app_skills_dir(&app)
                .unwrap()
                .join(&skill.directory);
            if symlink {
                fs::create_dir_all(native.parent().unwrap()).unwrap();
                let result = SkillService::create_symlink(&ssot, &native);
                #[cfg(windows)]
                if result.is_err() {
                    return; // Windows may not permit directory symlinks.
                }
                result.unwrap();
            } else {
                SkillService::copy_dir_recursive(&ssot, &native).unwrap();
            }
            let deployment =
                SkillService::inspect_pi_skill_destination(&ssot, &native, &skill.directory)
                    .unwrap()
                    .unwrap();
            deployments.push((native, deployment));
        }
        let mut updated = skill.clone();
        updated.name = "new".into();
        updated.content_hash = Some(SkillService::compute_dir_hash(&source).unwrap());
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only = ON")
            .unwrap();
        assert!(SkillService::update_mcode_skill_files(
            &source,
            &ssot,
            &skill.directory,
            &deployments,
            || SkillService::persist_updated_skill_metadata(&db, &updated),
        )
        .is_err());
        assert_eq!(
            db.get_installed_skill(&skill.id).unwrap().unwrap().name,
            skill.name
        );
        assert!(fs::read_to_string(ssot.join("SKILL.md"))
            .unwrap()
            .contains("name: old"));
        for (native, _) in &deployments {
            assert_eq!(SkillService::is_symlink(native), symlink);
            SkillService::ensure_pi_skill_destination_matches(&ssot, native, &skill.directory)
                .unwrap();
        }
        db.conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only = OFF")
            .unwrap();
        SkillService::update_mcode_skill_files(
            &source,
            &ssot,
            &skill.directory,
            &deployments,
            || SkillService::persist_updated_skill_metadata(&db, &updated),
        )
        .unwrap();
        assert_eq!(
            db.get_installed_skill(&skill.id).unwrap().unwrap().name,
            "new"
        );
        for (native, _) in &deployments {
            assert_eq!(SkillService::is_symlink(native), symlink);
            SkillService::ensure_pi_skill_destination_matches(&ssot, native, &skill.directory)
                .unwrap();
        }
        SkillService::uninstall(&db, &skill.id).unwrap();
        assert!(!ssot.exists());
        assert!(deployments.iter().all(|(native, _)| !native.exists()));
    }
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn mcode_skill_update_can_retry_after_native_directory_write_failure() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let ssot = SkillService::get_ssot_dir().unwrap().join("test-skill");
    let source = temp.path().join("download");
    let native = SkillService::get_app_skills_dir(&AppType::Mcode)
        .unwrap()
        .join("test-skill");
    write_skill(&ssot, "old");
    write_skill(&source, "new");
    SkillService::copy_dir_recursive(&ssot, &native).unwrap();
    let deployment = SkillService::inspect_pi_skill_destination(&ssot, &native, "test-skill")
        .unwrap()
        .unwrap();
    let deployments = vec![(native.clone(), deployment)];
    let parent = native.parent().unwrap();
    let original_permissions = fs::metadata(parent).unwrap().permissions();
    fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result: Result<()> =
        SkillService::update_mcode_skill_files(&source, &ssot, "test-skill", &deployments, || {
            panic!("failed native staging must not commit metadata")
        });
    fs::set_permissions(parent, original_permissions).unwrap();
    assert!(result.is_err());
    assert!(fs::read_to_string(ssot.join("SKILL.md"))
        .unwrap()
        .contains("name: old"));
    SkillService::ensure_pi_skill_destination_matches(&ssot, &native, "test-skill").unwrap();
    SkillService::update_mcode_skill_files(&source, &ssot, "test-skill", &deployments, || Ok(()))
        .unwrap();
    assert!(fs::read_to_string(native.join("SKILL.md"))
        .unwrap()
        .contains("name: new"));
}

#[test]
fn managed_pi_copy_refreshes_from_its_expected_old_content() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let destination = temp.path().join("pi-skill");
    write_skill(&source, "old");
    SkillService::copy_dir_recursive(&source, &destination).expect("seed managed copy");

    let deployment =
        SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
            .expect("inspect managed copy")
            .expect("managed deployment");

    fs::remove_dir_all(&source).expect("replace old source");
    write_skill(&source, "new");
    SkillService::refresh_pi_skill_destination(&source, &destination, "test-skill", &deployment)
        .expect("refresh managed copy");

    assert!(fs::read_to_string(destination.join("SKILL.md"))
        .expect("read refreshed copy")
        .contains("name: new"));
}

#[test]
fn managed_pi_copy_rejects_hidden_native_changes() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let destination = temp.path().join("pi-skill");
    write_skill(&source, "old");
    SkillService::copy_dir_recursive(&source, &destination).expect("seed managed copy");
    let deployment =
        SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
            .expect("inspect managed copy")
            .expect("managed deployment");

    fs::write(destination.join(".env"), "user-owned").expect("add hidden native file");
    fs::remove_dir_all(&source).expect("replace old source");
    write_skill(&source, "new");

    let result = SkillService::refresh_pi_skill_destination(
        &source,
        &destination,
        "test-skill",
        &deployment,
    );
    assert!(
        result.is_err(),
        "hidden native changes must block replacement"
    );
    assert_eq!(
        fs::read_to_string(destination.join(".env")).expect("read hidden native file"),
        "user-owned"
    );
}

#[test]
#[serial_test::serial]
fn importing_a_native_pi_skill_returns_its_derived_active_state() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("native-skill");
    write_skill(&pi_skill, "native");

    let imported = SkillService::import_from_apps(
        &db,
        vec![ImportSkillSelection {
            directory: "native-skill".to_string(),
            apps: SkillApps::default(),
        }],
    )
    .expect("import native Pi skill");

    assert_eq!(imported.len(), 1);
    assert!(imported[0].apps.pi);
    assert!(SkillService::get_all_installed(&db).unwrap()[0].apps.pi);
}

#[test]
#[serial_test::serial]
fn uninstall_preserves_an_external_pi_skill_and_removes_the_managed_record() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    write_skill(&pi_skill, "external");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

    assert!(!source.exists(), "managed source must be removed");
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
    assert_eq!(
        result.preserved_pi_path,
        Some(pi_skill.to_string_lossy().to_string())
    );
    assert!(result.pi_cleanup_incomplete);
    assert!(fs::read_to_string(pi_skill.join("SKILL.md"))
        .expect("read external skill")
        .contains("name: external"));
}

#[test]
#[serial_test::serial]
fn mcode_and_kimi_uninstall_keep_modified_copy_and_record_until_cleanup_succeeds() {
    for app in [
        AppType::Mcode,
        AppType::Kimi,
        AppType::Dsh,
        AppType::WorkBuddy,
        AppType::Codebuddy,
    ] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
        skill.apps.set_enabled_for(&app, true);
        db.save_skill(&skill).unwrap();
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        let native = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join("test-skill");
        write_skill(&source, "managed");
        SkillService::copy_dir_recursive(&source, &native).unwrap();
        fs::write(native.join("local.txt"), "user changes").unwrap();
        assert!(SkillService::uninstall(&db, &skill.id).is_err());
        assert!(db.get_installed_skill(&skill.id).unwrap().is_some());
        assert!(source.join("SKILL.md").exists());
        assert_eq!(
            fs::read_to_string(native.join("local.txt")).unwrap(),
            "user changes"
        );
        fs::remove_file(native.join("local.txt")).unwrap();
        SkillService::uninstall(&db, &skill.id).unwrap();
        assert!(db.get_installed_skill(&skill.id).unwrap().is_none());
        assert!(!native.exists());
        assert!(!source.exists());
    }
}

#[test]
#[serial_test::serial]
fn uninstall_removes_a_dangling_link_to_a_missing_managed_copy() {
    for app in [AppType::Dsh, AppType::Agy] {
        let temp = tempdir().unwrap();
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
        let db = Arc::new(Database::memory().unwrap());
        let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
        skill.apps.set_enabled_for(&app, true);
        db.save_skill(&skill).unwrap();
        let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        let native = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join("test-skill");
        write_skill(&source, "managed");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        let linked = SkillService::create_symlink(&source, &native);
        #[cfg(windows)]
        if linked.is_err() {
            return;
        }
        linked.unwrap();
        fs::remove_dir_all(&source).unwrap();

        SkillService::uninstall(&db, &skill.id).expect("uninstall with a missing managed copy");

        assert!(db.get_installed_skill(&skill.id).unwrap().is_none());
        assert!(
            !SkillService::is_symlink(&native),
            "dangling link must be removed"
        );
    }
}

#[test]
#[serial_test::serial]
fn uninstall_keeps_a_dangling_link_to_another_path_when_the_managed_copy_is_missing() {
    let temp = tempdir().unwrap();
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
    let db = Arc::new(Database::memory().unwrap());
    let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
    skill.apps.set_enabled_for(&AppType::Dsh, true);
    db.save_skill(&skill).unwrap();
    let native = SkillService::get_app_skills_dir(&AppType::Dsh)
        .unwrap()
        .join("test-skill");
    let elsewhere = temp.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(native.parent().unwrap()).unwrap();
    let linked = SkillService::create_symlink(&elsewhere, &native);
    #[cfg(windows)]
    if linked.is_err() {
        return;
    }
    linked.unwrap();
    fs::remove_dir_all(&elsewhere).unwrap();

    assert!(SkillService::uninstall(&db, &skill.id).is_err());

    assert!(db.get_installed_skill(&skill.id).unwrap().is_some());
    assert!(
        SkillService::is_symlink(&native),
        "foreign link must be kept"
    );
}

#[test]
#[serial_test::serial]
fn uninstall_does_not_remove_a_preserved_pi_skill_through_another_app() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".claude"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");
    let shared_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    assert_eq!(
        shared_skill,
        SkillService::get_app_skills_dir(&AppType::Claude)
            .unwrap()
            .join("test-skill")
    );
    write_skill(&shared_skill, "external");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

    assert!(!source.exists(), "managed source must be removed");
    assert_eq!(
        result.preserved_pi_path,
        Some(shared_skill.to_string_lossy().to_string())
    );
    assert!(result.pi_cleanup_incomplete);
    assert!(fs::read_to_string(shared_skill.join("SKILL.md"))
        .expect("read shared external skill")
        .contains("name: external"));
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn uninstall_preserves_a_dangling_pi_link_through_an_aliased_app_root() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let shared_agent = temp.path().join("shared-agent");
    fs::create_dir_all(shared_agent.join("skills")).expect("create shared skills root");
    symlink(&shared_agent, temp.path().join(".claude")).expect("alias Claude root");
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&shared_agent);

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");
    let pi_skill = shared_agent.join("skills").join("test-skill");
    symlink(temp.path().join("missing-target"), &pi_skill).expect("dangling Pi link");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

    assert_eq!(
        result.preserved_pi_path,
        Some(pi_skill.to_string_lossy().to_string())
    );
    assert!(result.pi_cleanup_incomplete);
    assert!(
        SkillService::is_symlink(&pi_skill),
        "the aliased application cleanup must not remove the preserved link"
    );
    assert!(!source.exists());
}

#[test]
fn pi_uninstall_rechecks_a_destination_created_after_preflight() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let destination = temp.path().join("pi-skill");
    write_skill(&source, "managed");
    assert!(
        SkillService::inspect_pi_skill_destination(&source, &destination, "test-skill")
            .expect("inspect absent destination")
            .is_none()
    );

    SkillService::copy_dir_recursive(&source, &destination).expect("simulate concurrent Pi sync");
    SkillService::remove_verified_pi_destination(&source, &destination, "test-skill")
        .expect("remove destination created after preflight");

    assert!(!destination.exists());
}

#[test]
#[serial_test::serial]
fn uninstall_keeps_ssot_when_it_contains_a_preserved_pi_skill() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&source.join("pi-agent"));
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    write_skill(&pi_skill, "external");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall managed record");

    assert!(result.backup_path.is_none());
    assert_eq!(
        result.preserved_pi_path,
        Some(pi_skill.to_string_lossy().to_string())
    );
    assert!(result.pi_cleanup_incomplete);
    assert!(source.exists(), "the preserved path is nested under SSOT");
    assert!(pi_skill.exists(), "the external Pi skill must remain");
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
}

#[test]
#[serial_test::serial]
fn uninstall_with_a_missing_source_does_not_backup_or_delete_the_pi_directory() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    write_skill(&pi_skill, "external");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall missing source");

    assert!(result.backup_path.is_none());
    assert_eq!(
        result.preserved_pi_path,
        Some(pi_skill.to_string_lossy().to_string())
    );
    assert!(result.pi_cleanup_incomplete);
    assert!(pi_skill.join("SKILL.md").exists());
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
}

#[test]
#[serial_test::serial]
fn uninstall_treats_an_aliased_pi_root_as_the_ssot() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::Unified);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall aliased skill");

    assert!(!source.exists());
    assert!(result.preserved_pi_path.is_none());
    assert!(!result.pi_cleanup_incomplete);
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
}

#[test]
#[serial_test::serial]
fn uninstall_warns_when_the_pi_root_cannot_be_resolved() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(Path::new("relative/pi-agent"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&source, "managed");

    let result = SkillService::uninstall(&db, &skill.id).expect("uninstall with invalid Pi root");

    assert!(!source.exists());
    assert!(result.preserved_pi_path.is_none());
    assert!(result.pi_cleanup_incomplete);
    assert!(db
        .get_installed_skill(&skill.id)
        .expect("query skill")
        .is_none());
}

/// CC_SWITCH_TEST_HOME 隔离守卫（serial 测试间互斥由 #[serial] 保证，
/// 守卫只负责在测试结束后恢复原值）。
struct TestHomeGuard {
    home: PathBuf,
    _env: EnvVars,
}
impl TestHomeGuard {
    fn set(home: &Path) -> Self {
        let env = EnvVars::new()
            .set("DSH_HOME", home.join(".dsh"))
            .set("CC_SWITCH_TEST_HOME", home)
            .set("KIMI_CODE_HOME", home.join(".kimi-code"));
        Self {
            home: home.to_path_buf(),
            _env: env,
        }
    }
}

/// 设置/恢复存储位置都会写 `settings.json`，所以必须借用一个已生效的
/// `TestHomeGuard`：借用保证它在 HOME 隔离之后创建、在 HOME 恢复之前 drop，
/// 不会写到真实 `~/.cc-switch/settings.json`（Windows 上该文件被占用时原子替换
/// 会报 os error 32 / 1176）。
struct StorageLocationGuard<'a> {
    home: &'a TestHomeGuard,
    previous: SkillStorageLocation,
}
impl<'a> StorageLocationGuard<'a> {
    fn set(home: &'a TestHomeGuard, location: SkillStorageLocation) -> Self {
        assert_settings_write_isolated(home);
        let previous = crate::settings::get_skill_storage_location();
        crate::settings::set_skill_storage_location(location)
            .expect("set test skill storage location");
        Self { home, previous }
    }
}
impl Drop for StorageLocationGuard<'_> {
    fn drop(&mut self) {
        assert_settings_write_isolated(self.home);
        let _ = crate::settings::set_skill_storage_location(self.previous);
    }
}

fn assert_settings_write_isolated(home: &TestHomeGuard) {
    let config_dir = crate::config::get_home_dir().join(".cc-switch");
    assert!(
        config_dir.starts_with(&home.home),
        "settings.json would be written outside the test home: {}",
        config_dir.display()
    );
}

fn poisoned_skill(id: &str, directory: &str) -> InstalledSkill {
    InstalledSkill {
        id: id.to_string(),
        name: "poisoned".to_string(),
        description: None,
        directory: directory.to_string(),
        repo_host: None,
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps::default(),
        installed_at: 0,
        content_hash: None,
        updated_at: 0,
    }
}

#[test]
fn persist_updated_skill_metadata_uses_database_apps() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let mut installed = poisoned_skill("owner/repo:skill", "skill");
    installed.name = "old name".to_string();
    installed.apps = SkillApps::only(&AppType::Claude);
    db.save_skill(&installed).expect("seed skill");

    // 模拟下载期间用户将 Skill 从 Claude 切换到 Codex。待写入的 metadata
    // 仍携带下载开始时的旧 apps 快照。
    let authoritative_apps = SkillApps::only(&AppType::Codex);
    db.update_skill_apps(&installed.id, &authoritative_apps)
        .expect("toggle apps");

    let mut updated_metadata = installed.clone();
    updated_metadata.name = "new name".to_string();
    updated_metadata.content_hash = Some("new hash".to_string());
    updated_metadata.updated_at = 42;

    let persisted = SkillService::persist_updated_skill_metadata(&db, &updated_metadata)
        .expect("persist metadata");

    assert_eq!(persisted.name, "new name");
    assert_eq!(persisted.content_hash.as_deref(), Some("new hash"));
    assert_eq!(persisted.updated_at, 42);
    assert_eq!(persisted.apps, authoritative_apps);
    assert_eq!(
        db.get_installed_skill(&installed.id)
            .expect("query skill")
            .expect("skill remains installed")
            .apps,
        authoritative_apps
    );
}

#[test]
fn persist_updated_skill_metadata_does_not_restore_uninstalled_skill() {
    let db = Arc::new(Database::memory().expect("memory db"));
    let installed = poisoned_skill("owner/repo:skill", "skill");
    db.save_skill(&installed).expect("seed skill");

    // 模拟下载期间卸载完成，随后旧的更新任务才尝试落库。
    assert!(db.delete_skill(&installed.id).expect("uninstall skill"));

    let mut updated_metadata = installed.clone();
    updated_metadata.name = "downloaded update".to_string();
    let err = SkillService::persist_updated_skill_metadata(&db, &updated_metadata)
        .expect_err("an uninstalled skill must not be restored");

    assert!(
        err.to_string().contains("Skill no longer installed"),
        "unexpected error: {err}"
    );
    assert!(db
        .get_installed_skill(&installed.id)
        .expect("query skill")
        .is_none());
}

#[test]
fn require_valid_directory_accepts_single_segment_names_only() {
    assert_eq!(
        SkillService::require_valid_directory("my-skill").expect("valid name"),
        "my-skill"
    );
    for bad in [
        "..",
        "../..",
        "../../etc",
        "a/b",
        "a\\b",
        "",
        ".hidden",
        "C:\\evil",
        "/etc",
    ] {
        assert!(
            SkillService::require_valid_directory(bad).is_err(),
            "must reject: {bad:?}"
        );
    }
}

#[test]
fn local_hash_for_update_check_ignores_cached_hash_when_dir_missing() {
    // 换机恢复数据库备份的现场：content_hash 还在库里，SSOT 目录已不在。
    // 必须无视缓存返回 None，让 Skill 进入更新列表以便重建文件；
    // 若信任缓存则界面显示「无更新」，缺失状态被永久掩盖。
    let ssot = tempdir().expect("tempdir");
    assert_eq!(
        SkillService::local_hash_for_update_check(ssot.path(), "my-skill", Some("cached")),
        None
    );
}

#[test]
fn local_hash_for_update_check_uses_cache_when_dir_exists() {
    let ssot = tempdir().expect("tempdir");
    fs::create_dir(ssot.path().join("my-skill")).expect("create skill dir");
    assert_eq!(
        SkillService::local_hash_for_update_check(ssot.path(), "my-skill", Some("cached")),
        Some(("cached".to_string(), false))
    );
}

#[test]
fn local_hash_for_update_check_computes_and_backfills_when_cache_empty() {
    let ssot = tempdir().expect("tempdir");
    let dir = ssot.path().join("my-skill");
    fs::create_dir(&dir).expect("create skill dir");
    fs::write(dir.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");

    let expected = SkillService::compute_dir_hash(&dir).expect("hash");
    assert_eq!(
        SkillService::local_hash_for_update_check(ssot.path(), "my-skill", None),
        Some((expected, true))
    );
}

#[test]
fn local_hash_for_update_check_keeps_cache_for_invalid_directory() {
    // 非法 directory 无法安全拼路径，做不了存在性检查：有缓存沿用缓存
    // （维持修复前行为），无缓存返回 None。不能因非法值报「可更新」，
    // 否则用户点更新会在 update_skill 的同一校验上硬报错。
    let ssot = tempdir().expect("tempdir");
    assert_eq!(
        SkillService::local_hash_for_update_check(ssot.path(), "../evil", Some("cached")),
        Some(("cached".to_string(), false))
    );
    assert_eq!(
        SkillService::local_hash_for_update_check(ssot.path(), "../evil", None),
        None
    );
}

/// `#7772`：运行 Skill 留下的 `__pycache__/`、`*.pyc` 不属于 Skill 自身
/// 内容，必须在目录哈希里忽略，否则只跑一遍 Skill 就会改变哈希并被报为
/// 「有更新」，点更新又会触发整目录替换、覆盖本地修改。
#[test]
fn compute_dir_hash_ignores_python_bytecode() {
    let dir = tempdir().expect("tempdir");
    let baseline = dir.path().join("baseline");
    fs::create_dir(&baseline).expect("create baseline dir");
    fs::write(baseline.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");
    fs::write(baseline.join("helper.py"), "def f():\n    return 1\n").expect("write helper");
    let baseline_hash = SkillService::compute_dir_hash(&baseline).expect("baseline hash");

    // 复制一份并补上 `__pycache__/` 与同名 `.pyc`，其它内容不变。
    let with_bytecode = dir.path().join("with-bytecode");
    SkillService::copy_dir_recursive(&baseline, &with_bytecode).expect("copy baseline");
    fs::create_dir(with_bytecode.join("__pycache__")).expect("create pycache dir");
    fs::write(
        with_bytecode
            .join("__pycache__")
            .join("helper.cpython-311.pyc"),
        b"\x00\x00\x00\x00 bytecode blob \x00\x00",
    )
    .expect("write nested pyc");
    fs::write(
        with_bytecode.join("helper.pyc"),
        b"\x00\x00\x00\x00 standalone pyc \x00\x00",
    )
    .expect("write root pyc");
    let with_bytecode_hash =
        SkillService::compute_dir_hash(&with_bytecode).expect("with-bytecode hash");

    assert_eq!(
        baseline_hash, with_bytecode_hash,
        "__pycache__/ 与 *.pyc 不应影响目录哈希"
    );

    // 仍然要识别真正修改过的源文件。
    let edited = dir.path().join("edited");
    SkillService::copy_dir_recursive(&baseline, &edited).expect("copy edited");
    fs::write(edited.join("helper.py"), "def f():\n    return 2\n").expect("edit helper");
    let edited_hash = SkillService::compute_dir_hash(&edited).expect("edited hash");
    assert_ne!(
        baseline_hash, edited_hash,
        "修改过的源文件仍必须改变目录哈希"
    );
}

/// 上游仓库自带字节码时，#7772 修复前保存的缓存哈希按旧算法计入了
/// `.pyc`；检查更新要能认出它，而不是对未变化的 Skill 误报「有更新」。
#[test]
fn legacy_cached_hash_with_upstream_bytecode_is_recognized() {
    let dir = tempdir().expect("tempdir");
    let remote = dir.path().join("remote");
    fs::create_dir_all(remote.join("__pycache__")).expect("create remote dir");
    fs::write(remote.join("SKILL.md"), "---\nname: x\n---\n").expect("write skill");
    fs::write(remote.join("helper.py"), "def f():\n    return 1\n").expect("write helper");
    fs::write(remote.join("helper.pyc"), b"\x00 shipped pyc \x00").expect("write pyc");
    fs::write(
        remote.join("__pycache__").join("helper.cpython-311.pyc"),
        b"\x00 shipped nested pyc \x00",
    )
    .expect("write nested pyc");

    let legacy_cached = SkillService::compute_legacy_dir_hash(&remote).expect("legacy hash");
    let new_remote = SkillService::compute_dir_hash(&remote).expect("new hash");
    assert_ne!(legacy_cached, new_remote, "上游带字节码时新旧算法结果不同");
    assert!(SkillService::is_legacy_hash_of(&legacy_cached, &remote));

    // 上游真的改了源文件：旧缓存对不上新内容，仍要报更新。
    let changed = dir.path().join("changed");
    SkillService::copy_dir_recursive(&remote, &changed).expect("copy remote");
    fs::write(changed.join("helper.py"), "def f():\n    return 2\n").expect("edit helper");
    assert!(!SkillService::is_legacy_hash_of(&legacy_cached, &changed));
}

#[test]
#[serial_test::serial]
fn restore_from_backup_rejects_traversal_directory_in_metadata() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());

    // 手工放置一个备份：meta.json 里的 directory 指向 SSOT 之外。
    // SSOT 位于 {home}/.cc-switch/skills，"../../pwned-restore" 若生效会写到 {home}/pwned-restore。
    let backup_id = "20260727_120000_evil";
    let backup_dir = SkillService::get_backup_dir()
        .expect("backup dir")
        .join(backup_id);
    write_skill(&backup_dir.join("skill"), "evil");
    let metadata = SkillBackupMetadata {
        skill: poisoned_skill("owner/repo:evil", "../../pwned-restore"),
        backup_created_at: 0,
        source_path: "x".to_string(),
    };
    fs::write(
        backup_dir.join("meta.json"),
        serde_json::to_string_pretty(&metadata).expect("serialize metadata"),
    )
    .expect("write meta.json");

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let result = SkillService::restore_from_backup(&db, backup_id, &AppType::Claude);

    assert!(
        result.is_err(),
        "restore must reject a traversal directory from meta.json"
    );
    assert!(
        !temp.path().join("pwned-restore").exists(),
        "restore must not write outside the SSOT dir"
    );
}

#[test]
fn create_symlink_error_message_keeps_the_os_error() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let dest = temp.path().join("dest");
    fs::create_dir_all(&source).expect("create source");
    fs::create_dir_all(&dest).expect("occupy dest");

    // 命令层用 to_string() 传给前端，系统错误必须在这一层就可见。
    let message = SkillService::create_symlink(&source, &dest)
        .expect_err("dest already exists")
        .to_string();

    assert!(message.contains("创建符号链接失败"), "{message}");
    assert!(message.contains("os error"), "{message}");
}

#[test]
#[serial_test::serial]
fn remove_from_app_rejects_traversal_directory() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());

    // 受害目录与 app skills 目录都先建好，保证未修复时代码真的能删到它：
    // app_dir = {home}/.claude/skills，"../../victim-remove" 解析为 {home}/victim-remove。
    let victim = temp.path().join("victim-remove");
    fs::create_dir_all(&victim).expect("create victim dir");
    fs::create_dir_all(temp.path().join(".claude").join("skills")).expect("create app dir");

    let result = SkillService::remove_from_app("../../victim-remove", &AppType::Claude);

    assert!(result.is_err(), "remove_from_app must reject traversal");
    assert!(victim.exists(), "victim directory must not be deleted");
}

#[test]
#[serial_test::serial]
fn aliased_skill_roots_reject_sync_and_remove_without_deleting_the_source() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::Unified);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

    let source = SkillService::get_ssot_dir()
        .expect("SSOT")
        .join("test-skill");
    write_skill(&source, "managed");

    let sync = SkillService::sync_to_app_dir("test-skill", &AppType::Pi);
    assert!(sync.is_err(), "an aliased deployment root must be rejected");
    assert!(source.join("SKILL.md").exists());

    let remove = SkillService::remove_from_app("test-skill", &AppType::Pi);
    assert!(
        remove.is_err(),
        "an aliased deployment root must be rejected"
    );
    assert!(
        source.join("SKILL.md").exists(),
        "rejecting the operation must leave the SSOT untouched"
    );
}

#[test]
#[serial_test::serial]
fn migrate_storage_rejects_an_aliased_destination_before_moving_skills() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let source = SkillService::get_ssot_dir()
        .expect("SSOT")
        .join("test-skill");
    write_skill(&source, "managed");

    let migration = SkillService::migrate_storage(&db, SkillStorageLocation::Unified);

    assert!(
        migration.is_err(),
        "migration must reject a target that aliases an app deployment root"
    );
    assert_eq!(
        crate::settings::get_skill_storage_location(),
        SkillStorageLocation::CcSwitch
    );
    assert!(
        source.join("SKILL.md").exists(),
        "validation must happen before any source is moved"
    );
}

#[test]
#[serial_test::serial]
fn migrate_storage_safely_leaves_an_existing_pi_ssot_alias() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::Unified);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join(".agents"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let old_source = SkillService::get_ssot_dir()
        .expect("SSOT")
        .join("test-skill");
    write_skill(&old_source, "managed");

    let result = SkillService::migrate_storage(&db, SkillStorageLocation::CcSwitch)
        .expect("migrate away from alias");
    let new_source = temp
        .path()
        .join(".cc-switch")
        .join("skills")
        .join("test-skill");
    let pi_skill = temp
        .path()
        .join(".agents")
        .join("skills")
        .join("test-skill");

    assert_eq!(result.migrated_count, 1);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert!(new_source.join("SKILL.md").exists());
    assert!(
        pi_skill.join("SKILL.md").exists(),
        "the previously native Pi skill must stay active after migration"
    );
}

#[test]
#[serial_test::serial]
fn uninstall_rejects_traversal_directory_from_db_row() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());

    // 模拟同步导入灌进来的脏数据：directory 含路径穿越（save_skill 不校验，
    // 与 import_sql_string_for_sync 的效果一致）。SSOT = {home}/.cc-switch/skills，
    // "../../victim-uninstall" 解析为 {home}/victim-uninstall。
    let victim = temp.path().join("victim-uninstall");
    fs::create_dir_all(&victim).expect("create victim dir");

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:evil", "../../victim-uninstall");
    db.save_skill(&skill).expect("seed poisoned row");

    let result = SkillService::uninstall(&db, &skill.id)
        .expect("uninstall must remove the poisoned database row");

    // 危险的文件系统操作必须被跳过……
    assert!(victim.exists(), "victim directory must not be deleted");
    // ……但记录本身必须能删掉。db.delete_skill 全项目只有 uninstall 一处调用
    // 且未暴露为命令，若这里返回 Err，脏行就永远无法从界面清除。
    assert!(
        result.pi_cleanup_incomplete,
        "skipped filesystem cleanup must be reported"
    );
    assert!(
        db.get_installed_skill(&skill.id)
            .expect("query skill")
            .is_none(),
        "poisoned row must be deleted from the database"
    );
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn migrate_storage_retargets_managed_pi_mcode_and_kimi_symlinks() {
    for app in [
        AppType::Pi,
        AppType::Mcode,
        AppType::Kimi,
        AppType::Dsh,
        AppType::WorkBuddy,
        AppType::Codebuddy,
    ] {
        let temp = tempdir().expect("tempdir");
        let _home = TestHomeGuard::set(temp.path());
        let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
        let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();

        let db = std::sync::Arc::new(Database::memory().expect("memory db"));
        let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
        skill.apps.set_enabled_for(&app, true);
        db.save_skill(&skill).expect("save skill");
        let old_source = SkillService::get_ssot_dir().unwrap().join("test-skill");
        write_skill(&old_source, "managed");
        SkillService::sync_to_app_dir("test-skill", &app).expect("enable skill");
        let app_skill = SkillService::get_app_skills_dir(&app)
            .unwrap()
            .join("test-skill");
        assert!(SkillService::is_symlink(&app_skill));

        let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
            .expect("migrate storage");
        let new_source = temp
            .path()
            .join(".agents")
            .join("skills")
            .join("test-skill");

        assert_eq!(result.migrated_count, 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(!old_source.exists());
        assert!(new_source.exists());
        assert_eq!(
            app_skill.canonicalize().expect("resolve skill symlink"),
            new_source.canonicalize().expect("resolve new source")
        );
    }
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn migrate_storage_retargets_an_equivalent_relative_pi_symlink() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);
    let _pi_dir = crate::pi_config::test_support::TestAgentDir::at(&temp.path().join("pi-agent"));

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    let old_source = SkillService::get_ssot_dir().unwrap().join("test-skill");
    write_skill(&old_source, "managed");

    let pi_skill = SkillService::get_app_skills_dir(&AppType::Pi)
        .unwrap()
        .join("test-skill");
    fs::create_dir_all(pi_skill.parent().expect("Pi skills directory"))
        .expect("create Pi skills directory");
    std::os::unix::fs::symlink(Path::new("../../.cc-switch/skills/test-skill"), &pi_skill)
        .expect("create relative Pi symlink");

    let result =
        SkillService::migrate_storage(&db, SkillStorageLocation::Unified).expect("migrate storage");
    let new_source = temp
        .path()
        .join(".agents")
        .join("skills")
        .join("test-skill");

    assert_eq!(result.migrated_count, 1);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(
        pi_skill.canonicalize().expect("resolve Pi symlink"),
        new_source.canonicalize().expect("resolve new source")
    );
}

#[cfg(unix)]
#[test]
#[serial_test::serial]
fn migrate_storage_reports_skills_an_app_could_not_relink() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let mut skill = poisoned_skill("owner/repo:skill", "test-skill");
    skill.apps.set_enabled_for(&AppType::Claude, true);
    db.save_skill(&skill).expect("save skill");
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "managed",
    );
    SkillService::sync_to_app_dir("test-skill", &AppType::Claude).expect("enable skill");

    // Claude 的 skills 目录只读：旧链接摘不掉，也建不了新链接。
    let claude_dir = SkillService::get_app_skills_dir(&AppType::Claude).unwrap();
    fs::set_permissions(&claude_dir, fs::Permissions::from_mode(0o555)).unwrap();
    let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified);
    fs::set_permissions(&claude_dir, fs::Permissions::from_mode(0o755)).unwrap();
    let result = result.expect("migration itself completes");

    assert_eq!(result.migrated_count, 1);
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.starts_with("claude/test-skill: ")),
        "{:?}",
        result.errors
    );
}

#[test]
#[serial_test::serial]
fn migrate_storage_reports_an_app_whose_skills_dir_cannot_be_read() {
    let temp = tempdir().expect("tempdir");
    let _home = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_home, SkillStorageLocation::CcSwitch);

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:skill", "test-skill");
    db.save_skill(&skill).expect("save skill");
    write_skill(
        &SkillService::get_ssot_dir().unwrap().join("test-skill"),
        "managed",
    );
    // Codex 的 skills 路径被一个普通文件占了，整应用的重链都做不了。
    let codex_dir = SkillService::get_app_skills_dir(&AppType::Codex).unwrap();
    fs::create_dir_all(codex_dir.parent().unwrap()).unwrap();
    fs::write(&codex_dir, "not a directory").unwrap();

    let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
        .expect("migration itself completes");

    assert_eq!(result.migrated_count, 1);
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.starts_with("codex: ")),
        "{:?}",
        result.errors
    );
}

#[test]
#[serial_test::serial]
fn migrate_storage_skips_bad_rows_without_moving_foreign_dirs() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());
    let _location = StorageLocationGuard::set(&_guard, SkillStorageLocation::CcSwitch);

    // migrate_storage 会 fs::rename / remove_dir_all，脏 directory 能把
    // SSOT 之外的任意目录搬走或删掉。
    let victim = temp.path().join("victim-migrate");
    fs::create_dir_all(&victim).expect("create victim dir");

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    let skill = poisoned_skill("owner/repo:evil", "../../victim-migrate");
    db.save_skill(&skill).expect("seed poisoned row");

    // 必须迁到与当前不同的位置，否则函数在 current == target 处直接短路返回
    let result = SkillService::migrate_storage(&db, SkillStorageLocation::Unified)
        .expect("migration must not abort");

    assert!(victim.exists(), "foreign directory must not be moved away");
    assert_eq!(
        result.migrated_count, 0,
        "poisoned row must not count as migrated"
    );
    assert!(
        !result.errors.is_empty(),
        "the skipped row must be reported through the errors channel"
    );
}

#[test]
#[serial_test::serial]
fn uninstall_backup_source_rejects_traversal_directory() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());

    // 备份源会被整目录复制进 skill-backups 并在界面列出 → 任意文件外泄面。
    let secrets = temp.path().join("secrets");
    fs::create_dir_all(&secrets).expect("create secrets dir");
    fs::write(secrets.join("id_rsa"), b"PRIVATE").expect("write secret");

    let skill = poisoned_skill("owner/repo:evil", "../../secrets");
    let result = SkillService::resolve_uninstall_backup_source_excluding(&skill, &[]);

    assert!(
        result.is_err(),
        "backup source must reject a traversal directory"
    );
}

#[test]
#[serial_test::serial]
fn sync_to_app_skips_bad_rows_instead_of_aborting_the_whole_app() {
    let temp = tempdir().expect("tempdir");
    let _guard = TestHomeGuard::set(temp.path());

    let ssot_dir = SkillService::get_ssot_dir().expect("ssot dir");
    write_skill(&ssot_dir.join("good-skill"), "good");

    let db = std::sync::Arc::new(Database::memory().expect("memory db"));
    // 一条脏行 + 一条正常行。脏行来自同步导入/存量数据，不得连累正常行——
    // sync_to_app 在切换供应商时触发，整体中断会让所有 skill 一起失效。
    //
    // 名字刻意让脏行排前面：查询是 `ORDER BY name ASC`，脏行必须先被处理，
    // 否则「未修复时会中断」这个前提不成立，测试就成了摆设。
    let mut bad = poisoned_skill("owner/repo:bad", "../../escape-sync");
    bad.name = "a-poisoned".to_string();
    bad.apps = SkillApps::only(&AppType::Claude);
    db.save_skill(&bad).expect("seed poisoned row");

    let mut good = poisoned_skill("owner/repo:good", "good-skill");
    good.name = "z-healthy".to_string();
    good.apps = SkillApps::only(&AppType::Claude);
    db.save_skill(&good).expect("seed good row");

    SkillService::sync_to_app(&db, &AppType::Claude).expect("sync must not abort");

    let app_dir = SkillService::get_app_skills_dir(&AppType::Claude).expect("app dir");
    assert!(
        app_dir.join("good-skill").exists(),
        "the healthy skill must still be synced despite the poisoned row"
    );
}

#[test]
// serial：与 backup/s3_sync/deeplink 等同样读写进程级 CC_SWITCH_TEST_HOME 的测试互斥，
// EnvVars 只负责恢复不提供互斥。
#[serial_test::serial]
fn get_app_skills_dir_honors_test_home_override() {
    // 回归：曾直呼 dirs::home_dir() 绕过 CC_SWITCH_TEST_HOME——Unix 上碰巧跟 $HOME
    // 一致所以测试能过，Windows 上 dirs 走 Known Folder API，测试隔离整体失效
    // （tests/skill_sync.rs 扫到 runner 真实用户目录）。
    let temp = tempdir().expect("tempdir");
    let _guard = EnvVars::new().set("CC_SWITCH_TEST_HOME", temp.path());

    let dir =
        SkillService::get_app_skills_dir(&AppType::Claude).expect("resolve claude skills dir");
    assert!(
        dir.starts_with(temp.path()),
        "skills dir must live under the overridden test home, got {}",
        dir.display()
    );
}

#[test]
fn resolve_skill_source_dir_returns_repo_root_for_root_level_skill() {
    let temp = tempdir().expect("tempdir");
    write_skill(temp.path(), "Root Skill");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "last30days-skill-cn")
        .expect("root-level skill should resolve to the extracted repo root");

    assert_eq!(resolved, temp.path());
}

#[test]
fn resolve_skill_source_dir_returns_direct_nested_directory_when_present() {
    let temp = tempdir().expect("tempdir");
    let nested = temp.path().join("skills").join("nested-skill");
    write_skill(&nested, "Nested Skill");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "skills/nested-skill")
        .expect("nested skill should resolve from its relative source path");

    assert_eq!(resolved, nested);
}

#[test]
fn resolve_skill_source_dir_falls_back_to_matching_install_name() {
    let temp = tempdir().expect("tempdir");
    let nested = temp.path().join("skills").join("nested-skill");
    write_skill(&nested, "Nested Skill");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "nested-skill")
        .expect("install name should fall back to the matching discovered skill directory");

    assert_eq!(resolved, nested);
}

#[test]
fn resolve_skill_source_dir_falls_back_to_unique_metadata_name() {
    // tencent/WeChatReading: skills.sh returns `weread-skills`, while the
    // repository stores it at skills/SKILL.md with `name: weread-skills`.
    let temp = tempdir().expect("tempdir");
    let skill_dir = temp.path().join("skills");
    write_skill(&skill_dir, "weread-skills");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "weread-skills")
        .expect("skillId should resolve through the SKILL.md metadata name");

    assert_eq!(resolved, skill_dir);
}

#[test]
fn resolve_skill_source_dir_rejects_duplicate_metadata_names() {
    let temp = tempdir().expect("tempdir");
    write_skill(&temp.path().join("skills-a"), "duplicate-skill");
    write_skill(&temp.path().join("skills-b"), "duplicate-skill");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "duplicate-skill");

    assert!(
        resolved.is_none(),
        "ambiguous metadata names must not select an arbitrary skill directory"
    );
}

#[test]
fn update_lookup_uses_unique_metadata_name_without_overriding_directory_match() {
    let temp = tempdir().expect("tempdir");
    write_skill(&temp.path().join("skills"), "weread-skills");
    let repo = SkillRepo {
        host: default_repo_host(),
        owner: "owner".to_string(),
        name: "repo".to_string(),
        branch: "main".to_string(),
        enabled: true,
    };
    let scan = || {
        let mut skills = Vec::new();
        SkillService::new()
            .scan_dir_recursive(temp.path(), temp.path(), &repo, &mut skills)
            .expect("scan skills");
        skills
    };

    let skills = scan();
    assert_eq!(
        SkillService::find_remote_skill_for_install(&skills, "weread-skills", None)
            .map(|skill| skill.directory.as_str()),
        Some("skills")
    );

    write_skill(&temp.path().join("other"), "weread-skills");
    assert!(
        SkillService::find_remote_skill_for_install(&scan(), "weread-skills", None).is_none(),
        "duplicate metadata names must remain ambiguous during updates"
    );

    write_skill(&temp.path().join("weread-skills"), "Other Skill");
    assert_eq!(
        SkillService::find_remote_skill_for_install(&scan(), "weread-skills", None)
            .map(|skill| skill.directory.as_str()),
        Some("weread-skills"),
        "an exact directory match must keep its original priority"
    );

    let root_only = tempdir().expect("root tempdir");
    write_skill(root_only.path(), "Root Skill");
    let mut root_skills = Vec::new();
    SkillService::new()
        .scan_dir_recursive(root_only.path(), root_only.path(), &repo, &mut root_skills)
        .expect("scan root skill");
    assert!(
        SkillService::find_remote_skill_for_install(&root_skills, "removed-child", None).is_none(),
        "a removed child skill must not fall back to an unrelated root skill"
    );
}

#[test]
fn discovery_lists_one_copy_per_install_name_like_waza_layout() {
    // tw93/Waza 把每个技能放两份：skills/<名> 和 plugins/waza/skills/<名>。
    let temp = tempdir().expect("tempdir");
    for name in ["check", "hunt"] {
        write_skill(&temp.path().join("plugins/waza/skills").join(name), name);
        write_skill(&temp.path().join("skills").join(name), name);
    }
    write_skill(&temp.path().join("b/Utils"), "utils-b");
    write_skill(&temp.path().join("a/utils"), "utils-a");
    let repo = SkillRepo {
        host: default_repo_host(),
        owner: "tw93".to_string(),
        name: "Waza".to_string(),
        branch: "main".to_string(),
        enabled: true,
    };
    let mut skills = Vec::new();
    SkillService::new()
        .scan_dir_recursive(temp.path(), temp.path(), &repo, &mut skills)
        .expect("scan skills");
    assert_eq!(skills.len(), 6, "检查更新要看到每一份，扫描本身不去重");

    SkillService::keep_one_per_install_name(&mut skills);
    let mut directories: Vec<_> = skills.iter().map(|s| s.directory.as_str()).collect();
    directories.sort_unstable();
    assert_eq!(directories, ["a/utils", "skills/check", "skills/hunt"]);
}

#[test]
fn update_lookup_prefers_persisted_source_path_after_metadata_rename() {
    let temp = tempdir().expect("tempdir");
    write_skill(&temp.path().join("skills"), "renamed-skill");
    write_skill(&temp.path().join("weread-skills"), "weread-skills");
    let repo = SkillRepo {
        host: default_repo_host(),
        owner: "owner".to_string(),
        name: "repo".to_string(),
        branch: "main".to_string(),
        enabled: true,
    };
    let mut skills = Vec::new();
    SkillService::new()
        .scan_dir_recursive(temp.path(), temp.path(), &repo, &mut skills)
        .expect("scan skills");

    let stored_url = "https://github.com/owner/repo/blob/main/skills/SKILL.md";
    assert_eq!(
        SkillService::find_remote_skill_for_install(&skills, "weread-skills", Some(stored_url),)
            .map(|skill| skill.directory.as_str()),
        Some("skills"),
        "persisted source path should survive metadata changes and competing matches"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn update_skill_persists_relocated_source_before_metadata_rename() {
    for location in [
        SkillStorageLocation::CcSwitch,
        SkillStorageLocation::Unified,
    ] {
        for (directory, old_path, new_path) in [
            ("weread-skills", "skills", "new-location"),
            ("ordinary-skill", "old/ordinary-skill", "new/ordinary-skill"),
            ("weread-skills", "skills", "."),
        ] {
            let home = tempdir().expect("home");
            let config_dir = home.path().join(".cc-switch");
            fs::create_dir_all(&config_dir).expect("isolated config directory");
            // Keep Windows' legacy-HOME fallback out of this destructive test.
            fs::File::create(config_dir.join("cc-switch.db")).expect("isolated database sentinel");
            let _home = TestHomeGuard::set(home.path());
            assert_eq!(crate::config::get_app_config_dir(), config_dir);
            let _storage = StorageLocationGuard::set(&_home, location);
            let _pi_dir = crate::pi_config::test_support::TestAgentDir::new();
            let remote = tempdir().expect("remote repo");
            let db = Arc::new(Database::memory().expect("memory db"));
            let service = SkillService {
                repo_fixture: Some(remote.path().to_path_buf()),
                ..SkillService::new()
            };
            let mut installed = poisoned_skill("owner/repo:skill", directory);
            installed.name = directory.to_string();
            installed.repo_owner = Some("owner".to_string());
            installed.repo_name = Some("repo".to_string());
            installed.repo_branch = Some("main".to_string());
            installed.readme_url = SkillService::build_skill_doc_url(
                RepoPlatform::GitHub,
                GITHUB_HOST,
                "owner",
                "repo",
                "main",
                &format!("{old_path}/SKILL.md"),
            );
            db.save_skill(&installed).expect("seed installed skill");
            let local = SkillService::get_ssot_dir().unwrap().join(directory);
            assert!(local.starts_with(home.path()), "SSOT must stay isolated");
            write_skill(&local, directory);
            write_skill(&remote.path().join(new_path), directory);

            let updated = service
                .update_skill(&db, &installed.id)
                .await
                .expect("update moved skill");
            let expected_path = if new_path == "." {
                "SKILL.md".to_string()
            } else {
                format!("{new_path}/SKILL.md")
            };
            let expected_url = SkillService::build_skill_doc_url(
                RepoPlatform::GitHub,
                GITHUB_HOST,
                "owner",
                "repo",
                "main",
                &expected_path,
            );
            let saved = db.get_installed_skill(&installed.id).unwrap().unwrap();
            assert_eq!(
                updated.readme_url, expected_url,
                "update must return the resolved source URL"
            );
            assert_eq!(
                saved.readme_url, expected_url,
                "the next update must read the new source URL from the DB"
            );
            assert_eq!(saved.directory, installed.directory);
            assert_eq!(saved.apps, installed.apps);
            assert_eq!(saved.installed_at, installed.installed_at);
            assert_eq!(
                saved.content_hash,
                Some(SkillService::compute_dir_hash(&local).unwrap())
            );
            assert!(service.check_updates(&db).await.unwrap().is_empty());

            write_skill(&remote.path().join(new_path), "renamed-skill");
            // Competing directory/name matches must not override the saved source.
            write_skill(&remote.path().join(directory), directory);
            let updates = service
                .check_updates(&db)
                .await
                .expect("check renamed skill");
            assert_eq!(
                updates.len(),
                1,
                "renamed skill must not silently disappear from update checks"
            );
            let updated = service
                .update_skill(&db, &installed.id)
                .await
                .expect("update renamed skill");
            assert_eq!(updated.name, "renamed-skill");
            assert_eq!(updated.readme_url, expected_url);
            assert_eq!(updated.directory, installed.directory);
            assert_eq!(updated.apps, installed.apps);
            assert_eq!(
                SkillService::read_skill_name_desc(&local.join("SKILL.md"), directory).0,
                "renamed-skill"
            );
            assert!(service.check_updates(&db).await.unwrap().is_empty());
        }
    }
}

#[test]
fn replace_dest_with_copy_rejects_empty_source_without_touching_existing_dest() {
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source-skill");
    let dest = temp.path().join("app-skills").join("source-skill");
    fs::create_dir_all(&source).expect("create empty source");
    write_skill(&dest, "Existing Skill");

    let err = SkillService::replace_dest_with_copy(&source, &dest, "source-skill")
        .expect_err("empty source should not replace existing app skill");

    assert!(
        err.to_string().contains("SKILL.md"),
        "unexpected error: {err:#}"
    );
    assert!(
        dest.join("SKILL.md").is_file(),
        "existing destination skill should be preserved"
    );
}

#[test]
fn resolve_skill_source_dir_rejects_same_name_wrapper_without_skill_md() {
    // 复刻 issue #4141：ast-grep/agent-skill 结构。仓库根下有同名目录 ast-grep/
    // （plugin 包，无 SKILL.md），真正的 skill 在 ast-grep/skills/ast-grep/SKILL.md。
    let temp = tempdir().expect("tempdir");
    let wrapper = temp.path().join("ast-grep");
    fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
    fs::write(
        wrapper.join(".claude-plugin").join("plugin.json"),
        "{\"name\":\"ast-grep\"}",
    )
    .expect("write plugin.json");
    let real_skill = wrapper.join("skills").join("ast-grep");
    write_skill(&real_skill, "ast-grep");

    // directory 只给了 skill 名 "ast-grep"（skills.sh API 的语义），不能命中空壳 wrapper。
    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep")
        .expect("should resolve to the inner skill dir, not the same-name wrapper");

    assert_eq!(resolved, real_skill);
    assert!(resolved.join("SKILL.md").is_file());
}

#[test]
fn resolve_skill_source_dir_finds_two_level_catalog_skill() {
    // catalog layout：skills/category/foo/SKILL.md（depth 3，find_skill_dir_by_name 可达）。
    let temp = tempdir().expect("tempdir");
    let catalog_skill = temp.path().join("skills").join("category").join("foo");
    write_skill(&catalog_skill, "Foo Skill");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo")
        .expect("should resolve the two-level catalog skill by name");

    assert_eq!(resolved, catalog_skill);
}

#[test]
fn resolve_skill_source_dir_returns_none_for_wrapper_without_inner_skill() {
    // 同名 wrapper 存在、无 SKILL.md，且无 inner skill / root SKILL.md 可兜底时，
    // 必须返回 None——守住 #4141 这个 bug class 的负例（不能把空壳目录当源目录）。
    let temp = tempdir().expect("tempdir");
    let wrapper = temp.path().join("ast-grep");
    fs::create_dir_all(wrapper.join(".claude-plugin")).expect("create wrapper plugin dir");
    fs::write(
        wrapper.join(".claude-plugin").join("plugin.json"),
        "{\"name\":\"ast-grep\"}",
    )
    .expect("write plugin.json");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "ast-grep");
    assert!(
        resolved.is_none(),
        "wrapper dir without SKILL.md and no inner skill must resolve to None, got {:?}",
        resolved
    );
}

#[test]
fn resolve_skill_source_dir_returns_none_when_no_skill_md_anywhere() {
    let temp = tempdir().expect("tempdir");
    fs::create_dir_all(temp.path().join("skills").join("foo")).expect("create empty skill dir");
    fs::write(temp.path().join("README.md"), "no skills here").expect("write README");

    let resolved = SkillService::resolve_skill_source_dir(temp.path(), "foo");
    assert!(
        resolved.is_none(),
        "no SKILL.md anywhere must resolve to None"
    );
}

#[test]
fn choose_doc_path_prefers_resolved_source_over_stale_readme_url() {
    // #6111 现场还原：skills.sh 安装嵌套目录 skill——directory 是 skillId
    // （末级目录名），readme_url 是仓库根 URL，真实嵌套路径只能来自
    // 解析出的源目录。若退回"readme_url 提取优先、directory 拼接兜底"，
    // 本断言即失败（旧逻辑产出 alibabacloud-cli-guidance/SKILL.md → 404）。
    let doc_path = SkillService::choose_doc_path(
        Some("skills/developertools/solutions/alibabacloud-cli-guidance/SKILL.md".to_string()),
        Some("https://github.com/aliyun/alibabacloud-aiops-skills"),
        "alibabacloud-cli-guidance",
    );
    assert_eq!(
        doc_path,
        "skills/developertools/solutions/alibabacloud-cli-guidance/SKILL.md"
    );
}

#[test]
fn choose_doc_path_falls_back_to_readme_url_path_then_directory() {
    // 无解析结果（SSOT 目录已存在、未重新下载）时沿用旧 readme_url 的仓库内
    // 路径，兼容 blob/tree 两种格式并补 SKILL.md 后缀
    let doc_path = SkillService::choose_doc_path(
        None,
        Some("https://github.com/o/r/tree/main/skills/foo"),
        "foo",
    );
    assert_eq!(doc_path, "skills/foo/SKILL.md");

    // 旧 readme_url 已是完整文档路径时原样保留
    let doc_path = SkillService::choose_doc_path(
        None,
        Some("https://github.com/o/r/blob/main/skills/foo/SKILL.md"),
        "foo",
    );
    assert_eq!(doc_path, "skills/foo/SKILL.md");

    // 两者都没有时按 directory 拼接
    let doc_path = SkillService::choose_doc_path(None, None, "skills/foo");
    assert_eq!(doc_path, "skills/foo/SKILL.md");
}

#[test]
fn doc_path_for_source_returns_repo_relative_skill_md_path() {
    let temp = tempdir().expect("tempdir");
    let nested = temp
        .path()
        .join("skills")
        .join("developertools")
        .join("solutions")
        .join("foo");
    fs::create_dir_all(&nested).expect("create nested dirs");

    assert_eq!(
        SkillService::doc_path_for_source(temp.path(), &nested),
        Some("skills/developertools/solutions/foo/SKILL.md".to_string())
    );
    // 源目录即仓库根：文档路径就是根下的 SKILL.md
    assert_eq!(
        SkillService::doc_path_for_source(temp.path(), temp.path()),
        Some("SKILL.md".to_string())
    );
    // 仓库根之外：None（调用方已做包含性校验，防御性兜底）
    assert_eq!(
        SkillService::doc_path_for_source(temp.path(), std::path::Path::new("/elsewhere")),
        None
    );
}

fn repo(owner: &str, name: &str, enabled: bool) -> SkillRepo {
    SkillRepo {
        host: default_repo_host(),
        owner: owner.to_string(),
        name: name.to_string(),
        branch: "main".to_string(),
        enabled,
    }
}

#[tokio::test]
async fn discover_report_lists_unreadable_repos_and_ignores_disabled_ones() {
    let remote = tempdir().expect("remote repo");
    write_skill(&remote.path().join("pdf"), "pdf");
    let service = SkillService {
        repo_fixture: Some(remote.path().to_path_buf()),
        ..SkillService::new()
    };

    let report = service
        .discover_available_report(
            vec![
                repo("owner", "repo", true),
                // 坐标不合法：下载前就失败，不碰网络
                repo("bad owner!", "broken", true),
                // 停用的仓库不读，也不该出现在失败里
                repo("bad owner?", "disabled", false),
            ],
            false,
        )
        .await
        .expect("discover report");

    assert_eq!(
        report
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        vec!["pdf"]
    );
    assert_eq!(report.failures.len(), 1);
    let failure = &report.failures[0];
    assert_eq!(
        (
            failure.owner.as_str(),
            failure.name.as_str(),
            failure.branch.as_str()
        ),
        ("bad owner!", "broken", "main")
    );
    assert!(
        failure.error.contains("INVALID_REPO_REF"),
        "unexpected error: {}",
        failure.error
    );

    // 旧接口只取 skills，行为不变
    let skills = service
        .discover_available(vec![
            repo("owner", "repo", true),
            repo("bad owner!", "x", true),
        ])
        .await
        .expect("discover");
    assert_eq!(skills.len(), 1);
}

#[tokio::test]
async fn discover_reuses_cached_repos_unless_forced() {
    let remote = tempdir().expect("remote repo");
    write_skill(&remote.path().join("pdf"), "pdf");
    let service = SkillService {
        repo_fixture: Some(remote.path().to_path_buf()),
        ..SkillService::new()
    };
    let repos = || vec![repo("owner", "repo", true)];

    let first = service
        .discover_available_report(repos(), false)
        .await
        .expect("first discover");
    assert_eq!(first.skills.len(), 1);

    // 仓库里多了一个 Skill：缓存期内不重新下载，强制刷新才看得到
    write_skill(&remote.path().join("docx"), "docx");
    let cached = service
        .discover_available_report(repos(), false)
        .await
        .expect("cached discover");
    assert_eq!(cached.skills.len(), 1);
    let single = service.discover_repo(&repo("owner", "repo", true)).await;
    assert_eq!(single.skills.len(), 1);

    let forced = service
        .discover_available_report(repos(), true)
        .await
        .expect("forced discover");
    assert_eq!(forced.skills.len(), 2);
    // 强制刷新的结果写回缓存
    let single = service.discover_repo(&repo("owner", "repo", true)).await;
    assert_eq!(single.skills.len(), 2);
}

#[tokio::test]
async fn discover_repo_reports_failure_and_skips_disabled_repo() {
    let service = SkillService::new();

    let failed = service
        .discover_repo(&repo("bad owner!", "broken", true))
        .await;
    assert!(failed.skills.is_empty());
    assert_eq!(failed.failures.len(), 1);
    assert!(failed.failures[0].error.contains("INVALID_REPO_REF"));

    let disabled = service
        .discover_repo(&repo("bad owner!", "broken", false))
        .await;
    assert!(disabled.skills.is_empty());
    assert!(disabled.failures.is_empty());
}

#[tokio::test]
#[serial_test::serial]
async fn check_updates_report_lists_unreadable_repos() {
    let home = tempdir().expect("home");
    let _home = TestHomeGuard::set(home.path());
    let db = Arc::new(Database::memory().expect("memory db"));
    let mut installed = poisoned_skill("bad owner!/broken:skill", "skill");
    installed.repo_owner = Some("bad owner!".to_string());
    installed.repo_name = Some("broken".to_string());
    installed.repo_branch = Some("main".to_string());
    db.save_skill(&installed).expect("seed installed skill");

    let report = SkillService::new()
        .check_updates_report(&db)
        .await
        .expect("check updates report");

    assert!(report.updates.is_empty());
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.failures[0].owner, "bad owner!");
    assert_eq!(report.failures[0].name, "broken");
    assert!(report.failures[0].error.contains("INVALID_REPO_REF"));
}

fn build_skills_zip(dirs: &[&str]) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();
        for dir in dirs {
            zip.start_file(format!("{dir}/SKILL.md"), opts).unwrap();
            zip.write_all(format!("---\nname: {dir}\ndescription: zipped\n---\n").as_bytes())
                .unwrap();
        }
        zip.finish().unwrap();
    }
    buf
}

/// #3749：ZIP 里和已安装 Skill 同名的被跳过时要如实返回，而不是让界面说「ZIP 中没有技能」。
#[test]
#[serial_test::serial]
fn install_from_zip_reports_skills_skipped_for_taken_directory() {
    let home = tempdir().expect("home");
    let _home = TestHomeGuard::set(home.path());
    let db = Arc::new(Database::memory().expect("memory db"));
    let mut existing = poisoned_skill("local:my-helper", "my-helper");
    existing.name = "My Helper".to_string();
    db.save_skill(&existing).expect("seed existing skill");

    let zip_path = home.path().join("team.zip");
    fs::write(&zip_path, build_skills_zip(&["My-Helper", "fresh"])).expect("write zip");

    let result =
        SkillService::install_from_zip(&db, &zip_path, &AppType::Claude).expect("install from zip");

    assert_eq!(
        result
            .installed
            .iter()
            .map(|skill| skill.directory.as_str())
            .collect::<Vec<_>>(),
        vec!["fresh"]
    );
    assert_eq!(
        result.skipped,
        vec![ZipSkippedSkill {
            directory: "My-Helper".to_string(),
            existing_id: "local:my-helper".to_string(),
            existing_name: "My Helper".to_string(),
        }]
    );

    // 全部同名：不再报错，安装列表为空、跳过列表如实给出
    let only_taken = home.path().join("taken.zip");
    fs::write(&only_taken, build_skills_zip(&["fresh"])).expect("write zip");
    let result = SkillService::install_from_zip(&db, &only_taken, &AppType::Claude)
        .expect("all-skipped ZIP is not an error");
    assert!(result.installed.is_empty());
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].existing_id, "local:fresh");
}

#[test]
#[serial_test::serial]
fn resync_all_apps_reports_each_app_and_failed_skills() {
    let home = tempdir().expect("home");
    let _home = TestHomeGuard::set(home.path());
    let ssot_dir = SkillService::get_ssot_dir().expect("ssot dir");
    write_skill(&ssot_dir.join("good-skill"), "good");

    let db = Arc::new(Database::memory().expect("memory db"));
    let mut good = poisoned_skill("owner/repo:good", "good-skill");
    good.name = "good".to_string();
    good.apps = SkillApps::only(&AppType::Claude);
    good.apps.codex = true;
    db.save_skill(&good).expect("seed good row");
    let mut bad = poisoned_skill("owner/repo:bad", "../../escape-sync");
    bad.apps = SkillApps::only(&AppType::Claude);
    db.save_skill(&bad).expect("seed poisoned row");

    let outcomes = SkillService::resync_all_apps(&db);

    let apps: Vec<&str> = outcomes.iter().map(|o| o.app.as_str()).collect();
    assert!(!apps.contains(&"claude-desktop") && !apps.contains(&"pi"));
    assert!(apps.contains(&"claude") && apps.contains(&"codex"));

    let claude = outcomes.iter().find(|o| o.app == "claude").unwrap();
    assert!(!claude.ok);
    assert!(claude.error.is_none());
    assert_eq!(claude.failed_skills.len(), 1);
    assert_eq!(claude.failed_skills[0].directory, "../../escape-sync");

    let codex = outcomes.iter().find(|o| o.app == "codex").unwrap();
    assert!(codex.ok, "codex should sync: {codex:?}");
    for app in [AppType::Claude, AppType::Codex] {
        let dir = SkillService::get_app_skills_dir(&app).expect("app dir");
        assert!(dir.join("good-skill").join("SKILL.md").exists());
    }
    assert!(outcomes.iter().filter(|o| o.app != "claude").all(|o| o.ok));
}

/// OpenClaw 不支持 Skills，`~/.openclaw/skills` 归它自己：里面和受管 Skill 同名的
/// 真实目录不能被当成「已关掉的投影」删掉。
#[test]
#[serial_test::serial]
fn resync_all_apps_leaves_openclaw_skills_dir_alone() {
    let home = tempdir().expect("home");
    let _home = TestHomeGuard::set(home.path());
    let ssot_dir = SkillService::get_ssot_dir().expect("ssot dir");
    write_skill(&ssot_dir.join("shared-name"), "managed copy");

    let openclaw_dir = SkillService::get_app_skills_dir(&AppType::OpenClaw).expect("dir");
    let own_skill = openclaw_dir.join("shared-name").join("SKILL.md");
    fs::create_dir_all(own_skill.parent().unwrap()).unwrap();
    fs::write(&own_skill, "openclaw's own skill").unwrap();
    write_skill(&openclaw_dir.join("other"), "untouched");

    let db = Arc::new(Database::memory().expect("memory db"));
    let mut skill = poisoned_skill("owner/repo:shared", "shared-name");
    skill.name = "shared".to_string();
    skill.apps = SkillApps::only(&AppType::Claude);
    db.save_skill(&skill).expect("seed row");

    let outcomes = SkillService::resync_all_apps(&db);

    assert!(outcomes.iter().all(|o| o.app != "openclaw"));
    assert!(outcomes.iter().all(|o| o.ok), "{outcomes:?}");
    assert_eq!(
        fs::read_to_string(&own_skill).unwrap(),
        "openclaw's own skill"
    );
    assert!(openclaw_dir.join("other").join("SKILL.md").exists());

    // 切换供应商走的同一条同步也不能碰它
    SkillService::sync_to_app(&db, &AppType::OpenClaw).expect("no-op sync");
    assert!(own_skill.exists());
}
