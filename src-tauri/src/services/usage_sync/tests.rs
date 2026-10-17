//! Multi-device scenarios against the in-memory remote.
//!
//! Each `Device` owns an in-memory database (its "installation") and shares
//! one `MemoryStore` (the cloud). Scenarios cover idempotence, discovery,
//! concurrent uploads, interruption/corruption recovery, cross-device
//! de-duplication, rollup boundaries, cost corrections, identity conflicts
//! and version gating.

use std::sync::Arc;

use rusqlite::params;
// Tests that import a database write a safety backup under the app home;
// they run serialized with the backup tests that swap that home (#[serial]).
use serial_test::serial;

use super::protocol::{self, manifest_path, DeviceManifest};
use super::store;
use super::transport::memory::MemoryStore;
use super::transport::UsageTransport;
use super::{sync_with, SyncOptions, UsageSyncReport, ORPHAN_SWEEP_INTERVAL_SECS};
use crate::database::Database;
use crate::error::AppError;
use crate::services::usage_stats::UsageDeviceScope;
use crate::settings::UsageSyncDetailLevel;

const NOW: i64 = 1_760_000_000; // 2025-10-09T08:53:20Z

struct Device {
    db: Arc<Database>,
    name: &'static str,
    detail_level: UsageSyncDetailLevel,
    now: i64,
}

impl Device {
    fn new(name: &'static str) -> Self {
        let db = Arc::new(Database::memory().expect("memory db"));
        // Setup (R4): these devices have usage sync enabled; the identity
        // ledger is only written once a device enables it.
        db.set_usage_sync_enabled_for_test(true);
        Self {
            db,
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
            app_version: "0.0.0-test".to_string(),
            now: self.now,
        }
    }

    async fn sync(&self, remote: &Arc<MemoryStore>) -> UsageSyncReport {
        let transport = UsageTransport::Memory(remote.clone());
        sync_with(&self.db, &transport, &self.options())
            .await
            .expect("sync runs")
    }

    #[allow(clippy::too_many_arguments)]
    fn log(
        &self,
        request_id: &str,
        data_source: &str,
        created_at: i64,
        model: &str,
        input: i64,
        output: i64,
        cost: &str,
        status: i64,
    ) {
        let conn = self.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics,
                total_cost_usd, latency_ms, status_code, created_at, data_source
             ) VALUES (?1, 'prov-1', 'claude', ?2, ?3, ?4, 0, 0, 2, ?5, 120, ?6, ?7, ?8)",
            params![
                request_id,
                model,
                input,
                output,
                cost,
                status,
                created_at,
                data_source
            ],
        )
        .unwrap();
    }

    fn summary(&self, scope: UsageDeviceScope) -> crate::services::usage_stats::UsageSummary {
        self.db
            .get_usage_summary_scoped(None, None, None, None, None, &scope)
            .unwrap()
    }

    fn total_requests(&self, scope: UsageDeviceScope) -> u64 {
        self.summary(scope).total_requests
    }

    fn id_unlocked(&self, conn: &rusqlite::Connection) -> String {
        store::load_or_create_identity(conn).unwrap().device_id
    }

    fn remote_device(&self, device_id: &str) -> Option<store::RemoteDeviceInfo> {
        let conn = self.db.conn.lock().unwrap();
        store::list_remote_devices(&conn)
            .unwrap()
            .into_iter()
            .find(|d| d.device_id == device_id)
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

// ─── Single device ───────────────────────────────────────────

#[tokio::test]
async fn first_sync_publishes_manifest_and_parts() {
    let cloud = remote();
    let a = Device::new("Laptop");
    a.log(
        "r1",
        "proxy",
        NOW - 3600,
        "claude-sonnet",
        100,
        50,
        "0.01",
        200,
    );
    a.log(
        "r2",
        "proxy",
        NOW - 86400 * 2,
        "claude-sonnet",
        10,
        5,
        "0.001",
        500,
    );

    let report = a.sync(&cloud).await;
    assert!(report.upload_error.is_none(), "{report:?}");
    assert!(report.manifest_written);
    assert_eq!(report.uploaded_partitions, 2);

    let manifest = read_manifest(&cloud, &a.id());
    assert_eq!(manifest.device_name, "Laptop");
    assert_eq!(manifest.seq, 1);
    assert_eq!(manifest.partitions.len(), 2);
    for meta in manifest.partitions.values() {
        assert!(cloud
            .keys()
            .contains(&protocol::object_path(&a.id(), &meta.object)));
    }
}

#[tokio::test]
async fn repeated_sync_is_idempotent_and_cheap() {
    let cloud = remote();
    let a = Device::new("Laptop");
    a.log("r1", "proxy", NOW - 3600, "m", 100, 50, "0.01", 200);
    a.sync(&cloud).await;
    let puts_after_first = *cloud.put_count.lock().unwrap();

    let report = a.sync(&cloud).await;
    assert_eq!(report.uploaded_partitions, 0);
    assert!(
        !report.manifest_written,
        "unchanged data must not rewrite the manifest"
    );
    assert_eq!(*cloud.put_count.lock().unwrap(), puts_after_first);
    assert_eq!(read_manifest(&cloud, &a.id()).seq, 1);
}

#[tokio::test]
async fn heartbeat_rewrites_unchanged_manifest_after_an_hour() {
    let cloud = remote();
    let mut a = Device::new("Laptop");
    a.log("r1", "proxy", NOW - 3600, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    a.now += super::HEARTBEAT_SECS;
    let report = a.sync(&cloud).await;
    assert!(report.manifest_written);
    assert_eq!(report.uploaded_partitions, 0);
    assert_eq!(read_manifest(&cloud, &a.id()).updated_at, a.now);
}

#[tokio::test]
async fn local_scope_is_unchanged_by_remote_data() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    a.log("a1", "proxy", NOW - 100, "m", 10, 10, "0.10", 200);
    b.log("b1", "proxy", NOW - 100, "m", 20, 20, "0.20", 200);
    let before = a.summary(UsageDeviceScope::Local);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let after = a.summary(UsageDeviceScope::Local);
    assert_eq!(before.total_requests, after.total_requests);
    assert_eq!(before.total_cost, after.total_cost);
    // The legacy entry point is the local scope.
    let legacy =
        a.db.get_usage_summary(None, None, None, None, None)
            .unwrap();
    assert_eq!(legacy.total_requests, after.total_requests);
    assert_eq!(legacy.total_cost, after.total_cost);
}

// ─── Multi device ────────────────────────────────────────────

#[tokio::test]
async fn two_devices_aggregate_and_filter() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    a.log("a1", "proxy", NOW - 100, "model-x", 10, 10, "0.10", 200);
    a.log("a2", "proxy", NOW - 200, "model-x", 10, 10, "0.10", 200);
    b.log("b1", "proxy", NOW - 100, "model-y", 20, 20, "0.25", 200);

    a.sync(&cloud).await;
    b.sync(&cloud).await;
    a.sync(&cloud).await;

    assert_eq!(a.total_requests(UsageDeviceScope::Local), 2);
    assert_eq!(a.total_requests(UsageDeviceScope::All), 3);
    assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 1);
    // Selecting this device's own id is the local scope.
    assert_eq!(a.total_requests(UsageDeviceScope::Device(a.id())), 2);

    let all = a.summary(UsageDeviceScope::All);
    assert_eq!(all.total_cost, "0.450000");
    assert_eq!(all.total_input_tokens, 40);

    // B sees the mirror image.
    assert_eq!(b.total_requests(UsageDeviceScope::All), 3);

    // Model filter composes with device scope.
    let model_y =
        a.db.get_usage_summary_scoped(
            None,
            None,
            None,
            None,
            Some("model-y"),
            &UsageDeviceScope::All,
        )
        .unwrap();
    assert_eq!(model_y.total_requests, 1);
    let models =
        a.db.get_model_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
    assert_eq!(models.len(), 2);

    let info = a.remote_device(&b.id()).expect("B is known on A");
    assert_eq!(info.device_name, "B");
    assert_eq!(info.status, store::DEVICE_STATUS_OK);
    assert_eq!(info.log_rows, 1);
}

#[tokio::test]
async fn provider_stats_use_the_name_recorded_by_the_source_device() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    {
        let conn = b.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, app_type, name, settings_config) VALUES ('prov-1', 'claude', 'Remote Name', '{}')",
            [],
        )
        .unwrap();
    }
    b.log("b1", "proxy", NOW - 100, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let stats =
        a.db.get_provider_stats_scoped(
            None,
            None,
            None,
            None,
            None,
            &UsageDeviceScope::Device(b.id()),
        )
        .unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].provider_name, "Remote Name");
    // Filtering by that name works across scopes.
    let filtered =
        a.db.get_usage_summary_scoped(
            None,
            None,
            None,
            Some("Remote Name"),
            None,
            &UsageDeviceScope::All,
        )
        .unwrap();
    assert_eq!(filtered.total_requests, 1);
}

#[tokio::test]
async fn devices_are_discovered_without_a_shared_index() {
    let cloud = remote();
    let devices = [Device::new("A"), Device::new("B"), Device::new("C")];
    for (i, device) in devices.iter().enumerate() {
        device.log(&format!("r{i}"), "proxy", NOW - 10, "m", 1, 1, "0.01", 200);
        device.sync(&cloud).await;
    }
    let report = devices[0].sync(&cloud).await;
    assert_eq!(report.devices_seen, 2);
    assert_eq!(devices[0].total_requests(UsageDeviceScope::All), 3);
    assert!(
        !cloud.keys().iter().any(|k| !k.starts_with("devices/")),
        "no shared files outside per-device directories"
    );
}

#[tokio::test]
async fn concurrent_uploads_do_not_interfere() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    for i in 0..20 {
        a.log(
            &format!("a{i}"),
            "proxy",
            NOW - i * 60,
            "m",
            1,
            1,
            "0.01",
            200,
        );
        b.log(
            &format!("b{i}"),
            "proxy",
            NOW - i * 60,
            "m",
            1,
            1,
            "0.01",
            200,
        );
    }
    let (ra, rb) = tokio::join!(a.sync(&cloud), b.sync(&cloud));
    assert!(ra.upload_error.is_none() && rb.upload_error.is_none());
    a.sync(&cloud).await;
    b.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 40);
    assert_eq!(b.total_requests(UsageDeviceScope::All), 40);
}

#[tokio::test]
async fn offline_device_catches_up_without_losing_history() {
    let cloud = remote();
    let a = Device::new("A");
    let mut b = Device::new("B");
    b.log("b1", "proxy", NOW - 86400 * 3, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;

    // B goes offline: every upload fails while it keeps recording usage.
    *cloud.fail_put_containing.lock().unwrap() = Some(b.id());
    *cloud.fail_list.lock().unwrap() = true;
    for day in 0..3 {
        b.now = NOW + day * 86400;
        b.log(
            &format!("b-off-{day}"),
            "proxy",
            b.now - 60,
            "m",
            1,
            1,
            "0.01",
            200,
        );
        let report = b.sync(&cloud).await;
        assert!(report.upload_error.is_some());
        assert!(report.download_error.is_some());
    }

    *cloud.fail_put_containing.lock().unwrap() = None;
    *cloud.fail_list.lock().unwrap() = false;
    let report = b.sync(&cloud).await;
    assert!(report.upload_error.is_none(), "{report:?}");
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 4);
}

#[tokio::test]
async fn interrupted_upload_keeps_previous_generation_readable() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);

    // New data; the manifest PUT fails after the segment was uploaded.
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    *cloud.fail_put_containing.lock().unwrap() = Some("manifest.json".into());
    let report = b.sync(&cloud).await;
    assert!(report.upload_error.is_some());
    a.sync(&cloud).await;
    assert_eq!(
        a.total_requests(UsageDeviceScope::All),
        1,
        "old manifest still consistent"
    );

    // A crash after the manifest PUT but before the nonce commit must not be
    // mistaken for a foreign writer: the pending nonce is accepted.
    *cloud.fail_put_containing.lock().unwrap() = None;
    let report = b.sync(&cloud).await;
    assert!(report.upload_error.is_none(), "{report:?}");
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
}

#[tokio::test]
async fn pending_nonce_survives_crash_between_manifest_and_commit() {
    let cloud = remote();
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    // Simulate: manifest written with nonce N, crash before commit_nonce.
    let mut manifest = read_manifest(&cloud, &b.id());
    manifest.writer_nonce = "crash-nonce".into();
    manifest.seq += 1;
    cloud
        .put(
            &manifest_path(&b.id()),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    {
        let conn = b.db.conn.lock().unwrap();
        store::set_pending_nonce(&conn, "crash-nonce").unwrap();
    }
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    let report = b.sync(&cloud).await;
    assert!(!report.identity_conflict, "{report:?}");
    assert!(report.upload_error.is_none());
}

#[tokio::test]
async fn corrupt_or_missing_segment_is_rejected_and_cache_is_kept() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);

    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let manifest = read_manifest(&cloud, &b.id());
    let object = protocol::object_path(
        &b.id(),
        &manifest.partitions.values().next().unwrap().object,
    );

    // Corrupt the new object.
    let original = cloud.get(&object, usize::MAX).unwrap().unwrap();
    let mut corrupted = original.clone();
    let mid = corrupted.len() / 2;
    corrupted[mid] ^= 0x55;
    cloud.put(&object, corrupted).unwrap();
    let report = a.sync(&cloud).await;
    assert_eq!(report.device_errors.len(), 1);
    assert_eq!(
        a.total_requests(UsageDeviceScope::All),
        1,
        "previous data retained"
    );
    let info = a.remote_device(&b.id()).unwrap();
    assert_eq!(info.status, store::DEVICE_STATUS_ERROR);
    assert!(info.last_error.is_some());

    // Missing object.
    cloud.delete(&object).unwrap();
    let report = a.sync(&cloud).await;
    assert_eq!(report.device_errors.len(), 1);
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);

    // Repaired: the next sync recovers.
    cloud.put(&object, original).unwrap();
    let report = a.sync(&cloud).await;
    assert!(report.device_errors.is_empty(), "{report:?}");
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
    assert_eq!(
        a.remote_device(&b.id()).unwrap().status,
        store::DEVICE_STATUS_OK
    );
}

#[tokio::test]
async fn transport_failure_for_one_device_does_not_block_others() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    c.log("c1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    c.sync(&cloud).await;
    *cloud.fail_get_containing.lock().unwrap() = Some(format!("devices/{}/", b.id()));
    let report = a.sync(&cloud).await;
    assert_eq!(report.device_errors.len(), 1);
    assert_eq!(a.total_requests(UsageDeviceScope::Device(c.id())), 1);
}

// ─── De-duplication ──────────────────────────────────────────

#[tokio::test]
async fn same_session_record_on_two_devices_counts_once() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    // A synced home directory imported the same Claude message on all devices.
    for device in [&a, &b, &c] {
        device.log(
            "session:msg_shared",
            "session_log",
            NOW - 60,
            "m",
            100,
            10,
            "0.05",
            200,
        );
    }
    b.log("b-only", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    for device in [&a, &b, &c, &a] {
        device.sync(&cloud).await;
    }
    // Local copy wins over both remote copies; B's own record still counts.
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
    let all = a.summary(UsageDeviceScope::All);
    assert_eq!(all.total_cost, "0.060000");
    // Per-device views show each device's own data in full.
    assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 2);
    assert_eq!(a.total_requests(UsageDeviceScope::Device(c.id())), 1);

    // A viewer without the record keeps exactly one remote copy.
    let d = Device::new("D");
    d.sync(&cloud).await;
    assert_eq!(d.total_requests(UsageDeviceScope::All), 2);
    let logs =
        d.db.get_request_logs(
            &crate::services::usage_stats::LogFilters {
                device: Some("all".into()),
                ..Default::default()
            },
            0,
            50,
        )
        .unwrap();
    assert_eq!(logs.total, 2);
    let shared = logs
        .data
        .iter()
        .find(|l| l.request_id == "session:msg_shared")
        .unwrap();
    // D sees three copies (A, B and C all published it): the smallest id wins.
    let expected_owner = [a.id(), b.id(), c.id()].into_iter().min().unwrap();
    assert_eq!(shared.device_id.as_deref(), Some(expected_owner.as_str()));
}

