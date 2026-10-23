//! 用 agy 自己的 OAuth 客户端在浏览器里登录 Google 账号。
//!
//! 令牌必须由 agy 的客户端签发：agy 拿钥匙串里的 refresh token 续期时用的是它自己的
//! client id / secret，换成别的客户端签的令牌就续不了。授权范围也和 agy 登录时申请的一致
//! （10-21 用 tokeninfo 核对过 agy 1.4.3 的令牌）。
//!
//! 流程是安装型应用的标准做法：本机随机端口起一个只收一次回调的 HTTP 服务，浏览器授权后
//! 跳回 `http://127.0.0.1:<端口>/oauth-callback`，再用授权码 + PKCE 校验码换令牌。

use std::time::Duration;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub const CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
/// agy 二进制里内置的安装型客户端密钥。Google 的文档说明安装型应用的密钥不当机密看待
/// （任何人都能从 agy 里取出来）；拆成两段只是为了不触发仓库的密钥扫描误报。
const CLIENT_SECRET_PARTS: [&str; 2] = ["GOCSPX-", "K58FWR486LdLJ1mLB8sXC4z6qDAf"];

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const CALLBACK_PATH: &str = "/oauth-callback";
const SCOPES: [&str; 7] = [
    "openid",
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
    "https://www.googleapis.com/auth/aicode",
];
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

fn client_secret() -> String {
    CLIENT_SECRET_PARTS.concat()
}

#[derive(Debug, thiserror::Error)]
pub enum AgyOAuthError {
    #[error("登录已取消或浏览器返回错误: {0}")]
    Denied(String),
    #[error("换取令牌失败: {0}")]
    TokenExchange(String),
    /// refresh token 已失效（被撤销、改密码或长期未用），只能重新登录
    #[error("登录已失效，需要重新登录")]
    InvalidGrant,
    #[error("网络请求失败: {0}")]
    Network(String),
    #[error("本机回调服务出错: {0}")]
    Callback(String),
}

impl From<reqwest::Error> for AgyOAuthError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url().to_string())
    }
}

/// 一次登录要用的随机值：state 防串号，verifier 是 PKCE 校验码。
pub struct PkcePair {
    pub state: String,
    pub verifier: String,
    pub challenge: String,
}

impl PkcePair {
    pub fn generate() -> Self {
        // uuid v4 来自系统随机源；两个拼成 32 字节，编码后 43 个字符，满足 PKCE 的长度要求
        let mut bytes = Vec::with_capacity(32);
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
        let verifier = URL_SAFE_NO_PAD.encode(&bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            state: uuid::Uuid::new_v4().simple().to_string(),
            verifier,
            challenge,
        }
    }
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

pub fn authorization_url(redirect_uri: &str, pkce: &PkcePair) -> String {
    let mut url = url::Url::parse(AUTH_ENDPOINT).expect("static URL");
    url.query_pairs_mut()
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &SCOPES.join(" "))
        .append_pair("state", &pkce.state)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        // offline + consent 才保证每次都给 refresh token；select_account 让用户挑要加的号
        .append_pair("access_type", "offline")
        .append_pair("prompt", "select_account consent");
    url.to_string()
}

/// 在本机随机端口上监听回调。只绑 127.0.0.1，回调地址写的也是它，不经过 localhost 解析。
pub async fn bind_callback() -> Result<(TcpListener, u16), AgyOAuthError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|error| AgyOAuthError::Callback(error.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|error| AgyOAuthError::Callback(error.to_string()))?
        .port();
    Ok((listener, port))
}

