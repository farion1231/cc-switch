//! 项目 Profile 快照/应用的端到端集成测试
//!
//! 全链路 apply 会写 live 配置文件——support.rs 已把 HOME 指向临时目录，安全。

use std::fs;

use serde_json::json;

use cc_switch_lib::{
    AppType, InstalledSkill, McpServer, McpService, ProfilePayload, ProfileService, Prompt,
    PromptService, Provider, ProviderService, SkillApps, SkillService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn claude_provider(id: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": token,
                "ANTHROPIC_BASE_URL": "https://api.test"
            }
        }),
        None,
    )
}

/// Claude Desktop 供应商：无 meta 时默认 Direct 模式，只要求 env 里有 token + base_url
fn desktop_provider(id: &str, token: &str) -> Provider {
    Provider::with_id(
        id.to_string(),
        id.to_uppercase(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": token,
                "ANTHROPIC_BASE_URL": "https://desktop.test"
            }
        }),
        None,
    )
}

fn mcp_server(id: &str, claude_enabled: bool) -> McpServer {
    serde_json::from_value(json!({
        "id": id,
        "name": id,
        "server": { "command": "echo", "args": [] },
        "apps": { "claude": claude_enabled }
    }))
    .expect("construct mcp server")
}

fn prompt(id: &str, enabled: bool) -> Prompt {
    Prompt {
        id: id.to_string(),
        name: id.to_uppercase(),
        content: format!("# prompt {id}\n"),
        description: None,
        enabled,
        created_at: Some(1_000),
        updated_at: Some(1_000),
    }
}

fn installed_skill(id: &str, directory: &str, claude_enabled: bool) -> InstalledSkill {
    InstalledSkill {
        id: id.to_string(),
        name: id.to_string(),
        description: None,
        directory: directory.to_string(),
        repo_owner: None,
        repo_name: None,
        repo_branch: None,
        readme_url: None,
        apps: SkillApps {
            claude: claude_enabled,
            ..Default::default()
        },
        installed_at: 1_000,
        content_hash: None,
        updated_at: 0,
    }
}

