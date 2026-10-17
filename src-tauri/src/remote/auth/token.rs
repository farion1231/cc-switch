//! 客户端凭据数据模型与**唯一**摘要比对入口。
//!
//! 服务端只存 `HMAC-SHA256(secret, token)`，明文 token 只在签发那一刻存在，
//! 之后任何时候都不落库 —— 摘要库整体泄露也不构成凭据泄露。
//!
//! 为什么不能用 `[u8; 32]` 的 `==` 比摘要：Rust 对数组/切片的 `PartialEq`
//! 是逐元素短路实现，首个不等字节就返回，执行时间与前缀相同的长度相关，
//! 攻击者能靠计时把摘要一字节一字节试出来。
//!
//! 因此本模块把两条比较路径都钉死：
//!
//! 1. **凭据 vs 摘要**（拿明文算 HMAC 再比）—— 唯一入口是
//!    [`hmac::Mac::verify_slice`]。它内部走 `digest` 的
//!    `subtle::ConstantTimeEq::ct_eq`，本模块**不直接引用 `subtle` crate**：
//!    `subtle` 只是传递依赖，直接引用等于把它钉成公开依赖。
//! 2. **摘要 vs 摘要**（库里两条记录比是否同一凭据）—— [`ClientDigest`] 的
//!    [`ClientDigest::ct_eq`]，其函数体就是 [`ct_eq_32`]：累加全部 32 字节、
//!    循环结束才返回，无提前 `return`。`ClientDigest` 字段私有、不派生
//!    `PartialEq`、不实现 `slice::eq`，所以这两棵子树里的调用方**没有第二条路**
//!    可走 —— 约束由类型构造保证，不靠扫描清单记得住。
//!
//! 依赖边界：只组合既有依赖（`hmac` / `sha2` / `uuid` / `base64`），
//! 不新增 crate、不新增或提升任何依赖版本。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// 客户端凭据。
///
/// 字段全部是「存下来也不怕泄露」的东西：没有明文 token，只有摘要。
/// `cookie_hash` 与 `token_hash` 同构，是会话 cookie 那条通道比对用的那一份摘要；
/// `None` 表示该客户端只发了 token、没建立 cookie 会话。
///
/// 摘要字段持 [`ClientDigest`] 而不是 `[u8; 32]`，因此本类型**不派生**
/// `PartialEq` / `Eq` —— 派生它们等于开一条经由字段逐元素短路的 `==`，
/// 而「两条记录是不是同一凭据」的正确问法是 [`ClientDigest::ct_eq`]。
/// 需要比对时写 `a.token_hash.ct_eq(&b.token_hash)`。
#[derive(Debug, Clone)]
pub struct ClientToken {
    pub client_name: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub token_hash: ClientDigest,
    pub cookie_hash: Option<ClientDigest>,
}

// 装配层接上来之前 `new` / `verify` 没有生产调用点，豁免已随之删除。
impl ClientToken {
    /// 签发一条凭据：只把明文 token 的摘要留下。
    ///
    /// `token` 是调用方刚生成的明文（本层不管生成，只管摘要），
    /// 返回后调用方应立即丢弃它 —— 本结构体里没有任何字段能还原出它。
    pub fn new(client_name: &str, secret: &[u8; 32], token: &str) -> Self {
        let now = now_secs();
        Self {
            client_name: client_name.to_string(),
            created_at: now,
            last_used_at: now,
            token_hash: token_hash(secret, token),
            cookie_hash: None,
        }
    }

    /// 拿「本次请求带上来的明文」比对本记录存的摘要。
    ///
    /// 恒时，路径见模块文档：走 [`hmac::Mac::verify_slice`]，不用 `==`。
    pub fn verify(&self, secret: &[u8; 32], presented: &str) -> bool {
        verify(secret, presented, self.token_hash.as_bytes())
    }
}

/// 生成 32 字节随机 token 并 base64 编码。
///
/// 熵走既有 `uuid` v4 路径（每条 16 字节，取两条凑 32 字节），不新增依赖。
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

