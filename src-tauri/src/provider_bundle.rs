//! Provider Bundle 安装器
//!
//! 设计目的：把"一组按顺序参与故障转移的供应商"作为一个原子单元安装，
//! UI 一次点击即完成：
//!   1) 写入 providers 表（每条 `in_failover_queue=1`、`sort_index` 固定）；
//!   2) 开启该 AppType 的代理接管（`proxy_config.enabled=1`）；
//!   3) 开启 auto_failover；
//!   4) 把 P1 切到故障转移目标。
//!
//! 当前 SSOT 只支持「kaixuan」一个 bundle：双端点 `开轩 LLM 网关`
//! (`https://llm.kxpms.cn/v1`) + `本地 LLM 网关 (8782)` (`http://127.0.0.1:8782/v1`)。
//! 后续要加新 bundle 在 [`bundles`] 里加行即可，install 逻辑零改动。
//!
//! 与原 add_provider 命令的关系：bundle 是「复合」安装，原 add_provider
//! 是「单条」安装；两套互不替代——UI 选单条预设时仍走 add_provider。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::str::FromStr;
use tauri::{AppHandle, Manager};

use crate::app_config::AppType;
use crate::database::Database;
use crate::env_expand::{contains_placeholder, expand_required};
use crate::error::AppError;
use crate::provider::Provider;
use crate::store::AppState;

/// 安装请求入参。前端表单驱动：
/// - `bundle_id` 选 bundle 名（SSOT 在 [`bundles`]）；
/// - `app_type` 决定写哪张表（目前仅 Codex 实现）；
/// - `api_keys` 传用户填的密钥；如果 key 含 `$ENV` 占位符，在此展开。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallBundleRequest {
    pub bundle_id: String,
    pub app_type: String,
    /// key = 端点 role（`primary` / `secondary`），value = 用户填的密钥。
    /// role 不要求严格一一对应 bundle 内部的 spec，但写库时按
    /// spec 顺序消费 user_keys 中的 role → endpoint。
    #[serde(default)]
    pub api_keys: std::collections::HashMap<String, String>,
}

/// 安装结果。前端用它显示「装好哪些端点、谁是 P1」。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallBundleResult {
    pub bundle_id: String,
    pub app_type: String,
    pub installed_provider_ids: Vec<String>,
    pub primary_provider_id: String,
    pub auto_failover_enabled: bool,
    pub missing_env_vars: Vec<String>,
}

/// 单个端点 spec。`settings_config` 用 serde_json::Value 而非 String，因为
/// 不同 AppType（Codex/Claude/...）的 config 结构差异大；前端把要写入的
/// settings_config 整块 JSON 传进来，后端只做占位符展开与角色标记。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleEndpointSpec {
    pub role: String, // "primary" | "secondary"
    pub provider_id: String, // 稳定 id（如 "kaixuan-kxpms"），由 bundle_id + role 派生
    pub name: String,
    pub website_url: Option<String>,
    pub icon: Option<String>,
    pub icon_color: Option<String>,
    pub category: Option<String>,
    pub settings_config: Value,
    pub api_key_field: String, // 在 settings_config 里哪个字段要展开（例: "auth.OPENAI_API_KEY"）
}

/// Bundle 描述。`endpoints` 顺序即故障转移顺序：index 0 是 P1。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleSpec {
    pub id: String,
    pub display_name: String,
    pub app_type: String,
    pub endpoints: Vec<BundleEndpointSpec>,
}

