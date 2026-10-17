//! 鉴权中间件：三类 scope 走同一个闸门。
//!
//! `AuthScope` 描述的是**可接受的凭据通道**，不是「要不要保护」：
//!
//! | scope | 接受的通道（按尝试顺序） | 由来 |
//! |-------|--------------------------|------|
//! | `Protected` | `Authorization: Bearer <token>` → 会话 cookie | 远端浏览器打开内嵌 Web UI 后，页面里的 `fetch` 只能靠浏览器自动带 cookie |
//! | `BrowserSafe` | 会话 cookie | 载体在协议层没有地方放 token（`<img src>`、`EventSource`） |
//! | `Public` | 无 | 无凭据时也必须可达（配对端点、根页面） |
//!
//! 三类**全部**经过同一段判定：`decide` 先否决 Origin（403），再归类通道，
//! 最后过凭据闸门（401）。`Public` 只豁免第三段，绝不豁免第一段。

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::cookie::REMOTE_LISTENER_IS_TLS;
use super::policy::{decide, Cred, Decision, DenyReason, OriginSet, Peer};
use super::secret::{load_clients, AUTH_SUBDIR, SECRET_FILE};
use super::token::ClientDigest;
use crate::remote::RemoteConfig;

/// 鉴权闸门的中间件返回值。
pub type AuthFuture = Pin<Box<dyn Future<Output = Response> + Send>>;

/// 路由的鉴权 scope。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScope {
    /// 载体能自己带 `Authorization` 头，或浏览器自动携带的会话 cookie。
    Protected,
    /// 载体在协议层没有地方放 token，只能靠 cookie。
    BrowserSafe,
    /// 无凭据也必须可达。
    Public,
}

/// 一条凭据载体的通道。没有 `None` 变体：「这个 scope 不要求凭据」由
/// [`AuthScope::credential_channels`] 返回空切片表达，不另设一个哨兵值 ——
/// 哨兵值一旦混进遍历就是一个能一路 `continue` 过去的空转项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialChannel {
    Header,
    Cookie,
}

impl AuthScope {
    /// 该 scope 接受的凭据通道，按**尝试顺序**排列。
    ///
    /// `Protected` 列两条，是因为「这条路由属于谁」与「哪条载体能自己设 header」
    /// 是两件事。远端浏览器打开内嵌 Web UI 之后，页面里的 `fetch('/api/providers')`
    /// 走的就是 `Protected` 路由，而浏览器能自动携带的只有 cookie ——
    /// `Authorization` 要带上就得先把凭据交给页面脚本，那正是会话 cookie 这条路
    /// 设计上要避免的（见 `html.rs` 里配对面板的注释）。
    ///
    /// **列两条不等于放宽要求。** 两条通道验出来的都是同一档 `Cred::Session`
    /// （`verify_credential` 的 Header 分支比 `token_hash`、Cookie 分支比
    /// `cookie_hash`，两者是**同一条** `ClientToken` 记录上的两个摘要，签发与撤销
    /// 都在配对那一处）。判定表 [`super::policy::decide`] 连「凭据来自哪条载体」
    /// 这一维都没有 —— 12 格 × 3 态共 36 个出口，这一改一个都不动。
    ///
    /// 顺带一提，`BrowserSafe` 早就在走 cookie → `Cred::Session` 这条路了，
    /// 所以 cookie 凭据并不是被这次改动**新引进**判定表的。
    ///
    /// 顺序是 Bearer 在前，且**不回落**：某个载体一旦在请求里出现，它就必须验过。
    /// 带了错的 `Authorization` 却靠 cookie 过关，等于让「你拿来证明身份的东西」
    /// 和「实际让你进来的东西」不是同一件。
    fn credential_channels(self) -> &'static [CredentialChannel] {
        match self {
            AuthScope::Protected => &[CredentialChannel::Header, CredentialChannel::Cookie],
            AuthScope::BrowserSafe => &[CredentialChannel::Cookie],
            AuthScope::Public => &[],
        }
    }

    /// 该 scope 是否不要求凭据（只有 `Public` 是）。
    ///
    /// 由 [`Self::credential_channels`] 推出来，而不是各写一份：两者不一致时的
    /// 故障是静默的 —— 给某个 scope 添了一条通道，却忘了它已经不是「不要求凭据」
    /// 的那一类。
    fn expects_no_credential(self) -> bool {
        self.credential_channels().is_empty()
    }
}

