//! Independent audit tests (not part of the candidate). Written against the
//! in-memory remote to probe cross-device aggregate correctness and protocol
//! robustness. Each test states the EXPECTED behaviour; a failure documents a
//! defect, not a broken test.

use std::sync::Arc;

use rusqlite::params;

use super::protocol::{self, manifest_path, DeviceManifest, PartitionMeta};
use super::store;
use super::transport::memory::MemoryStore;
use super::transport::UsageTransport;
use super::{sync_with, SyncOptions, UsageSyncReport};
use crate::database::Database;
use crate::services::usage_stats::UsageDeviceScope;
use crate::settings::UsageSyncDetailLevel;

const NOW: i64 = 1_760_000_000;

struct Device {
    db: Arc<Database>,
    name: &'static str,
    detail_level: UsageSyncDetailLevel,
    now: i64,
}

impl Device {
    fn new(name: &'static str) -> Self {
        Self {
            db: Arc::new(Database::memory().expect("memory db")),
            name,
            detail_level: UsageSyncDetailLevel::Records,
            now: NOW,
        }
    }
    fn id(&self) -> String {
        let conn = self.db.conn.lock().unwrap();
        store::load_or_create_identity(&conn).unwrap().device_id
    }
    fn options(&self) -> SyncOptions {
        SyncOptions {
            detail_level: self.detail_level,
            system_device_name: self.name.to_string(),
            platform: "test".to_string(),
            app_version: "0.0.0-audit".to_string(),
            now: self.now,
        }
    }
    async fn sync(&self, remote: &Arc<MemoryStore>) -> UsageSyncReport {
        let transport = UsageTransport::Memory(remote.clone());
        sync_with(&self.db, &transport, &self.options())
            .await
            .expect("sync runs")
    }
    fn log(&self, request_id: &str, data_source: &str, created_at: i64, cost: &str) {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics,
                total_cost_usd, latency_ms, status_code, created_at, data_source
             ) VALUES (?1, 'prov-1', 'claude', 'm', 100, 10, 0, 0, 2, ?2, 120, 200, ?3, ?4)",
            params![request_id, cost, created_at, data_source],
        )
        .unwrap();
    }
    fn summary(&self, scope: UsageDeviceScope) -> crate::services::usage_stats::UsageSummary {
        self.db
            .get_usage_summary_scoped(None, None, None, None, None, &scope)
            .unwrap()
    }
    fn total(&self, scope: UsageDeviceScope) -> u64 {
        self.summary(scope).total_requests
    }
}

fn remote() -> Arc<MemoryStore> {
    Arc::new(MemoryStore::default())
}

fn read_manifest(remote: &MemoryStore, device_id: &str) -> DeviceManifest {
    let bytes = remote
        .get(&manifest_path(device_id), usize::MAX)
        .unwrap()
        .expect("manifest exists");
    protocol::parse_manifest(&bytes).unwrap()
}

fn clone_db(from: &Device, to: &Device) {
    // Simulate copying the CC Switch data folder: identity + all usage rows + rollups.
    let src = from.db.conn.lock().unwrap();
    let dst = to.db.conn.lock().unwrap();
    let identity = store::load_or_create_identity(&src).unwrap();
    dst.execute("DELETE FROM usage_sync_identity", []).unwrap();
    dst.execute(
        "INSERT INTO usage_sync_identity (id, device_id, created_at, committed_nonce, pending_nonce, manifest_seq)
         VALUES (1, ?1, 0, ?2, ?3, ?4)",
        params![identity.device_id, identity.committed_nonce, identity.pending_nonce, identity.manifest_seq as i64],
    )
    .unwrap();
    let mut stmt = src
        .prepare(
            "SELECT request_id, data_source, created_at, total_cost_usd FROM proxy_request_logs",
        )
        .unwrap();
    let rows: Vec<(String, String, i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for (id, ds, ts, cost) in rows {
        dst.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics,
                total_cost_usd, latency_ms, status_code, created_at, data_source
             ) VALUES (?1, 'prov-1', 'claude', 'm', 100, 10, 0, 0, 2, ?2, 120, 200, ?3, ?4)",
            params![id, cost, ts, ds],
        )
        .unwrap();
    }
    let mut stmt = src
        .prepare(
            "SELECT date, app_type, provider_id, model, request_model, pricing_model, request_count,
                    success_count, input_tokens, output_tokens, cache_read_tokens,
                    cache_creation_tokens, input_token_semantics, total_cost_usd, avg_latency_ms
             FROM usage_daily_rollups",
        )
        .unwrap();
    let rows: Vec<Vec<rusqlite::types::Value>> = stmt
        .query_map([], |r| {
            (0..15)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for row in rows {
        dst.execute(
            "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_model,
                pricing_model, request_count, success_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics, total_cost_usd,
                avg_latency_ms) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            rusqlite::params_from_iter(row.iter()),
        )
        .unwrap();
    }
}

