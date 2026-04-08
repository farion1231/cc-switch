//! Skill command handlers

use crate::cli::{SkillCommands, SkillRepoCommands, SkillUnmanagedCommands};
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use cc_switch_core::{AppState, DiscoverableSkill, Skill, SkillRepo, SkillService};
use std::path::{Path, PathBuf};

pub async fn handle(cmd: SkillCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SkillCommands::List => handle_list(state, printer).await,
        SkillCommands::Search { keyword } => handle_search(&keyword, state, printer).await,
        SkillCommands::Install { id, app } => handle_install(&id, &app, state, printer).await,
        SkillCommands::Uninstall { id, yes } => handle_uninstall(&id, yes, state, printer).await,
        SkillCommands::Enable { id, app } => handle_toggle(&id, &app, true, state, printer).await,
        SkillCommands::Disable { id, app } => handle_toggle(&id, &app, false, state, printer).await,
        SkillCommands::Unmanaged { cmd } => handle_unmanaged(cmd, state, printer).await,
        SkillCommands::Repo { cmd } => handle_repo(cmd, state, printer).await,
        SkillCommands::ZipInstall { file, app } => {
            handle_zip_install(&file, &app, state, printer).await
        }
    }
}

async fn handle_list(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let skills = cc_switch_core::SkillService::get_all_installed(&state.db)?;
    printer.print_skills(&skills)?;
    Ok(())
}

async fn handle_search(keyword: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let service = SkillService::new();
    let repos = skill_repos_or_default(state)?;
    let keyword_lower = keyword.to_lowercase();
    let skills = service
        .list_skills(repos, &state.db)
        .await?
        .into_iter()
        .filter(|skill| skill_matches(skill, &keyword_lower))
        .collect::<Vec<_>>();

    printer.print_value(&skills)?;
    Ok(())
}

async fn handle_install(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let service = SkillService::new();
    let repos = skill_repos_or_default(state)?;
    let skills = service.discover_available(repos).await?;
    let skill = find_discoverable_skill(&skills, id)
        .ok_or_else(|| anyhow::anyhow!("Skill not found: {}", id))?;

    let installed = service.install(&state.db, skill, &app_type).await?;
    printer.print_value(&installed)?;
    Ok(())
}

async fn handle_unmanaged(
    cmd: SkillUnmanagedCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        SkillUnmanagedCommands::Scan => {
            let skills = SkillService::scan_unmanaged(&state.db)?;
            printer.print_value(&skills)?;
        }
        SkillUnmanagedCommands::Import { directories } => {
            anyhow::ensure!(
                !directories.is_empty(),
                "Please provide at least one unmanaged skill directory to import."
            );
            let imported = SkillService::import_from_apps(&state.db, directories)?;
            printer.print_value(&imported)?;
        }
    }
    Ok(())
}

async fn handle_repo(
    cmd: SkillRepoCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    ensure_skill_repo_store_initialized(state)?;

    match cmd {
        SkillRepoCommands::List => {
            let repos = state.db.get_skill_repos()?;
            printer.print_value(&repos)?;
        }
        SkillRepoCommands::Add {
            repo,
            branch,
            disabled,
        } => {
            let (owner, name) = parse_repo_spec(&repo)?;
            let repo = SkillRepo {
                owner,
                name,
                branch,
                enabled: !disabled,
            };
            state.db.save_skill_repo(&repo)?;
            printer.print_value(&repo)?;
        }
        SkillRepoCommands::Remove { repo } => {
            let (owner, name) = parse_repo_spec(&repo)?;
            state.db.delete_skill_repo(&owner, &name)?;
            printer.success(format!("✓ Removed skill repo {owner}/{name}"));
        }
    }

    Ok(())
}

async fn handle_zip_install(
    file: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let path = PathBuf::from(file);
    anyhow::ensure!(path.exists(), "ZIP file not found: {}", path.display());

    let installed = SkillService::install_from_zip(&state.db, &path, &app_type)?;
    printer.print_value(&installed)?;
    Ok(())
}