/// 会话档位：这条请求的调用方**到底有没有持有通过凭据闸门的凭据**。
///
/// 它回答的不是「放不放行」（那是 [`Outcome`]），而是「放行之后能给多少」。
/// 两者必须分开：判定格第 7 格（Loopback / 无 Origin / 无代理信号）对 `Cred::None`
/// 判 `Allow` —— 内嵌 Web UI 在配对之前就要能打开，改成 401 会把桌面端自己的
/// 页面一起锁死。但「放行」不等于「放行全部细节」：带 `provider_id` / `name`
/// 的切换事件流是供应商拓扑，同机无凭据调用方没有理由读到它。
///
/// 因此本档位由中间件在 `Allow` 出口经 `extensions_mut()` 注入，下游 handler
/// （如 `/api/events`）据此选择视图，而不是自己重新解析凭据 —— 凭据只应在闸门
/// 这一处被解释一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAudience {
    /// 凭据闸门真的验过一条凭据（header token 或会话 cookie 都算）。
    Authenticated,
    /// 匿名：loopback 放行或 `Public` 豁免的无凭据请求。
    AnonymousLoopback,
}

/// 「这一发请求是谁」—— 两段式绑定范围用它把确认绑回发起方。
///
/// 三种身份来自三条互相独立的入口：
///
/// - [`Self::Credential`]：远程 REST 请求，身份是鉴权中间件验过的那条凭据摘要。
///   Bearer 与 cookie 各自对应同一条 `ClientToken` 记录上的两个摘要，因此
///   「用同一个 Bearer 申请、再用同一个 cookie 确认」会被判成**不同**会话 ——
///   这比按客户端名分组更严，也更符合「同一个已认证会话」的字面意思。
/// - [`Self::LocalLoopback`]：本机回环、未带凭据的请求（配对前打开的内嵌页面）。
/// - [`Self::LocalWindow`]：桌面端 Tauri 命令，不经过 HTTP 中间件。
#[derive(Debug, Clone, Copy)]
pub enum SessionIdentity {
    /// 已验过的凭据摘要。
    Credential(ClientDigest),
    /// 本机回环匿名请求。
    LocalLoopback,
    /// 桌面端进程内的窗口。
    LocalWindow,
}

impl SessionIdentity {
    /// 两个身份是不是同一个会话。
    ///
    /// 摘要比较必须走 [`ClientDigest::ct_eq`]（恒时）—— `ClientDigest` 不提供
    /// `PartialEq`，这里因此故意不派生 `PartialEq` 给整个枚举，避免有人写出
    /// 一条经由字段短路的 `==`。
    pub fn same_session(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Credential(a), Self::Credential(b)) => a.ct_eq(b),
            (Self::LocalLoopback, Self::LocalLoopback) => true,
            (Self::LocalWindow, Self::LocalWindow) => true,
            _ => false,
        }
    }
}

impl SessionAudience {
    /// 由「凭据闸门是否真的验过一条凭据」决定档位。
    ///
    /// 被豁免放行的（`Public` scope、loopback 无凭据）一律 [`Self::AnonymousLoopback`]
    /// —— 豁免的含义就是「没验过凭据」，不能因为请求被放行就顺带升级成已鉴权。
    fn from_verified(authenticated: bool) -> Self {
        if authenticated {
            Self::Authenticated
        } else {
            Self::AnonymousLoopback
        }
    }
}

/// 闸门的三种出口。`Allow` 携带会话档位，由中间件注入请求 extension。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Allow(SessionAudience),
    ForbiddenOrigin,
    Unauthorized,
}

/// 本机代理的转发信号头（判定表第三列的三个来源）。
///
/// 三个都是**标准转发头**：`tailscale serve` 自 1.44 起自动注入
/// `Tailscale-User-Login`，nginx / Caddy 等反向代理默认注入 `X-Forwarded-For`
/// 与 `Forwarded`。所以这条规则**不需要任何转发器做额外配置**就会生效 ——
/// 换成一个只有本仓自己认识的自定义头，就会因为没人设置而静默失效。
///
/// 请求方可以任意伪造它们，但伪造**只会把要求抬高**：命中即把请求归入
/// [`Channel::LocalProxy`]（最低凭据档位 1），而不是本机直连通道（档位 0）。
///
/// 这三个头**绝不参与 peer 判定** —— peer 只能来自 `ConnectInfo`。一旦有人拿
/// 它们去判来源，远端随手写一行 `X-Forwarded-For: 127.0.0.1` 就能把自己伪装成
/// 本机页面，那才是真正的洞。`e2e` 里有一条用例专门钉住这条边界。
const LOCAL_PROXY_SIGNALS: [&str; 3] = ["tailscale-user-login", "x-forwarded-for", "forwarded"];

/// 承载凭据的 `Authorization` 方案名。
const BEARER_SCHEME: &str = "bearer";

/// 会话 cookie 名。
const SESSION_COOKIE: &str = "cc_switch_session";

