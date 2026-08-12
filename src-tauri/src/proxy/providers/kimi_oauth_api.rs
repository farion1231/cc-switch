//! Kimi Code OAuth and managed-API transport.
//!
//! The wire contract mirrors Kimi Code CLI 0.34.0. Network and waiting are
//! injected so the OAuth orchestration can be tested without real services or
//! wall-clock delays.

use super::kimi_oauth_auth::KimiOAuthError;
use bytes::Bytes;
use futures::{future::BoxFuture, Stream, StreamExt};
use serde_json::Value;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

/// OAuth client identifier shipped by Kimi Code CLI 0.34.0.
pub(crate) const KIMI_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
/// User-Agent shipped by Kimi Code CLI 0.34.0.
pub(crate) const KIMI_USER_AGENT: &str = "kimi-code-cli/0.34.0";
/// Platform identity shipped by Kimi Code CLI.
pub(crate) const KIMI_PLATFORM: &str = "kimi_code_cli";
/// Version identity shipped by Kimi Code CLI 0.34.0.
pub(crate) const KIMI_VERSION: &str = "0.34.0";
/// Canonical Kimi Code managed API root.
pub(crate) const KIMI_MANAGED_API_BASE_URL: &str = "https://api.kimi.com/coding/v1";

const KIMI_DEVICE_AUTH_URL: &str = "https://auth.kimi.com/api/oauth/device_authorization";
const KIMI_TOKEN_URL: &str = "https://auth.kimi.com/api/oauth/token";
const OAUTH_TIMEOUT: Duration = Duration::from_secs(30);
const MANAGEMENT_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_OAUTH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_MANAGEMENT_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_REFRESH_ATTEMPTS: usize = 3;
const MAX_PROFILE_ATTEMPTS: usize = 3;

/// Injectable clock used by the OAuth manager.
pub(crate) trait KimiClock: Send + Sync {
    /// Returns Unix time in milliseconds.
    fn now_millis(&self) -> i64;

    /// Returns Unix time in whole seconds.
    fn now_seconds(&self) -> i64 {
        self.now_millis().div_euclid(1_000)
    }
}

/// Production clock backed by UTC system time.
pub(crate) struct SystemKimiClock;

impl KimiClock for SystemKimiClock {
    fn now_millis(&self) -> i64 {
        chrono::Utc::now().timestamp_millis()
    }
}

/// Injectable identifier source used for stable device IDs and atomic files.
pub(crate) trait KimiIdSource: Send + Sync {
    /// Returns a new opaque identifier.
    fn next_id(&self) -> String;
}

/// Production identifier source backed by UUID v4.
pub(crate) struct UuidKimiIdSource;

impl KimiIdSource for UuidKimiIdSource {
    fn next_id(&self) -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// Injectable async delay used by refresh retries.
pub(crate) trait KimiSleeper: Send + Sync {
    /// Waits for the requested duration.
    fn sleep<'a>(&'a self, duration: Duration) -> BoxFuture<'a, ()>;
}

/// Production delay backed by Tokio.
pub(crate) struct TokioKimiSleeper;

impl KimiSleeper for TokioKimiSleeper {
    fn sleep<'a>(&'a self, duration: Duration) -> BoxFuture<'a, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// Host facts used to construct Kimi's device identity headers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiDeviceFacts {
    /// Machine hostname.
    pub(crate) device_name: String,
    /// Operating-system name, release, and architecture.
    pub(crate) device_model: String,
    /// Operating-system release.
    pub(crate) os_version: String,
}

impl KimiDeviceFacts {
    /// Reads host facts using only standard-library facilities.
    pub(crate) fn from_system() -> Self {
        let release = system_release();
        let device_name = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
        Self::from_platform(
            &device_name,
            std::env::consts::OS,
            &release,
            std::env::consts::ARCH,
            macos_product_version().as_deref(),
        )
    }

