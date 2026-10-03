//! Aggregate providers: virtual providers with no endpoint and no credentials that
//! dispatch requests to other providers based on the model being requested.

use crate::claude_desktop_config::{is_claude_safe_model_id, ResolvedModelRoute};
use crate::error::AppError;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AggregateTier {
    Sonnet,
    Opus,
    Haiku,
    Fable,
}

/// One slot = one model that can be picked in Claude Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRouteSlot {
    /// Slot ID (**persisted**; it is the stable routing key). The frontend
    /// generates it while editing and submits it with the form (the UI preview
    /// value is the value that gets saved); the backend only validates it
    /// (non-empty, claude-safe, unique) and never regenerates it at runtime —
    /// so renaming a provider cannot change routes that are already in effect.
    pub route_id: String,
    pub tier: AggregateTier,
    /// Target provider id (the referenced provider is protected from deletion)
    pub provider_id: String,
    /// Upstream model name at that provider
    pub upstream_model: String,
    /// Label shown in the picker, e.g. "Zhipu GLM-5.3"; defaults to the upstream
    /// model name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// When set, the model gets an extra "1M context window" line in the picker
    #[serde(default)]
    pub supports_1m: bool,
}

/// Fallback target used when no route matches. Referenced by slot id or
/// provider id (never by index — indices go stale as slots are added or
/// removed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum DefaultTarget {
    SlotId(String),
    ProviderId(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateRoutes {
    pub slots: Vec<AggregateRouteSlot>,
    pub default_target: DefaultTarget,
}

/// Whether this provider is an aggregate provider.
pub fn is_aggregate_provider(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.aggregate_routes.as_ref())
        .is_some()
}

/// Returns the aggregate provider's route table; profile derivation and runtime
/// route resolution share this same error.
fn routes_of(provider: &Provider) -> Result<&AggregateRoutes, AppError> {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.aggregate_routes.as_ref())
        .ok_or_else(|| {
            AppError::localized(
                "aggregate.routes_missing",
                "聚合供应商缺少路由表",
                "Aggregate provider is missing its route table",
            )
        })
}

/// Derives model specs from the slots (shared by the profile's inferenceModels
/// and the /models endpoint). Slot IDs are generated and persisted by the
/// frontend while editing and are used as-is here; sorting by route_id stays
/// consistent with the existing implementation.
pub fn aggregate_model_routes(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    let routes = routes_of(provider)?;

    let mut out = Vec::with_capacity(routes.slots.len());
    for slot in &routes.slots {
        let upstream = slot.upstream_model.trim();
        let route_id = slot.route_id.trim();
        // route_id is the stable routing key (the route table, DefaultTarget::SlotId
        // and the UI all reference it), so it is never repaired — a non
        // claude-safe ID makes Claude Desktop reject the entire inferenceModels
        // set, so drop the slot outright and keep the remaining valid slots.
        if upstream.is_empty() || route_id.is_empty() || !is_claude_safe_model_id(route_id) {
            continue;
        }
        out.push(ResolvedModelRoute {
            route_id: route_id.to_string(),
            upstream_model: upstream.to_string(),
            label_override: slot
                .label
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string),
            supports_1m: slot.supports_1m,
        });
    }
    // Keep the incoming slot order (the UI already flattens by "provider group x
    // tier strength" on submit) so that models from the same provider stay
    // together in Claude Desktop's picker, ordered internally
    // fable->opus->sonnet->haiku. **Do not** re-sort by route_id
    // lexicographically — that interleaves models from different providers (user
    // report 2026-09-24). Dedup is still by route_id (a duplicate keeps the
    // first occurrence).
    out.dedup_by(|a, b| a.route_id == b.route_id);

    if out.is_empty() {
        return Err(AppError::localized(
            "aggregate.routes_empty",
            "聚合供应商的路由表至少需要一个可用的模型槽位",
            "Aggregate provider requires at least one usable model route slot",
        ));
    }

    Ok(out)
}

