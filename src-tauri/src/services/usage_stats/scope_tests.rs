//! Regression matrix for multi-project usage scope propagation.
//!
//! These fixtures are deliberately synthetic and are kept in a memory database.

use super::*;

fn ts(day: u32, hour: u32) -> i64 {
    chrono::NaiveDate::from_ymd_opt(2024, 1, day)
        .unwrap()
        .and_hms_opt(hour, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp()
}

fn fixture() -> Result<Database, AppError> {
    let db = Database::memory()?;
    let conn = lock_conn!(db.conn);

    // Provider identity is app-scoped, so the fixture also exercises preserving
    // that identity when the same provider id exists for two client apps.
    conn.execute(
        "INSERT INTO providers (id, app_type, name, settings_config, meta, created_at)
         VALUES ('shared-provider', 'claude', 'Provider A', '{}', '{}', 1),
                ('shared-provider', 'codex', 'Provider B', '{}', '{}', 1)",
        [],
    )?;

    let rows = [
        (
            "detail-a1",
            "claude",
            "shared-provider",
            "model-x",
            "project-a",
            10,
            2,
            "1.0",
            10,
            0,
        ),
        (
            "detail-a2",
            "claude",
            "shared-provider",
            "model-y",
            "project-a",
            20,
            4,
            "2.0",
            11,
            0,
        ),
        (
            "detail-b1",
            "claude",
            "shared-provider",
            "model-x",
            "project-b",
            30,
            6,
            "3.0",
            12,
            0,
        ),
        (
            "detail-c1",
            "codex",
            "shared-provider",
            "model-x",
            "project-c",
            40,
            8,
            "4.0",
            13,
            0,
        ),
        (
            "detail-unknown",
            "claude",
            "shared-provider",
            "model-x",
            "",
            50,
            10,
            "5.0",
            14,
            1,
        ),
    ];
    for (id, app, provider, model, project, input, output, cost, day, unknown) in rows {
        conn.execute(
            "INSERT INTO proxy_request_logs (
                request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd,
                total_cost_usd, latency_ms, status_code, created_at, data_source,
                project_key, project_name, project_path, attribution_confidence
             ) VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, 0, 0, '0', '0', '0', '0', ?7,
                       100, 200, ?8, 'proxy', ?9, ?9, '', ?10)",
            params![
                id,
                provider,
                app,
                model,
                input,
                output,
                cost,
                ts(day, 12),
                project,
                if unknown == 1 { "unknown" } else { "exact" }
            ],
        )?;
    }

    // Older days are represented only by rollups, as after detail retention.
    for (date, app, model, project, count, input, output, cost) in [
        (
            "2024-01-05",
            "claude",
            "model-x",
            "project-a",
            2,
            100,
            20,
            "10.0",
        ),
        (
            "2024-01-06",
            "codex",
            "model-x",
            "project-c",
            3,
            200,
            30,
            "20.0",
        ),
    ] {
        conn.execute(
            "INSERT INTO usage_daily_rollups (
                date, app_type, provider_id, model, request_model, pricing_model,
                data_source, project_key, project_name, project_path,
                provider_name_snapshot, request_count, success_count,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                input_token_semantics, total_cost_usd, avg_latency_ms
             ) VALUES (?1, ?2, 'shared-provider', ?3, ?3, ?3, 'proxy', ?4, ?4, '',
                       '', ?5, ?5, ?6, ?7, 0, 0, 0, ?8, 100)",
            params![date, app, model, project, count, input, output, cost],
        )?;
    }
    drop(conn);
    Ok(db)
}

