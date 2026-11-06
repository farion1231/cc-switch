//! DeepSeek account (OAuth) login shared by DeepSeek Harness Web and Desktop.
//!
//! DSH stores the browser login in `$DSH_HOME/.credentials.yaml` under
//! `records["deepseek-account-platform/default"]` as `{kind: grant, payload: {version: 1, token,
//! issuer}}`. Both profiles read that one record, so signing in from either surface signs in
//! both, and DSH reloads the file when CC Switch writes it.
//!
//! CC Switch signs in with the same browser flow as DSH Web (PKCE S256 against Platform's
//! `auth-api/v0/dsh/auth_init|auth_exchange|auth_cancel`, loopback `/oauth/callback`), shows the
//! account and its balance, and signs out.
//!
//! Platform requests mirror `@deepseek-ai/dsh-deepseek-account-platform`: the token goes in
//! `x-dsh-auth-token` (no Bearer prefix), only to the grant's issuer, without following
//! redirects; responses are `{code: 0, data: {biz_code, biz_data}}`.

use crate::error::AppError;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Mutex, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

const TIMEOUT: Duration = Duration::from_secs(10);
/// DSH's default `platformOrigin`; account tokens are only ever sent here.
const PLATFORM_ORIGIN: &str = "https://platform.deepseek.com";
const CALLBACK_PATH: &str = "/oauth/callback";
/// Upper bound when Platform's `expires_in` is longer.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_REQUEST_BYTES: usize = 16 * 1024;
/// HTTP 401 or `code: 40003`: Platform rejected the stored token.
const UNAUTHORIZED_CODE: i64 = 40003;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DshAccountProfile {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Masked mobile number or email, as Platform returns it.
    pub contact: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DshWallet {
    pub currency: String,
    /// Decimal string, kept as Platform sends it.
    pub balance: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshAccountStatus {
    /// A login record exists (shared by DSH Web and Desktop).
    pub signed_in: bool,
    /// Platform rejected the stored token; DSH clears it on its next account request.
    pub expired: bool,
    pub issuer: Option<String>,
    pub profile: Option<DshAccountProfile>,
    /// Top-up balances (`normal_wallets`).
    pub wallets: Vec<DshWallet>,
    /// Positive bonus balances (`bonus_wallets`), shown apart from top-ups like DSH does.
    pub bonus_wallets: Vec<DshWallet>,
    pub usage_url: Option<String>,
    pub top_up_url: Option<String>,
    /// Profile or balance query failure; the login itself is kept.
    pub error: Option<String>,
}

enum PlatformError {
    Unauthorized,
    Failed(String),
}

/// Platform's wire locale for CC Switch's UI language (`zh_CN` or `en_US`, like DSH).
fn wire_locale() -> &'static str {
    if crate::settings::get_settings()
        .language
        .as_deref()
        .is_some_and(|language| language.starts_with("zh"))
    {
        "zh_CN"
    } else {
        "en_US"
    }
}

fn client_headers(request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
    identity_headers(request).header("x-dsh-auth-token", token)
}

/// The Platform client identity DSH Web sends (bundle id intentionally empty).
fn identity_headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    let offset = chrono::Local::now().offset().local_minus_utc();
    request
        .header("x-client-platform", "web")
        .header("x-client-bundle-id", "")
        .header("x-client-locale", wire_locale())
        .header("x-client-timezone-offset", offset.to_string())
        .timeout(TIMEOUT)
}

async fn platform_request(
    request: reqwest::RequestBuilder,
    token: &str,
) -> Result<Value, PlatformError> {
    read_platform(client_headers(request, token)).await
}

