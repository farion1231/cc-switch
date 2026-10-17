//! 配对状态机 —— **可穷举的纯函数集合**。
//!
//! 这一层不碰 socket、不读环境变量、**不取时钟**：五个状态转移
//! （`request` / `poll` / `approve` / `reject` / `revoke`）全部把 `now: Instant`
//! 作为显式参数收进来，调用方决定时间从哪来。
//!
//! 为什么不 `tokio::time::pause()`：本 crate 的 `tokio` 没开 `test-util` feature，
//! 冻结时钟根本编不出来；而为了可测把生产 TTL 调小，会让「配对码活 10 分钟」
//! 这条真实约束在测试里消失。参数注入是唯一既不改依赖、又不改生产语义的做法。
//!
//! 状态迁移：
//!
//! ```text
//!                 approve()             poll()
//!   Pending ──────────────────► Approved ────────► Consumed
//!      │                            │                 │
//!      │ poll() 且 now >= expires   │ poll()          │ poll() 再来一次
//!      ▼                            ▼                 ▼
//!   Expired                      Expired           Invalid
//!      │
//!      │ reject()
//!      ▼
//!   Rejected（终态）
//! ```
//!
//! 边界语义的唯一来源是 [`ttl_expired`]：全仓**只有**这一处判断配对码是否过期，
//! 任何地方想重算 TTL 都必须调它 —— `poll` 与 [`PairingStore::request`] 记下的
//! `expires_at` 因此不可能出现 `>=` / `>` 各写一处而对不上的情况。

// 五条路由都挂上了 HTTP，但本模块仍有「已定义、暂无生产调用点」的入口：
// `PairingStore::new`（惰性随机 secret；生产装配一律走 `with_secret`）与
// `request` / `revoke`（生产分别走 `request_from_peer` / `revoke_by_client_id`，
// 两者与它们共用 `Inner::issued` 索引）。它们不是「没用」—— 与
// `remote::auth::token` 同样的处境，按同一惯例整模块放行。
#![allow(dead_code)]

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::remote::auth::{token_hash, ClientDigest};

/// 配对码有效期：满 10 分钟失效。
pub const PAIRING_TTL: Duration = Duration::from_secs(600);

/// 单个来源在窗口内允许的配对请求次数。
pub const RATE_LIMIT_MAX: u32 = 20;

/// 限流窗口长度，与 [`PAIRING_TTL`] 同为 10 分钟。
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(600);

/// 建议客户端的轮询间隔（秒），写进 `/pairing/request` 的响应体。
pub const POLL_INTERVAL_SECS: u64 = 2;

/// 一个配对码在它的**整个生命周期**里，按本模块自己宣传的节奏轮询所需的次数。
///
/// `PAIRING_TTL / POLL_INTERVAL_SECS` —— 两个因子都是本模块自己发给客户端的
/// 指示，所以这不是一个可以被「调大一点」的量：它就是「客户端不违反指示时
/// 必然产生的请求数」。
pub const POLLS_PER_PAIRING_LIFETIME: u64 = PAIRING_TTL.as_secs() / POLL_INTERVAL_SECS;

/// 单个来源在窗口内允许的**轮询**次数。
///
/// 不是拍的数：它由本模块自己宣传的两件事推出来 —— 客户端按
/// [`POLL_INTERVAL_SECS`] 轮询，而一个码的整个生命周期是 [`PAIRING_TTL`]，
/// 所以一次规规矩矩的等待最多产生 [`POLLS_PER_PAIRING_LIFETIME`] 次。
/// 再留一倍余量给同一来源并发等两个码（NAT 后的多台设备）与客户端重试。
///
/// **它和 [`RATE_LIMIT_MAX`] 是两个互不相通的桶。** 这两件事的性质本来就不同：
/// 轮询是客户端遵守服务端指示所必然产生的量，而 [`RATE_LIMIT_MAX`] 管的是猜码、
/// 刷码那种攻击面。两者曾经共用同一个计数器，于是客户端只要按指示轮询 40 秒
/// （20 次 × 2s）就把同一来源**发起新配对**的额度一并烧光 —— 剩下的 9 分多钟里
/// 连 `/pairing/request` 都会 429，而码还能活 560 秒。
///
/// 不变量：`POLL_RATE_LIMIT_MAX > POLLS_PER_PAIRING_LIFETIME`
/// （由 `state::tests::poll_quota_covers_one_full_pairing_lifetime` 钉住）。
pub const POLL_RATE_LIMIT_MAX: u32 = (POLLS_PER_PAIRING_LIFETIME * 2) as u32;

/// 限流记在哪个桶上。
///
/// **两个桶必须互不相通。** 轮询与发起配对的上限各自推导（[`POLL_RATE_LIMIT_MAX`]
/// 与 [`RATE_LIMIT_MAX`]），共用一个计数器就等于让前者吃掉后者的额度；反过来
/// 也一样 —— 发起几次配对不该让已经拿到手的码轮询不动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RateLimitBucket {
    /// 发起一次配对（`/pairing/request`）。
    Request,
    /// 轮询一次配对结果（`/pairing/poll`）。
    Poll,
}

impl RateLimitBucket {
    /// 这个桶在 [`RATE_LIMIT_WINDOW`] 内允许的次数。
    fn max(self) -> u32 {
        match self {
            Self::Request => RATE_LIMIT_MAX,
            Self::Poll => POLL_RATE_LIMIT_MAX,
        }
    }
}

/// 配对请求的生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingStatus {
    /// 已发起，等待桌面端批准。
    Pending,
    /// 已批准，等待 CLI 用 pollToken 换凭据。
    Approved,
    /// pollToken 已用过，凭据已交付。
    Consumed,
    /// 超过 TTL。
    Expired,
    /// 被拒绝（终态）。
    Rejected,
}

impl PairingStatus {
    /// `Pending` 与 `Approved` 之外的状态都是终态。
    fn is_terminal(self) -> bool {
        !matches!(self, PairingStatus::Pending | PairingStatus::Approved)
    }
}

/// `poll` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// 还在等桌面端批准。
    Pending,
    /// 批准通过，本次返回凭据；同一个 pollToken 立刻作废。
    ///
    /// `client_id` 是这条凭据的稳定标识（= 配对请求 id），交付方拿它去
    /// [`PairingStore::revoke_by_client_id`] 撤销 —— 明文 token 在交付这一刻
    /// 现生成，只在这一刻出现；状态机自己一个字段都不留，连能把它重算出来的
    /// 那份派生输入也不留，撤销时交出去的是**摘要**。
    ///
    /// `client_name` 一并带出：凭据库里要落这条客户端的名字，而交付之后
    /// 状态机不再持有它，只能在这一刻交出去。
    Approved {
        token: String,
        client_id: String,
        client_name: String,
    },
    /// 已过期。
    Expired,
    /// 已被拒绝（终态）。
    Rejected,
    /// 触发限流。
    RateLimited,
    /// pollToken 不存在，或已被用过（一次性凭据失效）。
    ///
    /// 刻意不带任何字段：复用尝试者不该从返回值区分「这张 token 曾经有效」
    /// 与「这张 token 从来不存在」。
    Invalid,
}