    /// Builds device facts from explicit platform values for deterministic validation.
    pub(crate) fn from_platform(
        device_name: &str,
        os: &str,
        release: &str,
        architecture: &str,
        macos_product_version: Option<&str>,
    ) -> Self {
        let architecture = match architecture {
            "x86" => "ia32",
            "x86_64" => "x64",
            "aarch64" => "arm64",
            other => other,
        };
        let device_model = match os {
            "macos" => format!(
                "macOS {} {}",
                macos_product_version.unwrap_or(release),
                architecture
            ),
            "windows" => format!("Windows {release} {architecture}"),
            "linux" => format!("Linux {release} {architecture}"),
            other => format!("{other} {release} {architecture}")
                .trim()
                .to_string(),
        };
        Self {
            device_name: device_name.to_string(),
            device_model,
            os_version: release.to_string(),
        }
    }
}

fn system_release() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("cmd").args(["/C", "ver"]).output() {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout);
                if let Some(release) = parse_windows_release(&value) {
                    return release;
                }
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(output) = Command::new("uname").arg("-r").output() {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !value.is_empty() {
                    return value;
                }
            }
        }
    }
    std::env::var("OS").unwrap_or_else(|_| "unknown".to_string())
}

/// Extracts a language-independent dotted release from Windows `ver` output.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn parse_windows_release(output: &str) -> Option<String> {
    output
        .split(|character: char| !(character.is_ascii_digit() || character == '.'))
        .map(|candidate| candidate.trim_matches('.'))
        .find(|candidate| {
            candidate.split('.').count() >= 2
                && candidate.split('.').all(|component| {
                    !component.is_empty() && component.bytes().all(|b| b.is_ascii_digit())
                })
        })
        .map(ToString::to_string)
}

fn macos_product_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
        {
            if output.status.success() {
                let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// Validated Kimi host/device identity attached to OAuth and inference calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiClientIdentity {
    headers: Vec<(String, String)>,
}

impl KimiClientIdentity {
    /// Builds the exact Kimi Code CLI 0.34.0 identity from stable device facts.
    pub(crate) fn from_facts(device_id: &str, facts: KimiDeviceFacts) -> Self {
        Self {
            headers: vec![
                ("User-Agent".to_string(), KIMI_USER_AGENT.to_string()),
                ("X-Msh-Platform".to_string(), KIMI_PLATFORM.to_string()),
                ("X-Msh-Version".to_string(), KIMI_VERSION.to_string()),
                (
                    "X-Msh-Device-Name".to_string(),
                    Self::sanitize_header_value(&facts.device_name, "unknown"),
                ),
                (
                    "X-Msh-Device-Model".to_string(),
                    Self::sanitize_header_value(&facts.device_model, "unknown"),
                ),
                (
                    "X-Msh-Os-Version".to_string(),
                    Self::sanitize_header_value(&facts.os_version, "unknown"),
                ),
                (
                    "X-Msh-Device-Id".to_string(),
                    Self::sanitize_header_value(device_id, "unknown"),
                ),
            ],
        }
    }

    /// Removes characters that cannot be represented safely in HTTP headers.
    pub(crate) fn sanitize_header_value(value: &str, fallback: &str) -> String {
        let cleaned: String = value
            .chars()
            .filter(|character| matches!(*character as u32, 0x20..=0x7e))
            .collect::<String>()
            .trim()
            .to_string();
        if cleaned.is_empty() {
            fallback.to_string()
        } else {
            cleaned
        }
    }

    /// Returns the fixed Kimi Code CLI User-Agent.
    pub(crate) fn user_agent(&self) -> &str {
        self.header_value("User-Agent").unwrap_or(KIMI_USER_AGENT)
    }

    /// Looks up one identity header by case-insensitive name.
    pub(crate) fn header_value(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Returns all identity headers in canonical order.
    pub(crate) fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    /// Returns only device headers, excluding User-Agent.
    pub(crate) fn device_headers(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("User-Agent"))
            .cloned()
            .collect()
    }
}

/// HTTP method understood by the Kimi transport abstraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KimiHttpMethod {
    /// GET request.
    Get,
    /// POST request.
    Post,
}

