//! Multi-device scenarios against the in-memory remote.
//!
//! Each `Device` owns an in-memory database (its "installation") and shares
//! one `MemoryStore` (the cloud). Scenarios cover idempotence, discovery,
//! concurrent uploads, interruption/corruption recovery, cross-device
//! de-duplication, rollup boundaries, cost corrections, identity conflicts
//! and version gating.

use std::sync::Arc;

use rusqlite::params;

use super::protocol::{self, manifest_path, DeviceManifest};
use super::store;
use super::transport::memory::MemoryStore;
use super::transport::UsageTransport;
use super::{sync_with, SyncOptions, UsageSyncReport};
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
    let expected_owner = std::cmp::min(b.id(), c.id());
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
    b.log("b1", "proxy", NOW - 60, "m", 100, 10, "0.10", 200);
    b.log("b2", "proxy", NOW - 30, "m", 100, 10, "0.10", 500);
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
            !raw.contains("b1") && !raw.contains("b2"),
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
    ] {
        assert!(Database::table_exists(&conn, table)?, "{table} missing");
    }
    let kept: i64 = conn.query_row(
        "SELECT COUNT(*) FROM proxy_request_logs WHERE request_id = 'keep-me'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(kept, 1, "local usage untouched by the migration");
    // Idempotent re-run.
    Database::create_usage_sync_tables(&conn)?;
    Ok(())
}
