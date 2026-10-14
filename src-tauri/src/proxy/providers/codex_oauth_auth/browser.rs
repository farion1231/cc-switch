//! Browser authorization-code login. Only the callback code is held in memory;
//! tokens are committed through the same lifecycle gates as device-code login.

use super::*;
use axum::{
    extract::{RawQuery, State},
    http::{header, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use sha2::{Digest, Sha256};
use std::future::IntoFuture;

const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const LOGIN_LIFETIME: u64 = 900;

#[derive(Clone)]
struct CallbackState {
    state: String,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

pub(super) struct BrowserFlow {
    verifier: String,
    callback: CallbackState,
    task: tokio::task::AbortHandle,
    join: Mutex<Option<tokio::task::JoinHandle<()>>>,
    polling: Mutex<()>,
}

impl Drop for BrowserFlow {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl BrowserFlow {
    pub(super) async fn stop(&self) {
        self.task.abort();
        if let Some(join) = self.join.lock().await.take() {
            let _ = join.await;
        }
    }
}

fn authorize_url(verifier: &str, state: &str) -> String {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse("https://auth.openai.com/oauth/authorize").expect("constant URL");
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", CODEX_CLIENT_ID),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("state", state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", CODEX_OAUTH_ORIGINATOR),
    ]);
    url.into()
}

// Reject malformed/duplicate security parameters instead of letting a map silently
// select one value. An unrelated callback must not consume a valid pending login.
fn parse_callback(query: &str, expected_state: &str) -> Result<Result<String, String>, ()> {
    let mut values = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if values
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(());
        }
    }
    if values.get("state").map(String::as_str) != Some(expected_state) {
        return Err(());
    }
    match (values.get("code"), values.get("error")) {
        (Some(code), None) if !code.is_empty() => Ok(Ok(code.clone())),
        (None, Some(_)) => Ok(Err(
            "Browser authorization was denied. Please try again.".into()
        )),
        _ => Err(()),
    }
}

async fn callback(
    State(state): State<CallbackState>,
    RawQuery(query): RawQuery,
) -> impl IntoResponse {
    let result = match parse_callback(query.as_deref().unwrap_or_default(), &state.state) {
        Ok(result) => result,
        Err(()) => {
            return (
                StatusCode::BAD_REQUEST,
                [
                    (header::CACHE_CONTROL, "no-store"),
                    (header::REFERRER_POLICY, "no-referrer"),
                ],
                "Invalid login callback. Return to CC Switch and try again.",
            )
        }
    };
    let success = result.is_ok();
    let mut slot = state.result.lock().await;
    if slot.is_some() {
        return (
            StatusCode::CONFLICT,
            [
                (header::CACHE_CONTROL, "no-store"),
                (header::REFERRER_POLICY, "no-referrer"),
            ],
            "This login callback has already been received.",
        );
    }
    *slot = Some(result);
    (
        StatusCode::OK,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
        if success {
            "Authorization received. Return to CC Switch to finish signing in."
        } else {
            "Authorization was denied. Return to CC Switch to try again."
        },
    )
}

async fn bind_callback_listener(port: u16) -> Result<tokio::net::TcpListener, CodexOAuthError> {
    // Bind before changing login state. Never stop another app's listener or
    // silently fall back to device authentication.
    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
        .map_err(|_| CodexOAuthError::IoError(format!("Cannot open the browser login callback on localhost:{port}. Finish any other Codex login and retry, or choose device-code login.")))
}

impl CodexOAuthManager {
    pub async fn start_browser_flow(
        &self,
        target_account_id: Option<&str>,
    ) -> Result<GitHubDeviceCodeResponse, CodexOAuthError> {
        let listener = bind_callback_listener(1455).await?;
        self.start_browser_flow_with_listener(target_account_id, listener)
            .await
    }