#[tokio::test]
async fn proxy_and_session_rows_on_different_devices_are_not_merged_heuristically() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    // Same tokens and time, but different identities: only provable identity
    // may merge rows across devices, so both count.
    a.log("proxy-uuid-1", "proxy", NOW - 60, "m", 100, 10, "0.05", 200);
    b.log(
        "session:msg_x",
        "session_log",
        NOW - 60,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
}

#[tokio::test]
async fn source_device_dedup_is_applied_before_upload() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    // On B the proxy row and the session row are the same request (local heuristic).
    b.log("proxy-1", "proxy", NOW - 60, "m", 100, 10, "0.05", 200);
    b.log(
        "session:msg_1",
        "session_log",
        NOW - 50,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    assert_eq!(b.total_requests(UsageDeviceScope::Local), 1);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 1);
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);
}

// ─── Rollups, corrections, privacy level ─────────────────────

#[tokio::test]
async fn rollup_transition_keeps_full_day_totals() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = chrono::Utc::now().timestamp() - 40 * 86400;
    for i in 0..5 {
        b.log(
            &format!("old-{i}"),
            "proxy",
            old + i,
            "m",
            100,
            50,
            "0.01",
            200,
        );
    }
    b.log(
        "recent",
        "proxy",
        chrono::Utc::now().timestamp() - 3600,
        "m",
        1,
        1,
        "0.01",
        200,
    );
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let before = a.summary(UsageDeviceScope::Device(b.id()));
    assert_eq!(before.total_requests, 6);

    // B rolls up and prunes its 40-day-old details; the d- partition becomes an m- partition.
    assert_eq!(b.db.rollup_and_prune(30).unwrap(), 5);
    let report = b.sync(&cloud).await;
    assert!(report.manifest_written);
    let manifest = read_manifest(&cloud, &b.id());
    assert!(manifest.partitions.keys().any(|k| k.starts_with("m-")));
    a.sync(&cloud).await;

    let after = a.summary(UsageDeviceScope::Device(b.id()));
    assert_eq!(
        after.total_requests, before.total_requests,
        "no double count, no loss"
    );
    assert_eq!(after.total_cost, before.total_cost);
    assert_eq!(after.total_input_tokens, before.total_input_tokens);
    let info = a.remote_device(&b.id()).unwrap();
    assert_eq!(info.rollup_rows, 1);
    assert_eq!(info.log_rows, 1);

    // The superseded detail object is retired, not deleted immediately.
    assert!(!manifest.retired.is_empty());
}

#[tokio::test]
async fn retired_objects_are_garbage_collected_after_grace_period() {
    let cloud = remote();
    let mut b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let first_object = read_manifest(&cloud, &b.id())
        .partitions
        .values()
        .next()
        .unwrap()
        .object
        .clone();
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let path = protocol::object_path(&b.id(), &first_object);
    assert!(cloud.keys().contains(&path), "kept during grace period");

    b.now += protocol::RETIRED_GRACE_SECS + 1;
    b.sync(&cloud).await;
    assert!(!cloud.keys().contains(&path), "deleted after grace period");
    assert!(read_manifest(&cloud, &b.id()).retired.is_empty());
}

#[tokio::test]
async fn corrected_costs_and_model_mapping_are_reconciled() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "old-model", 100, 10, "0.10", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.summary(UsageDeviceScope::All).total_cost, "0.100000");

    {
        let conn = b.db.conn.lock().unwrap();
        conn.execute(
            "UPDATE proxy_request_logs SET total_cost_usd = '0.25', model = 'new-model' WHERE request_id = 'b1'",
            [],
        )
        .unwrap();
    }
    let report = b.sync(&cloud).await;
    assert_eq!(report.uploaded_partitions, 1);
    a.sync(&cloud).await;
    assert_eq!(a.summary(UsageDeviceScope::All).total_cost, "0.250000");
    let models =
        a.db.get_model_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].model, "new-model");
}

#[tokio::test]
async fn daily_detail_level_shares_no_request_records() {
    let cloud = remote();
    let a = Device::new("A");
    let mut b = Device::new("B");
    b.detail_level = UsageSyncDetailLevel::Daily;
    b.log(
        "req-private-1",
        "proxy",
        NOW - 60,
        "m",
        100,
        10,
        "0.10",
        200,
    );
    b.log(
        "req-private-2",
        "proxy",
        NOW - 30,
        "m",
        100,
        10,
        "0.10",
        500,
    );
    b.sync(&cloud).await;

    let manifest = read_manifest(&cloud, &b.id());
    assert_eq!(manifest.detail_level, protocol::DETAIL_LEVEL_DAILY);
    for (key, meta) in &manifest.partitions {
        let bytes = cloud
            .get(&protocol::object_path(&b.id(), &meta.object), usize::MAX)
            .unwrap()
            .unwrap();
        let segment = protocol::decode_segment(&bytes, meta).unwrap();
        assert!(
            segment.logs.is_empty(),
            "{key} must not carry request records"
        );
        let raw = String::from_utf8_lossy(&serde_json::to_vec(&segment).unwrap()).to_string();
        assert!(
            !raw.contains("req-private-1") && !raw.contains("req-private-2"),
            "no request ids leak"
        );
    }

    a.sync(&cloud).await;
    // Daily aggregates count in full-day ranges (same rule as local rollups).
    let summary = a.summary(UsageDeviceScope::Device(b.id()));
    assert_eq!(summary.total_requests, 2);
    assert!((summary.success_rate - 50.0).abs() < 0.01);
    let logs =
        a.db.get_request_logs(
            &crate::services::usage_stats::LogFilters {
                device: Some(b.id()),
                ..Default::default()
            },
            0,
            50,
        )
        .unwrap();
    assert_eq!(logs.total, 0, "no request log for daily-only devices");
}

#[tokio::test]
async fn uploaded_segments_contain_no_sensitive_fields() {
    let cloud = remote();
    let b = Device::new("B");
    {
        let conn = b.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, input_tokens, output_tokens,
                total_cost_usd, latency_ms, status_code, created_at, data_source,
                error_message, session_id
             ) VALUES ('r1', 'p', 'claude', 'm', 1, 1, '0.01', 1, 500, ?1, 'proxy',
                       'secret error body sk-ant-123', 'session-uuid-secret')",
            [NOW - 60],
        )
        .unwrap();
    }
    b.sync(&cloud).await;
    for key in cloud.keys() {
        let bytes = cloud.get(&key, usize::MAX).unwrap().unwrap();
        let text = if key.ends_with(".gz") {
            let mut out = String::new();
            use std::io::Read;
            flate2::read::GzDecoder::new(&bytes[..])
                .read_to_string(&mut out)
                .unwrap();
            out
        } else {
            String::from_utf8(bytes).unwrap()
        };
        assert!(!text.contains("sk-ant-123"), "{key} leaks error text");
        assert!(
            !text.contains("session-uuid-secret"),
            "{key} leaks session id"
        );
        assert!(!text.contains("errorMessage") && !text.contains("sessionId"));
    }
}

// ─── Identity ────────────────────────────────────────────────

fn clone_identity(from: &Device, to: &Device) {
    let identity = {
        let conn = from.db.conn.lock().unwrap();
        store::load_or_create_identity(&conn).unwrap()
    };
    let conn = to.db.conn.lock().unwrap();
    conn.execute("DELETE FROM usage_sync_identity", []).unwrap();
    conn.execute(
        "INSERT INTO usage_sync_identity (id, device_id, created_at, committed_nonce, pending_nonce, manifest_seq)
         VALUES (1, ?1, 0, ?2, ?3, ?4)",
        params![
            identity.device_id,
            identity.committed_nonce,
            identity.pending_nonce,
            identity.manifest_seq as i64
        ],
    )
    .unwrap();
}

#[tokio::test]
#[serial]
async fn cloned_profile_is_detected_and_resolved_with_a_new_identity() {
    let cloud = remote();
    let a = Device::new("Original");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;

    let clone = Device::new("Clone");
    clone_identity(&a, &clone);
    clone.log("c1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);

    // The original keeps writing; the clone then notices a foreign nonce.
    a.log("a2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let report = clone.sync(&cloud).await;
    assert!(report.identity_conflict, "{report:?}");
    assert!(report.upload_error.is_some());
    // Downloads still work while uploads are paused.
    assert!(report.download_error.is_none());
    let original_manifest = read_manifest(&cloud, &a.id());
    assert_eq!(
        original_manifest.device_name, "Original",
        "clone did not overwrite"
    );

    {
        let conn = clone.db.conn.lock().unwrap();
        store::reset_identity(&conn).unwrap();
    }
    assert_ne!(clone.id(), a.id());
    let report = clone.sync(&cloud).await;
    assert!(!report.identity_conflict);
    assert!(report.upload_error.is_none());
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 3);
}

#[tokio::test]
#[serial]
async fn take_over_adopts_the_remote_writer() {
    let cloud = remote();
    let a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let restored = Device::new("Restored");
    clone_identity(&a, &restored);
    a.log("a2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    assert!(restored.sync(&cloud).await.identity_conflict);

    let manifest = read_manifest(&cloud, &a.id());
    {
        let conn = restored.db.conn.lock().unwrap();
        store::adopt_remote_writer(&conn, &manifest.writer_nonce, manifest.seq).unwrap();
    }
    let report = restored.sync(&cloud).await;
    assert!(!report.identity_conflict);
    assert!(read_manifest(&cloud, &a.id()).seq > manifest.seq);
}

#[test]
fn identity_is_stable_and_device_local() {
    let db = Database::memory().unwrap();
    let conn = db.conn.lock().unwrap();
    let first = store::load_or_create_identity(&conn).unwrap();
    let second = store::load_or_create_identity(&conn).unwrap();
    assert_eq!(first.device_id, second.device_id);
    assert!(protocol::is_valid_device_id(&first.device_id));
    store::set_custom_device_name(&conn, Some("Work PC")).unwrap();
    assert_eq!(
        store::load_or_create_identity(&conn)
            .unwrap()
            .custom_name
            .as_deref(),
        Some("Work PC")
    );
}

// ─── Version gating & visibility ─────────────────────────────

#[tokio::test]
async fn newer_protocol_device_is_reported_and_cached_data_kept() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;

    let mut value = serde_json::to_value(read_manifest(&cloud, &b.id())).unwrap();
    value["version"] = serde_json::json!(protocol::USAGE_PROTOCOL_VERSION + 1);
    value["seq"] = serde_json::json!(99);
    cloud
        .put(&manifest_path(&b.id()), serde_json::to_vec(&value).unwrap())
        .unwrap();
    let report = a.sync(&cloud).await;
    assert_eq!(report.device_errors.len(), 1);
    let info = a.remote_device(&b.id()).unwrap();
    assert_eq!(info.status, store::DEVICE_STATUS_INCOMPATIBLE);
    assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 1);

    // B itself (older build) must not overwrite a newer writer's manifest.
    let report = b.sync(&cloud).await;
    assert!(report.upload_error.is_some());
}

#[tokio::test]
async fn older_manifest_never_rolls_the_cache_back() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let old = cloud
        .get(&manifest_path(&b.id()), usize::MAX)
        .unwrap()
        .unwrap();
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
    cloud.put(&manifest_path(&b.id()), old).unwrap();
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);
}

#[tokio::test]
async fn hidden_devices_are_excluded_until_unhidden() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    {
        let conn = a.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), true).unwrap();
    }
    assert_eq!(a.total_requests(UsageDeviceScope::All), 0);
    a.sync(&cloud).await;
    assert_eq!(
        a.total_requests(UsageDeviceScope::All),
        0,
        "stays hidden after sync"
    );
    {
        let conn = a.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), false).unwrap();
    }
    a.sync(&cloud).await;
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);
}

#[tokio::test]
async fn vanished_device_directory_keeps_history_and_is_flagged() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    for key in cloud.keys() {
        if key.contains(&b.id()) {
            cloud.delete(&key).unwrap();
        }
    }
    a.sync(&cloud).await;
    assert_eq!(
        a.remote_device(&b.id()).unwrap().status,
        store::DEVICE_STATUS_MISSING
    );
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);
}

#[test]
fn invalid_device_scope_is_rejected() {
    let a = Device::new("A");
    let err =
        a.db.get_usage_summary_scoped(
            None,
            None,
            None,
            None,
            None,
            &UsageDeviceScope::Device("x' OR 1=1 --".into()),
        )
        .unwrap_err();
    assert!(matches!(err, AppError::InvalidInput(_)));
}

// ─── Time handling ───────────────────────────────────────────