async fn read_platform(request: reqwest::RequestBuilder) -> Result<Value, PlatformError> {
    let response = request
        .send()
        .await
        .map_err(|error| PlatformError::Failed(error.without_url().to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(PlatformError::Unauthorized);
    }
    if status.is_redirection() || !status.is_success() {
        return Err(PlatformError::Failed(format!("HTTP {}", status.as_u16())));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| PlatformError::Failed(error.without_url().to_string()))?;
    match body["code"].as_i64() {
        Some(0) => Ok(body["data"]["biz_data"].clone()),
        Some(UNAUTHORIZED_CODE) => Err(PlatformError::Unauthorized),
        code => Err(PlatformError::Failed(format!(
            "unexpected response code {}",
            code.map_or_else(|| "none".into(), |code| code.to_string())
        ))),
    }
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn parse_profile(value: &Value) -> DshAccountProfile {
    DshAccountProfile {
        id: text(&value["id"]),
        name: text(&value["id_profile"]["name"]),
        contact: text(&value["mobile"])
            .or_else(|| text(&value["mobile_number"]))
            .or_else(|| text(&value["email"])),
        avatar_url: text(&value["id_profile"]["picture"]),
    }
}

fn parse_wallets(value: &Value, positive_only: bool) -> Vec<DshWallet> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|wallet| {
            let currency = text(&wallet["currency"])?;
            let balance = text(&wallet["balance"])?;
            if positive_only && balance.parse::<f64>().map_or(true, |amount| amount <= 0.0) {
                return None;
            }
            Some(DshWallet { currency, balance })
        })
        .collect()
}

/// Only the configured platform may receive the stored token. Do not derive request URLs
/// from a credential file, even when it was written by another DSH client.
fn validated_origin(issuer: &str) -> Result<&'static str, String> {
    if issuer.trim_end_matches('/') == PLATFORM_ORIGIN {
        Ok(PLATFORM_ORIGIN)
    } else {
        Err(login_error(
            "dsh.account.error_issuer",
            "DeepSeek 登录凭据的 issuer 与官方平台不一致，请重新登录",
            "The DeepSeek credential issuer does not match the official platform; sign in again",
        ))
    }
}

/// The shared login plus, when signed in, the account profile and balance.
pub async fn status() -> Result<DshAccountStatus, String> {
    let Some((token, issuer)) = crate::dsh_config::account_grant().map_err(|e| e.to_string())?
    else {
        return Ok(DshAccountStatus::default());
    };
    let origin = validated_origin(&issuer)?;
    let mut status = DshAccountStatus {
        signed_in: true,
        usage_url: Some(format!("{origin}/usage")),
        top_up_url: Some(format!("{origin}/top_up")),
        issuer: Some(issuer),
        ..Default::default()
    };
    let client = crate::proxy::http_client::no_redirect()?;
    let (profile, balance) = tokio::join!(
        platform_request(
            client.get(format!("{origin}/auth-api/v0/users/current")),
            &token
        ),
        platform_request(
            client.get(format!("{origin}/api/v0/users/get_user_summary")),
            &token
        ),
    );
    let mut errors = vec![];
    for result in [&profile, &balance] {
        match result {
            Err(PlatformError::Unauthorized) => status.expired = true,
            Err(PlatformError::Failed(error)) => errors.push(error.clone()),
            Ok(_) => {}
        }
    }
    if let Ok(value) = &profile {
        status.profile = Some(parse_profile(value));
    }
    if let Ok(value) = &balance {
        status.wallets = parse_wallets(&value["normal_wallets"], false);
        status.bonus_wallets = parse_wallets(&value["bonus_wallets"], true);
    }
    if !status.expired && !errors.is_empty() {
        errors.dedup();
        status.error = Some(errors.join("; "));
    }
    Ok(status)
}

/// Remove the shared login, then revoke it remotely (best effort; local sign-out stands).
pub async fn sign_out() -> Result<(), String> {
    let Some((token, issuer)) =
        crate::dsh_config::remove_account_grant().map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let Ok(origin) = validated_origin(&issuer) else {
        // Local sign-out must still work for an unsupported issuer, without contacting it.
        return Ok(());
    };
    let request = crate::proxy::http_client::no_redirect()?
        .post(format!("{origin}/auth-api/v0/users/logout"));
    if let Err(PlatformError::Failed(error)) = platform_request(request, &token).await {
        log::warn!("DeepSeek account remote sign-out failed: {error}");
    }
    Ok(())
}

// ---------- Browser sign-in ----------

/// What the UI needs to send the user to Platform.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DshLoginStart {
    pub flow_id: String,
    pub auth_url: String,
    pub expires_in: u64,
}

