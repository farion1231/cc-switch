//! 隐私替换内置插件（`builtin:privacy-replace`）
//!
//! 在本地代理路由层实现敏感信息"替换→还原"闭环：
//! - PreRequest：把白名单字符串里的敏感信息替换为 `⟦PII|<id>|<label>|<desc>⟧` 标记，
//!   并按需注入标记协议说明（prompt_note）；
//! - PostResponse：按持久化映射把标记还原为原文；
//! - SseChunk：流式 SSE 逐事件还原，跨 delta 的半截标记用 per-stream carry 缓冲续接；
//! - 十六进制转储防护（xxd / hexdump -C）：hex 列是原文的另一编码，文本正则不可见，
//!   引擎重建转储字节流复用规则检测，命中值在 hex 列（xx）与 ASCII 列（.）同时抹除，
//!   抹除不可还原、不产生标记映射。
//!
//! 检测来源（与外部 Python 参考实现同语义，无检测模型后端）：
//! - 正则/字面量规则（`rules.json` 文档，支持 capture 捕获组只替换值）；
//! - 用户自定义特殊值（`custom-values.json` 文档，登记即预注册映射，priority 缺省 1）。
//!
//! 配置存储：三份配置文档（`json/config.json` / `json/rules.json` /
//! `json/custom-values.json`，键为虚拟文件名）持久化在 settings 表
//! [`CONFIG_STORE_KEY`]；面板「设置」经 [`ProxyPlugin::config_read`] /
//! [`ProxyPlugin::config_write`] 读写，先整批校验（Rust 版 `_validate_*`）再落库，
//! 保存即重建内存快照生效。映射表持久化在 SQLite `privacy_mapping` 表（明文存原文，
//! 安全边界：数据库文件需要像密码一样妥善保护）。
//!
//! 构成：
//! - `codec`：标记编解码（id = 所选散列算法的十六进制前缀，长度 adaptive/fixed 可配，
//!   已有映射的 id 永不改变——稳定性优先）；
//! - `store`：id → 原文映射（内存 HashMap + SQLite 持久化）；
//! - `cache`：免重复解析缓存，key = (配置指纹, 原串内容 hash)，精确匹配不做归一化；
//! - `walk`：JSON 白名单走查（key 白名单 + 数据对象深走，跳过保护 key）；
//! - `hexdump`：xxd / hexdump -C 列视图的重建检测与抹除。
//!
//! 任何内部错误均 log::warn 后返回 Ok(false)（fail-open），绝不向管线外抛错。

use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use blake2::Blake2b512;
use fancy_regex::Regex;
use once_cell::sync::Lazy;
use serde_json::{json, Map, Value};
use sha2::Digest;

use super::types::{
    ConfigColumn, ConfigColumnType, ConfigFieldType, ConfigSchemaItem, PluginError,
    PluginRequestContext, PluginStage,
};
use super::ProxyPlugin;
use crate::database::Database;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 标记前缀（U+27E6 ⟦）
const MARKER_PREFIX: &str = "⟦PII|";
/// 标记后缀（U+27E7 ⟧）
const MARKER_SUFFIX: &str = "⟧";
/// id 最短长度（十六进制位数，adaptive 模式下限）
const ID_MIN_LEN: usize = 12;
/// id 最长长度（blake2b512 摘要十六进制全文长度为 128，截断到 64）
const ID_MAX_LEN: usize = 64;
/// 前缀碰撞时 id 长度步进
const ID_COLLISION_STEP: usize = 4;
/// 替换缓存容量缺省值：写满清一半（config.cache_capacity 可配，下限 16）
const DEFAULT_CACHE_CAPACITY: usize = 4096;
/// 映射表启动载入上限，超出淘汰最旧记录
const MAX_MAPPING_ROWS: usize = 100_000;
/// 插件 id
pub(crate) const PRIVACY_PLUGIN_ID: &str = "builtin:privacy-replace";

/// 配置文档在 settings 表中的存储键（单键存三份文档的映射）
const CONFIG_STORE_KEY: &str = "builtin_privacy_config";

/// 虚拟配置文件名（config 协议的文档键；与外部 Python 参考实现保持同形，
/// 面板 schema 的 file 字段引用这些键）
const CONFIG_DOC_FILE: &str = "json/config.json";
const RULES_DOC_FILE: &str = "json/rules.json";
const CUSTOM_DOC_FILE: &str = "json/custom-values.json";
/// config 协议支持的全部文档键
const CONFIG_DOC_FILES: [&str; 3] = [CONFIG_DOC_FILE, RULES_DOC_FILE, CUSTOM_DOC_FILE];

/// 支持的散列算法（config.hash.algorithm；只影响新登记的映射）
const HASH_ALGORITHMS: [&str; 4] = ["blake2b", "sha256", "sha512", "sha1"];

/// 替换方向（PreRequest）作用的 key：文本叶子 key ∪ 数据对象 key
/// （数据对象内部的**所有**字符串叶子都会被走查）
const REPLACE_TARGET_KEYS: &[&str] = &[
    "text",
    "partial_json",
    "input_text",
    "output_text",
    "instructions",
    // Claude 字符串形态 system（blocks 形态由 block.text 覆盖）
    "system",
    // 数据对象（tool_use 参数 / tool_result 内容）
    "input",
    "args",
    "content",
];

/// 还原方向（PostResponse / SseChunk）作用的 key
const RESTORE_TARGET_KEYS: &[&str] = &["text", "partial_json", "input"];

/// 保护 key：任何方向、任何深度都跳过（thinking 签名不可改动，否则上游校验失败）
const PROTECTED_KEYS: &[&str] = &[
    "signature",
    "thoughtSignature",
    "thinking",
    "type",
    "id",
    "role",
    "name",
    "model",
    "stop_reason",
];

/// 注入到系统提示的标记格式说明（含十六进制转储遮蔽说明第 6/7 条）。
/// 注入位置在规则走查之后，本段文本自身不参与替换；每轮恒定注入保持
/// 上游 prompt 缓存（cache_control）稳定。
const MARKER_PROMPT_NOTE: &str = "[隐私标记协议 / Privacy marker protocol]\n\
上下文中形如 ⟦PII|<id>|<label>|<描述>⟧ 的字符串是隐私替换标记，\
代表一个已被脱敏的真实值（路径、密钥、账号、人名等）。约定：\n\
1. 这是合法且预期的格式，不是乱码或语法错误；不要质疑、修复或重写标记本身。\n\
2. 把标记当作它所代表的真实值直接使用：写文件、执行命令、拼接路径、回复引用时\
原样完整保留；环境会在你的输出到达用户与工具之前自动还原为真实值。\n\
3. 不要尝试解码、猜测或推断标记对应的原值；同一原文的标记 id 恒定，\
可据此保持跨轮引用一致。\n\
4. 构造 shell 命令或 JSON 参数时，若标记代表路径，优先使用正斜杠或环境变量\
（如 $USERPROFILE/$HOME）拼接，避免手写反斜杠路径片段；JSON 字符串中的反斜杠\
必须按 JSON 规则转义。\n\
5. 向用户复述相关内容时同样保留标记（还原对用户透明）；报告需要标记本身时，\
不要输出完整 id，例如你可以只给出 id 前 6 位缩写。\n\
6. 工具输出的十六进制转储（xxd/hexdump）中，hex 列成片的 xx 与 ASCII 列成片的 . \
是隐私层对敏感字节的遮蔽：不代表文件本身如此，不要猜测或尝试还原被抹内容；\
未抹除的偏移与字节是精确的，可放心用于结构分析与按 offset 定点写入；\
不要把脱敏转储整段回写为文件。\n\
7. 查看文本内容请优先用文本方式读取；同一文件以十六进制方式读取时，\
其中敏感内容会呈现为 xx/. 而非本标记。确实需要某个被抹的值参与任务时，\
请让用户以文本方式提供，不要尝试绕过遮蔽。";

/// 面板展示的插件描述（含与外部 Python 版并存的提醒）
const PRIVACY_DESCRIPTION: &str = "把请求中的敏感信息替换为 ⟦PII|…⟧ 标记，流式与非流式响应中自动还原为原文；内置 Rust 引擎，零外部运行时依赖。与外部 Python 版隐私插件并存时请只启用其一，避免重复替换导致映射分家";

// ---------------------------------------------------------------------------
// 配置文档（serde_json Value 形态 + 校验器）
// ---------------------------------------------------------------------------

/// Python 真值判断（bool() 语义）：用于把面板传入的非布尔值归一化为开关
fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// 可选真值：key 存在时按真值归一化，缺省时用 fallback（Python `doc.get(key, default)` 语义）
fn truthy_or(v: Option<&Value>, fallback: bool) -> bool {
    v.map(truthy).unwrap_or(fallback)
}

/// JSON 值取整（Python `int(value) if isinstance(value, (int, float))` 语义：
/// 浮点截断；其它类型返回 None）
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

/// 可选取整：非法/缺失时用 fallback
fn as_int_or(v: Option<&Value>, fallback: i64) -> i64 {
    v.and_then(as_int).unwrap_or(fallback)
}

/// 校验并原地归一化 config 文档（非法配置返回 Err，消息展示在面板弹窗）。
/// 未列出的字段（如外部插件遗留的 detectors）原样保留。
fn validate_config_doc(doc: &mut Value) -> Result<(), String> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| "config.json 必须是 JSON 对象".to_string())?;
    for key in [
        "enable_regex",
        "enable_detectors",
        "prompt_note",
        "enable_hexdump_guard",
    ] {
        if let Some(v) = obj.get_mut(key) {
            *v = Value::Bool(truthy(v));
        }
    }
    let capacity = as_int_or(obj.get("cache_capacity"), DEFAULT_CACHE_CAPACITY as i64).max(16);
    obj.insert("cache_capacity".to_string(), Value::Number(capacity.into()));
    if let Some(hash) = obj.get_mut("hash").and_then(Value::as_object_mut) {
        let algorithm = hash
            .get("algorithm")
            .and_then(Value::as_str)
            .unwrap_or("blake2b");
        if !HASH_ALGORITHMS.contains(&algorithm) {
            return Err(format!(
                "hash.algorithm 只能是 {}",
                HASH_ALGORITHMS.join(", ")
            ));
        }
        let mode = hash
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("adaptive");
        if mode != "adaptive" && mode != "fixed" {
            return Err("hash.mode 只能是 adaptive 或 fixed".to_string());
        }
        let length = as_int_or(hash.get("length"), 16).clamp(4, 64);
        hash.insert("length".to_string(), Value::Number(length.into()));
    }
    Ok(())
}

/// 校验并原地归一化 rules 文档；literal 规则的逗号分隔串自动转数组（面板 textarea 形态）
fn validate_rules_doc(doc: &mut Value) -> Result<(), String> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| "rules.json 必须是 JSON 对象".to_string())?;
    let enabled = truthy_or(obj.get("enabled"), true);
    obj.insert("enabled".to_string(), Value::Bool(enabled));
    let Some(rules) = obj.get_mut("rules").and_then(Value::as_array_mut) else {
        return Err("rules 必须是数组".to_string());
    };
    for (i, rule) in rules.iter_mut().enumerate() {
        let obj = rule
            .as_object_mut()
            .ok_or_else(|| format!("rules[{i}] 必须是对象"))?;
        let kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind != "regex" && kind != "literal" {
            return Err(format!(
                "rules[{i}].kind 只能是 regex 或 literal（新增行请先选类型）"
            ));
        }
        if kind == "literal" {
            // 面板 textarea 形态：逗号分隔串（兼容全角逗号）→ 数组
            if let Some(Value::String(s)) = obj.get("values") {
                let arr: Vec<Value> = s
                    .split([',', '，'])
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| Value::String(v.to_string()))
                    .collect();
                obj.insert("values".to_string(), Value::Array(arr));
            }
            let ok = match obj.get("values") {
                Some(Value::Array(arr)) => arr
                    .iter()
                    .all(|v| matches!(v, Value::String(s) if !s.is_empty())),
                _ => false,
            };
            if !ok {
                return Err(format!(
                    "rules[{i}].values 必须是非空字符串数组（面板中用逗号分隔）"
                ));
            }
            obj.remove("pattern");
        } else {
            let pattern = obj
                .get("pattern")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if pattern.is_empty() {
                return Err(format!("rules[{i}].pattern 不能为空"));
            }
            if let Err(e) = Regex::new(&adapt_python_pattern(pattern)) {
                return Err(format!("rules[{i}].pattern 正则无效: {e}"));
            }
            // regex 规则不使用 values（面板误填时静默丢弃，与参考实现一致）
            obj.remove("values");
        }
        if let Some(capture) = obj.get("capture") {
            let valid = matches!(capture, Value::Null | Value::String(_))
                || matches!(capture, Value::Number(n) if n.is_i64() || n.is_u64());
            if !valid {
                return Err(format!("rules[{i}].capture 必须是组号(数字)或组名(字符串)"));
            }
        }
        let priority = as_int_or(obj.get("priority"), 20);
        obj.insert("priority".to_string(), Value::Number(priority.into()));
    }
    Ok(())
}