#[tokio::test]
async fn remote_records_bucket_by_the_viewers_local_calendar() {
    use chrono::{Local, TimeZone};
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    // Five minutes after a local midnight on the viewer.
    let day = Local::now().date_naive() - chrono::Duration::days(3);
    let midnight = Local
        .from_local_datetime(&day.and_hms_opt(0, 5, 0).unwrap())
        .earliest()
        .unwrap()
        .timestamp();
    b.log("b1", "proxy", midnight, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;

    let start = midnight - 5 * 86400;
    let end = midnight + 86400;
    let trends =
        a.db.get_daily_trends_scoped(
            Some(start),
            Some(end),
            None,
            None,
            None,
            &UsageDeviceScope::All,
        )
        .unwrap();
    let hit: Vec<_> = trends.iter().filter(|d| d.request_count > 0).collect();
    assert_eq!(hit.len(), 1);
    let bucket_day = chrono::DateTime::parse_from_rfc3339(&hit[0].date)
        .unwrap()
        .with_timezone(&Local)
        .date_naive();
    assert_eq!(bucket_day, day);
}

#[tokio::test]
async fn partitions_follow_the_source_local_calendar() {
    let cloud = remote();
    let b = Device::new("B");
    b.log("b1", "proxy", NOW, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let expected: String = {
        let conn = b.db.conn.lock().unwrap();
        conn.query_row("SELECT date(?1, 'unixepoch', 'localtime')", [NOW], |row| {
            row.get(0)
        })
        .unwrap()
    };
    let manifest = read_manifest(&cloud, &b.id());
    assert!(manifest.partitions.contains_key(&format!("d-{expected}")));
    assert_eq!(
        manifest.coverage.detail_from.as_deref(),
        Some(expected.as_str())
    );
}

// ─── Storage isolation ───────────────────────────────────────

#[tokio::test]
#[serial]
async fn config_snapshot_sync_neither_exports_nor_overwrites_usage_sync_state() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let a_id = a.id();

    // The configuration snapshot from B carries no usage-sync rows ...
    let snapshot = b.db.export_sql_string_for_sync().unwrap();
    for table in [
        "usage_sync_identity",
        "usage_sync_devices",
        "usage_sync_remote_partitions",
        "usage_sync_remote_logs",
        "usage_sync_remote_rollups",
        "usage_sync_remote_claims",
        "usage_sync_remote_inherited",
        "usage_sync_ledger_suppressed",
        "usage_sync_remote_multi",
        "usage_sync_inherited",
        "usage_rollup_ledger",
    ] {
        assert!(
            !snapshot.contains(&format!("INSERT INTO \"{table}\""))
                && !snapshot.contains(&format!("INSERT INTO {table} ")),
            "{table} rows must not be in the config snapshot"
        );
    }
    assert!(
        !snapshot.contains(&b.id()),
        "B's identity must not leak into the snapshot"
    );

    // ... and applying it on A keeps A's identity and remote cache.
    a.db.import_sql_string_for_sync(&snapshot).unwrap();
    assert_eq!(a.id(), a_id);
    assert_eq!(a.total_requests(UsageDeviceScope::All), 1);
}

#[test]
fn migration_from_v20_creates_usage_sync_tables() -> Result<(), AppError> {
    let conn = rusqlite::Connection::open_in_memory()?;
    Database::create_tables_on_conn(&conn)?;
    for table in [
        "usage_sync_identity",
        "usage_sync_devices",
        "usage_sync_remote_partitions",
        "usage_sync_remote_logs",
        "usage_sync_remote_rollups",
        "usage_sync_remote_claims",
        "usage_sync_remote_inherited",
        "usage_sync_ledger_suppressed",
        "usage_sync_remote_multi",
        "usage_sync_inherited_effective",
        "usage_sync_partition_versions",
        "usage_sync_export_cache",
        "usage_sync_namespaces",
        "usage_sync_inherited",
        "usage_rollup_ledger",
    ] {
        conn.execute_batch(&format!("DROP TABLE {table};"))?;
    }
    Database::set_user_version(&conn, 20)?;
    conn.execute(
        "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model, latency_ms, status_code, created_at)
         VALUES ('keep-me', 'p', 'claude', 'm', 1, 200, 1)",
        [],
    )?;
    Database::apply_schema_migrations_on_conn(&conn)?;
    assert_eq!(
        Database::get_user_version(&conn)?,
        crate::database::SCHEMA_VERSION
    );
    assert_eq!(crate::database::SCHEMA_VERSION, 21);
    for table in [
        "usage_sync_identity",
        "usage_sync_devices",
        "usage_sync_remote_partitions",
        "usage_sync_remote_logs",
        "usage_sync_remote_rollups",
        "usage_sync_remote_claims",
        "usage_sync_remote_inherited",
        "usage_sync_ledger_suppressed",
        "usage_sync_remote_multi",
        "usage_sync_inherited_effective",
        "usage_sync_partition_versions",
        "usage_sync_export_cache",
        "usage_sync_namespaces",
        "usage_sync_inherited",
        "usage_rollup_ledger",
    ] {
        assert!(Database::table_exists(&conn, table)?, "{table} missing");
    }
    let kept: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'keep-me'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(kept, 1, "local usage untouched by the migration");
    // All export-cache version triggers exist after the migration (the
    // cache is disabled otherwise).
    let triggers: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'trigger' AND name LIKE 'usage_sync_version_%'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(triggers, 12);
    // Idempotent re-run.
    Database::create_usage_sync_tables(&conn)?;
    Ok(())
}

#[tokio::test]
async fn failed_garbage_collection_is_retried_not_forgotten() {
    let cloud = remote();
    let mut b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let first = read_manifest(&cloud, &b.id())
        .partitions
        .values()
        .next()
        .unwrap()
        .object
        .clone();
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    let path = protocol::object_path(&b.id(), &first);

    // The store refuses deletes of that object: it must stay listed as retired.
    let fail_store = Arc::new(FailingDeletes {
        inner: cloud.clone(),
        needle: first.clone(),
    });
    b.now += protocol::RETIRED_GRACE_SECS + 1;
    fail_store.sync(&b).await;
    assert!(cloud.keys().contains(&path));
    assert!(read_manifest(&cloud, &b.id())
        .retired
        .iter()
        .any(|r| r.object == first));

    // Once deletes work again the object is collected and leaves the list.
    b.now += 60;
    b.sync(&cloud).await;
    assert!(!cloud.keys().contains(&path));
    assert!(read_manifest(&cloud, &b.id()).retired.is_empty());
}

/// Wraps the memory store so deletes of one object fail.
struct FailingDeletes {
    inner: Arc<MemoryStore>,
    needle: String,
}

impl FailingDeletes {
    async fn sync(&self, device: &Device) {
        // Make DELETE of this one object fail for the duration of the sync.
        let path = protocol::object_path(&device.id(), &self.needle);
        let bytes = self.inner.get(&path, usize::MAX).unwrap().unwrap();
        *self.inner.fail_delete_containing.lock().unwrap() = Some(self.needle.clone());
        device.sync(&self.inner).await;
        *self.inner.fail_delete_containing.lock().unwrap() = None;
        assert_eq!(self.inner.get(&path, usize::MAX).unwrap(), Some(bytes));
    }
}

/// Opt-in timing check: 50k local rows plus 3 remote devices × 50k cached rows.
/// `cargo test --release --lib usage_sync::tests::perf_ -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn perf_all_scope_queries_on_large_caches() {
    let cloud = remote();
    let devices: Vec<Device> = ["A", "B", "C", "D"].into_iter().map(Device::new).collect();
    let now = chrono::Utc::now().timestamp();
    for (d, device) in devices.iter().enumerate() {
        let conn = device.db.conn.lock().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, input_tokens, output_tokens,
                        cache_read_tokens, input_token_semantics, total_cost_usd, latency_ms,
                        status_code, created_at, data_source
                     ) VALUES (?1, ?2, ?3, ?4, 1000, 200, 500, 2, '0.0123', 900, 200, ?5, ?6)",
                )
                .unwrap();
            for i in 0..50_000i64 {
                // Every 10th record is a session record shared by all devices.
                let (id, source) = if i % 10 == 0 {
                    (format!("session:shared-{i}"), "session_log")
                } else {
                    (format!("dev{d}-{i}"), "proxy")
                };
                stmt.execute(rusqlite::params![
                    id,
                    format!("prov-{}", i % 7),
                    ["claude", "codex", "gemini"][(i % 3) as usize],
                    format!("model-{}", i % 11),
                    now - (i * 50) % (29 * 86_400),
                    source,
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    let export_lock = {
        let conn = devices[1].db.conn.lock().unwrap();
        let t = std::time::Instant::now();
        let export = store::export_local(&conn, "perf", UsageSyncDetailLevel::Records).unwrap();
        assert_eq!(
            export
                .partitions
                .values()
                .map(|p| p.logs.len())
                .sum::<usize>(),
            50_000
        );
        t.elapsed()
    };
    println!("export of 50k rows (DB lock held): {export_lock:?}");
    let started = std::time::Instant::now();
    for device in &devices[1..] {
        device.sync(&cloud).await;
    }
    let upload = started.elapsed();
    let started = std::time::Instant::now();
    devices[0].sync(&cloud).await;
    let download = started.elapsed();
    let a = &devices[0];
    let time = |label: &str, f: &dyn Fn()| {
        let t = std::time::Instant::now();
        f();
        let elapsed = t.elapsed();
        println!("{label}: {elapsed:?}");
        elapsed
    };
    let start = Some(now - 30 * 86_400);
    let end = Some(now);
    let all = UsageDeviceScope::All;
    let mut worst = std::time::Duration::ZERO;
    for elapsed in [
        time("summary(all, 30d)", &|| {
            a.db.get_usage_summary_scoped(start, end, None, None, None, &all)
                .unwrap();
        }),
        time("trends(all, 30d)", &|| {
            a.db.get_daily_trends_scoped(start, end, None, None, None, &all)
                .unwrap();
        }),
        time("provider stats(all, 30d)", &|| {
            a.db.get_provider_stats_scoped(start, end, None, None, None, &all)
                .unwrap();
        }),
        time("model stats(all, 30d)", &|| {
            a.db.get_model_stats_scoped(start, end, None, None, None, &all)
                .unwrap();
        }),
        time("request logs page(all)", &|| {
            a.db.get_request_logs(
                &crate::services::usage_stats::LogFilters {
                    device: Some("all".into()),
                    start_date: start,
                    end_date: end,
                    ..Default::default()
                },
                0,
                20,
            )
            .unwrap();
        }),
        time("summary(local, 30d)", &|| {
            a.db.get_usage_summary(start, end, None, None, None)
                .unwrap();
        }),
    ] {
        worst = worst.max(elapsed);
    }
    println!("upload 3×50k: {upload:?}; download 150k: {download:?}");
    let summary = a.summary(UsageDeviceScope::All);
    // 4 devices × 45k own records + 5k shared session records counted once.
    assert_eq!(summary.total_requests, 4 * 45_000 + 5_000);
    let objects: usize = cloud.objects.lock().unwrap().values().map(Vec::len).sum();
    println!("remote bytes for 3 devices: {objects}");
    assert!(
        worst < std::time::Duration::from_secs(10),
        "worst query {worst:?}"
    );
}

// ─── R1: cross-device history identity (P1-1) ────────────────

fn days_ago(days: i64) -> i64 {
    chrono::Utc::now().timestamp() - days * 86_400
}

fn all_summary(device: &Device) -> crate::services::usage_stats::UsageSummary {
    device.summary(UsageDeviceScope::All)
}

async fn sync_all(cloud: &Arc<MemoryStore>, devices: &[&Device]) {
    // Two rounds so every device sees every other device's latest state.
    for _ in 0..2 {
        for device in devices {
            device.sync(cloud).await;
        }
    }
}

fn assert_all_viewers(devices: &[&Device], requests: u64, cost: &str) {
    for device in devices {
        let summary = all_summary(device);
        assert_eq!(
            (summary.total_requests, summary.total_cost.as_str()),
            (requests, cost),
            "viewer {} disagrees",
            device.name
        );
    }
}

#[tokio::test]
async fn shared_request_rolled_up_on_either_side_counts_once_everywhere() {
    for (rolled_a, rolled_b) in [(true, false), (false, true), (true, true)] {
        let cloud = remote();
        let a = Device::new("A");
        let b = Device::new("B");
        let c = Device::new("C");
        let old = days_ago(40);
        a.log(
            "session:shared",
            "session_log",
            old,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        b.log(
            "session:shared",
            "session_log",
            old,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        if rolled_a {
            a.db.rollup_and_prune(30).unwrap();
        }
        if rolled_b {
            b.db.rollup_and_prune(30).unwrap();
        }
        sync_all(&cloud, &[&a, &b, &c]).await;
        assert_all_viewers(&[&a, &b, &c], 1, "0.050000");
        // Per-device views still show each device's own data.
        assert_eq!(a.total_requests(UsageDeviceScope::Device(b.id())), 1);
        assert_eq!(c.total_requests(UsageDeviceScope::Device(a.id())), 1);
    }
}

#[tokio::test]
async fn offline_device_catching_up_never_double_counts() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    let old = days_ago(40);
    for device in [&a, &b] {
        device.log("session:s1", "session_log", old, "m", 100, 10, "0.05", 200);
    }
    b.log("b-own", "proxy", old + 60, "m", 5, 5, "0.01", 200);
    // Stage 1: only A is online and has rolled up.
    a.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &c]).await;
    assert_all_viewers(&[&a, &c], 1, "0.050000");
    // Stage 2: B comes back with request-level rows.
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 2, "0.060000");
    // Stage 3: B rolls up later.
    b.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 2, "0.060000");
}

#[tokio::test]
async fn deduplicated_history_keeps_exact_tokens_costs_and_dimensions() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = days_ago(40);
    // Same day, provider and model: one shared request plus independent ones.
    a.log(
        "session:shared",
        "session_log",
        old,
        "model-x",
        100,
        10,
        "0.05",
        200,
    );
    b.log(
        "session:shared",
        "session_log",
        old,
        "model-x",
        100,
        10,
        "0.05",
        200,
    );
    a.log("a-own", "proxy", old + 10, "model-x", 30, 3, "0.02", 200);
    b.log("b-own", "proxy", old + 20, "model-x", 7, 2, "0.03", 500);
    b.log(
        "b-other-model",
        "proxy",
        old + 30,
        "model-y",
        1,
        1,
        "0.10",
        200,
    );
    a.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b]).await;

    for viewer in [&a, &b] {
        let all = all_summary(viewer);
        assert_eq!(all.total_requests, 4);
        assert_eq!(all.total_cost, "0.200000");
        assert_eq!(all.total_input_tokens, 100 + 30 + 7 + 1);
        assert_eq!(all.total_output_tokens, 10 + 3 + 2 + 1);
        assert!((all.success_rate - 75.0).abs() < 0.01);
        let models = viewer
            .db
            .get_model_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
        let x = models.iter().find(|m| m.model == "model-x").unwrap();
        let y = models.iter().find(|m| m.model == "model-y").unwrap();
        assert_eq!((x.request_count, y.request_count), (3, 1));
        let providers = viewer
            .db
            .get_provider_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
        assert_eq!(providers.iter().map(|p| p.request_count).sum::<u64>(), 4);
        let start = Some(old - 86_400);
        let end = Some(old + 86_400);
        let trends = viewer
            .db
            .get_daily_trends_scoped(start, end, None, None, None, &UsageDeviceScope::All)
            .unwrap();
        assert_eq!(trends.iter().map(|d| d.request_count).sum::<u64>(), 4);
        let model_x = viewer
            .db
            .get_usage_summary_scoped(
                None,
                None,
                None,
                None,
                Some("model-x"),
                &UsageDeviceScope::All,
            )
            .unwrap();
        assert_eq!(
            (model_x.total_requests, model_x.total_cost.as_str()),
            (3, "0.100000")
        );
    }
}

#[tokio::test]
async fn proxy_session_pair_alias_is_honoured_after_either_rollup() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    let old = days_ago(40);
    // B ran the call through its proxy and also imported the session line;
    // A only has the session line (synced ~/.claude).
    b.log("proxy-uuid-9", "proxy", old, "m", 100, 10, "0.05", 200);
    b.log(
        "session:msg_9",
        "session_log",
        old + 5,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    a.log(
        "session:msg_9",
        "session_log",
        old + 5,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 1, "0.050000");
    b.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 1, "0.050000");
    a.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 1, "0.050000");
}

#[tokio::test]
async fn corrected_copies_resolve_to_one_deterministic_winner() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    let old = days_ago(40);
    a.log("session:s", "session_log", old, "m", 100, 10, "0.05", 200);
    b.log("session:s", "session_log", old, "m", 100, 10, "0.05", 200);
    a.db.rollup_and_prune(30).unwrap();
    // B re-prices its (still request-level) copy afterwards.
    {
        let conn = b.db.conn.lock().unwrap();
        conn.execute("UPDATE proxy_request_logs SET total_cost_usd = '0.07'", [])
            .unwrap();
    }
    sync_all(&cloud, &[&a, &b, &c]).await;
    let winner_cost = if a.id() < b.id() {
        "0.050000"
    } else {
        "0.070000"
    };
    assert_all_viewers(&[&a, &b, &c], 1, winner_cost);
}

