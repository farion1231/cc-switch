//! Remote 授权判定
//!
//! 这一层只做**纯函数判定**：把「请求的来路形态」压成一个判定表，给出
//! `Allow` 或 `Deny(reason)`。它不碰 socket、不读环境变量、不取时钟，
//! 因此 12 格判定表可以被穷尽枚举测试（见 [`policy`]）。
//!
//! 分层边界：解析 ConnectInfo 拿 peer IP、读 header、映射成
//! [`policy::Peer`] / [`policy::Cred`]，都是调用方（装配层）的活；本层
//! 只回答「给定这四元组，能不能进」。

mod cookie;
mod middleware;
mod policy;
mod secret;
mod token;

// `middleware` 本身仍是私有的，但 `has_proxy_signal` 必须让配对 handler 取到：
// 批准 / 撤销同样要识别「本机代理在替别人转发」，而这三个头的名单只有这一份
// （`LOCAL_PROXY_SIGNALS`）。配对侧自己另列一份，症状是两边悄悄漂移 ——
// 中间件那一侧收紧时，配对侧仍在裸奔。
pub(crate) use middleware::has_proxy_signal;

// 装配层（`remote::mod`）挂在每条路由上的鉴权中间件与 scope 类型。
// `SessionAudience` 一并外露：它由中间件在 Allow 出口注入请求 extension，
// handler（如 `/api/events`）据此选视图 —— 凭据只在闸门这一处被解释一次。
pub use middleware::{auth_layer, AuthScope, SessionAudience, SessionIdentity};

// 判定表的消费方（中间件、`remote::mod`）接上来之后仍走各自的模块路径，只有
// `origin_is_allowed` 经这里被引用；其余名字依旧是「未使用」，豁免保留。
#[allow(unused_imports)]
pub use policy::{decide, origin_is_allowed, Cred, Decision, DenyReason, OriginSet, Peer};

// 浏览器原生请求（导航 / 子资源 / SSE）走的那条凭据通道，由配对交付时
// 用 `Set-Cookie` 下发；`parse_session_cookie` 至今只在测试里被引用，故仍需放行未使用。
#[allow(unused_imports)]
pub use cookie::{
    build_session_cookie, parse_session_cookie, REMOTE_LISTENER_IS_TLS, SESSION_COOKIE_NAME,
};

// 凭据数据模型与落盘路径：`load_or_create` / `load_clients` / `save_clients` 经这里
// 被装配层与配对 handler 取用；清单里剩下的常量与错误类型走各自的模块路径，故仍需放行未使用。
#[allow(unused_imports)]
pub use secret::{
    load_clients, load_or_create, save_clients, SecretError, AUTH_SUBDIR, CLIENTS_FILE,
    SECRET_FILE, SECRET_MODE,
};
// `token_hash` / `ClientDigest` 是摘要侧仅有的两个入口：配对在签发与撤销时都要
// 靠它们把明文换进 / 换出凭据库，而不接触中间件的私有状态。`ct_eq_32` 不再
// re-export —— 它是 `ClientDigest::ct_eq` 的实现体，对外只该看见类型本身。
pub use token::{generate_token, token_hash, ClientDigest, ClientToken};
