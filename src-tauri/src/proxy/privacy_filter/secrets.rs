//! 密钥 / 凭证检测：gitleaks 规则（关键词预筛）+ 上下文口令 + 高熵兜底

use super::{Kind, Span};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;

/// 高熵兜底默认阈值（贴近 gitleaks 经验值）
const ENTROPY_MIN: f64 = 4.0;
/// 周围无密钥语义关键词时启用，进一步压低误报
const ENTROPY_MIN_STRICT: f64 = 4.8;
/// 上下文判定往前回溯的字节数
const CONTEXT_LOOKBACK: usize = 30;

/// 上下文型口令：藏在句子里的密码/token，如「我的密码是 hunter2」「api_key: xxx」
static RE_CONTEXT_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(密码|口令|密钥|password|passwd|pwd|secret|token|api[_\s-]?key)\s*(?:是|为|:|：|=)\s*['"]?([^\s'"，。；;]{4,})"#,
    )
    .unwrap()
});

/// 高熵兜底：抓不匹配任何已知格式的随机串
static RE_ENTROPY_TOKEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{20,}").unwrap());

/// 密钥语义关键词。命中则保留 ENTROPY_MIN，否则收紧到 ENTROPY_MIN_STRICT。
static RE_SECRET_CONTEXT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(?:password|passwd|pwd|secret|token|api[_\s-]?key|access[_\s-]?key|bearer|authorization|credential|jwt|密码|口令|密钥|凭证|令牌|鉴权)",
    )
    .unwrap()
});

/// HTTP Authorization 头标准形态：Authorization: <scheme> <value>
static RE_AUTH_HEADER_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bauthorization\s*:\s*(?:basic|bearer|digest|ntlm|hmac|token)\s+$").unwrap()
});

/// 以 host.tld: 开头的命中（generic-api-key 把域名+冒号+路径一起吃掉的情形）
static RE_HOST_PORT_PREFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9.-]*\.[A-Za-z0-9-]+:").unwrap());

/// 模板变量：{{ X }} / ${X} / %{X} / <X>
static RE_TEMPLATE_VAR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(?:\{\{[^{}]+\}\}|\$\{[^{}]+\}|%\{[^{}]+\}|<[^<>]+>)$").unwrap());
static RE_UUID: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
        .unwrap()
});

/// 路径/URL/哈希边界字符：候选串左右贴着这些字符 → 路径分段 / sha256: / @host / query 参数
const PATH_BOUNDARY_CHARS: &[u8] = b"/\\:.@?=";
/// 候选串内部含这些字符 → 整段就是路径
const PATH_INTERNAL_CHARS: &[char] = &['/', '\\', ':'];

/// 协议/哈希前缀，候选串左侧短回溯命中即视作路径片段
const URL_PREFIXES: &[&str] = &[
    "http://", "https://", "ftp://", "ssh://", "s3://", "gs://", "oss://", "git@", "sha256:",
    "sha1:", "md5:",
];

/// 强上下文判定时允许出现在「关键词」和「候选串」之间的赋值分隔字符
const ASSIGNMENT_CHARS: &[u8] = b" \t\r\n=:'\"";

/// 业务标识符的常见变量名后缀。`order_id=xxxxx` 这类不应被当密钥。
const BENIGN_ID_SUFFIXES: &[&str] = &["_id", "_uuid", "_uid", "_oid", "_no", "_seq"];

/// 常见占位符词（大小写不敏感的子串匹配）
const COMMON_PLACEHOLDERS: &[&str] = &[
    "REPLACE_ME",
    "REPLACE_THIS",
    "REPLACE_WITH",
    "YOUR_KEY",
    "YOUR_TOKEN",
    "YOUR_SECRET",
    "YOUR_API_KEY",
    "YOUR_PASSWORD",
    "INSERT_HERE",
    "INSERT_KEY",
    "INSERT_TOKEN",
    "PLACEHOLDER",
    "EXAMPLE_KEY",
    "EXAMPLE_TOKEN",
    "TODO",
    "FIXME",
    "XXXX",
];

struct SecretRule {
    re: Regex,
    /// 已小写；空表示该规则总是参与
    keywords: Vec<String>,
    entropy: f64,
    secret_group: usize,
}

pub(super) struct SecretDetector {
    rules: Vec<SecretRule>,
    /// 因正则语法不兼容被跳过的规则数
    skipped: usize,
}