fn keys(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn multi_project_scope_unions_projects_across_detail_rollup_and_aggregates() -> Result<(), AppError>
{
    let db = fixture()?;
    let selected = keys(&["project-a", "project-b"]);
    let summary =
        db.get_usage_summary(None, None, None, None, None, None, None, Some(&selected))?;

    // A contributes two details plus two rolled-up requests; B contributes one detail.
    assert_eq!(summary.total_requests, 5);
    assert_eq!(summary.total_input_tokens, 160);
    assert_eq!(summary.total_output_tokens, 32);
    assert_eq!(summary.total_cost, "16.000000");

    let project_stats =
        db.get_project_stats(None, None, None, None, None, None, None, Some(&selected))?;
    assert_eq!(
        project_stats
            .iter()
            .map(|row| row.request_count)
            .sum::<u64>(),
        5
    );
    let mut project_keys: Vec<_> = project_stats
        .iter()
        .map(|row| row.project_key.as_str())
        .collect();
    project_keys.sort_unstable();
    assert_eq!(project_keys, vec!["project-a", "project-b"]);

    let provider_stats =
        db.get_provider_stats(None, None, None, None, None, None, None, Some(&selected))?;
    assert_eq!(
        provider_stats
            .iter()
            .map(|row| row.request_count)
            .sum::<u64>(),
        5
    );
    let model_stats =
        db.get_model_stats(None, None, None, None, None, None, None, Some(&selected))?;
    assert_eq!(
        model_stats.iter().map(|row| row.request_count).sum::<u64>(),
        5
    );

    // A+B has no C rows, while None and [] both mean all projects.
    assert_eq!(
        db.get_usage_summary(None, None, None, None, None, None, None, None)?
            .total_requests,
        10
    );
    assert_eq!(
        db.get_usage_summary(None, None, None, None, None, None, None, Some(&[]))?
            .total_requests,
        10
    );
    let selected_and_model = db.get_usage_summary(
        None,
        None,
        Some("claude"),
        Some("claude"),
        Some("shared-provider"),
        None,
        Some("model-x"),
        Some(&selected),
    )?;
    assert_eq!(selected_and_model.total_requests, 4); // A detail + A rollup count + B detail.

    Ok(())
}

#[test]
fn scope_composes_with_time_app_provider_and_model_and_flows_to_trends_and_first_date(
) -> Result<(), AppError> {
    let db = fixture()?;
    let selected = keys(&["project-a", "project-b"]);
    // This range includes the project-a rollup and all in-range details.
    let start = ts(4, 0);
    let end = ts(14, 23);

    let summary = db.get_usage_summary(
        Some(start),
        Some(end),
        Some("claude"),
        Some("claude"),
        Some("shared-provider"),
        None,
        Some("model-x"),
        Some(&selected),
    )?;
    assert_eq!(summary.total_requests, 4); // A rollup + A/B details; C is excluded by app and project.
    assert_eq!(summary.total_input_tokens, 140);

    let by_app = db.get_usage_summary_by_app(
        Some(start),
        Some(end),
        Some("claude"),
        Some("shared-provider"),
        None,
        Some("model-x"),
        Some(&selected),
    )?;
    assert_eq!(by_app.len(), 1);
    assert_eq!(by_app[0].app_type, "claude");
    assert_eq!(by_app[0].summary.total_requests, 4);

    let trends = db.get_daily_trends(
        Some(start),
        Some(end),
        Some("claude"),
        Some("claude"),
        Some("shared-provider"),
        None,
        Some("model-x"),
        Some(&selected),
    )?;
    assert_eq!(trends.iter().map(|row| row.request_count).sum::<u64>(), 4);

    assert_eq!(
        db.get_first_usage_date(None, None, None, None, None, Some(&keys(&["project-a"])))?,
        Some("2024-01-05".to_string()),
    );
    assert_eq!(
        db.get_first_usage_date(
            Some("claude"),
            Some("claude"),
            Some("shared-provider"),
            None,
            Some("model-x"),
            Some(&selected)
        )?,
        Some("2024-01-05".to_string()),
    );

    Ok(())
}

#[test]
fn project_facet_and_quality_obey_their_distinct_scope_rules() -> Result<(), AppError> {
    let db = fixture()?;
    let selected = keys(&["project-a", "project-b"]);
    let projects = db.get_usage_projects(
        None,
        None,
        Some("claude"),
        None,
        None,
        None,
        Some("model-x"),
    )?;
    let mut project_keys: Vec<_> = projects
        .iter()
        .map(|row| row.project_key.as_str())
        .collect();
    project_keys.sort_unstable();
    assert_eq!(project_keys, vec!["", "project-a", "project-b"]);
    assert!(!projects.iter().any(|row| row.project_key == "project-c"));

    let scoped_quality =
        db.get_usage_data_quality(None, None, None, None, None, None, None, Some(&selected))?;
    assert_eq!(scoped_quality.total_requests, 3);
    assert_eq!(scoped_quality.unknown_provider_requests, 0);
    assert_eq!(scoped_quality.unknown_project_requests, 0);

    let unknown = keys(&[""]);
    let unknown_quality =
        db.get_usage_data_quality(None, None, None, None, None, None, None, Some(&unknown))?;
    assert_eq!(unknown_quality.total_requests, 1);
    assert_eq!(unknown_quality.unknown_project_requests, 1);
    assert_eq!(
        db.get_usage_data_quality(None, None, None, None, None, None, None, Some(&[]))?
            .total_requests,
        5
    );
    Ok(())
}

#[test]
fn request_log_total_cache_key_includes_selected_projects() -> Result<(), AppError> {
    let db = fixture()?;
    let mut filters = LogFilters {
        project_keys: Some(keys(&["project-a", "project-b"])),
        ..Default::default()
    };
    assert_eq!(db.get_request_logs(&filters, 0, 20)?.total, 3);

    filters.project_keys = Some(keys(&["project-c"]));
    assert_eq!(db.get_request_logs(&filters, 0, 20)?.total, 1);
    assert_eq!(
        db.get_request_logs(&filters, 0, 20)?.data[0].project_key,
        "project-c"
    );

    // The compatibility single key still works when the multi-select field is absent.
    filters.project_keys = None;
    filters.project_key = Some("project-a".to_string());
    assert_eq!(db.get_request_logs(&filters, 0, 20)?.total, 2);
    Ok(())
}