/// Fully resolved outbound request used by production and fake transports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiHttpRequest {
    /// HTTP method.
    pub(crate) method: KimiHttpMethod,
    /// Absolute HTTPS URL.
    pub(crate) url: String,
    /// Validated headers.
    pub(crate) headers: Vec<(String, String)>,
    /// Form fields for OAuth POST requests.
    pub(crate) form: Vec<(String, String)>,
    /// Request timeout.
    pub(crate) timeout: Duration,
    /// Maximum accepted response size.
    pub(crate) max_response_bytes: usize,
}

/// Raw response returned by a Kimi transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiHttpResponse {
    /// Numeric HTTP status.
    pub(crate) status: u16,
    /// Raw response body.
    pub(crate) body: Vec<u8>,
}

/// Small outbound-I/O interface for Kimi endpoints.
pub(crate) trait KimiHttpTransport: Send + Sync {
    /// Executes one resolved request.
    fn execute<'a>(
        &'a self,
        request: KimiHttpRequest,
    ) -> BoxFuture<'a, Result<KimiHttpResponse, KimiOAuthError>>;
}

/// Production Kimi transport backed by the shared reqwest client.
pub(crate) struct ReqwestKimiHttpTransport;

async fn read_response_body_with_limit<S>(
    stream: S,
    max_response_bytes: usize,
) -> Result<Vec<u8>, KimiOAuthError>
where
    S: Stream<Item = Result<Bytes, KimiOAuthError>>,
{
    let mut stream = Box::pin(stream);
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        let length = body.len().checked_add(chunk.len()).ok_or_else(|| {
            KimiOAuthError::ParseError("Kimi response exceeded the size limit".to_string())
        })?;
        if length > max_response_bytes {
            return Err(KimiOAuthError::ParseError(
                "Kimi response exceeded the size limit".to_string(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

impl KimiHttpTransport for ReqwestKimiHttpTransport {
    fn execute<'a>(
        &'a self,
        request: KimiHttpRequest,
    ) -> BoxFuture<'a, Result<KimiHttpResponse, KimiOAuthError>> {
        Box::pin(async move {
            let client = crate::proxy::http_client::get();
            let mut builder = match request.method {
                KimiHttpMethod::Get => client.get(&request.url),
                KimiHttpMethod::Post => client.post(&request.url),
            }
            .timeout(request.timeout);
            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }
            if !request.form.is_empty() {
                builder = builder.form(&request.form);
            }
            let response = builder.send().await?;
            if response
                .content_length()
                .is_some_and(|length| length > request.max_response_bytes as u64)
            {
                return Err(KimiOAuthError::ParseError(
                    "Kimi response exceeded the size limit".to_string(),
                ));
            }
            let status = response.status().as_u16();
            let stream = response
                .bytes_stream()
                .map(|chunk| chunk.map_err(KimiOAuthError::from));
            let body = read_response_body_with_limit(stream, request.max_response_bytes).await?;
            Ok(KimiHttpResponse { status, body })
        })
    }
}

/// Validated device authorization payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiDeviceAuthorization {
    /// Opaque polling code.
    pub(crate) device_code: String,
    /// Human-entered short code.
    pub(crate) user_code: String,
    /// Basic verification URL.
    pub(crate) verification_uri: String,
    /// Verification URL containing the user code.
    pub(crate) verification_uri_complete: String,
    /// Server-reported lifetime in seconds.
    pub(crate) expires_in: u64,
    /// Server-reported poll interval in seconds.
    pub(crate) interval: u64,
}

/// Validated OAuth token bundle.
#[derive(Clone)]
pub(crate) struct KimiTokenBundle {
    /// Secret access token.
    pub(crate) access_token: String,
    /// Secret rotating refresh token.
    pub(crate) refresh_token: String,
    /// Optional identity token.
    pub(crate) id_token: Option<String>,
    /// Access-token lifetime in seconds.
    pub(crate) expires_in: i64,
}

/// Result of one RFC 8628 token poll.
pub(crate) enum KimiDevicePollResult {
    /// Authorization completed.
    Success(KimiTokenBundle),
    /// User action is still pending.
    Pending,
    /// Server requested a longer polling interval.
    SlowDown,
    /// User denied the request.
    Denied,
    /// Device authorization expired.
    Expired,
}

/// Minimal profile returned by Kimi's managed `/me` endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiUserProfile {
    /// Stable Kimi user identifier.
    pub(crate) user_id: String,
    /// Human-friendly nickname.
    pub(crate) nickname: Option<String>,
    /// Username when present.
    pub(crate) username: Option<String>,
    /// Email address when present.
    pub(crate) email: Option<String>,
    /// Avatar URL when present.
    pub(crate) avatar_url: Option<String>,
}