#[tokio::test]
async fn legacy_rollups_without_ledger_stay_per_device_and_are_reported() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = days_ago(40);
    for device in [&a, &b] {
        device.log(
            "session:legacy",
            "session_log",
            old,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        device.db.rollup_and_prune(30).unwrap();
        // Simulate rollups made by a version without the identity ledger.
        device
            .db
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM usage_rollup_ledger", [])
            .unwrap();
    }
    sync_all(&cloud, &[&a, &b]).await;
    // No provable identity: counted once per device, and reported as such.
    assert_eq!(all_summary(&a).total_requests, 2);
    let local = {
        let conn = a.db.conn.lock().unwrap();
        store::local_unverified(&conn).unwrap()
    };
    let expected_date = {
        let conn = a.db.conn.lock().unwrap();
        conn.query_row("SELECT date(?1, 'unixepoch', 'localtime')", [old], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    };
    assert_eq!(local.requests, 1);
    assert_eq!(local.first_date.as_deref(), Some(expected_date.as_str()));
    let remote_b = a.remote_device(&b.id()).unwrap();
    assert_eq!(remote_b.unverified.requests, 1);
}

#[tokio::test]
async fn ledger_is_bounded_by_retention_and_expired_history_is_unverified() {
    let a = Device::new("A");
    a.log("ancient", "proxy", days_ago(400), "m", 1, 1, "0.01", 200);
    a.log("recent-old", "proxy", days_ago(40), "m", 1, 1, "0.01", 200);
    a.db.rollup_and_prune(30).unwrap();
    let conn = a.db.conn.lock().unwrap();
    let ids: Vec<String> = conn
        .prepare("SELECT request_id FROM usage_rollup_ledger ORDER BY request_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids, vec!["recent-old".to_string()]);
    assert_eq!(store::local_unverified(&conn).unwrap().requests, 1);
}

#[tokio::test]
async fn daily_only_devices_are_reported_unverified() {
    let cloud = remote();
    let a = Device::new("A");
    let mut b = Device::new("B");
    b.detail_level = UsageSyncDetailLevel::Daily;
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.log("b2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    assert_eq!(a.remote_device(&b.id()).unwrap().unverified.requests, 2);
}

#[tokio::test]
async fn codex_rebuild_drops_its_ledger_entries_with_its_rollups() {
    let a = Device::new("A");
    {
        let conn = a.db.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs (request_id, provider_id, app_type, model,
                input_tokens, output_tokens, total_cost_usd, latency_ms, status_code,
                created_at, data_source)
             VALUES ('codex_session:x', '_codex_session', 'codex', 'gpt', 1, 1, '0.01', 1, 200,
                     ?1, 'codex_session')",
            [days_ago(40)],
        )
        .unwrap();
    }
    a.db.rollup_and_prune(30).unwrap();
    let conn = a.db.conn.lock().unwrap();
    let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM usage_rollup_ledger"), 1);
    let dir = tempfile::tempdir().unwrap();
    crate::services::session_usage_codex::reset_codex_usage_on_conn(&conn, dir.path()).unwrap();
    assert_eq!(count("SELECT COUNT(*) FROM usage_daily_rollups"), 0);
    assert_eq!(count("SELECT COUNT(*) FROM usage_rollup_ledger"), 0);
}

// ─── R1: copied installations (P1-2) ─────────────────────────

/// A real data-folder copy: the whole database, including the identity and
/// the rollup ledger.
fn copy_installation(from: &Device, to: &Device) {
    let dump = from.db.export_sql_string().unwrap();
    to.db.import_sql_string(&dump).unwrap();
}

async fn fork_after_conflict(cloud: &Arc<MemoryStore>, original: &Device, clone: &Device) {
    original.sync(cloud).await;
    let report = clone.sync(cloud).await;
    assert!(report.identity_conflict, "{report:?}");
    {
        let conn = clone.db.conn.lock().unwrap();
        let before = store::load_or_create_identity(&conn).unwrap().device_id;
        let after = store::reset_identity(&conn).unwrap();
        assert_eq!(after.parent_device_id.as_deref(), Some(before.as_str()));
    }
    let report = clone.sync(cloud).await;
    assert!(
        !report.identity_conflict && report.upload_error.is_none(),
        "{report:?}"
    );
}

fn seed_history(device: &Device) {
    for i in 0..5 {
        device.log(
            &format!("old-{i}"),
            "proxy",
            days_ago(40) + i,
            "m",
            10,
            1,
            "0.01",
            200,
        );
    }
    device.log("recent", "proxy", days_ago(1), "m", 10, 1, "0.01", 200);
    device.db.rollup_and_prune(30).unwrap();
}

#[tokio::test]
#[serial]
async fn copied_installation_with_new_identity_keeps_history_once_and_counts_new_work() {
    let cloud = remote();
    let a = Device::new("Original");
    let viewer = Device::new("Viewer");
    seed_history(&a);
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    copy_installation(&a, &clone);

    // Both installations keep working after the copy.
    a.log(
        "a-after",
        "proxy",
        days_ago(0) - 600,
        "m",
        10,
        1,
        "0.01",
        200,
    );
    clone.log(
        "c-after",
        "proxy",
        days_ago(0) - 500,
        "m",
        10,
        1,
        "0.01",
        200,
    );
    fork_after_conflict(&cloud, &a, &clone).await;
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    // 5 rolled-up + recent + a-after + c-after.
    assert_all_viewers(&[&a, &clone, &viewer], 8, "0.080000");

    // Both roll the shared and their own requests up later: still exact.
    for device in [&a, &clone] {
        let conn = device.db.conn.lock().unwrap();
        conn.execute(
            "UPDATE proxy_request_logs SET created_at = created_at - 45 * 86400",
            [],
        )
        .unwrap();
    }
    a.db.rollup_and_prune(30).unwrap();
    clone.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    assert_all_viewers(&[&a, &clone, &viewer], 8, "0.080000");
}

#[tokio::test]
#[serial]
async fn copied_legacy_history_is_attributed_to_the_parent_while_present() {
    let cloud = remote();
    let a = Device::new("Original");
    let viewer = Device::new("Viewer");
    seed_history(&a);
    // History rolled up before the identity ledger existed.
    a.db.conn
        .lock()
        .unwrap()
        .execute("DELETE FROM usage_rollup_ledger", [])
        .unwrap();
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    copy_installation(&a, &clone);
    // The original keeps writing, which is what surfaces the conflict.
    let now = days_ago(0);
    a.log("a-after", "proxy", now - 600, "m", 10, 1, "0.01", 200);
    fork_after_conflict(&cloud, &a, &clone).await;
    clone.log("c-after", "proxy", now - 500, "m", 10, 1, "0.01", 200);
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    // 5 legacy + recent + a-after + c-after: the legacy totals count once.
    assert_all_viewers(&[&a, &clone, &viewer], 8, "0.080000");

    // The original goes offline: its cached data stays, totals unchanged.
    a.log("a-offline", "proxy", now - 400, "m", 10, 1, "0.01", 200);
    sync_all(&cloud, &[&clone, &viewer]).await;
    assert_all_viewers(&[&clone, &viewer], 8, "0.080000");
    // It comes back: its offline work is counted, nothing twice.
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    assert_all_viewers(&[&a, &clone, &viewer], 9, "0.090000");

    // The original is retired and hidden: the copy now carries the history.
    for device in [&clone, &viewer] {
        let conn = device.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &a.id(), true).unwrap();
    }
    // a-after and a-offline were only ever on the original and leave with
    // it; the copied history (5 legacy + recent) is carried by the copy.
    assert_all_viewers(&[&clone, &viewer], 7, "0.070000");
}

#[tokio::test]
#[serial]
async fn restored_backup_conflict_resolves_either_way_without_double_counting() {
    for take_over in [true, false] {
        let cloud = remote();
        let a = Device::new("Machine");
        let viewer = Device::new("Viewer");
        seed_history(&a);
        a.sync(&cloud).await;
        let backup = a.db.export_sql_string().unwrap();
        a.log(
            "after-backup",
            "proxy",
            days_ago(0) - 900,
            "m",
            10,
            1,
            "0.01",
            200,
        );
        a.sync(&cloud).await;

        // The machine is restored from the backup (same identity, older nonce).
        let restored = Device::new("Machine");
        restored.db.import_sql_string(&backup).unwrap();
        let report = restored.sync(&cloud).await;
        assert!(report.identity_conflict);
        if take_over {
            let manifest = read_manifest(&cloud, &restored.id());
            let conn = restored.db.conn.lock().unwrap();
            store::adopt_remote_writer(&conn, &manifest.writer_nonce, manifest.seq).unwrap();
        } else {
            let conn = restored.db.conn.lock().unwrap();
            store::reset_identity(&conn).unwrap();
        }
        restored.log(
            "after-restore",
            "proxy",
            days_ago(0) - 300,
            "m",
            10,
            1,
            "0.01",
            200,
        );
        sync_all(&cloud, &[&restored, &viewer]).await;
        let expected = if take_over {
            // The restored machine republishes its own lineage; "after-backup"
            // existed only on the replaced database.
            (7, "0.070000")
        } else {
            // The pre-restore directory stays as the parent with "after-backup".
            (8, "0.080000")
        };
        assert_all_viewers(&[&restored, &viewer], expected.0, expected.1);
    }
}

// ─── R1: protocol hardening (P3) ─────────────────────────────

#[test]
fn manifest_with_out_of_range_seq_is_rejected_by_readers() {
    let mut manifest = DeviceManifest {
        format: protocol::USAGE_SYNC_FORMAT.into(),
        version: protocol::USAGE_PROTOCOL_VERSION,
        device_id: "0f8fad5b-d9cb-469f-a165-70867728950e".into(),
        device_name: "x".into(),
        platform: String::new(),
        app_version: String::new(),
        seq: protocol::MAX_MANIFEST_SEQ,
        writer_nonce: "n".into(),
        updated_at: 0,
        detail_level: protocol::DETAIL_LEVEL_RECORDS.into(),
        utc_offset_minutes: 0,
        coverage: Default::default(),
        partitions: Default::default(),
        retired: Vec::new(),
        lineage: None,
    };
    assert!(protocol::validate_manifest(&manifest, &manifest.device_id.clone()).is_ok());
    manifest.seq = protocol::MAX_MANIFEST_SEQ + 1;
    assert!(protocol::validate_manifest(&manifest, &manifest.device_id.clone()).is_err());
}

#[tokio::test]
async fn exhausted_own_sequence_fails_cleanly_and_does_not_wrap() {
    let cloud = remote();
    let a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let mut manifest = read_manifest(&cloud, &a.id());
    manifest.seq = protocol::MAX_MANIFEST_SEQ;
    cloud
        .put(
            &manifest_path(&a.id()),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    a.log("a2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    let report = a.sync(&cloud).await;
    assert!(report.upload_error.unwrap().contains("exhausted"));
    assert_eq!(
        read_manifest(&cloud, &a.id()).seq,
        protocol::MAX_MANIFEST_SEQ
    );
}

#[tokio::test]
async fn orphaned_objects_are_retired_and_collected() {
    let cloud = remote();
    let mut a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    // An object written by a losing same-identity writer: never referenced.
    let orphan = protocol::object_name("d-2025-10-09", &"b".repeat(64));
    let orphan_path = protocol::object_path(&a.id(), &orphan);
    cloud.put(&orphan_path, b"orphan".to_vec()).unwrap();
    // A foreign file in the directory is never touched.
    let foreign = protocol::object_path(&a.id(), "parts/notes.txt");
    cloud.put(&foreign, b"keep".to_vec()).unwrap();

    a.now += ORPHAN_SWEEP_INTERVAL_SECS;
    a.log("a2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let manifest = read_manifest(&cloud, &a.id());
    assert!(manifest.retired.iter().any(|r| r.object == orphan));
    assert!(
        cloud.keys().contains(&orphan_path),
        "kept during grace period"
    );

    a.now += protocol::RETIRED_GRACE_SECS + 1;
    a.sync(&cloud).await;
    assert!(
        !cloud.keys().contains(&orphan_path),
        "collected after grace"
    );
    assert!(cloud.keys().contains(&foreign));
}

#[test]
fn partition_of_object_only_accepts_protocol_names() {
    let name = protocol::object_name("m-2026-09", &"a".repeat(64));
    assert_eq!(
        protocol::partition_of_object(&name).as_deref(),
        Some("m-2026-09")
    );
    assert!(protocol::partition_of_object("parts/notes.txt").is_none());
    assert!(protocol::partition_of_object("parts/../x-aaaaaaaaaaaaaaaa.json.gz").is_none());
}

#[tokio::test]
async fn device_utc_offset_and_lineage_are_recorded_for_day_labels() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    b.log("b1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    b.sync(&cloud).await;
    a.sync(&cloud).await;
    let expected = i64::from(chrono::Local::now().offset().local_minus_utc() / 60);
    let info = a.remote_device(&b.id()).unwrap();
    assert_eq!(info.utc_offset_minutes, Some(expected));
    assert_eq!(info.parent_device_id, None);
}

/// Opt-in: storage and query cost of the identity ledger with 4 devices that
/// each rolled up 50k requests (10% shared between all devices).
/// `cargo test --lib usage_sync::tests::perf_ledger -- --ignored --nocapture`
#[tokio::test]
#[serial]
#[ignore]
async fn perf_ledger_storage_and_all_scope_queries() {
    let cloud = remote();
    let devices: Vec<Device> = ["A", "B", "C", "D"].into_iter().map(Device::new).collect();
    let base = days_ago(200);
    // Size of the ledger, measured on a separate copy before any sync.
    for (d, device) in devices.iter().enumerate() {
        let conn = device.db.conn.lock().unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO proxy_request_logs (
                        request_id, provider_id, app_type, model, input_tokens, output_tokens,
                        input_token_semantics, total_cost_usd, latency_ms, status_code,
                        created_at, data_source
                     ) VALUES (?1, ?2, 'claude', ?3, 1000, 200, 2, '0.0123', 900, 200, ?4, ?5)",
                )
                .unwrap();
            for i in 0..50_000i64 {
                let (id, source) = if i % 10 == 0 {
                    (format!("session:shared-{i}"), "session_log")
                } else {
                    (format!("dev{d}-{i}-0123456789abcdef"), "proxy")
                };
                stmt.execute(rusqlite::params![
                    id,
                    format!("prov-{}", i % 7),
                    format!("model-{}", i % 11),
                    // 250 s apart, no wrap-around: no two rows of the same
                    // model fall inside the local proxy/session match window.
                    base + i * 250,
                    source,
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();
    }
    for device in &devices {
        device.db.rollup_and_prune(30).unwrap();
    }
    let size = |device: &Device| -> i64 {
        let conn = device.db.conn.lock().unwrap();
        conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    let (with_ledger, without_ledger) = {
        let probe = &devices[3];
        let copy = Device::new("Probe");
        copy_installation(probe, &copy);
        copy.db
            .conn
            .lock()
            .unwrap()
            .execute_batch("VACUUM")
            .unwrap();
        let with = size(&copy);
        {
            let conn = copy.db.conn.lock().unwrap();
            conn.execute("DELETE FROM usage_rollup_ledger", []).unwrap();
            conn.execute_batch("VACUUM").unwrap();
        }
        (with, size(&copy))
    };
    let ledger_rows: i64 = devices[0]
        .db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM usage_rollup_ledger", [], |r| r.get(0))
        .unwrap();
    let started = std::time::Instant::now();
    for device in &devices[1..] {
        device.sync(&cloud).await;
    }
    let upload = started.elapsed();
    let started = std::time::Instant::now();
    devices[0].sync(&cloud).await;
    let download = started.elapsed();
    let remote_bytes: usize = cloud.objects.lock().unwrap().values().map(Vec::len).sum();
    let a = &devices[0];
    let t = std::time::Instant::now();
    let all = a.summary(UsageDeviceScope::All);
    let all_time = t.elapsed();
    let t = std::time::Instant::now();
    a.db.get_daily_trends_scoped(
        Some(days_ago(365)),
        Some(days_ago(0)),
        None,
        None,
        None,
        &UsageDeviceScope::All,
    )
    .unwrap();
    let trends_time = t.elapsed();
    let t = std::time::Instant::now();
    {
        let conn = a.db.conn.lock().unwrap();
        store::refresh_ledger_suppression(&conn).unwrap();
    }
    let refresh = t.elapsed();
    println!(
        "ledger rows/device={ledger_rows}; local DB {with_ledger} B with ledger vs {without_ledger} B without \
         → {} B/row; remote bytes (3 devices incl. ledgers)={remote_bytes}; upload 3 devices={upload:?}; \
         download+apply 3 devices={download:?}; suppression refresh={refresh:?}; \
         All summary={all_time:?}; All trends 365d={trends_time:?}",
        (with_ledger - without_ledger) / ledger_rows.max(1)
    );
    // 4 × 45k own + 5k shared counted once.
    assert_eq!(all.total_requests, 4 * 45_000 + 5_000);
}

#[tokio::test]
async fn deduplication_keeps_the_average_latency_of_rollup_groups() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = days_ago(40);
    for device in [&a, &b] {
        device.log(
            "session:shared",
            "session_log",
            old,
            "m",
            100,
            10,
            "0.05",
            200,
        );
    }
    // Two more requests on A in the same group, all at latency 120 ms.
    a.log("a-1", "proxy", old + 10, "m", 1, 1, "0.01", 200);
    a.log("a-2", "proxy", old + 20, "m", 1, 1, "0.01", 200);
    a.db.rollup_and_prune(30).unwrap();
    b.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b]).await;
    for viewer in [&a, &b] {
        let stats = viewer
            .db
            .get_provider_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
        assert_eq!(stats.iter().map(|p| p.request_count).sum::<u64>(), 3);
        for provider in &stats {
            assert_eq!(provider.avg_latency_ms, 120, "{provider:?}");
        }
    }
}

#[tokio::test]
async fn shared_request_rolled_up_after_it_was_already_synced_counts_once() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let c = Device::new("C");
    let old = days_ago(40);
    a.log("a-1", "proxy", old - 100, "m", 500, 10, "0.10", 200);
    for device in [&a, &b] {
        device.log(
            "session:shared",
            "session_log",
            old,
            "m",
            100,
            10,
            "0.05",
            200,
        );
    }
    b.log("b-1", "proxy", old + 100, "m", 700, 10, "0.20", 200);
    // Everything is request-level and synced first.
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 3, "0.350000");
    // Then A rolls up and everyone syncs once more.
    a.db.rollup_and_prune(30).unwrap();
    sync_all(&cloud, &[&a, &b, &c]).await;
    assert_all_viewers(&[&a, &b, &c], 3, "0.350000");
}

// ─── R2 corrective regressions ───────────────────────────────

#[tokio::test]
async fn expired_shared_history_is_symmetric_and_reported() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let old = (chrono::Local::now() - chrono::Duration::days(370))
        .format("%Y-%m-%d")
        .to_string();
    for device in [&a, &b] {
        device
            .db
            .conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_model,
                    pricing_model, request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, input_token_semantics,
                    total_cost_usd, avg_latency_ms)
                 VALUES ('{old}', 'claude', 'prov', 'm', '', '', 1, 1, 100, 10, 0, 0, 2, '0.05', 100);
                 INSERT INTO usage_rollup_ledger (request_id, data_source, kind, date, app_type,
                    provider_id, model, request_model, pricing_model, success, input_tokens,
                    output_tokens, cache_read_tokens, cache_creation_tokens, total_cost_usd,
                    rolled_up_at)
                 VALUES ('session:ancient', 'session_log', 0, '{old}', 'claude', 'prov', 'm', '',
                    '', 1, 100, 10, 0, 0, '0.05', 0);"
            ))
            .unwrap();
    }
    // A has new rows to roll up, B is idle: retention must run on both.
    a.log("a-new", "proxy", days_ago(40), "m", 100, 10, "0.01", 200);
    assert_eq!(a.db.rollup_and_prune(30).unwrap(), 1);
    assert_eq!(b.db.rollup_and_prune(30).unwrap(), 0);
    for device in [&a, &b] {
        let conn = device.db.conn.lock().unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_rollup_ledger WHERE request_id = 'session:ancient'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "{} kept an expired ledger entry", device.name);
    }
    sync_all(&cloud, &[&a, &b]).await;
    // Same answer on both viewers; the expired copy is reported on both sides.
    assert_all_viewers(&[&a, &b], 3, "0.110000");
    let local_a = {
        let conn = a.db.conn.lock().unwrap();
        store::local_unverified(&conn).unwrap()
    };
    assert_eq!(local_a.requests, 1);
    assert_eq!(a.remote_device(&b.id()).unwrap().unverified.requests, 1);
}