// ─── A. Cross-device aggregate correctness ───────────────────

/// A shared session record (same request_id on both devices, e.g. a cloud-synced
/// ~/.claude) is older than the 30-day detail window on device A, which has
/// already rolled it up, while device B still carries it as a detail row (B
/// was offline / not started for a while). EXPECTED: the "All devices" total
/// still counts the record once on every viewer.
#[tokio::test]
async fn audit_asymmetric_rollup_counts_shared_record_once() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    let old = chrono::Utc::now().timestamp() - 40 * 86_400;
    a.log("session:msg_shared", "session_log", old, "0.05");
    b.log("session:msg_shared", "session_log", old, "0.05");
    // A rolled the day up; B has not (yet).
    assert_eq!(a.db.rollup_and_prune(30).unwrap(), 1);
    assert_eq!(a.total(UsageDeviceScope::Local), 1);
    assert_eq!(b.total(UsageDeviceScope::Local), 1);

    a.sync(&cloud).await;
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    c.sync(&cloud).await;

    let on_a = a.summary(UsageDeviceScope::All);
    let on_b = b.summary(UsageDeviceScope::All);
    let on_c = c.summary(UsageDeviceScope::All);
    eprintln!(
        "AUDIT asymmetric rollup: A.all={} B.all={} C.all={} (cost A={} B={} C={})",
        on_a.total_requests,
        on_b.total_requests,
        on_c.total_requests,
        on_a.total_cost,
        on_b.total_cost,
        on_c.total_cost
    );
    assert_eq!(on_a.total_requests, 1, "viewer A double counts");
    assert_eq!(on_b.total_requests, 1, "viewer B double counts");
    assert_eq!(on_c.total_requests, 1, "third viewer C double counts");
}

/// Same as above but a *proxy* row on A and the matching *session* row on B
/// (A ran the proxy, ~/.claude is synced to B). On A the session duplicate is
/// removed locally; after A rolls up, only B's copy is request-level.
#[tokio::test]
async fn audit_proxy_then_rollup_vs_remote_session_copy() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = chrono::Utc::now().timestamp() - 40 * 86_400;
    a.log("proxy-uuid-1", "proxy", old, "0.05");
    a.log("session:msg_1", "session_log", old + 5, "0.05");
    b.log("session:msg_1", "session_log", old + 5, "0.05");
    assert_eq!(a.total(UsageDeviceScope::Local), 1);
    // Before rollup: local table still holds the session row → remote copy is suppressed.
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total(UsageDeviceScope::All), 1, "pre-rollup");
    // After rollup the session row is pruned from proxy_request_logs.
    a.db.rollup_and_prune(30).unwrap();
    a.sync(&cloud).await;
    let after = a.total(UsageDeviceScope::All);
    eprintln!("AUDIT proxy+rollup vs remote session copy: A.all after rollup = {after}");
    assert_eq!(
        after, 1,
        "A counts the same call twice after its own rollup"
    );
}

/// Developer-admitted case: both devices rolled up. Documented as a limitation;
/// this test just quantifies it.
#[tokio::test]
async fn audit_both_rolled_up_double_counts_documented() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = chrono::Utc::now().timestamp() - 40 * 86_400;
    a.log("session:msg_shared", "session_log", old, "0.05");
    b.log("session:msg_shared", "session_log", old, "0.05");
    a.db.rollup_and_prune(30).unwrap();
    b.db.rollup_and_prune(30).unwrap();
    a.sync(&cloud).await;
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let all = a.total(UsageDeviceScope::All);
    eprintln!("AUDIT both rolled up: A.all = {all} (documented limitation)");
    // R1 corrective change (the only edit to the independent audit tests):
    // this assertion encoded the audited limitation (2). Rollups now keep a
    // request-identity ledger, so both rolled-up copies are de-duplicated and
    // the correct total is 1. The remaining id-less case (rollups made before
    // the ledger existed) is pinned by
    // `tests::legacy_rollups_without_ledger_stay_per_device_and_are_reported`.
    assert_eq!(all, 1, "both rolled-up copies carry ledger identities");
}

