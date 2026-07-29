use cc_switch_lib::remote::capabilities::{CommandCapabilityRegistry, RemoteCapabilityError};

#[test]
fn provider_registry_is_explicit_and_deny_by_default() {
    let registry = CommandCapabilityRegistry::provider_phase();

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
    let registry = CommandCapabilityRegistry::provider_phase();

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