#[tokio::test]
async fn retention_boundary_keeps_day_365_and_drops_day_366() {
    let a = Device::new("A");
    let day = |n: i64| {
        (chrono::Local::now() - chrono::Duration::days(n))
            .format("%Y-%m-%d")
            .to_string()
    };
    {
        let conn = a.db.conn.lock().unwrap();
        for (id, date) in [("keep", day(365)), ("drop", day(366))] {
            conn.execute(
                "INSERT INTO usage_rollup_ledger (request_id, data_source, kind, date, app_type,
                    provider_id, model, success, total_cost_usd, rolled_up_at)
                 VALUES (?1, 'proxy', 0, ?2, 'claude', 'p', 'm', 1, '0.01', 0)",
                params![id, date],
            )
            .unwrap();
        }
    }
    // Nothing to roll up: retention still applies.
    assert_eq!(a.db.rollup_and_prune(30).unwrap(), 0);
    let conn = a.db.conn.lock().unwrap();
    let ids: Vec<String> = conn
        .prepare("SELECT request_id FROM usage_rollup_ledger")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids, vec!["keep".to_string()]);
}

/// Seeds history like the R2 probes: 5 ledger-covered + 1 recent + 4 legacy (id-less).
fn seed_with_legacy(device: &Device) {
    seed_history(device);
    let date = (chrono::Local::now() - chrono::Duration::days(60))
        .format("%Y-%m-%d")
        .to_string();
    device
        .db
        .conn
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_model,
                pricing_model, request_count, success_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics, total_cost_usd,
                avg_latency_ms)
             VALUES (?1, 'claude', 'prov-1', 'legacy', '', '', 4, 4, 400, 40, 0, 0, 2, '0.04', 100)",
            [date],
        )
        .unwrap();
}

fn legacy_group(device: &Device) -> Option<(u64, String)> {
    device
        .db
        .get_model_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
        .unwrap()
        .into_iter()
        .find(|m| m.model == "legacy")
        .map(|m| (m.request_count, m.total_cost))
}

#[tokio::test]
#[serial]
async fn codex_rebuild_on_a_copy_never_cancels_the_parents_history() {
    let cloud = remote();
    let a = Device::new("Original");
    let viewer = Device::new("Viewer");
    seed_with_legacy(&a);
    // Legacy Codex session history (id-less) on the original as well.
    {
        let conn = a.db.conn.lock().unwrap();
        let date = (chrono::Local::now() - chrono::Duration::days(70))
            .format("%Y-%m-%d")
            .to_string();
        conn.execute(
            "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_model,
                pricing_model, request_count, success_count, input_tokens, output_tokens,
                cache_read_tokens, cache_creation_tokens, input_token_semantics, total_cost_usd,
                avg_latency_ms)
             VALUES (?1, 'codex', '_codex_session', 'gpt', '', '', 3, 3, 30, 3, 0, 0, 2, '0.03', 50)",
            [date],
        )
        .unwrap();
    }
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    copy_installation(&a, &clone);
    a.log(
        "a-after",
        "proxy",
        days_ago(0) - 600,
        "m",
        10,
        1,
        "0.01",
        200,
    );
    fork_after_conflict(&cloud, &a, &clone).await;
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    // 5 + recent + 4 legacy + 3 codex legacy + a-after.
    assert_all_viewers(&[&a, &clone, &viewer], 14, "0.140000");

    // The copy rebuilds its Codex usage (deletes its Codex rollups).
    clone.db.reset_codex_usage().unwrap();
    {
        let conn = clone.db.conn.lock().unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_sync_inherited WHERE provider_id = '_codex_session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0, "inherited Codex rows dropped with the rollups");
        let other: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_sync_inherited", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(other > 0, "unrelated inherited sources are kept");
    }
    // Before and after the copy republishes: never below the parent's history.
    assert_all_viewers(&[&a, &clone], 14, "0.140000");
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    assert_all_viewers(&[&a, &clone, &viewer], 14, "0.140000");
    for viewer in [&a, &clone, &viewer] {
        assert_eq!(legacy_group(viewer), Some((4, "0.040000".to_string())));
    }
}

#[tokio::test]
#[serial]
async fn inherited_subtraction_is_bounded_by_what_the_ancestor_still_holds() {
    let cloud = remote();
    let a = Device::new("Original");
    let viewer = Device::new("Viewer");
    seed_with_legacy(&a);
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    copy_installation(&a, &clone);
    a.log(
        "a-after",
        "proxy",
        days_ago(0) - 600,
        "m",
        10,
        1,
        "0.01",
        200,
    );
    fork_after_conflict(&cloud, &a, &clone).await;
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    assert_all_viewers(&[&a, &clone, &viewer], 11, "0.110000");
    // The parent loses its legacy rows (cleanup); the copy still has them.
    a.db.conn
        .lock()
        .unwrap()
        .execute("DELETE FROM usage_daily_rollups WHERE model = 'legacy'", [])
        .unwrap();
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    // The copy now carries the 4 legacy requests: still 11, never 7.
    assert_all_viewers(&[&a, &clone, &viewer], 11, "0.110000");
    for device in [&a, &clone, &viewer] {
        assert_eq!(legacy_group(device), Some((4, "0.040000".to_string())));
    }
}

/// A → B → C, each a copied installation that took a new identity; hide,
/// restore and retire the middle device; the root keeps working.
#[tokio::test]
#[serial]
async fn lineage_chain_survives_hidden_restored_and_retired_middle_devices() {
    let cloud = remote();
    let a = Device::new("A");
    let v1 = Device::new("V1");
    let v2 = Device::new("V2");
    seed_with_legacy(&a);
    a.sync(&cloud).await;
    let b = Device::new("B");
    copy_installation(&a, &b);
    a.log("a-1", "proxy", days_ago(0) - 900, "m", 10, 1, "0.01", 200);
    fork_after_conflict(&cloud, &a, &b).await;
    let c = Device::new("C");
    copy_installation(&b, &c);
    b.log("b-1", "proxy", days_ago(0) - 800, "m", 10, 1, "0.01", 200);
    fork_after_conflict(&cloud, &b, &c).await;
    c.log("c-1", "proxy", days_ago(0) - 700, "m", 10, 1, "0.01", 200);
    let all = [&a, &b, &c, &v1, &v2];
    sync_all(&cloud, &all).await;
    // 10 seeded + a-1 + b-1 + c-1.
    assert_all_viewers(&all, 13, "0.130000");
    {
        let conn = c.db.conn.lock().unwrap();
        let identity = store::load_or_create_identity(&conn).unwrap();
        assert_eq!(identity.ancestors, vec![b.id(), a.id()]);
    }

    // Hide the middle device on two viewers: its own b-1 leaves, history stays once.
    for viewer in [&v1, &v2] {
        let conn = viewer.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), true).unwrap();
    }
    assert_all_viewers(&[&v1, &v2], 12, "0.120000");
    for viewer in [&v1, &v2] {
        assert_eq!(legacy_group(viewer), Some((4, "0.040000".to_string())));
    }
    // Restore it.
    for viewer in [&v1, &v2] {
        let conn = viewer.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), false).unwrap();
    }
    sync_all(&cloud, &all).await;
    assert_all_viewers(&all, 13, "0.130000");

    // The root keeps producing requests.
    a.log("a-2", "proxy", days_ago(0) - 600, "m", 10, 1, "0.01", 200);
    sync_all(&cloud, &all).await;
    assert_all_viewers(&all, 14, "0.140000");

    // Retire the root on every viewer: the chain's next holder carries history.
    for viewer in [&b, &c, &v1, &v2] {
        let conn = viewer.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &a.id(), true).unwrap();
    }
    // a-1 and a-2 lived only on A.
    assert_all_viewers(&[&b, &c, &v1, &v2], 12, "0.120000");

    // The middle device's directory disappears (deleted remotely): C carries it.
    for key in cloud.keys() {
        if key.contains(&b.id()) {
            cloud.delete(&key).unwrap();
        }
    }
    for viewer in [&v1, &v2] {
        let conn = viewer.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &b.id(), true).unwrap();
    }
    sync_all(&cloud, &[&c, &v1, &v2]).await;
    // b-1 leaves with B; seeded history (10) and c-1 remain once.
    assert_all_viewers(&[&v1, &v2], 11, "0.110000");
}

#[test]
fn malformed_or_cyclic_ancestry_is_rejected() {
    use protocol::is_valid_ancestry;
    let me = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let p = "1f8fad5b-d9cb-469f-a165-70867728950e";
    let q = "2f8fad5b-d9cb-469f-a165-70867728950e";
    assert!(is_valid_ancestry(me, p, &[p.into(), q.into()]));
    assert!(is_valid_ancestry(me, p, &[]));
    assert!(
        !is_valid_ancestry(me, p, &[q.into(), p.into()]),
        "must start with parent"
    );
    assert!(
        !is_valid_ancestry(me, p, &[p.into(), me.into()]),
        "cycle through self"
    );
    assert!(
        !is_valid_ancestry(me, p, &[p.into(), q.into(), q.into()]),
        "repeat"
    );
    assert!(
        !is_valid_ancestry(me, p, &[p.into(), "../x".into()]),
        "bad id"
    );
    let long: Vec<String> = (0..=protocol::MAX_LINEAGE_DEPTH)
        .map(|i| format!("{i:08x}-d9cb-469f-a165-70867728950e"))
        .collect();
    assert!(!is_valid_ancestry(me, &long[0], &long), "too deep");
}

#[tokio::test]
#[serial]
async fn consecutive_resets_keep_the_published_ancestor_and_mid_publish_extends_it() {
    let cloud = remote();
    let a = Device::new("A");
    seed_with_legacy(&a);
    a.sync(&cloud).await;
    let b = Device::new("B");
    copy_installation(&a, &b);
    a.log(
        "a-after",
        "proxy",
        days_ago(0) - 600,
        "m",
        10,
        1,
        "0.01",
        200,
    );
    a.sync(&cloud).await;
    assert!(b.sync(&cloud).await.identity_conflict);
    let reset = |device: &Device| {
        let conn = device.db.conn.lock().unwrap();
        store::reset_identity(&conn).unwrap()
    };
    let first = reset(&b);
    let second = reset(&b); // `first` never published
    assert_eq!(second.ancestors, vec![a.id()]);
    assert_ne!(first.device_id, second.device_id);
    // Publish, then reset again: now the published identity is the parent.
    b.sync(&cloud).await;
    let third = reset(&b);
    assert_eq!(third.ancestors, vec![second.device_id.clone(), a.id()]);
    let viewer = Device::new("Viewer");
    sync_all(&cloud, &[&a, &b, &viewer]).await;
    // second's directory still holds the copied history too: counted once.
    assert_all_viewers(&[&a, &b, &viewer], 11, "0.110000");
}

// ─── R2: heuristic aliases (P3-R2-4) ─────────────────────────

fn published_aliases(cloud: &MemoryStore, device: &Device) -> Vec<String> {
    let manifest = read_manifest(cloud, &device.id());
    let mut ids = Vec::new();
    for meta in manifest.partitions.values() {
        let bytes = cloud
            .get(
                &protocol::object_path(&device.id(), &meta.object),
                usize::MAX,
            )
            .unwrap()
            .unwrap();
        let segment = protocol::decode_segment(&bytes, meta).unwrap();
        ids.extend(segment.aliases.into_iter().map(|a| a.request_id));
        ids.extend(
            segment
                .ledger
                .into_iter()
                .filter(|l| l.kind == protocol::LEDGER_KIND_ALIAS)
                .map(|l| l.request_id),
        );
    }
    ids.sort();
    ids
}