struct PendingLogin {
    flow_id: String,
    result: oneshot::Receiver<Result<(), String>>,
    task: tokio::task::AbortHandle,
    authorize_id: String,
    verifier: String,
}

/// One sign-in at a time; starting again replaces the previous attempt.
static PENDING: Mutex<Option<PendingLogin>> = Mutex::new(None);

struct Pkce {
    state: String,
    verifier: String,
    challenge: String,
}

impl Pkce {
    fn generate() -> Self {
        let random = || {
            let mut bytes = Vec::with_capacity(32);
            bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
            bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
            URL_SAFE_NO_PAD.encode(&bytes)
        };
        let verifier = random();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            state: random(),
            verifier,
            challenge,
        }
    }
}

async fn platform_call(method: &str, body: Value) -> Result<Value, PlatformError> {
    let client = crate::proxy::http_client::no_redirect().map_err(PlatformError::Failed)?;
    read_platform(identity_headers(
        client
            .post(format!("{PLATFORM_ORIGIN}/auth-api/v0/dsh/{method}"))
            .json(&body),
    ))
    .await
}

/// 面向用户的登录错误文案跟随界面语言，不把英文串硬塞进中文界面。
fn login_error(key: &'static str, zh: impl Into<String>, en: impl Into<String>) -> String {
    AppError::localized(key, zh, en).to_string()
}

fn platform_error(error: PlatformError) -> String {
    match error {
        PlatformError::Unauthorized => login_error(
            "dsh.account.error_unauthorized",
            "DeepSeek 登录失败：平台拒绝了该账号令牌",
            "DeepSeek sign-in failed: the platform rejected this account token",
        ),
        PlatformError::Failed(error) => login_error(
            "dsh.account.error_platform",
            format!("DeepSeek 登录失败：{error}"),
            format!("DeepSeek sign-in failed: {error}"),
        ),
    }
}

/// Accept only Platform's own page at `path` (DSH's `browserUrl`): same origin, no credentials
/// or fragment.
fn platform_page(value: &str, path: &str) -> Option<url::Url> {
    let url = url::Url::parse(value).ok()?;
    (url.origin().ascii_serialization() == PLATFORM_ORIGIN
        && url.path() == path
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none())
    .then_some(url)
}

/// `<os>-<arch>` and `<os> <release>` in Node's `os.platform()` / `os.arch()` terms, as DSH sends.
fn device_model() -> (String, String) {
    let os = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        other => other,
    };
    (
        format!("{os}-{arch}"),
        format!("{os} {}", os_release()).trim().to_owned(),
    )
}

/// Node's `os.release()`: the kernel release (`uname -r`), or `10.0.<build>` on Windows.
fn os_release() -> String {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(not(windows))]
    let output = std::process::Command::new("uname").arg("-r").output();
    let text = output
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    if cfg!(windows) {
        // "Microsoft Windows [Version 10.0.26300.1234]" -> "10.0.26300"
        text.split("Version ")
            .nth(1)
            .and_then(|version| version.split(']').next())
            .map(|version| version.split('.').take(3).collect::<Vec<_>>().join("."))
            .unwrap_or_default()
    } else {
        text.trim().to_owned()
    }
}

