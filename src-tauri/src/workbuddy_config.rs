//! WorkBuddy 配置文件读写模块
//!
//! 处理 `~/.workbuddy/models.json` 配置文件的读写操作（纯 JSON 数组格式）。
//!
//! WorkBuddy 的 models.json 是一个**扁平的模型数组**，每个元素形如：
//! ```json
//! {
//!   "id": "glm-5.1", "model": "glm-5.1", "name": "GLM 5.1", "vendor": "Custom",
//!   "url": "https://api.example.com/v1", "apiKey": "sk-xxx",
//!   "supportsToolCall": true, "supportsImages": true, "supportsReasoning": true,
//!   "bypassProxy": true, "useCustomProtocol": false,
//!   "contextWindow": 200000, "maxTokens": 128000
//! }
//! ```
//!
//! 由于 CC-Switch 以「provider（服务商）」为管理单元，而 WorkBuddy 没有 provider
//! 分组概念，本模块采用「**按网关/凭据聚合**」策略：
//!   - 把 `url` + `apiKey` 相同的模型聚合为一个 provider（id 由调用方指定，如 "alpha"）。
//!   - provider 的 settings_config 形如 `{ baseUrl, apiKey, models: [ {id,name,...} ] }`。
//!   - 写 live 时：把 DB 里所有 WorkBuddy provider 的 models 展平，为每条模型注入其
//!     provider 的 url/apiKey，聚合成一个数组写回 models.json（additive 模式）。
//!   - 回填(backfill)时：读 models.json，按 (url, apiKey) 分组反推 provider。

use crate::config::get_home_dir;
use crate::error::AppError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 WorkBuddy 配置目录，默认 `~/.workbuddy/`
pub fn get_workbuddy_dir() -> PathBuf {
    get_home_dir().join(".workbuddy")
}

/// 获取 WorkBuddy 模型配置文件路径 `~/.workbuddy/models.json`
///
/// 注意：该文件可能是软链（用户常把它软链到云盘做同步），
/// `atomic_write_preserve_symlink` 内部会 `fs::canonicalize` 解析软链真实目标后写入，
/// 从而**保留软链**（不像上游 `config::atomic_write` 会用 tmp+rename 破坏软链）。
pub fn get_workbuddy_models_path() -> PathBuf {
    get_workbuddy_dir().join("models.json")
}

fn workbuddy_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

// ============================================================================
// Type Definitions
// ============================================================================

/// 单条模型（对应 models.json 数组里的一个元素）。
///
/// 已知字段显式列出，其余（supportsToolCall / contextWindow / reasoning 等）
/// 通过 `#[serde(flatten)] extra` 原样保留，避免升级 WorkBuddy 后字段丢失。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyModelEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 网关地址。写 live 时由 provider 注入；作为独立模型对象时应存在。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// API Key。写 live 时由 provider 注入。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// 其余能力标记与元数据原样透传（camelCase）。
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// 一个 WorkBuddy provider（= 一个网关：base_url + api_key + 若干模型）。
/// 存于 DB 的 `settings_config`。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkBuddyProviderConfig {
    /// 网关地址，如 `https://api.example.com/v1`
    pub base_url: String,
    /// 该网关的 API Key
    pub api_key: String,
    /// 该网关下的模型清单
    #[serde(default)]
    pub models: Vec<WorkBuddyModelEntry>,
}

// ============================================================================
// Raw file read/write
// ============================================================================

/// 读取 models.json，返回模型数组（原始 JSON）。文件不存在时返回空数组。
pub fn read_models() -> Result<Vec<Value>, AppError> {
    let path = get_workbuddy_models_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| AppError::Config(format!("读取 models.json 失败: {e}")))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_str(trimmed)
        .map_err(|e| AppError::Config(format!("解析 models.json 失败: {e}")))?;
    match value {
        Value::Array(arr) => Ok(arr),
        // 容错：个别版本可能包了一层 { "models": [...] }
        Value::Object(obj) => Ok(obj
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()),
        _ => Ok(Vec::new()),
    }
}

/// 原子写入模型数组到 models.json（**保留软链**、pretty 打印）。
///
/// 关键：用户常把 models.json 软链到云盘目录。上游通用的 `config::atomic_write`
/// 会把 tmp 放在 `path.parent()` 后 rename，**会用实体文件替换软链本身**、破坏同步。
/// 这里先 canonicalize 解析出软链的真实目标，tmp 写在真实文件所在目录，
/// rename 到真实文件 —— 软链得以保留。
pub fn write_models(models: &[Value]) -> Result<(), AppError> {
    let _guard = workbuddy_write_lock().lock().unwrap();
    let path = get_workbuddy_models_path();
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(format!("创建 ~/.workbuddy 目录失败: {e}")))?;
        }
    }
    // 解析软链到真实文件；文件尚不存在时用原路径。
    let real: PathBuf = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let content = serde_json::to_string_pretty(&Value::Array(models.to_vec()))
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_preserve_symlink(&real, format!("{content}\n").as_bytes())?;
    Ok(())
}