#[tokio::test]
async fn ambiguous_heuristic_pairings_stay_local_and_never_hide_other_devices_requests() {
    for rolled_up in [false, true] {
        let cloud = remote();
        let a = Device::new("A");
        let b = Device::new("B");
        let c = Device::new("C");
        let at = days_ago(40);
        // On A two proxy calls look exactly like the session line (same tokens,
        // model, inside the window): the local heuristic drops the session row,
        // but it cannot know which call it was, or whether it was either.
        a.log("proxy-1", "proxy", at, "m", 100, 10, "0.05", 200);
        a.log("proxy-2", "proxy", at + 30, "m", 100, 10, "0.05", 200);
        a.log(
            "session:s",
            "session_log",
            at + 10,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        b.log(
            "session:s",
            "session_log",
            at + 10,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        assert_eq!(a.total_requests(UsageDeviceScope::Local), 2);
        if rolled_up {
            a.db.rollup_and_prune(30).unwrap();
            let kind: i64 =
                a.db.conn
                    .lock()
                    .unwrap()
                    .query_row(
                        "SELECT kind FROM usage_rollup_ledger WHERE request_id = 'session:s'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
            assert_eq!(kind, 2, "ambiguous duplicate is local-only");
        }
        sync_all(&cloud, &[&a, &b, &c]).await;
        assert!(
            published_aliases(&cloud, &a).is_empty(),
            "no alias published"
        );
        // B's session row is not suppressed by A's guess: the pair is counted
        // as "not detectable" (both A's calls and B's session line).
        assert_all_viewers(&[&a, &b, &c], 3, "0.150000");
    }
}

#[tokio::test]
async fn one_session_matching_two_proxy_or_two_sessions_matching_one_proxy_are_ambiguous() {
    let cloud = remote();
    let a = Device::new("A");
    let at = days_ago(5);
    // One proxy call, two indistinguishable session lines.
    a.log("proxy-x", "proxy", at, "m", 100, 10, "0.05", 200);
    a.log(
        "session:x1",
        "session_log",
        at + 5,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    a.log(
        "session:x2",
        "session_log",
        at + 6,
        "m",
        100,
        10,
        "0.05",
        200,
    );
    a.sync(&cloud).await;
    assert!(published_aliases(&cloud, &a).is_empty());
}

#[tokio::test]
async fn unambiguous_pairing_is_published_and_counted_once_fleet_wide() {
    for rolled_up in [false, true] {
        let cloud = remote();
        let a = Device::new("A");
        let b = Device::new("B");
        let c = Device::new("C");
        let at = days_ago(40);
        a.log("proxy-1", "proxy", at, "m", 100, 10, "0.05", 200);
        a.log(
            "session:s",
            "session_log",
            at + 5,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        b.log(
            "session:s",
            "session_log",
            at + 5,
            "m",
            100,
            10,
            "0.05",
            200,
        );
        if rolled_up {
            a.db.rollup_and_prune(30).unwrap();
        }
        sync_all(&cloud, &[&a, &b, &c]).await;
        assert_eq!(published_aliases(&cloud, &a), vec!["session:s".to_string()]);
        assert_all_viewers(&[&a, &b, &c], 1, "0.050000");
    }
}

// ─── R2: incremental export (P3-R2-2) ────────────────────────

/// The published manifest must always equal what a full, uncached export
/// would publish (same partitions, same content hashes).
fn assert_manifest_is_full_export(device: &Device, cloud: &MemoryStore) {
    let device_id = device.id();
    let manifest = read_manifest(cloud, &device_id);
    let full = {
        let conn = device.db.conn.lock().unwrap();
        store::export_local(&conn, &device_id, device.detail_level).unwrap()
    };
    let expected: std::collections::BTreeMap<String, String> = full
        .partitions
        .iter()
        .map(|(key, segment)| {
            (
                key.clone(),
                protocol::encode_segment(segment).unwrap().content_sha256,
            )
        })
        .collect();
    let published: std::collections::BTreeMap<String, String> = manifest
        .partitions
        .iter()
        .map(|(key, meta)| (key.clone(), meta.content_sha256.clone()))
        .collect();
    assert_eq!(published, expected);
    assert_eq!(manifest.coverage, full.coverage);
}

fn seed_three_months(device: &Device) {
    for (i, days) in [100i64, 70, 40].into_iter().enumerate() {
        for j in 0..3 {
            device.log(
                &format!("m{i}-{j}"),
                "proxy",
                days_ago(days) + j * 60,
                "model-x",
                100,
                10,
                "0.10",
                200,
            );
        }
    }
    device.db.rollup_and_prune(30).unwrap();
}

fn month_count(device: &Device) -> usize {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row(
        "SELECT COUNT(DISTINCT substr(date, 1, 7)) FROM usage_daily_rollups",
        [],
        |r| r.get::<_, i64>(0),
    )
    .unwrap() as usize
}

fn exec(device: &Device, sql: &str) {
    device.db.conn.lock().unwrap().execute_batch(sql).unwrap();
}

#[tokio::test]
async fn export_cache_skips_unchanged_months_and_catches_every_change() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    seed_three_months(&a);
    let months = month_count(&a);
    assert!(months >= 2, "history spans several months");

    let first = a.sync(&cloud).await;
    assert_eq!(first.cached_partitions, 0, "nothing cached on first sync");
    assert_manifest_is_full_export(&a, &cloud);
    let again = a.sync(&cloud).await;
    assert_eq!(again.cached_partitions, months);
    assert_eq!(again.uploaded_partitions, 0);
    assert_manifest_is_full_export(&a, &cloud);
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 9);

    let oldest: String =
        a.db.conn
            .lock()
            .unwrap()
            .query_row("SELECT MIN(date) FROM usage_daily_rollups", [], |r| {
                r.get(0)
            })
            .unwrap();
    let oldest_month = &oldest[..7];

    // 1. Same-row-count price correction with no timestamp change.
    exec(
        &a,
        &format!(
            "UPDATE usage_daily_rollups SET total_cost_usd = '0.90'
             WHERE substr(date, 1, 7) = '{oldest_month}'"
        ),
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, months - 1);
    assert_eq!(report.uploaded_partitions, 1);
    assert_manifest_is_full_export(&a, &cloud);
    b.sync(&cloud).await;
    assert_eq!(
        b.summary(UsageDeviceScope::All).total_cost,
        "1.500000",
        "0.90 + 2 × 0.30"
    );

    // 2. Ledger-only correction (the rollup row itself is untouched).
    exec(
        &a,
        &format!(
            "UPDATE usage_rollup_ledger SET total_cost_usd = '0.11'
             WHERE substr(date, 1, 7) = '{oldest_month}'"
        ),
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, months - 1);
    assert_manifest_is_full_export(&a, &cloud);

    // 3. Delete + identical rebuild (Codex-style): re-read, nothing uploaded.
    exec(
        &a,
        &format!(
            "CREATE TEMP TABLE keep AS SELECT * FROM usage_daily_rollups
                WHERE substr(date, 1, 7) = '{oldest_month}';
             DELETE FROM usage_daily_rollups WHERE substr(date, 1, 7) = '{oldest_month}';
             INSERT INTO usage_daily_rollups SELECT * FROM keep;
             DROP TABLE keep;"
        ),
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, months - 1);
    assert_eq!(report.uploaded_partitions, 0, "identical content");
    assert_manifest_is_full_export(&a, &cloud);

    // 4. Deleting a whole month removes its partition everywhere.
    exec(
        &a,
        &format!(
            "DELETE FROM usage_daily_rollups WHERE substr(date, 1, 7) = '{oldest_month}';
             DELETE FROM usage_rollup_ledger WHERE substr(date, 1, 7) = '{oldest_month}';"
        ),
    );
    a.sync(&cloud).await;
    assert_manifest_is_full_export(&a, &cloud);
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 6);

    // 5. A provider rename changes every partition's provider name.
    exec(
        &a,
        "INSERT INTO providers (id, app_type, name, settings_config, meta)
         VALUES ('prov-1', 'claude', 'Renamed', '{}', '{}')",
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, 0);
    assert_manifest_is_full_export(&a, &cloud);
    // Unrelated provider updates (sort order) keep the cache.
    exec(
        &a,
        "UPDATE providers SET sort_index = 3 WHERE id = 'prov-1'",
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, months - 1);
    b.sync(&cloud).await;
    let providers =
        b.db.get_provider_stats_scoped(None, None, None, None, None, &UsageDeviceScope::All)
            .unwrap();
    assert!(providers.iter().all(|p| p.provider_name == "Renamed"));

    // 6. A remote manifest that no longer references the cached content
    //    (lost objects, restored remote) forces a full re-export.
    {
        let manifest = read_manifest(&cloud, &a.id());
        let mut objects = cloud.objects.lock().unwrap();
        for meta in manifest.partitions.values() {
            objects.retain(|path, _| !path.ends_with(&meta.object));
        }
    }
    exec(
        &a,
        "UPDATE usage_sync_export_cache SET content_sha256 = 'stale'",
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, 0);
    assert_manifest_is_full_export(&a, &cloud);

    // 7. Switching the detail level never reuses entries of the other level.
    let mut daily = Device::new("A");
    daily.db = a.db.clone();
    daily.detail_level = UsageSyncDetailLevel::Daily;
    let report = daily.sync(&cloud).await;
    assert_eq!(report.cached_partitions, 0);
    assert_manifest_is_full_export(&daily, &cloud);

    // 8. A missing version trigger disables the cache entirely.
    daily.sync(&cloud).await;
    exec(
        &a,
        "DROP TRIGGER usage_sync_version_usage_daily_rollups_upd",
    );
    exec(&a, "UPDATE usage_daily_rollups SET total_cost_usd = '0.77'");
    let report = daily.sync(&cloud).await;
    assert_eq!(report.cached_partitions, 0);
    assert_manifest_is_full_export(&daily, &cloud);
}

#[tokio::test]
async fn export_cache_follows_inherited_history_and_late_rollups() {
    let cloud = remote();
    let a = Device::new("A");
    seed_three_months(&a);
    a.sync(&cloud).await;
    // Inherited rows (identity reset snapshot) belong to month partitions too.
    exec(
        &a,
        "INSERT INTO usage_sync_inherited (date, app_type, provider_id, model,
                request_model, pricing_model, request_count, success_count, input_tokens,
                output_tokens, cache_read_tokens, cache_creation_tokens, total_cost_usd)
             SELECT date, app_type, provider_id, model, request_model, pricing_model, 1, 1,
                    1, 1, 0, 0, '0.01'
             FROM usage_daily_rollups WHERE date = (SELECT MAX(date) FROM usage_daily_rollups)",
    );
    let report = a.sync(&cloud).await;
    assert_eq!(report.cached_partitions, month_count(&a) - 1);
    assert_manifest_is_full_export(&a, &cloud);
    // A late rollup into an existing month (offline device catching up).
    a.log("late", "proxy", days_ago(41), "model-x", 1, 1, "0.01", 200);
    a.db.rollup_and_prune(30).unwrap();
    a.sync(&cloud).await;
    assert_manifest_is_full_export(&a, &cloud);
    let again = a.sync(&cloud).await;
    assert_eq!(again.cached_partitions, month_count(&a));
}

/// Bulk history: `rows` rolled-up requests spread over the last 360 days
/// (ledger + matching rollup groups), written through the version triggers.
fn seed_bulk_history(device: &Device, rows: i64) {
    let conn = device.db.conn.lock().unwrap();
    conn.execute(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i + 1 < ?1)
         INSERT INTO usage_rollup_ledger (request_id, data_source, kind, date, app_type,
            provider_id, model, success, input_tokens, output_tokens, total_cost_usd,
            rolled_up_at)
         SELECT printf('req-%08d-0123456789abcdef', i), 'proxy', 0,
                date('now', 'localtime', '-' || (31 + i % 330) || ' days'), 'claude',
                'prov-' || (i % 7), 'model-' || (i % 11), 1, 1000, 200, '0.0123', 0
         FROM n",
        [rows],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count,
            success_count, input_tokens, output_tokens, input_token_semantics,
            total_cost_usd, avg_latency_ms)
         SELECT date, app_type, provider_id, model, COUNT(*), COUNT(*), SUM(input_tokens),
                SUM(output_tokens), 2, CAST(COUNT(*) * 0.0123 AS TEXT), 900
         FROM usage_rollup_ledger GROUP BY date, app_type, provider_id, model;",
    )
    .unwrap();
}

/// Database-lock time of the export step exactly as `upload_phase` runs it.
fn timed_export_lock(device: &Device, cloud: &MemoryStore) -> (std::time::Duration, usize) {
    let device_id = device.id();
    let remote = protocol::parse_manifest(
        &cloud
            .get(&manifest_path(&device_id), usize::MAX)
            .unwrap()
            .unwrap_or_default(),
    )
    .map(|m| m.partitions)
    .unwrap_or_default();
    let conn = device.db.conn.lock().unwrap();
    let started = std::time::Instant::now();
    let skip = store::plan_export_skip(&conn, &device_id, device.detail_level, &remote).unwrap();
    let export = store::export_local_except(&conn, &device_id, device.detail_level, &skip).unwrap();
    (started.elapsed(), export.partitions.len())
}