impl KimiUserProfile {
    /// Selects the same stable presentation priority used by the add-on UI.
    pub(crate) fn display_label(&self) -> &str {
        self.nickname
            .as_deref()
            .or(self.username.as_deref())
            .or(self.email.as_deref())
            .unwrap_or(&self.user_id)
    }
}

/// Anthropic-protocol model advertised by Kimi's `/models` endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiModelInfo {
    /// Upstream model identifier.
    pub(crate) id: String,
    /// Validated context window.
    pub(crate) context_length: u64,
}

/// One normalized usage window.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KimiUsageTier {
    /// Stable cc-switch tier identifier.
    pub(crate) name: String,
    /// Used percentage clamped to 0–100.
    pub(crate) utilization: f64,
    /// Upstream reset timestamp.
    pub(crate) resets_at: Option<String>,
}

/// Normalized booster-wallet monthly fields representable by cc-switch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KimiExtraUsage {
    /// Whether the monthly cap is enabled.
    pub(crate) is_enabled: bool,
    /// Monthly cap in major currency units.
    pub(crate) monthly_limit: Option<f64>,
    /// Used amount in major currency units.
    pub(crate) used_credits: Option<f64>,
    /// Used percentage clamped to 0–100.
    pub(crate) utilization: Option<f64>,
    /// ISO currency code.
    pub(crate) currency: Option<String>,
}

/// Normalized Kimi usage report.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KimiUsageReport {
    /// Ordered normalized usage windows.
    pub(crate) tiers: Vec<KimiUsageTier>,
    /// Optional booster-wallet monthly fields.
    pub(crate) extra_usage: Option<KimiExtraUsage>,
}

/// Stateless Kimi OAuth and management API client.
pub(crate) struct KimiOAuthApiClient {
    transport: Arc<dyn KimiHttpTransport>,
    sleeper: Arc<dyn KimiSleeper>,
}

impl KimiOAuthApiClient {
    /// Creates a client with injected outbound I/O and retry waiting.
    pub(crate) fn new(
        transport: Arc<dyn KimiHttpTransport>,
        sleeper: Arc<dyn KimiSleeper>,
    ) -> Self {
        Self { transport, sleeper }
    }

    /// Creates a production client using the shared reqwest transport.
    pub(crate) fn production() -> Self {
        Self::new(
            Arc::new(ReqwestKimiHttpTransport),
            Arc::new(TokioKimiSleeper),
        )
    }

    /// Starts the RFC 8628 device authorization flow.
    pub(crate) async fn request_device_authorization(
        &self,
        identity: &KimiClientIdentity,
    ) -> Result<KimiDeviceAuthorization, KimiOAuthError> {
        let request = oauth_post_request(
            KIMI_DEVICE_AUTH_URL,
            identity,
            vec![("client_id".to_string(), KIMI_CLIENT_ID.to_string())],
        );
        let response = self.transport.execute(request).await?;
        if response.status != 200 {
            return Err(http_error("device authorization", response.status));
        }
        let value = parse_json(&response.body, "device authorization")?;
        parse_device_authorization(&value)
    }

