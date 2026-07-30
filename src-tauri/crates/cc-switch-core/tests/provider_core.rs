use cc_switch_core::{HeadlessState, ProviderRecord, ProviderService, ProviderSortUpdate};
use serde_json::json;

fn provider(id: &str, name: &str, token: &str) -> ProviderRecord {
    // 测试构造器显式覆盖完整 DTO，字段扩展时这里会促使 CRUD 回归同步更新。
    ProviderRecord {
        id: id.to_string(),
        name: name.to_string(),
        settings_config: json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com",
                "ANTHROPIC_AUTH_TOKEN": token
            }
        }),
        website_url: None,
        category: Some("custom".to_string()),
        created_at: None,
        sort_index: None,
        notes: None,
        icon: None,
        icon_color: None,
        meta: None,
        in_failover_queue: false,
    }
}

#[test]
fn headless_provider_service_completes_crud_switch_sort_and_live_write() {
    let home = tempfile::tempdir().expect("创建隔离 HOME");
    let state = HeadlessState::memory(home.path()).expect("创建无界面状态");

    let first = provider("remote-a", "Remote A", "sk-a");
    let second = provider("remote-b", "Remote B", "sk-b");
    assert!(ProviderService::add(&state, "claude", first, true).expect("新增首个供应商"));
    assert!(ProviderService::add(&state, "claude", second, false).expect("新增第二个供应商"));

    assert_eq!(
        ProviderService::current(&state, "claude").expect("读取当前供应商"),
        "remote-a"
    );
    let live_path = home.path().join(".claude").join("settings.json");
    let live: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&live_path).expect("首个供应商应写入 live 配置"))
            .expect("解析 live 配置");
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-a");

    ProviderService::switch(&state, "claude", "remote-b").expect("切换供应商");
    let live: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&live_path).expect("切换后应重写 live 配置"))
            .expect("解析切换后的 live 配置");
    assert_eq!(live["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-b");

    let updated = provider("remote-b", "Remote B Updated", "sk-b2");
    assert!(ProviderService::update(&state, "claude", "remote-b", updated).expect("更新供应商"));
    ProviderService::update_sort_order(
        &state,
        "claude",
        &[
            ProviderSortUpdate {
                id: "remote-b".to_string(),
                sort_index: 0,
            },
            ProviderSortUpdate {
                id: "remote-a".to_string(),
                sort_index: 1,
            },
        ],
    )
    .expect("更新排序");

    let listed = ProviderService::list(&state, "claude").expect("列出供应商");
    assert_eq!(
        listed.keys().map(String::as_str).collect::<Vec<_>>(),
        ["remote-b", "remote-a"]
    );
    assert_eq!(listed["remote-b"].name, "Remote B Updated");

    ProviderService::delete(&state, "claude", "remote-a").expect("删除非当前供应商");
    assert!(!ProviderService::list(&state, "claude")
        .expect("删除后列出供应商")
        .contains_key("remote-a"));
}

#[test]
fn disk_headless_state_persists_without_constructing_desktop_services() {
    let home = tempfile::tempdir().expect("创建隔离 HOME");
    {
        let state = HeadlessState::open(home.path()).expect("创建磁盘无界面状态");
        ProviderService::add(
            &state,
            "claude",
            provider("persistent", "Persistent", "sk-persist"),
            true,
        )
        .expect("写入磁盘供应商");
    }

    let reopened = HeadlessState::open(home.path()).expect("重新打开磁盘无界面状态");
    assert_eq!(
        ProviderService::current(&reopened, "claude").expect("读取持久化当前项"),
        "persistent"
    );
    assert!(ProviderService::list(&reopened, "claude")
        .expect("读取持久化供应商")
        .contains_key("persistent"));
    assert!(home
        .path()
        .join(".cc-switch")
        .join("cc-switch.db")
        .is_file());
}
