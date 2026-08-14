//! Contract tests derived from Kimi Code CLI 0.34.0.

use super::kimi_oauth_api::{
    parse_windows_release, KimiClientIdentity, KimiDeviceFacts, KimiHttpMethod, KimiHttpRequest,
    KimiHttpResponse, KimiHttpTransport, KimiOAuthApiClient, KimiSleeper,
};
use super::kimi_oauth_auth::KimiOAuthError;
use futures::future::BoxFuture;
use proptest::prelude::*;
use proptest::test_runner::{RngAlgorithm, RngSeed};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default)]
struct RecordingTransport {
    requests: Mutex<Vec<KimiHttpRequest>>,
    responses: Mutex<VecDeque<Result<KimiHttpResponse, KimiOAuthError>>>,
}

impl RecordingTransport {
    fn with_responses(responses: Vec<Result<KimiHttpResponse, KimiOAuthError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
        }
    }

    fn requests(&self) -> Vec<KimiHttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl KimiHttpTransport for RecordingTransport {
    fn execute<'a>(
        &'a self,
        request: KimiHttpRequest,
    ) -> BoxFuture<'a, Result<KimiHttpResponse, KimiOAuthError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("fake response queue exhausted")
        })
    }
}

#[derive(Default)]
struct RecordingSleeper {
    durations: Mutex<Vec<Duration>>,
}

impl KimiSleeper for RecordingSleeper {
    fn sleep<'a>(&'a self, duration: Duration) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.durations.lock().unwrap().push(duration);
        })
    }
}

fn identity() -> KimiClientIdentity {
    KimiClientIdentity::from_facts(
        "11111111-2222-4333-8444-555555555555",
        KimiDeviceFacts {
            device_name: "build-host".to_string(),
            device_model: "Linux 6.12.0 x64".to_string(),
            os_version: "6.12.0".to_string(),
        },
    )
}

fn json_response(status: u16, value: serde_json::Value) -> KimiHttpResponse {
    KimiHttpResponse {
        status,
        body: serde_json::to_vec(&value).unwrap(),
    }
}

fn raw_response(status: u16, body: &str) -> KimiHttpResponse {
    KimiHttpResponse {
        status,
        body: body.as_bytes().to_vec(),
    }
}

fn header(request: &KimiHttpRequest, name: &str) -> Option<String> {
    request
        .headers
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

#[test]
fn identity_matches_kimi_code_cli_0340() {
    let identity = identity();
    assert_eq!(identity.user_agent(), "kimi-code-cli/0.34.0");
    assert_eq!(
        identity.header_value("X-Msh-Platform"),
        Some("kimi_code_cli")
    );
    assert_eq!(identity.header_value("X-Msh-Version"), Some("0.34.0"));
    assert_eq!(
        identity.header_value("X-Msh-Device-Name"),
        Some("build-host")
    );
    assert_eq!(
        identity.header_value("X-Msh-Device-Model"),
        Some("Linux 6.12.0 x64")
    );
    assert_eq!(identity.header_value("X-Msh-Os-Version"), Some("6.12.0"));
    assert_eq!(
        identity.header_value("X-Msh-Device-Id"),
        Some("11111111-2222-4333-8444-555555555555")
    );
}

#[test]
fn linux_device_model_matches_kimi_code_cli_0340() {
    let facts = KimiDeviceFacts::from_platform("build-host", "linux", "6.12.0", "x86_64", None);

    assert_eq!(facts.device_name, "build-host");
    assert_eq!(facts.device_model, "Linux 6.12.0 x64");
    assert_eq!(facts.os_version, "6.12.0");
}

#[test]
fn macos_and_windows_device_models_match_kimi_code_cli_0340() {
    let macos = KimiDeviceFacts::from_platform("mac", "macos", "23.6.0", "aarch64", Some("14.6.1"));
    let windows =
        KimiDeviceFacts::from_platform("windows", "windows", "10.0.26100", "x86_64", None);

    assert_eq!(macos.device_model, "macOS 14.6.1 arm64");
    assert_eq!(macos.os_version, "23.6.0");
    assert_eq!(windows.device_model, "Windows 10.0.26100 x64");
    assert_eq!(windows.os_version, "10.0.26100");
}

#[test]
fn windows_ver_output_yields_the_node_compatible_os_release() {
    assert_eq!(
        parse_windows_release("Microsoft Windows [Version 10.0.26100.4652]\r\n"),
        Some("10.0.26100.4652".to_string())
    );
    assert_eq!(
        parse_windows_release("Microsoft Windows [Version 6.3.9600]"),
        Some("6.3.9600".to_string())
    );
    assert_eq!(parse_windows_release("Microsoft Windows"), None);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        rng_algorithm: RngAlgorithm::ChaCha,
        rng_seed: RngSeed::Fixed(0x4b49_4d49),
        .. ProptestConfig::default()
    })]

    #[test]
    fn identity_values_are_always_safe_ascii(value in ".{0,128}") {
        let sanitized = KimiClientIdentity::sanitize_header_value(&value, "unknown");
        prop_assert!(!sanitized.is_empty());
        prop_assert!(sanitized.chars().all(|character| character == ' ' || character.is_ascii_graphic()));
    }

    #[test]
    fn utilization_is_finite_and_clamped(used in any::<f64>(), limit in any::<f64>()) {
        let utilization = KimiOAuthApiClient::utilization_percent(used, limit);
        prop_assert!(utilization.is_finite());
        prop_assert!((0.0..=100.0).contains(&utilization));
    }
}

