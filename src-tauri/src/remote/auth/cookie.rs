//! 会话 cookie：给浏览器原生请求（导航 / 子资源 / `EventSource`）准备的凭据通道。
//!
//! 凭据通道只有 header 一条时，内嵌 Web UI 在远程整体失效：`EventSource` /
//! `<img>` / 顶层导航在协议层就没有放 header 的地方，而哪些浏览器原生请求
//! 不带凭据是一份需要永久人工维护的清单，破法是白屏而不是报错。
//!
//! cookie 通道补的正是这个缺口。它**不是** header token 的第二个副本：
//!
//! - **取值不同** —— 与 header token 一同签发，但各自独立生成；
//! - **摘要同处** —— 两个摘要存在**同一条** [`super::token::ClientToken`] 记录上，因此撤销
//!   摘掉一条记录就等于两个摘要同时消失，不需要「cookie 撤销表」这种第二份状态；
//! - **比对同路** —— 两者都用同一个 secret、同一个 `Mac::verify_slice` 路径
//!   （见 [`super::token`]），重启后都不需要重新配对。
//!
//! 依赖边界：纯字符串拼装与解析，不新增 crate、不碰 socket。

/// 会话 cookie 的名字。
pub const SESSION_COOKIE_NAME: &str = "cc_switch_session";

/// 远程 listener 当前**是不是**走 TLS。
///
/// 这是传输层的事实，不是口味：非 TLS 的 Tailscale listener 上给 cookie 加
/// `Secure`，浏览器会静默拒收整个 cookie —— 不报错、不提示，用户看到的只是
/// 「远程打开就是白屏」。
///
/// 现状是所有 listener 都是明文 `http://`（见 `remote::mod` 的 `start_addrs`），
/// 因此这里取 `false`。等真的落地 TLS listener 时，这里必须改成按 listener
/// 逐个判定 —— 一个常量的唯一价值就是让「哪里需要改」有且只有一处。
pub const REMOTE_LISTENER_IS_TLS: bool = false;

/// 拼出 `Set-Cookie` 的值。
///
/// 四个属性是各自的硬要求，不是口味：
///
/// | 属性 | 为什么 |
/// |---|---|
/// | `HttpOnly` | 页面里的任何脚本都读不到它 —— 内嵌的是第三方 Web UI |
/// | `SameSite=Strict` | 跨站请求不带它，CSRF 与凭据泄漏一并堵掉 |
/// | `Path=/` | 顶层导航 `GET /` 才是 Web UI 入口，收窄到 `/api` 等于让浏览器发不出那一次导航 |
/// | `Max-Age` | 与凭据库同寿：库在就在，浏览器重启不需要重新配对 |
///
/// `Secure` **只在 TLS listener 上附加**。非 TLS 的 Tailscale listener 上带了它，
/// 浏览器会静默拒收整个 cookie —— 不报错、不提示，表现正是「远程打开就是白屏」。
///
/// `value` 必须是 base64url 无填充那类 cookie 安全取值（见
/// [`super::token::generate_token`]），本函数不替它做编码。
#[allow(dead_code)]
pub fn build_session_cookie(value: &str, max_age_secs: i64, tls: bool) -> String {
    let mut out = format!(
        "{SESSION_COOKIE_NAME}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_secs}"
    );
    if tls {
        out.push_str("; Secure");
    }
    out
}