    async fn start_browser_flow_with_listener(
        &self,
        target_account_id: Option<&str>,
        listener: tokio::net::TcpListener,
    ) -> Result<GitHubDeviceCodeResponse, CodexOAuthError> {
        let (epoch, target, generation) = self.prepare_login_target(target_account_id).await?;
        let flow_id = format!("browser:{}", Uuid::new_v4());
        let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let authorization_url = authorize_url(&verifier, &state);
        let callback_state = CallbackState {
            state,
            result: Arc::new(Mutex::new(None)),
        };
        let router = Router::new()
            .route("/auth/callback", get(callback))
            .with_state(callback_state.clone());
        let task = tokio::spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(LOGIN_LIFETIME),
                axum::serve(listener, router).into_future(),
            )
            .await;
        });
        let flow = Arc::new(BrowserFlow {
            verifier,
            callback: callback_state,
            task: task.abort_handle(),
            join: Mutex::new(Some(task)),
            polling: Mutex::new(()),
        });
        let expires_at = chrono::Utc::now().timestamp_millis() + (LOGIN_LIFETIME as i64) * 1000;
        // Use the existing pending-flow registry so cancellation, clear-all, and
        // newer targeted reauthentication linearize against token persistence.
        self.register_pending_device_code(
            flow_id.clone(),
            String::new(),
            expires_at,
            epoch,
            target,
            generation,
        )
        .await?;
        let mut flows = self.browser_flows.write().await;
        if !self
            .pending_device_codes
            .read()
            .await
            .contains_key(&flow_id)
        {
            return Err(CodexOAuthError::ExpiredToken);
        }
        flows.insert(flow_id.clone(), flow);
        Ok(GitHubDeviceCodeResponse {
            device_code: flow_id,
            user_code: String::new(),
            verification_uri: authorization_url,
            expires_in: LOGIN_LIFETIME,
            interval: 1,
        })
    }

    pub(super) async fn poll_browser_flow<BeforeCommit, CommitFuture, CommitGuard>(
        &self,
        flow_id: &str,
        before_commit: BeforeCommit,
    ) -> Result<Option<GitHubAccount>, CodexOAuthError>
    where
        BeforeCommit: FnOnce() -> CommitFuture,
        CommitFuture: std::future::Future<Output = CommitGuard>,
    {
        let flow = self
            .browser_flows
            .read()
            .await
            .get(flow_id)
            .cloned()
            .ok_or(CodexOAuthError::ExpiredToken)?;
        let Ok(_poll_guard) = flow.polling.try_lock() else {
            return Ok(None);
        };
        let entry = self
            .pending_device_codes
            .read()
            .await
            .get(flow_id)
            .cloned()
            .ok_or(CodexOAuthError::ExpiredToken)?;
        if entry.expires_at_ms <= chrono::Utc::now().timestamp_millis() {
            self.cancel_device_flow(flow_id).await;
            return Err(CodexOAuthError::ExpiredToken);
        }
        let code = match flow.callback.result.lock().await.take() {
            None => return Ok(None),
            Some(Ok(code)) => code,
            Some(Err(error)) => {
                self.cancel_device_flow(flow_id).await;
                return Err(CodexOAuthError::TokenFetchFailed(error));
            }
        };
        // Stop accepting callbacks before exchanging the one-use code.
        flow.stop().await;
        let result = async {
            let tokens = self
                .exchange_code_for_tokens(&code, &flow.verifier, REDIRECT_URI)
                .await?;
            let _commit_guard = before_commit().await;
            self.commit_login_tokens(tokens, flow_id, &entry)
                .await
                .map(Some)
        }
        .await;
        self.cancel_device_flow(flow_id).await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_url_uses_pkce_and_loopback_not_device_auth() {
        let url = url::Url::parse(&authorize_url("verifier", "random-state")).unwrap();
        let query: HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(url.path(), "/oauth/authorize");
        assert_eq!(query["redirect_uri"], REDIRECT_URI);
        assert_eq!(query["state"], "random-state");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(b"verifier"))
        );
        assert!(!query.contains_key("code_verifier"));
    }

    #[test]
    fn callback_rejects_wrong_missing_and_duplicate_state() {
        for query in [
            "code=c",
            "state=wrong&code=c",
            "state=s&state=s&code=c",
            "state=s&code=c&error=denied",
            "state=s&code=",
        ] {
            assert!(parse_callback(query, "s").is_err());
        }
        assert_eq!(
            parse_callback("state=s&code=a%2Bb", "s"),
            Ok(Ok("a+b".into()))
        );
        assert!(matches!(
            parse_callback("state=s&error=access_denied", "s"),
            Ok(Err(_))
        ));
    }

    #[tokio::test]
    async fn browser_listener_validates_state_and_releases_port_on_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CodexOAuthManager::new(dir.path().into());
        let listener = bind_callback_listener(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}/auth/callback");
        let flow = manager
            .start_browser_flow_with_listener(None, listener)
            .await
            .unwrap();
        assert!(flow.user_code.is_empty());
        let url = url::Url::parse(&flow.verification_uri).unwrap();
        let state = url
            .query_pairs()
            .find(|(key, _)| key == "state")
            .unwrap()
            .1
            .into_owned();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let bad = client
            .get(format!("{base}?state=wrong&code=c"))
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), StatusCode::BAD_REQUEST);
        assert!(manager
            .poll_for_token(&flow.device_code, || async {})
            .await
            .unwrap()
            .is_none());
        let denied = client
            .get(&base)
            .query(&[("state", state.as_str()), ("error", "access_denied")])
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::OK);
        assert_eq!(denied.headers()[header::CACHE_CONTROL], "no-store");
        assert!(manager
            .poll_for_token(&flow.device_code, || async {})
            .await
            .is_err());
        assert!(manager.list_accounts().await.is_empty());
        let listener = bind_callback_listener(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let flow = manager
            .start_browser_flow_with_listener(None, listener)
            .await
            .unwrap();
        assert!(manager.cancel_device_flow(&flow.device_code).await);
        assert!(manager
            .poll_for_token(&flow.device_code, || async {})
            .await
            .is_err());
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(listener) =
                    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await
                {
                    drop(listener);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn callback_bind_reports_port_conflict() {
        let listener = bind_callback_listener(0).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(matches!(
            bind_callback_listener(port).await,
            Err(CodexOAuthError::IoError(_))
        ));
    }
}
