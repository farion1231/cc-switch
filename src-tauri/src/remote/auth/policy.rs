//! 授权判据纯函数：12 格穷尽判定表。
//!
//! 三段互斥、覆盖完备：
//!
//! 1. **Origin 否决** —— `Origin` 头在场且不在白名单，立刻 403，凭据再对也不救。
//! 2. **通道归类** —— `(peer, proxy_signal)` 二×二映射出四个通道，四格互斥无重叠。
//! 3. **凭据闸门** —— 每个通道挂一个**要求档位**（None / Token / Session），
//!    凭据档位 ≥ 要求即放行。
//!
//! 为什么必须先否决 Origin：Origin 是浏览器给的、攻击者完全可控的字段，
//! 拿它去参与「凭据够不够」的运算，等于让攻击者自己选判据。若把它放进
//! 第四个条件分支（`loopback && origin_ok && ...`），就会出现四条都不命中的
//! 空档 —— 而「本机 + 白名单 Origin + 无代理信号」正是内嵌 Web UI 和 Tauri
//! webview 的真实形态。改成「否决在最前」，就不存在空档。

// ---------------------------------------------------------------------------
// 输入类型
// ---------------------------------------------------------------------------

use super::token::ClientDigest;
use crate::remote::RemoteConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    Loopback,
    NonLoopback,
}

/// 已验过的凭据。带摘要的那两态持 [`ClientDigest`] 而不是裸 `[u8; 32]`：
/// 判定表只看档位、不看摘要内容，所以类型上没有任何理由再开一条能 `==` 的
/// 裸数组路径出去。
///
/// 刻意**不派生 `PartialEq` / `Eq`** —— `ClientDigest` 不提供它们（它只有
/// `ct_eq`），派生在这里会编译失败而不是给出一条短路比较路径。这正是要的：
/// 「凭据相等」这件事在判定表里根本不存在，只有档位高低。
#[allow(dead_code)] // `Cred::Token` 至今没有构造点；变体本身是判定表的输入档位
#[derive(Clone, Copy)]
pub enum Cred {
    None,
    Token(ClientDigest),
    Session(ClientDigest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    ForbiddenOrigin,
    Unauthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

/// 判定表：`(peer, origin, proxy_signal)` 三元组的全枚举。
///
/// 每格三态凭据的期望出口（独立于 `decide` 的实现写成数据，避免「用实现验
/// 实现」的同义反复）：
///
/// | 格(1-based) | peer | origin | proxy | Cred::None | Cred::Token | Cred::Session |
/// |---|---|---|---|---|---|---|
/// | 1 | NonLoopback | 无 | false | Unauthorized | Unauthorized | Allow |
/// | 2 | NonLoopback | 无 | true  | Unauthorized | Unauthorized | Allow |
/// | 3 | NonLoopback | 白名单 | false | Unauthorized | Unauthorized | Allow |
/// | 4 | NonLoopback | 白名单 | true  | Unauthorized | Unauthorized | Allow |
/// | 5 | NonLoopback | 其他 | false | ForbiddenOrigin | ForbiddenOrigin | ForbiddenOrigin |
/// | 6 | NonLoopback | 其他 | true  | ForbiddenOrigin | ForbiddenOrigin | ForbiddenOrigin |
/// | 7 | Loopback | 无 | false | Allow | Allow | Allow |
/// | 8 | Loopback | 无 | true  | Unauthorized | Allow | Allow |
/// | 9 | Loopback | 白名单 | false | Allow | Allow | Allow |
/// | 10 | Loopback | 白名单 | true  | Unauthorized | Allow | Allow |
/// | 11 | Loopback | 其他 | false | ForbiddenOrigin | ForbiddenOrigin | ForbiddenOrigin |
/// | 12 | Loopback | 其他 | true  | ForbiddenOrigin | ForbiddenOrigin | ForbiddenOrigin |
#[cfg(test)]
pub const CASES: [(Peer, Option<&str>, bool); 12] = [
    (Peer::NonLoopback, None, false),
    (Peer::NonLoopback, None, true),
    (Peer::NonLoopback, Some(ORIGIN_OK), false),
    (Peer::NonLoopback, Some(ORIGIN_OK), true),
    (Peer::NonLoopback, Some(ORIGIN_BAD), false),
    (Peer::NonLoopback, Some(ORIGIN_BAD), true),
    (Peer::Loopback, None, false),
    (Peer::Loopback, None, true),
    (Peer::Loopback, Some(ORIGIN_OK), false),
    (Peer::Loopback, Some(ORIGIN_OK), true),
    (Peer::Loopback, Some(ORIGIN_BAD), false),
    (Peer::Loopback, Some(ORIGIN_BAD), true),
];

#[cfg(test)]
pub const ORIGIN_OK: &str = "http://127.0.0.1:4000";
#[cfg(test)]
pub const ORIGIN_BAD: &str = "https://evil-127.0.0.1.example";

/// 12 格 × 凭据三态 = 36 行期望出口，与 `CASES` 一一对应。
#[cfg(test)]
const EXPECTED: [[Decision; 3]; 12] = [
    // 1 NonLoopback / 无 Origin / 无代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
    ],
    // 2 NonLoopback / 无 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
    ],
    // 3 NonLoopback / 白名单 Origin / 无代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
    ],
    // 4 NonLoopback / 白名单 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
    ],
    // 5 NonLoopback / 其他 Origin / 无代理信号
    [
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
    ],
    // 6 NonLoopback / 其他 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
    ],
    // 7 Loopback / 无 Origin / 无代理信号
    [Decision::Allow, Decision::Allow, Decision::Allow],
    // 8 Loopback / 无 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
        Decision::Allow,
    ],
    // 9 Loopback / 白名单 Origin / 无代理信号
    [Decision::Allow, Decision::Allow, Decision::Allow],
    // 10 Loopback / 白名单 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::Unauthorized),
        Decision::Allow,
        Decision::Allow,
    ],
    // 11 Loopback / 其他 Origin / 无代理信号
    [
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
    ],
    // 12 Loopback / 其他 Origin / 有代理信号
    [
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
        Decision::Deny(DenyReason::ForbiddenOrigin),
    ],
];