/// 从请求的 `Cookie` 头里取出会话 cookie 的值。带不上就是 `None`。
///
/// 认得多个 cookie 挤在一行里的形态（`Cookie: a=1; cc_switch_session=..; b=2`），
/// 只认自己的名字，且**不接受**空值。凭据只走这一个来源：URL 参数会让它进
/// 浏览器历史、代理日志与 Referer。
#[allow(dead_code)]
pub fn parse_session_cookie(cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case(SESSION_COOKIE_NAME) {
            return None;
        }
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::super::secret::{load_clients, load_or_create, save_clients};
    use super::super::token::{generate_token, token_hash, verify, ClientToken};
    use super::*;
    use crate::remote::pairing;
    use std::time::Instant;

    /// 属性串齐全：`HttpOnly` / `SameSite=Strict` / `Path=/` / `Max-Age`。
    ///
    /// `Path=/` 不许收窄到 `/api` —— 顶层导航 `GET /` 才是内嵌 Web UI 的入口，
    /// 收窄到 `/api` 等于让浏览器拿着 cookie 却发不出那一次导航，表现仍是白屏。
    #[test]
    fn session_cookie_has_httponly_samesite_and_path() {
        let value = "session-value-under-test";
        let max_age = 42;

        let hdr = build_session_cookie(value, max_age, false);

        assert_eq!(
            hdr,
            format!(
                "cc_switch_session={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}"
            )
        );

        // 拼出来的 Set-Cookie 与 Cookie 请求头是同一个值，往返不能丢。
        assert_eq!(
            parse_session_cookie(&format!("theme=dark; {SESSION_COOKIE_NAME}={value}; x=1")),
            Some(value.to_string())
        );
    }

    /// 非 TLS listener 上不带 `Secure`。
    ///
    /// 非 TLS 的 Tailscale listener 上带了会被浏览器**静默拒收**：既不报错也不
    /// 提示，表现正是「远程打开就是白屏」。TLS listener 上则必须带 —— 少了它等于
    /// 允许明文降级。
    #[test]
    fn session_cookie_omits_secure_on_plain_listener() {
        let value = "session-value-under-test";

        let plain = build_session_cookie(value, 42, false);
        assert!(
            !plain.contains("Secure"),
            "非 TLS listener 不得带 Secure，浏览器会静默拒收整个 cookie：{plain}"
        );
        assert!(plain.contains("Path=/"), "{plain}");

        let tls = build_session_cookie(value, 42, true);
        assert!(tls.contains("Secure"), "TLS listener 必须带 Secure：{tls}");
        assert!(
            tls.starts_with(&plain),
            "Secure 只能追加，不得改写已有属性：{plain} → {tls}"
        );
    }

    /// 撤销后 header token 与 cookie 两个摘要同时失效。
    ///
    /// 钉的是「两个摘要存在同一条记录上」这条结构：撤销摘掉一条记录，两个摘要
    /// 就一起没了 —— 不存在「cookie 还活着」这种只能靠另一张撤销表收尾的状态。
    #[test]
    fn revoke_clears_both_token_and_cookie() {
        let dir = tempfile::tempdir().expect("isolated config dir");
        let secret = load_or_create(dir.path()).expect("secret");

        let store = pairing::PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli", &["read"], now);
        store.approve(&ticket.user_code, false, now).expect("批准");
        // 明文只在交付这一刻出现：批准侧拿不到，只能 poll 换。
        let token = match store.poll(&ticket.poll_token, now) {
            pairing::PollOutcome::Approved { token, .. } => token,
            other => panic!("批准后必须换到凭据，得到 {other:?}"),
        };

        // 一同签发、取值不同的两个秘密，摘要挂在同一条记录上。
        let cookie_value = generate_token();
        assert_ne!(token, cookie_value, "cookie 值不得复用 header token");

        let mut entry = ClientToken::new("cli", &secret, &token);
        entry.cookie_hash = Some(token_hash(&secret, &cookie_value));
        save_clients(dir.path(), &[entry.clone()]).expect("落库");

        // 撤销前：两个值都还认（否则下面的否定断言是空的）。
        let before = load_clients(dir.path()).expect("读库");
        assert!(library_accepts(&before, &secret, &token), "token 必须先认");
        assert!(
            library_accepts(&before, &secret, &cookie_value),
            "cookie 必须先认"
        );
        // 摘要比对经 `ct_eq`：库读回来的 cookie 摘要必须与刚签发的那条一致。
        assert!(
            before[0]
                .cookie_hash
                .zip(entry.cookie_hash)
                .is_some_and(|(a, b)| a.ct_eq(&b)),
            "两个摘要在同一条记录上"
        );

        assert!(
            pairing::revoke_client_by_name(dir.path(), "cli").expect("撤销"),
            "撤销必须摘到东西"
        );

        let after = load_clients(dir.path()).expect("读库");
        assert!(
            !library_accepts(&after, &secret, &token),
            "撤销后 header token 摘要必须失效"
        );
        assert!(
            !library_accepts(&after, &secret, &cookie_value),
            "撤销后 cookie 摘要必须同时失效"
        );
        // 撤销后库里不该再有那条 cookie 摘要。同样走 `ct_eq` —— 写成
        // `c.cookie_hash != entry.cookie_hash` 在类型换掉之后编译不过。
        assert!(
            after.iter().all(|c| c
                .cookie_hash
                .zip(entry.cookie_hash)
                .is_none_or(|(a, b)| !a.ct_eq(&b))),
            "带 cookie 摘要的那条记录必须一起被摘掉"
        );
    }

    /// 库里有没有任何一条记录认这个明文 —— header 摘要与 cookie 摘要都算。
    ///
    /// 走的是 [`super::super::token::verify`]，与中间件同一条
    /// `Mac::verify_slice` 路径。
    fn library_accepts(clients: &[ClientToken], secret: &[u8; 32], presented: &str) -> bool {
        clients.iter().any(|c| {
            verify(secret, presented, c.token_hash.as_bytes())
                || c.cookie_hash
                    .is_some_and(|h| verify(secret, presented, h.as_bytes()))
        })
    }
}
