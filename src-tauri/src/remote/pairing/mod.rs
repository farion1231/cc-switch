//! 配对子系统：把一条「这台电脑要授权给某个 CLI」的请求从发起走到交付。
//!
//! 分层与 [`super::auth`] 一致 —— 判据与状态在 [`state`]，HTTP 形态在
//! [`handlers`]：
//!
//! | 模块 | 回答的问题 | 碰不碰 I/O |
//! |------|------------|------------|
//! | [`state`] | 这条请求现在处于哪个状态、这次转移合不合法 | 否（`now` 显式注入） |
//! | [`handlers`] | HTTP 上长什么样，凭据从哪读 | 只碰 socket 与事件出口 |
//!
//! 时间一律作为参数传进状态机：本 crate 的 `tokio` 没开 `test-util`，
//! 冻结时钟编不出来；而为了让测试好过把生产 TTL 调小，会让「配对码活十分钟」
//! 这条真实约束在测试里消失。参数注入两头都不亏。
//!
//! 配对路由是未持有凭据一方（`AuthScope::Public`）在路由分区里的唯一业务内容
//! —— 没有它，未持有凭据的一方没有任何合法入口能拿到凭据。其中 `approve` /
//! `revoke` 是桌面端那侧的动作，只接受 loopback
//! 来源，见 [`handlers`] 的模块文档。

mod handlers;
mod state;

use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::remote::auth::{load_clients, save_clients};
use crate::remote::events::{ApprovalEvent, ApprovalEvidence, EventSink, SourceKind};
use crate::remote::whois::{self_user_id, whois_evidence, WhoisFields};

// 路由分区与鉴权中间件接上来之前，这些 re-export 全是「未使用」；如今装配层
// 只经这里取 `router` / `PairingApiState` / `PairingStore`，其余名字由各模块内部
// 路径直接引用，故仍需放行未使用。
#[allow(unused_imports)]
pub use handlers::{
    pairing_approve, pairing_revoke, poll_token_from_headers, router, PairingApiState,
    PairingApproveBody, PairingPollBody, PairingPollResponse, PairingRequestBody,
    PairingRequestResponse, PairingRevokeBody, PAIRING_SCHEME,
};
#[allow(unused_imports)]
pub use state::{
    ttl_expired, PairingError, PairingStatus, PairingStore, PollOutcome, RequestTicket,
    PAIRING_TTL, POLL_INTERVAL_SECS, RATE_LIMIT_MAX, RATE_LIMIT_WINDOW,
};

/// 把 `whois` 的原始结果组装成一条审批证据。
///
/// 「是否本人账号」在这里被钉死为一次**字符串比对**：`tailscale whois --json` 的
/// `Node.User.ID`（对端所属账号）与 `tailscale status --json` 的 `Self.UserID`
/// （本机所属账号）不等，就说明这个节点是从**另一个账号** share 进本 tailnet 的
/// —— 标记为可疑，交界面默认拒绝。
///
/// 两个输入都可以缺，缺法各自的含义不同：
///
/// | 输入 | `source_kind` | `is_foreign_account` |
/// |---|---|---|
/// | `whois = Some(..)` | [`SourceKind::Tailnet`] | 两侧 id 都有且不等时为 `true` |
/// | `whois = None`（超时 / 找不到 bin / 判不出来） | [`SourceKind::Unavailable`] | `false` |
///
/// `self_uid = None` 时**不**判为可疑：本机账号查不到，任何对端都会被误标成「别人
/// 的账号」，那比漏标更糟。查不到就是查不到，交给界面按「无法判定」要求显式确认
/// （见 [`ApprovalDecision`]）。
///
/// 身份字段一律是 `Option`：[`WhoisFields`] 的字段是留空串的 `String`，空串不是身份
/// 证据，直接透传会让界面把「没查到」显示成「名字为空」。
pub fn build_evidence(
    peer: SocketAddr,
    whois: Option<WhoisFields>,
    self_uid: Option<&str>,
) -> ApprovalEvidence {
    let (source_kind, node_name, node_stable_id, login_name, is_foreign_account) = match whois {
        Some(fields) => (
            SourceKind::Tailnet,
            non_empty(fields.node_name),
            non_empty(fields.node_stable_id),
            non_empty(fields.login_name),
            match self_uid {
                Some(self_uid) => !self_uid.is_empty() && self_uid != fields.user_id,
                None => false,
            },
        ),
        None => (SourceKind::Unavailable, None, None, None, false),
    };

    ApprovalEvidence {
        source_kind,
        node_name,
        node_stable_id,
        login_name,
        is_foreign_account,
        peer_addr: peer.to_string(),
    }
}

