//! Usage Sync wire format (`usage-v1`).
//!
//! Remote layout, relative to `{remote_root}/usage-v1/{profile}/`:
//!
//! ```text
//! devices/{device_id}/manifest.json                     ← written last, per device
//! devices/{device_id}/parts/{partition}-{hash16}.json.gz ← immutable, content-addressed
//! ```
//!
//! Every device writes only below its own `devices/{device_id}/` prefix, so
//! concurrent uploads from different devices never touch the same object.
//! Readers discover devices by listing `devices/` (WebDAV PROPFIND Depth=1 /
//! S3 ListObjectsV2 with delimiter) instead of a shared index file.
//!
//! Partitions:
//! - `d-YYYY-MM-DD` — one local calendar day (source device's timezone) of
//!   detail data: request records (`records` mode) or that day's aggregates
//!   (`daily` mode).
//! - `m-YYYY-MM` — rows of the device's long-term `usage_daily_rollups` for one month.
//!
//! A row lives in exactly one partition of its device, mirroring the local
//! invariant that a request is either a detail row or part of a rollup.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::services::sync_protocol::sha256_hex;

pub(crate) const USAGE_SYNC_FORMAT: &str = "cc-switch-usage-sync";
pub(crate) const USAGE_SEGMENT_FORMAT: &str = "cc-switch-usage-segment";
/// Highest manifest/segment version this build understands.
pub(crate) const USAGE_PROTOCOL_VERSION: u32 = 1;
/// Remote namespace directory, sibling of the configuration snapshot's `v2/`.
pub(crate) const USAGE_NAMESPACE: &str = "usage-v1";
pub(crate) const DEVICES_DIR: &str = "devices";
pub(crate) const MANIFEST_FILE: &str = "manifest.json";
pub(crate) const PARTS_DIR: &str = "parts";

pub(crate) const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_SEGMENT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SEGMENT_DECOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_PARTITIONS: usize = 4096;
/// Manifest sequence numbers are persisted as SQLite integers.
pub(crate) const MAX_MANIFEST_SEQ: u64 = i64::MAX as u64;
pub(crate) const MAX_RETIRED_OBJECTS: usize = 4096;
/// Superseded objects stay readable this long so a reader holding the
/// previous manifest can still finish its download.
pub(crate) const RETIRED_GRACE_SECS: i64 = 24 * 60 * 60;
const MAX_TEXT_FIELD_CHARS: usize = 512;

pub(crate) const DETAIL_LEVEL_RECORDS: &str = "records";
pub(crate) const DETAIL_LEVEL_DAILY: &str = "daily";

// ─── Manifest ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Coverage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_date: Option<String>,
    /// First local date that still has request-level detail on the device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartitionMeta {
    /// Object path relative to the device directory (`parts/...json.gz`).
    pub object: String,
    /// SHA-256 of the stored (compressed) object bytes.
    pub sha256: String,
    pub size: u64,
    /// SHA-256 of the canonical uncompressed JSON; drives change detection.
    pub content_sha256: String,
    #[serde(default)]
    pub logs: u64,
    #[serde(default)]
    pub rollups: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetiredObject {
    pub object: String,
    pub retired_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceManifest {
    pub format: String,
    pub version: u32,
    pub device_id: String,
    pub device_name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub app_version: String,
    /// Monotonic per device; readers ignore manifests older than what they applied.
    pub seq: u64,
    /// Random per write; lets a device notice that another installation
    /// (a cloned profile) wrote to the same device directory.
    pub writer_nonce: String,
    /// Unix seconds of the upload that produced this manifest.
    pub updated_at: i64,
    pub detail_level: String,
    #[serde(default)]
    pub utc_offset_minutes: i32,
    #[serde(default)]
    pub coverage: Coverage,
    pub partitions: BTreeMap<String, PartitionMeta>,
    #[serde(default)]
    pub retired: Vec<RetiredObject>,
    /// Set when this identity was split off a copied installation; the
    /// `inherited` history rows belong to the parent while it is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<Lineage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Lineage {
    pub parent_device_id: String,
    pub forked_at: i64,
    /// Full ancestry, nearest first (starts with `parent_device_id`). Lets
    /// viewers attribute inherited history to the nearest ancestor whose data
    /// they hold, even when intermediate devices are hidden or gone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ancestors: Vec<String>,
}

/// Lineage chains longer than this are rejected (and never produced).
pub(crate) const MAX_LINEAGE_DEPTH: usize = 16;

/// A well-formed ancestry: valid ids, no repeats, not containing `self_id`,
/// bounded depth, starting with `parent` when non-empty.
pub(crate) fn is_valid_ancestry(self_id: &str, parent: &str, ancestors: &[String]) -> bool {
    if ancestors.len() > MAX_LINEAGE_DEPTH {
        return false;
    }
    if let Some(first) = ancestors.first() {
        if first != parent {
            return false;
        }
    }
    let mut seen = std::collections::HashSet::new();
    ancestors
        .iter()
        .all(|id| is_valid_device_id(id) && id != self_id && seen.insert(id.as_str()))
}

// ─── Segment ─────────────────────────────────────────────────

/// One request-level usage fact. Contains no prompt/response content, no
/// session IDs, file paths, error messages or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogRecord {
    /// Stable identity: proxy request UUID or the CLI log's own message key.
    pub request_id: String,
    pub data_source: String,
    pub created_at: i64,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing_model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub input_token_semantics: i64,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub total_cost_usd: String,
    pub cost_multiplier: String,
    pub status_code: i64,
    pub latency_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_token_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    #[serde(default)]
    pub is_streaming: bool,
}

