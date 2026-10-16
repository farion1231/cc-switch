//! Pure, account-scoped rules for Codex quota activation.
//!
//! This module intentionally has no OAuth or provider side effects.  A fresh
//! read-only `/wham/usage` response is converted into a private snapshot,
//! evaluated here, and only an explicit automatic caller may claim an event.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

pub(crate) const FIVE_HOUR_SECONDS: i64 = 18_000;
pub(crate) const WEEKLY_SECONDS: i64 = 604_800;

fn account_locks() -> &'static Mutex<HashMap<String, Arc<AsyncMutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn account_lock(account_id: &str) -> Arc<AsyncMutex<()>> {
    account_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(account_id.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub(crate) fn try_lock_account(account_id: &str) -> Option<OwnedMutexGuard<()>> {
    account_lock(account_id).try_lock_owned().ok()
}

pub(crate) async fn lock_account(account_id: &str) -> OwnedMutexGuard<()> {
    account_lock(account_id).lock_owned().await
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaWindowSnapshot {
    pub limit_id: String,
    pub is_additional: bool,
    pub used_percent: Option<f64>,
    pub limit_window_seconds: Option<i64>,
    pub reset_at: Option<i64>,
    pub reset_after_seconds: Option<i64>,
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub rate_limit_reached_type: Option<String>,
    /// Explicit permission fields were malformed or disagreed with one
    /// another.  This is distinct from a missing optional field so a later
    /// window value cannot accidentally re-enable a conflicted snapshot.
    pub permission_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CodexQuotaSnapshot {
    pub allowed: Option<bool>,
    pub limit_reached: Option<bool>,
    pub rate_limit_reached_type: Option<String>,
    pub windows: Vec<QuotaWindowSnapshot>,
    pub request_started_at_ms: i64,
    pub observed_at_ms: i64,
    #[serde(skip)]
    pub request_started_monotonic: Option<Instant>,
    #[serde(skip)]
    pub observed_monotonic: Option<Instant>,
    pub server_date_at_ms: Option<i64>,
    pub cache_age_seconds: Option<u64>,
    pub permission_conflict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowKind {
    FiveHour,
    Weekly,
}

impl WindowKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::Weekly => "weekly",
        }
    }

    const fn seconds(self) -> i64 {
        match self {
            Self::FiveHour => FIVE_HOUR_SECONDS,
            Self::Weekly => WEEKLY_SECONDS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivationDecision {
    Candidate(WindowKind),
    Observe,
    Blocked(&'static str),
    Unsupported(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivationState {
    BootstrapPending,
    TrackingUsage {
        target: WindowKind,
        boundary_reset_at: i64,
    },
    Eligible {
        target: WindowKind,
        event_key: String,
    },
    Spent {
        target: WindowKind,
        event_key: String,
        attempt_id: String,
        attempted_at_ms: i64,
    },
    AwaitingBaseline,
}

impl Default for ActivationState {
    fn default() -> Self {
        Self::BootstrapPending
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedAttempt {
    pub attempt_id: String,
    pub target: WindowKind,
}

/// Parse a raw `wham/usage` JSON value without changing the numeric precision.
/// Unknown additional buckets are retained and therefore can be rejected by
/// selection when they collide with a supported duration.
pub(crate) fn snapshot_from_value(
    value: &Value,
    request_started_at_ms: i64,
    observed_at_ms: i64,
) -> Option<CodexQuotaSnapshot> {
    let rate_limit = value
        .get("rate_limit")
        .or_else(|| value.get("rateLimit"))?
        .as_object()?;
    let allowed = merge_bool_fields(value, rate_limit, &["allowed"]);
    let limit_reached = merge_bool_fields(value, rate_limit, &["limit_reached", "limitReached"]);
    let permission_conflict = bool_fields_conflict(value, rate_limit, &["allowed"])
        || bool_fields_conflict(value, rate_limit, &["limit_reached", "limitReached"]);
    let rate_limit_reached_type = merge_reached_type_fields(
        value,
        rate_limit,
        &["rate_limit_reached_type", "rateLimitReachedType"],
    );

    let mut windows = Vec::new();
    for (key, additional) in [("primary_window", false), ("secondary_window", false)] {
        if let Some(raw) = rate_limit
            .get(key)
            .or_else(|| {
                rate_limit.get(match key {
                    "primary_window" => "primaryWindow",
                    _ => "secondaryWindow",
                })
            })
            .filter(|raw| !raw.is_null())
        {
            windows.push(parse_window(key, raw, additional));
        }
    }
    if let Some(additional) = rate_limit
        .get("additional_rate_limits")
        .or_else(|| rate_limit.get("additionalRateLimits"))
    {
        collect_additional_windows(additional, &mut windows);
    }
    if let Some(additional) = value
        .get("additional_rate_limits")
        .or_else(|| value.get("additionalRateLimits"))
    {
        collect_additional_windows(additional, &mut windows);
    }
    if windows.is_empty() {
        return None;
    }
    Some(CodexQuotaSnapshot {
        allowed,
        limit_reached,
        rate_limit_reached_type,
        windows,
        request_started_at_ms,
        observed_at_ms,
        request_started_monotonic: None,
        observed_monotonic: None,
        server_date_at_ms: None,
        cache_age_seconds: None,
        permission_conflict,
    })
}

fn parse_reached_type(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Object(map) => map
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| Some("unknown".to_string())),
        _ => Some("unknown".to_string()),
    }
}

fn parse_bool_field(value: &Value, keys: &[&str]) -> Result<Option<bool>, ()> {
    let mut found = None;
    for raw in keys.iter().filter_map(|key| value.get(*key)) {
        if raw.is_null() {
            continue;
        }
        let parsed = raw.as_bool().ok_or(())?;
        if found.is_some_and(|previous| previous != parsed) {
            return Err(());
        }
        found = Some(parsed);
    }
    Ok(found)
}

fn merge_bool_fields(
    outer: &Value,
    nested: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<bool> {
    let outer = parse_bool_field(outer, keys);
    let nested = parse_bool_field(&Value::Object(nested.clone()), keys);
    match (outer, nested) {
        (Err(()), _) | (_, Err(())) => None,
        (Ok(outer), Ok(nested)) => match (outer, nested) {
            (Some(a), Some(b)) if a != b => None,
            (Some(value), _) | (_, Some(value)) => Some(value),
            _ => None,
        },
    }
}

fn bool_fields_conflict(
    outer: &Value,
    nested: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> bool {
    let outer = parse_bool_field(outer, keys);
    let nested = parse_bool_field(&Value::Object(nested.clone()), keys);
    match (outer, nested) {
        (Err(()), _) | (_, Err(())) => true,
        (Ok(Some(a)), Ok(Some(b))) => a != b,
        _ => false,
    }
}

fn parse_reached_type_field(
    value: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(parse_reached_type)
}

fn merge_reached_type_fields(
    outer: &Value,
    nested: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    let outer = outer
        .as_object()
        .and_then(|map| parse_reached_type_field(map, keys));
    let nested = parse_reached_type_field(nested, keys);
    match (outer, nested) {
        (Some(a), Some(b)) if !a.eq_ignore_ascii_case(&b) => Some("unknown".to_string()),
        (Some(value), _) | (_, Some(value)) => Some(value),
        _ => None,
    }
}

fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = value.get(*key)?;
        match value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    })
}

fn integer(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| value.get(*key)?.as_i64())
}

fn parse_window(limit_id: &str, value: &Value, is_additional: bool) -> QuotaWindowSnapshot {
    QuotaWindowSnapshot {
        limit_id: value
            .get("limit_id")
            .or_else(|| value.get("limitId"))
            .and_then(Value::as_str)
            .unwrap_or(limit_id)
            .to_string(),
        is_additional,
        used_percent: number(value, &["used_percent", "usedPercent", "percent"]),
        limit_window_seconds: integer(value, &["limit_window_seconds", "limitWindowSeconds"]),
        reset_at: integer(value, &["reset_at", "resetAt"]),
        reset_after_seconds: integer(value, &["reset_after_seconds", "resetAfterSeconds"]),
        allowed: value.get("allowed").and_then(Value::as_bool),
        limit_reached: value
            .get("limit_reached")
            .or_else(|| value.get("limitReached"))
            .and_then(Value::as_bool),
        rate_limit_reached_type: value
            .get("rate_limit_reached_type")
            .or_else(|| value.get("rateLimitReachedType"))
            .and_then(parse_reached_type),
        permission_conflict: bool_fields_conflict_in_window(value),
    }
}

fn bool_fields_conflict_in_window(value: &Value) -> bool {
    [
        parse_bool_field(value, &["allowed"]),
        parse_bool_field(value, &["limit_reached", "limitReached"]),
    ]
    .iter()
    .any(|result| result.is_err())
}

fn collect_additional_windows(value: &Value, output: &mut Vec<QuotaWindowSnapshot>) {
    match value {
        Value::Array(items) => items
            .iter()
            .for_each(|item| collect_additional_windows(item, output)),
        Value::Object(map) => {
            let has_window_fields = [
                "used_percent",
                "usedPercent",
                "percent",
                "limit_window_seconds",
                "limitWindowSeconds",
            ]
            .iter()
            .any(|key| map.contains_key(*key));
            if has_window_fields {
                output.push(parse_window("additional", value, true));
            } else {
                for (name, item) in map {
                    let before = output.len();
                    collect_additional_windows(item, output);
                    for window in &mut output[before..] {
                        if window.limit_id == "additional" {
                            window.limit_id = name.clone();
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

fn valid_used(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && (0.0..=100.0).contains(&value))
}

fn blocking_type(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| !matches!(value.to_ascii_lowercase().as_str(), "none" | "not_reached"))
}

/// Select the unique target before lifecycle/claim checks. This is deliberately
/// fail-closed for malformed or incomplete window structures.
pub(crate) fn select_target(snapshot: &CodexQuotaSnapshot) -> ActivationDecision {
    select_target_at(snapshot, snapshot.observed_at_ms)
}

pub(crate) fn select_target_at(snapshot: &CodexQuotaSnapshot, now_ms: i64) -> ActivationDecision {
    if let Some(reason) = snapshot_freshness_reason(snapshot, now_ms) {
        return ActivationDecision::Blocked(reason);
    }
    let five = snapshot
        .windows
        .iter()
        .filter(|window| window.limit_window_seconds == Some(FIVE_HOUR_SECONDS))
        .collect::<Vec<_>>();
    let weekly = snapshot
        .windows
        .iter()
        .filter(|window| window.limit_window_seconds == Some(WEEKLY_SECONDS))
        .collect::<Vec<_>>();

    if snapshot.windows.iter().any(|window| {
        window.is_additional
            || !matches!(
                window.limit_window_seconds,
                Some(FIVE_HOUR_SECONDS | WEEKLY_SECONDS)
            )
    }) || five.len() > 1
        || weekly.len() > 1
    {
        return ActivationDecision::Unsupported("ambiguous_or_additional_window");
    }
    let five = five.first().copied();
    let weekly = weekly.first().copied();

    // A present but damaged 5h object is not equivalent to no 5h object.
    if five.is_some_and(|window| !valid_used(window.used_percent)) {
        return ActivationDecision::Unsupported("invalid_five_hour");
    }
    if weekly.is_some_and(|window| !valid_used(window.used_percent)) {
        return ActivationDecision::Unsupported("invalid_weekly");
    }

    let (target, target_window, weekly_window): (
        WindowKind,
        &QuotaWindowSnapshot,
        Option<&QuotaWindowSnapshot>,
    ) = if let Some(five) = five {
        let Some(weekly) = weekly else {
            return ActivationDecision::Unsupported("weekly_missing");
        };
        (WindowKind::FiveHour, five, Some(weekly))
    } else {
        let Some(weekly) = weekly else {
            return ActivationDecision::Unsupported("unsupported_window_shape");
        };
        (WindowKind::Weekly, weekly, Some(weekly))
    };

    let weekly_window = weekly_window.expect("weekly window always selected");
    let permission_conflict = snapshot.permission_conflict
        || target_window.permission_conflict
        || (target == WindowKind::FiveHour && weekly_window.permission_conflict);
    let mut allowed_values = vec![snapshot.allowed, target_window.allowed];
    let mut limit_values = vec![snapshot.limit_reached, target_window.limit_reached];
    if target == WindowKind::FiveHour {
        allowed_values.push(weekly_window.allowed);
        limit_values.push(weekly_window.limit_reached);
    }
    let allowed = consistent_bool(&allowed_values);
    let limit_reached = consistent_bool(&limit_values);
    let timing_valid = valid_timing(target_window, target, snapshot)
        && (target != WindowKind::FiveHour
            || valid_timing(weekly_window, WindowKind::Weekly, snapshot));
    if permission_conflict || allowed != Some(true) || limit_reached != Some(false) {
        // A coherent exhausted response is useful lifecycle evidence even
        // though it can never qualify a generation.  Keep it separate from
        // malformed/conflicting permission fields so bad data cannot move the
        // trusted boundary.
        if !permission_conflict
            && trusted_exhaustion_observation(
                snapshot,
                target,
                target_window,
                weekly_window,
                allowed,
                limit_reached,
            )
        {
            if !timing_valid {
                return ActivationDecision::Unsupported("invalid_window_timing");
            }
            return if weekly_window.used_percent == Some(100.0) {
                ActivationDecision::Blocked("weekly_exhausted")
            } else {
                ActivationDecision::Blocked("permission_or_limit")
            };
        }
        return ActivationDecision::Blocked("permission_or_limit");
    }
    if !timing_valid {
        return ActivationDecision::Unsupported("invalid_window_timing");
    }
    if weekly_window.used_percent == Some(100.0) {
        return ActivationDecision::Blocked("weekly_exhausted");
    }
    if blocking_type(snapshot.rate_limit_reached_type.as_deref())
        || blocking_type(target_window.rate_limit_reached_type.as_deref())
        || (target == WindowKind::FiveHour
            && blocking_type(weekly_window.rate_limit_reached_type.as_deref()))
    {
        return ActivationDecision::Blocked("rate_limit_reached");
    }
    if target_window.used_percent == Some(0.0) {
        ActivationDecision::Candidate(target)
    } else {
        ActivationDecision::Observe
    }
}

pub(crate) fn snapshot_freshness_reason(
    snapshot: &CodexQuotaSnapshot,
    now_ms: i64,
) -> Option<&'static str> {
    if snapshot.cache_age_seconds.is_some_and(|age| age > 0) {
        return Some("stale_snapshot");
    }
    if snapshot.observed_at_ms < snapshot.request_started_at_ms {
        return Some("invalid_snapshot_time");
    }
    let monotonic_request_age = snapshot
        .request_started_monotonic
        .zip(snapshot.observed_monotonic)
        .map(|(started, observed)| observed.saturating_duration_since(started));
    if monotonic_request_age.is_some_and(|age| age > Duration::from_secs(30))
        || (monotonic_request_age.is_none()
            && snapshot
                .observed_at_ms
                .saturating_sub(snapshot.request_started_at_ms)
                > 30_000)
    {
        return Some("stale_snapshot");
    }
    if now_ms < snapshot.observed_at_ms {
        return Some("clock_rollback");
    }
    if snapshot
        .observed_monotonic
        .is_some_and(|observed| observed.elapsed() > Duration::from_secs(30))
        || (snapshot.observed_monotonic.is_none()
            && now_ms.saturating_sub(snapshot.observed_at_ms) > 30_000)
    {
        return Some("stale_snapshot");
    }
    if let Some(server_date_at_ms) = snapshot.server_date_at_ms {
        if (server_date_at_ms - snapshot.observed_at_ms).abs() > 5 * 60_000 {
            return Some("server_date_conflict");
        }
    }
    None
}

fn valid_timing(
    window: &QuotaWindowSnapshot,
    kind: WindowKind,
    snapshot: &CodexQuotaSnapshot,
) -> bool {
    let Some(reset_at) = window.reset_at else {
        return false;
    };
    if reset_at < 0 {
        return false;
    }
    if window
        .reset_after_seconds
        .is_some_and(|after| after < 0 || after > kind.seconds().saturating_add(300))
    {
        return false;
    }
    let Some(server_date_at_ms) = snapshot.server_date_at_ms else {
        // `reset_at` is a server epoch.  Without a trustworthy HTTP Date we
        // cannot safely compare it with a possibly skewed local wall clock.
        // Age/timeout checks still use local timestamps, but lifecycle
        // qualification stops conservatively here.
        return false;
    };
    let server_date_seconds = server_date_at_ms.div_euclid(1000);
    if let Some(reset_after) = window.reset_after_seconds {
        if reset_after > 0 {
            let expected_reset_at = server_date_seconds.saturating_add(reset_after);
            if reset_at.abs_diff(expected_reset_at) > 5 {
                return false;
            }
        } else if reset_at > server_date_seconds.saturating_add(5) {
            // A zero countdown may describe a boundary that has just passed,
            // but it cannot point materially into the future.
            return false;
        }
    } else if reset_at > server_date_seconds.saturating_add(5) {
        // A missing countdown is weaker evidence, but a reset materially in
        // the future is still contradictory to the server's Date.
        return false;
    }
    true
}

fn consistent_bool(values: &[Option<bool>]) -> Option<bool> {
    let mut result = None;
    for value in values.iter().flatten().copied() {
        if result.is_some_and(|previous| previous != value) {
            return None;
        }
        result = Some(value);
    }
    result
}

impl ActivationState {
    pub(crate) fn observe(
        &mut self,
        snapshot: &CodexQuotaSnapshot,
        now_ms: i64,
    ) -> ActivationDecision {
        let decision = select_target_at(snapshot, now_ms);
        let target = match decision {
            ActivationDecision::Candidate(_) | ActivationDecision::Observe => {
                structurally_selected_target(snapshot)
            }
            ActivationDecision::Blocked("weekly_exhausted") => {
                structurally_selected_target(snapshot)
            }
            ActivationDecision::Blocked("permission_or_limit")
                if trusted_exhaustion_snapshot(snapshot) =>
            {
                structurally_selected_target(snapshot)
            }
            ActivationDecision::Blocked(_) | ActivationDecision::Unsupported(_) => None,
        };
        let Some(target) = target else {
            return decision;
        };
        let Some(window) = find_window(snapshot, target) else {
            return ActivationDecision::Unsupported("target_missing");
        };
        let previous_target = match self {
            Self::TrackingUsage { target, .. } | Self::Eligible { target, .. } => Some(*target),
            Self::Spent { .. } => None,
            Self::BootstrapPending | Self::AwaitingBaseline => None,
        };
        if previous_target.is_some_and(|previous| previous != target) {
            *self = Self::AwaitingBaseline;
            return ActivationDecision::Unsupported("window_structure_changed");
        }
        if !valid_used(window.used_percent) {
            return ActivationDecision::Unsupported("invalid_target");
        }
        let trusted_observation = match decision {
            ActivationDecision::Candidate(_) | ActivationDecision::Observe => true,
            ActivationDecision::Blocked("weekly_exhausted") => {
                valid_timing(window, target, snapshot)
                    && (target != WindowKind::FiveHour
                        || find_window(snapshot, WindowKind::Weekly).is_some_and(|weekly| {
                            valid_timing(weekly, WindowKind::Weekly, snapshot)
                        }))
            }
            ActivationDecision::Blocked("permission_or_limit")
                if trusted_exhaustion_snapshot(snapshot) =>
            {
                valid_timing(window, target, snapshot)
                    && (target != WindowKind::FiveHour
                        || find_window(snapshot, WindowKind::Weekly).is_some_and(|weekly| {
                            valid_timing(weekly, WindowKind::Weekly, snapshot)
                        }))
            }
            ActivationDecision::Blocked(_) | ActivationDecision::Unsupported(_) => false,
        };
        if !trusted_observation {
            return decision;
        }
        let can_qualify = matches!(decision, ActivationDecision::Candidate(_));
        let event_key = format!(
            "{}:{}",
            target.as_str(),
            window.reset_at.unwrap_or_default()
        );
        match self {
            Self::BootstrapPending => {
                if window.used_percent == Some(0.0) && can_qualify {
                    *self = Self::Eligible { target, event_key };
                    decision
                } else if window.used_percent.is_some_and(|value| value > 0.0) {
                    if !current_nonzero_window_is_valid(snapshot, window, target) {
                        return decision;
                    }
                    let Some(boundary_reset_at) = window.reset_at else {
                        *self = Self::AwaitingBaseline;
                        return ActivationDecision::Unsupported("missing_window_boundary");
                    };
                    *self = Self::TrackingUsage {
                        target,
                        boundary_reset_at,
                    };
                    decision
                } else {
                    // A blocked zero observation does not consume the first-use
                    // qualification. It may become eligible on the next normal tick.
                    decision
                }
            }
            Self::TrackingUsage {
                target: previous,
                boundary_reset_at,
            } => {
                if window.used_percent.is_some_and(|value| value > 0.0) {
                    if current_nonzero_window_is_valid(snapshot, window, target) {
                        if let Some(reset_at) =
                            window.reset_at.filter(|reset| *reset >= *boundary_reset_at)
                        {
                            *boundary_reset_at = reset_at;
                        }
                    }
                    decision
                } else if can_qualify
                    && *previous == target
                    && server_observation_seconds(snapshot)
                        .is_some_and(|observed| observed > *boundary_reset_at)
                    && window
                        .reset_at
                        .is_some_and(|reset| reset >= *boundary_reset_at)
                {
                    *self = Self::Eligible { target, event_key };
                    decision
                } else {
                    ActivationDecision::Observe
                }
            }
            Self::Eligible {
                target: previous, ..
            } => {
                if *previous != target {
                    *self = Self::AwaitingBaseline;
                    ActivationDecision::Unsupported("window_structure_changed")
                } else if window.used_percent.is_some_and(|value| value > 0.0) {
                    if current_nonzero_window_is_valid(snapshot, window, target) {
                        if let Some(reset_at) = window.reset_at {
                            *self = Self::TrackingUsage {
                                target,
                                boundary_reset_at: reset_at,
                            };
                        }
                    }
                    decision
                } else {
                    decision
                }
            }
            Self::Spent {
                target: previous,
                event_key,
                attempted_at_ms,
                ..
            } => {
                if *previous != target {
                    *self = Self::AwaitingBaseline;
                    return ActivationDecision::Unsupported("window_structure_changed");
                }
                // A nonzero observation after the attempt establishes use in
                // the current window even while its reset boundary is still
                // ahead. Crossing that boundary is reserved for the later
                // zero observation that creates the next candidate.
                if *previous == target
                    && window.used_percent.is_some_and(|value| value > 0.0)
                    && server_observation_after_attempt(snapshot, *attempted_at_ms)
                    && current_nonzero_window_is_valid(snapshot, window, target)
                    && window.reset_at.is_some_and(|reset| {
                        event_boundary(event_key).is_some_and(|boundary| reset >= boundary)
                    })
                {
                    *self = Self::TrackingUsage {
                        target,
                        boundary_reset_at: window.reset_at.unwrap_or_default(),
                    };
                }
                if matches!(decision, ActivationDecision::Candidate(_)) {
                    // A Spent account still needs a trusted nonzero
                    // observation before a later zero can qualify.
                    ActivationDecision::Observe
                } else {
                    decision
                }
            }
            Self::AwaitingBaseline => {
                if window.used_percent.is_some_and(|value| value > 0.0)
                    && current_nonzero_window_is_valid(snapshot, window, target)
                {
                    if let Some(reset_at) = window.reset_at {
                        *self = Self::TrackingUsage {
                            target,
                            boundary_reset_at: reset_at,
                        };
                    }
                    decision
                } else {
                    // AwaitingBaseline deliberately cannot return Candidate: a
                    // lifecycle reset must first observe a trustworthy nonzero use.
                    ActivationDecision::Observe
                }
            }
        }
    }

    pub(crate) fn claim(
        &mut self,
        attempt_id: impl Into<String>,
        attempted_at_ms: i64,
    ) -> Option<ClaimedAttempt> {
        let (target, event_key) = match self {
            Self::Eligible { target, event_key } => (*target, event_key.clone()),
            _ => return None,
        };
        let attempt_id = attempt_id.into();
        *self = Self::Spent {
            target,
            event_key,
            attempt_id: attempt_id.clone(),
            attempted_at_ms,
        };
        Some(ClaimedAttempt { attempt_id, target })
    }
}

fn server_observation_seconds(snapshot: &CodexQuotaSnapshot) -> Option<i64> {
    snapshot
        .server_date_at_ms
        .map(|value| value.div_euclid(1000))
}

fn server_observation_after_attempt(snapshot: &CodexQuotaSnapshot, attempted_at_ms: i64) -> bool {
    // Attempt timestamps and request start timestamps are both persisted in
    // the local wall-clock domain. HTTP Date independently validates the
    // server window fields; it must not be used to reinterpret an old local
    // attempt after a clock offset changes.
    snapshot.server_date_at_ms.is_some() && snapshot.request_started_at_ms > attempted_at_ms
}

fn current_nonzero_window_is_valid(
    snapshot: &CodexQuotaSnapshot,
    window: &QuotaWindowSnapshot,
    kind: WindowKind,
) -> bool {
    let Some(server_date_at_ms) = snapshot.server_date_at_ms else {
        return false;
    };
    let server_seconds = server_date_at_ms.div_euclid(1000);
    window.reset_after_seconds.is_some_and(|after| after > 0)
        && window.reset_at.is_some_and(|reset| reset > server_seconds)
        && valid_timing(window, kind, snapshot)
}

fn trusted_exhaustion_snapshot(snapshot: &CodexQuotaSnapshot) -> bool {
    let Some(target) = structurally_selected_target(snapshot) else {
        return false;
    };
    let Some(target_window) = find_window(snapshot, target) else {
        return false;
    };
    let Some(weekly_window) = find_window(snapshot, WindowKind::Weekly) else {
        return false;
    };
    let allowed = if target == WindowKind::FiveHour {
        consistent_bool(&[
            snapshot.allowed,
            target_window.allowed,
            weekly_window.allowed,
        ])
    } else {
        consistent_bool(&[snapshot.allowed, target_window.allowed])
    };
    let limit_reached = if target == WindowKind::FiveHour {
        consistent_bool(&[
            snapshot.limit_reached,
            target_window.limit_reached,
            weekly_window.limit_reached,
        ])
    } else {
        consistent_bool(&[snapshot.limit_reached, target_window.limit_reached])
    };
    trusted_exhaustion_observation(
        snapshot,
        target,
        target_window,
        weekly_window,
        allowed,
        limit_reached,
    )
}

fn trusted_exhaustion_observation(
    snapshot: &CodexQuotaSnapshot,
    target: WindowKind,
    target_window: &QuotaWindowSnapshot,
    weekly_window: &QuotaWindowSnapshot,
    allowed: Option<bool>,
    limit_reached: Option<bool>,
) -> bool {
    if snapshot.permission_conflict
        || target_window.permission_conflict
        || (target == WindowKind::FiveHour && weekly_window.permission_conflict)
    {
        return false;
    }
    if allowed != Some(false) || limit_reached != Some(true) {
        return false;
    }
    target_window.used_percent.is_some_and(|value| value > 0.0)
        || weekly_window.used_percent == Some(100.0)
}

fn event_boundary(event_key: &str) -> Option<i64> {
    event_key.rsplit_once(':')?.1.parse().ok()
}

fn structurally_selected_target(snapshot: &CodexQuotaSnapshot) -> Option<WindowKind> {
    let five = snapshot
        .windows
        .iter()
        .filter(|window| {
            !window.is_additional && window.limit_window_seconds == Some(FIVE_HOUR_SECONDS)
        })
        .collect::<Vec<_>>();
    let weekly = snapshot
        .windows
        .iter()
        .filter(|window| {
            !window.is_additional && window.limit_window_seconds == Some(WEEKLY_SECONDS)
        })
        .collect::<Vec<_>>();
    if five.len() == 1
        && weekly.len() == 1
        && valid_used(five[0].used_percent)
        && valid_used(weekly[0].used_percent)
    {
        Some(WindowKind::FiveHour)
    } else if five.is_empty() && weekly.len() == 1 && valid_used(weekly[0].used_percent) {
        Some(WindowKind::Weekly)
    } else {
        None
    }
}

fn find_window(snapshot: &CodexQuotaSnapshot, kind: WindowKind) -> Option<&QuotaWindowSnapshot> {
    snapshot
        .windows
        .iter()
        .find(|window| !window.is_additional && window.limit_window_seconds == Some(kind.seconds()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(five: Option<f64>, weekly: Option<f64>) -> Value {
        let window = |used: Option<f64>, seconds: i64| {
            used.map(|used| json!({"used_percent": used, "limit_window_seconds": seconds, "reset_at": 1_800_018_000, "reset_after_seconds": seconds}))
                .unwrap_or_else(|| json!({"limit_window_seconds": seconds, "reset_at": 1_800_018_000, "reset_after_seconds": seconds}))
        };
        json!({"rate_limit": {"allowed": true, "limit_reached": false, "primary_window": five.map(|_| window(five, FIVE_HOUR_SECONDS)), "secondary_window": weekly.map(|_| window(weekly, WEEKLY_SECONDS))}})
    }

    fn snapshot(five: Option<f64>, weekly: Option<f64>) -> CodexQuotaSnapshot {
        let mut snapshot = snapshot_from_value(&response(five, weekly), 0, 1).unwrap();
        // Unit fixtures model a trusted HTTP Date. Production snapshots set
        // this from the response header; missing-Date cases are explicit.
        snapshot.server_date_at_ms = Some(snapshot.observed_at_ms);
        for window in &mut snapshot.windows {
            if let Some(reset_after) = window.reset_after_seconds {
                window.reset_at = Some(snapshot.observed_at_ms / 1000 + reset_after);
            }
        }
        snapshot
    }

    fn snapshot_at(
        five: Option<f64>,
        weekly: Option<f64>,
        request_started_at_ms: i64,
        reset_at: i64,
    ) -> CodexQuotaSnapshot {
        let mut snapshot = snapshot(five, weekly);
        snapshot.request_started_at_ms = request_started_at_ms;
        snapshot.observed_at_ms = request_started_at_ms;
        snapshot.server_date_at_ms = Some(request_started_at_ms);
        for window in &mut snapshot.windows {
            window.reset_at = Some(reset_at);
            window.reset_after_seconds = Some(
                reset_at
                    .saturating_sub(request_started_at_ms.div_euclid(1000))
                    .max(0),
            );
        }
        snapshot
    }

    fn audit_observation(
        server_seconds: i64,
        five_hour: Option<f64>,
        five_reset: Option<i64>,
    ) -> CodexQuotaSnapshot {
        let value = json!({
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": five_hour.map(|used| json!({
                    "used_percent": used,
                    "limit_window_seconds": FIVE_HOUR_SECONDS,
                    "reset_at": five_reset.unwrap(),
                    "reset_after_seconds": five_reset.unwrap().saturating_sub(server_seconds).max(0)
                })),
                "secondary_window": {
                    "used_percent": 50,
                    "limit_window_seconds": WEEKLY_SECONDS,
                    "reset_at": server_seconds + WEEKLY_SECONDS,
                    "reset_after_seconds": WEEKLY_SECONDS
                }
            }
        });
        let mut snapshot =
            snapshot_from_value(&value, server_seconds * 1000, server_seconds * 1000).unwrap();
        snapshot.server_date_at_ms = Some(server_seconds * 1000);
        snapshot.cache_age_seconds = Some(0);
        snapshot
    }

    #[test]
    fn strict_zero_preserves_small_nonzero_values() {
        assert_eq!(
            select_target(&snapshot(Some(0.0), Some(50.0))),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert_eq!(
            select_target(&snapshot(Some(0.001), Some(0.0))),
            ActivationDecision::Observe
        );
        assert_eq!(
            select_target(&snapshot(Some(0.1), Some(0.0))),
            ActivationDecision::Observe
        );
    }

    #[test]
    fn five_hour_wins_and_weekly_exhaustion_blocks() {
        assert_eq!(
            select_target(&snapshot(Some(0.0), Some(0.0))),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert_eq!(
            select_target(&snapshot(Some(0.0), Some(100.0))),
            ActivationDecision::Blocked("weekly_exhausted")
        );
    }

    #[test]
    fn damaged_five_hour_does_not_fallback_to_weekly() {
        let value = json!({"rate_limit": {"allowed": true, "limit_reached": false, "primary_window": {"limit_window_seconds": FIVE_HOUR_SECONDS}, "secondary_window": {"used_percent": 0, "limit_window_seconds": WEEKLY_SECONDS}}});
        let parsed = snapshot_from_value(&value, 1, 1).unwrap();
        assert_eq!(
            select_target(&parsed),
            ActivationDecision::Unsupported("invalid_five_hour")
        );
    }

    #[test]
    fn weekly_only_can_be_selected() {
        assert_eq!(
            select_target(&snapshot(None, Some(0.0))),
            ActivationDecision::Candidate(WindowKind::Weekly)
        );
    }

    #[test]
    fn claim_is_at_most_once_for_repeated_zero() {
        let mut state = ActivationState::default();
        let snap = snapshot(Some(0.0), Some(50.0));
        assert!(matches!(
            state.observe(&snap, 1),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
        assert!(state.claim("attempt-1", 1).is_some());
        assert_eq!(state.observe(&snap, 2), ActivationDecision::Observe);
        assert!(state.claim("attempt-2", 2).is_none());
    }

    #[test]
    fn weekly_exhaustion_never_creates_a_claim() {
        let mut state = ActivationState::default();
        let exhausted = snapshot(Some(0.0), Some(100.0));
        assert_eq!(
            state.observe(&exhausted, 1),
            ActivationDecision::Blocked("weekly_exhausted")
        );
        assert!(state.claim("attempt", 1).is_none());
    }

    #[test]
    fn spent_requires_a_later_nonzero_observation_before_rearming() {
        let mut state = ActivationState::default();
        let zero = snapshot(Some(0.0), Some(50.0));
        assert!(matches!(
            state.observe(&zero, 1),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
        assert!(state.claim("attempt", 1).is_some());
        assert_eq!(state.observe(&zero, 2), ActivationDecision::Observe);

        let used = snapshot_at(Some(20.0), Some(50.0), 18_001_000, 36_001);
        assert_eq!(
            state.observe(&used, 18_001_000),
            ActivationDecision::Observe
        );

        let recovered = snapshot_at(Some(0.0), Some(50.0), 1_800_018_001_000, 1_800_018_001);
        assert_eq!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
    }

    #[test]
    fn f04_stale_snapshot_after_model_preflight_clock_advance_is_rejected() {
        let mut state = ActivationState::default();
        let snapshot = snapshot_at(Some(0.0), Some(50.0), 1_000, 300);

        assert_eq!(
            state.observe(&snapshot, 1_000),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        let trusted_state = state.clone();

        assert_eq!(
            state.observe(&snapshot, 31_001),
            ActivationDecision::Blocked("stale_snapshot")
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn f04_time_rollback_and_server_date_conflict_fail_closed() {
        let mut snapshot = snapshot_at(Some(0.0), Some(50.0), 1_000, 300);
        assert_eq!(
            snapshot_freshness_reason(&snapshot, 999),
            Some("clock_rollback")
        );
        snapshot.server_date_at_ms = Some(1_000_000);
        assert_eq!(
            snapshot_freshness_reason(&snapshot, 1_000),
            Some("server_date_conflict")
        );
    }

    #[test]
    fn n03_reset_boundary_and_countdown_conflict_cannot_create_candidate() {
        let now_ms = 1_800_000_000_000;
        let mut snapshot = snapshot_at(Some(0.0), Some(50.0), now_ms, 1_799_999_900);
        for window in &mut snapshot.windows {
            window.reset_after_seconds = Some(window.limit_window_seconds.unwrap_or(0));
        }
        assert_eq!(
            select_target_at(&snapshot, now_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        let mut state = ActivationState::default();
        assert_eq!(
            state.observe(&snapshot, now_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        assert_eq!(state, ActivationState::BootstrapPending);

        // A weekly block still permits a trustworthy nonzero five-hour
        // observation to establish its baseline; only the contradictory
        // timing is rejected.
        snapshot = snapshot_at(Some(20.0), Some(100.0), now_ms, 1_799_999_900);
        for window in &mut snapshot.windows {
            window.reset_after_seconds = Some(window.limit_window_seconds.unwrap_or(0));
        }
        assert_eq!(
            state.observe(&snapshot, now_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        assert_eq!(state, ActivationState::BootstrapPending);
    }

    #[test]
    fn n03_all_epoch_offsets_use_the_same_timing_rule() {
        let now_ms = 1_800_000_000_000;
        for (offset, expected) in [
            (
                -301,
                ActivationDecision::Unsupported("invalid_window_timing"),
            ),
            (
                -100,
                ActivationDecision::Unsupported("invalid_window_timing"),
            ),
            (30, ActivationDecision::Unsupported("invalid_window_timing")),
            (18_000, ActivationDecision::Candidate(WindowKind::FiveHour)),
        ] {
            let mut snapshot = snapshot(Some(0.0), Some(50.0));
            snapshot.request_started_at_ms = now_ms;
            snapshot.observed_at_ms = now_ms;
            snapshot.server_date_at_ms = Some(now_ms);
            for window in &mut snapshot.windows {
                let duration = window.limit_window_seconds.unwrap_or(0);
                let reset_in = if duration == FIVE_HOUR_SECONDS {
                    offset
                } else {
                    duration
                };
                window.reset_at = Some(now_ms / 1000 + reset_in);
                window.reset_after_seconds = Some(duration);
            }
            assert_eq!(select_target_at(&snapshot, now_ms), expected);
        }
    }

    #[test]
    fn s03_server_date_normalizes_local_clock_skew_and_body_delay() {
        let server_now = 1_800_000_000_i64;
        let mut skewed = snapshot(Some(0.0), Some(50.0));
        skewed.request_started_at_ms = (server_now + 60) * 1000;
        skewed.observed_at_ms = (server_now + 61) * 1000;
        skewed.server_date_at_ms = Some(server_now * 1000);
        for window in &mut skewed.windows {
            let duration = window.limit_window_seconds.unwrap();
            window.reset_at = Some(server_now + duration);
            window.reset_after_seconds = Some(duration);
        }
        assert_eq!(
            select_target_at(&skewed, skewed.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );

        let mut delayed = skewed.clone();
        delayed.request_started_at_ms = server_now * 1000;
        delayed.observed_at_ms = (server_now + 8) * 1000;
        delayed.server_date_at_ms = Some(server_now * 1000);
        assert_eq!(
            select_target_at(&delayed, delayed.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
    }

    #[test]
    fn s03_missing_date_is_conservative_and_does_not_rearm() {
        let mut missing_date = snapshot_at(Some(0.0), Some(50.0), 1_800_000_000_000, 1_800_018_000);
        missing_date.server_date_at_ms = None;
        assert_eq!(
            select_target_at(&missing_date, missing_date.observed_at_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );

        let mut state = ActivationState::TrackingUsage {
            target: WindowKind::FiveHour,
            boundary_reset_at: 1_800_018_000,
        };
        let trusted_state = state.clone();
        assert_eq!(
            state.observe(&missing_date, missing_date.observed_at_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn f05_tracking_usage_boundary_does_not_rollback_after_later_observation() {
        let mut state = ActivationState::default();
        let initial = snapshot_at(Some(20.0), Some(50.0), 1_000, 300);
        assert_eq!(state.observe(&initial, 1_000), ActivationDecision::Observe);
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::FiveHour,
                boundary_reset_at: 300,
            }
        );

        let advanced = snapshot_at(Some(20.0), Some(50.0), 2_000, 400);
        assert_eq!(state.observe(&advanced, 2_000), ActivationDecision::Observe);
        let trusted_state = state.clone();

        let rollback = snapshot_at(Some(20.0), Some(50.0), 3_000, 350);
        assert_eq!(state.observe(&rollback, 3_000), ActivationDecision::Observe);
        assert_eq!(state, trusted_state);

        let zero_before_trusted_boundary = snapshot_at(Some(0.0), Some(50.0), 401_000, 390);
        assert_eq!(
            state.observe(&zero_before_trusted_boundary, 401_000),
            ActivationDecision::Observe
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn f05_spent_rearm_does_not_accept_boundary_rollback() {
        let mut state = ActivationState::default();
        let zero = snapshot_at(Some(0.0), Some(50.0), 1_000, 300);
        assert_eq!(
            state.observe(&zero, 1_000),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(state.claim("attempt", 2_000).is_some());

        let used_after_attempt = snapshot_at(Some(20.0), Some(50.0), 3_000, 400);
        assert_eq!(
            state.observe(&used_after_attempt, 3_000),
            ActivationDecision::Observe
        );
        let trusted_state = state.clone();

        let rollback = snapshot_at(Some(20.0), Some(50.0), 4_000, 350);
        assert_eq!(state.observe(&rollback, 4_000), ActivationDecision::Observe);
        assert_eq!(state, trusted_state);

        let zero_before_trusted_boundary = snapshot_at(Some(0.0), Some(50.0), 401_000, 390);
        assert_eq!(
            state.observe(&zero_before_trusted_boundary, 401_000),
            ActivationDecision::Observe
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn f05_invalid_observation_does_not_change_trusted_state() {
        let mut state = ActivationState::TrackingUsage {
            target: WindowKind::FiveHour,
            boundary_reset_at: 400,
        };
        let mut invalid = snapshot_at(Some(20.0), Some(50.0), 5_000, 500);
        invalid.allowed = Some(false);
        let trusted_state = state.clone();

        assert_eq!(
            state.observe(&invalid, 5_000),
            ActivationDecision::Blocked("permission_or_limit")
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn f05_weekly_exhausted_valid_five_hour_nonzero_still_records_baseline() {
        let mut state = ActivationState::default();
        let exhausted_weekly = snapshot_at(Some(20.0), Some(100.0), 6_000, 600);

        assert_eq!(
            state.observe(&exhausted_weekly, 6_000),
            ActivationDecision::Blocked("weekly_exhausted")
        );
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::FiveHour,
                boundary_reset_at: 600,
            }
        );
    }

    #[test]
    fn s02_trusted_permission_exhaustion_rearms_spent_after_boundary() {
        let mut state = ActivationState::Spent {
            target: WindowKind::FiveHour,
            event_key: "five_hour:300".to_string(),
            attempt_id: "attempt".to_string(),
            attempted_at_ms: 1_000,
        };
        let mut exhausted = snapshot_at(Some(20.0), Some(50.0), 301_000, 400);
        exhausted.allowed = Some(false);
        exhausted.limit_reached = Some(true);
        assert_eq!(
            state.observe(&exhausted, 301_000),
            ActivationDecision::Blocked("permission_or_limit")
        );
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::FiveHour,
                boundary_reset_at: 400,
            }
        );

        let recovered = snapshot_at(Some(0.0), Some(50.0), 401_000, 400);
        assert_eq!(
            state.observe(&recovered, 401_000),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(matches!(state, ActivationState::Eligible { .. }));
    }

    #[test]
    fn s02_conflicted_exhaustion_does_not_move_trusted_state() {
        let mut state = ActivationState::TrackingUsage {
            target: WindowKind::FiveHour,
            boundary_reset_at: 400,
        };
        let mut conflicted = snapshot_at(Some(20.0), Some(50.0), 2_000, 500);
        conflicted.allowed = Some(false);
        conflicted.limit_reached = Some(true);
        conflicted.permission_conflict = true;
        let trusted_state = state.clone();
        assert_eq!(
            state.observe(&conflicted, 2_000),
            ActivationDecision::Blocked("permission_or_limit")
        );
        assert_eq!(state, trusted_state);
    }

    #[test]
    fn t01_post_attempt_nonzero_before_old_boundary_rearms_five_hour() {
        let mut state = ActivationState::default();
        let initial = snapshot_at(Some(0.0), Some(50.0), 1_000_000, 19_000);
        assert_eq!(
            state.observe(&initial, initial.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(state.claim("attempt", 1_000_100).is_some());

        let used_before_boundary = snapshot_at(Some(20.0), Some(50.0), 1_300_000, 19_000);
        assert_eq!(
            state.observe(&used_before_boundary, used_before_boundary.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::FiveHour,
                boundary_reset_at: 19_000,
            }
        );

        let recovered = snapshot_at(Some(0.0), Some(50.0), 19_001_000, 19_000);
        assert_eq!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(state
            .claim("second-attempt", recovered.observed_at_ms)
            .is_some());
    }

    #[test]
    fn t01_post_attempt_exhaustion_before_old_boundary_rearms_five_hour() {
        let mut state = ActivationState::Spent {
            target: WindowKind::FiveHour,
            event_key: "five_hour:19000".to_string(),
            attempt_id: "attempt".to_string(),
            attempted_at_ms: 1_000_100,
        };
        let mut exhausted = snapshot_at(Some(100.0), Some(50.0), 1_300_000, 19_000);
        exhausted.allowed = Some(false);
        exhausted.limit_reached = Some(true);
        assert_eq!(
            state.observe(&exhausted, exhausted.observed_at_ms),
            ActivationDecision::Blocked("permission_or_limit")
        );
        assert!(matches!(state, ActivationState::TrackingUsage { .. }));

        let recovered = snapshot_at(Some(0.0), Some(50.0), 19_001_000, 19_000);
        assert_eq!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
    }

    #[test]
    fn t01_post_attempt_nonzero_before_old_boundary_rearms_weekly_only() {
        let mut state = ActivationState::default();
        let initial = snapshot_at(None, Some(0.0), 1_000_000, 605_800);
        assert_eq!(
            state.observe(&initial, initial.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::Weekly)
        );
        assert!(state.claim("attempt", 1_000_100).is_some());

        let used_before_boundary = snapshot_at(None, Some(20.0), 1_300_000, 605_800);
        assert_eq!(
            state.observe(&used_before_boundary, used_before_boundary.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::Weekly,
                boundary_reset_at: 605_800,
            }
        );

        let recovered = snapshot_at(None, Some(0.0), 605_801_000, 605_800);
        assert_eq!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::Weekly)
        );
    }

    #[test]
    fn u01_expired_five_hour_nonzero_cannot_rearm_but_t01_current_window_still_can() {
        const B: i64 = 1_800_000_000;
        let mut expired_state = ActivationState::default();
        let first = audit_observation(B + 60, Some(0.0), Some(B));
        assert_eq!(
            expired_state.observe(&first, first.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(expired_state
            .claim("attempt-a", first.observed_at_ms)
            .is_some());

        let expired_nonzero = audit_observation(B + 120, Some(20.0), Some(B));
        assert_eq!(
            expired_state.observe(&expired_nonzero, expired_nonzero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(expired_state, ActivationState::Spent { .. }));
        let repeated_zero = audit_observation(B + 180, Some(0.0), Some(B));
        assert_eq!(
            expired_state.observe(&repeated_zero, repeated_zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(expired_state, ActivationState::Spent { .. }));
        assert!(expired_state
            .claim("attempt-b", repeated_zero.observed_at_ms)
            .is_none());

        let mut valid_state = ActivationState::default();
        let before_boundary = audit_observation(B - 60, Some(0.0), Some(B));
        assert!(matches!(
            valid_state.observe(&before_boundary, before_boundary.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
        assert!(valid_state
            .claim("valid-a", before_boundary.observed_at_ms)
            .is_some());
        let valid_nonzero = audit_observation(B - 30, Some(20.0), Some(B));
        assert_eq!(
            valid_state.observe(&valid_nonzero, valid_nonzero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(valid_state, ActivationState::TrackingUsage { .. }));
        let recovered = audit_observation(B + 1, Some(0.0), Some(B));
        assert!(matches!(
            valid_state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
    }

    #[test]
    fn u01_expired_weekly_only_nonzero_cannot_rearm() {
        const B: i64 = 1_800_000_000;
        let mut state = ActivationState::default();
        let first = audit_observation(B + 60, None, None);
        // The weekly fixture is currently future-dated. Move it to the same
        // expired boundary while keeping Date and countdown internally valid.
        let mut first = first;
        let weekly = first
            .windows
            .iter_mut()
            .find(|window| window.limit_window_seconds == Some(WEEKLY_SECONDS))
            .unwrap();
        weekly.used_percent = Some(0.0);
        weekly.reset_at = Some(B);
        weekly.reset_after_seconds = Some(0);
        assert_eq!(
            state.observe(&first, first.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::Weekly)
        );
        assert!(state.claim("weekly-a", first.observed_at_ms).is_some());

        let mut expired_nonzero = audit_observation(B + 120, None, None);
        let weekly = expired_nonzero
            .windows
            .iter_mut()
            .find(|window| window.limit_window_seconds == Some(WEEKLY_SECONDS))
            .unwrap();
        weekly.used_percent = Some(20.0);
        weekly.reset_at = Some(B);
        weekly.reset_after_seconds = Some(0);
        assert_eq!(
            state.observe(&expired_nonzero, expired_nonzero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(state, ActivationState::Spent { .. }));
        let mut next_zero = audit_observation(B + 180, None, None);
        let weekly = next_zero
            .windows
            .iter_mut()
            .find(|window| window.limit_window_seconds == Some(WEEKLY_SECONDS))
            .unwrap();
        weekly.used_percent = Some(0.0);
        weekly.reset_at = Some(B);
        weekly.reset_after_seconds = Some(0);
        assert_eq!(
            state.observe(&next_zero, next_zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(state.claim("weekly-b", next_zero.observed_at_ms).is_none());
    }

    #[test]
    fn u01_post_attempt_nonzero_with_a_new_future_boundary_is_valid() {
        const B: i64 = 1_800_000_000;
        let mut state = ActivationState::default();
        let first = audit_observation(B - 60, Some(0.0), Some(B));
        assert!(matches!(
            state.observe(&first, first.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
        assert!(state.claim("attempt", first.observed_at_ms).is_some());

        let new_boundary = B + FIVE_HOUR_SECONDS;
        let used = audit_observation(B + 120, Some(20.0), Some(new_boundary));
        assert_eq!(
            state.observe(&used, used.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_eq!(
            state,
            ActivationState::TrackingUsage {
                target: WindowKind::FiveHour,
                boundary_reset_at: new_boundary,
            }
        );

        let recovered = audit_observation(new_boundary + 1, Some(0.0), Some(new_boundary));
        assert!(matches!(
            state.observe(&recovered, recovered.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        ));
    }

    #[test]
    fn v01_shape_change_awaiting_baseline_rejects_expired_nonzero() {
        const B: i64 = 1_800_000_000;
        let mut state = ActivationState::default();
        let first = audit_observation(B + 60, Some(0.0), Some(B));
        assert_eq!(
            state.observe(&first, first.observed_at_ms),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(state.claim("first", first.observed_at_ms).is_some());

        let weekly_only = audit_observation(B + 300, None, None);
        assert_eq!(
            state.observe(&weekly_only, weekly_only.observed_at_ms),
            ActivationDecision::Unsupported("window_structure_changed")
        );
        assert!(matches!(state, ActivationState::AwaitingBaseline));

        let expired_nonzero = audit_observation(B + 600, Some(20.0), Some(B));
        assert_eq!(
            state.observe(&expired_nonzero, expired_nonzero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(state, ActivationState::AwaitingBaseline));
        let expired_zero = audit_observation(B + 900, Some(0.0), Some(B));
        assert!(!matches!(
            state.observe(&expired_zero, expired_zero.observed_at_ms),
            ActivationDecision::Candidate(_)
        ));
        assert!(state
            .claim("must-not-send", expired_zero.observed_at_ms)
            .is_none());
    }

    #[test]
    fn v01_awaiting_baseline_rejects_expired_nonzero_with_missing_countdown() {
        const B: i64 = 1_800_000_000;
        let mut state = ActivationState::AwaitingBaseline;
        let mut expired = audit_observation(B + 300, Some(20.0), Some(B));
        if let Some(window) = expired
            .windows
            .iter_mut()
            .find(|window| window.limit_window_seconds == Some(FIVE_HOUR_SECONDS))
        {
            window.reset_after_seconds = None;
        }
        assert_eq!(
            state.observe(&expired, expired.observed_at_ms),
            ActivationDecision::Observe
        );
        assert!(matches!(state, ActivationState::AwaitingBaseline));
    }

    #[test]
    fn f05_audit_bad_timing_never_rolls_back_tracking_boundary() {
        let mut state = ActivationState::TrackingUsage {
            target: WindowKind::FiveHour,
            boundary_reset_at: 1_800_001_000,
        };
        let mut invalid = snapshot_at(Some(20.0), Some(50.0), 1_800_000_000_000, 1_799_999_990);
        for window in &mut invalid.windows {
            if window.limit_window_seconds == Some(FIVE_HOUR_SECONDS) {
                window.reset_after_seconds = Some(-1);
            }
        }
        let trusted = state.clone();
        assert_eq!(
            state.observe(&invalid, invalid.observed_at_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        assert_eq!(state, trusted);

        let zero = snapshot_at(Some(0.0), Some(50.0), 1_800_000_300_000, 1_800_001_000);
        assert_eq!(
            state.observe(&zero, zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_eq!(state, trusted);
    }

    #[test]
    fn f05_audit_bad_timing_never_rearms_spent_state() {
        let mut state = ActivationState::Spent {
            target: WindowKind::FiveHour,
            event_key: "five_hour:1800001000".to_string(),
            attempt_id: "fake-prior-attempt".to_string(),
            attempted_at_ms: 1_799_999_400_000,
        };
        let mut invalid = snapshot_at(Some(20.0), Some(50.0), 1_800_000_000_000, 1_799_999_990);
        for window in &mut invalid.windows {
            if window.limit_window_seconds == Some(FIVE_HOUR_SECONDS) {
                window.reset_after_seconds = Some(-1);
            }
        }
        let trusted = state.clone();
        assert_eq!(
            state.observe(&invalid, invalid.observed_at_ms),
            ActivationDecision::Unsupported("invalid_window_timing")
        );
        assert_eq!(state, trusted);
        let zero = snapshot_at(Some(0.0), Some(50.0), 1_800_000_300_000, 1_800_001_000);
        assert_eq!(
            state.observe(&zero, zero.observed_at_ms),
            ActivationDecision::Observe
        );
        assert_eq!(state, trusted);
        assert!(state.claim("second-attempt", zero.observed_at_ms).is_none());
    }

    #[test]
    fn unknown_permission_fails_closed() {
        let value = json!({"rate_limit": {"primary_window": {"used_percent": 0, "limit_window_seconds": FIVE_HOUR_SECONDS}, "secondary_window": {"used_percent": 0, "limit_window_seconds": WEEKLY_SECONDS}}});
        assert_eq!(
            select_target(&snapshot_from_value(&value, 1, 1).unwrap()),
            ActivationDecision::Blocked("permission_or_limit")
        );
    }

    #[test]
    fn permission_conflicts_and_nested_rate_limit_are_retained() {
        let conflict = json!({
            "allowed": true,
            "limit_reached": false,
            "rate_limit": {
                "allowed": false,
                "limit_reached": true,
                "primary_window": {"used_percent": 0, "limit_window_seconds": FIVE_HOUR_SECONDS, "reset_at": FIVE_HOUR_SECONDS, "reset_after_seconds": FIVE_HOUR_SECONDS},
                "secondary_window": {"used_percent": 50, "limit_window_seconds": WEEKLY_SECONDS, "reset_at": WEEKLY_SECONDS, "reset_after_seconds": WEEKLY_SECONDS}
            }
        });
        let parsed = snapshot_from_value(&conflict, 1, 1).unwrap();
        assert_eq!(
            select_target(&parsed),
            ActivationDecision::Blocked("permission_or_limit")
        );

        let nested_type = json!({
            "allowed": true,
            "limit_reached": false,
            "rate_limit_reached_type": null,
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "rate_limit_reached_type": "unknown-server-block",
                "primary_window": {"used_percent": 0, "limit_window_seconds": FIVE_HOUR_SECONDS, "reset_at": FIVE_HOUR_SECONDS, "reset_after_seconds": FIVE_HOUR_SECONDS},
                "secondary_window": {"used_percent": 50, "limit_window_seconds": WEEKLY_SECONDS, "reset_at": WEEKLY_SECONDS, "reset_after_seconds": WEEKLY_SECONDS}
            }
        });
        let mut parsed = snapshot_from_value(&nested_type, 1, 1).unwrap();
        parsed.server_date_at_ms = Some(parsed.observed_at_ms);
        assert_eq!(
            select_target(&parsed),
            ActivationDecision::Blocked("rate_limit_reached")
        );
    }

    #[test]
    fn additional_bucket_is_conservative_and_bootstrap_block_is_preserved() {
        let additional = json!({
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {"used_percent": 0, "limit_window_seconds": FIVE_HOUR_SECONDS, "reset_at": 1_800_018_000, "reset_after_seconds": FIVE_HOUR_SECONDS},
                "secondary_window": {"used_percent": 50, "limit_window_seconds": WEEKLY_SECONDS, "reset_at": 1_800_604_800, "reset_after_seconds": WEEKLY_SECONDS},
                "additional_rate_limits": {"spend_control": {"used_percent": 0, "limit_window_seconds": 3600}}
            }
        });
        let parsed = snapshot_from_value(&additional, 1, 1).unwrap();
        assert!(matches!(
            select_target(&parsed),
            ActivationDecision::Unsupported(_)
        ));

        let mut state = ActivationState::default();
        let blocked = snapshot_at(Some(0.0), Some(100.0), 7_000, 700);
        assert_eq!(
            state.observe(&blocked, 7_000),
            ActivationDecision::Blocked("weekly_exhausted")
        );
        assert_eq!(state, ActivationState::BootstrapPending);
        let allowed = snapshot_at(Some(0.0), Some(50.0), 8_000, 700);
        assert_eq!(
            state.observe(&allowed, 8_000),
            ActivationDecision::Candidate(WindowKind::FiveHour)
        );
        assert!(state.claim("first", 8_001).is_some());
        assert!(state.claim("second", 8_002).is_none());

        let mut awaiting = ActivationState::AwaitingBaseline;
        assert_eq!(
            awaiting.observe(&allowed, 8_000),
            ActivationDecision::Observe
        );
        assert!(awaiting.claim("awaiting", 8_001).is_none());
    }

    #[test]
    fn handoff_decision_vectors_run_through_the_real_parser_and_selector() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/decision_cases.json")).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 27, "fixture case list should remain complete");
        for case in cases {
            let id = case["id"].as_str().unwrap();
            let snapshot =
                snapshot_from_value(&case["raw_response"], 1_799_999_999_000, 1_800_000_000_000);
            let expected_target = case["expected"]["target_by_structure"]
                .as_str()
                .unwrap_or("");
            let expected_decision = case["expected"]["decision"].as_str().unwrap();
            let expected_max_posts = case["expected"]["max_posts_if_all_later_guards_pass"]
                .as_u64()
                .unwrap();
            let Some(mut snapshot) = snapshot else {
                assert_eq!(
                    expected_target, "",
                    "{id}: unparseable case must have no target"
                );
                assert_eq!(expected_max_posts, 0, "{id}");
                continue;
            };
            snapshot.server_date_at_ms = Some(snapshot.observed_at_ms);
            assert_eq!(
                structurally_selected_target(&snapshot).map(WindowKind::as_str),
                (!expected_target.is_empty()).then_some(expected_target),
                "{id}: target structure"
            );
            let decision = select_target(&snapshot);
            match expected_decision {
                "candidate" => assert_eq!(
                    decision,
                    ActivationDecision::Candidate(structurally_selected_target(&snapshot).unwrap()),
                    "{id}"
                ),
                "observe_only" => assert_eq!(decision, ActivationDecision::Observe, "{id}"),
                "blocked" => assert!(
                    matches!(
                        decision,
                        ActivationDecision::Blocked(_) | ActivationDecision::Unsupported(_)
                    ),
                    "{id}: {decision:?}"
                ),
                other => panic!("{id}: unexpected fixture outcome {other}"),
            }
            assert_eq!(
                expected_max_posts,
                if matches!(&decision, ActivationDecision::Candidate(_)) {
                    1
                } else {
                    0
                },
                "{id}: post upper bound"
            );
            if matches!(&decision, ActivationDecision::Candidate(_)) {
                let mut state = ActivationState::default();
                assert!(matches!(
                    state.observe(&snapshot, snapshot.observed_at_ms),
                    ActivationDecision::Candidate(_)
                ));
                assert!(state
                    .claim(format!("{id}-attempt"), snapshot.observed_at_ms)
                    .is_some());
            }
        }
    }
}
