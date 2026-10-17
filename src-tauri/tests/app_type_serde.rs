use cc_switch_lib::AppType;

/// AppType 的 serde 表示是远程 API 与磁盘配置的契约：
/// 序列化后的字符串必须能原样反序列化回同一个变体。
///
/// 遍历用 `AppType::all()` 而不是手写列表，这样新增变体不会漏测。
#[test]
fn test_app_type_serde_roundtrip() {
    for app in AppType::all() {
        let serialized = serde_json::to_string(&app).expect("serialize AppType");
        let deserialized: AppType = serde_json::from_str(&serialized).expect("deserialize AppType");
        assert_eq!(
            deserialized, app,
            "roundtrip mismatch for {:?} via {}",
            app, serialized
        );
    }
}

/// serde 序列化出的必须是 on-disk / API 使用的 kebab-case 名称，
/// 而不是 Rust 变体名。
#[test]
fn test_app_type_serde_uses_wire_names() {
    let expected = [
        (AppType::Claude, "\"claude\""),
        (AppType::ClaudeDesktop, "\"claude-desktop\""),
        (AppType::Codex, "\"codex\""),
        (AppType::Gemini, "\"gemini\""),
        (AppType::GrokBuild, "\"grokbuild\""),
        (AppType::OpenCode, "\"opencode\""),
        (AppType::OpenClaw, "\"openclaw\""),
        (AppType::Hermes, "\"hermes\""),
        (AppType::Pi, "\"pi\""),
        (AppType::Mcode, "\"mcode\""),
    ];

    // 变体数量对不上说明上面漏了新变体，先让这条失败再加。
    assert_eq!(
        expected.len(),
        AppType::all().count(),
        "wire-name table is out of sync with AppType::all()"
    );

    for (app, wire) in expected {
        assert_eq!(
            serde_json::to_string(&app).expect("serialize AppType"),
            wire
        );
        let parsed: AppType = serde_json::from_str(wire).expect("deserialize AppType");
        assert_eq!(parsed, app);
    }
}

/// `claude_desktop` / `claudeDesktop` 是历史别名，仍需被接受。
#[test]
fn test_app_type_serde_accepts_claude_desktop_aliases() {
    for wire in ["\"claude_desktop\"", "\"claudeDesktop\""] {
        let parsed: AppType = serde_json::from_str(wire).expect("deserialize AppType");
        assert_eq!(parsed, AppType::ClaudeDesktop);
    }
}