/// One daily aggregate row, shaped like `usage_daily_rollups`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RollupRecord {
    /// Local calendar date on the source device.
    pub date: String,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    #[serde(default)]
    pub request_model: String,
    #[serde(default)]
    pub pricing_model: String,
    pub request_count: i64,
    pub success_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub input_token_semantics: i64,
    pub total_cost_usd: String,
    pub avg_latency_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Segment {
    pub format: String,
    pub version: u32,
    pub device_id: String,
    pub partition: String,
    #[serde(default)]
    pub logs: Vec<LogRecord>,
    #[serde(default)]
    pub rollups: Vec<RollupRecord>,
    // Additions within protocol v1 (older readers ignore unknown fields and
    // keep their previous behaviour). Omitted when empty, so segments without
    // them keep byte-identical canonical JSON and content hashes.
    /// `d-` partitions: requests this device dropped as duplicates of its own
    /// rows (proxy/session pair), i.e. counted through another row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<AliasRecord>,
    /// `m-` partitions: identities of rolled-up requests and what each one
    /// contributed to which rollup group.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<LedgerRecord>,
    /// `m-` partitions: history rows without request ids inherited from the
    /// manifest's `lineage.parentDeviceId`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inherited: Vec<RollupRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AliasRecord {
    pub request_id: String,
    pub data_source: String,
}

/// Ledger kinds: the request is part of a rollup row (0) or was dropped as a
/// duplicate of another row on the same device (1).
pub(crate) const LEDGER_KIND_COUNTED: i64 = 0;
pub(crate) const LEDGER_KIND_ALIAS: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LedgerRecord {
    pub request_id: String,
    pub data_source: String,
    pub kind: i64,
    /// Rollup group: local date + dimensions, exactly as in usage_daily_rollups.
    pub date: String,
    pub app_type: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    #[serde(default)]
    pub request_model: String,
    #[serde(default)]
    pub pricing_model: String,
    pub success: i64,
    /// Fresh-input semantics.
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost_usd: String,
}

impl Segment {
    pub(crate) fn new(device_id: &str, partition: &str) -> Self {
        Self {
            format: USAGE_SEGMENT_FORMAT.to_string(),
            version: USAGE_PROTOCOL_VERSION,
            device_id: device_id.to_string(),
            partition: partition.to_string(),
            logs: Vec::new(),
            rollups: Vec::new(),
            aliases: Vec::new(),
            ledger: Vec::new(),
            inherited: Vec::new(),
        }
    }
}

/// Encoded segment ready for upload.
pub(crate) struct EncodedSegment {
    pub content_sha256: String,
    pub gz_bytes: Vec<u8>,
    pub sha256: String,
}

// ─── Errors ──────────────────────────────────────────────────

fn invalid(detail: impl Into<String>) -> AppError {
    let detail = detail.into();
    AppError::localized(
        "usage_sync.remote_invalid",
        format!("远端用量数据无效: {detail}"),
        format!("Remote usage data is invalid: {detail}"),
    )
}