/// Start a sign-in: listen on loopback, register the attempt with Platform, and finish it in the
/// background once the browser comes back. The UI opens `auth_url` and polls.
pub async fn start_login() -> Result<DshLoginStart, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK_PATH}");
    let pkce = Pkce::generate();
    let init = platform_call(
        "auth_init",
        json!({
            "code_challenge": pkce.challenge,
            "code_challenge_method": "S256",
            "state": pkce.state,
            "redirect_uri": redirect_uri,
            "locale": wire_locale(),
            "login_source": "web",
        }),
    )
    .await
    .map_err(platform_error)?;
    let auth_url = init["authorize_url"]
        .as_str()
        .and_then(|url| platform_page(url, "/dsh/authorize"))
        .ok_or_else(|| {
            login_error(
                "dsh.account.error_authorize_page",
                "DeepSeek 登录失败：平台返回了非预期的授权地址",
                "DeepSeek sign-in failed: the platform returned an unexpected authorization URL",
            )
        })?
        .to_string();
    let authorize_id = init["authorize_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            login_error(
                "dsh.account.error_authorize_id",
                "DeepSeek 登录失败：平台没有返回授权 id",
                "DeepSeek sign-in failed: the platform returned no authorization id",
            )
        })?
        .to_owned();
    let timeout = init["expires_in"]
        .as_f64()
        .filter(|seconds| *seconds > 0.0)
        .map(Duration::from_secs_f64)
        .map_or(LOGIN_TIMEOUT, |expires| expires.min(LOGIN_TIMEOUT));

    let (sender, receiver) = oneshot::channel();
    let state = pkce.state.clone();
    let verifier = pkce.verifier.clone();
    let task = tokio::spawn(async move {
        let outcome = tokio::time::timeout(
            timeout,
            finish_login(listener, &state, &verifier, &redirect_uri),
        )
        .await
        .unwrap_or_else(|_| {
            Err(login_error(
                "dsh.account.error_expired",
                "DeepSeek 登录超时，请重新登录",
                "DeepSeek sign-in timed out; start again",
            ))
        });
        let _ = sender.send(outcome);
    });

    let previous = PENDING.lock().unwrap().replace(PendingLogin {
        flow_id: pkce.state.clone(),
        result: receiver,
        task: task.abort_handle(),
        authorize_id,
        verifier: pkce.verifier,
    });
    if let Some(previous) = previous {
        abandon(previous);
    }
    Ok(DshLoginStart {
        flow_id: pkce.state,
        auth_url,
        expires_in: timeout.as_secs(),
    })
}

/// Wait for the browser, exchange the code, store the grant, then send the browser on to
/// Platform's completion page (DSH redirects only after the login is stored).
async fn finish_login(
    listener: TcpListener,
    state: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<(), String> {
    let (code, mut browser) = wait_for_callback(&listener, state).await?;
    let result = async {
        let device_id = crate::dsh_config::account_device_id().map_err(|e| e.to_string())?;
        let (device_model, os_version) = device_model();
        let exchange = platform_call(
            "auth_exchange",
            json!({
                "code": code,
                "code_verifier": verifier,
                "redirect_uri": redirect_uri,
                "device_id": device_id,
                "device_model": device_model,
                "os_version": os_version,
            }),
        )
        .await
        .map_err(platform_error)?;
        let token = exchange["token"]
            .as_str()
            .filter(|token| !token.is_empty() && token.bytes().all(|b| (0x21..=0x7e).contains(&b)))
            .ok_or_else(|| {
                login_error(
                    "dsh.account.error_missing_token",
                    "DeepSeek 登录失败：平台没有返回令牌",
                    "DeepSeek sign-in failed: the platform returned no token",
                )
            })?;
        let completion = exchange["authorized_url"]
            .as_str()
            .and_then(|url| platform_page(url, "/dsh/authorized"))
            .map(|mut url| {
                url.query_pairs_mut().append_pair("login_source", "web");
                url.to_string()
            });
        crate::dsh_config::store_account_grant(token, PLATFORM_ORIGIN)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(completion)
    }
    .await;
    let _ = match &result {
        Ok(Some(completion)) => {
            respond(&mut browser, "302 Found", &[("Location", completion)], "").await
        }
        Ok(None) => respond(&mut browser, "200 OK", &[], PAGE_DONE).await,
        Err(_) => respond(&mut browser, "200 OK", &[], PAGE_FAILED).await,
    };
    result.map(|_| ())
}

/// Accept connections until a GET to the callback path carries exactly one matching `state` and
/// one `code`; anything else is answered and ignored, like DSH's callback route.
async fn wait_for_callback(
    listener: &TcpListener,
    state: &str,
) -> Result<(String, TcpStream), String> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let Ok(head) = read_request_head(&mut stream).await else {
            continue;
        };
        let Some(query) = callback_query(&head) else {
            let _ = respond(&mut stream, "404 Not Found", &[], "").await;
            continue;
        };
        match callback_code(query, state) {
            Some(code) => return Ok((code, stream)),
            None => {
                let _ = respond(&mut stream, "400 Bad Request", &[], "").await;
            }
        }
    }
}

