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
    let kxpms_config = r#"[model_providers.custom]
name = "kxpms_gateway"
base_url = "https://llm.kxpms.cn/v1"
wire_api = "responses"
requires_openai_auth = true"#;

    let local_config = r#"[model_providers.custom]
name = "local_gateway"
base_url = "http://127.0.0.1:8782/v1"
wire_api = "responses"
requires_openai_auth = true"#;

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
                website_url: Some("http://127.0.0.1:8782".to_string()),
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