/// Marker text used to classify "newer protocol" errors without a new AppError variant.
pub(crate) const INCOMPATIBLE_MARKER: &str = "usage_sync.incompatible_version";

pub(crate) fn incompatible(version: u32) -> AppError {
    AppError::localized(
        INCOMPATIBLE_MARKER,
        format!(
            "远端用量数据由更新版本的 CC Switch 生成（协议 v{version}，本地支持 v{USAGE_PROTOCOL_VERSION}），请升级后再同步"
        ),
        format!(
            "Remote usage data was written by a newer CC Switch (protocol v{version}, this build supports v{USAGE_PROTOCOL_VERSION}); update to sync it"
        ),
    )
}

pub(crate) fn is_incompatible_error(error: &AppError) -> bool {
    matches!(error, AppError::Localized { key, .. } if *key == INCOMPATIBLE_MARKER)
}

// ─── Identifiers ─────────────────────────────────────────────

/// Device IDs are generated as lowercase UUIDv4; accept the general
/// `[0-9a-z-]{8,64}` shape so future generators stay readable.
pub(crate) fn is_valid_device_id(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b'-')
}

fn is_digits(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_digit())
}

fn is_valid_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() && value.len() == 10
}

/// `d-YYYY-MM-DD` or `m-YYYY-MM`.
pub(crate) fn is_valid_partition_key(key: &str) -> bool {
    if let Some(date) = key.strip_prefix("d-") {
        return is_valid_date(date);
    }
    if let Some(month) = key.strip_prefix("m-") {
        let mut parts = month.split('-');
        return matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(y), Some(m), None)
                if is_digits(y, 4) && is_digits(m, 2)
                    && (1..=12).contains(&m.parse::<u32>().unwrap_or(0))
        );
    }
    false
}

pub(crate) fn day_partition_key(date: &str) -> String {
    format!("d-{date}")
}

pub(crate) fn month_partition_key(date: &str) -> String {
    format!("m-{}", date.get(..7).unwrap_or(date))
}

pub(crate) fn object_name(partition: &str, content_sha256: &str) -> String {
    format!(
        "{PARTS_DIR}/{partition}-{}.json.gz",
        content_sha256.get(..16).unwrap_or(content_sha256)
    )
}