/// `USAGE_SYNC_PERF_ROWS` (default 300000) rolled-up requests on one device:
/// trigger overhead on bulk writes, cold vs. cached export lock time and
/// sync time, and the cost of a single historic price correction.
#[tokio::test]
#[ignore]
async fn perf_incremental_export() {
    let rows: i64 = std::env::var("USAGE_SYNC_PERF_ROWS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300_000);
    let cloud = remote();
    let a = Device::new("A");
    let t = std::time::Instant::now();
    seed_bulk_history(&a, rows);
    let seed_with = t.elapsed();
    let plain = Device::new("Plain");
    exec(
        &plain,
        "DROP TRIGGER usage_sync_version_usage_rollup_ledger_ins;
         DROP TRIGGER usage_sync_version_usage_daily_rollups_ins;",
    );
    let t = std::time::Instant::now();
    seed_bulk_history(&plain, rows);
    let seed_without = t.elapsed();

    let (cold_lock, parts) = timed_export_lock(&a, &cloud);
    let t = std::time::Instant::now();
    let first = a.sync(&cloud).await;
    let cold_sync = t.elapsed();
    let (warm_lock, warm_parts) = timed_export_lock(&a, &cloud);
    let t = std::time::Instant::now();
    let warm = a.sync(&cloud).await;
    let warm_sync = t.elapsed();
    assert_eq!(warm.cached_partitions, parts);
    assert_eq!(warm_parts, 0);

    exec(
        &a,
        "UPDATE usage_rollup_ledger SET total_cost_usd = '0.0200'
         WHERE date = (SELECT MIN(date) FROM usage_rollup_ledger)",
    );
    let (one_lock, one_parts) = timed_export_lock(&a, &cloud);
    let t = std::time::Instant::now();
    let one = a.sync(&cloud).await;
    let one_sync = t.elapsed();
    assert_eq!(one_parts, 1);
    assert_eq!(one.uploaded_partitions, 1);
    assert_manifest_is_full_export(&a, &cloud);
    let remote_bytes: usize = cloud.objects.lock().unwrap().values().map(Vec::len).sum();
    println!(
        "rows={rows} month partitions={parts} remote bytes={remote_bytes}\n\
         bulk insert with version triggers={seed_with:?} without={seed_without:?}\n\
         cold: export lock={cold_lock:?} sync={cold_sync:?} uploaded={}\n\
         unchanged: export lock={warm_lock:?} sync={warm_sync:?} cached={}\n\
         one-month price correction: export lock={one_lock:?} sync={one_sync:?} cached={}",
        first.uploaded_partitions, warm.cached_partitions, one.cached_partitions
    );
}

#[tokio::test]
async fn export_by_month_ranges_ignores_malformed_dates_without_overlap() {
    let cloud = remote();
    let a = Device::new("A");
    seed_three_months(&a);
    let before = {
        let conn = a.db.conn.lock().unwrap();
        store::export_local(&conn, &a.id_unlocked(&conn), a.detail_level).unwrap()
    };
    exec(
        &a,
        "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count)
         VALUES ('', 'claude', 'p', 'm', 1), ('2026', 'claude', 'p', 'm', 1),
                ('20xx-01-01', 'claude', 'p', 'm', 1), ('garbage', 'claude', 'p', 'm', 1);",
    );
    let after = {
        let conn = a.db.conn.lock().unwrap();
        store::export_local(&conn, &a.id_unlocked(&conn), a.detail_level).unwrap()
    };
    let shas = |export: &store::LocalExport| -> Vec<(String, String)> {
        export
            .partitions
            .iter()
            .map(|(k, seg)| {
                (
                    k.clone(),
                    protocol::encode_segment(seg).unwrap().content_sha256,
                )
            })
            .collect()
    };
    assert_eq!(shas(&before), shas(&after));
    a.sync(&cloud).await;
    assert_manifest_is_full_export(&a, &cloud);
}

// ─── Review: retired objects must stay inside the own parts directory ──

#[test]
fn manifest_with_foreign_or_traversing_retired_objects_is_rejected() {
    let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
    let mut manifest = DeviceManifest {
        format: protocol::USAGE_SYNC_FORMAT.into(),
        version: protocol::USAGE_PROTOCOL_VERSION,
        device_id: id.into(),
        device_name: "x".into(),
        platform: String::new(),
        app_version: String::new(),
        seq: 1,
        writer_nonce: "n".into(),
        updated_at: 0,
        detail_level: protocol::DETAIL_LEVEL_RECORDS.into(),
        utc_offset_minutes: 0,
        coverage: Default::default(),
        partitions: Default::default(),
        retired: vec![protocol::RetiredObject {
            object: protocol::object_name("m-2026-01", &"a".repeat(64)),
            retired_at: 0,
        }],
        lineage: None,
    };
    assert!(protocol::validate_manifest(&manifest, id).is_ok());
    for bad in [
        "../../../../v2/db-v6/default/db.sql",
        "parts/../../other-device/manifest.json",
        "parts/m-2026-01-aaaaaaaaaaaaaaaa.json.gz/../../x",
        "parts/notes.txt",
        "manifest.json",
        "/parts/m-2026-01-aaaaaaaaaaaaaaaa.json.gz",
        "",
    ] {
        manifest.retired[0].object = bad.into();
        assert!(
            protocol::validate_manifest(&manifest, id).is_err(),
            "{bad:?} accepted"
        );
    }
    manifest.retired[0].object = protocol::object_name("m-2026-01", &"a".repeat(64));
    manifest.retired[0].retired_at = -1;
    assert!(protocol::validate_manifest(&manifest, id).is_err());
    manifest.retired[0].retired_at = 0;
    manifest.retired = vec![manifest.retired[0].clone(); protocol::MAX_RETIRED_OBJECTS + 1];
    assert!(protocol::validate_manifest(&manifest, id).is_err());
}

#[tokio::test]
async fn tampered_retired_entries_never_delete_outside_the_parts_directory() {
    let cloud = remote();
    let mut a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let id = a.id();

    // Victims at the literal keys a traversing entry would address (the
    // memory store does not normalize, so a DELETE would hit these exactly).
    let traversal = "../../../../v2/db-v6/default/db.sql";
    let victims = [
        protocol::object_path(&id, traversal),
        protocol::object_path(&id, "parts/notes.txt"),
        protocol::object_path(&id, "manifest.json.bak"),
    ];
    for victim in &victims {
        cloud.put(victim, b"keep".to_vec()).unwrap();
    }

    // Keep the valid writer nonce, inject expired foreign entries.
    let mut manifest = read_manifest(&cloud, &id);
    for object in [traversal, "parts/notes.txt", "manifest.json.bak"] {
        manifest.retired.push(protocol::RetiredObject {
            object: object.into(),
            retired_at: 0,
        });
    }
    cloud
        .put(&manifest_path(&id), serde_json::to_vec(&manifest).unwrap())
        .unwrap();

    a.now += protocol::RETIRED_GRACE_SECS + 1;
    a.log("a2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    let report = a.sync(&cloud).await;
    assert!(report.manifest_written, "the tampered manifest is replaced");
    for victim in &victims {
        assert!(cloud.keys().contains(victim), "{victim} was deleted");
    }
    let rewritten = read_manifest(&cloud, &id);
    assert!(rewritten
        .retired
        .iter()
        .all(|r| protocol::partition_of_object(&r.object).is_some()));
    assert!(protocol::validate_manifest(&rewritten, &id).is_ok());
}

#[tokio::test]
async fn transport_refuses_paths_that_could_leave_the_namespace() {
    let cloud = remote();
    let transport = UsageTransport::Memory(cloud.clone());
    cloud.put("a/../b", b"x".to_vec()).unwrap();
    for rel in [
        "a/../b", "../b", "a//b", "./a", "/a", "a\\b", "", "a/\u{0}b",
    ] {
        assert!(transport.delete(rel).await.is_err(), "delete {rel:?}");
        assert!(transport.get(rel, 16).await.is_err(), "get {rel:?}");
        assert!(
            transport.put(rel, b"y".to_vec(), "x").await.is_err(),
            "put {rel:?}"
        );
    }
    assert!(cloud.keys().contains(&"a/../b".to_string()));
}

// ─── Review: remote namespaces are isolated (backend / location / profile) ──

fn listed_devices(device: &Device) -> Vec<(String, String)> {
    let conn = device.db.conn.lock().unwrap();
    store::list_remote_devices(&conn)
        .unwrap()
        .into_iter()
        .map(|d| (d.device_id, d.status))
        .collect()
}

#[tokio::test]
async fn switching_namespaces_a_b_a_keeps_writer_state_and_never_mixes_caches() {
    let space_a = remote();
    let space_b = remote();
    let me = Device::new("Me");
    let x = Device::new("X"); // only in namespace A
    let y = Device::new("Y"); // only in namespace B
    let z = Device::new("Z"); // only in namespace A, hidden by me
    me.log("me-1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    x.log("x-1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    y.log("y-1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    z.log("z-1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    x.sync(&space_a).await;
    z.sync(&space_a).await;
    y.sync(&space_b).await;

    // A: me + X (Z hidden).
    me.sync(&space_a).await;
    {
        let conn = me.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &z.id(), true).unwrap();
    }
    assert_eq!(me.total_requests(UsageDeviceScope::All), 2);

    // B: me + Y only; A's devices are gone from the cache and the list.
    let report = me.sync(&space_b).await;
    assert!(!report.identity_conflict, "{report:?}");
    assert!(report.manifest_written);
    assert_eq!(me.total_requests(UsageDeviceScope::All), 2);
    let listed = listed_devices(&me);
    assert!(listed.iter().all(|(id, _)| id != &x.id() && id != &z.id()));
    assert!(listed.iter().any(|(id, _)| id == &y.id()));

    // Back to A: no false identity conflict, uploads continue, Z still hidden.
    me.log("me-2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    let report = me.sync(&space_a).await;
    assert!(!report.identity_conflict, "{report:?}");
    assert!(report.upload_error.is_none(), "{report:?}");
    assert!(report.manifest_written);
    assert_eq!(me.total_requests(UsageDeviceScope::All), 3, "me ×2 + X");
    let listed = listed_devices(&me);
    assert!(listed.iter().all(|(id, _)| id != &y.id()));
    assert!(listed
        .iter()
        .any(|(id, status)| id == &z.id() && status == "hidden"));
    // Viewers of A see my new request: the manifest really advanced.
    x.sync(&space_a).await;
    assert_eq!(
        x.total_requests(UsageDeviceScope::All),
        4,
        "me ×2 + X + Z (X does not hide Z)"
    );

    // And B again: still no conflict, sequence continues from B's own state.
    let before = read_manifest(&space_b, &me.id()).seq;
    let report = me.sync(&space_b).await;
    assert!(!report.identity_conflict, "{report:?}");
    assert!(read_manifest(&space_b, &me.id()).seq > before);
}

#[tokio::test]
#[serial]
async fn a_real_second_writer_is_still_detected_within_one_namespace() {
    let cloud = remote();
    let other = remote();
    let a = Device::new("A");
    a.log("a1", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    a.sync(&cloud).await;
    let copy = Device::new("Copy");
    clone_identity(&a, &copy);
    copy.log("c1", "proxy", NOW - 50, "m", 1, 1, "0.01", 200);
    copy.sync(&cloud).await;
    // A visits another namespace and comes back: the copy's write is still
    // a conflict, not hidden by the namespace switch.
    a.sync(&other).await;
    let report = a.sync(&cloud).await;
    assert!(report.identity_conflict, "{report:?}");
}

#[test]
fn namespace_key_tracks_location_not_credentials() {
    use crate::settings::{S3SyncSettings, WebDavSyncSettings};
    let dav = |url: &str, user: &str, pass: &str, root: &str, profile: &str| {
        UsageTransport::from_webdav(&WebDavSyncSettings {
            base_url: url.into(),
            username: user.into(),
            password: pass.into(),
            remote_root: root.into(),
            profile: profile.into(),
            ..WebDavSyncSettings::default()
        })
        .unwrap()
        .namespace_key()
    };
    let base = dav(
        "https://dav.example.com/remote.php/dav",
        "u",
        "p1",
        "sync",
        "default",
    );
    assert_eq!(
        base,
        dav(
            "https://DAV.example.com:443/remote.php/dav/",
            "other",
            "p2",
            "sync",
            "default"
        ),
        "credentials, host case, default port and trailing slash do not matter"
    );
    assert!(!base.contains("example") && base.len() == 64);
    for other in [
        dav(
            "https://dav.example.com/remote.php/dav",
            "u",
            "p1",
            "sync",
            "work",
        ),
        dav(
            "https://dav.example.com/remote.php/dav",
            "u",
            "p1",
            "other",
            "default",
        ),
        dav(
            "https://nas.example.com/remote.php/dav",
            "u",
            "p1",
            "sync",
            "default",
        ),
    ] {
        assert_ne!(base, other);
    }
    let s3 = |bucket: &str, ak: &str, profile: &str| {
        UsageTransport::from_s3(&S3SyncSettings {
            region: "us-east-1".into(),
            bucket: bucket.into(),
            access_key_id: ak.into(),
            secret_access_key: format!("{ak}-secret"),
            endpoint: "https://s3.example.com".into(),
            remote_root: "sync".into(),
            profile: profile.into(),
            ..S3SyncSettings::default()
        })
        .unwrap()
        .namespace_key()
    };
    assert_eq!(s3("b1", "AK1", "default"), s3("b1", "AK2", "default"));
    assert_ne!(s3("b1", "AK1", "default"), s3("b2", "AK1", "default"));
    assert_ne!(s3("b1", "AK1", "default"), s3("b1", "AK1", "work"));
    assert_ne!(s3("b1", "AK1", "default"), base);
}

// ─── Review: hiding or restoring a device while its data downloads ──────

fn remote_rows_of(viewer: &Device, device_id: &str) -> i64 {
    let conn = viewer.db.conn.lock().unwrap();
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM usage_sync_remote_logs WHERE device_id = ?1)
              + (SELECT COUNT(*) FROM usage_sync_remote_partitions WHERE device_id = ?1)",
        [device_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// D has two day partitions cached on V; D then changes only one of them.
/// Returns the object name of the changed partition.
async fn two_partitions_one_changed(cloud: &Arc<MemoryStore>, d: &Device, v: &Device) -> String {
    d.log("d-old", "proxy", NOW - 3 * 86_400, "m", 1, 1, "0.01", 200);
    d.log("d-new", "proxy", NOW - 60, "m", 1, 1, "0.01", 200);
    d.sync(cloud).await;
    v.sync(cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 2);
    let before = read_manifest(cloud, &d.id()).partitions;
    assert_eq!(before.len(), 2);
    d.log("d-new-2", "proxy", NOW - 30, "m", 1, 1, "0.01", 200);
    d.sync(cloud).await;
    let after = read_manifest(cloud, &d.id()).partitions;
    let changed: Vec<_> = after
        .iter()
        .filter(|(key, meta)| before.get(*key).map(|m| &m.sha256) != Some(&meta.sha256))
        .collect();
    assert_eq!(changed.len(), 1, "only one partition changed");
    changed[0].1.object.clone()
}

#[tokio::test]
async fn hiding_a_device_during_its_download_is_not_overwritten() {
    let cloud = remote();
    let d = Device::new("D");
    let v = Device::new("V");
    let changed_object = two_partitions_one_changed(&cloud, &d, &v).await;

    let (db, id) = (v.db.clone(), d.id());
    *cloud.before_get.lock().unwrap() = Some((
        changed_object,
        Box::new(move || {
            let conn = db.conn.lock().unwrap();
            store::set_device_hidden(&conn, &id, true).unwrap();
        }),
    ));
    v.sync(&cloud).await;
    assert!(cloud.before_get.lock().unwrap().is_none(), "hook ran");

    assert_eq!(v.remote_device(&d.id()).unwrap().status, "hidden");
    assert_eq!(remote_rows_of(&v, &d.id()), 0, "nothing re-inserted");
    assert_eq!(v.total_requests(UsageDeviceScope::All), 0);
    // Staying hidden across later syncs.
    v.sync(&cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 0);

    // Restoring imports both partitions again, including the one that was
    // not re-downloaded when the hide happened.
    {
        let conn = v.db.conn.lock().unwrap();
        store::set_device_hidden(&conn, &d.id(), false).unwrap();
    }
    v.sync(&cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 3);
    v.sync(&cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 3);
}

#[tokio::test]
async fn hide_and_restore_during_a_download_never_loses_unchanged_partitions() {
    let cloud = remote();
    let d = Device::new("D");
    let v = Device::new("V");
    let changed_object = two_partitions_one_changed(&cloud, &d, &v).await;

    let (db, id) = (v.db.clone(), d.id());
    *cloud.before_get.lock().unwrap() = Some((
        changed_object,
        Box::new(move || {
            let conn = db.conn.lock().unwrap();
            store::set_device_hidden(&conn, &id, true).unwrap();
            store::set_device_hidden(&conn, &id, false).unwrap();
        }),
    ));
    v.sync(&cloud).await;
    assert!(cloud.before_get.lock().unwrap().is_none(), "hook ran");
    // The stale plan (keep the unchanged partition, fetch one) was dropped:
    // no partial cache that a later "unchanged manifest" would freeze.
    let cached = v.remote_device(&d.id()).unwrap();
    assert_eq!(cached.status, "ok");
    v.sync(&cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 3);
    v.sync(&cloud).await;
    assert_eq!(v.total_requests(UsageDeviceScope::All), 3);
}

// ─── R4 (maintainer review at 845addf5) ──────────────────────

fn month_version(device: &Device, key: &str) -> i64 {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row(
        "SELECT COALESCE((SELECT version FROM usage_sync_partition_versions
                          WHERE partition_key = ?1), 0)",
        [key],
        |r| r.get(0),
    )
    .unwrap()
}

/// Two local-noon timestamps on consecutive days of one calendar month that
/// lies entirely past the 30-day detail window.
fn two_days_in_one_old_month() -> (i64, i64, String) {
    use chrono::{Datelike, Local, TimeZone};
    let base = Local::now() - chrono::Duration::days(75);
    let day = |d: u32| {
        Local
            .with_ymd_and_hms(base.year(), base.month(), d, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp()
    };
    (
        day(10),
        day(11),
        format!("m-{:04}-{:02}", base.year(), base.month()),
    )
}

#[test]
fn month_version_counts_up_under_every_write_policy() {
    // The maintainer's reproducer on the real tables and triggers: an
    // aggregate `INSERT OR REPLACE` and a ledger `INSERT OR IGNORE` on two
    // days of one month must keep counting up (it used to stay at 2).
    let a = Device::new("A");
    let version = || month_version(&a, "m-2026-09");
    let insert_rollup = |date: &str| {
        exec(
            &a,
            &format!(
                "INSERT OR REPLACE INTO usage_daily_rollups (date, app_type, provider_id, model,
                    request_count) VALUES ('{date}', 'claude', 'p', 'm', 1)"
            ),
        )
    };
    let insert_ledger = |id: &str, date: &str| {
        exec(
            &a,
            &format!(
                "INSERT OR IGNORE INTO usage_rollup_ledger (request_id, data_source, date,
                    app_type, provider_id, model, rolled_up_at)
                 VALUES ('{id}', 'proxy', '{date}', 'claude', 'p', 'm', 0)"
            ),
        )
    };
    insert_rollup("2026-09-01");
    insert_ledger("a", "2026-09-01");
    let after_first = version();
    insert_rollup("2026-09-02");
    insert_ledger("b", "2026-09-02");
    let after_second = version();
    assert!(after_second > after_first, "{after_first} → {after_second}");

    // Every other statement shape that writes these tables also bumps.
    let mut last = after_second;
    for sql in [
        "INSERT OR REPLACE INTO usage_daily_rollups (date, app_type, provider_id, model,
            request_count) VALUES ('2026-09-01', 'claude', 'p', 'm', 2)",
        "UPDATE usage_daily_rollups SET total_cost_usd = '0.5' WHERE date = '2026-09-01'",
        "UPDATE OR REPLACE usage_daily_rollups SET request_count = 3 WHERE date = '2026-09-02'",
        "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count)
            VALUES ('2026-09-03', 'claude', 'p', 'm', 1)
            ON CONFLICT DO UPDATE SET request_count = request_count + 1",
        "INSERT INTO usage_daily_rollups (date, app_type, provider_id, model, request_count)
            VALUES ('2026-09-03', 'claude', 'p', 'm', 1)
            ON CONFLICT DO UPDATE SET request_count = request_count + 1",
        "INSERT OR ABORT INTO usage_rollup_ledger (request_id, data_source, date, app_type,
            provider_id, model, rolled_up_at) VALUES ('c', 'proxy', '2026-09-04', 'claude',
            'p', 'm', 0)",
        "INSERT OR FAIL INTO usage_sync_inherited (date, app_type, provider_id, model)
            VALUES ('2026-09-05', 'claude', 'p', 'm')",
        "DELETE FROM usage_rollup_ledger WHERE request_id = 'a'",
    ] {
        exec(&a, sql);
        let now = version();
        assert!(
            now > last,
            "version did not count up after {sql}: {last} → {now}"
        );
        last = now;
    }
}

#[tokio::test]
async fn two_equal_rollups_in_one_month_are_both_published() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let (day1, day2, month) = two_days_in_one_old_month();

    // Day 1: one request, rolled up, published and seen by B.
    a.log("r1", "proxy", day1, "m", 100, 10, "0.10", 200);
    a.db.rollup_and_prune(30).unwrap();
    a.sync(&cloud).await;
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 1);
    let v1 = month_version(&a, &month);

    // Day 2 of the same month: same row count as day 1. First published as a
    // detail (day) partition, then rolled up: the day partition goes away and
    // the month partition must carry the request instead.
    a.log("r2", "proxy", day2, "m", 100, 10, "0.10", 200);
    a.sync(&cloud).await;
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 2);
    let day_key = {
        let conn = a.db.conn.lock().unwrap();
        conn.query_row(
            "SELECT 'd-' || date(?1, 'unixepoch', 'localtime')",
            [day2],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    };
    assert!(read_manifest(&cloud, &a.id())
        .partitions
        .contains_key(&day_key));

    a.db.rollup_and_prune(30).unwrap();
    let v2 = month_version(&a, &month);
    assert!(v2 > v1, "month version must count up: {v1} → {v2}");
    let report = a.sync(&cloud).await;
    assert!(report.uploaded_partitions >= 1, "{report:?}");
    assert_manifest_is_full_export(&a, &cloud);
    let manifest = read_manifest(&cloud, &a.id());
    assert!(
        !manifest.partitions.contains_key(&day_key),
        "pruned day partition retired"
    );
    assert!(manifest.partitions.contains_key(&month));

    b.sync(&cloud).await;
    assert_eq!(
        b.total_requests(UsageDeviceScope::All),
        2,
        "not lost, not doubled"
    );
    assert_eq!(a.total_requests(UsageDeviceScope::All), 2);

    // A third equal rollup in the same month keeps counting.
    let day3 = day2 + 86_400;
    a.log("r3", "proxy", day3, "m", 100, 10, "0.10", 200);
    a.db.rollup_and_prune(30).unwrap();
    assert!(month_version(&a, &month) > v2);
    a.sync(&cloud).await;
    assert_manifest_is_full_export(&a, &cloud);
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 3);
}