/// The `code` when the query has exactly one `state` equal to ours and exactly one `code`.
fn callback_code(query: &str, state: &str) -> Option<String> {
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let values = |name: &str| -> Vec<&str> {
        params
            .iter()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
            .collect()
    };
    let (states, codes) = (values("state"), values("code"));
    let state_matches = states.len() == 1
        && states[0].len() == state.len()
        && bool::from(subtle::ConstantTimeEq::ct_eq(
            states[0].as_bytes(),
            state.as_bytes(),
        ));
    (state_matches && codes.len() == 1 && !codes[0].is_empty()).then(|| codes[0].to_owned())
}

async fn read_request_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
            .await
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))??;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() > MAX_REQUEST_BYTES
        {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

/// Query string of `GET /oauth/callback?... HTTP/1.1`; `None` for any other request.
fn callback_query(head: &str) -> Option<&str> {
    let mut parts = head.lines().next()?.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let (path, query) = parts.next()?.split_once('?')?;
    (path == CALLBACK_PATH).then_some(query)
}

async fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> std::io::Result<()> {
    let mut response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n",
        body.len()
    );
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(body);
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>CC Switch</title><body style=\"font-family:system-ui;padding:48px\"><h2>登录成功 · Signed in</h2><p>可以关闭这个页面，回到 CC Switch。<br>You can close this page and return to CC Switch.</p></body>";
const PAGE_FAILED: &str = "<!doctype html><meta charset=utf-8><title>CC Switch</title><body style=\"font-family:system-ui;padding:48px\"><h2>登录没有完成 · Sign-in not completed</h2><p>回到 CC Switch 重试。<br>Return to CC Switch and try again.</p></body>";

/// `Ok(false)` while waiting for the browser; `Ok(true)` once the login is stored.
pub fn poll_login(flow_id: &str) -> Result<bool, String> {
    let mut pending = PENDING.lock().unwrap();
    let Some(login) = pending.as_mut().filter(|login| login.flow_id == flow_id) else {
        return Err(login_error(
            "dsh.account.error_flow_ended",
            "这次 DeepSeek 登录已经结束，请重新开始",
            "This DeepSeek sign-in has ended; start again",
        ));
    };
    match login.result.try_recv() {
        Err(oneshot::error::TryRecvError::Empty) => Ok(false),
        Err(oneshot::error::TryRecvError::Closed) => {
            pending.take();
            Err(login_error(
                "dsh.account.error_flow_interrupted",
                "DeepSeek 登录被中断",
                "DeepSeek sign-in was interrupted",
            ))
        }
        Ok(outcome) => {
            pending.take();
            outcome.map(|()| true)
        }
    }
}

/// Stop waiting and withdraw the attempt at Platform (best effort, like DSH's `auth_cancel`).
pub fn cancel_login(flow_id: &str) {
    let login = {
        let mut pending = PENDING.lock().unwrap();
        if pending
            .as_ref()
            .is_none_or(|login| login.flow_id != flow_id)
        {
            return;
        }
        pending.take()
    };
    if let Some(login) = login {
        abandon(login);
    }
}

fn abandon(login: PendingLogin) {
    login.task.abort();
    tauri::async_runtime::spawn(async move {
        let _ = platform_call(
            "auth_cancel",
            json!({"authorize_id": login.authorize_id, "code_verifier": login.verifier}),
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn credential_issuer_must_match_the_configured_platform() {
        for issuer in [PLATFORM_ORIGIN, "https://platform.deepseek.com/"] {
            assert_eq!(validated_origin(issuer).unwrap(), PLATFORM_ORIGIN);
        }
        for issuer in [
            "http://platform.deepseek.com",
            "https://platform.deepseek.com.attacker.example",
            "https://platform.deepseek.com@attacker.example",
            "https://platform.deepseek.com/another-path",
            "https://platform.deepseek.com?redirect=elsewhere",
            "http://127.0.0.1:1234",
        ] {
            assert!(validated_origin(issuer).is_err(), "{issuer}");
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unsupported_issuer_is_never_contacted_and_can_be_signed_out_locally() {
        let home = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", home.path());
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        crate::dsh_config::store_account_grant("test-token", &issuer).unwrap();
        assert!(status().await.unwrap_err().contains("issuer"));
        sign_out().await.unwrap();
        assert!(crate::dsh_config::account_grant().unwrap().is_none());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), listener.accept())
                .await
                .is_err()
        );
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
    }

    #[test]
    fn pkce_challenge_is_the_s256_of_the_verifier() {
        let pkce = Pkce::generate();
        assert_eq!(pkce.verifier.len(), 43);
        assert_eq!(
            pkce.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()))
        );
        assert_ne!(pkce.state, Pkce::generate().state);
    }

    #[test]
    fn only_platform_pages_on_the_expected_path_are_followed() {
        assert!(platform_page(
            "https://platform.deepseek.com/dsh/authorize?x=1",
            "/dsh/authorize"
        )
        .is_some());
        for url in [
            "https://evil.example/dsh/authorize",
            "http://platform.deepseek.com/dsh/authorize",
            "https://platform.deepseek.com/other",
            "https://user:pw@platform.deepseek.com/dsh/authorize",
            "https://platform.deepseek.com/dsh/authorize#frag",
        ] {
            assert!(platform_page(url, "/dsh/authorize").is_none(), "{url}");
        }
    }

    #[test]
    fn callback_needs_exactly_one_matching_state_and_code() {
        assert_eq!(
            callback_query("GET /oauth/callback?code=c&state=s HTTP/1.1\r\n"),
            Some("code=c&state=s")
        );
        assert_eq!(callback_query("GET /favicon.ico HTTP/1.1\r\n"), None);
        assert_eq!(
            callback_query("POST /oauth/callback?code=c HTTP/1.1\r\n"),
            None
        );
        assert_eq!(callback_code("code=c&state=s", "s").as_deref(), Some("c"));
        assert_eq!(callback_code("code=c&state=x", "s"), None);
        assert_eq!(callback_code("code=c&state=s&state=s", "s"), None);
        assert_eq!(callback_code("code=c&code=d&state=s", "s"), None);
        assert_eq!(callback_code("code=&state=s", "s"), None);
    }

    #[tokio::test]
    async fn callback_listener_skips_strays_and_returns_the_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiter = tokio::spawn(async move {
            let (code, mut stream) = wait_for_callback(&listener, "s").await.unwrap();
            respond(&mut stream, "200 OK", &[], "done").await.unwrap();
            code
        });
        for target in [
            "/favicon.ico",
            "/oauth/callback?code=x&state=wrong",
            "/oauth/callback?code=ok&state=s",
        ] {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream
                .write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1"), "{response}");
        }
        assert_eq!(waiter.await.unwrap(), "ok");
    }

    #[test]
    fn profile_prefers_mobile_then_email() {
        let profile = parse_profile(&json!({
            "id": "u1",
            "email": "a***@example.com",
            "mobile_number": "138****0000",
            "id_profile": {"name": "Ada", "picture": null}
        }));
        assert_eq!(profile.name.as_deref(), Some("Ada"));
        assert_eq!(profile.contact.as_deref(), Some("138****0000"));
        assert_eq!(profile.avatar_url, None);
        let profile = parse_profile(&json!({"email": "a***@example.com", "id_profile": null}));
        assert_eq!(profile.contact.as_deref(), Some("a***@example.com"));
        assert_eq!(profile.name, None);
    }

    #[test]
    fn bonus_wallets_drop_empty_balances() {
        let wallets = json!([
            {"currency": "CNY", "balance": "12.50"},
            {"currency": "USD", "balance": "0E-16"},
            {"currency": "USD"}
        ]);
        assert_eq!(parse_wallets(&wallets, false).len(), 2);
        assert_eq!(
            parse_wallets(&wallets, true),
            vec![DshWallet {
                currency: "CNY".into(),
                balance: "12.50".into()
            }]
        );
    }
}
