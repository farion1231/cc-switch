use cc_switch_lib::{app_config::AppType, provider::Provider, test_provider::test_provider};
use serde_json::json;
use std::collections::HashMap;

#[tokio::main]
async fn main() {

    println!("═══════════════════════════════════════════════════════════");
    println!("🧪 测试 88code API 配置");
    println!("═══════════════════════════════════════════════════════════");

    // 创建测试 Provider
    let mut settings_config = HashMap::new();
    
    let mut env_config = HashMap::new();
    env_config.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        json!("88_dfa9ac78f91f6064fed556803b7dcac7ee07af25d94b3bac0f7a3fcff7bdb5fb"),
    );
    env_config.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        json!("https://www.88code.org/api"),
    );
    env_config.insert(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
        json!("1"),
    );
    
    settings_config.insert("env".to_string(), json!(env_config));

    let provider = Provider {
        id: "88code-test".to_string(),
        name: "88code API Test".to_string(),
        settings_config: json!(settings_config),
        website_url: Some("https://www.88code.org".to_string()),
        category: None,
        created_at: None,
        meta: None,
        group_id: None,
        priority: None,
        contract_expiry: None,
        last_used_at: None,
        tags: None,
        custom_order: None,
    };

    println!("\n📋 Provider 配置:");
    println!("   ID: {}", provider.id);
    println!("   Name: {}", provider.name);
    println!("   Base URL: https://www.88code.org/api");
    println!("   API Key: 88_dfa9ac78f91f...cff7bdb5fb");

    println!("\n🔄 开始测试连接...\n");

    // 测试连接
    let result = test_provider(provider, AppType::Claude).await;

    println!("═══════════════════════════════════════════════════════════");
    println!("📊 测试结果");
    println!("═══════════════════════════════════════════════════════════");
    println!("✅ 成功: {}", result.success);
    println!("📝 消息: {}", result.message);
    
    if let Some(status) = result.status {
        println!("🔢 状态码: {}", status);
    }
    
    if let Some(latency) = result.latency_ms {
        println!("⏱️  延迟: {}ms", latency);
    }
    
    if let Some(detail) = result.detail {
        println!("📄 详细信息: {}", detail);
    }

    println!("═══════════════════════════════════════════════════════════");
    
    if result.success {
        println!("\n✅ API 配置可用!");
        println!("   该配置可以在 cc-switch 中正常使用。");
    } else {
        println!("\n⚠️  API 测试未完全通过");
        println!("   请查看上面的详细信息了解具体情况。");
        println!("   注意: 某些第三方代理可能限制测试端点访问,");
        println!("   但实际使用时可能仍然正常工作。");
    }
    
    println!("\n═══════════════════════════════════════════════════════════\n");
}