/// A copied data folder resolved with "Use a new identity" (the recommended
/// resolution). Both installations then publish the same pre-copy history:
/// request-level rows de-duplicate by id, but the rolled-up history does not.
/// EXPECTED (for a correct total): history counted once.
#[tokio::test]
async fn audit_new_identity_after_clone_double_counts_rolled_up_history() {
    let cloud = remote();
    let a = Device::new("Original");
    let old = chrono::Utc::now().timestamp() - 40 * 86_400;
    for i in 0..5 {
        a.log(&format!("old-{i}"), "proxy", old + i, "0.01");
    }
    a.log(
        "recent",
        "proxy",
        chrono::Utc::now().timestamp() - 3600,
        "0.01",
    );
    assert_eq!(a.db.rollup_and_prune(30).unwrap(), 5);
    a.sync(&cloud).await;

    let clone = Device::new("Clone");
    clone_db(&a, &clone);
    // Original keeps writing → clone detects the conflict → "Use a new identity".
    a.log(
        "a-after-copy",
        "proxy",
        chrono::Utc::now().timestamp() - 1800,
        "0.01",
    );
    a.sync(&cloud).await;
    let report = clone.sync(&cloud).await;
    assert!(report.identity_conflict);
    {
        let conn = clone.db.conn.lock().unwrap();
        store::reset_identity(&conn).unwrap();
    }
    clone.sync(&cloud).await;
    a.sync(&cloud).await;

    let on_a = a.summary(UsageDeviceScope::All);
    let on_clone = clone.summary(UsageDeviceScope::All);
    eprintln!(
        "AUDIT clone+new identity: A.all={} (local={}), Clone.all={} (local={})",
        on_a.total_requests,
        a.total(UsageDeviceScope::Local),
        on_clone.total_requests,
        clone.total(UsageDeviceScope::Local)
    );
    // 5 rolled-up + "recent" + "a-after-copy" = 7 distinct requests.
    assert_eq!(on_a.total_requests, 7, "A: shared history counted twice");
    assert_eq!(
        on_clone.total_requests, 7,
        "Clone: shared history counted twice"
    );
}

/// Hidden device: its rows are dropped from "All" and stay dropped even when a
/// later sync hits an error for that device (status must stay hidden).
#[tokio::test]
async fn audit_hidden_device_stays_hidden_through_errors() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "0.01");
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    {
        let conn = a.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), true).unwrap();
    }
    // Corrupt B's manifest so a non-hidden device would record an error.
    cloud
        .put(&manifest_path(&b.id()), b"{garbage".to_vec())
        .unwrap();
    let report = a.sync(&cloud).await;
    assert!(
        report.device_errors.is_empty(),
        "hidden devices are skipped"
    );
    assert_eq!(a.total(UsageDeviceScope::All), 0);
    let conn = a.db.conn.lock().unwrap();
    let status: String = conn
        .query_row(
            "SELECT status FROM usage_sync_devices WHERE device_id = ?1",
            [b.id()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, store::DEVICE_STATUS_HIDDEN);
}

/// Request detail in the "all" scope resolves a remote-only id and a local id.
#[tokio::test]
async fn audit_request_detail_all_scope() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    a.log("local-1", "proxy", NOW - 60, "0.01");
    b.log("remote-1", "proxy", NOW - 60, "0.01");
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let local =
        a.db.get_request_detail_scoped("local-1", Some("all"))
            .unwrap()
            .unwrap();
    assert!(local.device_id.is_none());
    let remote =
        a.db.get_request_detail_scoped("remote-1", Some("all"))
            .unwrap()
            .unwrap();
    assert_eq!(remote.device_id.as_deref(), Some(b.id().as_str()));
    let by_device =
        a.db.get_request_detail_scoped("remote-1", Some(&b.id()))
            .unwrap()
            .unwrap();
    assert_eq!(by_device.device_id.as_deref(), Some(b.id().as_str()));
    // Local scope must not see remote rows.
    assert!(a
        .db
        .get_request_detail_scoped("remote-1", None)
        .unwrap()
        .is_none());
}

// ─── B. Protocol robustness ──────────────────────────────────

