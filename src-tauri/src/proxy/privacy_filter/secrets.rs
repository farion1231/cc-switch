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
pub(super) const CONTEXT_LOOKBACK: usize = 30;

/// 上下文型口令：藏在句子里的密码/token，如「我的密码是 hunter2」「api_key: xxx」
static RE_CONTEXT_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(密码|口令|密钥|password|passwd|pwd|secret|token|api[_\s-]?key)\s*['"]?\s*(?:是|为|:|：|=)\s*['"]?([^\s'"，。；;]{4,})"#,
    )
    .unwrap()
});

/// 高熵兜底：赋值表达式只捕获 value，避免将字段名一起替换。
static RE_ENTROPY_TOKEN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:[A-Za-z_][A-Za-z0-9_-]*=)?([A-Za-z0-9+/_\-]{20,}={0,3})").unwrap()
});

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

/// 模板变量：{{ X }} / ${X} / %{X} / <X> / $X / %X%
static RE_TEMPLATE_VAR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:\{\{[^{}]+\}\}|\$\{[^{}]+\}|%\{[^{}]+\}|<[^<>]+>|\$[A-Za-z_][A-Za-z0-9_]*|%[A-Za-z_][A-Za-z0-9_]*%)$",
    )
    .unwrap()
});
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

/// 上下文口令 value 的占位词（去掉两端标点后大小写不敏感的整词匹配）：空值、示例值，
/// 以及 `password: string` 这类类型标注。靠词表而不是熵，否则 hunter2 这样的短密码会被放行。
const CONTEXT_VALUE_PLACEHOLDERS: &[&str] = &[
    // 空值
    "null",
    "nil",
    "none",
    "undefined",
    "nan",
    "empty",
    "blank",
    "true",
    "false",
    // 示例值与字段说明
    "xxx",
    "test",
    "example",
    "sample",
    "dummy",
    "fake",
    "mock",
    "changeme",
    "redacted",
    "masked",
    "hidden",
    "required",
    "optional",
    "default",
    "unknown",
    "value",
    // 值就是字段名
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "key",
    "apikey",
    "api_key",
    "api-key",
    // 类型名
    "str",
    "string",
    "secretstr",
    "text",
    "varchar",
    "char",
    "bool",
    "boolean",
    "int",
    "integer",
    "uint",
    "i32",
    "i64",
    "u32",
    "u64",
    "long",
    "float",
    "double",
    "number",
    "decimal",
    "bigint",
    "byte",
    "bytes",
    "object",
    "array",
    "list",
    "dict",
    "map",
    "any",
    "void",
    "json",
];

/// 打码字符：value 全由这些字符组成时（`****` / `...`）不是真实口令
const MASK_CHARS: &[char] = &['*', '•', '·', '.', '?', '-', '_', ','];

struct SecretRule {
    re: Regex,
    /// 已小写；空表示该规则总是参与
    keywords: Vec<String>,
    entropy: f64,
    secret_group: usize,
    is_generic: bool,
    whole_secret: bool,
    allowlists: Vec<RuleAllowlist>,
}

struct RuleAllowlist {
    regexes: Vec<Regex>,
    regex_target: String,
    stopwords: Vec<String>,
    match_all: bool,
}

impl RuleAllowlist {
    fn from_toml(allowlist: TomlAllowlist) -> Option<Self> {
        let match_all = allowlist.condition.eq_ignore_ascii_case("AND");
        // 请求文本没有文件路径；要求路径同时命中的白名单不能放行。
        if match_all && !allowlist.paths.is_empty() {
            return None;
        }
        let regexes = allowlist
            .regexes
            .iter()
            .filter_map(|pattern| match Regex::new(pattern) {
                Ok(re) => Some(re),
                Err(e) => {
                    log::warn!("[PrivacyFilter] Skipped incompatible allowlist regex: {e}");
                    None
                }
            })
            .collect();
        Some(Self {
            regexes,
            regex_target: allowlist.regex_target,
            stopwords: allowlist
                .stopwords
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
            match_all,
        })
    }

    fn allows(&self, text: &str, whole: regex::Match<'_>, secret: regex::Match<'_>) -> bool {
        let target = match self.regex_target.as_str() {
            "match" => whole.as_str(),
            "line" => {
                let start = text[..secret.start()].rfind('\n').map_or(0, |i| i + 1);
                let end = text[secret.end()..]
                    .find('\n')
                    .map_or(text.len(), |i| secret.end() + i);
                &text[start..end]
            }
            _ => secret.as_str(),
        };
        let regex_match = self.regexes.iter().any(|re| re.is_match(target));
        let stopword_match = if self.stopwords.is_empty() {
            false
        } else {
            let lower = secret.as_str().to_lowercase();
            self.stopwords
                .iter()
                .any(|word| lower.contains(word.as_str()))
        };
        if self.match_all {
            (!self.regexes.is_empty() || !self.stopwords.is_empty())
                && (self.regexes.is_empty() || regex_match)
                && (self.stopwords.is_empty() || stopword_match)
        } else {
            regex_match || stopword_match
        }
    }
}