/// 鉴权中间件的类型。三类 scope 共用同一个中间件函数与同一个层类型，
/// 差别只在传进去的 `AuthScope`。
///
/// 中间件状态里的第三项是 [`OriginSet`] —— Origin 判据编译好的结果，与 CORS
/// 装配读的是同一个 `allowed_origins(&cfg, tls)`。中间件**不持有任何自己写的
/// origin 名单**：各写一份的故障是静默的（显式白名单模式下 CORS 放行、
/// 中间件 403，用户的配置看起来保存成功却毫无作用）。
type AuthState = (AuthScope, PathBuf, OriginSet);

type AuthLayer = axum::middleware::FromFnLayer<
    fn(State<AuthState>, Request<Body>, Next) -> AuthFuture,
    AuthState,
    (State<AuthState>, Request<Body>),
>;

/// 构造 scope 对应的鉴权中间件。
///
/// `config_dir` 由装配层显式传入，而不是在这里读全局：夹具必须能指向自己的
/// 临时目录，绝不能因为「进程里恰好能读到 `~/.cc-switch`」就去验用户真实的
/// 凭据库。配对子系统写的是同一个目录，两端共用一把 secret。
///
/// `cfg` 是暴露边界，**不是**可信输入之外的什么东西：它决定 Origin 名录，
/// 名录则与 `CorsLayer` 的 `Access-Control-Allow-Origin` 同源。同样的
/// `(cfg, tls)` 在两边算一次，编译期求值而不是每请求现算。
pub fn auth_layer(scope: AuthScope, config_dir: PathBuf, cfg: &RemoteConfig) -> AuthLayer {
    axum::middleware::from_fn_with_state(
        (
            scope,
            config_dir,
            OriginSet::from_config(cfg, REMOTE_LISTENER_IS_TLS),
        ),
        gate as fn(State<AuthState>, Request<Body>, Next) -> AuthFuture,
    )
}

/// 中间件本体。
fn gate(
    State((scope, config_dir, origins)): State<AuthState>,
    mut req: Request<Body>,
    next: Next,
) -> AuthFuture {
    Box::pin(async move {
        let (outcome, credential) = evaluate(scope, &config_dir, &origins, &req);
        match outcome {
            Outcome::Allow(audience) => {
                // 会话档位随请求往下传：下游 handler 据此选视图，不必重新解析凭据。
                req.extensions_mut().insert(audience);
                // 两段式绑定范围要的是「同一个已认证会话」：已验过的凭据摘要
                // 就是最常见的那种身份；没有凭据而被回环豁免放行的请求归入
                // `LocalLoopback`，不会与任何远程凭据相等。
                let identity = match credential {
                    Some(digest) => SessionIdentity::Credential(digest),
                    None => SessionIdentity::LocalLoopback,
                };
                req.extensions_mut().insert(identity);
                next.run(req).await
            }
            Outcome::ForbiddenOrigin => forbidden(),
            Outcome::Unauthorized => unauthorized(),
        }
    })
}

/// 请求的判定结果。与 [`decide`] 同序：Origin 否决 → 通道归类 → 凭据闸门。
///
/// 第二个返回值是「这次放行走的是哪条凭据」：只有凭据闸门真的换出摘要才算
/// 有身份，其余一律 `None`。
fn evaluate(
    scope: AuthScope,
    config_dir: &Path,
    origins: &OriginSet,
    req: &Request<Body>,
) -> (Outcome, Option<ClientDigest>) {
    // 凭据为零时连文件都不读：本机内嵌页面在配对之前就存在。
    let cred = credential_of(req, scope, config_dir);
    let credential = match cred {
        Cred::Session(digest) => Some(digest),
        _ => None,
    };
    // 只有凭据闸门真的换出摘要才算「已鉴权」；`Cred::None` 一律匿名。
    let audience = SessionAudience::from_verified(credential.is_some());
    let decision = decide(
        peer_of(req),
        header_str(req.headers(), header::ORIGIN),
        has_proxy_signal(req.headers()),
        cred,
        origins,
    );

    let outcome = match decision {
        Decision::Deny(DenyReason::ForbiddenOrigin) => Outcome::ForbiddenOrigin,
        // Public 只豁免凭据闸门；Origin 否决在上面已经先行返回。
        Decision::Deny(DenyReason::Unauthorized) if scope.expects_no_credential() => {
            Outcome::Allow(audience)
        }
        Decision::Deny(DenyReason::Unauthorized) => Outcome::Unauthorized,
        Decision::Allow => Outcome::Allow(audience),
    };
    (outcome, credential)
}

/// 来源地址归类。拿不到来源就是「不知道是谁来的」，按最严的一侧算。
fn peer_of(req: &Request<Body>) -> Peer {
    match req.extensions().get::<ConnectInfo<SocketAddr>>() {
        Some(peer) if peer.0.ip().is_loopback() => Peer::Loopback,
        Some(_) => Peer::NonLoopback,
        None => Peer::NonLoopback,
    }
}