fn write_ssot_skill(directory: &str) {
    let dir = SkillService::get_ssot_dir()
        .expect("resolve skills SSOT dir")
        .join(directory);
    fs::create_dir_all(&dir).expect("create skill dir");
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {directory}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

#[test]
fn profile_snapshot_apply_roundtrip_restores_configuration() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    // ---- 种子数据：2 个 Claude 供应商（p1 为当前）+ 2 个 MCP + 1 个 Skill + 2 个 Prompt ----
    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("p1", "key-1"))
        .expect("save provider p1");
    state
        .db
        .save_provider(AppType::Claude.as_str(), &claude_provider("p2", "key-2"))
        .expect("save provider p2");
    state
        .db
        .set_current_provider(AppType::Claude.as_str(), "p1")
        .expect("set current provider p1");

    // Claude Desktop 只有供应商一个活跃维度（MCP/Skills/Prompt 对它不适用）
    state
        .db
        .save_provider(
            AppType::ClaudeDesktop.as_str(),
            &desktop_provider("d1", "dk-1"),
        )
        .expect("save desktop provider d1");
    state
        .db
        .save_provider(
            AppType::ClaudeDesktop.as_str(),
            &desktop_provider("d2", "dk-2"),
        )
        .expect("save desktop provider d2");
    state
        .db
        .set_current_provider(AppType::ClaudeDesktop.as_str(), "d1")
        .expect("set current desktop provider d1");

    // 让 live settings.json 与 p1 一致（switch_normal 回填需要）
    let claude_dir = home.join(".claude");
    fs::create_dir_all(&claude_dir).expect("create .claude dir");
    fs::write(
        claude_dir.join("settings.json"),
        serde_json::to_string_pretty(&claude_provider("p1", "key-1").settings_config)
            .expect("serialize p1 settings"),
    )
    .expect("seed live settings.json");

    state
        .db
        .save_mcp_server(&mcp_server("m1", true))
        .expect("save mcp m1");
    state
        .db
        .save_mcp_server(&mcp_server("m2", false))
        .expect("save mcp m2");

    write_ssot_skill("test-skill");
    state
        .db
        .save_skill(&installed_skill("local:test-skill", "test-skill", true))
        .expect("save skill");

    state
        .db
        .save_prompt(AppType::Claude.as_str(), &prompt("pr1", true))
        .expect("save prompt pr1");
    state
        .db
        .save_prompt(AppType::Claude.as_str(), &prompt("pr2", false))
        .expect("save prompt pr2");

    // ---- 保存项目 A（快照当前状态）----
    let profile_a = ProfileService::create(&state, "Project A").expect("create profile A");
    let payload: ProfilePayload =
        serde_json::from_str(&profile_a.payload).expect("parse profile A payload");
    assert_eq!(payload.providers.claude.as_deref(), Some("p1"));
    assert_eq!(payload.mcp.claude, vec!["m1".to_string()]);
    assert_eq!(payload.skills.claude, vec!["local:test-skill".to_string()]);
    assert_eq!(payload.prompts.claude.as_deref(), Some("pr1"));
    assert_eq!(
        payload.providers.codex, None,
        "codex has no current provider"
    );
    assert!(payload.mcp.codex.is_empty());
    assert_eq!(payload.providers.claude_desktop.as_deref(), Some("d1"));
    assert!(
        payload.mcp.claude_desktop.is_empty() && payload.skills.claude_desktop.is_empty(),
        "desktop has no MCP/Skills dimension"
    );
    assert_eq!(payload.prompts.claude_desktop, None);

    // ---- 改动全部四类配置（走真实切换路径）----
    ProviderService::switch(&state, AppType::Claude, "p2").expect("switch to p2");
    // Desktop live 写入仅 macOS/Windows 可用；其他平台不动 d1，
    // apply 时命中幂等跳过（这正是 Linux 上带 Desktop 槽的 payload 应有行为）
    #[cfg(any(target_os = "macos", windows))]
    ProviderService::switch(&state, AppType::ClaudeDesktop, "d2").expect("switch desktop to d2");
    McpService::toggle_app(&state, "m1", AppType::Claude, false).expect("disable m1");
    McpService::toggle_app(&state, "m2", AppType::Claude, true).expect("enable m2");
    SkillService::toggle_app(&state.db, "local:test-skill", &AppType::Claude, false)
        .expect("disable skill");
    PromptService::enable_prompt(&state, AppType::Claude, "pr2").expect("enable pr2");

    // ---- 应用项目 A：全部复原 ----
    let warnings = ProfileService::apply(&state, &profile_a.id).expect("apply profile A");
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let current = state
        .db
        .get_current_provider(AppType::Claude.as_str())
        .expect("get current provider");
    assert_eq!(current.as_deref(), Some("p1"), "provider restored to p1");

    let current_desktop = state
        .db
        .get_current_provider(AppType::ClaudeDesktop.as_str())
        .expect("get current desktop provider");
    assert_eq!(
        current_desktop.as_deref(),
        Some("d1"),
        "desktop provider restored to d1"
    );

    let servers = state.db.get_all_mcp_servers().expect("get mcp servers");
    assert!(servers.get("m1").expect("m1").apps.claude, "m1 re-enabled");
    assert!(!servers.get("m2").expect("m2").apps.claude, "m2 disabled");

    let skills = state.db.get_all_installed_skills().expect("get skills");
    assert!(
        skills.get("local:test-skill").expect("skill").apps.claude,
        "skill re-enabled"
    );

    let prompts = state
        .db
        .get_prompts(AppType::Claude.as_str())
        .expect("get prompts");
    assert!(prompts.get("pr1").expect("pr1").enabled, "pr1 re-enabled");
    assert!(!prompts.get("pr2").expect("pr2").enabled, "pr2 disabled");

    let live_prompt = fs::read_to_string(claude_dir.join("CLAUDE.md")).expect("read CLAUDE.md");
    assert_eq!(
        live_prompt,
        prompt("pr1", true).content,
        "live memory file restored"
    );

    assert_eq!(
        state
            .db
            .get_current_profile_id()
            .expect("get current profile id")
            .as_deref(),
        Some(profile_a.id.as_str()),
        "profile A marked as current"
    );
}

#[test]
fn profile_apply_reports_dangling_references_and_continues() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    state
        .db
        .save_mcp_server(&mcp_server("m1", false))
        .expect("save mcp m1");

    // 手工构造引用了不存在资源的 payload
    let payload = json!({
        "providers": { "claude": "ghost-provider" },
        "mcp": { "claude": ["m1", "ghost-mcp"] },
        "skills": { "claude": ["ghost-skill"] },
        "prompts": { "claude": "ghost-prompt" }
    });
    let profile = cc_switch_lib::Profile {
        id: "dangling-test".to_string(),
        name: "Dangling".to_string(),
        payload: payload.to_string(),
        sort_order: None,
        created_at: Some(1_000),
        updated_at: Some(1_000),
    };
    state.db.save_profile(&profile).expect("save profile");

    let warnings = ProfileService::apply(&state, "dangling-test").expect("apply succeeds");
    assert_eq!(
        warnings.len(),
        4,
        "each dangling reference yields one warning: {warnings:?}"
    );

    // 有效条目照常生效：m1 被启用
    let servers = state.db.get_all_mcp_servers().expect("get mcp servers");
    assert!(
        servers.get("m1").expect("m1").apps.claude,
        "m1 enabled despite warnings"
    );

    // best-effort 完成后仍标记为当前项目
    assert_eq!(
        state
            .db
            .get_current_profile_id()
            .expect("get current profile id")
            .as_deref(),
        Some("dangling-test")
    );
}

#[test]
fn clear_current_profile_only_clears_marker() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    state
        .db
        .set_current_profile_id(Some("some-profile"))
        .expect("set current profile");
    state
        .db
        .set_current_profile_id(None)
        .expect("clear current profile");
    assert_eq!(
        state
            .db
            .get_current_profile_id()
            .expect("get current profile id"),
        None
    );
}