/// 取数之前能**确定**来源的情形。`None` = 「要问 tailscale 才知道」。
///
/// 只有回环一种：对端就在这台机器上，这是地址本身决定的确定性事实。tailnet
/// 地址一律返回 `None` —— 是不是网内节点、属于哪个账号，只有 `whois` 说了算。
fn classify_peer(peer: SocketAddr) -> Option<SourceKind> {
    peer.ip().is_loopback().then_some(SourceKind::Loopback)
}

/// 走真实 `tailscale` 二进制取一条证据：先问对端是谁，再问本机是谁。
///
/// 两条查询各自有 2s 上限（见 [`crate::remote::whois::WHOIS_TIMEOUT`]），任一条查不到
/// 都只是退化成「无法判定」，不会让审批流程卡住或崩掉。
///
/// 回环对端走短路（见 [`classify_peer`]）：它不需要查，却**能被确定地**判出来，
/// 而「查出来是这样」与「就是本机进程」不是同一句话。
pub async fn evidence_for(peer: SocketAddr) -> ApprovalEvidence {
    if classify_peer(peer) == Some(SourceKind::Loopback) {
        return ApprovalEvidence::loopback(peer.to_string());
    }
    let (_, fields) = whois_evidence(peer.ip()).await;
    let self_uid = self_user_id().await;
    build_evidence(peer, fields, self_uid.as_deref())
}

/// 把一条待批请求组装成事件推给桌面端。
///
/// 事件出口与 `provider-switched` 同款（同一个 [`EventSink`]），桌面端只订阅一处。
pub fn emit_approval(
    sink: &dyn EventSink,
    request_id: &str,
    client_name: &str,
    user_code: &str,
    evidence: ApprovalEvidence,
) {
    sink.emit(&ApprovalEvent {
        request_id: request_id.to_string(),
        client_name: client_name.to_string(),
        user_code: user_code.to_string(),
        evidence,
    });
}

/// 桌面端拿到证据后该怎么处置。
///
/// `Unavailable` 与 `NotInTailnet` 走的是同一条分支：两者都**无法为这次授权背书**
/// —— 一个是「没查出来」，一个是「查出来了，但它不在本 tailnet」。界面因此把建议
/// 动作翻成拒绝，并要求一次显式确认；后端也据此默认不批（见 [`approve`]）。
/// `ConfirmForeign` 同理 —— 桌面端仍然可以批准，但默认动作是拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    /// 证据齐备且是本账号的节点：正常批准。
    Approve,
    /// 来源无法核实：默认拒绝，勾了确认才放行。
    ConfirmUnavailable,
    /// 对端属于另一个账号：界面默认拒绝。
    ConfirmForeign,
}

/// 证据 → 处置建议。纯函数，不碰 I/O、不取时钟。
pub fn decide(evidence: &ApprovalEvidence) -> ApprovalDecision {
    if evidence.is_foreign_account {
        return ApprovalDecision::ConfirmForeign;
    }
    match evidence.source_kind {
        // `NotInTailnet` 与 `Unavailable` 一视同仁：前者是确定的否定，后者是查不到，
        // 但都拿不出一个能为这次授权背书的来源。前端的 `isDegraded` 覆盖的正是这两类。
        SourceKind::Unavailable | SourceKind::NotInTailnet => ApprovalDecision::ConfirmUnavailable,
        _ => ApprovalDecision::Approve,
    }
}