/// 软链安全的原子写：tmp 与目标同目录，写完 rename 覆盖真实文件。
fn atomic_write_preserve_symlink(real_path: &Path, data: &[u8]) -> Result<(), AppError> {
    let parent = real_path
        .parent()
        .ok_or_else(|| AppError::Config("models.json 路径无 parent".to_string()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::Config(format!("创建目录失败: {e}")))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp = parent.join(format!("models.json.tmp.{ts}"));
    {
        let mut f = fs::File::create(&tmp)
            .map_err(|e| AppError::Config(format!("创建临时文件失败: {e}")))?;
        f.write_all(data)
            .map_err(|e| AppError::Config(format!("写临时文件失败: {e}")))?;
        f.flush()
            .map_err(|e| AppError::Config(format!("flush 失败: {e}")))?;
    }
    fs::rename(&tmp, real_path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        AppError::Config(format!("原子替换失败: {e}"))
    })?;
    Ok(())
}

// ============================================================================
// Provider Functions (Typed) — 按 (url, apiKey) 聚合
// ============================================================================

/// 从 live models.json 读出、按 (url, apiKey) 聚合成 provider 集合。
///
/// provider id 规则：优先取 url 的 host 段（如 `api.alpha.test`），
/// 冲突时追加序号。返回 IndexMap 保持稳定顺序。
pub fn get_typed_providers() -> Result<IndexMap<String, WorkBuddyProviderConfig>, AppError> {
    let models = read_models()?;
    // key = (url, apiKey)
    let mut buckets: IndexMap<(String, String), WorkBuddyProviderConfig> = IndexMap::new();

    for m in models {
        let entry: WorkBuddyModelEntry = match serde_json::from_value(m.clone()) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("跳过无法解析的 WorkBuddy 模型条目: {e}");
                continue;
            }
        };
        let url = entry.url.clone().unwrap_or_default();
        let api_key = entry.api_key.clone().unwrap_or_default();
        if url.is_empty() {
            log::warn!("跳过缺少 url 的 WorkBuddy 模型 '{}'", entry.id);
            continue;
        }
        let bucket = buckets
            .entry((url.clone(), api_key.clone()))
            .or_insert_with(|| WorkBuddyProviderConfig {
                base_url: url.clone(),
                api_key: api_key.clone(),
                models: Vec::new(),
            });
        // provider 内的模型条目去掉 url/apiKey（由 provider 统一持有），保留其余
        let mut clean = entry;
        clean.url = None;
        clean.api_key = None;
        bucket.models.push(clean);
    }

    // (url, apiKey) → 稳定 provider id
    let mut result: IndexMap<String, WorkBuddyProviderConfig> = IndexMap::new();
    for ((url, _key), cfg) in buckets {
        let mut base_id = provider_id_from_url(&url);
        let mut id = base_id.clone();
        let mut n = 2;
        while result.contains_key(&id) {
            id = format!("{base_id}-{n}");
            n += 1;
        }
        // 避免未使用告警
        base_id.clear();
        result.insert(id, cfg);
    }
    Ok(result)
}

/// 由 url 推导一个人类可读的 provider id（取 host，去掉端口和常见前缀）。
fn provider_id_from_url(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("workbuddy")
        .split(':')
        .next()
        .unwrap_or("workbuddy");
    // api.alpha.test → alpha；api.beta.test → beta
    let parts: Vec<&str> = host.split('.').collect();
    let candidate = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        host
    };
    if candidate.is_empty() {
        "workbuddy".to_string()
    } else {
        candidate.to_string()
    }
}

// ============================================================================
// Live sync — 展平所有 provider 的模型写回 models.json
// ============================================================================

/// 把「provider id → provider 配置」整体展平写回 models.json。
///
/// 每条模型注入其 provider 的 url/apiKey。这是 additive 模式下的全量写入，
/// 由 `sync_all_providers_to_live` 收集所有 WorkBuddy provider 后调用。
pub fn write_all_providers(
    providers: &IndexMap<String, WorkBuddyProviderConfig>,
) -> Result<(), AppError> {
    let mut flat: Vec<Value> = Vec::new();
    for (_pid, cfg) in providers {
        for m in &cfg.models {
            let mut model = m.clone();
            model.url = Some(cfg.base_url.clone());
            model.api_key = Some(cfg.api_key.clone());
            // model 字段缺省时用 id 兜底
            if model.model.is_none() {
                model.model = Some(model.id.clone());
            }
            let v =
                serde_json::to_value(&model).map_err(|e| AppError::JsonSerialize { source: e })?;
            flat.push(v);
        }
    }
    write_models(&flat)
}