/// A gzip bomb referenced by a (self-written or tampered) manifest must be
/// rejected at the decompression cap, not parsed.
#[test]
fn audit_gzip_bomb_is_capped() {
    use std::io::Write;
    let size = protocol::MAX_SEGMENT_DECOMPRESSED_BYTES as usize + 1024;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    let chunk = vec![b' '; 1 << 20];
    let mut written = 0;
    while written < size {
        enc.write_all(&chunk).unwrap();
        written += chunk.len();
    }
    let gz = enc.finish().unwrap();
    eprintln!(
        "AUDIT gzip bomb: compressed {} bytes → {} bytes",
        gz.len(),
        written
    );
    assert!(gz.len() < protocol::MAX_SEGMENT_BYTES);
    let meta = PartitionMeta {
        object: "parts/d-2026-10-13-0000000000000000.json.gz".into(),
        sha256: crate::services::sync_protocol::sha256_hex(&gz),
        size: gz.len() as u64,
        content_sha256: "0".repeat(64),
        logs: 0,
        rollups: 0,
    };
    let err = protocol::decode_segment(&gz, &meta)
        .unwrap_err()
        .to_string();
    assert!(err.contains("expands too large"), "{err}");
}

/// Numeric edge cases in a segment: non-finite cost strings, float token
/// counts, negative counts.
#[test]
fn audit_segment_numeric_edge_cases() {
    const DEV: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let base = serde_json::json!({
        "format": protocol::USAGE_SEGMENT_FORMAT, "version": 1, "deviceId": DEV,
        "partition": "d-2026-10-13",
        "logs": [{
            "requestId": "r", "dataSource": "proxy", "createdAt": 1, "appType": "claude",
            "providerId": "p", "providerName": "P", "model": "m",
            "inputTokens": 1, "outputTokens": 1, "cacheReadTokens": 0, "cacheCreationTokens": 0,
            "inputTokenSemantics": 2, "inputCostUsd": "0", "outputCostUsd": "0",
            "cacheReadCostUsd": "0", "cacheCreationCostUsd": "0", "totalCostUsd": "0",
            "costMultiplier": "1", "statusCode": 200, "latencyMs": 1
        }]
    });
    let ok: protocol::Segment = serde_json::from_value(base.clone()).unwrap();
    assert!(protocol::validate_segment(&ok, DEV, "d-2026-10-13").is_ok());

    for (field, value) in [
        ("totalCostUsd", serde_json::json!("inf")),
        ("totalCostUsd", serde_json::json!("1e400")),
        ("totalCostUsd", serde_json::json!("NaN")),
        ("costMultiplier", serde_json::json!("-1")),
    ] {
        let mut v = base.clone();
        v["logs"][0][field] = value.clone();
        let seg: protocol::Segment = serde_json::from_value(v).unwrap();
        assert!(
            protocol::validate_segment(&seg, DEV, "d-2026-10-13").is_err(),
            "{field}={value} accepted"
        );
    }
    for (field, value) in [
        ("inputTokens", serde_json::json!(1.5)),
        ("inputTokens", serde_json::json!(1e30)),
        ("statusCode", serde_json::json!("200")),
    ] {
        let mut v = base.clone();
        v["logs"][0][field] = value.clone();
        assert!(
            serde_json::from_value::<protocol::Segment>(v).is_err(),
            "{field}={value} deserialised"
        );
    }
    // Control characters in text fields are rejected.
    let mut v = base.clone();
    v["logs"][0]["model"] = serde_json::json!("m\u{0}x");
    let seg: protocol::Segment = serde_json::from_value(v).unwrap();
    assert!(protocol::validate_segment(&seg, DEV, "d-2026-10-13").is_err());
}

/// Our own manifest with seq = u64::MAX (tampered or corrupted storage, nonce
/// still ours). The next upload computes `seq + 1`. EXPECTED: an error, not a panic.
#[tokio::test]
async fn audit_own_manifest_with_max_seq_does_not_panic() {
    let cloud = remote();
    let a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "0.01");
    a.sync(&cloud).await;
    let mut m = read_manifest(&cloud, &a.id());
    m.seq = u64::MAX;
    cloud
        .put(&manifest_path(&a.id()), serde_json::to_vec(&m).unwrap())
        .unwrap();
    a.log("a2", "proxy", NOW - 30, "0.01");
    let result = std::panic::AssertUnwindSafe(a.sync(&cloud));
    let outcome = futures::FutureExt::catch_unwind(result).await;
    match outcome {
        Ok(report) => eprintln!("AUDIT max seq: no panic, report={report:?}"),
        Err(_) => panic!("AUDIT max seq: sync panicked on seq overflow"),
    }
}