/// 桌面端批准一条待审请求，并交回**这条批准是在什么证据下做出的**。
///
/// 证据在这里被**消费**，而不是收下就丢：它折成一条 [`ApprovalDecision`] 交回调用
/// 方记审计（`approve_pairing` 会打日志）。返回 `None` 表示调用方手上根本没有证据
/// —— 如实说「没有」，而不是凭空编一条决策。
///
/// **来源核不出来时默认不放行。** `confirmed_unverified_source` 是用户在界面上
/// 勾过的那道「我已知悉来源无法核实」；缺它就返回 [`PairingError::Unconfirmed`]，
/// 且**不碰状态机** —— 请求仍待审，勾完确认再来一次即可。这是「默认拒绝 + 显式
/// 确认可覆盖」，不是硬拒绝：把 `whois` 超时当成一条谁也批不了的死路，同样是错的。
///
/// **外来账号同样默认不放行，且这一道由状态机自己拦。** 证据里的
/// `is_foreign_account` 先被**记回** store（[`PairingStore::note_foreign_account`]），
/// 再由 [`PairingStore::approve`] 用 `ack_foreign` 核对。记在库里而不是只留在
/// 调用点，是因为记在调用点的话，下一次不传证据（`None` 是合法形态）就把它洗掉了
/// —— 后端的防线会退回成「谁记得传证据谁安全」，而绕过渲染层的调用恰好就是那一类。
///
/// 两道确认是**两道独立的门**：`ConfirmForeign` 与 `ConfirmUnavailable` 各自只认
/// 自己的标志位，顶替勾选等于让「我认了这不是本机账号」顺带把「来源查不到我也认了」
/// 一起勾掉。
///
/// 反过来，已核实来源（Tailscale 节点 / 本机回环）**不需要**这两道确认 —— 否则这条
/// 命令会退化成「谁都得先勾一个框」。
pub fn approve(
    store: &PairingStore,
    user_code: &str,
    evidence: Option<&ApprovalEvidence>,
    confirmed_unverified_source: bool,
    confirmed_foreign_account: bool,
    now: Instant,
) -> Result<Option<ApprovalDecision>, PairingError> {
    let decision = evidence.map(decide);

    // 无证据时没有决策位可说，如实返回 None 放行（生产路径总是重算过的，恒为 Some）。
    if matches!(decision, Some(ApprovalDecision::ConfirmUnavailable))
        && !confirmed_unverified_source
    {
        return Err(PairingError::Unconfirmed);
    }

    // 把这一轮重算出的「是不是外来账号」记回库：批准那一步才是真正生效的那一次，
    // 记不上（码不存在）也无妨 —— 状态转移自己会先拒。
    if let Some(evidence) = evidence {
        store.note_foreign_account(user_code, evidence.is_foreign_account);
    }

    // 状态转移以 `user_code` 为准；外来账号那道闸门在 state 层，不靠调用方自觉。
    store.approve(user_code, confirmed_foreign_account, now)?;
    Ok(decision)
}

/// 凭据库里一条客户端的**元数据**。
///
/// 刻意不含 `token_hash`：摘要虽不可逆，但它是一份「可复现 token」的判定材料，
/// 不该跟着设备管理界面发到前端。前端要展示的就这些。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientMeta {
    pub client_name: String,
    /// Unix 秒。
    pub created_at: i64,
    /// Unix 秒。
    pub last_used_at: i64,
}

/// 列出已配对客户端的元数据。按名字排序，让界面顺序稳定。
///
/// 读的是凭据库本身 —— 也就是鉴权中间件真正认的那份文件，而不是某个内存索引。
pub fn list_client_meta(config_dir: &Path) -> Result<Vec<ClientMeta>, String> {
    let clients = load_clients(config_dir).map_err(|e| e.to_string())?;
    let mut metas: Vec<ClientMeta> = clients
        .into_iter()
        .map(|c| ClientMeta {
            client_name: c.client_name,
            created_at: c.created_at,
            last_used_at: c.last_used_at,
        })
        .collect();
    metas.sort_by(|a, b| a.client_name.cmp(&b.client_name));
    Ok(metas)
}

/// 按客户端名撤销其全部凭据。返回是否摘到了东西（撤销是幂等的）。
///
/// 摘的是**摘要**，不碰明文：明文除了交付那一刻与真正撤销那一刻以外不存在，按名字
/// 撤销时本来就拿不到它，也不需要它 —— 中间件只认库里的摘要。
pub fn revoke_client_by_name(config_dir: &Path, client_name: &str) -> Result<bool, String> {
    let mut clients = load_clients(config_dir).map_err(|e| e.to_string())?;
    let before = clients.len();
    clients.retain(|c| c.client_name != client_name);
    if clients.len() == before {
        return Ok(false);
    }
    save_clients(config_dir, &clients).map_err(|e| e.to_string())?;
    Ok(true)
}