/// 配对请求 / 审批的失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    /// 配对码不存在、不是待审状态、或已用尽尝试次数。
    ///
    /// 这三件事**必须**折叠成同一个值：分开报就等于给出了一个 oracle，
    /// 让发起方能靠错误差异反推某个配对码是否存在、处于什么状态。
    Denied,
    /// 触发限流。
    RateLimited,
    /// 一道默认拒绝的确认没有交回：来源核不出来（`Unavailable` / `NotInTailnet`），
    /// 或对端是外来账号而调用方没勾那两道里的对应一道。
    ///
    /// 与 [`PairingError::Denied`] **必须**分开：那条说的是「这张配对码不存在 /
    /// 已被处理」，把它俩折叠成同一个值，就会让「用户没勾确认」这件事在界面上
    /// 显示成「请求不见了」—— 用户会去重开一次配对，而不是去把那个勾补上。
    ///
    /// 不参与任何状态转移，因此它**不**消费这条待审请求：确认补上后同一条请求
    /// 仍能批准。它也**不**扩展出口面：`Denied` 仍是「不存在 / 已过期 / 非待审 /
    /// 码错」四件事折叠成的同一个值，码错走不到这里。
    Unconfirmed,
}

/// `/pairing/request` 成功时签发的一次性凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTicket {
    pub request_id: String,
    /// 给人看的 6 位配对码，桌面端靠它把请求和人对应起来。
    pub user_code: String,
    /// CLI 轮询用的**一次性**凭据，只能从 `Authorization: Pairing <token>` 读。
    pub poll_token: String,
    pub client_name: String,
    pub created_at: Instant,
    pub expires_at: Instant,
}

#[derive(Debug, Clone)]
struct Record {
    ticket: RequestTicket,
    /// 发起这条请求的来源 IP。
    ///
    /// 记它是为了让批准那一步能在**后端重算**一次证据：`tailscale whois` 按对端
    /// 地址判定，重算就得知道当初是谁发起的。不带来源的 `request` 记哨兵地址
    /// （`0.0.0.0`），不是「未知」—— 哨兵查不出身份，落到 `Unavailable`，正是
    /// 「查不到」该有的结果。
    peer: IpAddr,
    /// 这条请求的对端**是不是本机账号之外的另一个账号**（`tailscale whois` 的
    /// `Node.User.ID` 与 `tailscale status --json` 的 `Self.UserID` 不等）。
    ///
    /// 记在库里而不是只留在调用方，是为了让「非本人默认拒绝」这条防线属于状态机
    /// 自己：任何一次 `approve` 都要先过这一关，绕不绕得开渲染层都一样。判定由
    /// 装配层在取证后写回（[`PairingStore::note_foreign_account`]），本层不碰 I/O。
    foreign_account: bool,
    status: PairingStatus,
    /// 交付时签发的长期凭据的**摘要**。明文永不落在这里（它只存在于 `poll` 的
    /// 响应里），撤销时对得上就说明这条记录对应的那条凭据真的被交出去过。
    ///
    /// 批准时它是空的：凭据要到交付那一刻才生成，摘要也就那一刻才有。
    issued_digest: Option<ClientDigest>,
}

/// 进程内 secret：算凭据摘要用它。
///
/// `Debug` 只印「已设置 / 未设置」，字节本身绝不流进日志或 panic 消息 —— 它的
/// 作用就是让别人算不出凭据明文，印出来等于把这个作用自己拆掉。
#[derive(Clone, Copy)]
struct StoreSecret([u8; 32]);

impl std::fmt::Debug for StoreSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoreSecret(<redacted>)")
    }
}

impl StoreSecret {
    fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for StoreSecret {
    fn from(bytes: [u8; 32]) -> Self {
        StoreSecret(bytes)
    }
}

#[derive(Debug, Default)]
struct Inner {
    by_request: HashMap<String, Record>,
    request_by_poll: HashMap<String, String>,
    request_by_code: HashMap<String, String>,
    /// 已签发的长期凭据**摘要** → 配对请求 id。`revoke` 靠它反查。
    ///
    /// 用 `Vec` 而不是 `HashMap` 不是偷懒：`ClientDigest` 的字段私有、且刻意
    /// **不**实现 `Eq` / `Hash`，因此它当不了键 —— 一旦为了当键而给摘要开一条
    /// `Hash` + `Eq`，就等于在库外面留了一条逐元素短路的比较路径，正是
    /// [`crate::remote::auth`] 那层要杜绝的东西。改成按 `ct_eq` 线性找，库不大
    /// （客户端几十条），恒时这条判据反而是类型给足的。
    issued: Vec<(ClientDigest, String)>,
    /// 来源 IP → 窗口内的配对请求时刻（升序）。
    hits: HashMap<IpAddr, Vec<Instant>>,
    /// 来源 IP → 窗口内的**轮询**时刻（升序）。
    ///
    /// 与 [`Inner::hits`] 分成两个字段是刻意的，不是「顺手多开一个 map」：轮询
    /// 是客户端遵守 [`POLL_INTERVAL_SECS`] 这条指示所必然产生的量，发起配对是
    /// 猜码/刷码的敏感面，两条上限各自推导（见 [`POLL_RATE_LIMIT_MAX`]）。共用
    /// 一个桶会让前者的正常量直接吃掉后者的额度。
    poll_hits: HashMap<IpAddr, Vec<Instant>>,
    /// 惰性生成的进程内 secret：放进 `OnceLock` 是为了让 [`Inner`] 的 `Default`
    /// 派生与所有构造点都保持原样，不用逐个补参数。
    secret: OnceLock<StoreSecret>,
}

/// 配对状态机。`Clone` 廉价（内部是 `Arc`），因此可以直接作为 axum 的 router
/// 状态在请求之间共享。
#[derive(Debug, Clone, Default)]
pub struct PairingStore {
    inner: Arc<Mutex<Inner>>,
}

/// 配对码字母表：32 个字符，剔掉了肉眼易混的 `0/O/1/I`。
///
/// 长度取 2 的幂（32）不是为了好看：见 [`random_user_code`]，这样 `byte % 32`
/// 是**无偏**的取模，而不是像 33 个字符那样让开头几个字母多出约 3% 概率。
const USER_CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// 生成 6 位配对码。
fn random_user_code() -> String {
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    bytes[..6]
        .iter()
        .map(|b| USER_CODE_ALPHABET[(*b as usize) % 32] as char)
        .collect()
}

/// 生成 32 字节熵。
///
/// 熵走既有 `uuid` v4（每条 16 字节，取两条凑 32 字节），不新增依赖。
fn random_secret_bytes() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes
}

/// 生成 32 字节熵的凭据，base64url 无填充。
fn random_secret() -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;

    URL_SAFE_NO_PAD.encode(random_secret_bytes())
}

impl Inner {
    /// 本 store 的进程内 secret，取不到就当场生成一个。
    ///
    /// 惰性生成（`get_or_init`）是为了让 `Inner` 继续能 `#[derive(Default)]`：
    /// 放进构造参数就得改每一个构造点，而那些构造点与「要不要算摘要」毫无关系。
    ///
    /// 它**只**用来把明文压成摘要，方向是单向的：留着它并不能让谁重算出凭据
    /// —— 凭据本身不是它的函数（见 [`PairingStore::poll`]）。
    fn secret(&self) -> StoreSecret {
        *self
            .secret
            .get_or_init(|| StoreSecret(random_secret_bytes()))
    }
}