/// gitleaks.toml 的最小结构；未知字段（allowlist 等）忽略
#[derive(Deserialize)]
struct TomlConfig {
    #[serde(default)]
    rules: Vec<TomlRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TomlRule {
    #[serde(default)]
    regex: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    entropy: f64,
    #[serde(default)]
    secret_group: usize,
}

impl SecretDetector {
    pub(super) fn from_gitleaks_toml(toml_text: &str) -> Self {
        let mut sd = Self {
            rules: Vec::new(),
            skipped: 0,
        };
        match toml::from_str::<TomlConfig>(toml_text) {
            Ok(cfg) => {
                for r in cfg.rules {
                    if r.regex.is_empty() {
                        continue;
                    }
                    let Ok(re) = Regex::new(&r.regex) else {
                        sd.skipped += 1;
                        continue;
                    };
                    sd.rules.push(SecretRule {
                        re,
                        keywords: r.keywords.iter().map(|k| k.to_lowercase()).collect(),
                        entropy: r.entropy,
                        secret_group: r.secret_group,
                    });
                }
            }
            Err(e) => log::warn!("[PrivacyFilter] Failed to parse gitleaks rules: {e}"),
        }
        if sd.rules.is_empty() {
            sd.load_builtin();
        }
        sd
    }

    pub(super) fn stats(&self) -> (usize, usize) {
        (self.rules.len(), self.skipped)
    }

    /// gitleaks 规则缺失时的兜底规则
    fn load_builtin(&mut self) {
        let builtin: &[(&str, &[&str])] = &[
            (r"sk-(?:proj-)?[A-Za-z0-9_-]{20,}", &["sk-"]),
            (r"AKIA[0-9A-Z]{16}", &["akia"]),
            (
                r"gh[pousr]_[A-Za-z0-9]{36,}",
                &["ghp_", "gho_", "ghu_", "ghs_", "ghr_"],
            ),
            (r"AIza[0-9A-Za-z_-]{35}", &["aiza"]),
            (r"xox[baprs]-[0-9A-Za-z-]{10,}", &["xox"]),
            (
                r"eyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
                &["eyj"],
            ),
            (r"-----BEGIN[A-Z ]*PRIVATE KEY-----", &["private key"]),
        ];
        for (pat, kws) in builtin {
            self.rules.push(SecretRule {
                re: Regex::new(pat).unwrap(),
                keywords: kws.iter().map(|k| k.to_string()).collect(),
                entropy: 0.0,
                secret_group: 0,
            });
        }
    }