pub(super) struct SecretDetector {
    rules: Vec<SecretRule>,
    /// 因正则语法不兼容被跳过的规则数
    skipped: usize,
}

/// gitleaks.toml 的文本检测配置；不使用文件路径等扫描器专用字段。
#[derive(Deserialize)]
struct TomlConfig {
    #[serde(default)]
    rules: Vec<TomlRule>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TomlRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    regex: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    entropy: f64,
    #[serde(default)]
    secret_group: usize,
    #[serde(default)]
    allowlists: Vec<TomlAllowlist>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TomlAllowlist {
    regexes: Vec<String>,
    regex_target: String,
    stopwords: Vec<String>,
    condition: String,
    paths: Vec<String>,
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
                        is_generic: r.id == "generic-api-key",
                        // 这两条规则的捕获组只是格式分段，整段才是凭证。
                        whole_secret: matches!(
                            r.id.as_str(),
                            "microsoft-teams-webhook" | "jwt-base64"
                        ),
                        allowlists: r
                            .allowlists
                            .into_iter()
                            .filter_map(RuleAllowlist::from_toml)
                            .collect(),
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
                is_generic: false,
                whole_secret: false,
                allowlists: Vec::new(),
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
                let m = if r.whole_secret {
                    whole
                } else if r.secret_group > 0 {
                    let Some(secret) = caps.get(r.secret_group) else {
                        continue;
                    };
                    secret
                } else {
                    caps.iter()
                        .skip(1)
                        .flatten()
                        .find(|m| !m.is_empty())
                        .unwrap_or(whole)
                };
                let (s, e) = (m.start(), m.end());
                if s >= e {
                    continue;
                }
                let cand = m.as_str();
                if r.allowlists.iter().any(|a| a.allows(text, whole, m)) {
                    continue;
                }
                if r.entropy > 0.0 && shannon_entropy(cand) < r.entropy {
                    continue; // 复刻 gitleaks 的熵阈值
                }
                // 专用规则已识别凭证格式，不能再把 webhook URL、hex 或 UUID 密钥当误报。
                if (r.is_generic
                    && (looks_like_url_match(whole.as_str())
                        || is_hex_hash(cand)
                        || is_uuid(cand)
                        || is_business_id_assignment(whole.as_str())
                        || cand.contains(',')))
                    || is_template_var(cand)
                    || is_likely_placeholder(cand)
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
            if is_template_var(value) || is_context_placeholder(value) {
                continue;
            }
            push(&mut spans, m.start(), m.end());
        }

        // Route 3: 高熵兜底
        for caps in RE_ENTROPY_TOKEN.captures_iter(text) {
            let whole = caps.get(0).unwrap();
            let m = caps.get(1).unwrap();
            let (s, e) = (m.start(), m.end());
            let cand = m.as_str();
            let strong = has_strong_secret_context(text, s, e);
            // 强上下文（Bearer / token= 等）凌驾于路径检查，避免 Bearer abc/xyz== 被误放
            // 边界从 name= 之前检查，避免将赋值号当成路径；替换范围仍只包含 value。
            if !strong && is_on_path_or_url_boundary(text, whole.start(), e) {
                continue;
            }
            if is_template_var(cand)
                || is_hex_hash(cand)
                || is_uuid(cand)
                || is_business_id_assignment(whole.as_str())
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
pub(super) fn lookback_start(text: &str, start: usize, n: usize) -> usize {
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

/// 上下文口令的 value 是否只是占位。先去掉两端标点，让 `string,` / `&str` / `string[]` 也能对上词表。
fn is_context_placeholder(value: &str) -> bool {
    let word = value.trim_matches(|c: char| !c.is_alphanumeric());
    if word.is_empty() {
        return value.chars().all(|c| MASK_CHARS.contains(&c));
    }
    CONTEXT_VALUE_PLACEHOLDERS
        .iter()
        .any(|p| word.eq_ignore_ascii_case(p))
        || is_likely_placeholder(value)
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
    // 关键词与候选串有交叠：整个落在候选串里（token=xxx），或跨过候选串起点（api key=xxx）→ 强
    if last.end() > cand_start {
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