    /// Polls once for a device-flow token.
    pub(crate) async fn poll_device_token(
        &self,
        identity: &KimiClientIdentity,
        device_code: &str,
    ) -> Result<KimiDevicePollResult, KimiOAuthError> {
        let request = oauth_post_request(
            KIMI_TOKEN_URL,
            identity,
            vec![
                ("client_id".to_string(), KIMI_CLIENT_ID.to_string()),
                ("device_code".to_string(), device_code.to_string()),
                (
                    "grant_type".to_string(),
                    "urn:ietf:params:oauth:grant-type:device_code".to_string(),
                ),
            ],
        );
        let response = self.transport.execute(request).await?;
        let value = parse_json(&response.body, "device token")?;
        if response.status == 200 {
            return parse_token_bundle(&value).map(KimiDevicePollResult::Success);
        }
        match oauth_error_code(&value).as_deref() {
            Some("authorization_pending") => Ok(KimiDevicePollResult::Pending),
            Some("slow_down") => Ok(KimiDevicePollResult::SlowDown),
            Some("access_denied") => Ok(KimiDevicePollResult::Denied),
            Some("expired_token") => Ok(KimiDevicePollResult::Expired),
            _ => Err(http_error("device token", response.status)),
        }
    }

    /// Refreshes an access token with the CLI's three-attempt retry policy.
    pub(crate) async fn refresh_access_token(
        &self,
        identity: &KimiClientIdentity,
        refresh_token: &str,
    ) -> Result<KimiTokenBundle, KimiOAuthError> {
        for attempt in 0..MAX_REFRESH_ATTEMPTS {
            let request = oauth_post_request(
                KIMI_TOKEN_URL,
                identity,
                vec![
                    ("client_id".to_string(), KIMI_CLIENT_ID.to_string()),
                    ("grant_type".to_string(), "refresh_token".to_string()),
                    ("refresh_token".to_string(), refresh_token.to_string()),
                ],
            );
            let response = match self.transport.execute(request).await {
                Ok(response) => response,
                Err(_error) if attempt + 1 < MAX_REFRESH_ATTEMPTS => {
                    self.sleep_before_retry(attempt).await;
                    continue;
                }
                Err(error) => return Err(error),
            };
            if response.status == 200 {
                let value = parse_json(&response.body, "token refresh")?;
                return parse_token_bundle(&value);
            }
            let value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
            let error_code = oauth_error_code(&value);
            if matches!(response.status, 401 | 403)
                || matches!(
                    error_code.as_deref(),
                    Some("invalid_grant" | "invalid_token")
                )
            {
                return Err(KimiOAuthError::RefreshTokenInvalid);
            }
            if is_retryable_status(response.status) && attempt + 1 < MAX_REFRESH_ATTEMPTS {
                self.sleep_before_retry(attempt).await;
                continue;
            }
            return Err(http_error("token refresh", response.status));
        }
        Err(KimiOAuthError::TokenFetchFailed(
            "token refresh exhausted retries".to_string(),
        ))
    }