#[tokio::test]
async fn device_authorization_matches_cli_wire_contract() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({
            "device_code": "device-code",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://auth.kimi.com/device",
            "verification_uri_complete": "https://auth.kimi.com/device?code=ABCD-EFGH",
            "expires_in": 900,
            "interval": 5
        }),
    ))]));
    let client = KimiOAuthApiClient::new(transport.clone(), Arc::new(RecordingSleeper::default()));

    let response = client
        .request_device_authorization(&identity())
        .await
        .unwrap();

    assert_eq!(response.device_code, "device-code");
    let requests = transport.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, KimiHttpMethod::Post);
    assert_eq!(
        request.url,
        "https://auth.kimi.com/api/oauth/device_authorization"
    );
    assert_eq!(
        request.form,
        vec![(
            "client_id".to_string(),
            "17e5f671-d194-4dfb-9706-5516cb48c098".to_string()
        )]
    );
    assert_eq!(
        header(request, "User-Agent").as_deref(),
        Some("kimi-code-cli/0.34.0")
    );
    assert_eq!(
        header(request, "X-Msh-Platform").as_deref(),
        Some("kimi_code_cli")
    );
    assert_eq!(
        header(request, "Accept").as_deref(),
        Some("application/json")
    );
    assert_eq!(
        header(request, "Content-Type").as_deref(),
        Some("application/x-www-form-urlencoded")
    );
    assert!(!request.form.iter().any(|(name, _)| name == "scope"));
}

#[tokio::test]
async fn device_authorization_accepts_verification_uri_without_complete() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({
            "device_code": "device-code",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://auth.kimi.com/device",
            "expires_in": 900,
            "interval": 5
        }),
    ))]));
    let client = KimiOAuthApiClient::new(transport, Arc::new(RecordingSleeper::default()));

    let response = client
        .request_device_authorization(&identity())
        .await
        .unwrap();

    assert_eq!(response.device_code, "device-code");
    assert_eq!(response.user_code, "ABCD-EFGH");
    assert_eq!(response.verification_uri, "https://auth.kimi.com/device");
    assert!(response.verification_uri_complete.is_empty());
}

#[tokio::test]
async fn device_authorization_rejects_missing_verification_uri() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({
            "device_code": "device-code",
            "user_code": "ABCD-EFGH",
            "expires_in": 900,
            "interval": 5
        }),
    ))]));
    let client = KimiOAuthApiClient::new(transport, Arc::new(RecordingSleeper::default()));

    let error = client
        .request_device_authorization(&identity())
        .await
        .expect_err("device authorization without a verification URI must fail");
    assert!(matches!(error, KimiOAuthError::ParseError(_)));
}