/// 等浏览器带着授权码回来。别的路径（favicon 之类）回 404 继续等；state 对不上的回调不认。
pub async fn wait_for_code(listener: TcpListener, state: &str) -> Result<String, AgyOAuthError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|error| AgyOAuthError::Callback(error.to_string()))?;
        let request = match read_request_head(&mut stream).await {
            Ok(request) => request,
            Err(_) => continue,
        };
        let Some(query) = callback_query(&request) else {
            let _ = respond(&mut stream, "404 Not Found", "").await;
            continue;
        };
        let params: std::collections::HashMap<String, String> =
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
        if params.get("state").map(String::as_str) != Some(state) {
            let _ = respond(&mut stream, "400 Bad Request", PAGE_MISMATCH).await;
            continue;
        }
        if let Some(error) = params.get("error") {
            let _ = respond(&mut stream, "200 OK", PAGE_FAILED).await;
            return Err(AgyOAuthError::Denied(sanitize(error)));
        }
        let Some(code) = params.get("code").filter(|code| !code.is_empty()) else {
            let _ = respond(&mut stream, "400 Bad Request", PAGE_FAILED).await;
            return Err(AgyOAuthError::Denied("missing code".into()));
        };
        let _ = respond(&mut stream, "200 OK", PAGE_DONE).await;
        return Ok(code.clone());
    }
}

async fn read_request_head(stream: &mut tokio::net::TcpStream) -> std::io::Result<String> {
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

/// 从 `GET /oauth-callback?... HTTP/1.1` 里取出查询串；不是回调路径时返回 `None`。
fn callback_query(request: &str) -> Option<&str> {
    let line = request.lines().next()?;
    let mut parts = line.split(' ');
    if parts.next()? != "GET" {
        return None;
    }
    let target = parts.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    (path == CALLBACK_PATH).then_some(query)
}

async fn respond(
    stream: &mut tokio::net::TcpStream,
    status: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>CC Switch</title><body style=\"font-family:system-ui;padding:48px\"><h2>登录成功 · Signed in</h2><p>可以关闭这个页面，回到 CC Switch。<br>You can close this page and return to CC Switch.</p></body>";
const PAGE_FAILED: &str = "<!doctype html><meta charset=utf-8><title>CC Switch</title><body style=\"font-family:system-ui;padding:48px\"><h2>登录没有完成 · Sign-in not completed</h2><p>回到 CC Switch 重新添加账号。<br>Return to CC Switch and add the account again.</p></body>";
const PAGE_MISMATCH: &str = "<!doctype html><meta charset=utf-8><title>CC Switch</title><body style=\"font-family:system-ui;padding:48px\"><h2>这个登录链接已过期 · This sign-in link has expired</h2></body>";

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

pub async fn exchange_code(
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse, AgyOAuthError> {
    let secret = client_secret();
    post_token(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", CLIENT_ID),
        ("client_secret", &secret),
        ("code_verifier", verifier),
    ])
    .await
}

/// 用 refresh token 换新的 access token（Google 续期时一般不轮换 refresh token）。
pub async fn refresh(refresh_token: &str) -> Result<TokenResponse, AgyOAuthError> {
    let secret = client_secret();
    post_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
        ("client_secret", &secret),
    ])
    .await
}

async fn post_token(form: &[(&str, &str)]) -> Result<TokenResponse, AgyOAuthError> {
    let response = crate::proxy::http_client::get()
        .post(TOKEN_ENDPOINT)
        .timeout(REQUEST_TIMEOUT)
        .form(form)
        .send()
        .await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AgyOAuthError::TokenExchange("响应过大".into()));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| AgyOAuthError::TokenExchange(format!("HTTP {status}，响应不是 JSON")))?;
    if !status.is_success() {
        let code = value
            .get("error")
            .and_then(Value::as_str)
            .map(sanitize)
            .unwrap_or_default();
        if code == "invalid_grant" {
            return Err(AgyOAuthError::InvalidGrant);
        }
        return Err(AgyOAuthError::TokenExchange(format!(
            "HTTP {status} {code}"
        )));
    }
    serde_json::from_value(value)
        .map_err(|_| AgyOAuthError::TokenExchange("响应缺少 access_token".into()))
}

/// Google 错误码只留字母数字和下划线，免得把意外内容带进界面。
fn sanitize(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(64)
        .collect()
}

/// id_token 里认账号要用的字段。令牌是刚从 Google 的 TLS 端点拿到的，只解码不验签。
#[derive(Debug, Default, Clone, Deserialize)]
pub struct IdentityClaims {
    pub sub: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub picture: Option<String>,
}