impl PairingStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用一把**外部已确定**的 secret 构造（生产装配必须走这一条）。
    ///
    /// 为什么不留在 [`PairingStore::new`] 的惰性随机：凭据库 `clients.json` 里的
    /// 摘要是用 `config_dir` 下那把共享 secret 算的，状态机若自己另生成一把，
    /// 同一条凭据在两边算出的摘要就永远不相等 —— 撤销会变成「受理成功、一条没
    /// 摘」，持有者拿着旧 token 继续畅通，而两边各自的单元用例都绿：状态机内部
    /// 比的是自己那把 secret，自洽；凭据库那条路径压根没被跨模块地测过。
    ///
    /// 换句话说，这不是「多一把 secret」的排版问题，而是**摘要同源性**：
    /// 撤销要跨的那条边界，靠的就是两边算出同一个摘要。
    pub fn with_secret(secret: [u8; 32]) -> Self {
        let store = Self::default();
        let _ = store
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .secret
            .set(StoreSecret::from(secret));
        store
    }

    /// 发起一次配对请求，签发新的 `user_code` 与一次性 `poll_token`。
    ///
    /// 不带来源 IP 的纯状态转移：限流在装配层（`request_from_peer`）做，
    /// 这样状态机本身与「请求从哪来」无关，可以被穷举。来源记哨兵地址。
    ///
    /// `_scopes` 只是把 wire 契约里的 `requestedScopes` 收下来：ticket 不外露它，
    /// 本层也不据此做任何判定。
    pub fn request(&self, client_name: &str, _scopes: &[&str], now: Instant) -> RequestTicket {
        self.request_with_peer(IpAddr::V4(Ipv4Addr::UNSPECIFIED), client_name, _scopes, now)
    }

    /// [`PairingStore::request`] 的带来源版本。
    fn request_with_peer(
        &self,
        peer: IpAddr,
        client_name: &str,
        _scopes: &[&str],
        now: Instant,
    ) -> RequestTicket {
        let ticket = RequestTicket {
            request_id: random_secret(),
            user_code: random_user_code(),
            poll_token: random_secret(),
            client_name: client_name.to_string(),
            created_at: now,
            expires_at: now + PAIRING_TTL,
        };

        let mut inner = self.lock();
        inner
            .request_by_poll
            .insert(ticket.poll_token.clone(), ticket.request_id.clone());
        inner
            .request_by_code
            .insert(ticket.user_code.clone(), ticket.request_id.clone());
        inner.by_request.insert(
            ticket.request_id.clone(),
            Record {
                ticket: ticket.clone(),
                peer,
                foreign_account: false,
                status: PairingStatus::Pending,
                issued_digest: None,
            },
        );
        ticket
    }

    /// 发起配对请求，来源 IP 参与限流。
    pub fn request_from_peer(
        &self,
        peer: IpAddr,
        client_name: &str,
        _scopes: &[&str],
        now: Instant,
    ) -> Result<RequestTicket, PairingError> {
        if !self.allow_hit(peer, now, RateLimitBucket::Request) {
            return Err(PairingError::RateLimited);
        }
        Ok(self.request_with_peer(peer, client_name, _scopes, now))
    }

    /// 轮询配对结果。`poll_token` 一次性：批准后第一次 poll 交付凭据并作废。
    pub fn poll(&self, poll_token: &str, now: Instant) -> PollOutcome {
        let mut inner = self.lock();

        let Some(request_id) = inner.request_by_poll.get(poll_token).cloned() else {
            // 不存在的凭据与用过的凭据都走这里 —— 两者对调用方不可区分，
            // 否则「这个 pollToken 曾经有效过吗」就成了 oracle。
            return PollOutcome::Invalid;
        };
        let Some(record) = inner.by_request.get_mut(&request_id) else {
            return PollOutcome::Invalid;
        };

        match record.status {
            // Pending 的过期判断走全仓唯一的 ttl_expired，不在这里另写一份。
            PairingStatus::Pending if ttl_expired(record.ticket.created_at, now) => {
                record.status = PairingStatus::Expired;
                PollOutcome::Expired
            }
            PairingStatus::Pending => PollOutcome::Pending,
            PairingStatus::Approved => {
                record.status = PairingStatus::Consumed;
                let client_name = record.ticket.client_name.clone();
                // `record` 的借用到此为止（上面就是它的最后一次使用），下面几步
                // 借用的都是整个 `inner`。
                //
                // 凭据是**这一刻现生成的一次性随机明文**，不是从库里某个字段
                // 算出来的。派生式（`token = f(secret, request_id)`）在这里是不
                // 能要的：store 永远留着 secret 与 request_id，那条派生式就等价
                // 于「明文一直存在，只是每次现算」，「随即不再持有原文」在可计算
                // 意义上根本不成立 —— 留存字段足够重算，就没有不持有。
                let secret = inner.secret();
                let token = random_secret();
                // 留下来的只有摘要。方向是单向的：拿到摘要算不出明文。
                let digest = token_hash(secret.as_bytes(), &token);
                // 一次性：从索引里摘掉，同一个 pollToken 再也走不到上面那个
                // `Approved` 分支。
                inner.request_by_poll.remove(poll_token);
                if let Some(record) = inner.by_request.get_mut(&request_id) {
                    record.issued_digest = Some(digest);
                }
                inner.issued.push((digest, request_id.clone()));
                PollOutcome::Approved {
                    token,
                    client_id: request_id,
                    client_name,
                }
            }
            PairingStatus::Rejected => PollOutcome::Rejected,
            // Consumed / Expired：pollToken 已经作废，或记录已到期。
            PairingStatus::Consumed | PairingStatus::Expired => PollOutcome::Invalid,
        }
    }

    /// 轮询配对结果，来源 IP 参与限流。
    pub fn poll_from_peer(&self, poll_token: &str, peer: IpAddr, now: Instant) -> PollOutcome {
        if !self.allow_hit(peer, now, RateLimitBucket::Poll) {
            return PollOutcome::RateLimited;
        }
        self.poll(poll_token, now)
    }

    /// 桌面端批准一条待审请求，签发长期凭据。
    ///
    /// **外来账号默认不放行。** `ack_foreign` 是调用方在界面上勾过的那道「我已知悉
    /// 这不是本机账号」；而「是不是外来账号」是本层自己记在 [`Record`] 里的判定
    /// （见 [`PairingStore::note_foreign_account`]），不是调用方递进来的参数。
    /// 记在库里才挡得住任何一次没带证据、没带确认的 `approve` —— 那正是「默认拒绝」
    /// 与「默认显示成拒绝」的分界线。
    ///
    /// 闸门失败返回 [`PairingError::Unconfirmed`] 且**不推进状态机**：确认补上后
    /// 同一条请求仍能批准。码错 / 已过期 / 非待审折叠成 [`PairingError::Denied`]，
    /// 不因这一层多出可区分的分支（`Unconfirmed` 只对「这张码确实存在、正卡在未确认」
    /// 开放）。
    ///
    /// **明文不在这条路径上出现，摘要也不在。** 批准只是把闸门放行；长期凭据要
    /// 到 CLI 用 pollToken 来取（`poll`）的那一刻才现生成，那时才把它的**摘要**
    /// 记进索引（[`Inner::issued`] 与 `Record::issued_digest`）。批准这一步连
    /// 摘要都还没有 —— 因为明文尚不存在，没有什么可摘要。桌面端要的本来也只是
    /// 「批没批」，不展示、不落库，所以返回值是 `()`。
    pub fn approve(
        &self,
        user_code: &str,
        ack_foreign: bool,
        now: Instant,
    ) -> Result<(), PairingError> {
        let mut inner = self.lock();
        let request_id = lookup_by_code(&inner, user_code).ok_or(PairingError::Denied)?;

        let record = inner
            .by_request
            .get_mut(&request_id)
            .ok_or(PairingError::Denied)?;

        if record.status != PairingStatus::Pending || ttl_expired(record.ticket.created_at, now) {
            // 「不存在」「已过期」「非待审」全折叠成 Denied。
            return Err(PairingError::Denied);
        }

        // 状态与 TTL 都合法之后才轮到这道闸门：让它排在后面，错码与未确认就共用
        // 同一个出口，前者拿不到任何可区分的信号。
        if record.foreign_account && !ack_foreign {
            return Err(PairingError::Unconfirmed);
        }

        record.status = PairingStatus::Approved;
        Ok(())
    }

    /// 记下「这条请求的对端是外来账号」的判定，返回是否记上了。
    ///
    /// 以配对码为键（与 [`PairingStore::approve`] 同一条键），因为取证发生在请求落库
    /// 之后、状态转移之前，两次调用手上都拿着那张码。查不到就返回 `false` 且什么也
    /// 不写 —— 调用方据此知道判定没有落地（生产路径会在批准那一步重算并补记）。
    ///
    /// 允许覆盖：同一条请求在 `request` 与 `approve` 两处各重算一次证据，后者才是
    /// 真正生效的那一次。
    pub fn note_foreign_account(&self, user_code: &str, foreign: bool) -> bool {
        let mut inner = self.lock();
        let Some(request_id) = lookup_by_code(&inner, user_code) else {
            return false;
        };
        let Some(record) = inner.by_request.get_mut(&request_id) else {
            return false;
        };
        record.foreign_account = foreign;
        true
    }

    /// 桌面端拒绝一条待审请求。`Rejected` 是终态，不可逆。
    pub fn reject(&self, user_code: &str, _now: Instant) -> Result<(), PairingError> {
        let mut inner = self.lock();
        let request_id = lookup_by_code(&inner, user_code).ok_or(PairingError::Denied)?;
        let record = inner
            .by_request
            .get_mut(&request_id)
            .ok_or(PairingError::Denied)?;

        if record.status.is_terminal() {
            return Err(PairingError::Denied);
        }
        record.status = PairingStatus::Rejected;
        Ok(())
    }

    /// 撤销一条已交付的长期凭据。返回是否命中过一条尚未被撤销过的凭据。
    ///
    /// 送上来的是明文：先在本 store 的 secret 下算成摘要，再去索引里找。索引
    /// 从头到尾只存摘要，因此「比对」只能是 `ClientDigest::ct_eq` 这一条恒时路径
    /// —— 库外面没有任何裸 `[u8; 32]` 可供短路 `==`。
    pub fn revoke(&self, client_token: &str, _now: Instant) -> bool {
        let mut inner = self.lock();
        let presented = token_hash(inner.secret().as_bytes(), client_token);
        let Some(idx) = inner
            .issued
            .iter()
            .position(|(stored, _)| stored.ct_eq(&presented))
        else {
            return false;
        };
        let (_, request_id) = inner.issued.remove(idx);
        if let Some(record) = inner.by_request.get_mut(&request_id) {
            record.issued_digest = None;
        }
        true
    }

    /// 按配对请求 id 反查它的配对码。查不到就是查不到，不透露原因。
    ///
    /// 桌面端拿到的是审批事件（带 `request_id` + `user_code`），而批准 / 拒绝的
    /// 状态转移以配对码为键。这条索引让调用方能在转移前核对「这条码确实属于这条
    /// 请求」，避免界面拿一条陈旧的码去批另一条待审请求。
    pub fn user_code_for_request(&self, request_id: &str) -> Option<String> {
        self.lock()
            .by_request
            .get(request_id)
            .map(|record| record.ticket.user_code.clone())
    }

    /// 按配对请求 id 反查它的**来源 IP**，供批准那一步重算证据。
    ///
    /// 与 [`PairingStore::user_code_for_request`] 同源：都只认 `request_id`，都
    /// 查不到就说查不到。没有这条索引，命令层就只能收下前端送上来的证据快照 ——
    /// 而那是谁都能伪造的东西。
    pub fn peer_for_request(&self, request_id: &str) -> Option<IpAddr> {
        self.lock()
            .by_request
            .get(request_id)
            .map(|record| record.peer)
    }

    /// 按 `client_id`（= 配对请求 id）撤销一条已交付的长期凭据。
    ///
    /// 返回被撤销凭据的**摘要**，调用方据此把凭据库里对应的记录摘掉（经
    /// `ClientDigest::ct_eq` 逐条比）。
    ///
    /// **为什么不返回明文**：交付那一刻的明文是随机生成的，store 从头到尾没有
    /// 任何字段能把它重算出来（这正是 [`PairingStore::poll`] 要保证的性质）。
    /// 要按 `client_id` 摘掉库里那条记录，靠的本来也是摘要而不是明文 ——
    /// 凭据库里存的正是这个摘要。用明文去倒推，等于为了让撤销成立而重新造出
    /// 一条可复算的派生链。
    ///
    /// 返回 `None` 表示这个 client 没有已签发凭据（不存在 / 已被撤销过）。
    pub fn revoke_by_client_id(&self, client_id: &str, _now: Instant) -> Option<ClientDigest> {
        let mut inner = self.lock();
        // `issued` 是摘要 → request_id；反过来按值找一次。库只在配对期间增长，
        // 客户端数量是几十量级，一次线性扫描比维护第二张索引更不容易写错。
        let idx = inner
            .issued
            .iter()
            .position(|(_, request_id)| request_id.as_str() == client_id)?;
        let (digest, _) = inner.issued.remove(idx);
        if let Some(record) = inner.by_request.get_mut(client_id) {
            record.issued_digest = None;
        }
        Some(digest)
    }

    /// 滑动窗口限流：`bucket` 对应的那份 `hits` 只保留**仍在窗口内**的时刻，
    /// 返回本次是否放行。
    ///
    /// 半开区间 `[now - WINDOW, now)`：`now - t < WINDOW` 才算数，因此
    /// 整整 600s 前的请求恰好在 600s 这一刻滚出窗口。
    ///
    /// `bucket` 决定两件事：记到哪个桶、以及这个桶的上限是多少。**桶是分开的
    /// 字段，不是同一份 `hits` 上的两个计数** —— 后者仍然会让轮询的正常量吃掉
    /// 发起配对的额度。
    fn allow_hit(&self, peer: IpAddr, now: Instant, bucket: RateLimitBucket) -> bool {
        let mut inner = self.lock();
        let hits = match bucket {
            RateLimitBucket::Request => inner.hits.entry(peer).or_default(),
            RateLimitBucket::Poll => inner.poll_hits.entry(peer).or_default(),
        };
        hits.retain(|t| {
            now.checked_duration_since(*t)
                .map(|elapsed| elapsed < RATE_LIMIT_WINDOW)
                // `t` 比本次调用的 `now` 还新，只可能是并发造成：`now` 由调用
                // 方在进 store **之前**采集，锁是在采集之后才拿到的。请求 A
                // 采到 T1 后被抢占，B 采到 T2>T1 并先拿到锁记下命中，随后 A
                // 才进来 —— 此时 `checked_duration_since` 返回 `None`。
                //
                // 这条命中必然落在窗口内（它比现在还「新」），删掉它等于凭空
                // 抹掉一次已经计入的请求，让并发下的上限漂到 `RATE_LIMIT_MAX`
                // 之上：实测 50 条并发配对请求放行了 21~33 条。顺序发送看不出
                // 这个问题，所以只锁「窗口滚动」的旧用例抓不住它。
                .unwrap_or(true)
        });
        if hits.len() as u32 >= bucket.max() {
            return false;
        }
        hits.push(now);
        true
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // 中毒意味着某个 handler 在持锁时 panic 了，此时读到的状态同样不可信。
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 配对码 → 配对请求 id。查不到就是查不到，不透露原因。
fn lookup_by_code(inner: &Inner, user_code: &str) -> Option<String> {
    inner.request_by_code.get(user_code).cloned()
}

/// TTL 边界的**唯一**判据。
///
/// 语义：满 [`PAIRING_TTL`] 即失效（`>=`）。599s 仍 pending，600s 过期。
/// 任何地方想判断过期都必须调它。
pub fn ttl_expired(created_at: Instant, now: Instant) -> bool {
    now.checked_duration_since(created_at)
        .map(|elapsed| elapsed >= PAIRING_TTL)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全套测试的公共起点：一个任意但固定的时刻。
    fn t0() -> Instant {
        Instant::now()
    }

    fn peer_a() -> IpAddr {
        "10.0.0.1".parse().unwrap()
    }

    fn peer_b() -> IpAddr {
        "10.0.0.2".parse().unwrap()
    }

    /// 批准 → poll，把交付到的那串凭据交回给用例。
    fn deliver(store: &PairingStore, user_code: &str, poll_token: &str, now: Instant) -> String {
        store.approve(user_code, false, now).expect("批准必须成功");
        match store.poll(poll_token, now) {
            PollOutcome::Approved { token, .. } => token,
            other => panic!("批准后必须换到凭据，得到 {other:?}"),
        }
    }

    /// 一个 **secret 与 request_id 都写死**的 store。
    ///
    /// 存在的唯一理由是把「交付出去的凭据是不是 `(secret, request_id)` 的确定性
    /// 函数」这条判据变成可判定的：两个这样的 store 手上留着**完全相同**的
    /// 输入（同一个 store secret、同一个 request_id、同一条记录的全部字段），
    /// 因此若明文是这两个输入算出来的，它们必然交出同一串 —— 那就证明服务端
    /// 随时能重算凭据，「随即不再持有原文」是假的。
    fn store_with_fixed_identity(t0: Instant) -> (PairingStore, String, String) {
        const FIXED_REQUEST_ID: &str = "fixed-request-id";

        let store = PairingStore::new();
        let ticket = store.request("cli", &["read"], t0);
        // secret 是 `OnceLock`，只认第一次写入 —— 写死它，两个 store 才是同一个。
        store
            .lock()
            .secret
            .get_or_init(|| StoreSecret([0x5au8; 32]));

        // 把这条请求整体改名成固定的 request_id。`by_request` 与两张索引必须
        // 一起改，否则状态机会在任一处查不到这条记录。
        let (poll_token, user_code) = {
            let mut inner = store.lock();
            let mut record = inner
                .by_request
                .remove(&ticket.request_id)
                .expect("刚建好的请求必须在库里");
            record.ticket.request_id = FIXED_REQUEST_ID.to_string();
            let (poll_token, user_code) = (
                record.ticket.poll_token.clone(),
                record.ticket.user_code.clone(),
            );
            inner
                .by_request
                .insert(FIXED_REQUEST_ID.to_string(), record);
            inner
                .request_by_poll
                .insert(poll_token.clone(), FIXED_REQUEST_ID.to_string());
            inner
                .request_by_code
                .insert(user_code.clone(), FIXED_REQUEST_ID.to_string());
            (poll_token, user_code)
        };

        (store, poll_token, user_code)
    }

    /// 交付出去的凭据**不是** store 留存输入的函数：两个 secret 与 request_id
    /// 完全相同的 store 必须交出**不同**的两串。
    ///
    /// 这条是「poll 成功即不再持有原文」在**可计算意义**上的判据。
    /// 派生式（`token = f(secret, request_id)`）下它必然失败：f 是纯函数，
    /// 同样的两个入参只能给出同一个输出，而服务端在交付之后仍然留着这两个入参 ——
    /// 也就是随时能把凭据重算出来。随机一次性明文下它成立：明文只在 poll 那
    /// 一瞬间生成，store 从头到尾只落它的摘要。
    #[test]
    fn delivered_credential_is_not_a_function_of_the_retained_inputs() {
        let t0 = t0();

        let (a, a_poll, a_code) = store_with_fixed_identity(t0);
        let (b, b_poll, b_code) = store_with_fixed_identity(t0);
        // 反向钉住「两个 store 的输入真的相同」：不等的话这条判据就退化成
        // 「两条随机值不一样」，证明不了任何东西。
        let secret_a = a.lock().secret.get().expect("secret 已写死").0;
        let secret_b = b.lock().secret.get().expect("secret 已写死").0;
        assert_eq!(
            secret_a, secret_b,
            "两个 store 的 secret 必须被写成同一个，否则这条判据是空的"
        );
        assert_eq!(
            a.lock().by_request.keys().collect::<Vec<_>>(),
            b.lock().by_request.keys().collect::<Vec<_>>(),
            "两个 store 留存的 request_id 必须相同"
        );

        let token_a = deliver(&a, &a_code, &a_poll, t0);
        let token_b = deliver(&b, &b_code, &b_poll, t0);
        assert_ne!(
            token_a, token_b,
            "凭据不得是 (secret, request_id) 的确定性函数 —— 否则服务端只要还留着 \
             这两个入参就能重算明文"
        );

        // 反向钉住「交出去的仍是真的凭据」：随机不等于无效。
        assert!(a.revoke(&token_a, t0), "交付出去的必须仍是可撤销的真凭据");
        assert!(b.revoke(&token_b, t0), "交付出去的必须仍是可撤销的真凭据");
    }

    /// 交付（Consumed）之后，store 留存的东西**推不出**那份明文。
    ///
    /// 判据取「服务端此刻还能交出什么」的最宽口径：交付之后唯一还能交出的凭据
    /// 材料是那一份**摘要**，它只能被拿去和库里已有的摘要比对（`ct_eq`），
    /// 方向是单向的 —— 拿到摘要算不出明文。摘掉之后连摘要也不留。
    #[test]
    fn after_consumed_the_retained_state_cannot_yield_the_plaintext() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);
        let token = deliver(&store, &r.user_code, &r.poll_token, t0);
        assert!(!token.is_empty(), "交付必须真产出凭据，否则本用例是空的");

        // store 之后唯一还能交出的凭据材料：摘要，且对得上交付的那一份。
        let secret = Inner::secret(&store.lock());
        let digest = store
            .revoke_by_client_id(&r.request_id, t0)
            .expect("已交付的凭据必须还能按 client_id 撤销");
        assert!(
            digest.ct_eq(&token_hash(secret.as_bytes(), &token)),
            "撤销交出的必须正是交付那条凭据的摘要"
        );
        // 摘要自己也不带明文：`Debug` 被钉死成 `<redacted>`，没有任何出口能把
        // 32 字节之外的东西交出去。
        assert_eq!(format!("{digest:?}"), "ClientDigest(<redacted>)");

        // 摘掉之后连摘要都不留存。
        assert!(
            store.revoke_by_client_id(&r.request_id, t0).is_none(),
            "同一条凭据只能撤销一次"
        );

        // 全量快照：留存的每一个字段都不含明文。
        let snapshot = format!("{:?}", *store.lock());
        assert!(
            !snapshot.contains(&token),
            "Consumed 之后 store 留存字段里仍出现凭据明文：{snapshot}"
        );
    }

    // --- TTL 边界 ---

    /// 599s 仍 pending，600s 恰好过期 —— TTL 边界钉死在「满 600 秒失效」。
    #[test]
    fn ttl_600s_is_expired() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        assert_eq!(
            store.poll(&r.poll_token, t0 + Duration::from_secs(599)),
            PollOutcome::Pending,
            "599s 仍在窗口内，必须还是 Pending"
        );
        assert_eq!(
            store.poll(&r.poll_token, t0 + Duration::from_secs(600)),
            PollOutcome::Expired,
            "满 600s 必须 Expired（边界取 >=）"
        );
    }

    /// `>=` 语义与 `expires_at` 自洽：`expires_at` 本身算过期，前一纳秒不算。
    #[test]
    fn ttl_boundary_matches_recorded_expires_at() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        assert_eq!(r.expires_at, t0 + PAIRING_TTL);
        assert!(!ttl_expired(
            r.created_at,
            r.expires_at - Duration::from_nanos(1)
        ));
        assert!(ttl_expired(r.created_at, r.expires_at));
    }

    /// TTL 与限流窗口同为 600s，两处边界必须同时翻面：
    /// 599s 时记录仍 pending 且窗口内的第 21 次仍被限；600s 时记录过期且窗口滚动。
    #[test]
    fn ttl_and_rate_limit_window_flip_at_the_same_instant() {
        assert_eq!(PAIRING_TTL, RATE_LIMIT_WINDOW);
        assert_eq!(PAIRING_TTL, Duration::from_secs(600));

        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        // 同一来源在 t0 已被计满 20 次
        for _ in 0..RATE_LIMIT_MAX {
            store
                .request_from_peer(peer_a(), "cli", &["read"], t0)
                .expect("前 20 次必须放行");
        }

        // 599s：窗口还没滚 → 仍限流；配对记录也还没过期
        assert_eq!(
            store.request_from_peer(peer_a(), "cli", &["read"], t0 + Duration::from_secs(599)),
            Err(PairingError::RateLimited),
            "599s 时第 21 次必须仍被限（窗口边界同样取半开区间）"
        );
        assert_eq!(
            store.poll(&r.poll_token, t0 + Duration::from_secs(599)),
            PollOutcome::Pending
        );

        // 600s：两个窗口同时翻面
        assert!(
            store
                .request_from_peer(peer_a(), "cli", &["read"], t0 + Duration::from_secs(600))
                .is_ok(),
            "600s 时窗口已滚动，第 21 次必须放行"
        );
        assert_eq!(
            store.poll(&r.poll_token, t0 + Duration::from_secs(600)),
            PollOutcome::Expired
        );
    }

    // --- 限流 ---

    /// 第 20 次放行、第 21 次 RateLimited；两个来源的窗口互不影响。
    #[test]
    fn rate_limit_21st_request_is_limited() {
        let store = PairingStore::new();
        let now = t0();

        for i in 0..RATE_LIMIT_MAX {
            assert!(
                store
                    .request_from_peer(peer_a(), "cli", &["read"], now)
                    .is_ok(),
                "第 {i} 次必须放行"
            );
        }
        assert_eq!(
            store.request_from_peer(peer_a(), "cli", &["read"], now),
            Err(PairingError::RateLimited),
            "第 21 次必须被限流"
        );

        // 另一个来源有自己独立的窗口
        assert!(
            store
                .request_from_peer(peer_b(), "cli", &["read"], now)
                .is_ok(),
            "限流按来源 IP 计，另一个 peer 不该被牵连"
        );
    }

    /// 窗口滚动后恢复：t0+600s 起重新放行。
    #[test]
    fn rate_limit_recovers_after_window_rolls() {
        let store = PairingStore::new();
        let t0 = t0();

        for _ in 0..RATE_LIMIT_MAX {
            store
                .request_from_peer(peer_a(), "cli", &["read"], t0)
                .unwrap();
        }
        assert_eq!(
            store.request_from_peer(peer_a(), "cli", &["read"], t0),
            Err(PairingError::RateLimited)
        );
        assert!(
            store
                .request_from_peer(peer_a(), "cli", &["read"], t0 + RATE_LIMIT_WINDOW)
                .is_ok(),
            "窗口滚动后必须恢复"
        );
    }

    /// **本轮修的就是这条不变量。** 轮询额度必须装得下「一个码按服务端自己宣传
    /// 的节奏，被老老实实轮询整整一个生命周期」。
    ///
    /// 它曾经不成立，而且是被一条测试（旧名 `poll_is_rate_limited_per_peer`）
    /// **保护**着的：服务端告诉客户端每 [`POLL_INTERVAL_SECS`] 秒来一次，却只给
    /// 600 秒内 20 次。需求量 [`POLLS_PER_PAIRING_LIFETIME`](300) 是额度(20) 的
    /// 15 倍 —— 点完按钮约 40 秒客户端就被限流，而码还能活 560 秒，浏览器里
    /// 表现为永远停在「Waiting for approval...」。
    ///
    /// 破的是**同一条路由自己宣传的两件事之间的自洽性**，所以断言写成常量之间的
    /// 关系再加一次真跑，而不是钉某个具体数字 —— 日后调 TTL 或调间隔，这里跟着
    /// 走；真把额度写回「不够用」，这里先红。
    #[test]
    fn poll_quota_covers_one_full_pairing_lifetime() {
        assert!(
            POLL_RATE_LIMIT_MAX as u64 > POLLS_PER_PAIRING_LIFETIME,
            "轮询额度({POLL_RATE_LIMIT_MAX})必须严格大于一个完整生命周期按宣传间隔\
             所需的次数({POLLS_PER_PAIRING_LIFETIME})：额度少于需求量，\
             客户端不违反指示也等不到批准"
        );

        // 常量关系对了还不够，真按服务端给节奏把整个生命周期走一遍，一次都不许被限。
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        for i in 0..POLLS_PER_PAIRING_LIFETIME {
            let now = t0 + Duration::from_secs(i * POLL_INTERVAL_SECS);
            assert_eq!(
                store.poll_from_peer(&r.poll_token, peer_a(), now),
                PollOutcome::Pending,
                "第 {i} 次轮询（t+{}s）就被限流了 —— 客户端是完全照服务端给的\
                 节奏在轮询，这一刻的请求量是服务端自己要求的",
                i * POLL_INTERVAL_SECS
            );
        }
    }

    /// 两个桶互不相通 —— 这是同一个 bug 的另一半。
    ///
    /// 共用一份 `hits` 时，客户端只要按指示轮询 40 秒（20 次 × 2s）就把同一来源
    /// **发起新配对**的额度一并烧光，之后 10 分钟里连 `/pairing/request` 都是 429。
    /// 两个方向都要钉住，因为「分开」是双向的性质。
    #[test]
    fn poll_and_request_quotas_are_separate_buckets() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        // 方向一：把 poll 的额度灌满，request 仍然可用。
        for _ in 0..POLL_RATE_LIMIT_MAX {
            assert_eq!(
                store.poll_from_peer(&r.poll_token, peer_a(), t0),
                PollOutcome::Pending
            );
        }
        assert_eq!(
            store.poll_from_peer(&r.poll_token, peer_a(), t0),
            PollOutcome::RateLimited,
            "poll 自己的桶仍然是满的"
        );
        assert!(
            store
                .request_from_peer(peer_a(), "cli", &["read"], t0)
                .is_ok(),
            "轮询把 poll 桶灌满之后，同一来源**发起新配对**仍必须放行：\
             轮询的正常量不许吃掉 request 的额度"
        );

        // 方向二：把 request 的额度灌满，poll 仍然可用。
        for _ in 0..RATE_LIMIT_MAX {
            store
                .request_from_peer(peer_b(), "cli", &["read"], t0)
                .expect("request 桶的前 20 次必须放行");
        }
        assert_eq!(
            store.request_from_peer(peer_b(), "cli", &["read"], t0),
            Err(PairingError::RateLimited),
            "request 自己的桶仍然是满的"
        );
        assert_eq!(
            store.poll_from_peer(&r.poll_token, peer_b(), t0),
            PollOutcome::Pending,
            "发起配对把 request 桶灌满之后，同一来源的 poll 仍必须放行：\
             反方向也不许串桶"
        );
    }

    /// 拆成两个桶**不是**把轮询限流删掉：poll 仍然按来源有上限，只是上限终于
    /// 对得上自己宣传的节奏。
    #[test]
    fn poll_quota_is_still_bounded_per_peer() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        for i in 0..POLL_RATE_LIMIT_MAX {
            assert_eq!(
                store.poll_from_peer(&r.poll_token, peer_a(), t0),
                PollOutcome::Pending,
                "第 {i} 次轮询必须放行"
            );
        }
        assert_eq!(
            store.poll_from_peer(&r.poll_token, peer_a(), t0),
            PollOutcome::RateLimited,
            "第 {} 次轮询必须被限流 —— 轮询额度仍然是有上限的",
            POLL_RATE_LIMIT_MAX + 1
        );
        assert_eq!(
            store.poll_from_peer(&r.poll_token, peer_b(), t0),
            PollOutcome::Pending,
            "轮询的限流同样按来源 IP 计，另一个 peer 不该被牵连"
        );

        // 窗口滚动后恢复。此刻配对码也同时到期（TTL == 窗口），所以这里只断言
        // 「不再被限流」：能走到 TTL 判定就说明 allow_hit 已经放行了。
        assert_ne!(
            store.poll_from_peer(&r.poll_token, peer_a(), t0 + RATE_LIMIT_WINDOW),
            PollOutcome::RateLimited,
            "窗口滚动后轮询必须恢复（半开区间语义与 request 桶同一条）"
        );
    }

    // --- 一次性消费 ---

    /// 同一个 pollToken 第二次 poll 失效（`Invalid`），不再吐出凭据。
    #[test]
    fn poll_is_consumed_once() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);
        store
            .approve(&r.user_code, false, t0)
            .expect("批准必须成功");

        let first = store.poll(&r.poll_token, t0);
        let token = match &first {
            PollOutcome::Approved {
                token,
                client_id,
                client_name,
            } => {
                assert_eq!(client_id, &r.request_id, "client_id 必须就是请求 id");
                assert_eq!(client_name, "cli", "凭据库要落客户端的名字");
                token.clone()
            }
            other => panic!("批准后首次 poll 必须换到凭据，得到 {other:?}"),
        };

        let second = store.poll(&r.poll_token, t0);
        assert_eq!(
            second,
            PollOutcome::Invalid,
            "同一 pollToken 第二次 poll 必须失效"
        );
        assert!(
            !format!("{second:?}").contains(&token),
            "失效响应里不得残留凭据明文"
        );
    }

    /// 凭据明文只在 `poll` 交付那一次出现，两条撤销路径各自认得交付出去的那一份。
    ///
    /// 按 `client_id` 撤销交出的是**摘要**（明文没有第二次可交：它是随机生成的，
    /// store 留不住也重算不出）；按明文撤销则是拿送上来的明文算摘要去比对。
    /// 两条路径各用一个 store：撤销是一次性的，同一条凭据先被一条路径摘掉，
    /// 另一条就本该查不到了。
    #[test]
    fn both_revoke_paths_recognise_the_delivered_credential() {
        let t0 = t0();

        let deliver = |store: &PairingStore| -> (RequestTicket, String) {
            let r = store.request("cli", &["read"], t0);
            let token = deliver(store, &r.user_code, &r.poll_token, t0);
            (r, token)
        };

        // 按 client_id 撤销：交出的摘要必须正是交付那条凭据的摘要，否则调用方
        // 会摘错凭据库里的记录。
        let by_id_store = PairingStore::new();
        let (r, delivered) = deliver(&by_id_store);
        let secret = Inner::secret(&by_id_store.lock());
        let revoked = by_id_store
            .revoke_by_client_id(&r.request_id, t0)
            .expect("按 client_id 撤销必须拿得到凭据摘要");
        assert!(
            revoked.ct_eq(&token_hash(secret.as_bytes(), &delivered)),
            "撤销交出的摘要必须与交付那条凭据逐字节对应，否则摘错库"
        );

        // 按明文撤销：必须认交付出去的那一份。篡改过的那一份排在真撤销**之前**
        // 试 —— 放在后面的话索引已被摘空，无论对不对都返回 false，那是空断言。
        let by_token_store = PairingStore::new();
        let (_, delivered) = deliver(&by_token_store);
        assert!(
            !by_token_store.revoke(&format!("{delivered}x"), t0),
            "篡改过的凭据不得撤销成功"
        );
        assert!(
            by_token_store.revoke(&delivered, t0),
            "交付出去的那份必须可撤销"
        );
    }

    /// 没批准过的 pollToken 一律 `Invalid`。
    #[test]
    fn poll_with_unknown_token_is_invalid() {
        let store = PairingStore::new();
        assert_eq!(store.poll("no-such-token", t0()), PollOutcome::Invalid);
    }

    // --- 批准 / 拒绝 / 撤销 ---

    /// 配对码错 N 次：每一次都是同一个不可区分的 `Denied`，不给 oracle。
    #[test]
    fn approve_rejects_wrong_code_without_distinguishing_detail() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        let mut codes = Vec::new();
        for i in 0..10 {
            let wrong = format!("WRONG{i}");
            assert_eq!(
                store.approve(&wrong, false, t0),
                Err(PairingError::Denied),
                "第 {i} 次错码必须被拒"
            );
            codes.push(store.approve(&wrong, false, t0).unwrap_err());
        }
        assert!(
            codes.windows(2).all(|w| w[0] == w[1]),
            "所有失败必须折叠成同一个错误，不能让调用方靠差异反推配对码是否存在"
        );

        // 真实配对码仍然可用 —— 错码尝试没有把它废掉。
        assert!(store.approve(&r.user_code, false, t0).is_ok());
    }

    /// `Rejected` 是终态：拒绝后 approve / poll 都拿不到凭据。
    #[test]
    fn rejected_is_terminal() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        assert_eq!(store.reject(&r.user_code, t0), Ok(()));
        assert_eq!(
            store.approve(&r.user_code, false, t0),
            Err(PairingError::Denied),
            "已拒绝的配对码不得再被批准"
        );
        assert_eq!(store.poll(&r.poll_token, t0), PollOutcome::Rejected);
        assert_eq!(
            store.poll(&r.poll_token, t0),
            PollOutcome::Rejected,
            "终态下重复 poll 的结果必须稳定"
        );
    }

    /// 撤销后，交付出去的长期凭据在 store 里失效。
    ///
    /// 凭据只能从 `poll` 取回（`approve` 不再产出明文），所以这条用例的顺序是
    /// 批准 → 交付 → 拿交付到的那份去撤销。
    #[test]
    fn revoke_invalidates_issued_credential() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);
        store
            .approve(&r.user_code, false, t0)
            .expect("批准必须成功");
        let token = match store.poll(&r.poll_token, t0) {
            PollOutcome::Approved { token, .. } => token,
            other => panic!("批准后必须换到凭据，得到 {other:?}"),
        };

        assert!(store.revoke(&token, t0), "撤销已签发的凭据必须返回 true");
        assert!(!store.revoke(&token, t0), "同一凭据重复撤销必须返回 false");
    }

    /// 批准之后、交付之前，库里**一条凭据材料都还没有**。
    ///
    /// 批准只是放行：长期凭据要到 CLI 用 pollToken 来取的那一刻才随机生成，
    /// 摘要也就那一刻才有。这一刻库里连摘要都不该有 —— 判据落在两个出口上：
    /// 调试输出（`Inner` 整体 `Debug` 是「服务端此刻持有什么」的最宽口径）与
    /// 按 `client_id` 撤销（拿不到就说明库里确实没有已签发凭据）。
    #[test]
    fn nothing_is_issued_between_approval_and_delivery() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);
        store
            .approve(&r.user_code, false, t0)
            .expect("批准必须成功");

        let snapshot = format!("{:?}", *store.lock());
        assert!(
            !snapshot.contains("ClientDigest(<redacted>)"),
            "交付之前库里还不该有任何摘要：{snapshot}"
        );
        assert!(
            store.lock().issued.is_empty(),
            "交付之前 `issued` 索引必须是空的"
        );
        assert!(
            store.revoke_by_client_id(&r.request_id, t0).is_none(),
            "还没交付的请求不得能被按 client_id 撤销"
        );
    }

    /// 交付 → 撤销 → 重复撤销：中途每一次快照都不得带明文，且只撤销得掉一次。
    #[test]
    fn revoke_after_delivery_hits_once_and_no_snapshot_carries_plaintext() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);
        store
            .approve(&r.user_code, false, t0)
            .expect("批准必须成功");
        let token = match store.poll(&r.poll_token, t0) {
            PollOutcome::Approved { token, .. } => token,
            other => panic!("批准后必须换到凭据，得到 {other:?}"),
        };

        let mut snapshots = vec![format!("{:?}", *store.lock())];
        assert!(store.revoke(&token, t0), "撤销已签发的凭据必须返回 true");
        snapshots.push(format!("{:?}", *store.lock()));
        assert!(!store.revoke(&token, t0), "同一凭据重复撤销必须返回 false");
        snapshots.push(format!("{:?}", *store.lock()));

        for (i, snapshot) in snapshots.iter().enumerate() {
            assert!(
                !snapshot.contains(&token),
                "第 {i} 次快照里出现了凭据明文：{snapshot}"
            );
        }
    }

    /// 记下请求来源 IP：批准那一步要在**后端重算**一次证据（`tailscale whois`
    /// 按对端地址判定），而重算必须知道当初是谁发起的。
    #[test]
    fn peer_for_request_returns_the_source_recorded_at_request_time() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store
            .request_from_peer(peer_a(), "cli", &["read"], t0)
            .expect("请求必须放行");

        assert_eq!(
            store.peer_for_request(&r.request_id),
            Some(peer_a()),
            "批准时必须能取回当初的来源 IP，否则后端无从重算证据"
        );

        // 不带来源的 `request` 记哨兵地址：查不到来源的请求不得凭空变成可核实。
        let bare = store.request("cli", &[], t0);
        assert_eq!(
            store.peer_for_request(&bare.request_id),
            Some(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
        );

        assert_eq!(store.peer_for_request("no-such-request"), None);
    }

    /// 每个请求的配对码与 pollToken 都是新的，互不串味。
    #[test]
    fn request_ticket_carries_fresh_credentials() {
        let store = PairingStore::new();
        let t0 = t0();
        let a = store.request("cli", &["read"], t0);
        let b = store.request("cli", &["read"], t0);

        assert_ne!(a.user_code, b.user_code);
        assert_ne!(a.poll_token, b.poll_token);
        assert_eq!(a.user_code.len(), 6, "配对码固定 6 位");
        assert!(
            a.user_code
                .chars()
                .all(|c| "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(c)),
            "配对码不得含易混字符（0/O/1/I）"
        );
    }

    // --- 外来账号：后端自己记账的那道防线 ---

    /// 对端属于**别的账号**时，默认不批；必须由后端收到一次确认才放行。
    ///
    /// 这道防线不能只活在渲染层：界面上的那个勾谁都能绕过，而
    /// `tailscale whois` 判定出的 `Node.User.ID != Self.UserID` 是「这条授权要交给
    /// 一个非本人账号」这一事实本身。所以判定结果记进 [`Record`]，由
    /// `approve` 自己拦。
    #[test]
    fn foreign_account_is_refused_unless_the_backend_is_told_otherwise() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli-under-test", &["read"], t0);

        assert!(
            store.note_foreign_account(&r.user_code, true),
            "外来账号的判定必须能记到这条待审请求上"
        );

        assert_eq!(
            store.approve(&r.user_code, false, t0),
            Err(PairingError::Unconfirmed),
            "外来账号且后端未收到确认时必须拒绝批准 —— 默认动作是拒绝"
        );
        assert!(
            matches!(store.poll(&r.poll_token, t0), PollOutcome::Pending),
            "这道拒绝不得推进状态机：勾完确认后同一条请求仍能批"
        );

        assert!(
            store.approve(&r.user_code, true, t0).is_ok(),
            "显式确认之后必须放行：这是「默认拒绝 + 可覆盖」，不是硬拒绝"
        );
        assert!(matches!(
            store.poll(&r.poll_token, t0),
            PollOutcome::Approved { .. }
        ));
    }

    /// 判定结果**不是**外来账号时，确认位传不传都照批 —— 否则这道闸门会把
    /// 「谁都得先勾一下才让批」当成「非本人默认拒绝」来实现，是另一种劣化。
    #[test]
    fn non_foreign_record_needs_no_confirmation() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli", &["read"], t0);

        assert!(store.note_foreign_account(&r.user_code, false));
        assert!(
            store.approve(&r.user_code, false, t0).is_ok(),
            "不是外来账号时确认位必须为假也放行"
        );

        // 从没被记过判定的请求（没查过）同样照批：查不到不得凭空判成外来。
        let unnoted = store.request("other-cli", &["read"], t0);
        assert!(store.approve(&unnoted.user_code, false, t0).is_ok());
    }

    /// 码错时走 [`PairingError::Denied`]，不得走 `Unconfirmed` 那条出口。
    ///
    /// `Unconfirmed` 只对「这张码确实存在、正卡在『外来账号未确认』」开放。把它
    /// 也用在猜码上，就等于给了一个「这张码存不存在、是不是外来」的探针；错码与
    /// 未确认都必须**不碰状态机**，让真正的码在确认之后仍然可用。
    #[test]
    fn wrong_code_is_denied_and_leaves_the_record_untouched() {
        let store = PairingStore::new();
        let t0 = t0();
        let r = store.request("cli-under-test", &["read"], t0);
        store.note_foreign_account(&r.user_code, true);

        assert_eq!(
            store.approve("WRONG1", false, t0),
            Err(PairingError::Denied),
            "错码不得借 Unconfirmed 多问出一个分支"
        );
        assert_eq!(store.approve("WRONG2", true, t0), Err(PairingError::Denied));
        assert!(
            matches!(store.poll(&r.poll_token, t0), PollOutcome::Pending),
            "错码尝试不得改变这条请求的状态"
        );

        assert!(
            store.approve(&r.user_code, true, t0).is_ok(),
            "错码与未确认都没有把真正的配对码废掉"
        );
    }

    /// 记不上就是记不上：外来账号判定写不进任何一条记录时不许凭空造一条出来。
    ///
    /// 写不进去时若默认按「外来」处理，所有查不到判定的请求都会被卡死；若默认按
    /// 「非外来」处理，判定又会被悄悄丢掉。返回 `false` 就是把这件事如实交回调用
    /// 方 —— 生产路径随后仍会在批准那一步重算一次并补记（见
    /// `pairing::approve`）。
    #[test]
    fn noting_a_foreign_verdict_on_an_unknown_code_records_nothing() {
        let store = PairingStore::new();
        let t0 = t0();

        assert!(
            !store.note_foreign_account("NO-SUCH-CODE", true),
            "不存在的配对码不许被记上判定"
        );
        assert!(!store.note_foreign_account("", true), "空配对码同样记不上");
        assert_eq!(store.poll("no-such-token", t0), PollOutcome::Invalid);
    }
}
