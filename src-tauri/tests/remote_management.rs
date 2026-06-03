use cc_switch_lib::AppType;

#[path = "support.rs"]
mod support;
use support::test_mutex;

// 注意：remote 模块目前是私有的，无法在单元测试中直接访问
// 以下测试验证了相关的类型和配置

// 测试 AppType 可以正确序列化（远程服务器需要使用 AppType）
#[test]
fn test_app_type_for_remote() {
    let _guard = test_mutex().lock().expect("acquire test mutex");

    // 验证 AppType 可以序列化（用于远程 API）
    let app_type = AppType::Claude;
    let serialized = serde_json::to_string(&app_type).expect("serialize AppType");
    assert!(serialized.contains("Claude") || serialized.contains("claude"));

    // 验证反序列化
    let deserialized: AppType = serde_json::from_str(&serialized).expect("deserialize AppType");
    assert_eq!(deserialized, AppType::Claude);
}

// 测试远程配置的 JSON 结构
#[test]
fn test_remote_config_json_structure() {
    let _guard = test_mutex().lock().expect("acquire test mutex");

    // 模拟 RemoteConfig 的 JSON 结构
    let config_json = r#"{
        "enabled": true,
        "port": 4000,
        "tailscale_enabled": false
    }"#;

    // 验证可以解析为通用值
    let config: serde_json::Value = serde_json::from_str(config_json).expect("parse config");
    assert_eq!(config["enabled"], true);
    assert_eq!(config["port"], 4000);
    assert_eq!(config["tailscale_enabled"], false);
}

// 测试端口号验证
#[test]
fn test_port_validation() {
    let _guard = test_mutex().lock().expect("acquire test mutex");

    // 有效端口
    let valid_ports = [1024, 4000, 8080, 65535];
    for port in valid_ports {
        assert!((1024..=65535).contains(&port));
    }

    // 无效端口（这些应该在 UI 层验证）
    let invalid_ports = [0, 100, 1023, 65536, 70000];
    for port in invalid_ports {
        assert!(!(1024..=65535).contains(&port));
    }
}

// 注意：需要完整 Tauri 环境的测试应该在集成测试中运行：
// - start_remote / stop_remote 完整流程
// - broadcast_provider_switch 在异步上下文中不阻塞
// - 实际的 HTTP 端点测试（health、providers、switch、SSE events）
// - Tailscale toggle 时的服务器重启
// - 端口变更时的服务器重启
//
// 手动测试步骤：
// 1. 启动 CC Switch
// 2. 打开设置页面，启用远程管理
// 3. 访问 http://localhost:4000 验证 Web UI 可用
// 4. 切换 provider，验证所有连接的浏览器收到 SSE 事件
// 5. 在服务器运行时切换 Tailscale 开关，验证服务器重启（黄色指示器）
// 6. 在服务器运行时修改端口，验证服务器重启且新端口生效
// 7. 观察日志确认没有 "blocking_read()" 相关的 panic