#[tokio::test]
async fn refresh_retries_network_and_retryable_statuses_with_cli_backoff() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![
        Ok(json_response(
            503,
            serde_json::json!({"error":"temporarily_unavailable"}),
        )),
        Err(KimiOAuthError::NetworkError("connection reset".to_string())),
        Ok(json_response(
            200,
            serde_json::json!({
                "access_token":"new-access",
                "refresh_token":"new-refresh",
                "expires_in":900
            }),
        )),
    ]));
    let sleeper = Arc::new(RecordingSleeper::default());
    let client = KimiOAuthApiClient::new(transport.clone(), sleeper.clone());

    let tokens = client
        .refresh_access_token(&identity(), "old-refresh")
        .await
        .unwrap();

    assert_eq!(tokens.access_token, "new-access");
    assert_eq!(transport.requests().len(), 3);
    assert_eq!(
        *sleeper.durations.lock().unwrap(),
        vec![Duration::from_secs(1), Duration::from_secs(2)]
    );
}

#[tokio::test]
async fn refresh_retries_retryable_status_with_non_json_body() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![
        Ok(raw_response(503, "upstream temporarily unavailable")),
        Ok(json_response(
            200,
            serde_json::json!({
                "access_token":"new-access",
                "refresh_token":"new-refresh",
                "expires_in":900
            }),
        )),
    ]));
    let sleeper = Arc::new(RecordingSleeper::default());
    let client = KimiOAuthApiClient::new(transport.clone(), sleeper.clone());

    let tokens = client
        .refresh_access_token(&identity(), "old-refresh")
        .await
        .unwrap();

    assert_eq!(tokens.access_token, "new-access");
    assert_eq!(transport.requests().len(), 2);
    assert_eq!(
        *sleeper.durations.lock().unwrap(),
        vec![Duration::from_secs(1)]
    );
}

#[tokio::test]
async fn profile_and_usage_send_identity_headers() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![
        Ok(json_response(
            200,
            serde_json::json!({"user_id":"u-1","nickname":"Moon Walker","email":"moon@example.com"}),
        )),
        Ok(json_response(
            200,
            serde_json::json!({"usage":{"used":"40","limit":"100"}}),
        )),
    ]));
    let client = KimiOAuthApiClient::new(transport.clone(), Arc::new(RecordingSleeper::default()));

    let profile = client
        .fetch_profile("access-token", &identity())
        .await
        .unwrap();
    let usage = client
        .fetch_usage("access-token", &identity())
        .await
        .unwrap();

    assert_eq!(profile.display_label(), "Moon Walker");
    assert_eq!(usage.tiers[0].name, "weekly_limit");
    assert_eq!(usage.tiers[0].utilization, 40.0);
    for request in transport.requests() {
        assert_eq!(request.method, KimiHttpMethod::Get);
        assert_eq!(
            header(&request, "Authorization").as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(
            header(&request, "Accept").as_deref(),
            Some("application/json")
        );
        assert_eq!(
            header(&request, "X-Msh-Platform").as_deref(),
            Some("kimi_code_cli")
        );
    }
}

#[tokio::test]
async fn profile_retries_transient_transport_and_server_failures() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![
        Err(KimiOAuthError::NetworkError("timeout".to_string())),
        Ok(raw_response(503, "temporarily unavailable")),
        Ok(json_response(
            200,
            serde_json::json!({"user_id":"u-1","nickname":"Moon Walker"}),
        )),
    ]));
    let sleeper = Arc::new(RecordingSleeper::default());
    let client = KimiOAuthApiClient::new(transport.clone(), sleeper.clone());

    let profile = client
        .fetch_profile("access-token", &identity())
        .await
        .expect("transient profile failures should be retried");

    assert_eq!(profile.user_id, "u-1");
    assert_eq!(transport.requests().len(), 3);
    assert_eq!(
        *sleeper.durations.lock().unwrap(),
        vec![Duration::from_secs(1), Duration::from_secs(2)]
    );
}