    /// Fetches the authenticated Kimi profile.
    pub(crate) async fn fetch_profile(
        &self,
        access_token: &str,
        identity: &KimiClientIdentity,
    ) -> Result<KimiUserProfile, KimiOAuthError> {
        for attempt in 0..MAX_PROFILE_ATTEMPTS {
            match self
                .get_management_json("me", access_token, identity.headers())
                .await
            {
                Ok(value) => return parse_profile(&value),
                Err(error)
                    if is_retryable_management_error(&error)
                        && attempt + 1 < MAX_PROFILE_ATTEMPTS =>
                {
                    self.sleep_before_retry(attempt).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(KimiOAuthError::TokenFetchFailed(
            "profile lookup exhausted retries".to_string(),
        ))
    }

    /// Fetches, validates, sorts, and deduplicates Anthropic-protocol models.
    pub(crate) async fn fetch_models(
        &self,
        access_token: &str,
        identity: &KimiClientIdentity,
    ) -> Result<Vec<KimiModelInfo>, KimiOAuthError> {
        let value = self
            .get_management_json("models", access_token, identity.headers())
            .await?;
        parse_models(&value)
    }

    /// Fetches and normalizes managed Kimi usage.
    pub(crate) async fn fetch_usage(
        &self,
        access_token: &str,
        identity: &KimiClientIdentity,
    ) -> Result<KimiUsageReport, KimiOAuthError> {
        let value = self
            .get_management_json("usages", access_token, identity.headers())
            .await?;
        Ok(parse_usage(&value))
    }

    /// Computes a finite usage percentage clamped to the UI's 0–100 range.
    pub(crate) fn utilization_percent(used: f64, limit: f64) -> f64 {
        if !used.is_finite() || !limit.is_finite() || limit <= 0.0 {
            return 0.0;
        }
        (used / limit * 100.0).clamp(0.0, 100.0)
    }

    async fn sleep_before_retry(&self, attempt: usize) {
        self.sleeper
            .sleep(Duration::from_secs(1_u64 << attempt))
            .await;
    }

    async fn get_management_json(
        &self,
        endpoint: &'static str,
        access_token: &str,
        mut identity_headers: Vec<(String, String)>,
    ) -> Result<Value, KimiOAuthError> {
        identity_headers.push((
            "Authorization".to_string(),
            format!("Bearer {access_token}"),
        ));
        identity_headers.push(("Accept".to_string(), "application/json".to_string()));
        let request = KimiHttpRequest {
            method: KimiHttpMethod::Get,
            url: format!("{KIMI_MANAGED_API_BASE_URL}/{endpoint}"),
            headers: identity_headers,
            form: Vec::new(),
            timeout: MANAGEMENT_TIMEOUT,
            max_response_bytes: MAX_MANAGEMENT_RESPONSE_BYTES,
        };
        let response = self.transport.execute(request).await?;
        if matches!(response.status, 401..=403) {
            return Err(KimiOAuthError::ManagementUnauthorized(endpoint));
        }
        if !(200..300).contains(&response.status) {
            return Err(KimiOAuthError::UpstreamRejected {
                endpoint,
                status: response.status,
            });
        }
        parse_json(&response.body, endpoint)
    }
}

fn oauth_post_request(
    url: &str,
    identity: &KimiClientIdentity,
    form: Vec<(String, String)>,
) -> KimiHttpRequest {
    let mut headers = identity.headers();
    headers.push(("Accept".to_string(), "application/json".to_string()));
    headers.push((
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    ));
    KimiHttpRequest {
        method: KimiHttpMethod::Post,
        url: url.to_string(),
        headers,
        form,
        timeout: OAUTH_TIMEOUT,
        max_response_bytes: MAX_OAUTH_RESPONSE_BYTES,
    }
}

fn parse_json(body: &[u8], operation: &str) -> Result<Value, KimiOAuthError> {
    serde_json::from_slice(body).map_err(|_| {
        KimiOAuthError::ParseError(format!("Kimi {operation} response was not valid JSON"))
    })
}

fn http_error(operation: &str, status: u16) -> KimiOAuthError {
    KimiOAuthError::TokenFetchFailed(format!("{operation} failed: HTTP {status}"))
}

fn required_string(value: &Value, field: &str, operation: &str) -> Result<String, KimiOAuthError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            KimiOAuthError::ParseError(format!(
                "Kimi {operation} response missing or invalid {field}"
            ))
        })
}

fn optional_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn numeric_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(number) => number.parse::<f64>().ok(),
        _ => None,
    }
    .filter(|number| number.is_finite())
}