/// Resolves an aggregate route: given the aggregate provider and the requested
/// model, returns (target provider, upstream model name to rewrite to).
/// - Slot matched -> return that slot's target and upstream model
/// - No match -> return the default target; a SlotId default target also
///   returns that slot's upstream model (the default slot is the default
///   model), a ProviderId default target leaves the model unrewritten (None)
/// - Default target also unusable -> return an explicit error (no silent
///   degradation)
///
/// Strips the `[1m]` marker before lookup: once a slot enables `supports1m`,
/// Claude Desktop appends that suffix to the model name before sending the
/// request. Without stripping it the whole 1M variant falls through to the
/// default target and has its model name rewritten by the target provider's own
/// route table — "picked provider A's 1M variant, actually hit provider B's
/// model".
pub fn resolve_target(
    db: &crate::database::Database,
    app_type: &str,
    aggregate: &Provider,
    request_model: &str,
) -> Result<(Provider, Option<String>), AppError> {
    let routes = routes_of(aggregate)?;

    let requested =
        crate::claude_desktop_config::strip_one_m_suffix_for_route_lookup(request_model);

    // Look up the persisted slot ID directly (the frontend generates the ID
    // while editing and submits it with the form; it is never regenerated at
    // runtime)
    for slot in &routes.slots {
        if slot.route_id == requested {
            let target = load_provider(db, app_type, &slot.provider_id)?;
            return Ok((target, Some(slot.upstream_model.clone())));
        }
    }

    // No match -> default target
    let (fallback_id, fallback_upstream) = match &routes.default_target {
        DefaultTarget::ProviderId(id) => (id.clone(), None),
        DefaultTarget::SlotId(slot_id) => {
            // The slot's upstream model rides along: the default slot is the
            // default model, and without it the forwarder would re-resolve the
            // original request model against the target's own route table —
            // defeating the "default slot = default model" purpose.
            let slot = routes
                .slots
                .iter()
                .find(|slot| &slot.route_id == slot_id)
                .ok_or_else(|| {
                    AppError::localized(
                        "aggregate.default_target_slot_missing",
                        "聚合供应商的默认目标指向了不存在的槽位",
                        "Aggregate default target points to a missing slot",
                    )
                })?;
            (slot.provider_id.clone(), Some(slot.upstream_model.clone()))
        }
    };
    let target = load_provider(db, app_type, &fallback_id)?;
    Ok((target, fallback_upstream))
}