/// The trigger body shipped up to 845addf5 (INSERT OR IGNORE + UPDATE).
fn install_pre_r4_version_triggers(device: &Device) {
    let conn = device.db.conn.lock().unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'usage_sync_version_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(names.len(), 12);
    let bump = |key: &str| {
        format!(
            "INSERT OR IGNORE INTO usage_sync_partition_versions (partition_key, version)
                VALUES ({key}, 0);
             UPDATE usage_sync_partition_versions SET version = version + 1
                WHERE partition_key = {key};"
        )
    };
    let mut sql = String::new();
    for name in &names {
        sql.push_str(&format!("DROP TRIGGER \"{name}\";"));
    }
    for table in [
        "usage_daily_rollups",
        "usage_rollup_ledger",
        "usage_sync_inherited",
    ] {
        let (n, o) = (
            "'m-' || substr(NEW.date, 1, 7)",
            "'m-' || substr(OLD.date, 1, 7)",
        );
        sql.push_str(&format!(
            "CREATE TRIGGER usage_sync_version_{table}_ins AFTER INSERT ON {table} BEGIN {} END;
             CREATE TRIGGER usage_sync_version_{table}_upd AFTER UPDATE ON {table} BEGIN {} {} END;
             CREATE TRIGGER usage_sync_version_{table}_del AFTER DELETE ON {table} BEGIN {} END;",
            bump(n),
            bump(n),
            bump(o),
            bump(o)
        ));
    }
    for (suffix, event) in [
        ("ins", "INSERT"),
        ("upd", "UPDATE OF id, app_type, name"),
        ("del", "DELETE"),
    ] {
        sql.push_str(&format!(
            "CREATE TRIGGER usage_sync_version_providers_{suffix} AFTER {event} ON providers
             BEGIN {} END;",
            bump("'*names'")
        ));
    }
    conn.execute_batch(&sql).unwrap();
}

fn trigger_state(device: &Device) -> (i64, i64) {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row(
        "SELECT COUNT(*), SUM(instr(sql, ?1) > 0) FROM sqlite_master
         WHERE type = 'trigger' AND name LIKE 'usage_sync_version_%'",
        [crate::database::VERSION_TRIGGER_MARKER],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

fn export_cache_rows(device: &Device) -> i64 {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM usage_sync_export_cache", [], |r| {
        r.get(0)
    })
    .unwrap()
}

#[tokio::test]
async fn existing_v21_databases_get_repaired_triggers_and_a_one_time_cache_reset() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    let (day1, day2, month) = two_days_in_one_old_month();

    // A v21 database from before the fix: old trigger body, populated cache.
    install_pre_r4_version_triggers(&a);
    assert_eq!(trigger_state(&a), (12, 0));
    a.log("r1", "proxy", day1, "m", 100, 10, "0.10", 200);
    a.db.rollup_and_prune(30).unwrap();
    a.sync(&cloud).await;
    // While old triggers are installed the export cache is not trusted.
    assert_eq!(a.sync(&cloud).await.cached_partitions, 0);
    exec(
        &a,
        "INSERT OR REPLACE INTO usage_sync_export_cache
            (partition_key, device_id, detail_level, version, names_version, content_sha256)
         VALUES ('m-1999-01', 'x', 'records', 1, 0, 'stale')",
    );
    assert!(export_cache_rows(&a) > 0);

    // Re-opening the database (startup runs create_usage_sync_tables).
    {
        let conn = a.db.conn.lock().unwrap();
        Database::create_usage_sync_tables(&conn).unwrap();
    }
    assert_eq!(trigger_state(&a), (12, 12), "all triggers replaced");
    assert_eq!(export_cache_rows(&a), 0, "cache forgotten once");

    // Idempotent: a later start changes nothing and keeps the cache.
    a.sync(&cloud).await;
    let cached = export_cache_rows(&a);
    assert!(cached > 0);
    {
        let conn = a.db.conn.lock().unwrap();
        Database::create_usage_sync_tables(&conn).unwrap();
    }
    assert_eq!(trigger_state(&a), (12, 12));
    assert_eq!(export_cache_rows(&a), cached, "no reset on a normal start");
    assert_eq!(a.sync(&cloud).await.cached_partitions, month_count(&a));

    // The repaired database now handles the equal-rollup case correctly.
    let v1 = month_version(&a, &month);
    a.log("r2", "proxy", day2, "m", 100, 10, "0.10", 200);
    a.db.rollup_and_prune(30).unwrap();
    assert!(month_version(&a, &month) > v1);
    a.sync(&cloud).await;
    assert_manifest_is_full_export(&a, &cloud);
    b.sync(&cloud).await;
    assert_eq!(b.total_requests(UsageDeviceScope::All), 2);
}

fn ledger_rows(device: &Device) -> i64 {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM usage_rollup_ledger", [], |r| r.get(0))
        .unwrap()
}

fn detail_rows(device: &Device) -> i64 {
    let conn = device.db.conn.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM proxy_request_logs", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn rollup_without_usage_sync_writes_no_ledger_and_keeps_local_statistics() {
    let enabled = Device::new("Enabled");
    let disabled = Device::new("Disabled");
    disabled.db.set_usage_sync_enabled_for_test(false);
    for device in [&enabled, &disabled] {
        for i in 0..20 {
            device.log(
                &format!("r{i}"),
                if i % 4 == 0 { "session_log" } else { "proxy" },
                days_ago(40 + i % 3) + i * 7,
                if i % 2 == 0 { "m1" } else { "m2" },
                100 + i,
                10 + i,
                "0.01",
                if i % 5 == 0 { 500 } else { 200 },
            );
        }
        device.log("recent", "proxy", days_ago(2), "m1", 1, 1, "0.01", 200);
    }
    let before = disabled.summary(UsageDeviceScope::Local);
    for device in [&enabled, &disabled] {
        device.db.rollup_and_prune(30).unwrap();
    }
    // Plain rollup behaviour is unchanged: old details pruned, recent kept.
    assert_eq!(detail_rows(&disabled), 1);
    assert_eq!(detail_rows(&enabled), 1);
    assert_eq!(
        ledger_rows(&disabled),
        0,
        "no identities without usage sync"
    );
    assert_eq!(ledger_rows(&enabled), 20);
    // Local statistics are identical either way, and equal to the pre-rollup view.
    let after_disabled = disabled.summary(UsageDeviceScope::Local);
    let after_enabled = enabled.summary(UsageDeviceScope::Local);
    for after in [&after_disabled, &after_enabled] {
        assert_eq!(after.total_requests, before.total_requests);
        assert_eq!(after.total_cost, before.total_cost);
        assert_eq!(after.total_input_tokens, before.total_input_tokens);
        assert_eq!(after.total_output_tokens, before.total_output_tokens);
        assert_eq!(after.success_rate, before.success_rate);
    }
    let models = |d: &Device| {
        d.db.get_model_stats_scoped(None, None, None, None, None, &UsageDeviceScope::Local)
            .unwrap()
            .into_iter()
            .map(|m| (m.model, m.request_count))
            .collect::<Vec<_>>()
    };
    assert_eq!(models(&disabled), models(&enabled));
    // No cross-device state was derived for the disabled device.
    let conn = disabled.db.conn.lock().unwrap();
    for table in [
        "usage_sync_ledger_suppressed",
        "usage_sync_inherited_effective",
    ] {
        let rows: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "{table}");
    }
}

#[test]
#[serial]
fn ledger_follows_the_device_usage_sync_setting_including_manual_mode() {
    use crate::settings::{get_usage_sync_settings, set_usage_sync_settings};
    let original = get_usage_sync_settings();
    let a = Device::new("A");
    // Use the real device setting, not the per-test stand-in.
    *a.db.usage_sync_enabled_override.lock().unwrap() = None;
    let rollup_one = |id: &str, days: i64| {
        a.log(id, "proxy", days_ago(days), "m", 1, 1, "0.01", 200);
        a.db.rollup_and_prune(30).unwrap();
    };
    let mut settings = original.clone();

    settings.enabled = false;
    set_usage_sync_settings(settings.clone()).unwrap();
    rollup_one("off", 40);
    assert_eq!(ledger_rows(&a), 0);

    // Manual mode (enabled, no automatic sync) still records identities.
    settings.enabled = true;
    settings.auto_sync = false;
    set_usage_sync_settings(settings.clone()).unwrap();
    rollup_one("manual", 41);
    assert_eq!(ledger_rows(&a), 1);

    settings.auto_sync = true;
    set_usage_sync_settings(settings.clone()).unwrap();
    rollup_one("auto", 42);
    assert_eq!(ledger_rows(&a), 2);

    // Disabling again stops new identities; existing ones stay until retention.
    settings.enabled = false;
    set_usage_sync_settings(settings.clone()).unwrap();
    rollup_one("off-again", 43);
    assert_eq!(ledger_rows(&a), 2);

    set_usage_sync_settings(original).unwrap();
}

#[tokio::test]
async fn history_rolled_up_while_disabled_is_reported_not_faked_as_exact() {
    let cloud = remote();
    let a = Device::new("A");
    let b = Device::new("B");
    // The same session record on both devices.
    for device in [&a, &b] {
        device.log(
            "session:shared-1",
            "session_log",
            days_ago(40),
            "m",
            100,
            10,
            "0.05",
            200,
        );
    }
    // A had usage sync off while its copy was rolled up; B had it on.
    a.db.set_usage_sync_enabled_for_test(false);
    a.db.rollup_and_prune(30).unwrap();
    b.db.rollup_and_prune(30).unwrap();
    assert_eq!(ledger_rows(&a), 0);
    assert_eq!(ledger_rows(&b), 1);

    // A enables usage sync afterwards: no identity is invented for history
    // rolled up meanwhile, so the shared request cannot be proven to be the
    // same one and is counted on both devices and reported as such.
    a.db.set_usage_sync_enabled_for_test(true);
    a.log(
        "session:shared-2",
        "session_log",
        days_ago(35),
        "m",
        100,
        10,
        "0.05",
        200,
    );
    b.log(
        "session:shared-2",
        "session_log",
        days_ago(35),
        "m",
        100,
        10,
        "0.05",
        200,
    );
    for device in [&a, &b] {
        device.db.rollup_and_prune(30).unwrap();
    }
    assert_eq!(
        ledger_rows(&a),
        1,
        "only the request rolled up after enabling"
    );
    sync_all(&cloud, &[&a, &b]).await;
    for viewer in [&a, &b] {
        // shared-1: no identity on A → 2; shared-2: identities on both → 1.
        assert_eq!(
            viewer.total_requests(UsageDeviceScope::All),
            3,
            "{}",
            viewer.name
        );
    }
    let local = {
        let conn = a.db.conn.lock().unwrap();
        store::local_unverified(&conn).unwrap()
    };
    assert_eq!(local.requests, 1, "A reports its id-less history");
    assert_eq!(b.remote_device(&a.id()).unwrap().unverified.requests, 1);
}

/// Independent R4 review: a copy of a published installation with usage sync
/// switched off rolls up its own new requests; enabling sync later and taking
/// a new identity must not attribute them to the parent and drop them.
async fn copy_then_rollup_with_sync_setting(clone_enabled: bool) -> (u64, i64) {
    let cloud = remote();
    let a = Device::new("Original");
    let viewer = Device::new("Viewer");
    seed_history(&a); // 6 requests, 5 rolled up with ledger
    a.sync(&cloud).await;
    let clone = Device::new("Clone");
    copy_installation(&a, &clone);
    clone.db.set_usage_sync_enabled_for_test(clone_enabled);
    // Both devices work after the copy, same day and group, then those days
    // age past the 30-day window and are rolled up.
    let day = days_ago(35);
    a.log("a-post", "proxy", day, "m", 10, 1, "0.01", 200);
    clone.log("c-post", "proxy", day + 10, "m", 10, 1, "0.01", 200);
    a.db.rollup_and_prune(30).unwrap();
    clone.db.rollup_and_prune(30).unwrap();
    // The clone enables usage sync, hits the identity conflict and forks.
    clone.db.set_usage_sync_enabled_for_test(true);
    fork_after_conflict(&cloud, &a, &clone).await;
    sync_all(&cloud, &[&a, &clone, &viewer]).await;
    let unverified = {
        let conn = clone.db.conn.lock().unwrap();
        store::local_unverified(&conn).unwrap().requests
    };
    (all_summary(&viewer).total_requests, unverified)
}

#[tokio::test]
#[serial]
async fn copied_published_installation_with_sync_off_keeps_its_own_history() {
    let enabled = copy_then_rollup_with_sync_setting(true).await;
    let disabled = copy_then_rollup_with_sync_setting(false).await;
    // 6 seed + one request on the original + one on the copy after the copy.
    assert_eq!(enabled.0, 8);
    assert_eq!(
        disabled.0, 8,
        "copy's own request lost; unverified={}",
        disabled.1
    );
}

#[tokio::test]
async fn identities_keep_being_recorded_only_where_the_identity_already_published() {
    let cloud = remote();
    let published = Device::new("Published");
    let never = Device::new("Never");
    published.log("p0", "proxy", days_ago(40), "m", 1, 1, "0.01", 200);
    published.db.rollup_and_prune(30).unwrap();
    published.sync(&cloud).await;
    assert_eq!(ledger_rows(&published), 1);

    // Both switch usage sync off.
    published.db.set_usage_sync_enabled_for_test(false);
    never.db.set_usage_sync_enabled_for_test(false);
    for device in [&published, &never] {
        device.log("x1", "proxy", days_ago(41), "m", 1, 1, "0.01", 200);
        device.db.rollup_and_prune(30).unwrap();
    }
    assert_eq!(
        ledger_rows(&published),
        2,
        "a published identity keeps its ledger complete"
    );
    assert_eq!(ledger_rows(&never), 0, "never published: no identities");
    // The never-published device did not even create an identity row.
    let identities: i64 = never
        .db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM usage_sync_identity", [], |r| r.get(0))
        .unwrap();
    assert_eq!(identities, 0);
}
