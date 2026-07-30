use cc_switch_lib::remote::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};

const USAGE_READS: &[&str] = &[
    "usage.summary",
    "usage.summary_by_app",
    "usage.trends",
    "usage.provider_stats",
    "usage.model_stats",
    "usage.logs",
    "usage.detail",
    "usage.data_sources",
    "usage.pricing.list",
    "usage.limits",
    "usage.provider_query",
];

const USAGE_WRITES: &[&str] = &[
    "usage.pricing.update",
    "usage.pricing.delete",
    "usage.provider_test",
    "usage.session_sync",
    "usage.codex_rebuild",
];

#[test]
fn provider_registry_is_explicit_and_deny_by_default() {
    let registry = CommandCapabilityRegistry::remote_supported();

    assert!(
        registry
            .require("provider.list")
            .expect("provider.list")
            .read_only
    );
    assert!(
        !registry
            .require("provider.add")
            .expect("provider.add")
            .read_only
    );
    assert!(
        registry
            .require("provider.switch")
            .expect("provider.switch")
            .timeout_ms
            >= 30_000
    );
    assert!(matches!(
        registry.require("unknown.command"),
        Err(RemoteCapabilityError::CommandNotExposed(command))
            if command == "unknown.command"
    ));
}

#[test]
fn provider_registry_marks_only_reads_as_idempotent() {
    let registry = CommandCapabilityRegistry::remote_supported();

    for command in ["provider.list", "provider.current"] {
        let capability = registry.require(command).expect("read capability");
        assert!(capability.read_only);
        assert!(capability.idempotent);
    }

    for command in [
        "provider.add",
        "provider.update",
        "provider.delete",
        "provider.switch",
        "provider.update_sort_order",
    ] {
        let capability = registry.require(command).expect("write capability");
        assert!(!capability.read_only);
        assert!(!capability.idempotent);
    }
}

#[test]
fn remote_registry_declares_complete_usage_metadata() {
    let registry = CommandCapabilityRegistry::remote_supported();

    for command in USAGE_READS {
        let capability = registry.require(command).expect("Usage 读取能力");
        assert!(capability.read_only, "{command} 必须标记为只读");
        assert!(capability.idempotent, "{command} 必须可安全重试");
        assert_eq!(capability.timeout_ms, 30_000);
    }

    for command in USAGE_WRITES {
        let capability = registry.require(command).expect("Usage 写能力");
        assert!(!capability.read_only, "{command} 不能标记为只读");
        assert!(!capability.idempotent, "{command} 不能自动重试");
        let expected_timeout = if matches!(*command, "usage.session_sync" | "usage.codex_rebuild") {
            300_000
        } else {
            30_000
        };
        assert_eq!(capability.timeout_ms, expected_timeout);
    }

    assert!(matches!(
        registry.require("usage.not_exposed"),
        Err(RemoteCapabilityError::CommandNotExposed(command)) if command == "usage.not_exposed"
    ));
}