// ---------------------------------------------------------------------------
// 实现
// ---------------------------------------------------------------------------

/// 端口字段是否形如一个 1..=5 位的十进制端口（`0` 也算）。
///
/// 这里答的是「这个字符串长得像不像端口」，不是「这个端口能不能连」——
/// 放不放行由后面的整条 origin 逐字节相等决定，形态这一层不构成任何放宽。
///
/// 所以 `0` 必须在列：`RemoteConfig::port` 取 `0`（端口交内核分配）是合法的配置
/// 形态，`allowed_origins` 于是会产出 `http://127.0.0.1:0`，而中间件若在形态这
/// 一层就把它否掉，等于**自己发出的名单自己不认** —— 两层对同一份来源给出不同
/// 的答案，正是本模块要根治的那件事。
fn valid_port(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 5
        && p.bytes().all(|b| b.is_ascii_digit())
        && p.parse::<u16>().is_ok()
}

/// authority 是否恰为 `host` 或 `host:port`。
///
/// IPv6 literal 必须带方括号（`Origin` 的 authority 里就是这样），因此冒号
/// 出现在方括号内不算端口分隔符 —— `[::1]:4000` 是一个 host 加一个端口，
/// 而 `[::1]` 本身就是一个 host。漏掉这一层，`::1` 会被 `rsplit_once(':')`
/// 切成 `[:` 与 `1]` 两个残段，于是任何 IPv6 origin 都过不了形态判定。
fn authority_is_host_and_optional_port(authority: &str) -> bool {
    if let Some(rest) = authority.strip_prefix('[') {
        let Some((host, tail)) = rest.split_once(']') else {
            return false;
        };
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.'))
        {
            return false;
        }
        return match tail {
            "" => true,
            // 只接受 `:port`，`]:4000/../x` 之类的尾巴在上面已被 `contains` 挡掉。
            _ => tail.strip_prefix(':').is_some_and(valid_port),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => !host.is_empty() && valid_port(port),
        None => true,
    }
}

/// 把 `Origin` 头规范成 `scheme://host[:port]`，形态不符立刻 `None`。
///
/// 规范化只做**剔除**，不做任何等价变形：不折叠大小写、不补默认端口、不拆
/// IPv6 分段。名单里存的就是签发时那条串，比对必须是逐字节相等 —— 多做一步
/// 变形就多一层「两种写法指同一个来源」的等价关系要维护，而 `Origin` 是
/// 攻击者完全可控的字段。
fn normalize_origin(origin: &str) -> Option<&str> {
    let (scheme, authority) = origin.split_once("://")?;
    // scheme 只收 RFC 3986 那三类字符。`/ ? # @ 空白` 已在下面按 authority 挡掉，
    // 这里再挡一次是为了让「scheme 里塞冒号」也无处下手。
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return None;
    }
    if authority.is_empty() || authority.contains(['/', '?', '#', '@', ' ', '\\']) {
        return None;
    }
    if !authority_is_host_and_optional_port(authority) {
        return None;
    }
    Some(origin)
}