/// 校验并原地归一化 custom-values 文档
fn validate_custom_doc(doc: &mut Value) -> Result<(), String> {
    let obj = doc
        .as_object_mut()
        .ok_or_else(|| "custom-values.json 必须是 JSON 对象".to_string())?;
    let enabled = truthy_or(obj.get("enabled"), true);
    obj.insert("enabled".to_string(), Value::Bool(enabled));
    let Some(values) = obj.get_mut("values").and_then(Value::as_array_mut) else {
        return Err("values 必须是数组".to_string());
    };
    for (i, row) in values.iter_mut().enumerate() {
        let obj = row
            .as_object_mut()
            .ok_or_else(|| format!("values[{i}] 必须是对象"))?;
        let value_ok = obj
            .get("value")
            .and_then(Value::as_str)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if !value_ok {
            return Err(format!("values[{i}].value 不能为空"));
        }
        let priority = as_int_or(obj.get("priority"), 1);
        obj.insert("priority".to_string(), Value::Number(priority.into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 散列（config.hash，只影响新登记的映射；已有映射 id 永不改变）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashAlgorithm {
    Blake2b,
    Sha256,
    Sha512,
    Sha1,
}

impl HashAlgorithm {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "blake2b" => Some(Self::Blake2b),
            "sha256" => Some(Self::Sha256),
            "sha512" => Some(Self::Sha512),
            "sha1" => Some(Self::Sha1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashMode {
    /// id 长度随原文：min(max(12, 原文字节数), 64)
    Adaptive,
    /// 固定 hash.length（防长度泄漏）
    Fixed,
}

/// 散列参数快照（来自 config.hash；缺省 blake2b / adaptive / 16）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HashConfig {
    algorithm: HashAlgorithm,
    mode: HashMode,
    length: usize,
}

impl Default for HashConfig {
    fn default() -> Self {
        Self {
            algorithm: HashAlgorithm::Blake2b,
            mode: HashMode::Adaptive,
            length: 16,
        }
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// 按所选算法计算十六进制摘要
fn digest_hex(algorithm: HashAlgorithm, data: &[u8]) -> String {
    match algorithm {
        HashAlgorithm::Blake2b => to_hex(&Blake2b512::digest(data)),
        HashAlgorithm::Sha256 => to_hex(&sha2::Sha256::digest(data)),
        HashAlgorithm::Sha512 => to_hex(&sha2::Sha512::digest(data)),
        HashAlgorithm::Sha1 => to_hex(&sha1::Sha1::digest(data)),
    }
}

/// blake2b512 摘要的十六进制小写编码（指纹 / 缓存键用，与标记 id 散列无关）
fn blake2b_hex(data: &[u8]) -> String {
    to_hex(&Blake2b512::digest(data))
}

// ---------------------------------------------------------------------------
// 标记编解码
// ---------------------------------------------------------------------------

/// 拼装标记：`⟦PII|<id>|<label>|<desc>⟧`
fn format_marker(id: &str, label: &str, desc: &str) -> String {
    format!("{MARKER_PREFIX}{id}|{label}|{desc}{MARKER_SUFFIX}")
}

/// 扫描文本中的 `⟦PII|<id>|…⟧` 标记，返回 (起始字节, 结束字节, id)。
/// 标记内部形如 "<id>|<label>|<desc>"，id 不含 '|'，还原只需 id。
fn scan_markers(text: &str) -> Vec<(usize, usize, &str)> {
    let mut result = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(MARKER_PREFIX) {
        let start = from + rel;
        let id_start = start + MARKER_PREFIX.len();
        let Some(rest) = text[id_start..].find(MARKER_SUFFIX) else {
            break;
        };
        let end = id_start + rest + MARKER_SUFFIX.len();
        let inner = &text[id_start..id_start + rest];
        if let Some(sep) = inner.find('|') {
            let id = &inner[..sep];
            if !id.is_empty() {
                result.push((start, end, id));
            }
        }
        from = end;
    }
    result
}

/// 把文本中 id 已知的标记还原为原文；未知 id 原样保留。
/// 返回 None 表示没有发生任何替换。
fn restore_markers(text: &str, lookup: &mut dyn FnMut(&str) -> Option<String>) -> Option<String> {
    if !text.contains(MARKER_PREFIX) {
        return None;
    }
    let markers = scan_markers(text);
    if markers.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut replaced = false;
    for (start, end, id) in markers {
        if let Some(original) = lookup(id) {
            out.push_str(&text[last..start]);
            out.push_str(&original);
            last = end;
            replaced = true;
        }
    }
    if !replaced {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

// ---------------------------------------------------------------------------
// 映射存储
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct MappingRecord {
    original: String,
}

/// id → 原文映射（内存态；label 只在落库时随 INSERT 写入，还原仅需 original）
#[derive(Debug, Default)]
struct MappingStore {
    by_id: HashMap<String, MappingRecord>,
    /// 原文 → id 的反查索引（同一原文永远映射到同一 id）
    by_original: HashMap<String, String>,
}

impl MappingStore {
    /// 登记一个原文：已存在则复用既有 id（label 以首条记录为准），返回 (id, 是否为新增)
    fn insert(&mut self, original: &str, hash: &HashConfig) -> (String, bool) {
        if let Some(id) = self.by_original.get(original) {
            return (id.clone(), false);
        }
        let id = self.resolve_id(original, hash);
        self.by_id.insert(
            id.clone(),
            MappingRecord {
                original: original.to_string(),
            },
        );
        self.by_original.insert(original.to_string(), id.clone());
        (id, true)
    }

    /// 计算未被占用的 id：所选散列算法的十六进制前缀。
    /// 长度策略：adaptive = min(max(12, 原文字节数), 64)；fixed = 固定 hash.length
    /// （防长度泄漏，下限 4）。被不同原文占用则长度 +4 重算；摘要全长仍碰撞时
    /// 追加 `-<序号>` 兜底（sha1 摘要仅 40 位，长度以摘要实际长度封顶）。
    fn resolve_id(&self, original: &str, hash: &HashConfig) -> String {
        let digest = digest_hex(hash.algorithm, original.as_bytes());
        let mut len = match hash.mode {
            HashMode::Fixed => hash.length.clamp(4, ID_MAX_LEN),
            HashMode::Adaptive => original.len().clamp(ID_MIN_LEN, ID_MAX_LEN),
        }
        .min(digest.len());
        loop {
            let candidate = &digest[..len];
            match self.by_id.get(candidate) {
                Some(record) if record.original == original => return candidate.to_string(),
                Some(_) if len < digest.len() && len < ID_MAX_LEN => {
                    len = (len + ID_COLLISION_STEP).min(digest.len()).min(ID_MAX_LEN);
                }
                Some(_) => {
                    let mut suffix = 0u32;
                    loop {
                        let id = format!("{digest}-{suffix}");
                        match self.by_id.get(&id) {
                            Some(record) if record.original == original => return id,
                            Some(_) => suffix += 1,
                            None => return id,
                        }
                    }
                }
                None => return candidate.to_string(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 免重复解析缓存
// ---------------------------------------------------------------------------

/// 替换结果缓存：key = (配置指纹, 原串内容 hash)，value = 替换后字符串。
/// 精确内容 hash，不做归一化；容量可配（config.cache_capacity），写满清一半（FIFO）。
#[derive(Debug, Default)]
struct ReplaceCache {
    map: HashMap<String, String>,
    order: VecDeque<String>,
}

impl ReplaceCache {
    fn get(&self, key: &str) -> Option<&String> {
        self.map.get(key)
    }

    fn put(&mut self, key: String, value: String, capacity: usize) {
        if self.map.contains_key(&key) {
            return;
        }
        let capacity = capacity.max(16);
        if self.map.len() >= capacity {
            let evict = capacity / 2;
            for _ in 0..evict {
                if let Some(oldest) = self.order.pop_front() {
                    self.map.remove(&oldest);
                }
            }
        }
        self.map.insert(key.clone(), value);
        self.order.push_back(key);
    }
}

// ---------------------------------------------------------------------------
// 规则（rules 文档；支持 capture 捕获组只替换值）
// ---------------------------------------------------------------------------

/// capture 规格（组号或组名；Python `capture.isdigit()` 字符串归一化为组号）
#[derive(Debug, Clone, PartialEq, Eq)]
enum CaptureSpec {
    None,
    Index(usize),
    Name(String),
}

fn parse_capture(v: &Value) -> Option<CaptureSpec> {
    match v {
        Value::Null => None,
        Value::Number(n) => n.as_u64().map(|n| CaptureSpec::Index(n as usize)),
        Value::String(s) => {
            if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
                s.parse::<usize>().ok().map(CaptureSpec::Index)
            } else {
                Some(CaptureSpec::Name(s.clone()))
            }
        }
        _ => None,
    }
}

/// 编译后的规则匹配器
#[derive(Debug)]
enum RuleMatcher {
    Regex(Regex),
    Literal(Vec<String>),
}

/// 编译后的规则（内存快照）
#[derive(Debug)]
struct CompiledRule {
    matcher: RuleMatcher,
    capture: CaptureSpec,
    label: String,
    desc: String,
    priority: i32,
}

/// 把 Python 风格 pattern 适配为 fancy-regex 语法：
/// 命名组反引用 `(?P=name)` → `\k<name>`；其余（`(?i)`、`(?P<name>…)`、
/// 前后查找断言等）fancy-regex 原生兼容。
fn adapt_python_pattern(pattern: &str) -> String {
    if !pattern.contains("(?P=") {
        return pattern.to_string();
    }
    let mut out = String::with_capacity(pattern.len());
    let mut i = 0usize;
    while i < pattern.len() {
        if pattern[i..].starts_with("(?P=") {
            if let Some(end_rel) = pattern[i + 4..].find(')') {
                let name = &pattern[i + 4..i + 4 + end_rel];
                out.push_str("\\k<");
                out.push_str(name);
                out.push('>');
                i += 4 + end_rel + 1;
                continue;
            }
        }
        let ch_len = pattern[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&pattern[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// capture 组在 pattern 中是否存在（编译期校验；无效 capture 整条规则跳过）
fn capture_exists(re: &Regex, capture: &CaptureSpec) -> bool {
    match capture {
        CaptureSpec::None => true,
        CaptureSpec::Index(i) => *i < re.captures_len(),
        CaptureSpec::Name(name) => re.capture_names().flatten().any(|n| n == name),
    }
}

/// 编译规则文档（非法 pattern 跳过该条，不影响其他规则；文档总开关关闭返回空集）
fn compile_rules(rules_doc: &Value) -> Vec<CompiledRule> {
    let Some(obj) = rules_doc.as_object() else {
        return Vec::new();
    };
    if !truthy_or(obj.get("enabled"), true) {
        return Vec::new();
    }
    let Some(specs) = obj.get("rules").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for spec in specs {
        let Some(spec) = spec.as_object() else {
            continue;
        };
        if !truthy_or(spec.get("enabled"), true) {
            continue;
        }
        let name = spec.get("name").and_then(Value::as_str).unwrap_or("");
        let label = spec
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let desc = spec
            .get("desc")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let priority = as_int_or(spec.get("priority"), 20) as i32;
        let capture = spec
            .get("capture")
            .and_then(parse_capture)
            .unwrap_or(CaptureSpec::None);
        match spec.get("kind").and_then(Value::as_str) {
            Some("regex") => {
                let Some(pattern) = spec.get("pattern").and_then(Value::as_str) else {
                    log::warn!("[PRIVACY] 规则 {name} 缺少 pattern，已跳过");
                    continue;
                };
                match Regex::new(&adapt_python_pattern(pattern)) {
                    Ok(re) => {
                        if capture != CaptureSpec::None && !capture_exists(&re, &capture) {
                            log::warn!("[PRIVACY] 规则 {name} 的 capture 无效，已跳过整条规则");
                            continue;
                        }
                        rules.push(CompiledRule {
                            matcher: RuleMatcher::Regex(re),
                            capture,
                            label,
                            desc,
                            priority,
                        });
                    }
                    Err(e) => {
                        log::warn!("[PRIVACY] 规则 {name} 的 pattern 非法，已跳过: {e}");
                    }
                }
            }
            Some("literal") => {
                let values = spec
                    .get("values")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .filter(|v| !v.is_empty())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                rules.push(CompiledRule {
                    matcher: RuleMatcher::Literal(values),
                    capture: CaptureSpec::None,
                    label,
                    desc,
                    priority,
                });
            }
            other => {
                log::warn!("[PRIVACY] 规则 {name} 的 kind 未知（{other:?}），已跳过");
            }
        }
    }
    rules
}

// ---------------------------------------------------------------------------
// 用户自定义特殊值（custom-values 文档）
// ---------------------------------------------------------------------------

/// 一条自定义特殊值（priority 缺省 1，压过常规规则——用户明确登记视为最高置信）
#[derive(Debug, Clone, PartialEq, Eq)]
struct CustomValue {
    value: String,
    label: String,
    desc: String,
    priority: i32,
}

/// 编译自定义特殊值文档
fn compile_custom_values(doc: &Value) -> Vec<CustomValue> {
    let Some(obj) = doc.as_object() else {
        return Vec::new();
    };
    if !truthy_or(obj.get("enabled"), true) {
        return Vec::new();
    }
    let Some(rows) = obj.get("values").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for row in rows {
        let Some(row) = row.as_object() else {
            continue;
        };
        if !truthy_or(row.get("enabled"), true) {
            continue;
        }
        let Some(value) = row.get("value").and_then(Value::as_str) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        values.push(CustomValue {
            value: value.to_string(),
            label: row
                .get("label")
                .and_then(Value::as_str)
                .filter(|l| !l.is_empty())
                .unwrap_or("CUSTOM")
                .to_string(),
            desc: row
                .get("desc")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            priority: as_int_or(row.get("priority"), 1) as i32,
        });
    }
    values
}

// ---------------------------------------------------------------------------
// JSON 白名单走查
// ---------------------------------------------------------------------------

/// JSON 白名单走查：`target_keys` 命中的 key 使其子树进入"深走"（该子树内所有
/// 字符串叶子都交给 f）；`PROTECTED_KEYS` 下的子树任何情况下都跳过。
/// 返回是否有修改。
fn walk_json(
    value: &mut Value,
    target_keys: &[&str],
    f: &mut dyn FnMut(&mut String) -> bool,
) -> bool {
    walk_value(value, false, target_keys, f)
}

fn walk_value(
    value: &mut Value,
    deep: bool,
    target_keys: &[&str],
    f: &mut dyn FnMut(&mut String) -> bool,
) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            for (key, child) in map.iter_mut() {
                if PROTECTED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let child_deep = deep || target_keys.contains(&key.as_str());
                changed |= walk_value(child, child_deep, target_keys, f);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= walk_value(item, deep, target_keys, f);
            }
            changed
        }
        Value::String(s) if deep => f(s),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// SSE 流式还原
// ---------------------------------------------------------------------------

/// SSE 流式还原的 per-stream 状态：每条流由 [`ProxyPlugin::new_sse_state`]
/// 创建一次，跨事件携带（SseChunk 管线保证同一流的 state 槽位复用）。
#[derive(Debug, Default)]
struct SseStreamState {
    /// carry 缓冲：key = "块标识|字段 key"，value = 扣下的可能含半截标记的文本尾巴。
    /// 块标识 = delta 类型 + index（如 `text_delta:0`，解析不出为空串），
    /// 只在非空时写入（map 为空即所有桶为空）。
    carries: HashMap<String, String>,
    /// 流已见标记标志：置位表示曾扣留过半截标记（carry 桶可能非空）。
    /// 用于事件级零开销短路：未置位且本事件 data 不含 `⟦` 时直接透传。
    seen_marker: bool,
}

/// SSE 流式还原事件名：收到时 flush 对应/全部 carry（无扣留）
fn is_sse_flush_event(event_name: Option<&str>) -> bool {
    matches!(
        event_name,
        Some("content_block_stop") | Some("message_stop") | Some("message_delta")
    )
}

/// 从 SSE data JSON 提取块标识（delta 类型 + index），解析不出返回空串。
/// 兼容 Claude（`delta.type` + `index`）与 Codex（顶层 `type` + `content_index`）两种形状。
fn sse_block_key(value: &Value) -> String {
    let kind = value
        .get("delta")
        .and_then(|d| d.get("type"))
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("");
    if kind.is_empty() {
        return String::new();
    }
    let index = value
        .get("index")
        .or_else(|| value.get("content_index"))
        .and_then(Value::as_u64);
    match index {
        Some(i) => format!("{kind}:{i}"),
        None => kind.to_string(),
    }
}

/// SSE 流式还原走查：与 [`walk_json`] 的还原方向同构（RESTORE_TARGET_KEYS 白名单 +
/// PROTECTED_KEYS 保护），但把叶子所在的 key 名交给回调，供 carry 缓冲按
/// "块标识|字段 key" 分桶（key 为叶子直属 key，数据对象深走时为叶子自己的 key）。
fn walk_sse_restore(
    value: &mut Value,
    deep: bool,
    key: &str,
    f: &mut dyn FnMut(&str, &mut String) -> bool,
) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            for (k, child) in map.iter_mut() {
                if PROTECTED_KEYS.contains(&k.as_str()) {
                    continue;
                }
                let child_deep = deep || RESTORE_TARGET_KEYS.contains(&k.as_str());
                changed |= walk_sse_restore(child, child_deep, k, f);
            }
            changed
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= walk_sse_restore(item, deep, key, f);
            }
            changed
        }
        Value::String(s) if deep => f(key, s),
        _ => false,
    }
}

/// 后缀是否可能是未写完的标记：以 `⟦PII|` 开头（标记进行中），
/// 或本身是 `⟦PII|` 的前缀（如刚收到 `⟦` / `⟦P`，下一 delta 可能续成标记）。
/// 普通包含 `⟦` 的字面文本不满足，避免长期滞留。
fn is_possible_marker_prefix(suffix: &str) -> bool {
    suffix.starts_with(MARKER_PREFIX) || MARKER_PREFIX.starts_with(suffix)
}

/// 找到需要扣留的位置：文本尾部存在未闭合的标记前缀时返回其起始字节偏移。
/// - 从最后一个 `⟦`（U+27E6）起若无 `⟧`（U+27E7），且该后缀是可能的标记前缀
///   （[`is_possible_marker_prefix`]），扣下该后缀；
/// - 尾部是 `⟦` 的 UTF-8 编码（E2 9F A6）不完整字节前缀时同样扣下
///   （防御性：上游包装器保证 data 合法 UTF-8，此分支正常不可达）。
fn withhold_incomplete_marker(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.ends_with(&[0xE2]) {
        return Some(bytes.len() - 1);
    }
    if bytes.ends_with(&[0xE2, 0x9F]) {
        return Some(bytes.len() - 2);
    }
    if let Some(pos) = text.rfind('\u{27E6}') {
        let suffix = &text[pos..];
        if !suffix.contains('\u{27E7}') && is_possible_marker_prefix(suffix) {
            return Some(pos);
        }
    }
    None
}

/// 对拼接 carry 后的文本做流式还原：先替换完整标记（id 在映射表内，未知 id 原样
/// 保留），再按需扣留尾部半截标记。flush=true 时不扣留（剩余内容全部发出）。
/// 返回 (应发出的文本, 扣下的尾巴, 是否发生了还原替换)。
fn restore_stream_text(
    combined: &str,
    flush: bool,
    lookup: &mut dyn FnMut(&str) -> Option<String>,
) -> (String, String, bool) {
    let restored = restore_markers(combined, lookup).unwrap_or_else(|| combined.to_string());
    let replaced = restored != combined;
    if flush {
        return (restored, String::new(), replaced);
    }
    match withhold_incomplete_marker(&restored) {
        Some(pos) => (
            restored[..pos].to_string(),
            restored[pos..].to_string(),
            replaced,
        ),
        None => (restored, String::new(), replaced),
    }
}

// ---------------------------------------------------------------------------
// 匹配 span 与重叠消解
// ---------------------------------------------------------------------------

/// 一个候选匹配 span（字节偏移，作用于某个白名单字符串）
#[derive(Debug, Clone, PartialEq, Eq)]
struct MatchSpan {
    start: usize,
    end: usize,
    priority: i32,
    /// 收集顺序（同级同长同 start 时先收集者胜；规则内即规则声明顺序）
    order: usize,
    /// 标记类别名
    label: String,
    /// 标记描述
    desc: String,
}

/// 收集正则规则在 text 中的匹配 span；capture 指定时只取该捕获组的范围
/// （组未参与本次匹配则跳过该次匹配）
fn collect_regex_spans(text: &str, rules: &[CompiledRule]) -> Vec<MatchSpan> {
    let mut spans = Vec::new();
    let mut order = 0usize;
    for rule in rules {
        match &rule.matcher {
            RuleMatcher::Regex(re) => {
                macro_rules! push_group_span {
                    ($group:expr) => {
                        if let Some(g) = $group {
                            let (start, end) = (g.start(), g.end());
                            if end > start {
                                spans.push(MatchSpan {
                                    start,
                                    end,
                                    priority: rule.priority,
                                    order,
                                    label: rule.label.clone(),
                                    desc: rule.desc.clone(),
                                });
                                order += 1;
                            }
                        }
                    };
                }
                match &rule.capture {
                    CaptureSpec::None => {
                        for m in re.find_iter(text) {
                            let Ok(m) = m else { continue };
                            if m.end() > m.start() {
                                spans.push(MatchSpan {
                                    start: m.start(),
                                    end: m.end(),
                                    priority: rule.priority,
                                    order,
                                    label: rule.label.clone(),
                                    desc: rule.desc.clone(),
                                });
                                order += 1;
                            }
                        }
                    }
                    CaptureSpec::Index(idx) => {
                        for caps in re.captures_iter(text) {
                            let Ok(caps) = caps else { continue };
                            push_group_span!(caps.get(*idx));
                        }
                    }
                    CaptureSpec::Name(name) => {
                        for caps in re.captures_iter(text) {
                            let Ok(caps) = caps else { continue };
                            push_group_span!(caps.name(name));
                        }
                    }
                }
            }
            RuleMatcher::Literal(values) => {
                for value in values {
                    let mut from = 0usize;
                    while let Some(pos) = text[from..].find(value.as_str()) {
                        let start = from + pos;
                        let end = start + value.len();
                        spans.push(MatchSpan {
                            start,
                            end,
                            priority: rule.priority,
                            order,
                            label: rule.label.clone(),
                            desc: rule.desc.clone(),
                        });
                        order += 1;
                        from = end;
                    }
                }
            }
        }
    }
    spans
}

/// 收集自定义特殊值在 text 中的匹配 span（与正则命中完全同路，走同一标记映射）
fn collect_custom_spans(text: &str, values: &[CustomValue]) -> Vec<MatchSpan> {
    let mut spans = Vec::new();
    let mut order = 0usize;
    for cv in values {
        let mut from = 0usize;
        while let Some(pos) = text[from..].find(cv.value.as_str()) {
            let start = from + pos;
            let end = start + cv.value.len();
            spans.push(MatchSpan {
                start,
                end,
                priority: cv.priority,
                order,
                label: cv.label.clone(),
                desc: cv.desc.clone(),
            });
            order += 1;
            from = end;
        }
    }
    spans
}

/// 重叠消解：priority 小者胜，同级长 span 胜，再按出现顺序；
/// 返回按起始位置升序、互不重叠的 span 集合
fn resolve_overlaps(mut spans: Vec<MatchSpan>) -> Vec<MatchSpan> {
    spans.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.start.cmp(&b.start))
            .then(a.order.cmp(&b.order))
    });
    let mut selected: Vec<MatchSpan> = Vec::new();
    for span in spans {
        if selected
            .iter()
            .all(|s| span.start >= s.end || span.end <= s.start)
        {
            selected.push(span);
        }
    }
    selected.sort_by_key(|s| (s.start, s.end));
    selected
}

// ---------------------------------------------------------------------------
// 十六进制转储防护（xxd / hexdump -C 列视图）
//
// 工具输出的 hex 列是原文的另一种编码：文本正则看不见它，敏感值会原样泄漏；
// 同时 16 字节定宽列把原文切碎，ASCII 列只剩片段（片段仍可能被正则命中，
// 产生片段映射，甚至前缀 IP 误配成另一个映射值）。这里把连续转储行重建为
// 连续字节流，在字节流上复用正则/特殊值检测，命中的字节在 hex 列替换为 xx、
// ASCII 列替换为 . ——两者都是投影上的不可逆清除，不走标记映射（转储片段
// 不产生映射与标记）。base64/压缩等其它编码形态仍是盲区。
// ---------------------------------------------------------------------------

/// 单行 pair 上限（标准工具 16/行，留余量；超宽视为误判）
const HEXD_MAX_PAIRS: usize = 32;
/// 少于 4 对不视为转储行（排除散文里零星 hex 词）
const HEXD_MIN_PAIRS: usize = 4;

/// xxd 偏移列（"00000000: "）；hexdump -C 偏移列（"00000000  "）
static HEXD_OFFSET_COLON_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^\s*[0-9A-Fa-f]{1,16}:\s").unwrap());
static HEXD_OFFSET_BLANK_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"^\s*[0-9A-Fa-f]{6,16}\s{2,}").unwrap());

/// 一行转储解析结果：(pair 列表, ASCII 列文本, ASCII 列起始字节偏移)
/// pair = (行内文本起始字节, 结束字节, 字节值)
type HexdLine = (Vec<(usize, usize, u8)>, String, Option<usize>);

/// 一行转储的掩码工作行：(行下标, pair 列表, ASCII 列文本, ASCII 列起始字节偏移)
type HexdRow = (usize, Vec<(usize, usize, u8)>, String, Option<usize>);

/// 解析一行转储输出。hex 列按"空格分隔的偶长 hex 段"贪心解析，遇到 ≥2 空格的
/// 列间隔或首个异样段即进入 ASCII 列（xxd 与 hexdump -C 的 ASCII 列前都有两空格；
/// hexdump -C 的 ASCII 列带 |…| 包裹，此处剥掉）。
fn hexd_parse_line(line: &str) -> Option<HexdLine> {
    let raw = line.trim_end_matches(['\r', '\n']);
    let pos = if let Some(m) = HEXD_OFFSET_COLON_RE.find(raw) {
        m.end()
    } else {
        HEXD_OFFSET_BLANK_RE.find(raw)?.end()
    };
    let mut pairs: Vec<(usize, usize, u8)> = Vec::new();
    let mut ascii_start: Option<usize> = None;
    let mut pos = pos;
    let bytes = raw.as_bytes();
    while pos < raw.len() {
        let mut gap = 0usize;
        while pos < raw.len() && (bytes[pos] == b' ' || bytes[pos] == b'\t') {
            gap += 1;
            pos += 1;
        }
        if pos >= raw.len() {
            break;
        }
        let run_start = pos;
        while pos < raw.len() && bytes[pos] != b' ' && bytes[pos] != b'\t' {
            pos += 1;
        }
        let run = &raw[run_start..pos];
        let is_hex = run.len() % 2 == 0
            && (2..=16).contains(&run.len())
            && run.bytes().all(|b| b.is_ascii_hexdigit());
        if !pairs.is_empty() && (gap >= 2 || !is_hex) {
            // 列间隔或异样段：ASCII 列开始（形似 hex 也不再解析）
            ascii_start = Some(run_start);
            break;
        }
        if !is_hex {
            return None;
        }
        for k in (0..run.len()).step_by(2) {
            let value = u8::from_str_radix(&run[k..k + 2], 16).ok()?;
            pairs.push((run_start + k, run_start + k + 2, value));
        }
        if pairs.len() > HEXD_MAX_PAIRS {
            return None;
        }
    }
    if pairs.len() < HEXD_MIN_PAIRS {
        return None;
    }
    let mut ascii_text = String::new();
    if let Some(start) = ascii_start {
        ascii_text = raw[start..].to_string();
        if ascii_text.len() >= 2 && ascii_text.starts_with('|') && ascii_text.ends_with('|') {
            ascii_text = ascii_text[1..ascii_text.len() - 1].to_string();
            ascii_start = Some(start + 1);
        }
    }
    Some((pairs, ascii_text, ascii_start))
}

/// 文本第 k 个字符在字符串中的字节范围（k 超出字符数返回 None；
/// 替换目标是 ASCII 列的第 k 个字符，多字节字符场景下与字节下标不同）
fn char_byte_range(s: &str, k: usize) -> Option<(usize, usize)> {
    for (idx, (byte_pos, ch)) in s.char_indices().enumerate() {
        if idx == k {
            return Some((byte_pos, byte_pos + ch.len_utf8()));
        }
    }
    None
}

/// 对 lines[i..j] 的连续转储块做检测与抹除，改动写入 out_lines。
fn hexd_mask_block(
    out_lines: &mut [String],
    lines: &[String],
    i: usize,
    j: usize,
    detect: &mut dyn FnMut(&str) -> Vec<MatchSpan>,
) {
    // 先整块解析；任一行形态不齐则整块放弃（宁可漏不误伤普通文本）
    let mut rows: Vec<HexdRow> = Vec::new();
    for (offset, line) in lines[i..j].iter().enumerate() {
        let Some((pairs, ascii_text, ascii_start)) = hexd_parse_line(line) else {
            return;
        };
        rows.push((i + offset, pairs, ascii_text, ascii_start));
    }
    let mut data: Vec<u8> = Vec::new();
    let mut owner: Vec<(usize, usize)> = Vec::new(); // 全局字节下标 -> (行下标, 行内 pair 下标)
    for (r, (_, pairs, _, _)) in rows.iter().enumerate() {
        for (k, (_, _, value)) in pairs.iter().enumerate() {
            data.push(*value);
            owner.push((r, k));
        }
    }
    // 字节流按 Latin-1 语义解码（字节 ↔ 字符 1:1，检测下标即字节下标）；
    // 邮箱/IP/手机号等规则目标均为 ASCII，解码失真不影响命中
    let decoded: String = data.iter().map(|&b| b as char).collect();
    let spans = detect(&decoded);
    if spans.is_empty() {
        return;
    }
    let mut edits: HashMap<usize, Vec<(usize, usize, &'static str)>> = HashMap::new();
    for span in &spans {
        let end = span.end.min(owner.len());
        for &(r, k) in &owner[span.start..end] {
            let (li, pairs, ascii_text, ascii_start) = &rows[r];
            let (s, e, _) = pairs[k];
            edits.entry(*li).or_default().push((s, e, "xx"));
            if let Some(a_start) = ascii_start {
                if let Some((cs, ce)) = char_byte_range(ascii_text, k) {
                    edits
                        .entry(*li)
                        .or_default()
                        .push((a_start + cs, a_start + ce, "."));
                }
            }
        }
    }
    for (li, mut items) in edits {
        let new = &mut out_lines[li];
        // "xx"/"." 与被替换片段等长，替换不位移；倒序仅为直观安全
        items.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        for (s, e, rep) in items {
            new.replace_range(s..e, rep);
        }
    }
}

/// 按 '\n' 切分并保留行尾（含结尾空段）；转储输出以 '\n' 行尾，
/// 解析前再剥 '\r'，与 Python splitlines(keepends=True) 在该场景等价
fn split_lines_keepends(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// 扫描文本中的 xxd/hexdump -C 转储块并抹除其中的规则命中值。
/// 无转储块时原样返回；命中值在 hex 列（xx）与 ASCII 列（.）同时抹除，
/// 不可还原、不产生标记映射。
fn hexdump_mask(text: &str, detect: &mut dyn FnMut(&str) -> Vec<MatchSpan>) -> String {
    if !text.contains("  ") {
        return text.to_string();
    }
    let lines = split_lines_keepends(text);
    let mut out = lines.clone();
    let n = lines.len();
    let mut i = 0usize;
    while i < n {
        let Some(first) = hexd_parse_line(&lines[i]) else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < n && hexd_parse_line(&lines[j]).is_some() {
            j += 1;
        }
        // ≥2 行连续即认块；孤立单行需 ≥8 对（16 字节整行）才算转储
        if (j - i) >= 2 || first.0.len() >= 8 {
            hexd_mask_block(&mut out, &lines, i, j, detect);
        }
        i = j;
    }
    if out == lines {
        return text.to_string();
    }
    out.concat()
}

// ---------------------------------------------------------------------------
// 标记协议说明注入（Claude / Codex / Gemini 三形状）
// ---------------------------------------------------------------------------

/// 把标记协议说明注入请求的系统提示，兼容三种请求形状：
/// - Claude：`system`（缺省/字符串/blocks 数组）
/// - Codex Responses：`instructions`（字符串）
/// - Gemini：`systemInstruction.parts[]`
///
/// 返回是否修改了 body。
fn inject_marker_prompt(body: &mut Value) -> bool {
    let note_value = || Value::String(MARKER_PROMPT_NOTE.to_string());
    if let Some(system) = body.get_mut("system") {
        return match system {
            Value::String(s) => {
                s.push_str("\n\n");
                s.push_str(MARKER_PROMPT_NOTE);
                true
            }
            Value::Array(blocks) => {
                blocks.push(json!({"type": "text", "text": MARKER_PROMPT_NOTE}));
                true
            }
            _ => false,
        };
    }
    if let Some(Value::String(instructions)) = body.get_mut("instructions") {
        instructions.push_str("\n\n");
        instructions.push_str(MARKER_PROMPT_NOTE);
        return true;
    }
    if let Some(si) = body.get_mut("systemInstruction") {
        if let Some(parts) = si.get_mut("parts").and_then(|p| p.as_array_mut()) {
            parts.push(json!({"text": MARKER_PROMPT_NOTE}));
            return true;
        }
    }
    // 三种字段都缺失：默认按 Claude 形状创建 system blocks
    body.as_object_mut()
        .map(|map| {
            map.insert(
                "system".to_string(),
                json!([{"type": "text", "text": note_value()}]),
            );
        })
        .is_some()
}

// ---------------------------------------------------------------------------
// 配置文档：默认值、加载、快照
// ---------------------------------------------------------------------------

/// 默认规则集（与外部 Python 参考实现的 rules.json 同源：kv 密钥（capture 只遮值）、
/// 邮箱、手机号、内网 IPv4、用户目录、API 令牌、JWT、供应商前缀、Bearer、私钥块）
const DEFAULT_RULES_JSON: &str = r#"{
  "enabled": true,
  "rules": [
    {
      "kind": "regex",
      "name": "kv-secret",
      "comment": "键值对密钥兜底（键名清单源自 Khan 安全团队《使用一个正则表达式搜索所有泄露的密钥》，本插件改写：命名组 capture=v 只遮值、键名/引号/分隔符原样保留；容忍键值间空白（含换行，≤32 字符）与 =/:/=>/:=/->/:: 等分隔符、成对单双反引号（env/JSON/YAML/log 通吃）",
      "pattern": "(?i)(?P<k>(?:access_key|access_token|admin_pass|admin_user|algolia_admin_key|algolia_api_key|alias_pass|alicloud_access_key|amazon_secret_access_key|amazonaws|ansible_vault_password|aos_key|api_key|api_key_secret|api_key_sid|api_secret|api.googlemaps|apikey|apiSecret|app_debug|app_id|app_key|app_log_level|app_secret|appkey|appkeysecret|application_key|appsecret|appspot|auth_token|authorizationToken|authsecret|aws_access|aws_access_key_id|aws_bucket|aws_key|aws_secret|aws_secret_key|aws_token|AWSSecretKey|b2_app_key|bashrc password|bintray_apikey|bintray_gpg_password|bintray_key|bintraykey|bluemix_api_key|bluemix_pass|browserstack_access_key|bucket_password|bucketeer_aws_access_key_id|bucketeer_aws_secret_access_key|built_branch_deploy_key|bx_password|cache_driver|cache_s3_secret_key|cattle_access_key|cattle_secret_key|certificate_password|ci_deploy_password|client_secret|client_zpk_secret_key|clojars_password|cloud_api_key|cloud_watch_aws_access_key|cloudant_password|cloudflare_api_key|cloudflare_auth_key|cloudinary_api_secret|cloudinary_name|codecov_token|config|conn.login|connectionstring|consumer_key|consumer_secret|credentials|cypress_record_key|database_password|database_schema_test|datadog_api_key|datadog_app_key|db_password|db_server|db_username|dbpasswd|dbpassword|dbuser|deploy_password|digitalocean_ssh_key_body|digitalocean_ssh_key_ids|docker_hub_password|docker_key|docker_pass|docker_passwd|docker_password|dockerhub_password|dockerhubpassword|dot-files|dotfiles|droplet_travis_password|dynamoaccesskeyid|dynamosecretaccesskey|elastica_host|elastica_port|elasticsearch_password|encryption_key|encryption_password|env.heroku_api_key|env.sonatype_password|eureka.awssecretkey)[a-z0-9_.\\-, ]{0,25})[\"']?[\\s]{0,32}(?:=>|:=|->|\\|=|<=|::|[:=])[\\s]{0,32}(?P<q>[\"'`]?)(?P<v>[0-9a-zA-Z\\-_/+=]{8,128})(?P=q)",
      "capture": "v",
      "label": "SECRET",
      "desc": "kv密钥值",
      "priority": 10,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "kv-generic",
      "comment": "裸通用键名兜底（原 Khan 清单不含裸 password/secret/token/key——最常见的形态反而漏）；值下限放宽到 6，键名保留、只遮值",
      "pattern": "(?i)(?P<k>(?:password|passwd|pwd|passphrase|secret|token|apikey|api_key|credential|key|secret_key|access_key|secret_id|auth_key|session_key|signing_key|private_key)[a-z0-9_.\\-, ]{0,25})[\"']?[\\s]{0,32}(?:=>|:=|->|\\|=|<=|::|[:=])[\\s]{0,32}(?P<q>[\"'`]?)(?P<v>[0-9a-zA-Z\\-_/+=]{6,128})(?P=q)",
      "capture": "v",
      "label": "SECRET",
      "desc": "kv密钥值",
      "priority": 11,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "email",
      "pattern": "[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\\.[A-Za-z]{2,}",
      "label": "EMAIL",
      "desc": "邮箱",
      "priority": 20,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "cn-mobile",
      "pattern": "(?<!\\d)1[3-9]\\d{9}(?!\\d)",
      "label": "PHONE",
      "desc": "中国大陆手机号",
      "priority": 20,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "private-ipv4",
      "pattern": "(?:10\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}|172\\.(?:1[6-9]|2[0-9]|3[01])\\.[0-9]{1,3}\\.[0-9]{1,3}|192\\.168\\.[0-9]{1,3}\\.[0-9]{1,3})(?![0-9])",
      "label": "IPV4",
      "desc": "内网 IPv4",
      "priority": 20,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "win-user-profile",
      "pattern": "[A-Za-z]:\\\\Users\\\\[^\\\\/\"'\\s]+",
      "label": "PATH",
      "desc": "Windows 用户目录",
      "priority": 30,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "api-token-sk",
      "pattern": "\\bsk-[A-Za-z0-9_-]{20,}",
      "label": "TOKEN",
      "desc": "OpenAI 形态 API 密钥",
      "priority": 15,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "jwt",
      "pattern": "\\beyJ[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{10,}\\.[A-Za-z0-9_-]{5,}",
      "label": "TOKEN",
      "desc": "JWT",
      "priority": 15,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "token-prefix",
      "comment": "供应商前缀令牌：前缀（AWS AKIA / 阿里云 LTAI / 腾讯云 AKID / Google AIza / Slack xox- / GitHub ghp / Stripe sk_live_ 等）保留给 AI 以识别厂商，仅遮主体；无需键名上下文",
      "pattern": "\\b(?P<pre>AKIA|LTAI|AKID|AIza|xox[a-prs]-|gh[pousr]_|(?:sk|rk)_(?:live|test)_)(?P<v>[0-9A-Za-z_\\-]{10,64})\\b",
      "capture": "v",
      "label": "TOKEN",
      "desc": "供应商密钥",
      "priority": 15,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "bearer-token",
      "comment": "Authorization: Bearer <token>——Bearer 字样保留，仅遮令牌本体",
      "pattern": "(?i)\\bbearer[ \\t]+(?P<v>[A-Za-z0-9._\\-]{20,256})",
      "capture": "v",
      "label": "TOKEN",
      "desc": "Bearer令牌",
      "priority": 15,
      "enabled": true
    },
    {
      "kind": "regex",
      "name": "pem-private-key",
      "comment": "私钥块整体遮蔽：BEGIN/END 头保留（AI 知道这里是什么），base64 主体作为命名组整段遮蔽",
      "pattern": "-----BEGIN [A-Z ]*PRIVATE KEY-----(?P<v>[\\s\\S]{100,}?-----END [A-Z ]*PRIVATE KEY-----)",
      "capture": "v",
      "label": "KEY",
      "desc": "私钥主体",
      "priority": 15,
      "enabled": true
    },
    {
      "kind": "literal",
      "name": "real-name",
      "comment": "把真实姓名填进 values 数组即生效",
      "values": [],
      "label": "NAME",
      "desc": "姓名",
      "priority": 5,
      "enabled": true
    }
  ]
}"#;

/// 默认配置文档（无任何存档时物化入库；用户随后在面板上编辑的就是这份）
fn default_docs() -> Value {
    json!({
        CONFIG_DOC_FILE: {
            "prompt_note": true,
            "enable_regex": true,
            "enable_hexdump_guard": true,
            "cache_capacity": DEFAULT_CACHE_CAPACITY,
            "hash": { "algorithm": "blake2b", "mode": "adaptive", "length": 16 }
        },
        RULES_DOC_FILE: serde_json::from_str::<Value>(DEFAULT_RULES_JSON)
            .expect("内置默认规则集必须是合法 JSON"),
        CUSTOM_DOC_FILE: { "enabled": true, "values": [] }
    })
}

/// 从 settings 表读取配置文档映射：None = 无存档（首次启动）；Err = 存档损坏
fn load_docs_from(db: &Database) -> Result<Option<Value>, String> {
    let Some(raw) = db
        .get_setting(CONFIG_STORE_KEY)
        .map_err(|e| format!("读取配置存储失败: {e}"))?
    else {
        return Ok(None);
    };
    let docs: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("配置文档解析失败: {e}（请修复后在面板重新保存）"))?;
    if !docs.is_object() {
        return Err("配置文档必须是 JSON 对象".to_string());
    }
    Ok(Some(docs))
}

/// 把配置文档映射写入 settings 表
fn store_docs_to(db: &Database, docs: &Value) -> Result<(), String> {
    let raw = serde_json::to_string(docs).map_err(|e| format!("配置文档序列化失败: {e}"))?;
    db.set_setting(CONFIG_STORE_KEY, &raw)
        .map_err(|e| format!("写入配置存储失败: {e}"))
}

/// 单份文档的结构防御：非对象时告警并回退默认（fail-open，不阻断启动）。
/// 内容级容错不在这里做——规则/特殊值逐条跳过、config 字段按缺省读取，
/// 与参考实现的引擎语义一致（批量校验器只用于面板保存入口）。
fn doc_or_fallback(docs: &Value, file: &str, fallback: Value) -> Value {
    match docs.get(file) {
        Some(doc) if doc.is_object() => doc.clone(),
        Some(_) => {
            log::warn!("[PRIVACY] 配置文档 {file} 不是 JSON 对象，已回退默认");
            fallback
        }
        None => fallback,
    }
}

/// 引擎配置快照（不可变；config_write 时整体重建）
#[derive(Debug, Default)]
struct EngineSnapshot {
    /// 编译后的规则（rules 文档总开关关闭时为空）
    rules: Vec<CompiledRule>,
    /// config.enable_regex：正则引擎总开关
    enable_regex: bool,
    /// config.prompt_note：标记协议说明注入
    prompt_note: bool,
    /// config.enable_hexdump_guard：十六进制转储防护
    enable_hexdump_guard: bool,
    /// config.cache_capacity：替换缓存条数
    cache_capacity: usize,
    /// config.hash：新映射的散列参数
    hash: HashConfig,
    /// 自定义特殊值
    custom_values: Vec<CustomValue>,
    /// 配置指纹：三份文档内容的 blake2b（缓存键的一部分，配置变更自动失效）
    fingerprint: String,
}

impl EngineSnapshot {
    /// 是否有任何可用检测来源：正则规则（enable_regex 开且非空）或自定义特殊值。
    /// 无来源时插件整体直通（含说明注入也不做）。
    fn has_backends(&self) -> bool {
        (self.enable_regex && !self.rules.is_empty()) || !self.custom_values.is_empty()
    }
}

/// 由配置文档映射构建引擎快照（纯函数；结构级防御，内容级逐条容错）
fn build_snapshot(docs: &Value) -> EngineSnapshot {
    let config_doc = doc_or_fallback(docs, CONFIG_DOC_FILE, json!({}));
    let rules_doc = doc_or_fallback(docs, RULES_DOC_FILE, json!({"enabled": true, "rules": []}));
    let custom_doc = doc_or_fallback(
        docs,
        CUSTOM_DOC_FILE,
        json!({"enabled": true, "values": []}),
    );

    let config_obj = config_doc.as_object().expect("doc_or_fallback 后必为对象");
    let enable_regex = truthy_or(config_obj.get("enable_regex"), true);
    let prompt_note = truthy_or(config_obj.get("prompt_note"), true);
    let enable_hexdump_guard = truthy_or(config_obj.get("enable_hexdump_guard"), true);
    let cache_capacity = as_int_or(
        config_obj.get("cache_capacity"),
        DEFAULT_CACHE_CAPACITY as i64,
    )
    .max(16) as usize;

    let mut hash = HashConfig::default();
    if let Some(hash_obj) = config_obj.get("hash").and_then(Value::as_object) {
        if let Some(name) = hash_obj.get("algorithm").and_then(Value::as_str) {
            match HashAlgorithm::from_name(name) {
                Some(a) => hash.algorithm = a,
                None => log::warn!(
                    "[PRIVACY] 散列算法 {name:?} 不支持（可选 {}），回退 blake2b",
                    HASH_ALGORITHMS.join(", ")
                ),
            }
        }
        match hash_obj.get("mode").and_then(Value::as_str) {
            Some("fixed") => hash.mode = HashMode::Fixed,
            Some("adaptive") => hash.mode = HashMode::Adaptive,
            Some(other) => {
                log::warn!(
                    "[PRIVACY] 散列方式 {other:?} 不支持（可选 adaptive / fixed），回退 adaptive"
                )
            }
            None => {}
        }
        hash.length = as_int_or(hash_obj.get("length"), 16).clamp(4, 64) as usize;
    }

    // 指纹 = 三份文档序列化的内容 hash（任何配置变更自动失效缓存）
    let mut fingerprint_input = String::new();
    fingerprint_input.push_str(&config_doc.to_string());
    fingerprint_input.push('\u{0}');
    fingerprint_input.push_str(&rules_doc.to_string());
    fingerprint_input.push('\u{0}');
    fingerprint_input.push_str(&custom_doc.to_string());

    EngineSnapshot {
        rules: compile_rules(&rules_doc),
        enable_regex,
        prompt_note,
        enable_hexdump_guard,
        cache_capacity,
        hash,
        custom_values: compile_custom_values(&custom_doc),
        fingerprint: blake2b_hex(fingerprint_input.as_bytes()),
    }
}

// ---------------------------------------------------------------------------
// 插件
// ---------------------------------------------------------------------------

/// 插件共享状态（配置快照 + 映射 + 缓存）
struct PrivacyState {
    /// 当前配置快照（config_write 时整体换新；请求路径只取 Arc 克隆）
    snapshot: RwLock<Arc<EngineSnapshot>>,
    store: Mutex<MappingStore>,
    cache: Mutex<ReplaceCache>,
    /// 替换实际执行次数（观测/测试用：验证缓存命中时检测未重跑）
    replace_runs: AtomicU64,
}

/// 隐私替换内置插件：请求方向替换敏感信息，流式/非流式响应方向还原标记。
pub struct BuiltinPrivacyPlugin {
    db: Arc<Database>,
    state: Arc<PrivacyState>,
}

impl BuiltinPrivacyPlugin {
    /// 构造并初始化：载入映射 → 读取（或物化）配置文档 → 预注册自定义特殊值
    pub fn new(db: Arc<Database>) -> Self {
        let state = Arc::new(PrivacyState {
            snapshot: RwLock::new(Arc::new(EngineSnapshot::default())),
            store: Mutex::new(MappingStore::default()),
            cache: Mutex::new(ReplaceCache::default()),
            replace_runs: AtomicU64::new(0),
        });
        let plugin = Self { db, state };
        plugin.load_startup_mappings();

        match load_docs_from(&plugin.db) {
            Ok(Some(docs)) => plugin.apply_docs(&docs),
            Ok(None) => {
                // 首次启动：物化默认配置（含默认规则集），面板读到的就是这份
                let docs = default_docs();
                if let Err(e) = store_docs_to(&plugin.db, &docs) {
                    log::warn!("[PRIVACY] 默认配置写入失败（本次以内存默认值运行）: {e}");
                }
                plugin.apply_docs(&docs);
            }
            Err(e) => {
                // 存档损坏：以默认配置启动（fail-open），不覆盖原存档，可在面板重新保存修复
                log::warn!("[PRIVACY] {e}，本次以默认配置启动");
                plugin.apply_docs(&default_docs());
            }
        }
        plugin
    }

    /// 启动时全量载入映射（上限 [`MAX_MAPPING_ROWS`]，超出按 created_at 淘汰最旧记录）
    fn load_startup_mappings(&self) {
        let rows = match self.db.load_all_privacy_mappings() {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("[PRIVACY] 载入隐私映射失败，本次以空映射启动(fail-open): {e}");
                return;
            }
        };
        let kept: &[(String, String, String, i64)] = if rows.len() > MAX_MAPPING_ROWS {
            &rows[rows.len() - MAX_MAPPING_ROWS..]
        } else {
            &rows
        };
        if rows.len() > MAX_MAPPING_ROWS {
            match self.db.delete_privacy_mappings_older_than(kept[0].3) {
                Ok(n) => log::info!("[PRIVACY] 映射超出上限，已淘汰 {n} 条最旧记录"),
                Err(e) => log::warn!("[PRIVACY] 淘汰超限映射失败: {e}"),
            }
        }
        let mut store = self.state.store.lock().unwrap();
        for (id, original, _, _) in kept {
            store.by_id.insert(
                id.clone(),
                MappingRecord {
                    original: original.clone(),
                },
            );
            // setdefault 语义：同一原文重复出现时保留既有反查（映射稳定性优先）
            store
                .by_original
                .entry(original.clone())
                .or_insert_with(|| id.clone());
        }
        if !kept.is_empty() {
            log::info!("[PRIVACY] 已载入 {} 条隐私映射", kept.len());
        }
    }

    /// 应用新的配置文档：重建快照 → 预注册新增自定义特殊值 → 原子换新
    fn apply_docs(&self, docs: &Value) {
        let snapshot = Arc::new(build_snapshot(docs));
        // 用户自定义特殊值：登记即预注册映射（用户"手动记录"的值立刻拥有稳定 id，
        // 即使尚未在任何请求里出现过，响应侧也已可还原）
        let mut pending: Vec<(String, String, String)> = Vec::new();
        {
            let mut store = self.state.store.lock().unwrap();
            for cv in &snapshot.custom_values {
                if !store.by_original.contains_key(&cv.value) {
                    let (id, is_new) = store.insert(&cv.value, &snapshot.hash);
                    if is_new {
                        pending.push((id, cv.value.clone(), cv.label.clone()));
                    }
                }
            }
        }
        for (id, original, label) in pending {
            if let Err(e) = self.db.upsert_privacy_mapping(&id, &original, &label) {
                log::warn!("[PRIVACY] 预注册映射写入失败（内存映射仍生效）: {e}");
            }
        }
        *self.state.snapshot.write().unwrap() = snapshot;
    }

    fn current_snapshot(&self) -> Arc<EngineSnapshot> {
        self.state.snapshot.read().unwrap().clone()
    }

    fn mapping_is_empty(&self) -> bool {
        self.state.store.lock().unwrap().by_id.is_empty()
    }

    /// 十六进制转储防护的检测回调：在重建的字节流上复用特殊值与正则规则
    fn hexguard_detect(&self, decoded: &str, snapshot: &EngineSnapshot) -> Vec<MatchSpan> {
        let mut spans = collect_custom_spans(decoded, &snapshot.custom_values);
        if snapshot.enable_regex {
            spans.extend(collect_regex_spans(decoded, &snapshot.rules));
        }
        if spans.is_empty() {
            Vec::new()
        } else {
            resolve_overlaps(spans)
        }
    }

    /// 批量替换（两段式）：缓存命中直接取；未命中的字符串先做十六进制转储防护
    /// （命中值不可逆抹除），再收集自定义特殊值 + 正则规则的 span，统一替换并写缓存。
    /// 返回 原串 → 替换结果（键为未经转储抹除的原始串）。
    fn compute_replacements(
        &self,
        db: &Database,
        texts: &[String],
        snapshot: &EngineSnapshot,
    ) -> HashMap<String, String> {
        let mut result: HashMap<String, String> = HashMap::with_capacity(texts.len());
        // 阶段 1a：缓存命中直接取，未命中的进入待检测列表
        let pending: Vec<String> = {
            let cache = self.state.cache.lock().unwrap();
            texts
                .iter()
                .filter_map(
                    |text| match cache.get(&cache_key(&snapshot.fingerprint, text)) {
                        Some(cached) => {
                            result.insert(text.clone(), cached.clone());
                            None
                        }
                        None => Some(text.clone()),
                    },
                )
                .collect()
        };
        if pending.is_empty() {
            return result;
        }
        self.state
            .replace_runs
            .fetch_add(pending.len() as u64, Ordering::Relaxed);
        // 阶段 1b：转储防护 → span 收集 → 统一替换，写缓存
        let mut cache = self.state.cache.lock().unwrap();
        for text in &pending {
            let masked;
            let work: &str = if snapshot.enable_hexdump_guard {
                let snapshot_ref = snapshot;
                masked = hexdump_mask(text, &mut |decoded| {
                    self.hexguard_detect(decoded, snapshot_ref)
                });
                &masked
            } else {
                text.as_str()
            };
            // span 来源：自定义特殊值（用户登记，优先）+ 正则规则
            let mut spans = collect_custom_spans(work, &snapshot.custom_values);
            if snapshot.enable_regex {
                spans.extend(collect_regex_spans(work, &snapshot.rules));
            }
            let replaced = self.apply_spans(db, work, &spans, &snapshot.hash);
            cache.put(
                cache_key(&snapshot.fingerprint, text),
                replaced.clone(),
                snapshot.cache_capacity,
            );
            result.insert(text.clone(), replaced);
        }
        result
    }

    /// 合并 span → 标记区守卫（先剔除与 ⟦PII|…⟧ 区域重叠的 span）→ 重叠消解
    /// （priority 小者胜、同级长 span 胜）→ 从左到右统一替换（每个唯一原文经映射表换取 id）
    fn apply_spans(
        &self,
        db: &Database,
        text: &str,
        spans: &[MatchSpan],
        hash: &HashConfig,
    ) -> String {
        // 防御性过滤越界/非字符边界的 span（fail-open）
        let valid: Vec<MatchSpan> = spans
            .iter()
            .filter(|span| {
                span.start < span.end
                    && span.end <= text.len()
                    && text.is_char_boundary(span.start)
                    && text.is_char_boundary(span.end)
            })
            .cloned()
            .collect();
        // 标记区守卫：已存在的 ⟦PII|…⟧ 文本不得再次入库/再遮罩，
        // 否则模型复述标记、或标记残留在落盘文件里时会被"标记套标记"，产生垃圾映射
        let zones: Vec<(usize, usize)> = scan_markers(text)
            .into_iter()
            .map(|(s, e, _)| (s, e))
            .collect();
        let spans = resolve_overlaps(
            valid
                .into_iter()
                .filter(|span| {
                    !zones
                        .iter()
                        .any(|(zs, ze)| span.start < *ze && span.end > *zs)
                })
                .collect(),
        );
        if spans.is_empty() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0usize;
        // 新增映射 (id, original, label)，锁外落库
        let mut pending_db: Vec<(String, String, String)> = Vec::new();
        {
            let mut store = self.state.store.lock().unwrap();
            for span in spans {
                let original = &text[span.start..span.end];
                let (id, is_new) = store.insert(original, hash);
                if is_new {
                    pending_db.push((id.clone(), original.to_string(), span.label.clone()));
                }
                out.push_str(&text[last..span.start]);
                out.push_str(&format_marker(&id, &span.label, &span.desc));
                last = span.end;
            }
        }
        // 最后一个 span 之后的尾巴
        out.push_str(&text[last..]);
        for (id, original, label) in pending_db {
            if let Err(e) = db.upsert_privacy_mapping(&id, &original, &label) {
                log::warn!("[PRIVACY] 写入映射表失败（内存映射仍生效）: {e}");
            }
        }
        out
    }

    // -- 面板配置（config_read / config_write） ------------------------------

    /// 面板读取：返回三份完整文档（缺失文件以默认文档补齐）
    fn handle_config_read(&self) -> Result<Value, String> {
        let stored = load_docs_from(&self.db)?;
        let docs = stored.unwrap_or_else(default_docs);
        let mut out = Map::new();
        for file in CONFIG_DOC_FILES {
            match docs.get(file) {
                Some(doc) if doc.is_object() => {
                    out.insert(file.to_string(), doc.clone());
                }
                Some(_) => return Err(format!("{file} 必须是 JSON 对象")),
                None => {
                    let fallback = match file {
                        CONFIG_DOC_FILE => json!({}),
                        RULES_DOC_FILE => json!({"enabled": true, "rules": []}),
                        _ => json!({"enabled": true, "values": []}),
                    };
                    out.insert(file.to_string(), fallback);
                }
            }
        }
        Ok(Value::Object(out))
    }

    /// 面板保存：先整批校验（校验函数会顺带归一化），任一失败即整批拒绝，不产生半写入；
    /// 通过后与既有文档合并落库，并重建快照即时生效
    fn handle_config_write(&self, docs: &Value) -> Result<(), String> {
        let Some(map) = docs.as_object() else {
            return Err("config 缺少文档映射".to_string());
        };
        let unknown: Vec<&String> = map
            .keys()
            .filter(|k| !CONFIG_DOC_FILES.contains(&k.as_str()))
            .collect();
        if !unknown.is_empty() {
            let names = unknown
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!("不支持的配置文件: {names}"));
        }
        let mut normalized: Vec<(String, Value)> = Vec::new();
        for (key, doc) in map {
            if !doc.is_object() {
                return Err(format!("{key} 必须是 JSON 对象"));
            }
            let mut doc = doc.clone();
            match key.as_str() {
                CONFIG_DOC_FILE => validate_config_doc(&mut doc)?,
                RULES_DOC_FILE => validate_rules_doc(&mut doc)?,
                CUSTOM_DOC_FILE => validate_custom_doc(&mut doc)?,
                _ => unreachable!("unknown keys filtered above"),
            }
            normalized.push((key.clone(), doc));
        }
        let mut merged = load_docs_from(&self.db)?.unwrap_or_else(default_docs);
        if !merged.is_object() {
            merged = default_docs();
        }
        {
            let merged_obj = merged.as_object_mut().unwrap();
            for (key, doc) in normalized {
                merged_obj.insert(key, doc);
            }
        }
        store_docs_to(&self.db, &merged)?;
        self.apply_docs(&merged);
        Ok(())
    }
}

/// 替换缓存键 = (配置指纹, 串精确内容 hash)；配置变更（指纹变化）自动失效
fn cache_key(fingerprint: &str, text: &str) -> String {
    format!("{fingerprint}:{}", blake2b_hex(text.as_bytes()))
}

/// 面板配置 schema（11 项，页签：正则规则 / 特殊值 / 散列与选项）
fn privacy_config_schema() -> Vec<ConfigSchemaItem> {
    fn col(key: &str, column_type: ConfigColumnType, label: &str) -> ConfigColumn {
        ConfigColumn {
            key: key.to_string(),
            column_type,
            label: label.to_string(),
            options: Vec::new(),
        }
    }
    #[allow(clippy::too_many_arguments)] // schema 构建辅助：11 项字段形状各异，分组反而更绕
    fn item(
        field_type: ConfigFieldType,
        key: &str,
        file: &str,
        tab: &str,
        label: &str,
        description: Option<&str>,
        path: Option<&str>,
        options: Vec<&str>,
        columns: Vec<ConfigColumn>,
    ) -> ConfigSchemaItem {
        ConfigSchemaItem {
            field_type,
            key: key.to_string(),
            file: file.to_string(),
            tab: Some(tab.to_string()),
            path: path.map(str::to_string),
            label: label.to_string(),
            description: description.map(str::to_string),
            options: options.into_iter().map(str::to_string).collect(),
            columns,
        }
    }
    vec![
        item(
            ConfigFieldType::Table,
            "rules",
            RULES_DOC_FILE,
            "正则规则",
            "正则/字面量规则",
            Some("同一套规则作用于两个通道：文本读取（标记替换）与十六进制转储（xx/. 抹除）；literal 规则的 values 用逗号分隔填写；comment 等未列字段保存时原样保留"),
            None,
            vec![],
            vec![
                col("enabled", ConfigColumnType::Toggle, "启用"),
                col("name", ConfigColumnType::Text, "名称"),
                col("kind", ConfigColumnType::Select, "类型").with_options(["regex", "literal"]),
                col("pattern", ConfigColumnType::Textarea, "pattern（regex）"),
                col("values", ConfigColumnType::Textarea, "values（literal，逗号分隔）"),
                col("capture", ConfigColumnType::Text, "capture（组号/组名）"),
                col("label", ConfigColumnType::Text, "标记label"),
                col("desc", ConfigColumnType::Text, "描述"),
                col("priority", ConfigColumnType::Number, "优先级"),
            ],
        ),
        item(
            ConfigFieldType::Toggle,
            "enabled",
            RULES_DOC_FILE,
            "正则规则",
            "正则规则引擎总开关",
            Some("关闭后仅自定义特殊值参与"),
            Some("enabled"),
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Table,
            "values",
            CUSTOM_DOC_FILE,
            "特殊值",
            "自定义特殊值",
            Some("逐字面完整匹配；登记即预注册映射（id 稳定）"),
            None,
            vec![],
            vec![
                col("enabled", ConfigColumnType::Toggle, "启用"),
                col("value", ConfigColumnType::Textarea, "值"),
                col("label", ConfigColumnType::Text, "标记label"),
                col("desc", ConfigColumnType::Text, "描述"),
                col("priority", ConfigColumnType::Number, "优先级"),
            ],
        ),
        item(
            ConfigFieldType::Toggle,
            "enabled",
            CUSTOM_DOC_FILE,
            "特殊值",
            "自定义特殊值总开关",
            None,
            Some("enabled"),
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Toggle,
            "enable_regex",
            CONFIG_DOC_FILE,
            "散列与选项",
            "启用正则引擎",
            None,
            None,
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Toggle,
            "prompt_note",
            CONFIG_DOC_FILE,
            "散列与选项",
            "注入隐私协议说明（prompt_note）",
            Some("含转储 xx/. 遮蔽说明（第 6/7 条）"),
            None,
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Toggle,
            "enable_hexdump_guard",
            CONFIG_DOC_FILE,
            "散列与选项",
            "十六进制转储防护（xxd/hexdump）",
            Some("命中字节在 hex 列(xx)与 ASCII 列(.)同时抹除，不可还原"),
            None,
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Select,
            "algorithm",
            CONFIG_DOC_FILE,
            "散列与选项",
            "标记 id 散列算法",
            None,
            Some("hash.algorithm"),
            vec!["blake2b", "sha256", "sha512", "sha1"],
            vec![],
        ),
        item(
            ConfigFieldType::Select,
            "mode",
            CONFIG_DOC_FILE,
            "散列与选项",
            "散列长度方式",
            None,
            Some("hash.mode"),
            vec!["adaptive", "fixed"],
            vec![],
        ),
        item(
            ConfigFieldType::Number,
            "length",
            CONFIG_DOC_FILE,
            "散列与选项",
            "fixed 模式基础长度（4-64）",
            None,
            Some("hash.length"),
            vec![],
            vec![],
        ),
        item(
            ConfigFieldType::Number,
            "cache_capacity",
            CONFIG_DOC_FILE,
            "散列与选项",
            "替换缓存条数",
            None,
            None,
            vec![],
            vec![],
        ),
    ]
}

/// 便捷构造：给 Select 列补 options（仅 schema 构建期使用）
trait ConfigColumnWithOptions {
    fn with_options(self, options: impl IntoIterator<Item = &'static str>) -> Self;
}

impl ConfigColumnWithOptions for ConfigColumn {
    fn with_options(mut self, options: impl IntoIterator<Item = &'static str>) -> Self {
        self.options = options.into_iter().map(str::to_string).collect();
        self
    }
}

static CONFIG_SCHEMA: Lazy<Vec<ConfigSchemaItem>> = Lazy::new(privacy_config_schema);

impl ProxyPlugin for BuiltinPrivacyPlugin {
    fn id(&self) -> &str {
        PRIVACY_PLUGIN_ID
    }

    fn display_name(&self) -> &str {
        "隐私替换"
    }

    fn description(&self) -> &str {
        PRIVACY_DESCRIPTION
    }

    fn is_builtin(&self) -> bool {
        true
    }

    fn stages(&self) -> &'static [PluginStage] {
        &[
            PluginStage::PreRequest,
            PluginStage::PostResponse,
            PluginStage::SseChunk,
        ]
    }

    fn default_priority(&self) -> i32 {
        // 内置插件段位（100-899）内取最小，先于其它内置变换器执行
        100
    }

    fn config_title(&self) -> Option<&str> {
        Some("隐私替换设置")
    }

    fn config_schema(&self) -> &[ConfigSchemaItem] {
        &CONFIG_SCHEMA
    }

    fn config_read(&self) -> Result<Value, PluginError> {
        self.handle_config_read()
            .map_err(|message| PluginError::Execution {
                plugin_id: PRIVACY_PLUGIN_ID.to_string(),
                message,
            })
    }

    fn config_write(&self, docs: &Value) -> Result<(), PluginError> {
        self.handle_config_write(docs)
            .map_err(|message| PluginError::Execution {
                plugin_id: PRIVACY_PLUGIN_ID.to_string(),
                message,
            })
    }

    fn transform_request(
        &self,
        _ctx: &PluginRequestContext,
        body: &mut Value,
    ) -> Result<bool, PluginError> {
        let snapshot = self.current_snapshot();
        // 无任何可用检测来源 → 与主线一致直通（含说明注入也不做）
        if !snapshot.has_backends() {
            return Ok(false);
        }
        // 阶段 1：走查收集白名单字符串（去重，保持首次出现顺序）
        let mut texts: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        walk_json(body, REPLACE_TARGET_KEYS, &mut |s| {
            if seen.insert(s.clone()) {
                texts.push(s.clone());
            }
            false
        });
        // 阶段 2：转储防护 + 批量替换（标记区守卫 + 重叠消解）→ 写缓存
        let replacements = self.compute_replacements(&self.db, &texts, &snapshot);
        // 阶段 3：统一应用替换结果
        let mut changed = false;
        walk_json(body, REPLACE_TARGET_KEYS, &mut |s| {
            if let Some(replaced) = replacements.get(s.as_str()) {
                if replaced != s {
                    *s = replaced.clone();
                    changed = true;
                    return true;
                }
            }
            false
        });
        // 标记协议说明注入：在走查之后进行（注入文本自身不参与替换）；
        // 每轮恒定注入保持系统提示跨轮稳定以维持上游 prompt 缓存。
        if snapshot.prompt_note && inject_marker_prompt(body) {
            changed = true;
        }
        Ok(changed)
    }

    fn transform_response(
        &self,
        _ctx: &PluginRequestContext,
        body: &mut Value,
    ) -> Result<bool, PluginError> {
        // 映射为空：零开销直通（回归红线：无标记时代理行为与主线一致）
        if self.mapping_is_empty() {
            return Ok(false);
        }
        let store = self.state.store.lock().unwrap();
        let mut changed = false;
        walk_json(
            body,
            RESTORE_TARGET_KEYS,
            &mut |s| match restore_markers(s, &mut |id| {
                store.by_id.get(id).map(|record| record.original.clone())
            }) {
                Some(restored) => {
                    *s = restored;
                    changed = true;
                    true
                }
                None => false,
            },
        );
        Ok(changed)
    }

    fn new_sse_state(&self) -> Option<Box<dyn Any + Send>> {
        Some(Box::new(SseStreamState::default()))
    }

    /// 流式还原：对单个 SSE 事件的 data 负载做白名单还原，
    /// 跨 delta 的半截标记用 per-stream carry 缓冲续接。任何异常均原样透传（fail-open）。
    fn transform_sse_event(
        &self,
        _ctx: &PluginRequestContext,
        event_name: Option<&str>,
        data: &mut String,
        state: &mut dyn Any,
    ) -> Result<bool, PluginError> {
        let Some(sse_state) = state.downcast_mut::<SseStreamState>() else {
            log::warn!("[PRIVACY] SSE 状态槽类型不匹配，本事件原样透传(fail-open)");
            return Ok(false);
        };

        // 事件级短路 a：映射为空 → 整体透传（零开销回归红线）
        if self.mapping_is_empty() {
            if event_name == Some("message_stop") {
                // 流结束：无条件清空全部状态（即使本事件走短路）
                *sse_state = SseStreamState::default();
            }
            return Ok(false);
        }

        // 事件级短路 b：本事件 data 不含 ⟦（U+27E6）且流中无扣留痕迹 → 零开销直通
        if !data.contains('\u{27E6}') && !sse_state.seen_marker {
            return Ok(false);
        }

        // data 是 JSON 才能按白名单定位；非 JSON（如 [DONE]）原样透传
        let Ok(mut value) = serde_json::from_str::<Value>(data) else {
            return Ok(false);
        };

        let block_key = sse_block_key(&value);
        let flush = is_sse_flush_event(event_name);
        let store = self.state.store.lock().unwrap();
        let mut lookup = |id: &str| store.by_id.get(id).map(|record| record.original.clone());

        let mut changed = false;
        walk_sse_restore(
            &mut value,
            false,
            "",
            &mut |leaf_key: &str, s: &mut String| {
                // 跨 delta 半截标记处理：new = carry + 本段文本，替换完整标记后按需扣留尾部
                let carry_key = format!("{block_key}|{leaf_key}");
                let carry = sse_state.carries.remove(&carry_key).unwrap_or_default();
                let mut combined = carry;
                combined.push_str(s);
                let (emitted, withheld, _replaced) =
                    restore_stream_text(&combined, flush, &mut lookup);
                if withheld.is_empty() {
                    if emitted != *s {
                        *s = emitted;
                        changed = true;
                        return true;
                    }
                    return false;
                }
                // 有扣留：只发出前半，尾巴存回 carry（空结果 + carry 时本字段输出空串）
                sse_state.carries.insert(carry_key, withheld);
                sse_state.seen_marker = true;
                if emitted != *s {
                    *s = emitted;
                    changed = true;
                    return true;
                }
                false
            },
        );
        drop(store);

        // flush 事件清空对应/全部 carry。content_block_stop 之后该块不再有 delta，
        // 剩余半截标记永远无法闭合（不可还原），而 stop 事件本身没有文本字段
        // 可承载残留，故直接清除并记录日志（残留仅出现在标记被上游截断的异常流中）。
        if event_name == Some("content_block_stop") {
            let stop_index = value.get("index").and_then(Value::as_u64);
            match stop_index {
                Some(idx) => sse_state.carries.retain(|key, withheld| {
                    let block = key.split('|').next().unwrap_or("");
                    let hit =
                        block.rsplit(':').next().and_then(|s| s.parse::<u64>().ok()) == Some(idx);
                    if hit && !withheld.is_empty() {
                        log::debug!("[PRIVACY] content_block_stop 丢弃未闭合标记残留: {withheld}");
                    }
                    !hit
                }),
                None => sse_state.carries.clear(),
            }
        } else if flush {
            // message_delta / message_stop：流收尾，flush 全部 carry
            for (key, withheld) in sse_state.carries.drain() {
                if !withheld.is_empty() {
                    log::debug!("[PRIVACY] {event_name:?} 丢弃未闭合标记残留 ({key}): {withheld}");
                }
            }
        }
        if event_name == Some("message_stop") {
            *sse_state = SseStreamState::default();
        }

        if !changed {
            return Ok(false);
        }
        // 走查有修改：重新序列化写回（serde_json 开启 preserve_order，键序不变）
        match serde_json::to_string(&value) {
            Ok(serialized) => {
                *data = serialized;
                Ok(true)
            }
            Err(e) => {
                log::warn!("[PRIVACY] SSE 事件还原后序列化失败，原样透传(fail-open): {e}");
                Ok(false)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// 测试用规则集：Windows 路径（prio 10）+ 邮箱（prio 20）+ 手机号（prio 20，
    /// 前后查找断言验证 fancy-regex 兼容）+ 内网 IPv4 + 字面量（prio 5）
    fn test_rules() -> Value {
        json!({
            "enabled": true,
            "rules": [
                {
                    "kind": "regex",
                    "name": "win-path",
                    "pattern": r#"[A-Za-z]:\\Users\\[^\\/"'\s]+"#,
                    "label": "PATH",
                    "desc": "用户目录",
                    "priority": 10,
                    "enabled": true
                },
                {
                    "kind": "regex",
                    "name": "email",
                    "pattern": r#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#,
                    "label": "EMAIL",
                    "desc": "邮箱",
                    "priority": 20,
                    "enabled": true
                },
                {
                    "kind": "regex",
                    "name": "cn-mobile",
                    "pattern": r"(?<!\d)1[3-9]\d{9}(?!\d)",
                    "label": "PHONE",
                    "desc": "中国大陆手机号",
                    "priority": 20,
                    "enabled": true
                },
                {
                    "kind": "regex",
                    "name": "private-ipv4",
                    "pattern": r"(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})(?![0-9])",
                    "label": "IPV4",
                    "desc": "内网 IPv4",
                    "priority": 20,
                    "enabled": true
                },
                {
                    "kind": "literal",
                    "name": "secret-word",
                    "values": ["TOPSECRET"],
                    "label": "WORD",
                    "desc": "字面量",
                    "priority": 5,
                    "enabled": true
                }
            ]
        })
    }

    /// kv-secret 同形的小型捕获组规则（(?P=q) 反引用 + capture=v 只遮值）
    fn kv_capture_rules() -> Value {
        json!({
            "enabled": true,
            "rules": [
                {
                    "kind": "regex",
                    "name": "kv-test",
                    "pattern": r#"(?i)(?P<k>(?:api_key|secret)[a-z0-9_.\-]{0,20})["']?[\s]{0,8}(?:=>|:=|->|\|=|<=|::|[:=])[\s]{0,8}(?P<q>["'`]?)(?P<v>[0-9a-zA-Z\-_/+=]{8,64})(?P=q)"#,
                    "capture": "v",
                    "label": "SECRET",
                    "desc": "kv密钥值",
                    "priority": 10,
                    "enabled": true
                },
                {
                    "kind": "regex",
                    "name": "email",
                    "pattern": r#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#,
                    "label": "EMAIL",
                    "desc": "邮箱",
                    "priority": 20,
                    "enabled": true
                }
            ]
        })
    }

    /// 残缺 id 的 kv 规则（标记区守卫测试用，与默认集同形）
    fn kv_secret_rules() -> Value {
        json!({
            "enabled": true,
            "rules": [
                {
                    "kind": "regex",
                    "name": "kv-secret",
                    "pattern": r#"(?i)\b(?:deploy_?secret|secret|token)\s*[=:]\s*["']?[^\s"']{8,}"#,
                    "label": "SECRET",
                    "desc": "密钥赋值",
                    "priority": 25,
                    "enabled": true
                },
                {
                    "kind": "regex",
                    "name": "email",
                    "pattern": r#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#,
                    "label": "EMAIL",
                    "desc": "邮箱",
                    "priority": 20,
                    "enabled": true
                }
            ]
        })
    }

    fn base_config() -> Value {
        json!({
            "prompt_note": true,
            "enable_regex": true,
            "enable_hexdump_guard": true,
            "cache_capacity": DEFAULT_CACHE_CAPACITY,
            "hash": { "algorithm": "blake2b", "mode": "adaptive", "length": 16 }
        })
    }

    fn docs_with_rules(rules: Value) -> Value {
        json!({
            CONFIG_DOC_FILE: base_config(),
            RULES_DOC_FILE: rules,
            CUSTOM_DOC_FILE: { "enabled": true, "values": [] }
        })
    }

    /// 用指定配置文档构造插件（跳过 DB 读取/物化；映射表从 DB 载入 + 预注册）
    fn plugin_with_docs(db: Arc<Database>, docs: &Value) -> BuiltinPrivacyPlugin {
        let state = Arc::new(PrivacyState {
            snapshot: RwLock::new(Arc::new(EngineSnapshot::default())),
            store: Mutex::new(MappingStore::default()),
            cache: Mutex::new(ReplaceCache::default()),
            replace_runs: AtomicU64::new(0),
        });
        let plugin = BuiltinPrivacyPlugin { db, state };
        plugin.load_startup_mappings();
        plugin.apply_docs(docs);
        plugin
    }

    fn plugin_with_rules(rules: Value) -> BuiltinPrivacyPlugin {
        plugin_with_docs(
            Arc::new(Database::memory().unwrap()),
            &docs_with_rules(rules),
        )
    }

    fn pre_request_ctx() -> PluginRequestContext {
        PluginRequestContext {
            app_type: "claude".to_string(),
            session_id: "sess-1".to_string(),
            request_model: "claude-x".to_string(),
            stage: PluginStage::PreRequest,
            provider: None,
        }
    }

    fn post_response_ctx() -> PluginRequestContext {
        PluginRequestContext {
            stage: PluginStage::PostResponse,
            ..pre_request_ctx()
        }
    }

    fn sse_ctx() -> PluginRequestContext {
        PluginRequestContext {
            stage: PluginStage::SseChunk,
            ..pre_request_ctx()
        }
    }

    // --- 编解码与映射 ---

    #[test]
    fn test_format_marker_layout() {
        let marker = format_marker("abcdef123456", "PATH", "用户目录");
        assert_eq!(marker, "⟦PII|abcdef123456|PATH|用户目录⟧");
        assert!(marker.starts_with('\u{27E6}'));
        assert!(marker.ends_with('\u{27E7}'));
    }

    #[test]
    fn test_marker_id_length_policy_adaptive() {
        let mut store = MappingStore::default();
        let hash = HashConfig::default();
        // 1 字节原文 → 下限 12
        let (short, _) = store.insert("a", &hash);
        assert_eq!(short.len(), 12);
        // 30 字节原文 → 30
        let (mid, _) = store.insert("x".repeat(30).as_str(), &hash);
        assert_eq!(mid.len(), 30);
        // 100 字节原文 → 上限 64
        let (long, _) = store.insert("y".repeat(100).as_str(), &hash);
        assert_eq!(long.len(), 64);
        // 全部为十六进制小写
        for id in [short, mid, long] {
            assert!(id
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        }
    }

    #[test]
    fn test_marker_id_fixed_length_mode() {
        let mut store = MappingStore::default();
        let hash = HashConfig {
            mode: HashMode::Fixed,
            length: 8,
            ..HashConfig::default()
        };
        let (id, _) = store.insert("a@b.com", &hash);
        assert_eq!(id.len(), 8, "fixed 模式固定长度（防长度泄漏）");
        let digest = digest_hex(hash.algorithm, b"a@b.com");
        assert_eq!(id, &digest[..8]);
    }

    #[test]
    fn test_sha1_short_digest_does_not_panic() {
        // sha1 摘要仅 40 位：fixed/adaptive 长度上限以摘要实际长度封顶（Python 切片语义）
        let mut store = MappingStore::default();
        for mode in [HashMode::Fixed, HashMode::Adaptive] {
            let hash = HashConfig {
                algorithm: HashAlgorithm::Sha1,
                mode,
                length: 64,
            };
            let (id, _) = store.insert("x".repeat(100).as_str(), &hash);
            assert_eq!(id.len(), 40, "sha1 摘要十六进制全长 40 位");
        }
    }

    #[test]
    fn test_same_original_reuses_id() {
        let mut store = MappingStore::default();
        let hash = HashConfig::default();
        let (id1, new1) = store.insert("a@b.com", &hash);
        let (id2, new2) = store.insert("a@b.com", &hash);
        assert_eq!(id1, id2);
        assert!(new1);
        assert!(
            !new2,
            "同一原文应复用既有标记（label 以首条记录为准，落库后不更新）"
        );
    }

    #[test]
    fn test_resolve_id_collision_lengthens_by_four() {
        let mut store = MappingStore::default();
        let hash = HashConfig::default();
        // 占用 victim 真实摘要的 12 位前缀（模拟被不同原文占用）
        let digest = digest_hex(hash.algorithm, b"victim");
        store.by_id.insert(
            digest[..12].to_string(),
            MappingRecord {
                original: "OTHER-ORIGINAL".to_string(),
            },
        );
        let id = store.resolve_id("victim", &hash);
        assert_eq!(id, &digest[..16], "碰撞后应加长 4 位");
        assert!(id.starts_with(&digest[..12]));
    }

    #[test]
    fn test_hash_algorithm_change_keeps_existing_ids() {
        // 稳定性红线：散列配置只影响新登记的映射，已有映射 id 永不改变
        let docs_blake2b = docs_with_rules(test_rules());
        let db = Arc::new(Database::memory().unwrap());
        let plugin = plugin_with_docs(db.clone(), &docs_blake2b);
        let (old_id, _) = plugin
            .state
            .store
            .lock()
            .unwrap()
            .insert("a@b.com", &HashConfig::default());

        // 切换到 sha256 后：同一原文复用旧 id；新原文用 sha256 前缀
        let mut docs_sha256 = docs_blake2b;
        docs_sha256[CONFIG_DOC_FILE]["hash"]["algorithm"] = json!("sha256");
        plugin.apply_docs(&docs_sha256);
        let hash_sha256 = HashConfig {
            algorithm: HashAlgorithm::Sha256,
            ..HashConfig::default()
        };
        {
            let mut store = plugin.state.store.lock().unwrap();
            let (same, is_new) = store.insert("a@b.com", &hash_sha256);
            assert_eq!(same, old_id, "已有映射 id 不随散列配置改变");
            assert!(!is_new);
            let (fresh, is_new) = store.insert("fresh@example.com", &hash_sha256);
            assert!(is_new);
            let digest = digest_hex(HashAlgorithm::Sha256, b"fresh@example.com");
            assert!(fresh.starts_with(&digest[..12]));
        }
    }

    #[test]
    fn test_scan_and_restore_markers() {
        let text = "before ⟦PII|abc123|PATH|目录⟧ middle ⟦PII|xyz789|EMAIL|邮箱⟧ after";
        let markers = scan_markers(text);
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].2, "abc123");
        assert_eq!(markers[1].2, "xyz789");

        let mut unknown = 0;
        let restored = restore_markers(text, &mut |id| {
            if id == "abc123" {
                Some("C:\\Users\\a".to_string())
            } else {
                unknown += 1;
                None
            }
        })
        .unwrap();
        assert_eq!(
            restored,
            "before C:\\Users\\a middle ⟦PII|xyz789|EMAIL|邮箱⟧ after"
        );
        assert_eq!(unknown, 1, "未知 id 原样保留");

        // 无标记文本返回 None
        assert!(restore_markers("plain text", &mut |_| None).is_none());
        // 半截标记（缺后缀）不解析
        assert!(restore_markers("⟦PII|abc123|PATH", &mut |_| None).is_none());
    }

    // --- 重叠消解 ---

    #[test]
    fn test_resolve_overlaps_priority_wins() {
        let spans = vec![
            MatchSpan {
                start: 0,
                end: 6,
                priority: 20,
                order: 1,
                label: "B".into(),
                desc: String::new(),
            },
            MatchSpan {
                start: 0,
                end: 3,
                priority: 10,
                order: 0,
                label: "A".into(),
                desc: String::new(),
            },
        ];
        let selected = resolve_overlaps(spans);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].end, 3, "priority 小者胜");
        assert_eq!(selected[0].label, "A");
    }

    #[test]
    fn test_resolve_overlaps_same_priority_longer_wins() {
        let spans = vec![
            MatchSpan {
                start: 0,
                end: 3,
                priority: 20,
                order: 0,
                label: "A".into(),
                desc: String::new(),
            },
            MatchSpan {
                start: 0,
                end: 6,
                priority: 20,
                order: 1,
                label: "B".into(),
                desc: String::new(),
            },
        ];
        let selected = resolve_overlaps(spans);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].end, 6, "同级长 span 胜");
    }

    #[test]
    fn test_resolve_overlaps_keeps_disjoint_spans() {
        let spans = vec![
            MatchSpan {
                start: 0,
                end: 3,
                priority: 20,
                order: 0,
                label: "A".into(),
                desc: String::new(),
            },
            MatchSpan {
                start: 5,
                end: 9,
                priority: 20,
                order: 1,
                label: "A".into(),
                desc: String::new(),
            },
        ];
        let selected = resolve_overlaps(spans);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].start, 0);
        assert_eq!(selected[1].start, 5);
    }

    // --- 替换与走查 ---

    #[test]
    fn test_replace_text_produces_markers_and_restore_roundtrip() {
        let plugin = plugin_with_rules(test_rules());

        let text = "mail a@b.com now";
        let (replaced, changed) = plugin.replace_text(text);
        assert!(changed);
        assert!(!replaced.contains("a@b.com"));
        let markers = scan_markers(&replaced);
        assert_eq!(markers.len(), 1);
        let marker = &replaced[markers[0].0..markers[0].1];
        assert!(
            marker.contains("|EMAIL|邮箱"),
            "标记应带 label 与 desc: {marker}"
        );

        // 还原回原文
        let store = plugin.state.store.lock().unwrap();
        assert_eq!(store.by_id.get(markers[0].2).unwrap().original, "a@b.com");
        let restored = restore_markers(&replaced, &mut |i| {
            store.by_id.get(i).map(|r| r.original.clone())
        })
        .unwrap();
        assert_eq!(restored, text);
    }

    #[test]
    fn test_capture_group_only_masks_value() {
        // Python 移植能力①：capture 指定时只替换该捕获组命中的部分，
        // 键名/引号/分隔符原样保留（AI 始终知道这一项是什么）
        let plugin = plugin_with_rules(kv_capture_rules());

        let text = r#"api_key = "abcd1234efgh5678""#;
        let (replaced, changed) = plugin.replace_text(text);
        assert!(changed);
        assert!(!replaced.contains("abcd1234"), "值应被遮蔽: {replaced}");
        assert!(
            replaced.contains(r#"api_key = "⟦PII|"#),
            "键名与引号应保留: {replaced}"
        );
        assert!(replaced.ends_with("|SECRET|kv密钥值⟧\""));
        assert!(replaced.contains("|SECRET|"));

        // 反引用语义：闭引号必须与开引号一致——无引号形态不匹配（(?P=q) 适配验证）
        let (replaced, changed) = plugin.replace_text("secret: \"ZZZZ99998888\"");
        assert!(changed);
        assert!(!replaced.contains("ZZZZ99998888"));

        // 邮箱规则照常工作（同一文本两类规则并存）
        let (replaced, _) = plugin.replace_text("mail a@b.com");
        assert!(replaced.contains("|EMAIL|"));
    }

    #[test]
    fn test_transform_request_claude_shape_with_protection() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "model": "claude-sonnet-4",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "mail a@b.com please"},
                    {"type": "tool_use", "id": "toolu_1", "name": "edit_file",
                     "input": {"file_path": "C:\\Users\\alice\\notes.txt", "note": "TOPSECRET"}}
                ]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "email a@b.com is secret", "signature": "sig-abc"}
                ]}
            ]
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());

        // 命中：text / tool_use.input（深走）/ 字面量
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(!text.contains("a@b.com"));
        let file_path = body["messages"][0]["content"][1]["input"]["file_path"]
            .as_str()
            .unwrap();
        assert!(
            file_path.starts_with(MARKER_PREFIX),
            "tool_use.input 应深走替换"
        );
        let note = body["messages"][0]["content"][1]["input"]["note"]
            .as_str()
            .unwrap();
        assert!(note.starts_with(MARKER_PREFIX), "数据对象内字面量应替换");

        // 保护：thinking / signature / type / id / role / name / model 不动
        let thinking = body["messages"][1]["content"][0]["thinking"]
            .as_str()
            .unwrap();
        assert_eq!(thinking, "email a@b.com is secret");
        assert_eq!(
            body["messages"][1]["content"][0]["signature"],
            json!("sig-abc")
        );
        assert_eq!(body["messages"][0]["content"][1]["id"], json!("toolu_1"));
        assert_eq!(
            body["messages"][0]["content"][1]["name"],
            json!("edit_file")
        );
        assert_eq!(body["messages"][0]["role"], json!("user"));
        assert_eq!(body["model"], json!("claude-sonnet-4"));
    }

    #[test]
    fn test_transform_request_codex_shape() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "model": "gpt-5",
            "instructions": "contact admin@example.com for help",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "path is C:\\Users\\bob\\file.txt"}
                ]}
            ]
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let instructions = body["instructions"].as_str().unwrap();
        assert!(
            !instructions.contains("admin@example.com"),
            "instructions 应替换"
        );
        let text = body["input"][0]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("path is ⟦PII|"), "input 深走应替换路径");
        assert!(!text.contains("bob"));
    }

    #[test]
    fn test_transform_request_gemini_shape() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "contents": [
                {"role": "user", "parts": [{"text": "call 13800138000"}]}
            ],
            "systemInstruction": {"parts": [{"text": "you are helpful at a@b.com"}]}
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
        assert!(text.contains("|PHONE|"), "contents.parts.text 应替换手机号");
        assert!(!text.contains("13800138000"));
        let system = body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap();
        assert!(
            !system.contains("a@b.com"),
            "systemInstruction.parts.text 应替换"
        );
        assert_eq!(body["contents"][0]["role"], json!("user"), "role 不动");
    }

    #[test]
    fn test_transform_request_tool_result_content_replaced() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1",
                     "content": [{"type": "text", "text": "output from 192.168.1.10"}]}
                ]}
            ]
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["messages"][0]["content"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(
            text.starts_with("output from ⟦PII|"),
            "tool_result.content 应替换: {text}"
        );
        assert!(!text.contains("192.168.1.10"));
    }

    #[test]
    fn test_transform_request_no_pii_only_injects_prompt() {
        // 无 PII：不做替换，但仍注入标记协议说明（每轮恒定注入，保持系统提示稳定）
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "model": "claude-sonnet-4",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hello world"}]}]
        });
        let before = body.clone();
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        // 消息内容不动，仅新增 system 说明
        assert_eq!(body["messages"], before["messages"]);
        assert!(body["system"][0]["text"]
            .as_str()
            .unwrap()
            .contains("隐私标记协议"));
        assert!(body["model"] == before["model"]);
    }

    #[test]
    fn test_transform_request_skipped_when_rules_disabled_or_empty() {
        // enabled=false
        let plugin = plugin_with_rules(json!({"enabled": false, "rules": test_rules()["rules"]}));
        let mut body = json!({"messages": [{"content": [{"text": "a@b.com"}]}]});
        assert!(!plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        assert_eq!(body["messages"][0]["content"][0]["text"], json!("a@b.com"));

        // 规则为空
        let plugin = plugin_with_rules(json!({"enabled": true, "rules": []}));
        let mut body = json!({"messages": [{"content": [{"text": "a@b.com"}]}]});
        assert!(!plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        assert_eq!(body["messages"][0]["content"][0]["text"], json!("a@b.com"));
    }

    #[test]
    fn test_transform_request_invalid_regex_skipped_but_others_apply() {
        let mut rules = test_rules();
        rules["rules"][0]["pattern"] = json!("([invalid");
        let plugin = plugin_with_rules(rules);
        let mut body = json!({"messages": [{"content": [{"text": "mail a@b.com"}]}]});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(
            !text.contains("a@b.com"),
            "非法 pattern 跳过，其余规则仍生效"
        );
    }

    // --- 自定义特殊值（Python 移植能力②） ---

    fn docs_with_custom_values(values: Value) -> Value {
        json!({
            CONFIG_DOC_FILE: base_config(),
            RULES_DOC_FILE: test_rules(),
            CUSTOM_DOC_FILE: { "enabled": true, "values": values }
        })
    }

    #[test]
    fn test_custom_value_preregistered_and_restorable() {
        // 登记即预注册映射：未在任何请求里出现，id 已稳定、响应侧已可还原
        let db = Arc::new(Database::memory().unwrap());
        let docs = docs_with_custom_values(json!([
            { "value": "my-secret-password-01", "label": "PASSWORD", "desc": "主密码", "enabled": true }
        ]));
        let plugin = plugin_with_docs(db.clone(), &docs);

        assert!(
            plugin
                .state
                .store
                .lock()
                .unwrap()
                .by_original
                .contains_key("my-secret-password-01"),
            "加载时即应预注册映射"
        );
        let rows = db.load_all_privacy_mappings().unwrap();
        assert_eq!(rows.len(), 1, "预注册映射应落库");
        assert_eq!(rows[0].1, "my-secret-password-01");
        assert_eq!(rows[0].2, "PASSWORD");

        // 响应侧还原：无需请求先行（用预注册的真实 id 构造标记）
        let id = plugin.state.store.lock().unwrap().by_original["my-secret-password-01"].clone();
        let mut body = json!({"text": format!("key is ⟦PII|{id}|PASSWORD|主密码⟧")});
        assert!(plugin
            .transform_response(&post_response_ctx(), &mut body)
            .unwrap());
        assert_eq!(body["text"], json!("key is my-secret-password-01"));
    }

    #[test]
    fn test_custom_value_replaces_in_request_with_high_priority() {
        // priority 缺省 1 压过正则规则（20）：邮箱命中自定义值时用 CUSTOM 类别名
        let db = Arc::new(Database::memory().unwrap());
        let docs = docs_with_custom_values(json!([
            { "value": "a@b.com", "label": "MYMAIL", "desc": "个人邮箱", "priority": 1, "enabled": true }
        ]));
        let plugin = plugin_with_docs(db, &docs);
        let mut body = json!({"messages": [{"content": [{"text": "mail a@b.com"}]}]});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("|MYMAIL|个人邮箱"),
            "自定义特殊值应压过正则: {text}"
        );
        assert!(!text.contains("a@b.com"));
    }

    #[test]
    fn test_custom_value_disabled_row_ignored() {
        // enabled=false 的行不参与（空 value 行由面板校验器拒绝，见 test_validate_custom_doc）
        let docs = docs_with_custom_values(json!([
            { "value": "gone", "label": "X", "enabled": false },
            { "value": "active", "label": "Z", "enabled": true }
        ]));
        let plugin = plugin_with_docs(Arc::new(Database::memory().unwrap()), &docs);
        let snapshot = plugin.current_snapshot();
        assert_eq!(snapshot.custom_values.len(), 1);
        assert_eq!(snapshot.custom_values[0].value, "active");
    }

    // --- 散列（Python 移植能力③） ---

    #[test]
    fn test_hash_fixed_length_prevents_length_leak() {
        let mut docs = docs_with_rules(test_rules());
        docs[CONFIG_DOC_FILE]["hash"] =
            json!({ "algorithm": "sha256", "mode": "fixed", "length": 8 });
        let plugin = plugin_with_docs(Arc::new(Database::memory().unwrap()), &docs);
        let (short, _) = plugin.replace_text("mail a@b.com");
        let (long, _) = plugin.replace_text("mail long.local-part+tag@sub.example-domain.com");
        for replaced in [&short, &long] {
            let markers = scan_markers(replaced);
            assert_eq!(markers[0].2.len(), 8, "fixed 模式 id 恒定 8 位: {replaced}");
        }
        // 与 adaptive 对比：adaptive 有 12 位下限（7 字节邮箱 → 下限 12 位），fixed 可低至 4
        let adaptive = plugin_with_rules(test_rules());
        let (short_a, _) = adaptive.replace_text("mail a@b.com");
        let id_a = scan_markers(&short_a)[0].2.len();
        assert_eq!(id_a, 12, "adaptive 模式下限 12 位");
    }

    // --- 响应还原 ---

    #[test]
    fn test_transform_response_restores_known_markers_only() {
        let plugin = plugin_with_rules(test_rules());
        // 先替换构造映射
        let mut request_body = json!({"messages": [{"content": [{"text": "mail a@b.com"}]}]});
        plugin
            .transform_request(&pre_request_ctx(), &mut request_body)
            .unwrap();
        let marked = request_body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();

        let mut response_body = json!({
            "output": [{"content": [{"type": "output_text", "text": format!("found {marked} ok")}]}]
        });
        assert!(plugin
            .transform_response(&post_response_ctx(), &mut response_body)
            .unwrap());
        let text = response_body["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "found mail a@b.com ok");

        // 未知 id 原样保留
        let mut unknown_body = json!({"text": "keep ⟦PII|unknown99|X|Y⟧ here"});
        assert!(!plugin
            .transform_response(&post_response_ctx(), &mut unknown_body)
            .unwrap());
        assert_eq!(unknown_body["text"], json!("keep ⟦PII|unknown99|X|Y⟧ here"));
    }

    #[test]
    fn test_transform_response_noop_when_mapping_empty() {
        let plugin = plugin_with_rules(test_rules());
        // 未发生任何替换 → 映射为空 → 直通
        let mut body = json!({"text": "⟦PII|abc123|PATH|X⟧"});
        assert!(!plugin
            .transform_response(&post_response_ctx(), &mut body)
            .unwrap());
        assert_eq!(body["text"], json!("⟦PII|abc123|PATH|X⟧"));
    }

    // --- SSE 流式还原 ---

    /// 构造插件并生成 "a@b.com" 的标记（走真实替换路径建立映射），返回标记全文
    fn plugin_with_email_marker() -> (BuiltinPrivacyPlugin, String) {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({"messages": [{"content": [{"text": "mail a@b.com"}]}]});
        plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap();
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        let marker = text["mail ".len()..].to_string();
        assert!(marker.starts_with(MARKER_PREFIX));
        (plugin, marker)
    }

    /// Claude 形状的 content_block_delta data JSON
    fn text_delta_data(index: u64, text: &str) -> String {
        json!({
            "type": "content_block_delta",
            "index": index,
            "delta": {"type": "text_delta", "text": text}
        })
        .to_string()
    }

    /// 取出 delta data JSON 中 delta.text 字段
    fn delta_text(data: &str) -> String {
        let value: Value = serde_json::from_str(data).unwrap();
        value["delta"]["text"].as_str().unwrap().to_string()
    }

    #[test]
    fn test_sse_marker_split_across_three_deltas_restored() {
        let (plugin, marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();

        // 标记按字符边界切成 3 段，逐个 delta 喂入
        let chars: Vec<char> = marker.chars().collect();
        let p1: String = chars[..10].iter().collect();
        let p2: String = chars[10..20].iter().collect();
        let p3: String = chars[20..].iter().collect();
        assert!(!p1.contains('\u{27E7}') && !p2.contains('\u{27E7}'));

        // delta 1：⟦PII|… 前缀不完整被扣下，只发出 "email: "
        let mut data = text_delta_data(0, &format!("email: {p1}"));
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "email: ");
        {
            let s = state.downcast_mut::<SseStreamState>().unwrap();
            assert!(!s.carries.is_empty(), "delta 1 应扣下半截标记");
            assert!(s.seen_marker);
        }

        // delta 2：carry 续上仍不完整 → 本字段输出空串，carry 继续滞留
        let mut data = text_delta_data(0, &p2);
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "");

        // delta 3：拼接后标记完整 → 还原为原文
        let mut data = text_delta_data(0, &format!("{p3} thanks"));
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "a@b.com thanks");

        // 三个 delta 的输出按序拼接 == 直接对完整标记做还原的结果（无残留、无丢失）
        let store = plugin.state.store.lock().unwrap();
        let direct = restore_markers(&marker, &mut |id| {
            store.by_id.get(id).map(|record| record.original.clone())
        })
        .unwrap();
        assert_eq!(format!("email: {direct}"), "email: a@b.com");
    }

    #[test]
    fn test_sse_carry_survives_unrelated_events_and_stop_flushes_by_index() {
        let (plugin, marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();
        let chars: Vec<char> = marker.chars().collect();
        let p1: String = chars[..10].iter().collect();
        let p2: String = chars[10..].iter().collect();

        // 块 0 扣下前半
        let mut data = text_delta_data(0, &p1);
        plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut(),
            )
            .unwrap();

        // 中间插入其他块（index 1）：互不影响（无标记则零改动）
        let mut data = text_delta_data(1, "block one");
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "block one");

        // 块 0 续上后半：跨多个事件还原完整
        let mut data = text_delta_data(0, &p2);
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "a@b.com");

        // content_block_stop：对应块 carry 清空（此时已空），其他事件字节不动
        let mut data = json!({"type": "content_block_stop", "index": 0}).to_string();
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_stop"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(
            data,
            json!({"type": "content_block_stop", "index": 0}).to_string()
        );
    }

    #[test]
    fn test_sse_message_delta_and_stop_flush_state() {
        let (plugin, marker) = plugin_with_email_marker();
        let chars: Vec<char> = marker.chars().collect();
        let p1: String = chars[..10].iter().collect();

        // message_delta：flush 全部 carry（剩余半截标记无法注入无文本字段的事件，被清除）
        let mut state = plugin.new_sse_state().unwrap();
        let mut data = text_delta_data(0, &p1);
        plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut(),
            )
            .unwrap();
        assert!(!state
            .downcast_mut::<SseStreamState>()
            .unwrap()
            .carries
            .is_empty());
        let mut data =
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}).to_string();
        assert!(!plugin
            .transform_sse_event(&sse_ctx(), Some("message_delta"), &mut data, state.as_mut())
            .unwrap());
        let s = state.downcast_mut::<SseStreamState>().unwrap();
        assert!(s.carries.is_empty(), "message_delta 应 flush 全部 carry");

        // message_stop：清空全部状态（即使 carry 非空）
        let mut state = plugin.new_sse_state().unwrap();
        let mut data = text_delta_data(0, &p1);
        plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut(),
            )
            .unwrap();
        let mut data = json!({"type": "message_stop"}).to_string();
        assert!(!plugin
            .transform_sse_event(&sse_ctx(), Some("message_stop"), &mut data, state.as_mut())
            .unwrap());
        let s = state.downcast_mut::<SseStreamState>().unwrap();
        assert!(
            s.carries.is_empty() && !s.seen_marker,
            "message_stop 后状态应全空"
        );
    }

    #[test]
    fn test_sse_flush_emits_remaining_carry_without_withholding() {
        let (plugin, marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();

        // flush 事件自身字段：完整标记照常还原
        let mut data = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "text": format!("done {marker}")}
        })
        .to_string();
        assert!(plugin
            .transform_sse_event(&sse_ctx(), Some("message_delta"), &mut data, state.as_mut())
            .unwrap());
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["delta"]["text"], json!("done a@b.com"));

        // flush 事件不扣留：残缺标记前缀全量发出（不存入 carry）
        let mut data = json!({
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn", "text": "leftover ⟦PII|abc"}
        })
        .to_string();
        assert!(!plugin
            .transform_sse_event(&sse_ctx(), Some("message_delta"), &mut data, state.as_mut())
            .unwrap());
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["delta"]["text"], json!("leftover ⟦PII|abc"));
        assert!(state
            .downcast_mut::<SseStreamState>()
            .unwrap()
            .carries
            .is_empty());
    }

    #[test]
    fn test_sse_unknown_id_and_protected_keys_untouched() {
        let (plugin, _marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();

        // 未知 id 的完整标记原样保留（事件零改动）
        let mut data = text_delta_data(0, "keep ⟦PII|unknown99|X|Y⟧ here");
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "keep ⟦PII|unknown99|X|Y⟧ here");

        // thinking / signature 等 key 不碰（thinking_delta 的字段在保护名单内）
        let mut data = json!({
            "type": "content_block_delta",
            "index": 1,
            "delta": {"type": "thinking_delta", "thinking": "saw ⟦PII|abc123|PATH|X⟧ here"}
        })
        .to_string();
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(
            value["delta"]["thinking"],
            json!("saw ⟦PII|abc123|PATH|X⟧ here")
        );
    }

    #[test]
    fn test_sse_empty_mapping_passthrough_and_state_slot_mismatch() {
        // 映射为空：任何事件整体透传（零改动），含含标记的 data
        let plugin = plugin_with_rules(test_rules());
        let mut state = plugin.new_sse_state().unwrap();
        let original = text_delta_data(0, "mail ⟦PII|abc123|EMAIL|X⟧ end");
        let mut data = original.clone();
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(data, original);

        // 状态槽类型不匹配：fail-open 透传
        let (plugin, _marker) = plugin_with_email_marker();
        let mut data = text_delta_data(0, "x ⟦PII|abc123|EMAIL|X⟧");
        let mut state_none: Box<dyn Any + Send> = Box::new(());
        assert!(!plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state_none.as_mut()
            )
            .unwrap());
        assert_eq!(data, text_delta_data(0, "x ⟦PII|abc123|EMAIL|X⟧"));
    }

    #[test]
    fn test_sse_non_json_data_and_partial_json_leaf() {
        let (plugin, marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();

        // 非 JSON data（[DONE]）原样透传
        let mut data = "[DONE]".to_string();
        assert!(!plugin
            .transform_sse_event(&sse_ctx(), Some("message_stop"), &mut data, state.as_mut())
            .unwrap());
        assert_eq!(data, "[DONE]");

        // partial_json 叶子（input_json_delta）同样参与还原
        let chars: Vec<char> = marker.chars().collect();
        let p1: String = chars[..12].iter().collect();
        let p2: String = chars[12..].iter().collect();
        let mut data = json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "input_json_delta", "partial_json": format!("{{\"q\":\"{p1}")}
        })
        .to_string();
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(
            value["delta"]["partial_json"],
            Value::String("{\"q\":\"".to_string()),
            "carry 前缀扣下后只发出前半"
        );

        let mut data = json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "input_json_delta", "partial_json": format!("{p2}\"}}")}
        })
        .to_string();
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(
            value["delta"]["partial_json"],
            json!("a@b.com\"}"),
            "partial_json 中跨 delta 标记应还原"
        );
    }

    #[test]
    fn test_sse_literal_marker_char_not_held_hostage() {
        let (plugin, _marker) = plugin_with_email_marker();
        let mut state = plugin.new_sse_state().unwrap();

        // 字面文本中的孤立 ⟦ 不是可能的标记前缀：下一 delta 到达即原样发出
        let mut data = text_delta_data(0, "math ⟦");
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "math ");
        assert!(!state
            .downcast_mut::<SseStreamState>()
            .unwrap()
            .carries
            .is_empty());

        let mut data = text_delta_data(0, "x⟧ y");
        assert!(plugin
            .transform_sse_event(
                &sse_ctx(),
                Some("content_block_delta"),
                &mut data,
                state.as_mut()
            )
            .unwrap());
        assert_eq!(delta_text(&data), "⟦x⟧ y", "字面 ⟦ 不应长期滞留");
    }

    #[test]
    fn test_sse_block_key_derivation() {
        let claude = json!({
            "type": "content_block_delta", "index": 3,
            "delta": {"type": "text_delta", "text": "x"}
        });
        assert_eq!(sse_block_key(&claude), "text_delta:3");
        let codex = json!({
            "type": "response.output_text.delta", "output_index": 0, "content_index": 1,
            "delta": "x"
        });
        assert_eq!(sse_block_key(&codex), "response.output_text.delta:1");
        assert_eq!(sse_block_key(&json!({"foo": 1})), "");
        assert_eq!(
            sse_block_key(&json!({"type": "message_stop"})),
            "message_stop"
        );
    }

    #[test]
    fn test_withhold_incomplete_marker_positions() {
        // 完整标记不扣留
        assert_eq!(withhold_incomplete_marker("a ⟦PII|id|L|D⟧ b"), None);
        // 未闭合的 ⟦PII| 前缀：从最后一个 ⟦ 起扣下
        assert_eq!(withhold_incomplete_marker("a ⟦PII|id|L"), Some("a ".len()));
        // 孤立 ⟦ 是 ⟦PII| 的前缀：扣下
        assert_eq!(withhold_incomplete_marker("text ⟦"), Some("text ".len()));
        // 非标记前缀的 ⟦ 后缀：不扣留
        assert_eq!(withhold_incomplete_marker("text ⟦x"), None);
        // ⟦ 之后已有 ⟧（即便后面还有别的）：最后一个 ⟦ 已闭合则不扣
        assert_eq!(withhold_incomplete_marker("⟦x⟧"), None);
        // 完整标记后再出现未闭合前缀
        assert_eq!(
            withhold_incomplete_marker("⟦PII|a|L|D⟧⟦PII|b"),
            Some("⟦PII|a|L|D⟧".len())
        );
        // 孤立 ⟦（完整字符）按"⟦PII| 前缀"规则扣下
        assert_eq!(withhold_incomplete_marker("a\u{27E6}"), Some("a".len()));
    }

    // --- 标记区守卫（防二次遮罩） ---

    #[test]
    fn test_marker_zone_guard_prevents_double_masking() {
        let plugin = plugin_with_rules(kv_secret_rules());

        // 残缺 id（末尾 XX）的标记文本：形状仍是标记，必须原样保留，
        // 不得被 kv-secret 规则把 "token = ⟦…⟧" 整段再包一层新标记
        let text = "jwt_token = \u{27E6}PII|aa710a41XX|TOKEN|JWT\u{27E7}";
        let (replaced, changed) = plugin.replace_text(text);
        assert!(!changed, "标记区文本不应被再次替换");
        assert_eq!(replaced, text);
        // 映射表不得为标记文本本身建立条目
        assert!(plugin.mapping_is_empty(), "标记原文不得入库");
    }

    #[test]
    fn test_marker_zone_guard_keeps_adjacent_real_value_replacement() {
        let plugin = plugin_with_rules(kv_secret_rules());

        // 同串里既有标记又有真实敏感值：只替换真实值，标记原样保留
        let text = "\u{27E6}PII|aa710a41XX|TOKEN|JWT\u{27E7} 联系 a@b.com";
        let (replaced, changed) = plugin.replace_text(text);
        assert!(changed);
        assert!(
            replaced.contains("\u{27E6}PII|aa710a41XX|TOKEN|JWT\u{27E7}"),
            "标记区不应被改动"
        );
        assert!(!replaced.contains("a@b.com"));
    }

    #[test]
    fn test_kv_secret_word_boundary_avoids_mid_key_match() {
        let plugin = plugin_with_rules(kv_secret_rules());

        // "my_token" 的 token 前无词边界，不应从键名中间命中（否则替换后残留 "my_"）
        let text = "my_token = 0123456789abcdef";
        let (replaced, changed) = plugin.replace_text(text);
        assert!(!changed, "词边界应阻止键名中间匹配: {replaced}");
        assert_eq!(replaced, text);

        // 正常键名整行替换
        let (replaced, changed) = plugin.replace_text("deploy_secret = 0123456789abcdef");
        assert!(changed);
        assert!(
            replaced.starts_with('\u{27E6}'),
            "整行（含键名）应被标记替换: {replaced}"
        );
    }

    // --- 标记协议说明注入 ---

    #[test]
    fn test_marker_prompt_injected_into_claude_string_system() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({"model": "m", "system": "You are a coding agent.", "messages": []});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let system = body["system"].as_str().unwrap();
        assert!(system.starts_with("You are a coding agent."));
        assert!(system.contains("隐私标记协议"));
        assert!(
            system.contains("⟦PII|<id>|<label>|<描述>⟧"),
            "应含标记形状示例"
        );
        assert!(
            system.contains("十六进制转储"),
            "应含转储遮蔽说明（第 6 条）"
        );
    }

    #[test]
    fn test_marker_prompt_injected_into_claude_blocks_system() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "model": "m",
            "system": [{"type": "text", "text": "base", "cache_control": {"type": "ephemeral"}}],
            "messages": []
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let blocks = body["system"].as_array().unwrap();
        assert_eq!(blocks.len(), 2, "应追加而非覆盖既有 blocks");
        assert_eq!(
            blocks[0]["text"],
            json!("base"),
            "既有 block（含 cache_control）不动"
        );
        assert!(blocks[1]["text"].as_str().unwrap().contains("隐私标记协议"));
    }

    #[test]
    fn test_marker_prompt_injected_when_system_absent() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({"model": "m", "messages": []});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let blocks = body["system"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0]["text"].as_str().unwrap().contains("隐私标记协议"));
    }

    #[test]
    fn test_marker_prompt_injected_into_codex_instructions() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({"model": "m", "instructions": "base instructions", "input": []});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let instructions = body["instructions"].as_str().unwrap();
        assert!(instructions.starts_with("base instructions"));
        assert!(instructions.contains("隐私标记协议"));
        assert!(body.get("system").is_none(), "不应错误创建 claude 形状字段");
    }

    #[test]
    fn test_marker_prompt_injected_into_gemini_system_instruction() {
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({
            "contents": [],
            "systemInstruction": {"parts": [{"text": "gemini base"}]}
        });
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let parts = body["systemInstruction"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[1]["text"].as_str().unwrap().contains("隐私标记协议"));
    }

    #[test]
    fn test_marker_prompt_note_example_marker_not_re_masked() {
        // 注入发生在走查之后：说明文本里的示例标记形状不应被替换或入库
        let plugin = plugin_with_rules(test_rules());
        let mut body = json!({"model": "m", "system": "base", "messages": []});
        plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap();
        let system = body["system"].as_str().unwrap();
        assert!(
            system.contains("⟦PII|<id>|<label>|<描述>⟧"),
            "示例标记应原样保留"
        );
        let store = plugin.state.store.lock().unwrap();
        assert!(
            store
                .by_original
                .keys()
                .all(|original| !original.contains("隐私标记协议")),
            "说明文本本身不得成为映射原文"
        );
    }

    // --- 缓存 ---

    #[test]
    fn test_cache_hit_skips_rule_evaluation() {
        let plugin = plugin_with_rules(test_rules());
        let text = "mail a@b.com now";

        let (first, changed1) = plugin.replace_text(text);
        assert!(changed1);
        let runs_after_first = plugin.state.replace_runs.load(Ordering::Relaxed);
        assert_eq!(runs_after_first, 1);

        let (second, changed2) = plugin.replace_text(text);
        assert_eq!(first, second);
        assert!(changed2, "缓存命中时 changed 语义不变");
        assert_eq!(
            plugin.state.replace_runs.load(Ordering::Relaxed),
            runs_after_first,
            "同串二次处理应直接命中缓存，检测不重跑"
        );

        // 不同串重新走检测
        plugin.replace_text("other a@b.com");
        assert_eq!(plugin.state.replace_runs.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_config_change_invalidates_cache() {
        let plugin = plugin_with_rules(test_rules());
        let text = "mail a@b.com now";
        let (replaced_v1, _) = plugin.replace_text(text);
        assert!(replaced_v1.contains("|EMAIL|"));

        // 配置变更（label 改为 WORK）：指纹变化 → 同串重新解析并产出新标记
        let docs = docs_with_rules(test_rules());
        let mut new_docs = docs;
        new_docs[RULES_DOC_FILE]["rules"][1]["label"] = json!("WORK");
        plugin.apply_docs(&new_docs);

        let runs_before = plugin.state.replace_runs.load(Ordering::Relaxed);
        let (replaced_v2, _) = plugin.replace_text(text);
        assert_eq!(
            plugin.state.replace_runs.load(Ordering::Relaxed),
            runs_before + 1,
            "指纹变化后同串应重新解析"
        );
        assert!(replaced_v2.contains("|WORK|"));
        assert_ne!(replaced_v1, replaced_v2);
    }

    #[test]
    fn test_cache_evicts_half_when_full() {
        let mut cache = ReplaceCache::default();
        for i in 0..16 {
            cache.put(format!("key-{i}"), format!("value-{i}"), 16);
        }
        assert_eq!(cache.map.len(), 16);
        cache.put("overflow".to_string(), "v".to_string(), 16);
        // 写满清一半 + 新增 1 条
        assert_eq!(cache.map.len(), 16 / 2 + 1);
        assert!(cache.map.contains_key("overflow"));
        // 最旧的一半被淘汰
        assert!(!cache.map.contains_key("key-0"));
        assert!(cache.map.contains_key("key-15"));
    }

    // --- 十六进制转储防护 ---

    /// xxd 输出样例：16 字节 "aaaa@b.com  text"
    const XXD_LINE: &str = "00000000: 6161 6161 4062 2e63 6f6d 2020 7465 7874  aaaa@b.com  text";

    #[test]
    fn test_hexdump_guard_masks_email_in_dump() {
        let plugin = plugin_with_rules(test_rules());
        let (replaced, changed) = plugin.replace_text(XXD_LINE);
        assert!(changed, "转储行中的邮箱应被抹除");
        assert!(
            !replaced.contains("aaaa@b.com"),
            "hex+ASCII 双列应同时抹除: {replaced}"
        );
        assert!(
            !replaced.contains("6161 6161 4062"),
            "hex 列应抹为 xx: {replaced}"
        );
        assert!(
            replaced.contains("2020 7465 7874"),
            "未命中的字节保持精确: {replaced}"
        );
        assert!(replaced.contains("xxxx"), "hex 列替换为 xx");
        // 抹除不可还原：不产生标记映射
        assert!(plugin.mapping_is_empty(), "转储抹除不走标记映射");
        assert!(!replaced.contains(MARKER_PREFIX));
    }

    #[test]
    fn test_hexdump_guard_disabled_passes_through() {
        // 防护关闭：hex 列不再抹除（ASCII 列的邮箱仍走常规标记替换——这正是
        // 防护要堵的泄漏面：hex 列是文本正则不可见的另一编码）
        let mut docs = docs_with_rules(test_rules());
        docs[CONFIG_DOC_FILE]["enable_hexdump_guard"] = json!(false);
        let plugin = plugin_with_docs(Arc::new(Database::memory().unwrap()), &docs);
        let (replaced, changed) = plugin.replace_text(XXD_LINE);
        assert!(changed, "ASCII 列邮箱走常规规则");
        assert!(
            replaced.contains("6161 6161 4062"),
            "hex 列未抹除（泄漏面）：{replaced}"
        );
        assert!(!replaced.contains("xxxx"), "不应有 xx 抹除痕迹");
        assert!(!replaced.contains("aaaa@b.com"), "ASCII 列已标记化");
    }

    #[test]
    fn test_hexdump_guard_keeps_normal_text_with_double_space() {
        // 含双空格但非转储的普通文本不受影响
        let plugin = plugin_with_rules(test_rules());
        let text = "hello  world mail a@b.com";
        let (replaced, changed) = plugin.replace_text(text);
        assert!(changed, "邮箱规则照常");
        assert!(replaced.contains(MARKER_PREFIX));
        assert!(
            replaced.starts_with("hello  world mail ⟦PII|"),
            "普通文本不被误伤"
        );
    }

    #[test]
    fn test_hexdump_multiline_block() {
        let plugin = plugin_with_rules(test_rules());
        // 两行连续转储，敏感值在第二行（IP 10.0.0.1 → 31 30 2e 30 2e 30 2e 31）
        let line2_payload = "ip 10.0.0.1 xyz";
        let mut hex_part = String::new();
        for b in line2_payload.bytes() {
            let _ = write!(hex_part, "{b:02x} ");
        }
        let hex_part = hex_part.trim_end().to_string();
        let text = format!(
            "00000000: {}  {}\n00000010: {}  {}\n",
            "6161 6161 6161 6161 6161 6161 6161 6161", "aaaaaaaaaaaaaaaa", hex_part, line2_payload
        );
        let (replaced, changed) = plugin.replace_text(&text);
        assert!(changed);
        assert!(
            !replaced.contains("10.0.0.1"),
            "第二行的 IP 应被抹除: {replaced}"
        );
        assert!(
            replaced.contains("aaaaaaaaaaaaaaaa"),
            "第一行 padding 无规则命中，应原样保留: {replaced}"
        );
    }

    // --- 配置校验器（Rust 版 _validate_*） ---

    #[test]
    fn test_validate_config_doc() {
        let mut doc = json!({
            "enable_regex": 1,
            "prompt_note": "yes",
            "cache_capacity": 4,
            "hash": { "algorithm": "md5", "mode": "wrong", "length": 2 }
        });
        assert!(
            validate_config_doc(&mut doc).is_err(),
            "非法 algorithm 拒绝"
        );

        let mut doc = json!({
            "enable_regex": 1,
            "prompt_note": "yes",
            "cache_capacity": 4,
            "hash": { "algorithm": "sha256", "mode": "fixed", "length": 2 }
        });
        validate_config_doc(&mut doc).unwrap();
        let obj = doc.as_object().unwrap();
        assert_eq!(obj["enable_regex"], json!(true), "数值真值归一化");
        assert_eq!(obj["prompt_note"], json!(true), "非空串归一化为 true");
        assert_eq!(obj["cache_capacity"], json!(16), "下限 16");
        assert_eq!(obj["hash"]["length"], json!(4), "length 下限 4");
        // 缺省字段不写入
        assert!(obj.get("enable_hexdump_guard").is_none());
    }

    #[test]
    fn test_validate_rules_doc() {
        // literal 逗号串 → 数组；regex 的 values 丢弃；literal 的 pattern 丢弃
        let mut doc = json!({
            "enabled": true,
            "rules": [
                { "kind": "literal", "name": "a", "values": "x， y ,,z", "label": "L" },
                { "kind": "regex", "name": "b", "pattern": "[a-z]+", "values": ["stale"], "label": "R" },
                { "kind": "regex", "name": "c", "pattern": "([bad", "label": "R" },
                { "kind": "wrong", "name": "d", "label": "W" },
                { "kind": "regex", "name": "e", "pattern": "x", "capture": 99, "label": "C" }
            ]
        });
        assert!(validate_rules_doc(&mut doc).is_err(), "非法 pattern 拒绝");

        let mut doc = json!({
            "enabled": true,
            "rules": [
                { "kind": "literal", "name": "a", "values": "x， y ,,z", "label": "L" },
                { "kind": "regex", "name": "b", "pattern": "[a-z]+", "values": ["stale"], "label": "R" }
            ]
        });
        validate_rules_doc(&mut doc).unwrap();
        let rules = doc["rules"].as_array().unwrap();
        assert_eq!(
            rules[0]["values"],
            json!(["x", "y", "z"]),
            "全角逗号拆分 + 去空"
        );
        assert!(rules[0].get("pattern").is_none(), "literal 的 pattern 丢弃");
        assert!(rules[1].get("values").is_none(), "regex 的 values 丢弃");
        assert_eq!(rules[1]["priority"], json!(20), "priority 缺省 20");

        // capture 类型非法
        let mut doc = json!({
            "enabled": true,
            "rules": [{ "kind": "regex", "name": "a", "pattern": "x", "capture": 1.5, "label": "C" }]
        });
        assert!(
            validate_rules_doc(&mut doc).is_err(),
            "浮点 capture 拒绝（Python int 语义）"
        );

        // rules 缺失
        let mut doc = json!({"enabled": true});
        assert!(validate_rules_doc(&mut doc).is_err());
    }

    #[test]
    fn test_validate_custom_doc() {
        let mut doc = json!({
            "enabled": true,
            "values": [
                { "value": "  ", "label": "X" },
                { "value": "ok", "priority": 0.9 }
            ]
        });
        assert!(validate_custom_doc(&mut doc).is_err(), "空白 value 拒绝");

        let mut doc = json!({
            "values": [{ "value": "ok", "priority": 2.9 }]
        });
        validate_custom_doc(&mut doc).unwrap();
        let obj = doc.as_object().unwrap();
        assert_eq!(obj["enabled"], json!(true), "enabled 缺省 true");
        assert_eq!(obj["values"][0]["priority"], json!(2), "浮点截断");
        assert_eq!(
            obj["values"][0]["value"],
            json!("ok"),
            "value 原样保留（不 strip）"
        );
    }

    // --- 面板配置协议（config_read / config_write） ---

    #[test]
    fn test_first_startup_materializes_default_docs() {
        let db = Arc::new(Database::memory().unwrap());
        let plugin = BuiltinPrivacyPlugin::new(db.clone());

        // 无存档时物化默认配置（含默认规则集）
        let docs = load_docs_from(&db).unwrap().expect("默认配置应已落库");
        let rules = &docs[RULES_DOC_FILE]["rules"];
        assert!(rules.as_array().unwrap().len() >= 10, "默认规则集应存在");
        let names: Vec<&str> = rules
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["name"].as_str())
            .collect();
        assert!(names.contains(&"kv-secret"));
        assert!(names.contains(&"email"));

        // 默认规则全部可编译（含 (?P=q) 反引用与前后查找断言）
        let snapshot = plugin.current_snapshot();
        assert!(!snapshot.rules.is_empty());
        assert!(snapshot.enable_regex);
        assert!(snapshot.prompt_note);
        assert!(snapshot.enable_hexdump_guard);

        // 默认规则可用：邮箱命中 + kv capture 只遮值
        let mut body = json!({"messages": [{"content": [{"text": "api_key = \"abcd1234efgh5678\" mail a@b.com"}]}]});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("api_key = \"⟦PII|"), "kv 规则只遮值: {text}");
        assert!(!text.contains("abcd1234"));
        assert!(!text.contains("a@b.com"));
    }

    #[test]
    fn test_config_read_write_roundtrip() {
        let db = Arc::new(Database::memory().unwrap());
        let plugin = BuiltinPrivacyPlugin::new(db.clone());

        // read：三份文档齐全
        let docs = plugin.config_read().unwrap();
        for file in CONFIG_DOC_FILES {
            assert!(
                docs.get(file).map(Value::is_object).unwrap_or(false),
                "{file} 应为对象"
            );
        }

        // write：改规则 label + 新增特殊值
        let mut patched = docs.clone();
        patched[RULES_DOC_FILE]["rules"][0]["label"] = json!("RENAMED");
        patched[CUSTOM_DOC_FILE]["values"] = json!([
            { "value": "corp.internal", "label": "DOMAIN", "desc": "内网域名", "priority": 1, "enabled": true }
        ]);
        plugin.config_write(&patched).unwrap();

        // 落库验证
        let stored = load_docs_from(&db).unwrap().unwrap();
        assert_eq!(
            stored[RULES_DOC_FILE]["rules"][0]["label"],
            json!("RENAMED")
        );
        assert_eq!(
            stored[CUSTOM_DOC_FILE]["values"][0]["value"],
            json!("corp.internal")
        );

        // 即时生效：特殊值命中 RENAMED 邮箱规则照常
        let mut body = json!({"messages": [{"content": [{"text": "visit corp.internal"}]}]});
        assert!(plugin
            .transform_request(&pre_request_ctx(), &mut body)
            .unwrap());
        let text = body["messages"][0]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("|DOMAIN|"), "保存即生效: {text}");
        assert!(text.contains("⟦PII|"), "标记已生成");

        // 未列字段原样保留（comment）
        assert_eq!(
            stored[RULES_DOC_FILE]["rules"][0]["comment"],
            patched[RULES_DOC_FILE]["rules"][0]["comment"],
            "schema 未覆盖的字段保存时原样保留"
        );
    }

    #[test]
    fn test_config_write_rejects_bad_input() {
        let plugin = BuiltinPrivacyPlugin::new(Arc::new(Database::memory().unwrap()));

        // 未知文件
        let err = plugin
            .config_write(&json!({ "other.json": {} }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("不支持的配置文件"), "{err}");

        // 坏正则整批拒绝
        let docs = plugin.config_read().unwrap();
        let mut patched = docs.clone();
        patched[RULES_DOC_FILE]["rules"][0]["pattern"] = json!("([bad");
        let err = plugin.config_write(&patched).unwrap_err().to_string();
        assert!(err.contains("正则无效"), "{err}");
        // 拒绝后配置未变
        let after = plugin.config_read().unwrap();
        assert_ne!(after[RULES_DOC_FILE]["rules"][0]["pattern"], json!("([bad"));

        // 非 JSON 对象
        let err = plugin
            .config_write(&json!({ RULES_DOC_FILE: [1] }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("必须是 JSON 对象"), "{err}");
    }

    // --- pattern 适配 ---

    #[test]
    fn test_adapt_python_pattern() {
        assert_eq!(
            adapt_python_pattern("(?P<q>[\"'`]?)(?P<v>x)(?P=q)"),
            "(?P<q>[\"'`]?)(?P<v>x)\\k<q>"
        );
        assert_eq!(
            adapt_python_pattern("(?i)abc"),
            "(?i)abc",
            "无反引用时原样返回"
        );
        assert_eq!(adapt_python_pattern("a(?P=name)b"), "a\\k<name>b");
    }

    // --- 插件元信息与注册表集成 ---

    #[test]
    fn test_plugin_metadata() {
        let plugin = BuiltinPrivacyPlugin::new(Arc::new(Database::memory().unwrap()));
        assert_eq!(plugin.id(), "builtin:privacy-replace");
        assert_eq!(plugin.display_name(), "隐私替换");
        assert!(plugin.is_builtin());
        assert_eq!(
            plugin.stages(),
            &[
                PluginStage::PreRequest,
                PluginStage::PostResponse,
                PluginStage::SseChunk
            ]
        );
        assert_eq!(plugin.default_priority(), 100);
        assert!(plugin.default_enabled());
        assert_eq!(plugin.version(), None);
        assert_eq!(plugin.source(), None);
        assert_eq!(plugin.config_title(), Some("隐私替换设置"));
        assert!(
            !plugin.config_schema().is_empty(),
            "声明 schema 后面板出现设置按钮"
        );
        // schema 的 file 键与 config 协议文档键一致
        for item in plugin.config_schema() {
            assert!(
                CONFIG_DOC_FILES.contains(&item.file.as_str()),
                "{}",
                item.file
            );
        }
    }

    #[test]
    fn test_registry_pipeline_end_to_end() {
        // 注册表集成：PreRequest 替换 → PostResponse 还原，经 run_request_pipeline 全链路
        let db = Arc::new(Database::memory().unwrap());
        let registry = super::super::registry::PluginRegistry::new();
        registry.register(Arc::new(BuiltinPrivacyPlugin::new(db)));
        assert_eq!(
            registry
                .plugins_for_stage(PluginStage::PreRequest)
                .iter()
                .map(|p| p.id().to_string())
                .collect::<Vec<_>>(),
            vec!["builtin:privacy-replace".to_string()]
        );

        let mut body = json!({"messages": [{"content": [{"text": "mail a@b.com"}]}]});
        assert!(super::super::registry::run_request_pipeline(
            &registry,
            PluginStage::PreRequest,
            &pre_request_ctx(),
            &mut body,
            |p, c, b| p.transform_request(c, b),
        ));
        let marked = body["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!marked.contains("a@b.com"));

        let mut response = json!({"content": [{"text": marked}]});
        assert!(super::super::registry::run_request_pipeline(
            &registry,
            PluginStage::PostResponse,
            &post_response_ctx(),
            &mut response,
            |p, c, b| p.transform_response(c, b),
        ));
        assert_eq!(response["content"][0]["text"], json!("mail a@b.com"));
    }

    // --- 单串替换便捷路径（测试用） ---

    impl BuiltinPrivacyPlugin {
        /// 单串替换（测试路径）：走批量管线
        fn replace_text(&self, text: &str) -> (String, bool) {
            let snapshot = self.current_snapshot();
            let owned = text.to_string();
            let replaced = self
                .compute_replacements(&self.db, std::slice::from_ref(&owned), &snapshot)
                .remove(text)
                .unwrap_or_else(|| text.to_string());
            let changed = replaced != text;
            (replaced, changed)
        }
    }
}