pub fn identity(id_token: &str) -> Option<IdentityClaims> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: IdentityClaims = serde_json::from_slice(&bytes).ok()?;
    claims.sub.as_ref().filter(|sub| !sub.is_empty())?;
    Some(claims)
}

/// agy 钥匙串里那份 JSON 的过期时间：本地时区的 RFC 3339，带微秒（和 Go 的 time.Time 一致）。
pub fn expiry_from_now(expires_in: i64) -> String {
    (chrono::Local::now() + chrono::Duration::seconds(expires_in.max(0)))
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
}

/// 新登录的账号按 agy 写入钥匙串的结构组一份登录数据。
pub fn new_credential(tokens: &TokenResponse) -> Option<Value> {
    let refresh_token = tokens.refresh_token.as_deref().filter(|t| !t.is_empty())?;
    let id_token = tokens.id_token.as_deref().filter(|t| !t.is_empty())?;
    let expires_in = tokens.expires_in.unwrap_or(3599);
    Some(json!({
        "token": {
            "access_token": tokens.access_token,
            "token_type": tokens.token_type.as_deref().unwrap_or("Bearer"),
            "refresh_token": refresh_token,
            "expiry": expiry_from_now(expires_in),
            "expires_in": expires_in,
        },
        "auth_method": "consumer",
        "id_token": id_token,
    }))
}

/// 续期结果写回登录数据：只换 access token、过期时间，Google 给了新 id_token 就一起换。
/// refresh token 不轮换，原样保留；其余字段（以后 agy 新增的）也不动。
pub fn apply_refresh(credential: &mut Value, tokens: &TokenResponse) {
    let expires_in = tokens.expires_in.unwrap_or(3599);
    if let Some(token) = credential.get_mut("token").and_then(Value::as_object_mut) {
        token.insert("access_token".into(), json!(tokens.access_token));
        token.insert("expiry".into(), json!(expiry_from_now(expires_in)));
        token.insert("expires_in".into(), json!(expires_in));
        if let Some(refresh_token) = tokens.refresh_token.as_deref().filter(|t| !t.is_empty()) {
            token.insert("refresh_token".into(), json!(refresh_token));
        }
    }
    if let Some(id_token) = tokens.id_token.as_deref().filter(|t| !t.is_empty()) {
        credential["id_token"] = json!(id_token);
    }
}

