use super::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn provider(id: &str) -> Provider {
    Provider::with_id(id.into(), id.into(), json!({"env": {"key": "test"}}), None)
}

#[test]
fn provider_list_batches_endpoints_without_crossing_apps_or_changing_order() {
    let db = Database::memory().unwrap();
    let mut first = provider("z");
    first.sort_index = Some(0);
    let mut second = provider("a");
    second.sort_index = Some(1);
    db.save_provider("claude", &second).unwrap();
    db.save_provider("claude", &first).unwrap();
    db.save_provider("codex", &first).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        // Old metadata is not authoritative, even for a provider with no endpoints.
        conn.execute(
            "UPDATE providers SET meta = ?1 WHERE app_type = 'claude' AND id = 'a'",
            [json!({"custom_endpoints": {"stale": {"url": "stale", "addedAt": 1}}}).to_string()],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO provider_endpoints (provider_id, app_type, url, added_at) VALUES
             ('z', 'claude', 'https://shared', 20),
             ('z', 'claude', 'https://shared', 10),
             ('z', 'claude', 'https://null', NULL),
             ('z', 'codex', 'https://other-app', 30);",
        )
        .unwrap();
    }
    let providers = db.get_all_providers("claude").unwrap();
    assert_eq!(
        providers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["z", "a"]
    );
    let endpoints = &providers["z"].meta.as_ref().unwrap().custom_endpoints;
    assert_eq!(endpoints.len(), 2);
    assert_eq!(endpoints["https://shared"].added_at, 20);
    assert_eq!(endpoints["https://shared"].url, "https://shared");
    assert_eq!(endpoints["https://null"].added_at, 0);
    assert_eq!(endpoints["https://shared"].last_used, None);
    assert!(providers["a"]
        .meta
        .as_ref()
        .unwrap()
        .custom_endpoints
        .is_empty());
    assert!(db.get_all_providers("gemini").unwrap().is_empty());
    assert_eq!(
        db.get_all_providers("codex").unwrap()["z"]
            .meta
            .as_ref()
            .unwrap()
            .custom_endpoints
            .len(),
        1
    );
}

#[test]
fn unchanged_provider_save_does_not_emit_a_database_change() {
    let db = Database::memory().unwrap();
    let mut provider = provider("same");
    db.save_provider("claude", &provider).unwrap();
    db.add_custom_endpoint("claude", "same", "https://endpoint")
        .unwrap();
    {
        let conn = db.conn.lock().unwrap();
        conn.execute(
            "UPDATE providers SET is_current = 1, in_failover_queue = 1",
            [],
        )
        .unwrap();
    }
    let updates = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&updates);
    db.conn.lock().unwrap().update_hook(Some(
        move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            if action == rusqlite::hooks::Action::SQLITE_UPDATE && table == "providers" {
                count.fetch_add(1, Ordering::Relaxed);
            }
        },
    ));

    for _ in 0..5 {
        db.save_provider("claude", &provider).unwrap();
    }
    assert_eq!(updates.load(Ordering::Relaxed), 0);

    // NULL <-> value transitions and normal edits must still be persisted.
    provider.notes = Some("updated".into());
    db.save_provider("claude", &provider).unwrap();
    assert_eq!(
        db.get_provider_by_id("same", "claude")
            .unwrap()
            .unwrap()
            .notes,
        provider.notes
    );
    provider.notes = None;
    provider.name = "renamed".into();
    provider.settings_config = json!({"env": {"key": "changed"}});
    db.save_provider("claude", &provider).unwrap();
    db.save_provider("claude", &provider).unwrap();
    assert_eq!(updates.load(Ordering::Relaxed), 2);
    let loaded = db
        .get_all_providers("claude")
        .unwrap()
        .swap_remove("same")
        .unwrap();
    assert_eq!(loaded.name, "renamed");
    assert_eq!(loaded.notes, None);
    assert_eq!(loaded.settings_config, provider.settings_config);
    assert!(loaded.in_failover_queue);
    assert_eq!(loaded.meta.unwrap().custom_endpoints.len(), 1);
    // Check the stored flag without consulting the device-local current pointer.
    let is_current: bool = db
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT is_current FROM providers WHERE id = 'same' AND app_type = 'claude'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(is_current);
}

#[test]
fn unchanged_metadata_maps_do_not_emit_database_changes_after_reload() {
    let db = Database::memory().unwrap();
    let mut provider = provider("metadata");
    provider.meta = Some(
        serde_json::from_value(json!({
            "localProxyRequestOverrides": {
                "headers": {"X-First": "one", "X-Second": "two", "X-Third": "three"},
                "body": {"reasoning": {"effort": "high"}}
            }
        }))
        .unwrap(),
    );
    db.save_provider("claude", &provider).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        let stored: String = conn
            .query_row(
                "SELECT meta FROM providers WHERE id = 'metadata'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut reordered: serde_json::Value = serde_json::from_str(&stored).unwrap();
        let headers = reordered["localProxyRequestOverrides"]["headers"]
            .as_object_mut()
            .unwrap();
        let reversed = headers
            .iter()
            .rev()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        *headers = reversed;
        let reordered = reordered.to_string();
        assert_ne!(stored, reordered);
        // HashMap serialization order is unstable across reads/devices. Make
        // the first equivalent-but-differently-ordered save deterministic.
        conn.execute(
            "UPDATE providers SET meta = ?1 WHERE id = 'metadata'",
            [reordered],
        )
        .unwrap();
    }
    let updates = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&updates);
    db.conn.lock().unwrap().update_hook(Some(
        move |action: rusqlite::hooks::Action, _: &str, table: &str, _: i64| {
            if action == rusqlite::hooks::Action::SQLITE_UPDATE && table == "providers" {
                count.fetch_add(1, Ordering::Relaxed);
            }
        },
    ));
    db.save_provider("claude", &provider).unwrap();
    for _ in 0..5 {
        provider = db
            .get_provider_by_id("metadata", "claude")
            .unwrap()
            .unwrap();
        db.save_provider("claude", &provider).unwrap();
    }
    assert_eq!(updates.load(Ordering::Relaxed), 0);

    provider
        .meta
        .as_mut()
        .unwrap()
        .local_proxy_request_overrides
        .as_mut()
        .unwrap()
        .headers
        .insert("X-First".into(), "changed".into());
    db.save_provider("claude", &provider).unwrap();
    assert_eq!(updates.load(Ordering::Relaxed), 1);
    let loaded = db
        .get_provider_by_id("metadata", "claude")
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded
            .meta
            .unwrap()
            .local_proxy_request_overrides
            .unwrap()
            .headers["X-First"],
        "changed"
    );
}

#[test]
#[ignore = "manual performance benchmark; reports median latency without timing assertions"]
fn benchmark_provider_list() {
    for count in [10, 100, 1000] {
        let db = Database::memory().unwrap();
        for i in 0..count {
            let id = format!("provider-{i:04}");
            db.save_provider("claude", &provider(&id)).unwrap();
            for endpoint in 0..3 {
                db.add_custom_endpoint("claude", &id, &format!("https://endpoint-{endpoint}"))
                    .unwrap();
            }
        }
        let mut samples = Vec::new();
        for _ in 0..25 {
            let start = std::time::Instant::now();
            let result = db.get_all_providers("claude").unwrap();
            assert_eq!(result.len(), count);
            std::hint::black_box(result);
            samples.push(start.elapsed());
        }
        samples.sort_unstable();
        eprintln!(
            "provider_list count={count} endpoints={} median_us={}",
            count * 3,
            samples[samples.len() / 2].as_micros()
        );
    }
}