fn integer_value(value: &Value) -> Option<i64> {
    let number = numeric_value(value)?;
    if number < i64::MIN as f64 || number > i64::MAX as f64 {
        return None;
    }
    Some(number.trunc() as i64)
}

fn parse_device_authorization(value: &Value) -> Result<KimiDeviceAuthorization, KimiOAuthError> {
    let expires_in = value
        .get("expires_in")
        .and_then(integer_value)
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            KimiOAuthError::ParseError(
                "Kimi device authorization response missing or invalid expires_in".to_string(),
            )
        })?;
    let interval = value
        .get("interval")
        .and_then(integer_value)
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(5);
    Ok(KimiDeviceAuthorization {
        device_code: required_string(value, "device_code", "device authorization")?,
        user_code: required_string(value, "user_code", "device authorization")?,
        verification_uri: optional_string(value, "verification_uri").unwrap_or_default(),
        verification_uri_complete: required_string(
            value,
            "verification_uri_complete",
            "device authorization",
        )?,
        expires_in,
        interval,
    })
}

fn parse_token_bundle(value: &Value) -> Result<KimiTokenBundle, KimiOAuthError> {
    let expires_in = value
        .get("expires_in")
        .and_then(integer_value)
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            KimiOAuthError::ParseError(
                "Kimi OAuth response missing or invalid expires_in".to_string(),
            )
        })?;
    Ok(KimiTokenBundle {
        access_token: required_string(value, "access_token", "OAuth")?,
        refresh_token: required_string(value, "refresh_token", "OAuth")?,
        id_token: optional_string(value, "id_token"),
        expires_in,
    })
}

fn oauth_error_code(value: &Value) -> Option<String> {
    optional_string(value, "error").map(|value| {
        value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || "_.-".contains(*character))
            .take(64)
            .collect()
    })
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

fn is_retryable_management_error(error: &KimiOAuthError) -> bool {
    match error {
        KimiOAuthError::NetworkError(_) => true,
        KimiOAuthError::UpstreamRejected { status, .. } => is_retryable_status(*status),
        _ => false,
    }
}

fn parse_profile(value: &Value) -> Result<KimiUserProfile, KimiOAuthError> {
    Ok(KimiUserProfile {
        user_id: required_string(value, "user_id", "profile")?,
        nickname: optional_string(value, "nickname"),
        username: optional_string(value, "username"),
        email: optional_string(value, "email"),
        avatar_url: optional_string(value, "avatar"),
    })
}

