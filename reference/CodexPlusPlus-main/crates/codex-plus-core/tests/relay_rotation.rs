use codex_plus_core::relay_rotation::{
    RelayRotationSelector, RotationContext, RotationEvent, SelectionError, fallback_relays_after,
    record_relay_request_failure, select_relay_for_probe, select_relay_for_request,
};
use codex_plus_core::settings::{
    AggregateRelayMember, AggregateRelayProfile, AggregateRelayStrategy, BackendSettings,
    RelayMode, RelayProfile,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

fn global_selector_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn profile(id: &str) -> RelayProfile {
    RelayProfile {
        id: id.to_string(),
        name: id.to_string(),
        base_url: format!("https://{id}.example/v1"),
        api_key: format!("sk-{id}"),
        ..RelayProfile::default()
    }
}

fn aggregate(strategy: AggregateRelayStrategy) -> AggregateRelayProfile {
    AggregateRelayProfile {
        id: "agg".to_string(),
        name: "聚合".to_string(),
        strategy,
        members: vec![
            AggregateRelayMember {
                relay_id: "relay-a".to_string(),
                weight: 1,
            },
            AggregateRelayMember {
                relay_id: "relay-b".to_string(),
                weight: 2,
            },
            AggregateRelayMember {
                relay_id: "relay-c".to_string(),
                weight: 1,
            },
        ],
    }
}

fn aggregate_with_id(id: &str, strategy: AggregateRelayStrategy) -> AggregateRelayProfile {
    AggregateRelayProfile {
        id: id.to_string(),
        name: "聚合".to_string(),
        strategy,
        members: vec![
            AggregateRelayMember {
                relay_id: "relay-a".to_string(),
                weight: 1,
            },
            AggregateRelayMember {
                relay_id: "relay-b".to_string(),
                weight: 2,
            },
        ],
    }
}

fn settings(strategy: AggregateRelayStrategy) -> BackendSettings {
    BackendSettings {
        relay_profiles: vec![
            profile("relay-a"),
            profile("relay-b"),
            profile("relay-c"),
            RelayProfile {
                id: "agg".to_string(),
                name: "聚合".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
        ],
        aggregate_relay_profiles: vec![aggregate(strategy)],
        active_relay_id: "agg".to_string(),
        active_aggregate_relay_id: "agg".to_string(),
        ..BackendSettings::default()
    }
}

#[test]
fn failover_keeps_current_provider_until_failure_then_moves_to_next_member() {
    let settings = settings(AggregateRelayStrategy::Failover);
    let mut selector = RelayRotationSelector::from_settings(&settings).unwrap();

    let first = selector
        .select(&settings, RotationContext::for_conversation("chat-1"))
        .unwrap();
    selector.record_event(RotationEvent::Success);
    let second = selector
        .select(&settings, RotationContext::for_conversation("chat-1"))
        .unwrap();
    selector.record_event(RotationEvent::Failure);
    let third = selector
        .select(&settings, RotationContext::for_conversation("chat-1"))
        .unwrap();

    assert_eq!(first.id, "relay-a");
    assert_eq!(second.id, "relay-a");
    assert_eq!(third.id, "relay-b");
}

#[test]
fn conversation_rotation_sticks_each_conversation_to_a_stable_member() {
    let settings = settings(AggregateRelayStrategy::ConversationRoundRobin);
    let mut selector = RelayRotationSelector::from_settings(&settings).unwrap();

    let chat_a_first = selector
        .select(&settings, RotationContext::for_conversation("chat-a"))
        .unwrap();
    let chat_a_second = selector
        .select(&settings, RotationContext::for_conversation("chat-a"))
        .unwrap();
    let chat_b_first = selector
        .select(&settings, RotationContext::for_conversation("chat-b"))
        .unwrap();

    assert_eq!(chat_a_first.id, "relay-a");
    assert_eq!(chat_a_second.id, "relay-a");
    assert_eq!(chat_b_first.id, "relay-b");
}

#[test]
fn request_rotation_advances_on_every_request() {
    let settings = settings(AggregateRelayStrategy::RequestRoundRobin);
    let mut selector = RelayRotationSelector::from_settings(&settings).unwrap();

    let selected = (0..5)
        .map(|_| {
            selector
                .select(&settings, RotationContext::default())
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();

    assert_eq!(
        selected,
        vec!["relay-a", "relay-b", "relay-c", "relay-a", "relay-b"]
    );
}

#[test]
fn weighted_rotation_repeats_members_by_configured_weight() {
    let settings = settings(AggregateRelayStrategy::WeightedRoundRobin);
    let mut selector = RelayRotationSelector::from_settings(&settings).unwrap();

    let selected = (0..6)
        .map(|_| {
            selector
                .select(&settings, RotationContext::default())
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();

    assert_eq!(
        selected,
        vec![
            "relay-a", "relay-b", "relay-b", "relay-c", "relay-a", "relay-b"
        ]
    );
}

#[test]
fn aggregate_members_must_reference_existing_relay_profiles() {
    let mut settings = settings(AggregateRelayStrategy::RequestRoundRobin);
    settings.aggregate_relay_profiles[0]
        .members
        .push(AggregateRelayMember {
            relay_id: "missing-relay".to_string(),
            weight: 1,
        });

    let error = RelayRotationSelector::from_settings(&settings).unwrap_err();

    assert_eq!(
        error,
        SelectionError::UnknownMemberRelay {
            aggregate_id: "agg".to_string(),
            relay_id: "missing-relay".to_string()
        }
    );
}

#[test]
fn aggregate_with_one_member_is_allowed_without_rotation() {
    let mut settings = settings(AggregateRelayStrategy::RequestRoundRobin);
    settings.aggregate_relay_profiles[0].members.truncate(1);

    let mut selector = RelayRotationSelector::from_settings(&settings).unwrap();
    let first = selector
        .select(&settings, RotationContext::default())
        .unwrap();
    let second = selector
        .select(&settings, RotationContext::default())
        .unwrap();

    assert_eq!(first.id, "relay-a");
    assert_eq!(second.id, "relay-a");
}

#[test]
fn aggregate_members_must_be_api_capable_relay_profiles() {
    let mut settings = settings(AggregateRelayStrategy::WeightedRoundRobin);
    settings.relay_profiles.push(RelayProfile {
        id: "official-login".to_string(),
        name: "官方登录".to_string(),
        base_url: String::new(),
        api_key: String::new(),
        ..RelayProfile::default()
    });
    settings.aggregate_relay_profiles[0]
        .members
        .push(AggregateRelayMember {
            relay_id: "official-login".to_string(),
            weight: 1,
        });

    let error = RelayRotationSelector::from_settings(&settings).unwrap_err();

    assert_eq!(
        error,
        SelectionError::InvalidMemberRelay {
            aggregate_id: "agg".to_string(),
            relay_id: "official-login".to_string()
        }
    );
}

#[test]
fn select_relay_for_request_uses_active_relay_id_as_aggregate_source_of_truth() {
    let _guard = global_selector_test_lock();
    let mut settings = settings(AggregateRelayStrategy::WeightedRoundRobin);
    settings.active_relay_id = "agg".to_string();
    settings.active_aggregate_relay_id.clear();

    let selected = select_relay_for_request(&settings, RotationContext::default()).unwrap();

    assert_eq!(selected.id, "relay-a");
}

#[test]
fn select_relay_for_request_ignores_stale_active_aggregate_id_for_regular_relay() {
    let _guard = global_selector_test_lock();
    let mut settings = settings(AggregateRelayStrategy::WeightedRoundRobin);
    settings.active_relay_id = "relay-b".to_string();
    settings.active_aggregate_relay_id = "agg".to_string();

    let selected = select_relay_for_request(&settings, RotationContext::default()).unwrap();

    assert_eq!(selected.id, "relay-b");
}

#[test]
fn select_relay_for_request_resets_rotation_after_switching_to_regular_relay() {
    let _guard = global_selector_test_lock();
    let mut settings = settings(AggregateRelayStrategy::RequestRoundRobin);
    settings.active_relay_id = "agg".to_string();

    let first = select_relay_for_request(&settings, RotationContext::default()).unwrap();
    let mut regular_settings = settings.clone();
    regular_settings.active_relay_id = "relay-c".to_string();
    regular_settings.active_aggregate_relay_id.clear();
    let regular = select_relay_for_request(&regular_settings, RotationContext::default()).unwrap();
    let after_reselect = select_relay_for_request(&settings, RotationContext::default()).unwrap();

    assert_eq!(first.id, "relay-a");
    assert_eq!(regular.id, "relay-c");
    assert_eq!(after_reselect.id, "relay-a");
}

#[test]
fn record_relay_request_failure_advances_global_failover_selector() {
    let _guard = global_selector_test_lock();
    let aggregate_id = "agg-global-failure";
    let settings = BackendSettings {
        relay_profiles: vec![
            profile("relay-a"),
            profile("relay-b"),
            RelayProfile {
                id: aggregate_id.to_string(),
                name: "聚合".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
        ],
        aggregate_relay_profiles: vec![aggregate_with_id(
            aggregate_id,
            AggregateRelayStrategy::Failover,
        )],
        active_relay_id: aggregate_id.to_string(),
        active_aggregate_relay_id: aggregate_id.to_string(),
        ..BackendSettings::default()
    };

    let first = select_relay_for_request(&settings, RotationContext::default()).unwrap();
    record_relay_request_failure(&settings);
    let second = select_relay_for_request(&settings, RotationContext::default()).unwrap();

    assert_eq!(first.id, "relay-a");
    assert_eq!(second.id, "relay-b");
}

#[test]
fn select_relay_for_probe_does_not_advance_request_rotation() {
    let _guard = global_selector_test_lock();
    let aggregate_id = "agg-probe";
    let settings = BackendSettings {
        relay_profiles: vec![
            profile("relay-a"),
            profile("relay-b"),
            RelayProfile {
                id: aggregate_id.to_string(),
                name: "聚合".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
        ],
        aggregate_relay_profiles: vec![aggregate_with_id(
            aggregate_id,
            AggregateRelayStrategy::RequestRoundRobin,
        )],
        active_relay_id: aggregate_id.to_string(),
        active_aggregate_relay_id: aggregate_id.to_string(),
        ..BackendSettings::default()
    };

    let first_probe = select_relay_for_probe(&settings).unwrap();
    let second_probe = select_relay_for_probe(&settings).unwrap();
    let first_request = select_relay_for_request(&settings, RotationContext::default()).unwrap();
    let second_request = select_relay_for_request(&settings, RotationContext::default()).unwrap();

    assert_eq!(first_probe.id, "relay-a");
    assert_eq!(second_probe.id, "relay-a");
    assert_eq!(first_request.id, "relay-a");
    assert_eq!(second_request.id, "relay-b");
}

#[test]
fn fallback_relays_after_returns_remaining_aggregate_members_after_current_then_wraps() {
    let settings = settings(AggregateRelayStrategy::RequestRoundRobin);

    let fallbacks = fallback_relays_after(&settings, "relay-b").unwrap();

    assert_eq!(
        fallbacks
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>(),
        vec!["relay-c", "relay-a"]
    );
}

#[test]
fn fallback_relays_after_regular_relay_returns_empty_candidates() {
    let mut settings = settings(AggregateRelayStrategy::RequestRoundRobin);
    settings.active_relay_id = "relay-a".to_string();

    let fallbacks = fallback_relays_after(&settings, "relay-a").unwrap();

    assert!(fallbacks.is_empty());
}

#[test]
fn select_relay_for_request_rebuilds_selector_when_active_aggregate_changes() {
    let _guard = global_selector_test_lock();
    let aggregate_id = "agg-refresh";
    let mut settings = BackendSettings {
        relay_profiles: vec![
            profile("relay-a"),
            profile("relay-b"),
            RelayProfile {
                id: aggregate_id.to_string(),
                name: "聚合".to_string(),
                relay_mode: RelayMode::Aggregate,
                ..RelayProfile::default()
            },
        ],
        aggregate_relay_profiles: vec![aggregate_with_id(
            aggregate_id,
            AggregateRelayStrategy::Failover,
        )],
        active_relay_id: aggregate_id.to_string(),
        active_aggregate_relay_id: aggregate_id.to_string(),
        ..BackendSettings::default()
    };

    let first = select_relay_for_request(&settings, RotationContext::default()).unwrap();
    settings.aggregate_relay_profiles[0].strategy = AggregateRelayStrategy::WeightedRoundRobin;

    let selected = (0..3)
        .map(|_| {
            select_relay_for_request(&settings, RotationContext::default())
                .unwrap()
                .id
        })
        .collect::<Vec<_>>();

    assert_eq!(first.id, "relay-a");
    assert_eq!(selected, vec!["relay-a", "relay-b", "relay-b"]);
}
