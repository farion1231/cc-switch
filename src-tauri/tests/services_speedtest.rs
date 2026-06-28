use cc_switch_lib::{EndpointLatency, SpeedtestService};

#[path = "support.rs"]
mod support;

fn run_speedtest(urls: Vec<String>, timeout_secs: Option<u64>) -> Vec<EndpointLatency> {
    tauri::async_runtime::block_on(SpeedtestService::test_endpoints(urls, timeout_secs))
        .expect("test_endpoints should not return Err for input validation cases")
}

#[test]
fn test_endpoints_empty_list_returns_empty_vec() {
    let result = run_speedtest(vec![], None);
    assert!(result.is_empty(), "empty input should produce empty output");
}

#[test]
fn test_endpoints_empty_string_url_reports_error() {
    let result = run_speedtest(vec!["".to_string()], None);
    assert_eq!(result.len(), 1, "one input -> one result");
    let entry = &result[0];
    assert!(entry.latency.is_none(), "empty URL should have no latency");
    assert!(entry.status.is_none(), "empty URL should have no status");
    assert_eq!(
        entry.error.as_deref(),
        Some("URL 不能为空"),
        "empty URL should report 'URL 不能为空'"
    );
}

#[test]
fn test_endpoints_whitespace_url_reports_empty_url_error() {
    let result = run_speedtest(vec!["   ".to_string()], None);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].error.as_deref(),
        Some("URL 不能为空"),
        "whitespace-only URL should be treated as empty"
    );
}

#[test]
fn test_endpoints_invalid_url_reports_parse_error() {
    let result = run_speedtest(vec!["not-a-url".to_string()], None);
    assert_eq!(result.len(), 1);
    let error = result[0].error.as_deref().unwrap_or_default();
    assert!(
        error.starts_with("URL 无效"),
        "invalid URL should start with 'URL 无效', got: {error}"
    );
}

#[test]
fn test_endpoints_invalid_url_has_no_latency_or_status() {
    let result = run_speedtest(vec!["totally::invalid://url".to_string()], None);
    assert_eq!(result.len(), 1);
    assert!(
        result[0].latency.is_none(),
        "invalid URL should have no latency"
    );
    assert!(
        result[0].status.is_none(),
        "invalid URL should have no status"
    );
    assert!(
        result[0].error.is_some(),
        "invalid URL should have an error"
    );
}

#[test]
fn test_endpoints_multiple_invalid_urls_each_get_error() {
    let urls = vec!["".to_string(), "bad url".to_string(), "  ".to_string()];
    let result = run_speedtest(urls, None);
    assert_eq!(result.len(), 3, "three inputs -> three results");
    for entry in &result {
        assert!(entry.error.is_some(), "all invalid URLs should have errors");
        assert!(
            entry.latency.is_none(),
            "all invalid URLs should have no latency"
        );
    }
}

#[test]
fn test_endpoints_result_order_matches_input_order() {
    let urls = vec!["bad-1".to_string(), "".to_string(), "bad-2".to_string()];
    let result = run_speedtest(urls, None);
    assert_eq!(result.len(), 3);
    assert!(
        result[0]
            .error
            .as_deref()
            .unwrap_or("")
            .starts_with("URL 无效"),
        "first result should be the invalid url error, got: {:?}",
        result[0].error
    );
    assert_eq!(
        result[1].error.as_deref(),
        Some("URL 不能为空"),
        "second result should be the empty url error"
    );
    assert!(
        result[2]
            .error
            .as_deref()
            .unwrap_or("")
            .starts_with("URL 无效"),
        "third result should be invalid url error, got: {:?}",
        result[2].error
    );
}

#[test]
fn endpoint_latency_struct_fields_are_accessible() {
    let entry = EndpointLatency {
        url: "https://example.com".to_string(),
        latency: Some(100),
        status: Some(200),
        error: None,
    };
    assert_eq!(entry.url, "https://example.com");
    assert_eq!(entry.latency, Some(100));
    assert_eq!(entry.status, Some(200));
    assert!(entry.error.is_none());
}

#[test]
fn endpoint_latency_error_variant_has_no_latency_or_status() {
    let entry = EndpointLatency {
        url: "https://failed.example".to_string(),
        latency: None,
        status: None,
        error: Some("connection refused".to_string()),
    };
    assert!(entry.latency.is_none(), "error entry has no latency");
    assert!(entry.status.is_none(), "error entry has no status");
    assert!(entry.error.is_some(), "error entry has error message");
}

#[test]
fn test_endpoints_with_custom_timeout_does_not_panic() {
    let result = tauri::async_runtime::block_on(SpeedtestService::test_endpoints(vec![], Some(5)))
        .expect("should succeed with custom timeout");
    assert!(
        result.is_empty(),
        "empty input with custom timeout still returns empty vec"
    );
}

#[test]
fn test_endpoints_url_field_in_result_matches_trimmed_input() {
    let result = run_speedtest(vec!["  bad-url  ".to_string()], None);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].url, "bad-url",
        "url in result should be the trimmed version of the input"
    );
}

#[test]
fn test_endpoints_empty_url_field_in_result_is_original_input() {
    let result = run_speedtest(vec!["".to_string()], None);
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].url, "",
        "url field for empty input should be empty string"
    );
}