fn parse_models(value: &Value) -> Result<Vec<KimiModelInfo>, KimiOAuthError> {
    let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        KimiOAuthError::ParseError("Kimi models response missing data array".to_string())
    })?;
    let mut models = Vec::new();
    for item in data {
        let id = required_string(item, "id", "models")?;
        let context_length = item
            .get("context_length")
            .and_then(integer_value)
            .and_then(|value| u64::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                KimiOAuthError::ParseError(format!(
                    "Kimi model {id} must include a positive context_length"
                ))
            })?;
        if item.get("protocol").and_then(Value::as_str) == Some("anthropic") {
            models.push(KimiModelInfo { id, context_length });
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

fn parse_usage(value: &Value) -> KimiUsageReport {
    let mut tiers = Vec::new();
    if let Some(summary) = value.get("usage") {
        if let Some(tier) = parse_usage_detail(summary, "weekly_limit".to_string()) {
            tiers.push(tier);
        }
    }
    if let Some(limits) = value.get("limits").and_then(Value::as_array) {
        for item in limits {
            let Some(detail) = item.get("detail") else {
                continue;
            };
            let name = usage_tier_name(item.get("window"));
            if let Some(tier) = parse_usage_detail(detail, name) {
                tiers.push(tier);
            }
        }
    }
    KimiUsageReport {
        tiers,
        extra_usage: parse_extra_usage(value.get("boosterWallet")),
    }
}

fn parse_usage_detail(value: &Value, name: String) -> Option<KimiUsageTier> {
    let used = value.get("used").and_then(numeric_value);
    let limit = value.get("limit").and_then(numeric_value);
    if used.is_none() && limit.is_none() {
        return None;
    }
    Some(KimiUsageTier {
        name,
        utilization: KimiOAuthApiClient::utilization_percent(
            used.unwrap_or(0.0),
            limit.unwrap_or(0.0),
        ),
        resets_at: optional_string(value, "resetTime"),
    })
}

fn usage_tier_name(window: Option<&Value>) -> String {
    let duration = window
        .and_then(|window| window.get("duration"))
        .and_then(integer_value)
        .unwrap_or(0);
    let unit = window
        .and_then(|window| window.get("timeUnit"))
        .and_then(Value::as_str)
        .and_then(|unit| match unit {
            "TIME_UNIT_MINUTE" => Some("minute"),
            "TIME_UNIT_HOUR" => Some("hour"),
            "TIME_UNIT_DAY" => Some("day"),
            "TIME_UNIT_WEEK" => Some("week"),
            _ => None,
        })
        .unwrap_or("unknown");
    if (duration == 300 && unit == "minute") || (duration == 5 && unit == "hour") {
        "five_hour".to_string()
    } else if (duration == 1 && unit == "week") || (duration == 7 && unit == "day") {
        "weekly_limit".to_string()
    } else if duration == 30 && unit == "day" {
        "monthly".to_string()
    } else {
        format!("kimi_{duration}_{unit}")
    }
}

fn parse_extra_usage(value: Option<&Value>) -> Option<KimiExtraUsage> {
    let wallet = value?.as_object()?;
    let balance = wallet.get("balance")?.as_object()?;
    if balance.get("type").and_then(Value::as_str) != Some("BOOSTER") {
        return None;
    }
    if balance
        .get("amount")
        .and_then(integer_value)
        .is_none_or(|amount| amount <= 0)
    {
        return None;
    }
    let monthly_limit = parse_money(wallet.get("monthlyChargeLimit"));
    let used_credits = parse_money(wallet.get("monthlyUsed"));
    let currency = money_currency(wallet.get("monthlyChargeLimit"))
        .or_else(|| money_currency(wallet.get("monthlyUsed")))
        .or_else(|| Some("USD".to_string()));
    let utilization = monthly_limit
        .zip(used_credits)
        .map(|(limit, used)| KimiOAuthApiClient::utilization_percent(used, limit));
    Some(KimiExtraUsage {
        is_enabled: wallet
            .get("monthlyChargeLimitEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        monthly_limit,
        used_credits,
        utilization,
        currency,
    })
}

fn parse_money(value: Option<&Value>) -> Option<f64> {
    value?
        .get("priceInCents")
        .and_then(numeric_value)
        .map(|cents| cents / 100.0)
}

fn money_currency(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| optional_string(value, "currency"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn response_body_reader_accepts_a_body_at_the_limit() {
        let body = read_response_body_with_limit(
            stream::iter([
                Ok(Bytes::from_static(b"123")),
                Ok(Bytes::from_static(b"456")),
            ]),
            6,
        )
        .await
        .expect("body at the configured limit should be accepted");

        assert_eq!(body, b"123456");
    }

    #[tokio::test]
    async fn response_body_reader_stops_at_the_first_oversized_chunk() {
        let polled = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&polled);
        let stream = stream::iter([
            Ok(Bytes::from_static(b"1234")),
            Ok(Bytes::from_static(b"5678")),
            Ok(Bytes::from_static(b"unused")),
        ])
        .inspect(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        let error = read_response_body_with_limit(stream, 6)
            .await
            .expect_err("body exceeding the configured limit should be rejected");

        assert!(
            matches!(error, KimiOAuthError::ParseError(message) if message.contains("size limit"))
        );
        assert_eq!(polled.load(Ordering::SeqCst), 2);
    }
}