#[tokio::test]
async fn payment_required_is_an_upstream_failure_not_an_auth_failure() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        402,
        serde_json::json!({"error":"payment_required"}),
    ))]));
    let client = KimiOAuthApiClient::new(transport.clone(), Arc::new(RecordingSleeper::default()));

    let error = client
        .fetch_usage("access-token", &identity())
        .await
        .expect_err("HTTP 402 must surface as an error");

    assert!(
        matches!(error, KimiOAuthError::UpstreamRejected { status: 402, .. }),
        "402 must stay an ordinary upstream failure, got: {error:?}"
    );
    assert_eq!(transport.requests().len(), 1);
}

#[tokio::test]
async fn models_require_positive_context_and_return_only_anthropic_protocol() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({"data":[
            {"id":"k3","context_length":262144,"protocol":"anthropic"},
            {"id":"responses-only","context_length":"131072","protocol":"responses"},
            {"id":"k3","context_length":"262144","protocol":"anthropic"},
            {"id":"k3-256k","context_length":"262144","protocol":"anthropic"}
        ]}),
    ))]));
    let client = KimiOAuthApiClient::new(transport.clone(), Arc::new(RecordingSleeper::default()));

    let models = client
        .fetch_models("access-token", &identity())
        .await
        .unwrap();

    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["k3", "k3-256k"]
    );
    let request = &transport.requests()[0];
    assert_eq!(request.url, "https://api.kimi.com/coding/v1/models");
    assert_eq!(
        header(request, "Authorization").as_deref(),
        Some("Bearer access-token")
    );
    assert_eq!(header(request, "X-Msh-Version").as_deref(), Some("0.34.0"));

    let invalid_transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({"data":[
            {"id":"broken","context_length":0,"protocol":"anthropic"}
        ]}),
    ))]));
    let invalid_client =
        KimiOAuthApiClient::new(invalid_transport, Arc::new(RecordingSleeper::default()));
    assert!(invalid_client
        .fetch_models("access-token", &identity())
        .await
        .unwrap_err()
        .to_string()
        .contains("context_length"));
}

#[tokio::test]
async fn usage_maps_cli_windows_numeric_strings_and_booster_monthly_fields() {
    let transport = Arc::new(RecordingTransport::with_responses(vec![Ok(json_response(
        200,
        serde_json::json!({
            "usage":{"used":"150","limit":"100","resetTime":"2030-01-07T00:00:00Z"},
            "limits":[
                {"window":{"duration":"300","timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":"25","limit":"100","resetTime":"2030-01-01T05:00:00Z"}},
                {"window":{"duration":1,"timeUnit":"TIME_UNIT_DAY"},"detail":{"used":1,"limit":4}}
            ],
            "boosterWallet":{
                "balance":{"type":"BOOSTER","amount":"20000000000","amountLeft":"10000000000"},
                "monthlyChargeLimitEnabled":true,
                "monthlyChargeLimit":{"currency":"USD","priceInCents":"20000"},
                "monthlyUsed":{"currency":"USD","priceInCents":"5000"}
            }
        }),
    ))]));
    let client = KimiOAuthApiClient::new(transport, Arc::new(RecordingSleeper::default()));

    let usage = client
        .fetch_usage("access-token", &identity())
        .await
        .unwrap();

    assert_eq!(usage.tiers[0].name, "weekly_limit");
    assert_eq!(usage.tiers[0].utilization, 100.0);
    assert_eq!(usage.tiers[1].name, "five_hour");
    assert_eq!(usage.tiers[1].utilization, 25.0);
    assert_eq!(usage.tiers[2].name, "kimi_1_day");
    let extra = usage.extra_usage.unwrap();
    assert!(extra.is_enabled);
    assert_eq!(extra.monthly_limit, Some(200.0));
    assert_eq!(extra.used_credits, Some(50.0));
    assert_eq!(extra.utilization, Some(25.0));
    assert_eq!(extra.currency.as_deref(), Some("USD"));
}