async fn handle_uninstall(
    id: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!("Skill uninstall is destructive. Re-run with --yes to confirm.");
    }

    cc_switch_core::SkillService::uninstall(&state.db, id)?;
    printer.success(format!("✓ Uninstalled skill '{}'", id));
    Ok(())
}

async fn handle_toggle(
    id: &str,
    app: &str,
    enabled: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::SkillService::toggle_app(&state.db, id, &app_type, enabled)?;
    let action = if enabled { "enabled" } else { "disabled" };
    printer.success(format!("✓ {} skill '{}' for {}", action, id, app));
    Ok(())
}

fn skill_repos_or_default(state: &AppState) -> anyhow::Result<Vec<cc_switch_core::SkillRepo>> {
    ensure_skill_repo_store_initialized(state)?;
    cc_switch_core::SkillService::get_repos_or_default(&state.db).map_err(Into::into)
}

fn skill_matches(skill: &Skill, keyword: &str) -> bool {
    skill.key.to_lowercase().contains(keyword)
        || skill.name.to_lowercase().contains(keyword)
        || skill.description.to_lowercase().contains(keyword)
        || skill.directory.to_lowercase().contains(keyword)
}

fn find_discoverable_skill<'a>(
    skills: &'a [DiscoverableSkill],
    query: &str,
) -> Option<&'a DiscoverableSkill> {
    let exact = skills.iter().find(|skill| {
        skill.key.eq_ignore_ascii_case(query)
            || skill.directory.eq_ignore_ascii_case(query)
            || skill.name.eq_ignore_ascii_case(query)
            || install_name(&skill.directory).is_some_and(|name| name.eq_ignore_ascii_case(query))
    });
    if exact.is_some() {
        return exact;
    }

    let query_lower = query.to_lowercase();
    skills.iter().find(|skill| {
        skill.key.to_lowercase().contains(&query_lower)
            || skill.name.to_lowercase().contains(&query_lower)
            || skill.description.to_lowercase().contains(&query_lower)
            || skill.directory.to_lowercase().contains(&query_lower)
    })
}

fn install_name(directory: &str) -> Option<String> {
    Path::new(directory)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
}

fn ensure_skill_repo_store_initialized(state: &AppState) -> anyhow::Result<()> {
    state.db.init_default_skill_repos()?;
    Ok(())
}

fn parse_repo_spec(spec: &str) -> anyhow::Result<(String, String)> {
    let trimmed = spec.trim();
    anyhow::ensure!(
        !trimmed.is_empty(),
        "Skill repo cannot be empty. Use owner/name or https://github.com/owner/name."
    );

    let without_scheme = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("github.com/"))
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .trim_end_matches(".git");

    let parts: Vec<_> = without_scheme
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    anyhow::ensure!(
        parts.len() == 2,
        "Invalid skill repo '{trimmed}'. Use owner/name or https://github.com/owner/name."
    );

    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_discoverable_skill_matches_install_directory_name() {
        let skills = vec![DiscoverableSkill {
            key: "repo:code-review".to_string(),
            name: "Code Review".to_string(),
            description: "Review helper".to_string(),
            directory: "tools/code-review".to_string(),
            readme_url: None,
            repo_owner: "owner".to_string(),
            repo_name: "repo".to_string(),
            repo_branch: "main".to_string(),
        }];

        let found = find_discoverable_skill(&skills, "code-review").expect("skill should match");
        assert_eq!(found.key, "repo:code-review");
    }

    #[test]
    fn parse_repo_spec_accepts_owner_name_and_github_url() {
        assert_eq!(
            parse_repo_spec("owner/repo").expect("owner/name"),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            parse_repo_spec("https://github.com/owner/repo.git").expect("github url"),
            ("owner".to_string(), "repo".to_string())
        );
    }

    #[test]
    fn parse_repo_spec_accepts_github_host_without_scheme() {
        assert_eq!(
            parse_repo_spec("github.com/owner/repo").expect("github.com path"),
            ("owner".to_string(), "repo".to_string())
        );
    }

    #[test]
    fn parse_repo_spec_rejects_invalid_inputs() {
        assert!(parse_repo_spec("").is_err());
        assert!(parse_repo_spec("owner").is_err());
        assert!(parse_repo_spec("owner/repo/extra").is_err());
    }
}