fn load_provider(
    db: &crate::database::Database,
    app_type: &str,
    provider_id: &str,
) -> Result<Provider, AppError> {
    db.get_provider_by_id(provider_id, app_type)?
        .ok_or_else(|| {
            AppError::localized(
                "aggregate.target_provider_missing",
                "聚合供应商的目标供应商不存在",
                "Aggregate target provider does not exist",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(route_id: &str, upstream_model: &str) -> AggregateRouteSlot {
        AggregateRouteSlot {
            route_id: route_id.to_string(),
            tier: AggregateTier::Sonnet,
            provider_id: "p-target".to_string(),
            upstream_model: upstream_model.to_string(),
            label: None,
            supports_1m: false,
        }
    }

    fn aggregate_provider(slots: Vec<AggregateRouteSlot>) -> Provider {
        let mut provider = Provider::with_id(
            "agg".to_string(),
            "Aggregate".to_string(),
            serde_json::json!({ "env": {} }),
            Some("https://example.com".to_string()),
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            aggregate_routes: Some(AggregateRoutes {
                slots,
                default_target: DefaultTarget::ProviderId("p-target".to_string()),
            }),
            ..Default::default()
        });
        provider
    }

    #[test]
    fn aggregate_model_routes_filters_unsafe_route_ids() {
        // An unsafe route_id must be dropped (rather than repaired or emitted
        // verbatim) — one bad ID makes Claude Desktop reject the entire
        // inferenceModels set. The rest of the slots must still be produced.
        let provider = aggregate_provider(vec![
            slot("claude-sonnet-1", "glm-5.3"),
            slot("glm-5.3", "some-upstream-model"),
        ]);

        let routes = aggregate_model_routes(&provider).expect("routes");

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_id, "claude-sonnet-1");
        assert_eq!(routes[0].upstream_model, "glm-5.3");
    }

    #[test]
    fn aggregate_model_routes_errors_when_all_slots_are_unusable() {
        // Return Err when every slot is unusable (rather than silently writing
        // an empty list -> a blank model picker)
        let provider = aggregate_provider(vec![slot("glm-5.3", "some-upstream-model")]);

        assert!(aggregate_model_routes(&provider).is_err());
    }

    #[test]
    fn aggregate_model_routes_dedups_repeated_route_ids() {
        // A duplicate route_id from hand-editing must yield only one entry
        let provider = aggregate_provider(vec![
            slot("claude-sonnet-1", "glm-a"),
            slot("claude-sonnet-1", "glm-b"),
        ]);

        let routes = aggregate_model_routes(&provider).expect("routes");

        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].route_id, "claude-sonnet-1");
    }

    #[test]
    fn aggregate_model_routes_preserves_provider_grouped_order() {
        // The order of inferenceModels in the profile is the order of Claude
        // Desktop's model picker. The UI already flattens slots by
        // "provider group x fable->opus->sonnet->haiku" on submit, so the backend
        // must preserve it verbatim — re-sorting by route_id lexicographically
        // interleaves models from different providers (user report 2026-09-24:
        // Zhipu/DeepSeek/Ark/OpenCode models ended up mixed together).
        let grouped = |provider_id: &str, tier: AggregateTier, route_id: &str, model: &str| {
            slot_for(route_id, provider_id, tier, model)
        };
        let provider = aggregate_provider(vec![
            grouped("p-zhipu", AggregateTier::Fable, "claude-fable-1", "glm-5.3"),
            grouped(
                "p-zhipu",
                AggregateTier::Opus,
                "claude-opus-4-8",
                "glm-5.3-flash",
            ),
            grouped(
                "p-ds",
                AggregateTier::Fable,
                "claude-fable-2",
                "deepseek-v4-pro",
            ),
            grouped(
                "p-ds",
                AggregateTier::Opus,
                "claude-opus-4-7",
                "deepseek-flash",
            ),
            grouped("p-ark", AggregateTier::Fable, "claude-fable-3", "kimi-k3"),
            grouped(
                "p-oc",
                AggregateTier::Sonnet,
                "claude-sonnet-4-5",
                "space-bunny-free",
            ),
        ]);

        let routes = aggregate_model_routes(&provider).expect("routes");

        let order: Vec<&str> = routes.iter().map(|r| r.route_id.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "claude-fable-1",    // Zhipu
                "claude-opus-4-8",   // Zhipu
                "claude-fable-2",    // DeepSeek
                "claude-opus-4-7",   // DeepSeek
                "claude-fable-3",    // Ark
                "claude-sonnet-4-5", // OpenCode
            ],
            "必须保持供应商分组顺序，不能按 route_id 字典序重排"
        );
        // Lexicographic order puts sonnet before opus — exactly the interleaving
        // we are avoiding
        let mut lexical = order.clone();
        lexical.sort_unstable();
        assert_ne!(order, lexical, "本用例应能区分分组序与字典序");
    }

    #[tokio::test]
    async fn resolve_target_hits_slot_by_generated_id() {
        let db = crate::database::Database::memory().expect("db");
        // Target provider (built with Provider::with_id, the convention already
        // used in this repo; Provider does not derive Default)
        let target = crate::provider::Provider::with_id(
            "p-glm".to_string(),
            "GLM".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude-desktop", &target)
            .expect("save target");

        let mut aggregate = crate::provider::Provider::with_id(
            "agg".to_string(),
            "Aggregate".to_string(),
            serde_json::json!({}),
            None,
        );
        aggregate.meta = Some(crate::provider::ProviderMeta {
            aggregate_routes: Some(AggregateRoutes {
                slots: vec![AggregateRouteSlot {
                    route_id: "claude-sonnet-1".into(),
                    tier: AggregateTier::Sonnet,
                    provider_id: "p-glm".into(),
                    upstream_model: "glm-5.3".into(),
                    label: None,
                    supports_1m: false,
                }],
                default_target: DefaultTarget::ProviderId("p-glm".into()),
            }),
            ..Default::default()
        });

        let hit =
            resolve_target(&db, "claude-desktop", &aggregate, "claude-sonnet-1").expect("hit");
        assert_eq!(hit.0.id, "p-glm");
        assert_eq!(hit.1.as_deref(), Some("glm-5.3"));

        // No match -> default target, with the model name left unrewritten
        let miss = resolve_target(&db, "claude-desktop", &aggregate, "claude-haiku-4-5")
            .expect("miss falls back to default");
        assert_eq!(miss.0.id, "p-glm");
        assert_eq!(miss.1, None);
    }

    /// Returns the stable key of an `AppError::Localized` (asserts on the error
    /// kind, not on the localized text).
    fn localized_key(err: &AppError) -> &'static str {
        match err {
            AppError::Localized { key, .. } => key,
            other => panic!("expected a localized error, got: {other}"),
        }
    }

    /// Slot constructor: points at `provider_id` with the given `tier`.
    fn slot_for(
        route_id: &str,
        provider_id: &str,
        tier: AggregateTier,
        upstream_model: &str,
    ) -> AggregateRouteSlot {
        AggregateRouteSlot {
            route_id: route_id.to_string(),
            tier,
            provider_id: provider_id.to_string(),
            upstream_model: upstream_model.to_string(),
            label: None,
            supports_1m: false,
        }
    }

    fn aggregate_with(slots: Vec<AggregateRouteSlot>, default_target: DefaultTarget) -> Provider {
        let mut aggregate = crate::provider::Provider::with_id(
            "agg".to_string(),
            "Aggregate".to_string(),
            serde_json::json!({}),
            None,
        );
        aggregate.meta = Some(crate::provider::ProviderMeta {
            aggregate_routes: Some(AggregateRoutes {
                slots,
                default_target,
            }),
            ..Default::default()
        });
        aggregate
    }

    #[tokio::test]
    async fn resolve_target_errors_when_default_target_slot_missing() {
        let db = crate::database::Database::memory().expect("db");
        // The default target points at a slot id that does not exist — this must
        // raise an explicit error and **must not** silently fall back to "the
        // first slot" (that would quietly rewrite the user's fallback config).
        let aggregate = aggregate_with(
            vec![slot_for(
                "claude-sonnet-1",
                "p-glm",
                AggregateTier::Sonnet,
                "glm-5.3",
            )],
            DefaultTarget::SlotId("claude-sonnet-missing".into()),
        );

        let err = resolve_target(&db, "claude-desktop", &aggregate, "claude-haiku-4-5")
            .expect_err("default target pointing at a missing slot must error");
        assert_eq!(localized_key(&err), "aggregate.default_target_slot_missing");
    }

    #[tokio::test]
    async fn resolve_target_errors_when_target_provider_missing() {
        let db = crate::database::Database::memory().expect("db");
        // A slot matched, but the target provider it references is absent from
        // the store -> explicit error
        let aggregate = aggregate_with(
            vec![slot_for(
                "claude-sonnet-1",
                "p-missing",
                AggregateTier::Sonnet,
                "glm-5.3",
            )],
            DefaultTarget::ProviderId("p-missing".into()),
        );

        let err = resolve_target(&db, "claude-desktop", &aggregate, "claude-sonnet-1")
            .expect_err("missing target provider must error");
        assert_eq!(localized_key(&err), "aggregate.target_provider_missing");
    }

    /// The 1M variant: once a slot enables `supports1m`, Claude Desktop creates a
    /// `<slot ID>[1m]` model entry, and requests carry that suffix in `model`
    /// when the user picks it. Route lookup must strip the marker first, or the
    /// whole batch of 1M requests falls through to the default target and has
    /// its model name rewritten by the **target provider's own route table** —
    /// surfacing as "picked provider A's 1M variant, actually hit provider B's
    /// model" (observed locally on 2026-09-23: `claude-fable-1[1m]` went to
    /// DeepSeek's `deepseek-v4-pro`, even though that slot is Zhipu's `glm-5.3`;
    /// with the default target set to a different provider, the fallthrough is
    /// caught by the assertion).
    #[tokio::test]
    async fn resolve_target_strips_one_m_marker_before_slot_lookup() {
        let db = crate::database::Database::memory().expect("db");
        let target = crate::provider::Provider::with_id(
            "p-glm".to_string(),
            "GLM".to_string(),
            serde_json::json!({}),
            None,
        );
        let fallback = crate::provider::Provider::with_id(
            "p-other".to_string(),
            "Other".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude-desktop", &target)
            .expect("save target");
        db.save_provider("claude-desktop", &fallback)
            .expect("save fallback");

        let aggregate = aggregate_with(
            vec![slot_for(
                "claude-fable-1",
                "p-glm",
                AggregateTier::Sonnet,
                "glm-5.3",
            )],
            DefaultTarget::ProviderId("p-other".into()),
        );

        for requested in [
            "claude-fable-1[1m]",
            "claude-fable-1[1M] ",
            "claude-fable-1 [1m]",
        ] {
            let hit = resolve_target(&db, "claude-desktop", &aggregate, requested)
                .unwrap_or_else(|e| panic!("{requested} 应命中槽位: {e:?}"));
            assert_eq!(hit.0.id, "p-glm", "{requested} 不应漏到默认目标");
            assert_eq!(hit.1.as_deref(), Some("glm-5.3"), "{requested}");
        }
    }

    #[tokio::test]
    async fn resolve_target_slot_id_default_target_returns_slot_upstream_model() {
        // "Default slot = default model": an unmatched request must be
        // rewritten to the default slot's upstream model, not re-resolved
        // against the target's own route table.
        let db = crate::database::Database::memory().expect("db");
        let fallback = crate::provider::Provider::with_id(
            "p-glm".to_string(),
            "GLM".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude-desktop", &fallback)
            .expect("save fallback");

        let aggregate = aggregate_with(
            vec![slot_for(
                "claude-sonnet-1",
                "p-glm",
                AggregateTier::Sonnet,
                "glm-5.3",
            )],
            DefaultTarget::SlotId("claude-sonnet-1".into()),
        );

        let miss = resolve_target(&db, "claude-desktop", &aggregate, "claude-haiku-4-5")
            .expect("miss falls back to the default slot");
        assert_eq!(miss.0.id, "p-glm");
        assert_eq!(miss.1.as_deref(), Some("glm-5.3"));
    }

    #[tokio::test]
    async fn resolve_target_provider_id_default_target_leaves_model_unrewritten() {
        // A ProviderId default target names only the provider, so the request
        // model stays unrewritten (the target's own route table applies).
        let db = crate::database::Database::memory().expect("db");
        let fallback = crate::provider::Provider::with_id(
            "p-glm".to_string(),
            "GLM".to_string(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("claude-desktop", &fallback)
            .expect("save fallback");

        let aggregate = aggregate_with(
            vec![slot_for(
                "claude-sonnet-1",
                "p-glm",
                AggregateTier::Sonnet,
                "glm-5.3",
            )],
            DefaultTarget::ProviderId("p-glm".into()),
        );

        let miss = resolve_target(&db, "claude-desktop", &aggregate, "claude-haiku-4-5")
            .expect("miss falls back to the default provider");
        assert_eq!(miss.0.id, "p-glm");
        assert_eq!(miss.1, None);
    }
}