/// 请求是否携带任一代理转发信号头（判定表第三列）。
///
/// 按**在场**判定，不看取值：头的值由转发器自己填，本仓对它们的内容没有任何
/// 语义承诺，`Forwarded` 与 `X-Forwarded-For` 的语法差异也不影响结论。空值同样
/// 算在场（与改动前 `contains_key` 的语义逐字一致）：判定表第三列从来只有
/// 「有 / 没有」两态，没有第三态可表达「有但没填」。
fn has_proxy_signal(headers: &HeaderMap) -> bool {
    headers
        .keys()
        .any(|name| LOCAL_PROXY_SIGNALS.contains(&name.as_str()))
}

/// 从请求里取凭据；没有任何载体带凭据就零 I/O 短路成 [`Cred::None`]。
///
/// 逐条试 [`AuthScope::credential_channels`]，**第一条带了凭据的通道就是终局**：
/// 它验过就 [`Cred::Session`]，验不过就 [`Cred::None`]，不再往后面的通道退。
///
/// 零 I/O 那条仍然成立：`presented_credential` 只读 header、不碰磁盘，所以
/// 「一条载体都没带凭据」的请求走完全程也不会去读 secret 文件。
fn credential_of(req: &Request<Body>, scope: AuthScope, config_dir: &Path) -> Cred {
    for &channel in scope.credential_channels() {
        let Some(presented) = presented_credential(req, channel) else {
            continue;
        };
        return match verify_credential(config_dir, &presented, channel) {
            Some(hash) => Cred::Session(hash),
            None => Cred::None,
        };
    }
    Cred::None
}

/// 按通道取出请求携带的凭据明文。
fn presented_credential(req: &Request<Body>, channel: CredentialChannel) -> Option<String> {
    match channel {
        CredentialChannel::Header => bearer_token(req.headers()),
        CredentialChannel::Cookie => session_cookie(req.headers()),
    }
}

/// `Authorization: Bearer <token>`，只认恰好一个凭据。
fn bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = header_str(headers, header::AUTHORIZATION)?;
    let (scheme, token) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case(BEARER_SCHEME) {
        return None;
    }
    let token = token.trim();
    if token.is_empty() || token.split_whitespace().count() != 1 {
        return None;
    }
    Some(token.to_string())
}

/// `Cookie: cc_switch_session=<value>`。
fn session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    let raw = header_str(headers, header::COOKIE)?;
    raw.split(';')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| name.trim() == SESSION_COOKIE)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 拿请求目录下的 secret 与凭据库，把明文凭据换成摘要。
///
/// **任何一步失败都返回 `None`**：读不出 secret、读不出库、库里没有这条 ——
/// 一律按「没有凭据」处理，不放行。
///
/// 返回 [`ClientDigest`] 而不是裸 `[u8; 32]`：已存摘要一旦离开这一层就
/// 只以「没有 `PartialEq`、唯一比较是 `ct_eq`」的类型形态存在，中间件的
/// 私有状态与调用方都拿不到可 `==` 的裸数组。
fn verify_credential(
    config_dir: &Path,
    presented: &str,
    channel: CredentialChannel,
) -> Option<ClientDigest> {
    let secret = read_secret(config_dir)?;
    let clients = load_clients(config_dir).ok()?;
    clients.iter().find_map(|client| match channel {
        CredentialChannel::Header => client
            .verify(&secret, presented)
            .then_some(client.token_hash),
        CredentialChannel::Cookie => client
            .cookie_hash
            .filter(|stored| super::token::verify(&secret, presented, stored.as_bytes())),
    })
}

/// 只读 secret：中间件绝不因为「来了一发请求」就去创建凭据文件。
fn read_secret(config_dir: &Path) -> Option<[u8; 32]> {
    let path = config_dir.join(AUTH_SUBDIR).join(SECRET_FILE);
    std::fs::read(path).ok()?.try_into().ok()
}

