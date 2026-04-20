use super::*;

#[test]
fn extract_repo_source_from_gitlab_doc_url() {
    let (repo_url, kind) = SkillService::extract_repo_source_from_doc_url(
        "https://gitlab.com/group/subgroup/repo/-/blob/main/skills/demo/SKILL.md",
    )
    .expect("should extract repo source");

    assert_eq!(repo_url, "https://gitlab.com/group/subgroup/repo");
    assert_eq!(kind, RepoSourceKind::Gitlab);
}

#[test]
fn build_gitlab_urls_from_repo_source() {
    let repo = SkillRepo {
        owner: "gitlab.com/group/subgroup".to_string(),
        name: "repo".to_string(),
        url: Some("https://gitlab.com/group/subgroup/repo".to_string()),
        branch: "main".to_string(),
        enabled: true,
    };

    let readme_url = SkillService::build_skill_doc_url(
        &repo,
        "feature/test",
        "skills/demo/SKILL.md",
        RepoSourceKind::Gitlab,
    );
    assert_eq!(
        readme_url,
        "https://gitlab.com/group/subgroup/repo/-/blob/feature%2Ftest/skills/demo/SKILL.md"
    );

    let candidates = SkillService::repo_download_candidates(&repo, "feature/test");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates.first().map(|(_, kind)| *kind),
        Some(RepoSourceKind::Gitlab)
    );
    assert!(candidates.iter().any(|(url, kind)| {
        *kind == RepoSourceKind::Gitlab
            && url.contains("/api/v4/projects/group%2Fsubgroup%2Frepo/repository/archive.zip")
            && url.contains("sha=feature%2Ftest")
    }));
}

#[test]
fn infer_repo_source_kind_prefers_host_over_path_words() {
    let kind = SkillService::infer_repo_source_kind("https://git.example.com/tree/team/repo")
        .expect("should infer repo source");

    assert_eq!(kind, RepoSourceKind::Gitlab);
}