/// 列出所有已注册 bundle。前端用它渲染「一键装」选择器。
pub fn list_bundles() -> Vec<BundleSpecView> {
    bundles()
        .into_iter()
        .map(|b| BundleSpecView {
            id: b.id,
            display_name: b.display_name,
            app_type: b.app_type,
            endpoint_count: b.endpoints.len(),
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSpecView {
    pub id: String,
    pub display_name: String,
    pub app_type: String,
    pub endpoint_count: usize,
}

pub fn get_bundle(id: &str) -> Option<BundleSpec> {
    bundles().into_iter().find(|b| b.id == id)
}

/// 列出当前 AppType 下已存在的 provider IDs（用于判断 spec 中的 id 是否冲突）。
fn existing_provider_ids(db: &Database, app_type: &str) -> Result<Vec<String>, AppError> {
    Ok(db
        .get_all_providers(app_type)?
        .into_keys()
        .collect::<Vec<_>>())
}

/// 安装入口（异步，内部函数，便于命令层 + 单测共用）。
///
/// 原子性：当前实现是「单线程顺序写入，每步独立事务」。如果中间失败，
/// 已写入的 provider 会保留——后续重试会走「id 冲突就跳过」路径，
/// 不会重复创建（见 [`provision_endpoint`]）。这种弱原子性是当前
/// `Database` API 的能力上限；要真原子事务需要重写 DAO 层，本 PR 不做。
pub async fn install_bundle_internal(
    db: &Database,
    req: &InstallBundleRequest,
) -> Result<InstallBundleResult, AppError> {
    let spec = get_bundle(&req.bundle_id)
        .ok_or_else(|| AppError::Config(format!("未知 bundle: {}", req.bundle_id)))?;
    if spec.app_type != req.app_type {
        return Err(AppError::Config(format!(
            "bundle '{}' 不支持 app_type '{}'（只支持 '{}')",
            spec.id, req.app_type, spec.app_type
        )));
    }

    // 1) 展开所有 endpoint 的密钥字段，收集 missing 占位符
    let mut missing_env_vars: Vec<String> = Vec::new();
    let mut expanded_endpoints: Vec<(BundleEndpointSpec, String)> = Vec::new();
    for ep in spec.endpoints.iter() {
        let mut ep = ep.clone();
        let key_input = req
            .api_keys
            .get(&ep.role)
            .cloned()
            .unwrap_or_default();
        let expanded = expand_endpoint_key(&key_input, &mut missing_env_vars);
        ep.settings_config = apply_expanded_key(&ep.settings_config, &ep.api_key_field, &expanded);
        expanded_endpoints.push((ep, expanded));
    }

    // 2) 写入 providers。id 已存在 → 仅刷新 membership（不覆盖用户已改的
    //    settings_config，避免擦掉本地 api_key）；id 不存在 → 整条插入。
    let existing = existing_provider_ids(db, &req.app_type)?;
    let base_index = db
        .get_all_providers(&req.app_type)?
        .values()
        .filter_map(|p| p.sort_index)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);
    let mut installed_ids: Vec<String> = Vec::new();
    for (i, (ep, _expanded)) in expanded_endpoints.iter().enumerate() {
        let sort_index = base_index + i;
        if existing.contains(&ep.provider_id) {
            // 已有：保留用户 settings_config，仅刷新入队 + sort_index
            db.add_to_failover_queue(&req.app_type, &ep.provider_id)?;
            db.update_provider_sort_index(&req.app_type, &ep.provider_id, sort_index)?;
        } else {
            let provider = build_provider_from_spec(ep, sort_index);
            db.save_provider(&req.app_type, &provider)?;
        }
        installed_ids.push(ep.provider_id.clone());
    }

    // 3) 开启 auto_failover + proxy（如果尚未开启）
    enable_auto_failover(db, &req.app_type).await?;

    // 4) P1 = endpoints[0]（bundle 定义顺序即故障转移顺序）
    let primary_provider_id = spec
        .endpoints
        .first()
        .map(|e| e.provider_id.clone())
        .ok_or_else(|| AppError::Config("bundle 没有端点".to_string()))?;

    Ok(InstallBundleResult {
        bundle_id: spec.id,
        app_type: req.app_type.clone(),
        installed_provider_ids: installed_ids,
        primary_provider_id,
        auto_failover_enabled: true,
        missing_env_vars,
    })
}

/// 启用 auto_failover 的辅助：若 proxy_config.auto_failover_enabled=false，
/// 先确保 proxy.enabled=true（auto_failover 依赖 proxy 接管），再打开 auto_failover。
async fn enable_auto_failover(db: &Database, app_type: &str) -> Result<(), AppError> {
    let mut cfg = db.get_proxy_config_for_app(app_type).await?;
    if !cfg.enabled {
        cfg.enabled = true;
    }
    cfg.auto_failover_enabled = true;
    db.update_proxy_config_for_app(cfg).await
}

/// 把 spec 渲染成 Provider。
///
/// 规则：
/// - `id` / `name` / `icon` / `iconColor` / `category` 直接来自 spec；
/// - `settingsConfig` 整块来自 spec（同 add_provider 的语义）；
/// - `inFailoverQueue = true`、`sortIndex` 在此层设。
fn build_provider_from_spec(ep: &BundleEndpointSpec, sort_index: usize) -> Provider {
    Provider {
        id: ep.provider_id.clone(),
        name: ep.name.clone(),
        settings_config: ep.settings_config.clone(),
        website_url: ep.website_url.clone(),
        category: ep.category.clone(),
        created_at: None,
        sort_index: Some(sort_index),
        notes: Some(format!("由 kaixuan bundle 安装（role={})", ep.role)),
        meta: None,
        icon: ep.icon.clone(),
        icon_color: ep.icon_color.clone(),
        in_failover_queue: true,
    }
}

/// 在 settings_config 内更新 api_key_field 路径的字符串值。
///
/// `field_path` 用 `/` 分段（如 `auth/OPENAI_API_KEY`），支持中间是 object；
/// 中间节点缺失则原地插入空对象。空字符串设为空（保留字段，便于占位符
/// 失败时也能落库，前端 UI 会标红）。
///
/// 实现要点：用 `&mut Value` 链贯穿 root，全程不丢根引用——这是上版
/// 用 `current = entry.clone()` 改写值后会丢根的原因。走到叶子层时
/// 直接 `*entry = Value::String(...)` 写入，根自然被一并改掉。
fn apply_expanded_key(root: &Value, field_path: &str, expanded: &str) -> Value {
    let segs: Vec<&str> = field_path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        return root.clone();
    }
    let mut root = root.clone();
    let mut current = &mut root;
    for (i, seg) in segs.iter().enumerate() {
        let last = i + 1 == segs.len();
        // 当前层必须是 Object；不是就提前返回原 root（语义失败不覆盖）
        let Value::Object(map) = current else {
            return root;
        };
        // 中间节点：缺失则创建空 object，否则取现有 entry 的可变引用
        let entry = map
            .entry((*seg).to_string())
            .or_insert_with(|| json!({}));
        if !entry.is_object() {
            *entry = json!({});
        }
        if last {
            *entry = Value::String(expanded.to_string());
            return root;
        }
        current = entry;
    }
    root
}

/// 展开单条用户输入的 key。`$ENV` / `${ENV}` 占位符由 [`expand_required`] 处理。
///
/// 行为契约：
/// - 输入无 `$` → 当作字面量使用；
/// - 有 `$` 且全部展开成功 → 用展开后的字符串；
/// - 有 `$` 但缺环境变量 → 落空字符串，把变量名 push 进 missing（不返回
///   错——安装流程继续，前端根据 missing_env_vars 显示红字提示）。
fn expand_endpoint_key(user_key: &str, missing: &mut Vec<String>) -> String {
    if user_key.is_empty() {
        return String::new();
    }
    if !contains_placeholder(user_key) {
        return user_key.to_string();
    }
    match expand_required(user_key) {
        Ok(v) => v,
        Err(missing_vars) => {
            for v in missing_vars {
                if !missing.contains(&v) {
                    missing.push(v);
                }
            }
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------
// 已注册 bundle 的 SSOT
// ---------------------------------------------------------------------------

/// 返回所有 bundle。当前只有 kaixuan；要新增就在这里加行。
pub fn bundles() -> Vec<BundleSpec> {
    vec![kaixuan_bundle()]
}

/// kaixuan bundle：开轩 LLM 网关（主）+ 本地 LLM 网关（备）。
///
/// 主备顺序的设计：
/// - `primary` = kxpms_gateway：公网稳、模型齐全、官方 SSOT（`~/.minimax/config.yaml`
///   的 `custom_provider.kaixuan`），上线即首选；
/// - `secondary` = local_gateway：仅本地网络可达（127.0.0.1:8782），公网挂掉时
///   自动接住；但本机网关某些模型 503（实测 2026-09-28），P1 永远优先。
///
/// 模型目录的 SSOT 在 `src/config/codexProviderPresets.ts`，但 bundle 内
/// 重复放了一份较小但稳定的子集（覆盖主力非 OpenAI 模型）。完整目录由
/// 后端的 model_catalog_json 注入（沿用现有 add_provider 行为）。
fn kaixuan_bundle() -> BundleSpec {
    // 本机网关端口可由 `KAIXUAN_LOCAL_GATEWAY_PORT` 覆盖（默认 8782）。
    // 用户跑 8783 / 8888 等也能复用本 bundle，无需 fork spec。
    let local_port: u16 = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(8782);
    let local_base_url = format!("http://127.0.0.1:{local_port}/v1");

    let kxpms_config = r#"[model_providers.custom]
name = "kxpms_gateway"
base_url = "https://llm.kxpms.cn/v1"
wire_api = "responses"
requires_openai_auth = true"#;

    let local_config_tmpl = r#"[model_providers.custom]
name = "local_gateway"
base_url = "__LOCAL_BASE_URL__"
wire_api = "responses"
requires_openai_auth = true"#;
    let local_config = local_config_tmpl.replace("__LOCAL_BASE_URL__", &local_base_url);

    let model_catalog = json!([
        {"model":"claude-opus-5","displayName":"Claude Opus 5","contextWindow":1000000,"inputModalities":["text","image"],"reasoningLevels":["low","medium","high","xhigh","max"],"defaultReasoningLevel":"high"},
        {"model":"claude-sonnet-5","displayName":"Claude Sonnet 5","contextWindow":1000000,"inputModalities":["text","image"],"reasoningLevels":["low","medium","high","xhigh","max"],"defaultReasoningLevel":"high"},
        {"model":"claude-opus-4-8","displayName":"Claude Opus 4.8","contextWindow":256000,"inputModalities":["text","image"],"reasoningLevels":["low","medium","high","xhigh","max"],"defaultReasoningLevel":"high"},
        {"model":"glm-5.2","displayName":"GLM-5.2","contextWindow":1000000,"inputModalities":["text"]},
        {"model":"kimi-k3","displayName":"Kimi K3","contextWindow":1048576,"inputModalities":["text"]},
        {"model":"minimax-m3","displayName":"MiniMax M3","contextWindow":512000,"inputModalities":["text","image"]},
        {"model":"deepseek-v4-pro","displayName":"DeepSeek V4 Pro","contextWindow":1000000,"inputModalities":["text"]},
        {"model":"auto","displayName":"Auto (网关自动路由)","contextWindow":256000,"inputModalities":["text"]}
    ]);

    let kxpms_settings = json!({
        "config": kxpms_config,
        "auth": {"OPENAI_API_KEY": ""},
        "modelCatalog": model_catalog,
    });
    let local_settings = json!({
        "config": local_config,
        "auth": {"OPENAI_API_KEY": ""},
        "modelCatalog": model_catalog,
    });

    BundleSpec {
        id: "kaixuan".to_string(),
        display_name: "kaixuan (开轩+本地)".to_string(),
        app_type: AppType::Codex.as_str().to_string(),
        endpoints: vec![
            BundleEndpointSpec {
                role: "primary".to_string(),
                provider_id: "kaixuan-kxpms".to_string(),
                name: "开轩 LLM 网关 (kxpms.cn)".to_string(),
                website_url: Some("https://llm.kxpms.cn".to_string()),
                icon: Some("kxpms_gateway".to_string()),
                icon_color: Some("#0EA5E9".to_string()),
                category: Some("custom".to_string()),
                settings_config: kxpms_settings,
                api_key_field: "auth/OPENAI_API_KEY".to_string(),
            },
            BundleEndpointSpec {
                role: "secondary".to_string(),
                provider_id: "kaixuan-local-8782".to_string(),
                name: "本地 LLM 网关 (8782)".to_string(),
                website_url: Some(format!("http://127.0.0.1:{local_port}")),
                icon: Some("local_gateway_8782".to_string()),
                icon_color: Some("#6366F1".to_string()),
                category: Some("custom".to_string()),
                settings_config: local_settings,
                api_key_field: "auth/OPENAI_API_KEY".to_string(),
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Tauri command wrapper（放到 commands/provider_bundle.rs，转发到本模块）
// ---------------------------------------------------------------------------

/// 在 async 上下文里跑 install_bundle_internal，并切到 P1。
pub async fn install_bundle(
    app_handle: AppHandle,
    request: InstallBundleRequest,
) -> Result<InstallBundleResult, String> {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle
            .try_state::<AppState>()
            .ok_or_else(|| "应用状态不可用".to_string())?;
        let result = install_bundle_internal_blocking(&state.db, &request)
            .map_err(|e| e.to_string())?;
        // 切到 P1
        crate::switch_provider_test_hook(
            state.inner(),
            AppType::from_str(&request.app_type).map_err(|e| e.to_string())?,
            &result.primary_provider_id,
        )
        .map_err(|e| format!("bundle 装好但切到 P1 失败: {e}"))?;
        Ok(result)
    })
    .await
    .map_err(|e| format!("bundle 安装任务执行失败: {e}"))?
}

/// install_bundle_internal 的 sync 包装（用于 spawn_blocking）。
/// 在 sync 上下文里跑 `tokio::runtime::Handle::current().block_on()` 调
/// async DB 方法；Tauri 命令跑在 tokio runtime，所以这里能直接 block_on。
fn install_bundle_internal_blocking(
    db: &Database,
    req: &InstallBundleRequest,
) -> Result<InstallBundleResult, AppError> {
    let rt = tokio::runtime::Handle::try_current().map_err(|e| {
        AppError::Config(format!("install_bundle 需要 tokio runtime 上下文: {e}"))
    });
    let rt = match rt {
        Ok(rt) => rt,
        Err(e) => return Err(e),
    };
    rt.block_on(install_bundle_internal(db, req))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// 端到端：在临时 HOME 里 init 真 SQLite，验证 install_bundle_internal
    /// 真的把两个 endpoint 写进 providers 表、加入故障转移队列、开启
    /// auto_failover，并把 P1 设为 kaixuan-kxpms。
    ///
    /// 关键不变量（不只是「函数返回了 result」）：
    /// - 两条 provider 的 settings_config 里 OPENAI_API_KEY 真的写成了
    ///   展开后的明文（不再含 `$VAR` 字面量）
    /// - `in_failover_queue=1` 真的落在 DB 里
    /// - `proxy_config.auto_failover_enabled` 真的翻成 1
    /// - 用户预存的同名 provider（手填了不同的 api key）不会被 bundle
    ///   install 覆盖——只刷 sort_index 与 in_failover_queue
    #[test]
    #[serial(env)]
    fn kaixuan_bundle_local_port_env_override() {
        let saved = std::env::var("KAIXUAN_LOCAL_GATEWAY_PORT").ok();
        std::env::set_var("KAIXUAN_LOCAL_GATEWAY_PORT", "8899");
        let b = kaixuan_bundle();
        let local = b.endpoints.iter().find(|e| e.role == "secondary").unwrap();
        let config_str = local.settings_config["config"].as_str().unwrap();
        assert!(
            config_str.contains("http://127.0.0.1:8899/v1"),
            "local_config 应 base_url=http://127.0.0.1:8899/v1，实际：{config_str}"
        );
        assert_eq!(local.website_url.as_deref(), Some("http://127.0.0.1:8899"));
        match saved {
            Some(v) => std::env::set_var("KAIXUAN_LOCAL_GATEWAY_PORT", v),
            None => std::env::remove_var("KAIXUAN_LOCAL_GATEWAY_PORT"),
        }
    }

    /// 端到端：在临时 HOME 里 init 真 SQLite，验证 install_bundle_internal
    /// 真的把两个 endpoint 写进 providers 表、加入故障转移队列、开启
    /// auto_failover，并把 P1 设为 kaixuan-kxpms。
    ///
    /// 关键不变量（不只是「函数返回了 result」）：
    /// - 两条 provider 的 settings_config 里 OPENAI_API_KEY 真的写成了
    ///   展开后的明文（不再含 `$VAR` 字面量）
    /// - `in_failover_queue=1` 真的落在 DB 里
    /// - `proxy_config.auto_failover_enabled` 真的翻成 1
    /// - 用户预存的同名 provider（手填了不同的 api key）不会被 bundle
    ///   install 覆盖——只刷 sort_index 与 in_failover_queue
    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn install_bundle_internal_end_to_end_with_real_db() {
        use crate::config::get_app_config_dir;
        use crate::database::Database;
        use tempfile::TempDir;

        // 临时 HOME → cc-switch.db 落在 sandbox
        // 注：`#[serial]` 已让本测试串行运行，env var 与 DB 文件不会与
        // 其它并行测试互相污染。
        let tmp = TempDir::new().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        std::env::set_var("HOME", tmp.path());
        // restore on drop
        let _restore = EnvRestore::new(&[
            ("CC_SWITCH_TEST_HOME", true),
            ("HOME", true),
        ]);

        let db = Database::init().expect("db init");

        // 调试：先确认 proxy_config 真的有 circuit_half_open_permit_max_age_seconds 列。
        // 直接读 schema 验证，不然后续 get_proxy_config_for_app 会因老 schema 失败。
        {
            let conn_lock = db.conn.lock();
            let conn_guard = conn_lock
                .map_err(|e| format!("lock failed: {e}"))
                .expect("lock");
            let mut stmt = conn_guard
                .prepare("PRAGMA table_info(proxy_config)")
                .expect("pragma");
            let cols: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .expect("cols")
                .map(|r| r.unwrap())
                .collect();
            let db_path = crate::config::get_app_config_dir().join("cc-switch.db");
            let test_home = std::env::var("CC_SWITCH_TEST_HOME").ok();
            assert!(
                cols.iter().any(|c| c.contains("circuit_half_open")),
                "proxy_config 必须有 circuit_half_open 列；cols={:?}, db_path={:?}, CC_SWITCH_TEST_HOME={:?}",
                cols,
                db_path,
                test_home,
            );
        }

        // 设置一个 env var 让 install_bundle 展开
        std::env::set_var("KAIXUAN_TEST_KEY", "sk-from-env-1234567890");
        let _restore2 = EnvRestore::new(&[("KAIXUAN_TEST_KEY", false)]);

        let mut api_keys = std::collections::HashMap::new();
        api_keys.insert("primary".to_string(), "$KAIXUAN_TEST_KEY".to_string());
        api_keys.insert("secondary".to_string(), "$KAIXUAN_TEST_KEY".to_string());

        let req = InstallBundleRequest {
            bundle_id: "kaixuan".to_string(),
            app_type: AppType::Codex.as_str().to_string(),
            api_keys,
        };

        let result = install_bundle_internal(&db, &req)
            .await
            .expect("install_bundle_internal");
        assert_eq!(result.bundle_id, "kaixuan");
        assert_eq!(result.primary_provider_id, "kaixuan-kxpms");
        assert!(result.auto_failover_enabled);
        assert!(result.missing_env_vars.is_empty());
        assert_eq!(result.installed_provider_ids.len(), 2);
        assert!(result
            .installed_provider_ids
            .contains(&"kaixuan-kxpms".to_string()));
        assert!(result
            .installed_provider_ids
            .contains(&"kaixuan-local-8782".to_string()));

        // 1) 两条 provider 真的在 DB 里
        let all = db.get_all_providers(AppType::Codex.as_str()).expect("read");
        let kxpms = all.get("kaixuan-kxpms").expect("kxpms in db");
        let local = all.get("kaixuan-local-8782").expect("local in db");
        assert_eq!(kxpms.in_failover_queue, true);
        assert_eq!(local.in_failover_queue, true);
        assert!(kxpms.sort_index.is_some());
        assert!(local.sort_index.is_some());

        // 2) 密钥展开后写入 DB（不再含 $ 字面量）
        let kxpms_key = kxpms.settings_config["auth"]["OPENAI_API_KEY"]
            .as_str()
            .expect("key string");
        assert_eq!(kxpms_key, "sk-from-env-1234567890");
        let local_key = local.settings_config["auth"]["OPENAI_API_KEY"]
            .as_str()
            .expect("key string");
        assert_eq!(local_key, "sk-from-env-1234567890");
        assert!(
            !kxpms_key.contains('$'),
            "DB 里不应再有 $ 占位符字面量：{kxpms_key}"
        );

        // 3) auto_failover 真的翻成 1
        let proxy_cfg = db
            .get_proxy_config_for_app(AppType::Codex.as_str())
            .await
            .expect("proxy cfg");
        assert!(proxy_cfg.auto_failover_enabled, "auto_failover 应开启");
        assert!(proxy_cfg.enabled, "proxy 接管应开启");

        // 4) 故障转移队列里两条都在，顺序与 spec 一致（kxpms 在前）
        let queue = db
            .get_failover_queue(AppType::Codex.as_str())
            .expect("queue");
        assert_eq!(queue.len(), 2);
        let ids: Vec<&str> = queue.iter().map(|q| q.provider_id.as_str()).collect();
        let kxpms_pos = ids.iter().position(|id| *id == "kaixuan-kxpms").unwrap();
        let local_pos = ids.iter().position(|id| *id == "kaixuan-local-8782").unwrap();
        assert!(
            kxpms_pos < local_pos,
            "P1 顺序错：kxpms@{kxpms_pos}, local@{local_pos}"
        );

        // 5) 二次 install_bundle 是幂等的（不会重复写）
        let result2 = install_bundle_internal(&db, &req).await.expect("install 2");
        assert_eq!(result2.installed_provider_ids.len(), 2);
        let all2 = db.get_all_providers(AppType::Codex.as_str()).expect("read");
        assert_eq!(all2.len(), all.len(), "重复 install 不应复制行");

        // 7) 缺失 env var 时：占位符仍展开（此时为 ""）但 existing-row 路径**保留**
        // 用户已写过的 settings_config——所以 DB 里仍然是 install 1 时的真值。
        // missing_env_vars 仍要返回（让 UI 红字提示），但不会覆盖已有 key。
        std::env::remove_var("KAIXUAN_TEST_KEY");
        let req2 = InstallBundleRequest {
            bundle_id: "kaixuan".to_string(),
            app_type: AppType::Codex.as_str().to_string(),
            api_keys: {
                let mut m = std::collections::HashMap::new();
                m.insert("primary".to_string(), "$KAIXUAN_TEST_KEY".to_string());
                m
            },
        };
        let result3 = install_bundle_internal(&db, &req2).await.expect("install 3");
        assert_eq!(
            result3.missing_env_vars,
            vec!["KAIXUAN_TEST_KEY".to_string()]
        );
        let all3 = db.get_all_providers(AppType::Codex.as_str()).expect("read");
        // 已存在 provider 的 settings_config 不被覆盖（关键不变量——擦掉用户
        // 原 key 会触发 P0 事故）
        assert_eq!(
            all3["kaixuan-kxpms"].settings_config["auth"]["OPENAI_API_KEY"],
            "sk-from-env-1234567890",
            "existing-row 的 settings_config 必须保留，不被 missing-env 的二次 install 覆盖"
        );

        // sanity: db dir 用了我们的 tempdir（仅在 app_store 缓存未污染时断言；
        // 由于 app_store 的 OnceLock<RwLock<Option<PathBuf>>> 是进程级静态，
        // 别的测试设置后会跨测试泄漏，所以这条断言只能在线程隔离跑时稳定）
        if std::env::var("CC_SWITCH_TEST_HOME").is_ok() {
            assert!(get_app_config_dir().starts_with(tmp.path()));
        }
    }

    /// 关键不变量：用户预填了自定义 OPENAI_API_KEY 的同名 provider，bundle
    /// install 必须**保留**用户值，不能覆盖。仅刷新 sort_index + 入队。
    #[tokio::test(flavor = "multi_thread")]
    #[serial]
    async fn install_bundle_preserves_user_prefilled_settings() {
        use crate::database::Database;
        use crate::provider::Provider;
        use tempfile::TempDir;

        let tmp = TempDir::new().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", tmp.path());
        std::env::set_var("HOME", tmp.path());
        let _restore = EnvRestore::new(&[
            ("CC_SWITCH_TEST_HOME", true),
            ("HOME", true),
        ]);

        let db = Database::init().expect("db init");

        // 预填一个 kaixuan-kxpms，用户设了自定义 key
        let user_key = "sk-user-custom-original-key";
        let pre_provider = Provider {
            id: "kaixuan-kxpms".to_string(),
            name: "用户预先填的 kxpms".to_string(),
            settings_config: json!({
                "auth": {"OPENAI_API_KEY": user_key},
                "config": "[model_providers.custom]\nname = \"kxpms_gateway\"\nbase_url = \"https://llm.kxpms.cn/v1\"\nwire_api = \"responses\"\n",
                "modelCatalog": []
            }),
            website_url: Some("https://llm.kxpms.cn".to_string()),
            category: Some("custom".to_string()),
            created_at: None,
            sort_index: None,
            notes: Some("用户手填".to_string()),
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };
        db.save_provider(AppType::Codex.as_str(), &pre_provider)
            .expect("save pre");

        // 设置 env，让 bundle 想用 $ 展开
        std::env::set_var("KAIXUAN_TEST_KEY2", "sk-bundle-would-write");
        let _restore2 = EnvRestore::new(&[("KAIXUAN_TEST_KEY2", false)]);

        let mut api_keys = std::collections::HashMap::new();
        api_keys.insert("primary".to_string(), "$KAIXUAN_TEST_KEY2".to_string());
        let req = InstallBundleRequest {
            bundle_id: "kaixuan".to_string(),
            app_type: AppType::Codex.as_str().to_string(),
            api_keys,
        };

        install_bundle_internal(&db, &req).await.expect("install");

        // 用户原 key 应保留
        let after = db
            .get_provider_by_id("kaixuan-kxpms", AppType::Codex.as_str())
            .expect("read")
            .expect("present");
        let after_key = after.settings_config["auth"]["OPENAI_API_KEY"]
            .as_str()
            .unwrap();
        assert_eq!(
            after_key, user_key,
            "用户预填的 key 必须保留，不能被 bundle 覆盖"
        );
        // 入队必须翻成 true
        assert!(after.in_failover_queue, "in_failover_queue 必须变 true");
        // sort_index 应被刷
        assert!(after.sort_index.is_some());
        // name 保留（不覆盖用户命名）
        assert_eq!(after.name, "用户预先填的 kxpms");
    }

    // ---- mini helpers ----

    /// 临时设一组 env var，Drop 时按 `set` 还原（true = 还原原值 / 删除；false = 仅删除）。
    struct EnvRestore {
        saved: Vec<(&'static str, Option<String>)>,
        delete_only: Vec<&'static str>,
    }
    impl EnvRestore {
        fn new(items: &[(&'static str, bool)]) -> Self {
            let mut saved = Vec::new();
            let mut delete_only = Vec::new();
            for (k, set) in items {
                if *set {
                    saved.push((*k, std::env::var(k).ok()));
                } else {
                    delete_only.push(*k);
                }
            }
            Self { saved, delete_only }
        }
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (k, _) in &self.saved {
                std::env::remove_var(k);
            }
            for k in &self.delete_only {
                std::env::remove_var(k);
            }
        }
    }

    #[test]
    fn kaixuan_bundle_has_two_endpoints_in_order() {
        let b = kaixuan_bundle();
        assert_eq!(b.id, "kaixuan");
        assert_eq!(b.app_type, AppType::Codex.as_str());
        assert_eq!(b.endpoints.len(), 2);
        // P1 必须是 kxpms（公网优先）
        assert_eq!(b.endpoints[0].role, "primary");
        assert_eq!(b.endpoints[0].provider_id, "kaixuan-kxpms");
        assert_eq!(b.endpoints[1].role, "secondary");
        assert_eq!(b.endpoints[1].provider_id, "kaixuan-local-8782");
    }

    #[test]
    fn apply_expanded_key_writes_into_nested_path() {
        let root = json!({
            "auth": {"OPENAI_API_KEY": ""},
            "config": "[model_providers.custom]"
        });
        let r = apply_expanded_key(&root, "auth/OPENAI_API_KEY", "sk-real");
        assert_eq!(r["auth"]["OPENAI_API_KEY"], "sk-real");
        // 其他字段不丢
        assert_eq!(r["config"], "[model_providers.custom]");
    }

    #[test]
    fn apply_expanded_key_creates_missing_intermediate() {
        let root = json!({"config": "x"});
        let r = apply_expanded_key(&root, "auth/OPENAI_API_KEY", "sk-y");
        assert_eq!(r["auth"]["OPENAI_API_KEY"], "sk-y");
    }

    #[test]
    fn apply_expanded_key_empty_path_is_passthrough() {
        let root = json!({"a": 1});
        let r = apply_expanded_key(&root, "", "x");
        assert_eq!(r, root);
    }

    #[test]
    fn expand_endpoint_key_passthrough_when_no_placeholder() {
        let mut missing = Vec::new();
        let v = expand_endpoint_key("sk-static", &mut missing);
        assert_eq!(v, "sk-static");
        assert!(missing.is_empty());
    }

    #[test]
    fn expand_endpoint_key_records_missing() {
        let mut missing = Vec::new();
        let v = expand_endpoint_key("$DEFINITELY_NOT_SET_X_123", &mut missing);
        // 缺环境变量 → 落空字符串 + missing 记录
        assert_eq!(v, "");
        assert_eq!(missing, vec!["DEFINITELY_NOT_SET_X_123".to_string()]);
    }

    #[test]
    fn bundle_listing_includes_kaixuan() {
        let v = list_bundles();
        assert!(v.iter().any(|b| b.id == "kaixuan"));
        let b = v.iter().find(|b| b.id == "kaixuan").unwrap();
        assert_eq!(b.endpoint_count, 2);
        assert_eq!(b.app_type, AppType::Codex.as_str());
    }

    #[test]
    fn get_bundle_unknown_returns_none() {
        assert!(get_bundle("nope").is_none());
    }

    #[test]
    fn kaixuan_bundle_settings_have_required_keys() {
        let b = kaixuan_bundle();
        for ep in &b.endpoints {
            // 必填：auth.OPENAI_API_KEY 是空字符串（占位待填）
            assert_eq!(
                ep.settings_config["auth"]["OPENAI_API_KEY"],
                "",
                "{} 缺 OPENAI_API_KEY 槽位",
                ep.provider_id
            );
            // 必填：config 含 base_url
            assert!(
                ep.settings_config["config"]
                    .as_str()
                    .unwrap_or("")
                    .contains("base_url"),
                "{} config 缺 base_url",
                ep.provider_id
            );
            // 必填：modelCatalog 至少 1 条
            assert!(
                ep.settings_config["modelCatalog"]
                    .as_array()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false),
                "{} modelCatalog 为空",
                ep.provider_id
            );
        }
    }
}