/// 摘要计算：`HMAC-SHA256(secret, token)`。
///
/// 与 [`verify`] 共用同一段构造逻辑，避免「算的一方和比的一方用了不同的
/// 参数编码」这种最难查的错。
pub fn token_hash(secret: &[u8; 32], token: &str) -> ClientDigest {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(token.as_bytes());
    ClientDigest::new(mac.finalize().into_bytes().into())
}

/// 摘要 vs 已存摘要的唯一入口。
///
/// 恒时：累加全部 32 字节的差异，循环跑完才返回。**不许提前 `return`** ——
/// 一旦有 `return`，执行时间就又和前缀长度挂钩了。
pub fn verify(secret: &[u8; 32], presented: &str, stored: &[u8; 32]) -> bool {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(presented.as_bytes());
    mac.verify_slice(stored).is_ok()
}

/// 一条已存摘要的私有包装。
///
/// **为什么不直接拿 `[u8; 32]` 比**：`PartialEq` 对数组/切片是逐元素短路实现，
/// 首个不等字节就返回，执行时间与前缀相同的长度相关，攻击者能靠计时把摘要
/// 一字节一字节试出来。把类型收进这里，是让这条约束**由构造保证**而不是由
/// 一份需要人工维护的扫描清单保证：
///
/// - 字段私有 —— 外部拿不到裸数组，唯一取法是 [`ClientDigest::as_bytes`]；
/// - **不派生 `PartialEq` / `Eq`** —— 不存在能意外走通的 `==` 路径；
/// - **不实现 `slice::eq` / `Deref` / `AsRef<[u8]>`** —— 不存在换个拼写就能
///   走通的第二条路；
/// - 唯一的相等判定是 [`ClientDigest::ct_eq`]，函数体就是 [`ct_eq_32`] 的
///   累加逻辑。
#[derive(Clone, Copy)]
pub struct ClientDigest([u8; 32]);

/// 调试输出不印字节本身：摘要是凭据判定材料，`{:?}` 会流进日志与 panic 消息。
impl std::fmt::Debug for ClientDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientDigest(<redacted>)")
    }
}

impl ClientDigest {
    /// 从裸字节装配。落盘 / 解码那一侧需要它；比较一律走 [`ClientDigest::ct_eq`]。
    ///
    /// `const`：判定表的夹具要在 `const` 位置造凭据态。
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// 唯一访问器：交给 `Mac::verify_slice` 这类「自己保证恒时」的 API。
    ///
    /// 它给出裸数组是**故意的** —— 恒时性由接收方负责，而接收方（`hmac` 0.12）
    /// 正是走 `subtle::ConstantTimeEq` 的那一个。
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// 两个已存摘要的恒等比较 —— 本类型**唯一**的相等判定。
    ///
    /// 生产路径上的每个「这两条是不是同一凭据」都必须走这里。
    pub fn ct_eq(&self, other: &Self) -> bool {
        ct_eq_32(&self.0, &other.0)
    }
}