fn header_str(headers: &axum::http::HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({ "error": "forbidden_origin" })),
    )
        .into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"cc-switch-remote\"",
        )],
        axum::Json(serde_json::json!({ "error": "unauthorized" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::ROUTE_SCOPE_TABLE;
    use axum::routing::get;
    use axum::Router;
    use std::net::{IpAddr, Ipv4Addr};
    use tower::Service;

    async fn ok_handler() -> &'static str {
        "ok"
    }

    /// 剥掉行尾注释（区分字符串字面量），让断言读的是代码而不是说明。
    fn strip_comments(src: &str) -> String {
        let mut out = String::new();
        for line in src.lines() {
            let bytes = line.as_bytes();
            let mut in_str = false;
            let mut cut = bytes.len();
            let mut i = 0;
            while i < bytes.len() {
                match bytes[i] {
                    b'\\' if in_str => i += 1,
                    b'"' => in_str = !in_str,
                    b'/' if !in_str && bytes.get(i + 1) == Some(&b'/') => {
                        cut = i;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            let code = line[..cut].trim();
            if code.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(code);
        }
        out
    }

    /// `register(...)` 调用点数，跳过 `fn register(` 这条定义本身。
    fn register_call_count(src: &str) -> usize {
        let text = strip_comments(src);
        let mut count = 0;
        let mut from = 0;
        while let Some(rel) = text[from..].find("register(") {
            let at = from + rel;
            let before_ok = text[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let prev_word = text[..at]
                .split_whitespace()
                .next_back()
                .unwrap_or_default();
            if before_ok && prev_word != "fn" {
                count += 1;
            }
            from = at + "register(".len();
        }
        count
    }

    /// 裸 `.route("/…")` 字符串字面量的出现次数。
    fn bare_route_literals(src: &str) -> usize {
        strip_comments(src).matches(".route(\"/").count()
    }

    fn request_from(peer: IpAddr, path: &str) -> Request<Body> {
        let mut req = Request::builder()
            .uri(path)
            .body(Body::empty())
            .expect("valid request");
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer, 40_000)));
        req
    }

    async fn status(app: Router, req: Request<Body>) -> StatusCode {
        let mut app = app;
        std::future::poll_fn(|cx| Service::<Request<Body>>::poll_ready(&mut app, cx))
            .await
            .expect("router is always ready");
        Service::<Request<Body>>::call(&mut app, req)
            .await
            .expect("router error is infallible")
            .status()
    }

    /// scope 在装配处的写法（`AuthScope::X` 里的 `X`）。
    fn scope_token(scope: AuthScope) -> &'static str {
        match scope {
            AuthScope::Protected => "Protected",
            AuthScope::BrowserSafe => "BrowserSafe",
            AuthScope::Public => "Public",
        }
    }

    fn loopback() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    fn lan() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))
    }

    /// 非 loopback 且没带凭据 → 401。
    ///
    /// 这是整条链路最要紧的一格：局域网里的任何一台机器都不能匿名调用
    /// 受保护路由，403 也不行 —— 403 说的是 Origin 不对，401 说的是「你没证明
    /// 你是谁」，后者才是这里的事实。
    #[tokio::test]
    async fn middleware_denies_nonloopback_without_token() {
        let dir = tempfile::tempdir().expect("isolated config dir");
        let app: Router = crate::remote::register(
            Router::new(),
            "/api/health",
            axum::http::Method::GET,
            AuthScope::Protected,
            get(ok_handler),
            dir.path(),
            &RemoteConfig::default(),
        )
        .with_state(());

        assert_eq!(
            status(app.clone(), request_from(lan(), "/api/health")).await,
            StatusCode::UNAUTHORIZED,
            "非 loopback 无凭据必须 401"
        );

        // 同一路由，本机内嵌页面（loopback + 无代理信号）不受影响：
        // 全部拒绝会让桌面端自己的 Web UI 一起打不开。
        assert_eq!(
            status(app, request_from(loopback(), "/api/health")).await,
            StatusCode::OK,
            "本机内嵌访问仍应放行"
        );
    }

    /// `BrowserSafe` 不是绕过：它只是换了一条凭据通道。
    ///
    /// 如果把 `BrowserSafe` 当成「免鉴权」，那 `<img src="/api/icon">` 就成了一条
    /// 匿名可读的口子 —— 讽刺的是这条路由正是被归到 `BrowserSafe` 的原因。
    #[tokio::test]
    async fn browser_safe_is_not_a_bypass() {
        let dir = tempfile::tempdir().expect("isolated config dir");
        let app: Router = crate::remote::register(
            Router::new(),
            "/api/icon",
            axum::http::Method::GET,
            AuthScope::BrowserSafe,
            get(ok_handler),
            dir.path(),
            &RemoteConfig::default(),
        )
        .with_state(());

        assert_eq!(
            status(app.clone(), request_from(lan(), "/api/icon")).await,
            StatusCode::UNAUTHORIZED,
            "BrowserSafe 仍受中间件约束：远端无凭据必须 401"
        );
        assert_eq!(
            status(app.clone(), request_from(loopback(), "/api/icon")).await,
            StatusCode::OK,
            "BrowserSafe 在本机内嵌页面上照常放行"
        );

        // 换一条 scope 对照：Public 才是不要求凭据的那一类。
        let public: Router = crate::remote::register(
            Router::new(),
            "/",
            axum::http::Method::GET,
            AuthScope::Public,
            get(ok_handler),
            dir.path(),
            &RemoteConfig::default(),
        )
        .with_state(());
        assert_eq!(
            status(public, request_from(lan(), "/")).await,
            StatusCode::OK,
            "Public 路由在无凭据时必须可达，否则配对端点进不来"
        );
    }

    /// 表条目数 == `register(...)` 调用数。
    ///
    /// 只比数量是不够的：新增一条路由却忘了在表里登记，两边会同时各多一个、
    /// 数量照样相等。所以本测试之外还有 `table_matches_every_registration`。
    #[test]
    fn route_scope_table_matches_registration_count() {
        let src = include_str!("../mod.rs");
        assert_eq!(
            ROUTE_SCOPE_TABLE.len(),
            17,
            "业务路由表必须是 17 条：16 条 /api/* 加根部 GET /"
        );
        assert_eq!(
            register_call_count(src),
            ROUTE_SCOPE_TABLE.len(),
            "register(...) 调用数必须与 ROUTE_SCOPE_TABLE 条目数相等"
        );
    }

    /// 装配后 `mod.rs` 里不再有任何裸 `.route("/…")` 字面量。
    ///
    /// 业务路由只能经受控 `register()` 进 Router；裸调用意味着某条路由绕过了
    /// scope 声明，也就绕过了鉴权分区。
    #[test]
    fn bare_route_calls_are_zero_after_refactor() {
        let src = include_str!("../mod.rs");
        assert_eq!(
            bare_route_literals(src),
            0,
            "mod.rs 中不得再有裸 .route(\"/…\") 字面量：所有业务路由都必须走 register()"
        );
    }

    /// 表里的每一条都真的被注册了，且 scope 一致。
    ///
    /// 这条比计数更难骗：`register(...)` 传的 scope 与表里那一行不一致，
    /// 而路径又恰好在表里 —— 计数照样平衡，但鉴权分区是错的。
    #[test]
    fn table_matches_every_registration() {
        let text = strip_comments(include_str!("../mod.rs"));
        for (path, scope) in ROUTE_SCOPE_TABLE {
            let token = format!("AuthScope::{}", scope_token(*scope));
            let needle = format!("\"{path}\"");
            let at = text
                .find(&needle)
                .unwrap_or_else(|| panic!("{path} 必须出现在 mod.rs 的装配处"));
            // 调用形如 register(router, "<path>", Method::GET, AuthScope::X, …)
            let call = &text[at..];
            assert!(
                call[..call.find(')').unwrap_or(call.len())].contains(&token),
                "{path} 登记为 {scope:?}，但装配处传的 scope 不是 {scope:?}"
            );
        }
    }

    /// 表本身的两条不变式：17 条、且每条都有 scope。
    #[test]
    fn scope_table_declares_every_route_exactly_once() {
        let paths: Vec<&str> = ROUTE_SCOPE_TABLE.iter().map(|(p, _)| *p).collect();
        let mut unique = paths.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), paths.len(), "路由表里有重复路径");

        assert!(paths.contains(&"/"), "根页面必须是无凭据可达的 Public");
        for expected in ["/api/icon", "/api/provider-icons/:name", "/api/events"] {
            let scope = ROUTE_SCOPE_TABLE
                .iter()
                .find(|(p, _)| *p == expected)
                .map(|(_, s)| *s)
                .unwrap_or_else(|| panic!("{expected} 必须登记在表里"));
            assert_eq!(
                scope,
                AuthScope::BrowserSafe,
                "{expected} 的载体放不了 Authorization 头，只能是 BrowserSafe"
            );
        }
    }

    /// Origin 在最前：三类 scope 都一样，非白名单 Origin 一律 403，凭据救不了。
    #[tokio::test]
    async fn origin_veto_comes_before_the_credential_gate_for_every_scope() {
        for (path, scope) in [
            ("/", AuthScope::Public),
            ("/api/health", AuthScope::Protected),
        ] {
            let dir = tempfile::tempdir().expect("isolated config dir");
            let app: Router = crate::remote::register(
                Router::new(),
                path,
                axum::http::Method::GET,
                scope,
                get(ok_handler),
                dir.path(),
                &RemoteConfig::default(),
            )
            .with_state(());

            let mut req = request_from(lan(), path);
            req.headers_mut().insert(
                header::ORIGIN,
                "https://evil-127.0.0.1.example".parse().unwrap(),
            );
            assert_eq!(
                status(app, req).await,
                StatusCode::FORBIDDEN,
                "{path}（{scope:?}）：Origin 不在白名单必须 403，Public 也不例外"
            );
        }
    }

    /// 凭据解析：只认单值 Bearer，Cookie 只认本服务的会话 cookie。
    #[test]
    fn credential_parsing_is_single_valued() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(bearer_token(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers).as_deref(), Some("abc"));
        headers.insert(header::AUTHORIZATION, "bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers).as_deref(), Some("abc"));
        headers.insert(header::AUTHORIZATION, "Pairing abc".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer a b".parse().unwrap());
        assert_eq!(bearer_token(&headers), None);

        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(session_cookie(&headers), None);
        headers.insert(header::COOKIE, "other=1".parse().unwrap());
        assert_eq!(session_cookie(&headers), None);
        headers.insert(
            header::COOKIE,
            "other=1; cc_switch_session=s1".parse().unwrap(),
        );
        assert_eq!(session_cookie(&headers).as_deref(), Some("s1"));
    }

    /// 凭据核验：摘要库能换出凭据，错的明文换不出来，读不到文件一律 None。
    ///
    /// 注意断言形态：`verify_credential` 返回 [`ClientDigest`]，所以这里
    /// **只能**用 `ct_eq` 比 —— 上一版写的是 `assert_eq!(…, Some(hash))`，
    /// 那条写法在类型换掉之后直接编译不过，正是「由构造保证」的意思。
    #[test]
    fn verify_credential_uses_the_stored_digest_only() {
        use super::super::secret::{save_clients, CLIENTS_FILE, SECRET_MODE};
        use super::super::ClientToken;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("isolated config dir");
        let secret = super::super::secret::load_or_create(dir.path()).expect("secret");
        let client = ClientToken::new("cli", &secret, "plain-token");
        save_clients(dir.path(), std::slice::from_ref(&client)).expect("persist");

        let expected = client.token_hash;
        let got = verify_credential(dir.path(), "plain-token", CredentialChannel::Header)
            .expect("正确明文必须换出摘要");
        assert!(got.ct_eq(&expected), "换出的摘要必须与库里那条一致");
        assert!(
            verify_credential(dir.path(), "wrong", CredentialChannel::Header).is_none(),
            "错误明文换不出凭据"
        );
        // cookie 通道没有签发过 cookie —— 换不出任何东西。
        assert!(
            verify_credential(dir.path(), "plain-token", CredentialChannel::Cookie).is_none(),
            "Cookie 通道不得借用 token 摘要放行"
        );

        // 空目录：没有 secret 就一条都验不过（fail-closed）。
        let empty = tempfile::tempdir().expect("isolated config dir");
        assert!(
            verify_credential(empty.path(), "plain-token", CredentialChannel::Header).is_none(),
            "空目录必须 fail-closed"
        );
        let secret_path = empty.path().join(AUTH_SUBDIR).join(SECRET_FILE);
        assert!(!secret_path.exists(), "读凭据不得顺手创建 secret 文件");
        save_clients(empty.path(), std::slice::from_ref(&client)).expect("persist");
        assert_eq!(
            fs::metadata(empty.path().join(AUTH_SUBDIR).join(CLIENTS_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            SECRET_MODE,
            "凭据库同样必须是 0600"
        );
    }

    /// 通道表：`Protected` 两条且顺序固定，`BrowserSafe` 一条，`Public` 空。
    ///
    /// `expects_no_credential` 由通道表推出来，所以这里一并钉住 —— 两者各写一份时的
    /// 故障是静默的：给某个 scope 添了一条通道，却忘了它已经不是「不要求凭据」那一类。
    #[test]
    fn credential_channels_are_ordered_and_derived() {
        assert_eq!(
            AuthScope::Protected.credential_channels(),
            &[CredentialChannel::Header, CredentialChannel::Cookie][..],
            "Protected 必须先试 Bearer，再回落到会话 cookie"
        );
        assert_eq!(
            AuthScope::BrowserSafe.credential_channels(),
            &[CredentialChannel::Cookie][..],
            "BrowserSafe 只走 cookie：它的载体在协议层放不了 header"
        );
        assert!(
            AuthScope::Public.credential_channels().is_empty(),
            "Public 一条通道都不读"
        );

        assert!(AuthScope::Public.expects_no_credential());
        assert!(!AuthScope::Protected.expects_no_credential());
        assert!(!AuthScope::BrowserSafe.expects_no_credential());
    }

    /// 凭据取用：**Bearer 优先、且不回落**。
    ///
    /// 五条互相独立，第 4 条是本改动里唯一带取舍的一条：
    ///
    /// 1. `Protected` + 有效 cookie → 换成摘要（远端浏览器打开内嵌 Web UI 之后，
    ///    页面里的 `fetch` 只能靠浏览器自动带 cookie，塞不进 header）；
    /// 2. `Protected` + 无效 cookie → [`Cred::None`]；
    /// 3. `Protected` + **有效 Bearer** → 照旧换成摘要（不许被这次改动弄坏）；
    /// 4. `Protected` + **无效 Bearer** + 有效 cookie → [`Cred::None`]。
    ///    某个载体一旦在请求里**出现**，它就必须验过。否则「你拿来证明身份的东西」
    ///    和「实际让你进来的东西」可以是两件不同的东西 —— 而审计与告警看到的都是前者。
    /// 5. `BrowserSafe` 两条反向对照：cookie 收、Bearer 不收。
    ///
    /// 用的是真实落库的一条 `ClientToken`（两个摘要落在**同一条**记录上），
    /// 而不是手搓摘要 —— 摘要的构造方式本身就是这条契约的一部分。
    #[test]
    fn credential_of_tries_header_first_and_never_falls_back() {
        use super::super::secret::{load_or_create, save_clients};
        use super::super::token::token_hash;
        use super::super::ClientToken;

        const HEADER_TOKEN: &str = "header-token";
        const SESSION: &str = "session-value";
        const WRONG: &str = "not-the-session-value";

        let dir = tempfile::tempdir().expect("isolated config dir");
        let secret = load_or_create(dir.path()).expect("secret");
        let mut client = ClientToken::new("web", &secret, HEADER_TOKEN);
        // 生产路径同款：见 `pairing::handlers::issue_credential` —— cookie 明文
        // 独立生成、摘要落在那条记录的另一格上。
        client.cookie_hash = Some(token_hash(&secret, SESSION));
        save_clients(dir.path(), std::slice::from_ref(&client)).expect("persist");

        let header_digest = client.token_hash;
        let cookie_digest = client.cookie_hash.expect("上面刚设过");

        let probe = |scope: AuthScope, bearer: Option<&str>, cookie: Option<&str>| {
            let mut req = Request::builder()
                .uri("/api/providers")
                .body(Body::empty())
                .expect("valid request");
            if let Some(value) = bearer {
                req.headers_mut().insert(
                    header::AUTHORIZATION,
                    format!("Bearer {value}").parse().unwrap(),
                );
            }
            if let Some(value) = cookie {
                req.headers_mut().insert(
                    header::COOKIE,
                    format!("{SESSION_COOKIE}={value}").parse().unwrap(),
                );
            }
            match credential_of(&req, scope, dir.path()) {
                Cred::Session(digest) => Some(digest),
                Cred::None => None,
                // `credential_of` 只产出 `Session` / `None`：两条通道验出来的都是
                // 同一档。`Cred::Token` 在判定表里有位置（第 8/10 格要求它），
                // 但没有任何一条通道产出它。真出现了就是通道表被改动了，
                // 这里要响，不能悄悄并进「没凭据」那一支。
                Cred::Token(_) => {
                    panic!("credential_of 产出了 Cred::Token —— 通道表被改动了")
                }
            }
        };

        // 1. Protected + 有效 cookie。
        let got = probe(AuthScope::Protected, None, Some(SESSION)).expect(
            "Protected 必须接受会话 cookie：远端浏览器的页面里 `fetch('/api/providers')` \
             只带得了浏览器自动携带的那一条",
        );
        assert!(
            got.ct_eq(&cookie_digest),
            "换出的必须是记录上的 cookie 摘要"
        );

        // 2. Protected + 无效 cookie。
        assert!(
            probe(AuthScope::Protected, None, Some(WRONG)).is_none(),
            "无效 cookie 必须 401"
        );

        // 3. Protected + 有效 Bearer（回归）。
        assert!(
            probe(AuthScope::Protected, Some(HEADER_TOKEN), None)
                .is_some_and(|d| d.ct_eq(&header_digest)),
            "Bearer 通道不能被这次改动弄坏"
        );

        // 4. 不回落。
        assert!(
            probe(
                AuthScope::Protected,
                Some("never-issued-token"),
                Some(SESSION)
            )
            .is_none(),
            "Bearer 在场且验不过时不许回落到 cookie：否则「你拿来证明身份的东西」\
             和「实际让你进来的东西」可以是两件不同的东西"
        );

        // 5. BrowserSafe 反向对照。
        assert!(
            probe(AuthScope::BrowserSafe, None, Some(SESSION)).is_some(),
            "BrowserSafe 照旧接受会话 cookie"
        );
        assert!(
            probe(AuthScope::BrowserSafe, Some(HEADER_TOKEN), None).is_none(),
            "BrowserSafe 不接受 Bearer —— 它的载体在协议层放不了 header，这不是可选项"
        );

        // 6. Public 不读任何凭据。
        assert!(
            probe(AuthScope::Public, Some(HEADER_TOKEN), Some(SESSION)).is_none(),
            "Public 豁免的是凭据闸门，不是 Origin 否决；它不读凭据"
        );
    }
}
