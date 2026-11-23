//! skills.sh 搜索。

use super::*;

impl SkillService {
    // ========== skills.sh 搜索 ==========

    /// 搜索 skills.sh 公共目录
    pub async fn search_skills_sh(
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<SkillsShSearchResult> {
        let client = crate::proxy::http_client::get();

        let url = url::Url::parse_with_params(
            "https://skills.sh/api/search",
            &[
                ("q", query),
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ],
        )?;

        let resp = client
            .get(url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json::<SkillsShApiResponse>()
            .await?;

        let skills = resp
            .skills
            .into_iter()
            .filter_map(|s| {
                let parts: Vec<&str> = s.source.splitn(2, '/').collect();
                if parts.len() != 2 {
                    return None;
                }
                let (owner, repo) = (parts[0].to_string(), parts[1].to_string());
                // 用与 download_repo 同一套坐标校验，而不是就地写启发式：下面这个
                // readme_url 最终交给 openExternal 打开，是和 build_skill_doc_url
                // 同一个 sink。原来的 `contains('.')` 既漏（`splitn(2, '/')` 允许
                // repo 里带 `/`，`owner/a/b` 能拼出三段路径），又误伤（GitHub 仓库
                // 名合法含点）。校验 owner 同时也保留了"过滤非 GitHub 来源"的效果
                // ——`skills.volces.com` 这类带点的 owner 本来就不是合法用户名。
                if Self::validate_repo_ref(&owner, &repo, "main").is_err() {
                    return None;
                }
                Some(SkillsShDiscoverableSkill {
                    key: s.id,
                    name: s.name,
                    directory: s.skill_id.clone(),
                    repo_owner: owner.clone(),
                    repo_name: repo.clone(),
                    repo_branch: "main".to_string(),
                    installs: s.installs,
                    readme_url: Some(format!("https://github.com/{}/{}", owner, repo)),
                })
            })
            .collect();

        Ok(SkillsShSearchResult {
            skills,
            total_count: resp.count,
            query: resp.query,
        })
    }
}