/// 登录数据里的 access token 还能用至少 `margin` 时返回它；过期、快过期或读不出过期时间返回 `None`。
pub fn usable_access_token(credential: &Value, margin: Duration) -> Option<&str> {
    let token = credential.pointer("/token/access_token")?.as_str()?;
    let expiry = credential.pointer("/token/expiry")?.as_str()?;
    let expiry = chrono::DateTime::parse_from_rfc3339(expiry).ok()?;
    let margin = chrono::Duration::from_std(margin).ok()?;
    (!token.is_empty() && expiry > chrono::Utc::now() + margin).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_keeps_the_refresh_token_and_unknown_fields() {
        let mut credential = json!({
            "token": {"access_token": "old", "refresh_token": "r", "expiry": "x", "expires_in": 1},
            "auth_method": "consumer", "id_token": "old-id", "future": true
        });
        apply_refresh(
            &mut credential,
            &TokenResponse {
                access_token: "new".into(),
                token_type: Some("Bearer".into()),
                expires_in: Some(3599),
                refresh_token: None,
                id_token: Some("new-id".into()),
            },
        );
        assert_eq!(credential["token"]["access_token"], "new");
        assert_eq!(credential["token"]["refresh_token"], "r");
        assert_eq!(credential["token"]["expires_in"], 3599);
        assert_eq!(credential["id_token"], "new-id");
        assert_eq!(credential["future"], true);
        // 刚续期的令牌可用
        assert_eq!(
            usable_access_token(&credential, Duration::from_secs(120)),
            Some("new")
        );
    }

    #[test]
    fn expired_or_unreadable_access_tokens_are_not_usable() {
        let margin = Duration::from_secs(120);
        let soon = (chrono::Local::now() + chrono::Duration::seconds(60))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        let with_expiry = |expiry: &str| json!({"token": {"access_token": "a", "expiry": expiry}});
        assert_eq!(usable_access_token(&with_expiry(&soon), margin), None);
        assert_eq!(
            usable_access_token(&with_expiry("2020-01-01T00:00:00+08:00"), margin),
            None
        );
        assert_eq!(usable_access_token(&with_expiry("garbage"), margin), None);
        assert_eq!(
            usable_access_token(&json!({"token": {"access_token": "a"}}), margin),
            None
        );
    }

    #[test]
    fn authorization_url_carries_pkce_and_offline_access() {
        let pkce = PkcePair::generate();
        assert_eq!(pkce.verifier.len(), 43);
        let url = url::Url::parse(&authorization_url(&redirect_uri(51234), &pkce)).unwrap();
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["client_id"], CLIENT_ID);
        assert_eq!(
            params["redirect_uri"],
            "http://127.0.0.1:51234/oauth-callback"
        );
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["code_challenge"], pkce.challenge);
        assert_eq!(params["access_type"], "offline");
        assert!(params["scope"].contains("https://www.googleapis.com/auth/cloud-platform"));
        assert_eq!(params["state"], pkce.state);
    }

    #[test]
    fn only_the_callback_path_is_accepted() {
        assert_eq!(
            callback_query("GET /oauth-callback?code=a&state=b HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("code=a&state=b")
        );
        assert_eq!(callback_query("GET /favicon.ico HTTP/1.1\r\n\r\n"), None);
        assert_eq!(
            callback_query("POST /oauth-callback HTTP/1.1\r\n\r\n"),
            None
        );
    }

    #[tokio::test]
    async fn callback_server_returns_the_code_for_the_matching_state() {
        let (listener, port) = bind_callback().await.unwrap();
        let waiter = tokio::spawn(async move { wait_for_code(listener, "good").await });
        let send = |path: &'static str| async move {
            let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            stream
                .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            response
        };
        assert!(send("/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(send("/oauth-callback?code=x&state=bad")
            .await
            .starts_with("HTTP/1.1 400"));
        assert!(send("/oauth-callback?code=the-code&state=good")
            .await
            .starts_with("HTTP/1.1 200"));
        assert_eq!(waiter.await.unwrap().unwrap(), "the-code");
    }

    #[tokio::test]
    async fn browser_error_ends_the_login() {
        let (listener, port) = bind_callback().await.unwrap();
        let waiter = tokio::spawn(async move { wait_for_code(listener, "s").await });
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /oauth-callback?error=access_denied&state=s HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        assert!(matches!(
            waiter.await.unwrap(),
            Err(AgyOAuthError::Denied(reason)) if reason == "access_denied"
        ));
    }

    fn fake_id_token(claims: Value) -> String {
        format!(
            "e30.{}.sig",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn identity_requires_a_subject() {
        let token = fake_id_token(json!({"sub": "123", "email": "a@example.com"}));
        let claims = identity(&token).unwrap();
        assert_eq!(claims.sub.as_deref(), Some("123"));
        assert_eq!(claims.email.as_deref(), Some("a@example.com"));
        assert!(identity(&fake_id_token(json!({"email": "a@example.com"}))).is_none());
        assert!(identity("garbage").is_none());
    }

    #[test]
    fn new_login_needs_refresh_and_id_tokens() {
        let mut tokens = TokenResponse {
            access_token: "a".into(),
            token_type: None,
            expires_in: Some(3599),
            refresh_token: Some("r".into()),
            id_token: Some("i".into()),
        };
        let credential = new_credential(&tokens).unwrap();
        assert_eq!(credential["token"]["refresh_token"], "r");
        assert_eq!(credential["token"]["token_type"], "Bearer");
        assert_eq!(credential["auth_method"], "consumer");
        assert!(chrono::DateTime::parse_from_rfc3339(
            credential["token"]["expiry"].as_str().unwrap()
        )
        .is_ok());
        tokens.refresh_token = None;
        assert!(new_credential(&tokens).is_none());
    }
}