/// 两个已存摘要的恒等比较。
///
/// 用于「库里这条是不是刚才那条」这类没有明文可比、只能比摘要的场景。
///
/// 生产调用点是 [`ClientDigest::ct_eq`] —— 这就是「构造保证」的那一半：
/// 外部模块拿不到裸数组，也没有别的相等路径，于是绕不开这里。
pub fn ct_eq_32(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff: u8 = 0;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [7u8; 32];

    #[test]
    fn ct_eq_32_detects_last_byte_difference() {
        let a = [0u8; 32];
        let mut b = [0u8; 32];
        b[31] = 1;
        assert!(!ct_eq_32(&a, &b));
        assert!(!ct_eq_32(&b, &a));
        assert!(ct_eq_32(&a, &a));
    }

    #[test]
    fn verify_accepts_matching_token() {
        let t = ClientToken::new("cli", &SECRET, "plain-token-value");
        assert_eq!(t.token_hash.as_bytes().len(), 32);
        assert_eq!(t.client_name, "cli");
        assert!(verify(
            &SECRET,
            "plain-token-value",
            t.token_hash.as_bytes()
        ));
        assert!(t.verify(&SECRET, "plain-token-value"));
        assert!(!t.verify(&SECRET, "wrong-token"));
        assert!(!verify(&SECRET, "wrong-token", t.token_hash.as_bytes()));
    }

    #[test]
    fn no_direct_subtle_import() {
        // 只扫 `#[cfg(test)]` 之前的实现区：断言自身写着这个字面量，
        // 全文件扫会自己把自己判为不合格。
        let source = include_str!("token.rs");
        let impl_region = source
            .split("#[cfg(test)]")
            .next()
            .expect("source must have an impl region before the tests");
        assert!(
            !impl_region.contains("use subtle"),
            "token.rs must not import subtle directly"
        );
        // `subtle` 只允许出现在文档说明里，不允许出现在 use 树上。
        for line in impl_region
            .lines()
            .filter(|l| l.trim_start().starts_with("use "))
        {
            assert!(!line.contains("subtle"), "unexpected subtle import: {line}");
        }
    }

    // --- 以下为支撑上述三条规格的旁证，不属于规格本身 ---

    #[test]
    fn wrong_secret_is_rejected() {
        let t = ClientToken::new("cli", &SECRET, "plain-token-value");
        let other = [9u8; 32];
        assert!(!t.verify(&other, "plain-token-value"));
    }

    #[test]
    fn token_hash_is_deterministic_and_secret_dependent() {
        let x = token_hash(&SECRET, "x");
        assert!(
            x.ct_eq(&token_hash(&SECRET, "x")),
            "同 secret 同 token 必须同摘要"
        );
        assert!(!x.ct_eq(&token_hash(&SECRET, "y")), "token 变了摘要必须变");
        assert!(
            !x.ct_eq(&token_hash(&[9u8; 32], "x")),
            "secret 变了摘要必须变"
        );
    }

    #[test]
    fn generated_token_is_32_bytes_of_entropy_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert_eq!(URL_SAFE_NO_PAD.decode(&a).unwrap().len(), 32);
    }

    #[test]
    fn client_token_carries_no_plaintext() {
        let t = ClientToken::new("cli", &SECRET, "plain-token-value");
        let debug = format!("{:?}", t);
        assert!(!debug.contains("plain-token-value"));
        assert!(t.cookie_hash.is_none());
        assert!(t.created_at <= t.last_used_at);
    }

    #[test]
    fn client_digest_is_the_only_digest_comparison_entry() {
        let a = ClientDigest::new([0u8; 32]);
        let mut bytes = [0u8; 32];
        bytes[31] = 1;
        let b = ClientDigest::new(bytes);

        assert!(a.ct_eq(&a), "自身必相等");
        assert!(!a.ct_eq(&b), "末字节不同必须判不等");
        assert!(!b.ct_eq(&a), "比较必须对称");
    }

    #[test]
    fn client_digest_compares_real_hmac_output() {
        let secret = SECRET;
        let issued = token_hash(&secret, "tok");
        assert!(issued.ct_eq(&token_hash(&secret, "tok")));
        assert!(!issued.ct_eq(&token_hash(&secret, "tok-2")));
        assert!(!issued.ct_eq(&token_hash(&[9u8; 32], "tok")));
    }

    #[test]
    fn client_digest_debug_does_not_leak_bytes() {
        let d = token_hash(&SECRET, "plain-token-value");
        let shown = format!("{d:?}");
        assert_eq!(shown, "ClientDigest(<redacted>)");
        assert!(!shown.contains("plain-token-value"));
        // `ClientToken` 的 Debug 走同一条路：结构体整体打印不得带出摘要字节。
        let t = ClientToken::new("cli", &SECRET, "plain-token-value");
        assert_eq!(
            format!("{t:?}").matches("ClientDigest(<redacted>)").count(),
            1
        );
    }

    #[test]
    fn ct_eq_32_has_no_early_return() {
        let source = include_str!("token.rs");
        let body = source
            .split("pub fn ct_eq_32")
            .nth(1)
            .expect("ct_eq_32 must exist");
        let body = body.split("fn now_secs").next().unwrap();
        assert!(!body.contains("return"), "ct_eq_32 must not return early");
        assert!(!body.contains("break"), "ct_eq_32 must not break early");
    }
}