/// Concurrent writers with the same identity (two live copies) racing within
/// one sync window: both read the known nonce, both write. The loser's objects
/// are never referenced again. EXPECTED: conflict surfaces on the next round.
#[tokio::test]
async fn audit_same_identity_race_is_detected_next_round() {
    let cloud = remote();
    let a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "0.01");
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    clone_db(&a, &clone);
    // Race: both see the same manifest; A writes first, clone writes second.
    a.log("a2", "proxy", NOW - 30, "0.01");
    clone.log("c2", "proxy", NOW - 30, "0.01");
    let r1 = a.sync(&cloud).await;
    let r2 = clone.sync(&cloud).await; // sees A's nonce → conflict (sequential case)
    eprintln!(
        "AUDIT race: a={:?} clone_conflict={}",
        r1.upload_error, r2.identity_conflict
    );
    assert!(r2.identity_conflict);
    // A's next sync also conflicts? (Clone did not write, so no.)
    let r3 = a.sync(&cloud).await;
    assert!(!r3.identity_conflict);
}

/// Day-level remote data with a partial day: a daily-only device's today
/// partition must not be counted in a "today" range (same rule as local rollups).
#[tokio::test]
async fn audit_daily_only_device_partial_day_handling() {
    let cloud = remote();
    let a = Device::new("A");
    let mut b = Device::new("B");
    b.detail_level = UsageSyncDetailLevel::Daily;
    let now = chrono::Utc::now().timestamp();
    b.log("today-1", "proxy", now - 60, "0.10");
    b.log("old-1", "proxy", now - 3 * 86_400, "0.10");
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let all_time = a.summary(UsageDeviceScope::Device(b.id()));
    let last_24h =
        a.db.get_usage_summary_scoped(
            Some(now - 86_400),
            Some(now),
            None,
            None,
            None,
            &UsageDeviceScope::Device(b.id()),
        )
        .unwrap();
    let last_7d =
        a.db.get_usage_summary_scoped(
            Some(now - 7 * 86_400),
            Some(now),
            None,
            None,
            None,
            &UsageDeviceScope::Device(b.id()),
        )
        .unwrap();
    eprintln!(
        "AUDIT daily-only: all_time={} last24h={} last7d={}",
        all_time.total_requests, last_24h.total_requests, last_7d.total_requests
    );
    assert_eq!(all_time.total_requests, 2);
}

/// Dump exactly what leaves the device in `records` mode for realistic rows
/// (Claude session, Gemini session, proxy) so the privacy review is based on
/// bytes, not on field names.
#[tokio::test]
async fn audit_dump_uploaded_bytes() {
    let cloud = remote();
    let b = Device::new("Johns-MacBook-Pro");
    {
        let conn = b.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model, input_tokens,
                output_tokens, total_cost_usd, latency_ms, status_code, created_at, data_source,
                error_message, session_id, request_model, pricing_model)
             VALUES ('session:msg_01HXYZabc', '_session', 'claude', 'claude-sonnet-4-5', 10, 5, '0.01', 1, 200, ?1,
                     'session_log', NULL, 'c0ffee00-1111-2222-3333-444444444444', NULL, 'claude-sonnet-4-5'),
                    ('gemini_session:9f1e7c3a-aaaa-bbbb-cccc-dddddddddddd:7', '_gemini_session', 'gemini', 'gemini-2.5-pro',
                     10, 5, '0.01', 1, 200, ?1, 'gemini_session', NULL, NULL, NULL, NULL),
                    ('3b1b6d1e-uuid-proxy', 'my-provider', 'codex', 'gpt-5', 10, 5, '0.01', 1, 500, ?1, 'proxy',
                     'upstream said: invalid api key sk-live-XXXX', 'sess-9', 'gpt-5-alias', 'gpt-5')",
            [NOW - 60],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('my-provider', 'codex', 'Home NAS /Users/john/keys', '{}')",
            [],
        )
        .ok();
    }
    b.sync(&cloud).await;
    for key in cloud.keys() {
        let bytes = cloud.get(&key, usize::MAX).unwrap().unwrap();
        let text = if key.ends_with(".gz") {
            use std::io::Read;
            let mut out = String::new();
            flate2::read::GzDecoder::new(&bytes[..])
                .read_to_string(&mut out)
                .unwrap();
            out
        } else {
            String::from_utf8(bytes).unwrap()
        };
        eprintln!("AUDIT REMOTE OBJECT {key}\n{text}\n");
    }
}