/// Origin 名录：由一份 `(RemoteConfig, tls)` 编译出来的**唯一**来源。
///
/// 它装的不是另一份清单，而是 [`crate::remote::allowed_origins`] 的结果 ——
/// CORS 的 `Access-Control-Allow-Origin` 从那份算，中间件的 Origin 否决从本
/// 类型算，两边读的是同一个函数、同一份输入。两处各写一份清单的故障是静默的：
/// 显式白名单模式下用户 origin 被 CORS 层放行、被中间件 403，用户的配置看起来
/// 「保存成功」却什么也没发生。
///
/// 编译期求值（而不是每请求现算）还有一条实务理由：Tailscale 开启时
/// `allowed_origins` 会去解析一次 tailscale 地址，那是一次进程调用 —— 放在
/// 请求路径上等于让每个请求 spawn 一个进程。
#[derive(Debug, Clone, Default)]
pub struct OriginSet {
    entries: Vec<String>,
}

impl OriginSet {
    /// 按 `(cfg, tls)` 编译一次。
    pub fn from_config(cfg: &RemoteConfig, tls: bool) -> Self {
        Self {
            entries: crate::remote::allowed_origins(cfg, tls)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .map(str::to_string)
                .collect(),
        }
    }

    /// 该 Origin 是否在本名录内。
    ///
    /// 关键在**精确**：形态先过 [`normalize_origin`]，再与名录**逐条相等**。
    /// 任何 `starts_with` / `contains` 都会被 `https://evil-127.0.0.1.example`
    /// 这类串骗过 —— 而那正是前缀与包含两种写法各自最擅长的形态。
    pub fn allows(&self, origin: &str) -> bool {
        let Some(normalized) = normalize_origin(origin) else {
            return false;
        };
        self.entries.iter().any(|entry| entry == normalized)
    }
}

/// 直接从 `(cfg, tls)` 判一个 Origin —— [`OriginSet::allows`] 的一次性外壳。
///
/// 中间件装配期编译一次（见 [`OriginSet::from_config`]），其余调用方（测试、
/// 将来的诊断工具）用这个签名直接答。
#[allow(dead_code)] // 生产路径只走 `OriginSet`，经这个入口取用的目前只有测试
pub fn origin_is_allowed(origin: &str, cfg: &RemoteConfig, tls: bool) -> bool {
    OriginSet::from_config(cfg, tls).allows(origin)
}

/// 请求通道：由 `(peer, proxy_signal)` 二×二唯一确定，四格互斥。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    /// 本机 + 无代理信号：内嵌 Web UI 与 Tauri webview
    LocalEmbedded,
    /// 本机 + 代理信号：本机 HTTP 代理在转发（它替远端客户端说话，不能裸奔）
    LocalProxy,
    /// 远端 + 无代理信号：远程浏览器直连
    RemoteDirect,
    /// 远端 + 代理信号：远端经本机代理
    RemoteViaProxy,
}