    pub(super) fn detect(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let low = text.to_lowercase();
        let push = |spans: &mut Vec<Span>, start, end| {
            spans.push(Span {
                start,
                end,
                kind: Kind::Secret,
            })
        };

        // Route 1: gitleaks 规则，关键词预筛 —— 只对命中关键词的规则跑正则
        for r in &self.rules {
            if !r.keywords.is_empty() && !r.keywords.iter().any(|k| low.contains(k.as_str())) {
                continue;
            }
            for caps in r.re.captures_iter(text) {
                let whole = caps.get(0).unwrap();
                let m = if r.secret_group > 0 {
                    caps.get(r.secret_group).unwrap_or(whole)
                } else {
                    whole
                };
                let (s, e) = (m.start(), m.end());
                if s >= e {
                    continue;
                }
                let cand = m.as_str();
                if r.entropy > 0.0 && shannon_entropy(cand) < r.entropy {
                    continue; // 复刻 gitleaks 的熵阈值
                }
                // 不能用 Route 3 的 is_on_path_or_url_boundary：那会误杀 AWS Secret Access Key
                // 这类合法但含 base64 `/` 的密钥
                if looks_like_url_match(cand)
                    || is_template_var(cand)
                    || is_hex_hash(cand)
                    || is_uuid(cand)
                    || is_business_id_assignment(cand)
                    || is_likely_placeholder(cand)
                    || cand.contains(',')
                {
                    continue;
                }
                push(&mut spans, s, e);
            }
        }

        // Route 2: 上下文口令，只脱 value（第 2 个分组）
        for caps in RE_CONTEXT_SECRET.captures_iter(text) {
            let Some(m) = caps.get(2) else { continue };
            let value = m.as_str();
            if is_template_var(value) {
                continue;
            }
            // 低熵短串（"REPLACE_ME" / "null" / "abc" 等占位符）跳过
            if value.len() <= 16 && shannon_entropy(value) < 3.0 {
                continue;
            }
            push(&mut spans, m.start(), m.end());
        }

        // Route 3: 高熵兜底
        for m in RE_ENTROPY_TOKEN.find_iter(text) {
            let (s, e) = (m.start(), m.end());
            let cand = m.as_str();
            let strong = has_strong_secret_context(text, s, e);
            // 强上下文（Bearer / token= 等）凌驾于路径检查，避免 Bearer abc/xyz== 被误放
            if !strong && is_on_path_or_url_boundary(text, s, e) {
                continue;
            }
            if is_template_var(cand)
                || is_hex_hash(cand)
                || is_uuid(cand)
                || is_business_id_assignment(cand)
            {
                continue;
            }
            let threshold = if has_secret_context(text, s, e) {
                ENTROPY_MIN
            } else {
                ENTROPY_MIN_STRICT
            };
            if shannon_entropy(cand) >= threshold {
                push(&mut spans, s, e);
            }
        }
        spans
    }
}

/// 回溯起点：往前退 `n` 字节后再前移到字符边界，保证切片合法
fn lookback_start(text: &str, start: usize, n: usize) -> usize {
    let mut lo = start.saturating_sub(n);
    while !text.is_char_boundary(lo) {
        lo += 1;
    }
    lo
}

/// 命中含 URL，或以 host.tld: 开头
fn looks_like_url_match(s: &str) -> bool {
    s.contains("://") || RE_HOST_PORT_PREFIX.is_match(s)
}

fn is_likely_placeholder(s: &str) -> bool {
    let upper = s.to_uppercase();
    COMMON_PLACEHOLDERS.iter().any(|p| upper.contains(p))
}

/// [start, end) 是否处于路径 / URL / 哈希等「非密钥」上下文
fn is_on_path_or_url_boundary(text: &str, start: usize, end: usize) -> bool {
    let b = text.as_bytes();
    if text[start..end].contains(PATH_INTERNAL_CHARS) {
        return true;
    }
    if start > 0 && PATH_BOUNDARY_CHARS.contains(&b[start - 1]) {
        return true;
    }
    if end < b.len() && PATH_BOUNDARY_CHARS.contains(&b[end]) {
        return true;
    }
    let look = &text[lookback_start(text, start, 8)..start];
    URL_PREFIXES.iter().any(|p| look.contains(p))
}

/// 回溯区间内是否出现密钥语义关键词
fn has_secret_context(text: &str, start: usize, end: usize) -> bool {
    RE_SECRET_CONTEXT.is_match(&text[lookback_start(text, start, CONTEXT_LOOKBACK)..end])
}

/// 比 has_secret_context 更严：关键词紧贴候选串，中间只能是赋值分隔字符。
/// 命中时高熵兜底绕过路径检查，避免漏掉 Bearer xxx==/yyy 这类带 / 的真密钥。
fn has_strong_secret_context(text: &str, start: usize, end: usize) -> bool {
    let lo = lookback_start(text, start, CONTEXT_LOOKBACK);
    if RE_AUTH_HEADER_PREFIX.is_match(&text[lo..start]) {
        return true;
    }
    let region = &text[lo..end];
    let Some(last) = RE_SECRET_CONTEXT.find_iter(region).last() else {
        return false;
    };
    let cand_start = start - lo;
    // 关键词本身就在候选串里（如 token=xxx 整段匹配）→ 强
    if last.start() >= cand_start {
        return true;
    }
    region.as_bytes()[last.end()..cand_start]
        .iter()
        .all(|c| ASSIGNMENT_CHARS.contains(c))
}

fn is_template_var(s: &str) -> bool {
    RE_TEMPLATE_VAR.is_match(s)
}

/// md5(32) / sha1(40) / sha256(64) 长度的纯 hex 串
fn is_hex_hash(s: &str) -> bool {
    matches!(s.len(), 32 | 40 | 64) && s.bytes().all(|c| c.is_ascii_hexdigit())
}

fn is_uuid(s: &str) -> bool {
    RE_UUID.is_match(s)
}

/// `=` 左边的变量名以业务 ID 后缀结尾（_id / _uuid / _no ...）。
/// 变量名同时含凭证语义词时仍按密钥处理（如 AWS_ACCESS_KEY_ID）。
fn is_business_id_assignment(s: &str) -> bool {
    let Some(eq) = s.find('=') else {
        return false;
    };
    if eq == 0 {
        return false;
    }
    let name = s[..eq].to_lowercase();
    if ["key", "secret", "token", "auth", "password", "credential"]
        .iter()
        .any(|k| name.contains(k))
    {
        return false;
    }
    BENIGN_ID_SUFFIXES.iter().any(|suf| name.ends_with(suf))
}

/// 按字节计算香农熵（bits/byte）
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    for &c in s.as_bytes() {
        freq[c as usize] += 1;
    }
    let n = s.len() as f64;
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = f64::from(c) / n;
            -p * p.log2()
        })
        .sum()
}