/// 单个 provider 的展平写入（additive：先读现有、按 provider 的模型 id 覆盖再写回）。
///
/// 供 `write_live_with_common_config` 的 WorkBuddy 分支使用：只更新该 provider
/// 名下的模型，不影响其他 provider 的模型。
pub fn set_typed_provider(cfg: &WorkBuddyProviderConfig) -> Result<(), AppError> {
    let mut current = get_typed_providers()?;
    // 用 base_url 定位同一 provider（同网关同 key）
    let key = provider_id_from_url(&cfg.base_url);
    current.insert(key, cfg.clone());
    write_all_providers(&current)
}

/// 删除某网关下全部模型（按 base_url 匹配）。
///
/// 目前 live 层删除走 `remove_provider_by_id`（按 CC-Switch provider id），
/// 本函数作为按 base_url 直删的公开 API 保留备用（deeplink/测试可能用到）。
#[allow(dead_code)]
pub fn remove_provider_by_base_url(base_url: &str) -> Result<(), AppError> {
    let mut current = get_typed_providers()?;
    let target = provider_id_from_url(base_url);
    current.shift_remove(&target);
    write_all_providers(&current)
}

/// 删除某个 provider（按 `get_typed_providers` 生成的 provider id 匹配）名下全部模型。
///
/// live 层的删除入口按 CC-Switch 的 provider id 调用，而 WorkBuddy 的 provider id
/// 由网关 host 推导（见 `provider_id_from_url` / `get_typed_providers` 去重逻辑），
/// 因此这里直接按 id 从聚合结果里剔除后整体回写。
pub fn remove_provider_by_id(provider_id: &str) -> Result<(), AppError> {
    let mut current = get_typed_providers()?;
    if current.shift_remove(provider_id).is_none() {
        log::debug!("WorkBuddy provider '{provider_id}' 不在 live 配置中，跳过删除");
        return Ok(());
    }
    write_all_providers(&current)
}

// ============================================================================
// Health check（轻量）
// ============================================================================

/// 校验 models.json 是否为合法 JSON 数组，返回问题描述列表（空=健康）。
pub fn scan_health() -> Vec<String> {
    let mut warnings = Vec::new();
    let path = get_workbuddy_models_path();
    if !path.exists() {
        warnings.push("~/.workbuddy/models.json 不存在（首次同步会自动创建）".to_string());
        return warnings;
    }
    match fs::read_to_string(&path) {
        Ok(content) => {
            if let Err(e) = serde_json::from_str::<Value>(content.trim()) {
                warnings.push(format!("models.json 不是合法 JSON: {e}"));
            }
        }
        Err(e) => warnings.push(format!("无法读取 models.json: {e}")),
    }
    warnings
}