impl Channel {
    fn classify(peer: Peer, proxy_signal: bool) -> Self {
        match (peer, proxy_signal) {
            (Peer::Loopback, false) => Channel::LocalEmbedded,
            (Peer::Loopback, true) => Channel::LocalProxy,
            (Peer::NonLoopback, false) => Channel::RemoteDirect,
            (Peer::NonLoopback, true) => Channel::RemoteViaProxy,
        }
    }

    /// 该通道要求的**最低凭据档位**。
    fn required_rank(self) -> u8 {
        match self {
            // 同机内嵌页面，不是安全边界，不要求凭据
            Channel::LocalEmbedded => 0,
            // 本机代理在替别人转发，至少要证明自己是配对过的
            Channel::LocalProxy => 1,
            // 远端两条路径都要求会话凭据：是否经代理不降低要求
            Channel::RemoteDirect | Channel::RemoteViaProxy => 2,
        }
    }
}

impl Cred {
    /// 凭据强度档位：`None < Token < Session`。
    fn rank(self) -> u8 {
        match self {
            Cred::None => 0,
            Cred::Token(_) => 1,
            Cred::Session(_) => 2,
        }
    }
}

/// 全函数判定：三段互斥、覆盖完备，任何四元组都有唯一出口。
///
/// `origins` 是编译好的 Origin 名录，由调用方从 `(RemoteConfig, tls)` 编译而
/// 来 —— 判定表自己不读配置，因此仍可被穷尽枚举。
///
/// `proxy_signal` 由中间件从**三个标准转发头**（`Tailscale-User-Login` /
/// `X-Forwarded-For` / `Forwarded`，见 `middleware::LOCAL_PROXY_SIGNALS`）的
/// **在场**与否求得；本函数只收一个 bool，不解析任何头。这一维在 12 格里**只会
/// 抬高要求**（第 7 格 → 第 8 格），因此可伪造的头在这里伪造不出收益；同时它
/// **绝不参与 `peer` 判定** —— peer 只能来自 `ConnectInfo`。
///
/// ```ignore
/// let d = decide(Peer::Loopback, Some("http://127.0.0.1:4000"), false, Cred::None, &origins);
/// assert_eq!(d, Decision::Allow);
/// ```
pub fn decide(
    peer: Peer,
    origin: Option<&str>,
    proxy_signal: bool,
    cred: Cred,
    origins: &OriginSet,
) -> Decision {
    // 第一段：Origin 否决。在最前，且不可被凭据翻案。
    if let Some(o) = origin {
        if !origins.allows(o) {
            return Decision::Deny(DenyReason::ForbiddenOrigin);
        }
    }
    // 第二段：通道归类（二×二，四格互斥）。
    let channel = Channel::classify(peer, proxy_signal);
    // 第三段：凭据闸门。
    if cred.rank() >= channel.required_rank() {
        Decision::Allow
    } else {
        Decision::Deny(DenyReason::Unauthorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::auth::cookie::REMOTE_LISTENER_IS_TLS;
    use crate::remote::{CorsMode, RemoteConfig};

    // 判定表只看档位，摘要内容在这里是任意的常量 —— 走 `ClientDigest::new`
    // 而不是裸数组，与生产路径同一套类型。
    const T: Cred = Cred::Token(ClientDigest::new([0xA1; 32]));
    const S: Cred = Cred::Session(ClientDigest::new([0xB2; 32]));

    /// 判定表用的默认暴露边界：回环、同源、空白名单。
    fn default_cfg() -> RemoteConfig {
        RemoteConfig::default()
    }

    /// 判定表用的 origin 名录：由 `(cfg, tls)` 编译一次，与中间件装配同一条路。
    fn origins_of(cfg: &RemoteConfig) -> OriginSet {
        OriginSet::from_config(cfg, REMOTE_LISTENER_IS_TLS)
    }

    /// 本用例表里的白名单 Origin 与 [`ORIGIN_OK`] 同源。
    fn default_origins() -> OriginSet {
        origins_of(&default_cfg())
    }

    fn creds() -> [Cred; 3] {
        [Cred::None, T, S]
    }

    /// 判据不变量：peer 是显式二值枚举，不是 bool。
    ///
    /// bool 传错一个参数就静默互换，枚举在调用点就必须写清是哪一侧。
    #[test]
    fn peer_is_two_valued_enum_not_bool() {
        assert_eq!(Peer::Loopback, Peer::Loopback);
        assert_ne!(Peer::Loopback, Peer::NonLoopback);
    }

    /// 任务要求的第 9 格：loopback + 白名单 Origin + 无代理信号 → 放行。
    ///
    /// 这正是内嵌 Web UI / Tauri webview 的形态，也是原四层写法漏掉的空档。
    #[test]
    fn cell_09_loopback_whitelisted_origin_no_proxy_allows() {
        let origins = default_origins();
        let d = decide(Peer::Loopback, Some(ORIGIN_OK), false, Cred::None, &origins);
        assert_eq!(d, Decision::Allow);
        // 同一格三态凭据都放行（该通道不要求凭据）
        assert_eq!(
            decide(Peer::Loopback, Some(ORIGIN_OK), false, T, &origins),
            Decision::Allow
        );
        assert_eq!(
            decide(Peer::Loopback, Some(ORIGIN_OK), false, S, &origins),
            Decision::Allow
        );
    }

    /// `Origin: null`（opaque origin：sandboxed iframe、file://、data:）
    /// 是 403，不是「当作没有 Origin」。它只说明「有页面在发跨源请求」，
    /// 不说明是谁。
    #[test]
    fn origin_null_is_forbidden() {
        let origins = default_origins();
        assert_eq!(
            decide(Peer::Loopback, Some("null"), false, Cred::None, &origins),
            Decision::Deny(DenyReason::ForbiddenOrigin)
        );
        // 即便带了有效会话凭据也不能翻案 —— 否决在最前
        assert_eq!(
            decide(Peer::NonLoopback, Some("null"), true, S, &origins),
            Decision::Deny(DenyReason::ForbiddenOrigin)
        );
        assert!(!origins.allows("null"));
        assert!(!origin_is_allowed(
            "null",
            &default_cfg(),
            REMOTE_LISTENER_IS_TLS
        ));
    }

    /// 12 格穷尽，且每格凭据三态齐全（36 个出口逐个比对）。
    #[test]
    fn case_count_is_twelve() {
        assert_eq!(CASES.len(), 12);

        let origins = default_origins();
        let expected = &EXPECTED;
        for (i, &(peer, origin, proxy)) in CASES.iter().enumerate() {
            for (j, cred) in creds().iter().enumerate() {
                assert_eq!(
                    decide(peer, origin, proxy, *cred, &origins),
                    expected[i][j],
                    "格 {}（1-based {}）凭据态 {} 与期望不符",
                    i,
                    i + 1,
                    j
                );
            }
        }

        // 12 格互不重复：三元组两两不等，覆盖无空格
        for (i, a) in CASES.iter().enumerate() {
            for (j, b) in CASES.iter().enumerate() {
                assert!(i == j || a != b, "格 {} 与格 {} 重复", i, j);
            }
        }
    }

    /// 表序与格号对齐：CASES[8] 就是第 9 格（loopback + 白名单 + 无代理）。
    #[test]
    fn cases_are_ordered_remote_first() {
        assert_eq!(CASES[8], (Peer::Loopback, Some(ORIGIN_OK), false));
    }

    /// 名录判定必须精确到整条 origin，禁止前缀 / 包含匹配。
    ///
    /// 注意这里**不再**有「哪个 host 算白名单」的自定清单：正例直接从
    /// [`crate::remote::allowed_origins`] 编出来的名录里取，自己抄一份等于把
    /// 本模块要去掉的第二份来源又写回来一遍。
    #[test]
    fn whitelist_rejects_prefix_and_contains_matches() {
        let cfg = default_cfg();
        let origins = origins_of(&cfg);

        for bad in [
            // 尾部斜杠：不是我们签发的那条 URL
            "http://127.0.0.1:4000/",
            "http://127.0.0.1:4000//",
            // 大小写差异：比对必须逐字节，不折叠
            "HTTP://127.0.0.1:4000",
            "http://LOCALHOST:4000",
            // 包含「127.0.0.1」的恶意 host
            "https://evil-127.0.0.1.example",
            "http://127.0.0.1.evil.com:4000",
            "http://127.0.0.1:4000.evil.com",
            "http://127.0.0.1@evil.com",
            // 端口区间与 authority 尾巴
            "http://127.0.0.1:99999",
            "http://127.0.0.1#@evil.com",
            // 形态不是 `scheme://host[:port]` 的
            "localhost:4000",
            "file:///etc/passwd",
            "null",
            // 同 host 不同端口、以及 CORS 面根本不认的等价写法：都不放行。
            // 过去这份「localhost / [::1] 都在白名单」的硬编码 host 表正是在
            // 这里悄悄比 CORS 面宽 —— 现在它没了，宽窄只有一处说了算。
            "http://localhost:4000",
            "http://[::1]:4000",
            "http://127.0.0.1:4001",
        ] {
            assert!(
                !origin_is_allowed(bad, &cfg, REMOTE_LISTENER_IS_TLS),
                "误放行 {bad:?}：判定必须是整条 origin 逐字节相等"
            );
            assert!(!origins.allows(bad), "误放行 {bad:?}");
        }

        // 反过来，正例一个都不能误杀。本服务自身 + 两个内嵌 webview origin 在
        // release / debug 两种构建下都在列内。
        for ok in [
            "http://127.0.0.1:4000",
            "https://tauri.localhost",
            "tauri://localhost",
        ] {
            assert!(
                origin_is_allowed(ok, &cfg, REMOTE_LISTENER_IS_TLS),
                "误杀正例 {ok}"
            );
        }
        // round-trip：名录里的每一条都认得回来（`allowed_origins` 过滤掉的毒
        // 条目不会留下半条）。`port = 0`（端口交内核分配）的那份配置也要过这一关 ——
        // 名录自己产出的 origin，判定层必须全认。
        for cfg in [
            default_cfg(),
            RemoteConfig {
                port: 0,
                ..Default::default()
            },
        ] {
            let set = origins_of(&cfg);
            for entry in &set.entries {
                assert!(set.allows(entry), "名录里的 {entry:?} 自己都不认");
            }
        }

        // IPv6 literal：方括号在 Origin 的 authority 里必须带，带了就得认得。
        let v6 = RemoteConfig {
            cors_mode: CorsMode::ExplicitWhitelist,
            origin_whitelist: vec!["http://[::1]:4000".to_string()],
            ..Default::default()
        };
        assert!(
            origin_is_allowed("http://[::1]:4000", &v6, REMOTE_LISTENER_IS_TLS),
            "显式白名单里的 IPv6 literal 必须认得"
        );
        assert!(
            !origin_is_allowed("http://::1:4000", &v6, REMOTE_LISTENER_IS_TLS),
            "去掉方括号不是我们签发的那条 origin"
        );
    }

    /// 通道归类四格互斥：带代理信号一律要求 Token；不带信号时，本机放行、
    /// 远端要求 Session。
    #[test]
    fn channel_requires_session_for_non_loopback_without_proxy_signal() {
        let origins = default_origins();
        assert_eq!(
            decide(Peer::NonLoopback, None, false, Cred::None, &origins),
            Decision::Deny(DenyReason::Unauthorized)
        );
        assert_eq!(
            decide(Peer::NonLoopback, None, false, S, &origins),
            Decision::Allow
        );
        // 本机通道带代理信号时要求 Token（本机代理转发远端请求，不可裸奔）
        assert_eq!(
            decide(Peer::Loopback, None, true, Cred::None, &origins),
            Decision::Deny(DenyReason::Unauthorized)
        );
        assert_eq!(
            decide(Peer::Loopback, None, true, T, &origins),
            Decision::Allow
        );
    }

    /// 原四层写法的漏网空档：本机 + 白名单 Origin + 无代理信号。
    #[test]
    fn the_gap_of_the_old_four_layer_form_is_now_closed() {
        let combos = [
            (Peer::Loopback, Some(ORIGIN_OK), false),
            (Peer::Loopback, Some(ORIGIN_OK), true),
            (Peer::Loopback, None, false),
            (Peer::Loopback, None, true),
            (Peer::NonLoopback, Some(ORIGIN_OK), false),
            (Peer::NonLoopback, Some(ORIGIN_OK), true),
            (Peer::NonLoopback, None, false),
            (Peer::NonLoopback, None, true),
            (Peer::NonLoopback, Some(ORIGIN_BAD), false),
            (Peer::NonLoopback, Some(ORIGIN_BAD), true),
            (Peer::Loopback, Some(ORIGIN_BAD), false),
            (Peer::Loopback, Some(ORIGIN_BAD), true),
        ];
        let origins = default_origins();
        for (peer, origin, proxy) in combos {
            for cred in creds() {
                // 全函数：任何输入都有确定出口，不存在「无出口」状态
                let _ = decide(peer, origin, proxy, cred, &origins);
            }
        }
        // 12 格恰好等于三元组笛卡尔积 2×3×2
        assert_eq!(CASES.len(), 2 * 3 * 2);
    }

    /// 显式白名单里的条目，中间件第②层**也**认。
    ///
    /// 这条锁的是「Origin 的判据只有一份来源」。白名单模式下用户
    /// 把 `https://desk.example.invalid` 填进去，CORS 层按它下发了 ACAO，中间件
    /// 却因为自己那份硬编码 host 表不认识它而 403 —— 症状是「用户的 CORS 配置
    /// 静默失效」，且没有任何一层报错。
    #[test]
    fn explicit_whitelist_entry_reaches_the_origin_gate() {
        let user_origin = "https://desk.example.invalid";
        let cfg = RemoteConfig {
            cors_mode: CorsMode::ExplicitWhitelist,
            origin_whitelist: vec![user_origin.to_string()],
            ..Default::default()
        };

        assert!(
            origin_is_allowed(user_origin, &cfg, REMOTE_LISTENER_IS_TLS),
            "显式白名单里的 {user_origin} 必须被中间件第②层接受：CORS 放行、403 否决，\
             两层各说各话 = 用户配置静默失效"
        );

        // 换回同源模式，同一条 origin 两层一起关掉 —— 不是只关中间件那一层。
        let same_origin = RemoteConfig {
            cors_mode: CorsMode::SameOrigin,
            ..Default::default()
        };
        assert!(
            !origin_is_allowed(user_origin, &same_origin, REMOTE_LISTENER_IS_TLS),
            "同源模式下用户白名单不得生效"
        );
    }

    /// Origin 判定随配置走，不随某份写死的 host 表走。
    ///
    /// 端口一改，CORS 面与中间件必须**同时**换名单。两层各写一份时，这一条
    /// 只会有一边动 —— 硬编码 `127.0.0.1:4000` 的那份会照旧放行。
    #[test]
    fn origin_gate_follows_the_config_not_a_hardcoded_host_list() {
        let moved = RemoteConfig {
            port: 4001,
            ..Default::default()
        };
        assert!(
            !origin_is_allowed("http://127.0.0.1:4000", &moved, REMOTE_LISTENER_IS_TLS),
            "端口改成 4001 之后 http://127.0.0.1:4000 不再是本服务的 origin"
        );
        assert!(
            origin_is_allowed("http://127.0.0.1:4001", &moved, REMOTE_LISTENER_IS_TLS),
            "端口改成 4001 之后本服务自身的 origin 应当跟着换"
        );

        // TLS 也一样：CORS 面与中间件必须读到同一个 tls 事实。
        assert!(
            origin_is_allowed("https://127.0.0.1:4000", &default_cfg(), true),
            "tls=true 时本服务自身的 origin 应当是 https"
        );
        assert!(
            !origin_is_allowed("http://127.0.0.1:4000", &default_cfg(), true),
            "tls=true 时 http 版 origin 不是本服务的 origin"
        );
    }
}