/// 空串 → `None`。[`WhoisFields`] 的字段是留空串的 `String`，空串不是身份证据。
fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::whois::WhoisFields;
    use std::net::SocketAddr;
    use std::time::Instant;

    /// 一个**自造的**对端地址。真实 tailnet IP 与账号名一律不进测试夹具 ——
    /// 判据该对什么值敏感，就让它对明写着「u-other ≠ u-self」的值敏感。
    fn peer() -> SocketAddr {
        "198.51.100.7:53211".parse().expect("test peer addr")
    }

    fn fields(user_id: &str) -> WhoisFields {
        WhoisFields {
            node_name: "node-under-test".into(),
            node_stable_id: "stable-under-test".into(),
            login_name: "peer-under-test@example.invalid".into(),
            user_id: user_id.into(),
        }
    }

    /// `Node.User.ID != Self.UserID` → `is_foreign_account == true`。
    ///
    /// 这一条钉的是「从另一个账号 share 进本 tailnet」这个具体场景：节点在网内、
    /// tailscale 认得它，所以 `source_kind` 是 `Tailnet`，但账号不是本机的账号 ——
    /// 必须被标出来交界面默认拒绝。
    #[test]
    fn foreign_account_is_flagged() {
        let ev = build_evidence(peer(), Some(fields("u-other")), Some("u-self"));

        assert!(
            ev.is_foreign_account,
            "Node.User.ID 与 Self.UserID 不等时必须标记为外来账号"
        );
        assert_eq!(ev.source_kind, SourceKind::Tailnet, "节点确实在 tailnet 内");
        assert_eq!(decide(&ev), ApprovalDecision::ConfirmForeign);
        assert_eq!(ev.peer_addr, peer().to_string());

        // 同账号 → 不标记。这条不是凑数：不加它的话，
        // 「永远返回 true」也能让上面那条变绿。
        let same = build_evidence(peer(), Some(fields("u-self")), Some("u-self"));
        assert!(!same.is_foreign_account);
        assert_eq!(decide(&same), ApprovalDecision::Approve);

        // 本机账号查不到 → 无法判定，不是「外来」。
        let unknown = build_evidence(peer(), Some(fields("u-other")), None);
        assert!(
            !unknown.is_foreign_account,
            "本机账号查不到时不得把任何对端都标成外来"
        );
    }

    /// `Unavailable` → `node_*` / `login_name` 全为 `None`。
    ///
    /// 界面据此要求显式确认。空串不是「名字为空」的身份证据，`WhoisFields` 里
    /// 取不到的字段就是空串，直接透传会让界面显示一个假的空身份。
    #[test]
    fn unavailable_evidence_has_no_identity_fields() {
        let ev = build_evidence(peer(), None, Some("u-self"));

        assert_eq!(ev.source_kind, SourceKind::Unavailable);
        assert_eq!(ev.node_name, None);
        assert_eq!(ev.node_stable_id, None);
        assert_eq!(ev.login_name, None);
        assert!(!ev.is_foreign_account, "查不到来源时不得凭空判外来");
        assert_eq!(decide(&ev), ApprovalDecision::ConfirmUnavailable);

        // `whois` 回来了但字段全空：仍然是 Tailnet（CLI 认得这个地址），
        // 身份字段仍必须是 None。
        let empty = build_evidence(peer(), Some(WhoisFields::default()), Some("u-self"));
        assert_eq!(empty.source_kind, SourceKind::Tailnet);
        assert_eq!(empty.node_name, None);
        assert_eq!(empty.login_name, None);
    }

    /// `whois` 超时后仍能正常批准：证据降级不等于审批停摆。
    ///
    /// 判据不是「不报错」，而是走完整条链：`request` → 超时（`Unavailable`）→
    /// `approve` → `poll` 真的换到凭据。中间只要任何一步因为「查不到身份」而失败，
    /// 配对就成了一条谁也走不通的死路。
    #[test]
    fn approve_after_whois_timeout_still_works() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli-under-test", &["read"], now);

        // 超时走的就是 whois = None 这一支（找不到 bin / 判不出来同一条路）。
        let timed_out = build_evidence(peer(), None, None);
        assert_eq!(timed_out.source_kind, SourceKind::Unavailable);

        assert_eq!(
            decide(&timed_out),
            ApprovalDecision::ConfirmUnavailable,
            "超时要换来界面上的一次显式确认，而不是一条拒绝"
        );
        approve(
            &store,
            &ticket.user_code,
            Some(&timed_out),
            true,
            false,
            now,
        )
        .expect("whois 超时不得让批准失败");

        assert!(
            matches!(
                store.poll(&ticket.poll_token, now),
                PollOutcome::Approved { .. }
            ),
            "批准之后必须真的换到凭据"
        );

        // 拒绝路径同样不受影响：超时的另一条请求仍能被干净地拒掉。
        let other = store.request("other-cli", &["read"], now);
        store
            .reject(&other.user_code, now)
            .expect("超时后拒绝也必须可用");
        assert_eq!(store.poll(&other.poll_token, now), PollOutcome::Rejected);

        // 过期后批不动：降级不等于放宽 TTL。
        let stale = store.request("stale-cli", &["read"], now);
        let expired = build_evidence(peer(), None, None);
        assert!(
            approve(
                &store,
                &stale.user_code,
                Some(&expired),
                true,
                false,
                now + PAIRING_TTL
            )
            .is_err(),
            "降级不得放宽 TTL"
        );
    }

    /// `approve()` 必须**消费**证据并把决策交回调用方，而不是把它丢掉。
    ///
    /// 形参写成 `_evidence` 时证据进了函数就消失，审批链路上没有任何一处留下
    /// 「当时看到的来源是什么」—— 那是这条判据最初失效的形态：比对逻辑写对了，
    /// 却没有生产调用点。
    #[test]
    fn approve_consumes_evidence_and_returns_its_decision() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli", &["read"], now);

        let foreign = build_evidence(peer(), Some(fields("u-other")), Some("u-self"));
        assert_eq!(
            approve(&store, &ticket.user_code, Some(&foreign), false, true, now)
                .expect("确认外来账号后必须放行"),
            Some(ApprovalDecision::ConfirmForeign),
            "外来账号证据必须被折成决策交回调用方记审计，而不是被丢弃"
        );

        // 证据缺失仍然放行 —— 环境问题不是请求有问题，但决策位是「没有」。
        let other = store.request("other-cli", &["read"], now);
        assert_eq!(
            approve(&store, &other.user_code, None, false, false, now).expect("无证据也必须能批准"),
            None,
            "没有证据时如实交回 None，不得凭空编一条决策"
        );

        let third = store.request("third-cli", &["read"], now);
        let ok = build_evidence(peer(), Some(fields("u-self")), Some("u-self"));
        assert_eq!(
            approve(&store, &third.user_code, Some(&ok), false, false, now)
                .expect("同账号证据批准必须成功"),
            Some(ApprovalDecision::Approve)
        );
    }

    /// 来源核不出来时**默认不批**，勾了确认才放行；且这次拒绝不得吃掉这条请求。
    ///
    /// 「查不到来源」不是「可以批准」——旧策略让它一路直通，等于把一条谁也
    /// 没核实的请求当成了已核实。这里钉的是两件事：默认那一步必须失败，以及
    /// 失败之后请求仍**待审**（用户还能勾完确认再批），不是被判死。
    #[test]
    fn unverified_source_needs_an_explicit_confirmation() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli-under-test", &["read"], now);
        let unverifiable = build_evidence(peer(), None, None);

        assert_eq!(
            approve(
                &store,
                &ticket.user_code,
                Some(&unverifiable),
                false,
                false,
                now
            ),
            Err(PairingError::Unconfirmed),
            "证据核不出来时不得默认放行"
        );
        assert!(
            matches!(store.poll(&ticket.poll_token, now), PollOutcome::Pending),
            "默认拒绝不得把这条待审请求消费掉 —— 用户还能勾完确认再来一次"
        );

        // 不是硬拒绝：显式确认一勾，同一条请求就放行，且决策仍如实记下取证时的状态。
        assert_eq!(
            approve(
                &store,
                &ticket.user_code,
                Some(&unverifiable),
                true,
                false,
                now
            )
            .expect("显式确认后必须放行"),
            Some(ApprovalDecision::ConfirmUnavailable),
        );
        assert!(
            matches!(
                store.poll(&ticket.poll_token, now),
                PollOutcome::Approved { .. }
            ),
            "确认之后必须真的换到凭据"
        );
    }

    /// `NotInTailnet` 是**确定的否定**（CLI 认得这个地址并说它不在网内），不是
    /// 「查不到」。但它同样无法为这次授权背书，所以与 `Unavailable` 共用同一道
    /// 确认 —— 前端的 `isDegraded` 覆盖的正是这两类，两端必须一致。
    #[test]
    fn not_in_tailnet_needs_the_same_confirmation_as_unavailable() {
        let mut ev = build_evidence(peer(), Some(fields("u-self")), Some("u-self"));
        ev.source_kind = SourceKind::NotInTailnet;

        assert_eq!(
            decide(&ev),
            ApprovalDecision::ConfirmUnavailable,
            "不在本 tailnet 不得被当成已核实来源直接放行"
        );

        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli", &["read"], now);
        assert_eq!(
            approve(&store, &ticket.user_code, Some(&ev), false, false, now),
            Err(PairingError::Unconfirmed)
        );
        assert_eq!(
            approve(&store, &ticket.user_code, Some(&ev), true, false, now).expect("确认后放行"),
            Some(ApprovalDecision::ConfirmUnavailable)
        );
    }

    /// 证据齐备的请求**不需要**任何确认，标志位是 `false` 时也必须照批。
    ///
    /// 反向判据：把上一条做成「一律要确认」，这两条都能变绿 —— 那样等于把
    /// 「核不出来就默认不批」偷换成「谁都得勾一下才让批」，是另一种劣化。
    #[test]
    fn verified_source_needs_no_confirmation() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli", &["read"], now);
        let verified = build_evidence(peer(), Some(fields("u-self")), Some("u-self"));

        assert_eq!(
            approve(
                &store,
                &ticket.user_code,
                Some(&verified),
                false,
                false,
                now
            )
            .expect("已核实来源不得被拦下"),
            Some(ApprovalDecision::Approve)
        );

        // 回环同理：对端就在本机，地址本身说了算。
        let local = store.request("local-cli", &["read"], now);
        assert_eq!(
            approve(
                &store,
                &local.user_code,
                Some(&ApprovalEvidence::loopback("127.0.0.1:5000")),
                false,
                false,
                now
            )
            .expect("回环来源不得被拦下"),
            Some(ApprovalDecision::Approve)
        );
    }

    /// 外来账号的默认拒绝**必须由后端执行**，不能只靠界面上那个勾。
    ///
    /// 反向判据：把这条写成「后端照批、界面自己拦」，上面这条一样能绿 ——
    /// `decide()` 早就把外来账号折成了 `ConfirmForeign`，界面看到它就会默认勾成
    /// 拒绝。区别在于任何绕过渲染层的调用（脚本、别的命令、将来的 HTTP 路径）
    /// 都能批准一个外来账号，而这里要守的判据是「非本人时**默认拒绝**」，不是
    /// 「非本人时默认在界面上显示为拒绝」。
    #[test]
    fn foreign_account_is_refused_by_the_backend_not_only_by_the_ui() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli-under-test", &["read"], now);
        let foreign = build_evidence(peer(), Some(fields("u-other")), Some("u-self"));
        assert_eq!(decide(&foreign), ApprovalDecision::ConfirmForeign);

        assert_eq!(
            approve(&store, &ticket.user_code, Some(&foreign), false, false, now),
            Err(PairingError::Unconfirmed),
            "外来账号在默认动作上必须由后端拒绝"
        );
        assert!(
            matches!(store.poll(&ticket.poll_token, now), PollOutcome::Pending),
            "这道拒绝不得推进状态机：确认之后同一条请求仍能批"
        );

        assert_eq!(
            approve(&store, &ticket.user_code, Some(&foreign), false, true, now)
                .expect("显式确认之后必须放行"),
            Some(ApprovalDecision::ConfirmForeign),
            "决策位仍如实记下取证时的状态"
        );
        assert!(matches!(
            store.poll(&ticket.poll_token, now),
            PollOutcome::Approved { .. }
        ));
    }

    /// 外来账号的判定**记在 store 里**，不是记在调用点。
    ///
    /// 记在调用点的话，第二次进来只要不重复传证据（`evidence = None` 是一条合法的
    /// 生产路径，见 `approve_bound` 重算失败时的形态），判定就被洗掉了 —— 后端的
    /// 防线退回成「谁记得传证据谁安全」。记进 [`PairingStore`] 之后，省掉证据也
    /// 绕不过去。
    #[test]
    fn foreign_verdict_is_remembered_by_the_store_not_just_by_the_caller() {
        let store = PairingStore::new();
        let now = Instant::now();
        let ticket = store.request("cli", &["read"], now);
        let foreign = build_evidence(peer(), Some(fields("u-other")), Some("u-self"));

        assert_eq!(
            approve(&store, &ticket.user_code, Some(&foreign), false, false, now),
            Err(PairingError::Unconfirmed)
        );
        assert_eq!(
            approve(&store, &ticket.user_code, None, false, false, now),
            Err(PairingError::Unconfirmed),
            "外来账号的判定已经落在 store 里，这一轮不传证据不该把它洗掉"
        );
        assert!(
            approve(&store, &ticket.user_code, None, false, true, now).is_ok(),
            "确认位仍然生效"
        );
    }

    /// 确认外来账号**不**顶替确认来源不可核实：两道勾是两道独立的门。
    ///
    /// 把它们折叠成一道，等于让「我看到这是个别人的账号，我认了」顺带把「来源查不
    /// 到，我也认了」一起勾掉 —— 而后者的证据是 `Unavailable` / `NotInTailnet`，
    /// 与账号身份无关。
    #[test]
    fn confirming_foreign_does_not_confirm_an_unverifiable_source() {
        let store = PairingStore::new();
        let now = Instant::now();
        let unverifiable = build_evidence(peer(), None, None);
        assert_eq!(decide(&unverifiable), ApprovalDecision::ConfirmUnavailable);

        let ticket = store.request("cli", &["read"], now);
        assert_eq!(
            approve(
                &store,
                &ticket.user_code,
                Some(&unverifiable),
                false,
                true,
                now
            ),
            Err(PairingError::Unconfirmed),
            "只勾了外来账号确认时，来源核不出来这一道仍须拦下"
        );
        assert!(approve(
            &store,
            &ticket.user_code,
            Some(&unverifiable),
            true,
            false,
            now
        )
        .is_ok());
    }

    /// 结构锁：`approve()` 的形参不得再叫 `_evidence`。
    ///
    /// 参数名带下划线 = 编译器认定「读了也不用」。这条钉住的是名字本身：把
    /// `_evidence` 改回来时，不依赖任何运行时观察就会红。
    #[test]
    fn approve_signature_does_not_discard_the_evidence() {
        let source = include_str!("mod.rs");
        let signature = source
            .split("pub fn approve(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("approve() signature must be present");

        assert!(
            !signature.contains("_evidence"),
            "approve() 的证据形参不得再被丢弃：{signature}"
        );
    }

    /// 本机回环来源**不必查** `whois` 就能确定地判出来。
    ///
    /// 127.0.0.1 的对端就在这台机器上 —— 这是一条确定性事实，与「查不出来」是两
    /// 回事。硬把它塞进 `whois` 只会得到 `Unavailable`，让界面为一个它本来就知道的
    /// 结论多要一次确认。判据要求三类证据各自分支渲染，`Loopback` 这一支不能因为
    /// 「统一走 whois」而变成只有测试才走得到的死代码。
    #[test]
    fn loopback_peer_is_classified_without_probing() {
        assert_eq!(
            classify_peer("127.0.0.1:53211".parse().unwrap()),
            Some(SourceKind::Loopback),
            "回环来源必须被认成 Loopback，而不是降级成 Unavailable"
        );
        assert_eq!(
            classify_peer("[::1]:53211".parse().unwrap()),
            Some(SourceKind::Loopback),
            "IPv6 回环同理"
        );

        // tailnet 地址查不查得到是另一回事：不得在**取数之前**就替它下结论。
        assert_eq!(classify_peer(peer()), None);
    }

    /// 回环对端拿到的证据：Loopback 分类、不带任何 tailnet 身份、也不判外来。
    #[tokio::test]
    async fn evidence_for_a_loopback_peer_yields_loopback_evidence() {
        let ev = evidence_for("127.0.0.1:53211".parse().unwrap()).await;

        assert_eq!(ev.source_kind, SourceKind::Loopback);
        assert_eq!(ev.node_name, None);
        assert_eq!(ev.login_name, None);
        assert!(!ev.is_foreign_account, "本机进程不可能是别的账号");
        assert_eq!(ev.peer_addr, "127.0.0.1:53211");
        assert_eq!(decide(&ev), ApprovalDecision::Approve);
    }
}