// ============================================================================
// Tests — 用隔离 HOME 跑真实读写/聚合逻辑，验证 WorkBuddy 适配端到端正确
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::{Mutex, OnceLock};

    /// 串行化，避免并发改 CC_SWITCH_TEST_HOME / models.json
    fn guard() -> std::sync::MutexGuard<'static, ()> {
        static M: OnceLock<Mutex<()>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// 隔离 HOME：构造时准备 `~/.workbuddy/`，Drop 时恢复 `CC_SWITCH_TEST_HOME`
    /// 并清理临时目录，避免影响其他模块的并行测试。
    struct TestHome {
        path: PathBuf,
        previous: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TestHome {
        fn new() -> Self {
            let _guard = guard();
            let path = std::env::temp_dir().join(format!(
                "wb-cfg-test-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join(".workbuddy")).unwrap();
            let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", &path);
            Self {
                path,
                previous,
                _guard,
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn sample_models_json() -> &'static str {
        r#"[
          {"id":"glm-5.1","model":"glm-5.1","name":"GLM 5.1","url":"https://api.alpha.test/v1","apiKey":"sk-A","supportsToolCall":true,"contextWindow":200000},
          {"id":"kimi-k2.6","model":"kimi-k2.6","name":"Kimi K2.6","url":"https://api.alpha.test/v1","apiKey":"sk-A","supportsImages":true},
          {"id":"deepseek-v4","model":"deepseek-v4","name":"DeepSeek V4","url":"https://api.beta.test/v1","apiKey":"sk-B","maxTokens":8192}
        ]"#
    }

    #[test]
    #[serial]
    fn read_and_aggregate_by_gateway() {
        let home = TestHome::new();
        fs::write(
            home.path().join(".workbuddy/models.json"),
            sample_models_json(),
        )
        .unwrap();

        let models = read_models().unwrap();
        assert_eq!(models.len(), 3, "应读到 3 条模型");

        let providers = get_typed_providers().unwrap();
        // 两个网关 host 不同 → 2 个 provider，id 由 host 推导
        assert_eq!(providers.len(), 2, "应聚合成 2 个 provider");
        let alpha = providers.get("alpha").expect("alpha provider 存在");
        assert_eq!(alpha.base_url, "https://api.alpha.test/v1");
        assert_eq!(alpha.api_key, "sk-A");
        assert_eq!(alpha.models.len(), 2, "alpha 下挂 2 个模型");
        // provider 内的模型条目已剥离 url/apiKey（由 provider 统一持有）
        assert!(alpha.models[0].url.is_none());
        assert!(alpha.models[0].api_key.is_none());
        // 未知能力字段透传保留
        assert!(alpha.models[0].extra.contains_key("supportsToolCall"));

        let beta = providers.get("beta").expect("beta provider 存在");
        assert_eq!(beta.models.len(), 1);
        assert_eq!(beta.api_key, "sk-B");
    }

    #[test]
    #[serial]
    fn roundtrip_write_preserves_data_and_injects_credentials() {
        let home = TestHome::new();
        fs::write(
            home.path().join(".workbuddy/models.json"),
            sample_models_json(),
        )
        .unwrap();

        // 读出 → 原样写回 → 再读，模型数量与关键字段不丢
        let providers = get_typed_providers().unwrap();
        write_all_providers(&providers).unwrap();

        let after = read_models().unwrap();
        assert_eq!(after.len(), 3, "回写后仍是 3 条");
        // 每条重新注入了 url/apiKey
        for m in &after {
            assert!(m.get("url").and_then(Value::as_str).is_some(), "url 已回填");
            assert!(
                m.get("apiKey").and_then(Value::as_str).is_some(),
                "apiKey 已回填"
            );
        }
        // 聚合应与写回前一致（幂等）
        let providers2 = get_typed_providers().unwrap();
        assert_eq!(providers2.len(), providers.len());
    }

    #[test]
    #[serial]
    fn set_and_remove_provider() {
        let home = TestHome::new();
        fs::write(
            home.path().join(".workbuddy/models.json"),
            sample_models_json(),
        )
        .unwrap();

        // 新增第三个网关
        let new_model = WorkBuddyModelEntry {
            id: "gpt-x".to_string(),
            ..Default::default()
        };
        let cfg = WorkBuddyProviderConfig {
            base_url: "https://api.example.com/v1".to_string(),
            api_key: "sk-C".to_string(),
            models: vec![new_model],
        };
        set_typed_provider(&cfg).unwrap();
        let after_add = get_typed_providers().unwrap();
        assert_eq!(after_add.len(), 3, "新增后 3 个 provider");
        assert!(after_add.contains_key("example"));

        // 按 id 删除 alpha
        remove_provider_by_id("alpha").unwrap();
        let after_rm = get_typed_providers().unwrap();
        assert_eq!(after_rm.len(), 2, "删除 alpha 后剩 2 个");
        assert!(!after_rm.contains_key("alpha"));
        assert!(after_rm.contains_key("beta") && after_rm.contains_key("example"));
    }

    /// 软链保护逻辑依赖 Unix symlink 语义，Windows 下不参与。
    #[cfg(unix)]
    #[test]
    #[serial]
    fn write_preserves_symlink() {
        let home = TestHome::new();
        // 真实文件放别处，models.json 做软链指向它（模拟 iCloud 同步场景）
        let real_dir = home.path().join("cloud");
        fs::create_dir_all(&real_dir).unwrap();
        let real_file = real_dir.join("models.real.json");
        fs::write(&real_file, sample_models_json()).unwrap();
        let link = home.path().join(".workbuddy/models.json");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&real_file, &link).unwrap();

        // 触发一次写回
        let providers = get_typed_providers().unwrap();
        write_all_providers(&providers).unwrap();

        // models.json 仍是软链，且真实文件被更新
        let meta = fs::symlink_metadata(&link).unwrap();
        assert!(
            meta.file_type().is_symlink(),
            "写回后 models.json 仍应是软链"
        );
        let real_content = fs::read_to_string(&real_file).unwrap();
        assert!(real_content.contains("alpha.test"), "真实文件已被写入");
    }

    #[test]
    fn provider_id_from_url_rules() {
        assert_eq!(provider_id_from_url("https://api.alpha.test/v1"), "alpha");
        assert_eq!(provider_id_from_url("https://api.beta.test/v1"), "beta");
        assert_eq!(
            provider_id_from_url("http://localhost:8080/v1"),
            "localhost"
        );
    }

    #[test]
    #[serial]
    fn scan_health_detects_bad_json() {
        let home = TestHome::new();
        fs::write(
            home.path().join(".workbuddy/models.json"),
            "{not valid json",
        )
        .unwrap();
        let warnings = scan_health();
        assert!(!warnings.is_empty(), "非法 JSON 应产生告警");
    }
}