/// Object paths come from remote manifests and are joined into URLs/keys, so
/// only the exact shape this protocol produces is accepted (no traversal).
pub(crate) fn is_valid_object_name(object: &str, partition: &str) -> bool {
    let Some(rest) = object
        .strip_prefix(PARTS_DIR)
        .and_then(|r| r.strip_prefix('/'))
        .and_then(|r| r.strip_prefix(partition))
        .and_then(|r| r.strip_prefix('-'))
        .and_then(|r| r.strip_suffix(".json.gz"))
    else {
        return false;
    };
    rest.len() == 16 && rest.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Partition key of an object name this protocol produced (`parts/{partition}-{hash16}.json.gz`).
pub(crate) fn partition_of_object(object: &str) -> Option<String> {
    let stem = object
        .strip_prefix(PARTS_DIR)?
        .strip_prefix('/')?
        .strip_suffix(".json.gz")?;
    let partition = stem.get(..stem.len().checked_sub(17)?)?;
    (is_valid_partition_key(partition) && is_valid_object_name(object, partition))
        .then(|| partition.to_string())
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn device_dir(device_id: &str) -> String {
    format!("{DEVICES_DIR}/{device_id}")
}

pub(crate) fn manifest_path(device_id: &str) -> String {
    format!("{}/{MANIFEST_FILE}", device_dir(device_id))
}

pub(crate) fn object_path(device_id: &str, object: &str) -> String {
    format!("{}/{object}", device_dir(device_id))
}

// ─── Validation ──────────────────────────────────────────────

fn check_text(field: &str, value: &str) -> Result<(), AppError> {
    if value.chars().count() > MAX_TEXT_FIELD_CHARS || value.chars().any(char::is_control) {
        return Err(invalid(format!("field {field} is too long or malformed")));
    }
    Ok(())
}

fn check_non_negative(field: &str, value: i64) -> Result<(), AppError> {
    if value < 0 {
        return Err(invalid(format!("field {field} is negative")));
    }
    Ok(())
}

/// Costs are decimal strings as stored by SQLite (`"0.0123"`, `"1.5e-05"`).
fn check_decimal(field: &str, value: &str) -> Result<(), AppError> {
    let parsed = f64::from_str(value.trim())
        .map_err(|_| invalid(format!("field {field} is not a decimal number")))?;
    if !parsed.is_finite() || parsed < 0.0 {
        return Err(invalid(format!("field {field} is negative or not finite")));
    }
    Ok(())
}

/// Validate a downloaded manifest against the directory it was found in.
///
/// Returns an "incompatible" error (see [`is_incompatible_error`]) for newer
/// protocol versions so callers can keep cached data and show a clear status.
pub(crate) fn validate_manifest(
    manifest: &DeviceManifest,
    expected_device_id: &str,
) -> Result<(), AppError> {
    if manifest.format != USAGE_SYNC_FORMAT {
        return Err(invalid(format!("unknown format {}", manifest.format)));
    }
    if manifest.version > USAGE_PROTOCOL_VERSION {
        return Err(incompatible(manifest.version));
    }
    if manifest.version == 0 {
        return Err(invalid("version 0"));
    }
    if manifest.device_id != expected_device_id {
        return Err(invalid("device id does not match its directory"));
    }
    check_text("deviceName", &manifest.device_name)?;
    check_text("platform", &manifest.platform)?;
    check_text("appVersion", &manifest.app_version)?;
    if manifest.detail_level != DETAIL_LEVEL_RECORDS && manifest.detail_level != DETAIL_LEVEL_DAILY
    {
        return Err(invalid(format!(
            "unknown detail level {}",
            manifest.detail_level
        )));
    }
    if manifest.seq > MAX_MANIFEST_SEQ {
        return Err(invalid("sequence number out of range"));
    }
    if let Some(lineage) = &manifest.lineage {
        if !is_valid_device_id(&lineage.parent_device_id)
            || lineage.parent_device_id == manifest.device_id
            || !is_valid_ancestry(
                &manifest.device_id,
                &lineage.parent_device_id,
                &lineage.ancestors,
            )
        {
            return Err(invalid("bad lineage"));
        }
    }
    if manifest.partitions.len() > MAX_PARTITIONS {
        return Err(invalid("too many partitions"));
    }
    for (key, meta) in &manifest.partitions {
        if !is_valid_partition_key(key) {
            return Err(invalid(format!("bad partition key {key}")));
        }
        if !is_valid_object_name(&meta.object, key) {
            return Err(invalid(format!("bad object name for {key}")));
        }
        if !is_sha256_hex(&meta.sha256) || !is_sha256_hex(&meta.content_sha256) {
            return Err(invalid(format!("bad checksum for {key}")));
        }
        if meta.size == 0 || meta.size > MAX_SEGMENT_BYTES as u64 {
            return Err(invalid(format!("bad size for {key}")));
        }
    }
    Ok(())
}

/// Validate a decoded segment against the manifest entry that referenced it.
pub(crate) fn validate_segment(
    segment: &Segment,
    device_id: &str,
    partition: &str,
) -> Result<(), AppError> {
    if segment.format != USAGE_SEGMENT_FORMAT {
        return Err(invalid(format!(
            "unknown segment format {}",
            segment.format
        )));
    }
    if segment.version > USAGE_PROTOCOL_VERSION {
        return Err(incompatible(segment.version));
    }
    if segment.device_id != device_id || segment.partition != partition {
        return Err(invalid(format!("segment {partition} belongs elsewhere")));
    }
    let is_month = partition.starts_with("m-");
    if is_month && !segment.logs.is_empty() {
        return Err(invalid(format!(
            "monthly partition {partition} carries request records"
        )));
    }
    let mut seen = std::collections::HashSet::with_capacity(segment.logs.len());
    for log in &segment.logs {
        if log.request_id.is_empty() || !seen.insert(log.request_id.as_str()) {
            return Err(invalid(format!(
                "duplicate or empty request id in {partition}"
            )));
        }
        for (field, value) in [
            ("requestId", log.request_id.as_str()),
            ("dataSource", log.data_source.as_str()),
            ("appType", log.app_type.as_str()),
            ("providerId", log.provider_id.as_str()),
            ("providerName", log.provider_name.as_str()),
            ("model", log.model.as_str()),
            (
                "requestModel",
                log.request_model.as_deref().unwrap_or_default(),
            ),
            (
                "pricingModel",
                log.pricing_model.as_deref().unwrap_or_default(),
            ),
        ] {
            check_text(field, value)?;
        }
        for (field, value) in [
            ("createdAt", log.created_at),
            ("inputTokens", log.input_tokens),
            ("outputTokens", log.output_tokens),
            ("cacheReadTokens", log.cache_read_tokens),
            ("cacheCreationTokens", log.cache_creation_tokens),
            ("latencyMs", log.latency_ms),
        ] {
            check_non_negative(field, value)?;
        }
        for (field, value) in [
            ("inputCostUsd", log.input_cost_usd.as_str()),
            ("outputCostUsd", log.output_cost_usd.as_str()),
            ("cacheReadCostUsd", log.cache_read_cost_usd.as_str()),
            ("cacheCreationCostUsd", log.cache_creation_cost_usd.as_str()),
            ("totalCostUsd", log.total_cost_usd.as_str()),
            ("costMultiplier", log.cost_multiplier.as_str()),
        ] {
            check_decimal(field, value)?;
        }
    }
    for rollup in &segment.rollups {
        if !is_valid_date(&rollup.date) {
            return Err(invalid(format!("bad rollup date in {partition}")));
        }
        let belongs = if is_month {
            month_partition_key(&rollup.date) == partition
        } else {
            day_partition_key(&rollup.date) == partition
        };
        if !belongs {
            return Err(invalid(format!("rollup date outside {partition}")));
        }
        for (field, value) in [
            ("appType", rollup.app_type.as_str()),
            ("providerId", rollup.provider_id.as_str()),
            ("providerName", rollup.provider_name.as_str()),
            ("model", rollup.model.as_str()),
            ("requestModel", rollup.request_model.as_str()),
            ("pricingModel", rollup.pricing_model.as_str()),
        ] {
            check_text(field, value)?;
        }
        for (field, value) in [
            ("requestCount", rollup.request_count),
            ("successCount", rollup.success_count),
            ("inputTokens", rollup.input_tokens),
            ("outputTokens", rollup.output_tokens),
            ("cacheReadTokens", rollup.cache_read_tokens),
            ("cacheCreationTokens", rollup.cache_creation_tokens),
            ("avgLatencyMs", rollup.avg_latency_ms),
        ] {
            check_non_negative(field, value)?;
        }
        check_decimal("totalCostUsd", &rollup.total_cost_usd)?;
    }
    validate_identity_extensions(segment, partition, is_month, &seen)
}

fn validate_group_row(
    partition: &str,
    date: &str,
    texts: [(&str, &str); 6],
    numbers: &[(&str, i64)],
    cost: &str,
) -> Result<(), AppError> {
    if !is_valid_date(date) || month_partition_key(date) != partition {
        return Err(invalid(format!("history row date outside {partition}")));
    }
    for (field, value) in texts {
        check_text(field, value)?;
    }
    for (field, value) in numbers {
        check_non_negative(field, *value)?;
    }
    check_decimal("totalCostUsd", cost)
}

/// Aliases only in day partitions; ledger and inherited rows only in month
/// partitions; one identity at most once per segment and never also a log.
fn validate_identity_extensions(
    segment: &Segment,
    partition: &str,
    is_month: bool,
    log_ids: &std::collections::HashSet<&str>,
) -> Result<(), AppError> {
    if is_month && !segment.aliases.is_empty() {
        return Err(invalid(format!(
            "monthly partition {partition} carries aliases"
        )));
    }
    if !is_month && (!segment.ledger.is_empty() || !segment.inherited.is_empty()) {
        return Err(invalid(format!(
            "day partition {partition} carries history identities"
        )));
    }
    let mut ids = std::collections::HashSet::new();
    for alias in &segment.aliases {
        check_text("requestId", &alias.request_id)?;
        check_text("dataSource", &alias.data_source)?;
        if alias.request_id.is_empty()
            || log_ids.contains(alias.request_id.as_str())
            || !ids.insert(alias.request_id.as_str())
        {
            return Err(invalid(format!("duplicate alias in {partition}")));
        }
    }
    for entry in &segment.ledger {
        if entry.request_id.is_empty() || !ids.insert(entry.request_id.as_str()) {
            return Err(invalid(format!("duplicate ledger identity in {partition}")));
        }
        if entry.kind != LEDGER_KIND_COUNTED && entry.kind != LEDGER_KIND_ALIAS {
            return Err(invalid(format!("unknown ledger kind in {partition}")));
        }
        check_text("requestId", &entry.request_id)?;
        check_text("dataSource", &entry.data_source)?;
        validate_group_row(
            partition,
            &entry.date,
            [
                ("appType", &entry.app_type),
                ("providerId", &entry.provider_id),
                ("providerName", &entry.provider_name),
                ("model", &entry.model),
                ("requestModel", &entry.request_model),
                ("pricingModel", &entry.pricing_model),
            ],
            &[
                ("success", entry.success),
                ("inputTokens", entry.input_tokens),
                ("outputTokens", entry.output_tokens),
                ("cacheReadTokens", entry.cache_read_tokens),
                ("cacheCreationTokens", entry.cache_creation_tokens),
            ],
            &entry.total_cost_usd,
        )?;
        if entry.success > 1 {
            return Err(invalid(format!(
                "ledger success flag out of range in {partition}"
            )));
        }
    }
    for row in &segment.inherited {
        validate_group_row(
            partition,
            &row.date,
            [
                ("appType", &row.app_type),
                ("providerId", &row.provider_id),
                ("providerName", &row.provider_name),
                ("model", &row.model),
                ("requestModel", &row.request_model),
                ("pricingModel", &row.pricing_model),
            ],
            &[
                ("requestCount", row.request_count),
                ("successCount", row.success_count),
                ("inputTokens", row.input_tokens),
                ("outputTokens", row.output_tokens),
                ("cacheReadTokens", row.cache_read_tokens),
                ("cacheCreationTokens", row.cache_creation_tokens),
            ],
            &row.total_cost_usd,
        )?;
    }
    Ok(())
}

// ─── Encoding ────────────────────────────────────────────────

/// Canonical JSON (struct field order + caller-sorted rows) gzip-compressed.
pub(crate) fn encode_segment(segment: &Segment) -> Result<EncodedSegment, AppError> {
    let json = serde_json::to_vec(segment).map_err(|source| AppError::JsonSerialize { source })?;
    let content_sha256 = sha256_hex(&json);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&json)
        .map_err(|e| AppError::Message(format!("gzip usage segment failed: {e}")))?;
    let gz_bytes = encoder
        .finish()
        .map_err(|e| AppError::Message(format!("gzip usage segment failed: {e}")))?;
    let sha256 = sha256_hex(&gz_bytes);
    Ok(EncodedSegment {
        content_sha256,
        gz_bytes,
        sha256,
    })
}

/// Verify checksum/size, decompress with a hard size cap, then parse.
pub(crate) fn decode_segment(bytes: &[u8], meta: &PartitionMeta) -> Result<Segment, AppError> {
    if bytes.len() as u64 != meta.size {
        return Err(invalid(format!(
            "object {} size mismatch (expected {}, got {})",
            meta.object,
            meta.size,
            bytes.len()
        )));
    }
    if sha256_hex(bytes) != meta.sha256 {
        return Err(invalid(format!("object {} checksum mismatch", meta.object)));
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut json = Vec::new();
    decoder
        .take(MAX_SEGMENT_DECOMPRESSED_BYTES + 1)
        .read_to_end(&mut json)
        .map_err(|e| invalid(format!("object {} is not valid gzip: {e}", meta.object)))?;
    if json.len() as u64 > MAX_SEGMENT_DECOMPRESSED_BYTES {
        return Err(invalid(format!("object {} expands too large", meta.object)));
    }
    if sha256_hex(&json) != meta.content_sha256 {
        return Err(invalid(format!(
            "object {} content checksum mismatch",
            meta.object
        )));
    }
    serde_json::from_slice(&json)
        .map_err(|e| invalid(format!("object {} is not valid JSON: {e}", meta.object)))
}

pub(crate) fn parse_manifest(bytes: &[u8]) -> Result<DeviceManifest, AppError> {
    // Peek at format/version first so a newer, structurally different manifest
    // is reported as "incompatible" rather than "invalid".
    #[derive(Deserialize)]
    struct Header {
        format: Option<String>,
        version: Option<u32>,
    }
    let header: Header =
        serde_json::from_slice(bytes).map_err(|e| invalid(format!("manifest JSON: {e}")))?;
    if header.format.as_deref() == Some(USAGE_SYNC_FORMAT) {
        if let Some(version) = header.version.filter(|v| *v > USAGE_PROTOCOL_VERSION) {
            return Err(incompatible(version));
        }
    }
    serde_json::from_slice(bytes).map_err(|e| invalid(format!("manifest JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_log(id: &str) -> LogRecord {
        LogRecord {
            request_id: id.to_string(),
            data_source: "proxy".into(),
            created_at: 1_760_000_000,
            app_type: "claude".into(),
            provider_id: "p1".into(),
            provider_name: "Provider".into(),
            model: "claude-sonnet".into(),
            request_model: None,
            pricing_model: Some("claude-sonnet".into()),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            input_token_semantics: 2,
            input_cost_usd: "0.001".into(),
            output_cost_usd: "0.002".into(),
            cache_read_cost_usd: "0".into(),
            cache_creation_cost_usd: "0".into(),
            total_cost_usd: "0.003".into(),
            cost_multiplier: "1.0".into(),
            status_code: 200,
            latency_ms: 100,
            first_token_ms: None,
            duration_ms: None,
            is_streaming: false,
        }
    }

    fn sample_manifest(device_id: &str) -> DeviceManifest {
        DeviceManifest {
            format: USAGE_SYNC_FORMAT.into(),
            version: USAGE_PROTOCOL_VERSION,
            device_id: device_id.into(),
            device_name: "Laptop".into(),
            platform: "macos".into(),
            app_version: "4.0.10".into(),
            seq: 1,
            writer_nonce: "n".into(),
            updated_at: 1,
            detail_level: DETAIL_LEVEL_RECORDS.into(),
            utc_offset_minutes: 480,
            coverage: Coverage::default(),
            partitions: BTreeMap::new(),
            retired: Vec::new(),
            lineage: None,
        }
    }

    const DEVICE: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    #[test]
    fn partition_keys_are_strict() {
        assert!(is_valid_partition_key("d-2026-10-13"));
        assert!(is_valid_partition_key("m-2026-01"));
        assert!(!is_valid_partition_key("d-2026-13-01"));
        assert!(!is_valid_partition_key("m-2026-13"));
        assert!(!is_valid_partition_key("m-2026-1"));
        assert!(!is_valid_partition_key("x-2026-10"));
        assert!(!is_valid_partition_key("d-../../etc"));
    }

    #[test]
    fn object_names_reject_traversal() {
        let name = object_name("d-2026-10-13", &"a".repeat(64));
        assert_eq!(name, "parts/d-2026-10-13-aaaaaaaaaaaaaaaa.json.gz");
        assert!(is_valid_object_name(&name, "d-2026-10-13"));
        assert!(!is_valid_object_name(&name, "d-2026-10-12"));
        assert!(!is_valid_object_name(
            "parts/../d-2026-10-13-aaaaaaaaaaaaaaaa.json.gz",
            "d-2026-10-13"
        ));
        assert!(!is_valid_object_name(
            "parts/d-2026-10-13-aaaaaaaaaaaaaaa/.json.gz",
            "d-2026-10-13"
        ));
    }

    #[test]
    fn device_ids_are_restricted() {
        assert!(is_valid_device_id(DEVICE));
        assert!(!is_valid_device_id("ABC-def-123"));
        assert!(!is_valid_device_id("short"));
        assert!(!is_valid_device_id("../../x/yyyyyy"));
    }

    #[test]
    fn segment_roundtrip_verifies_checksums() {
        let mut segment = Segment::new(DEVICE, "d-2026-10-13");
        segment.logs.push(sample_log("a"));
        let encoded = encode_segment(&segment).unwrap();
        let meta = PartitionMeta {
            object: object_name("d-2026-10-13", &encoded.content_sha256),
            sha256: encoded.sha256.clone(),
            size: encoded.gz_bytes.len() as u64,
            content_sha256: encoded.content_sha256.clone(),
            logs: 1,
            rollups: 0,
        };
        let decoded = decode_segment(&encoded.gz_bytes, &meta).unwrap();
        assert_eq!(decoded, segment);

        let mut corrupted = encoded.gz_bytes.clone();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xff;
        assert!(decode_segment(&corrupted, &meta).is_err());
        assert!(decode_segment(&encoded.gz_bytes[..10], &meta).is_err());
    }

    #[test]
    fn encoding_is_deterministic() {
        let mut segment = Segment::new(DEVICE, "d-2026-10-13");
        segment.logs.push(sample_log("a"));
        let a = encode_segment(&segment).unwrap();
        let b = encode_segment(&segment).unwrap();
        assert_eq!(a.content_sha256, b.content_sha256);
        assert_eq!(a.gz_bytes, b.gz_bytes);
    }

    #[test]
    fn validate_segment_rejects_bad_rows() {
        let mut segment = Segment::new(DEVICE, "d-2026-10-13");
        segment.logs.push(sample_log("a"));
        assert!(validate_segment(&segment, DEVICE, "d-2026-10-13").is_ok());
        assert!(validate_segment(&segment, DEVICE, "d-2026-10-12").is_err());

        let mut dup = segment.clone();
        dup.logs.push(sample_log("a"));
        assert!(validate_segment(&dup, DEVICE, "d-2026-10-13").is_err());

        let mut negative = segment.clone();
        negative.logs[0].output_tokens = -1;
        assert!(validate_segment(&negative, DEVICE, "d-2026-10-13").is_err());

        let mut bad_cost = segment.clone();
        bad_cost.logs[0].total_cost_usd = "abc".into();
        assert!(validate_segment(&bad_cost, DEVICE, "d-2026-10-13").is_err());

        let mut month = Segment::new(DEVICE, "m-2026-09");
        month.logs.push(sample_log("b"));
        assert!(validate_segment(&month, DEVICE, "m-2026-09").is_err());
    }

    #[test]
    fn validate_segment_checks_rollup_dates_against_partition() {
        let rollup = RollupRecord {
            date: "2026-09-02".into(),
            app_type: "codex".into(),
            provider_id: "_codex_session".into(),
            provider_name: "Codex (Session)".into(),
            model: "gpt-5".into(),
            request_model: String::new(),
            pricing_model: String::new(),
            request_count: 3,
            success_count: 3,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            input_token_semantics: 2,
            total_cost_usd: "1.5e-3".into(),
            avg_latency_ms: 0,
        };
        let mut month = Segment::new(DEVICE, "m-2026-09");
        month.rollups.push(rollup.clone());
        assert!(validate_segment(&month, DEVICE, "m-2026-09").is_ok());
        let mut wrong = Segment::new(DEVICE, "m-2026-10");
        wrong.rollups.push(rollup);
        assert!(validate_segment(&wrong, DEVICE, "m-2026-10").is_err());
    }

    #[test]
    fn manifest_validation_and_version_gate() {
        let manifest = sample_manifest(DEVICE);
        assert!(validate_manifest(&manifest, DEVICE).is_ok());
        assert!(validate_manifest(&manifest, "1f8fad5b-d9cb-469f-a165-70867728950e").is_err());

        let mut newer = manifest.clone();
        newer.version = USAGE_PROTOCOL_VERSION + 1;
        let err = validate_manifest(&newer, DEVICE).unwrap_err();
        assert!(is_incompatible_error(&err));

        let mut bad_object = manifest.clone();
        bad_object.partitions.insert(
            "d-2026-10-13".into(),
            PartitionMeta {
                object: "../../secrets".into(),
                sha256: "a".repeat(64),
                size: 1,
                content_sha256: "a".repeat(64),
                logs: 0,
                rollups: 0,
            },
        );
        assert!(validate_manifest(&bad_object, DEVICE).is_err());
    }

    #[test]
    fn parse_manifest_reports_newer_versions_as_incompatible() {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "format": USAGE_SYNC_FORMAT,
            "version": 99,
            "somethingNew": true
        }))
        .unwrap();
        let err = parse_manifest(&bytes).unwrap_err();
        assert!(is_incompatible_error(&err));
        assert!(parse_manifest(b"{not json").is_err());
    }

    #[test]
    fn manifest_tolerates_unknown_fields_within_version() {
        let mut value = serde_json::to_value(sample_manifest(DEVICE)).unwrap();
        value["futureField"] = serde_json::json!({"x": 1});
        let bytes = serde_json::to_vec(&value).unwrap();
        let parsed = parse_manifest(&bytes).unwrap();
        assert_eq!(parsed.device_id, DEVICE);
    }
}
