//! Claude 模型路由（VS Code 网关）
//!
//! 确定性 provider/model 路由：
//! - 公开模型 ID 形如 `ccswitch/claude/<base64url(provider_id)>/<base64url(alias)>`
//! - 路由解析只依赖请求中的公开模型 ID 与数据库存储的路由元数据，
//!   不读取"当前供应商"，不进入故障转移队列，也不触发代理热切换
//! - 每次请求实时读取数据库（故意不做缓存）：供应商数量小、LLM 网络延迟占主导，
//!   且实时读取可避免凭证/路由过期与缓存失效子系统
//!
//! 解析失败一律返回 HTTP 400 + Anthropic 错误信封（见 [`ProxyError::InvalidRouterModel`]），
//! 绝不回退到当前激活供应商。

use super::error::ProxyError;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::{ClaudeRouterConfig, Provider};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::HashSet;

/// 公开模型 ID 的字面前缀
const PUBLIC_ID_PREFIX: &str = "ccswitch/claude/";

/// Claude 路由解析结果
///
/// 持有请求开始时的 Provider 克隆：路由建立后，对数据库中该供应商的后续编辑
/// （改名、改密钥、禁用路由）不影响本次请求的转发目标。
pub(crate) struct ClaudeRouteTarget {
    /// 请求开始时的 Provider 克隆（含凭证/端点快照）
    pub provider: Provider,
    /// 客户端请求中的公开模型 ID（原样回显，用于日志与 usage 归因）
    pub public_model_id: String,
    /// 实际发往上游的模型名（body.model 改写目标）
    pub upstream_model: String,
}

/// 生成公开模型 ID：`ccswitch/claude/<base64url-no-pad(provider_id)>/<base64url-no-pad(alias)>`
///
/// base64url 编码使任意供应商 ID / 别名（含 `/`、Unicode）都能安全嵌入路径，
/// 且 ID 跨供应商改名、上游模型重映射保持稳定。
pub(crate) fn public_model_id(provider_id: &str, alias: &str) -> String {
    format!(
        "{}{}/{}",
        PUBLIC_ID_PREFIX,
        URL_SAFE_NO_PAD.encode(provider_id.as_bytes()),
        URL_SAFE_NO_PAD.encode(alias.as_bytes())
    )
}

fn invalid_model(message: String) -> ProxyError {
    ProxyError::InvalidRouterModel(message)
}

fn decode_segment(segment: &str, original_id: &str) -> Result<String, ProxyError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(segment)
        .map_err(|_| invalid_model(format!("model '{original_id}': invalid base64url segment")))?;
    String::from_utf8(bytes)
        .map_err(|_| invalid_model(format!("model '{original_id}': segment is not valid UTF-8")))
}

/// 解析公开模型 ID 为 `(provider_id, alias)`
///
/// 严格要求：字面 `ccswitch/claude/` 前缀 + 恰好两个非空 base64url 段，
/// 任何偏差（段数错误、空段、base64/UTF-8 解码失败）都返回 400 错误，
/// 不做名称扫描或兼容别名匹配。
pub(crate) fn parse_public_model_id(id: &str) -> Result<(String, String), ProxyError> {
    let rest = id.strip_prefix(PUBLIC_ID_PREFIX).ok_or_else(|| {
        invalid_model(format!("model '{id}': missing '{PUBLIC_ID_PREFIX}' prefix"))
    })?;
    let segments: Vec<&str> = rest.split('/').collect();
    if segments.len() != 2 || segments.iter().any(|s| s.is_empty()) {
        return Err(invalid_model(format!(
            "model '{id}': expected exactly two non-empty segments"
        )));
    }
    let provider_id = decode_segment(segments[0], id)?;
    let alias = decode_segment(segments[1], id)?;
    Ok((provider_id, alias))
}

/// 字段必须本身即为非空 trim 后字符串：拒绝而不是静默 trim，
/// 避免"表单里带空格的别名"与"运行时精确匹配"产生口径分裂。
fn field_is_invalid(value: &str) -> bool {
    value.is_empty() || value.trim() != value
}

/// 校验启用态路由配置的合法性（保存时与运行时共用的同一口径）
fn check_router_config(provider: &Provider, router: &ClaudeRouterConfig) -> Result<(), String> {
    // 官方供应商走 Claude 原生登录态，不属于第三方路由目标
    if provider.category.as_deref() == Some("official") {
        return Err("官方（official）供应商不能启用 Claude 模型路由".to_string());
    }
    if router.models.is_empty() {
        return Err("Claude 模型路由已启用但未配置任何模型".to_string());
    }
    let mut seen_aliases: HashSet<&str> = HashSet::new();
    for model in &router.models {
        if field_is_invalid(&model.alias) {
            return Err("Claude 路由模型的 alias 不能为空或包含首尾空白".to_string());
        }
        if field_is_invalid(&model.upstream_model) {
            return Err(format!(
                "Claude 路由模型 '{}' 的 upstreamModel 不能为空或包含首尾空白",
                model.alias
            ));
        }
        if field_is_invalid(&model.display_name) {
            return Err(format!(
                "Claude 路由模型 '{}' 的 displayName 不能为空或包含首尾空白",
                model.alias
            ));
        }
        if !seen_aliases.insert(model.alias.as_str()) {
            return Err(format!("Claude 路由模型 alias '{}' 重复", model.alias));
        }
    }
    Ok(())
}

/// 保存时校验（由 ProviderService::add / update 在 `validate_provider_settings` 之后调用）。
///
/// 仅校验启用态配置：禁用态允许保留表单中间态（未完成的行），
/// 运行时解析对禁用/非法配置一律 fail-closed。
pub(crate) fn validate_provider_config(provider: &Provider) -> Result<(), AppError> {
    let Some(router) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.claude_router.as_ref())
    else {
        return Ok(());
    };
    if !router.enabled {
        return Ok(());
    }
    check_router_config(provider, router).map_err(AppError::InvalidInput)
}

/// 按公开模型 ID 解析确定性路由目标
///
/// 流程：解码 ID → 精确取该 Claude 供应商 → 要求 `claudeRouter.enabled`
/// → 运行时复验配置合法性（历史导入的非法启用配置 fail-closed）→ 精确匹配别名。
/// 任何一步失败都返回 [`ProxyError::InvalidRouterModel`]（HTTP 400），
/// 绝不回退到当前激活供应商。
pub(crate) fn resolve_route(db: &Database, id: &str) -> Result<ClaudeRouteTarget, ProxyError> {
    let (provider_id, alias) = parse_public_model_id(id)?;
    let provider = db
        .get_provider_by_id(&provider_id, "claude")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?
        .ok_or_else(|| invalid_model(format!("model '{id}': provider not found")))?;
    let Some(router) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.claude_router.as_ref())
        .filter(|router| router.enabled)
    else {
        return Err(invalid_model(format!(
            "model '{id}': Claude router is not enabled for this provider"
        )));
    };
    if let Err(reason) = check_router_config(&provider, router) {
        return Err(invalid_model(format!(
            "model '{id}': invalid router configuration ({reason})"
        )));
    }
    let upstream_model = router
        .models
        .iter()
        .find(|model| model.alias == alias)
        .map(|model| model.upstream_model.clone())
        .ok_or_else(|| invalid_model(format!("model '{id}': unknown model alias")))?;
    Ok(ClaudeRouteTarget {
        provider,
        public_model_id: id.to_string(),
        upstream_model,
    })
}

/// 构建网关模型目录（`GET /claude-router/v1/models`）
///
/// 仅收录启用路由的 Claude 供应商，按供应商 `sort_index`（`get_all_providers`
/// 已排序）再按模型配置顺序输出。响应只含公开 ID / object / 显示名，
/// 绝不携带供应商端点、凭证、meta 等敏感信息。
pub(crate) fn model_catalog(db: &Database) -> Result<Value, ProxyError> {
    let providers = db
        .get_all_providers("claude")
        .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;
    let mut data = Vec::new();
    for provider in providers.values() {
        let Some(router) = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.claude_router.as_ref())
            .filter(|router| router.enabled)
        else {
            continue;
        };
        if check_router_config(provider, router).is_err() {
            // 非法启用配置（历史导入）不出现在目录中，避免选择器暴露必然 400 的路由
            log::warn!(
                "[ClaudeRouter] 跳过非法路由配置的供应商: {} ({})",
                provider.name,
                provider.id
            );
            continue;
        }
        for model in &router.models {
            data.push(json!({
                "id": public_model_id(&provider.id, &model.alias),
                "object": "model",
                "display_name": format!("{} / {}", provider.name, model.display_name),
            }));
        }
    }
    Ok(json!({ "data": data }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeRouterModel, ProviderMeta};
    use serde_json::json;

    const TEST_SECRET: &str = "sk-router-test-secret";
    const TEST_BASE_URL: &str = "https://upstream.example.com";

    fn router_model(alias: &str, upstream: &str, display: &str) -> ClaudeRouterModel {
        ClaudeRouterModel {
            alias: alias.to_string(),
            upstream_model: upstream.to_string(),
            display_name: display.to_string(),
        }
    }

    fn router_provider(
        id: &str,
        name: &str,
        sort_index: Option<usize>,
        router: Option<ClaudeRouterConfig>,
    ) -> Provider {
        let mut provider = Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({
                "env": {
                    "ANTHROPIC_BASE_URL": TEST_BASE_URL,
                    "ANTHROPIC_AUTH_TOKEN": TEST_SECRET,
                }
            }),
            None,
        );
        provider.sort_index = sort_index;
        if let Some(router) = router {
            provider.meta = Some(ProviderMeta {
                claude_router: Some(router),
                ..ProviderMeta::default()
            });
        }
        provider
    }

    fn enabled_router(models: Vec<ClaudeRouterModel>) -> ClaudeRouterConfig {
        ClaudeRouterConfig {
            enabled: true,
            models,
        }
    }

    // ========================================================================
    // 公开模型 ID 编解码
    // ========================================================================

    #[test]
    fn public_id_roundtrips_unicode_and_slash_segments() {
        let provider_id = "provider/供应商甲";
        let alias = "别名/fast v2";
        let id = public_model_id(provider_id, alias);
        assert!(id.starts_with(PUBLIC_ID_PREFIX));
        // 编码后的 ID 自身不能再含未转义的分隔符
        assert_eq!(id[PUBLIC_ID_PREFIX.len()..].matches('/').count(), 1);
        let (parsed_provider, parsed_alias) = parse_public_model_id(&id).expect("roundtrip");
        assert_eq!(parsed_provider, provider_id);
        assert_eq!(parsed_alias, alias);
    }

    #[test]
    fn parse_rejects_malformed_ids() {
        // 缺少前缀
        assert!(parse_public_model_id("claude-3-5-sonnet").is_err());
        // 段数错误：1 段 / 3 段
        assert!(parse_public_model_id("ccswitch/claude/b25seQ").is_err());
        assert!(parse_public_model_id("ccswitch/claude/QQ/QQ/QQ").is_err());
        // 空段
        assert!(parse_public_model_id("ccswitch/claude//QQ").is_err());
        assert!(parse_public_model_id("ccswitch/claude/QQ/").is_err());
        // 非法 base64url（`!` 不在字符表内）
        assert!(parse_public_model_id("ccswitch/claude/QQ/a!!").is_err());
        // base64 合法但非 UTF-8：encode([0xFF, 0xFF]) == "__8"
        assert!(parse_public_model_id("ccswitch/claude/__8/QQ").is_err());
    }

    // ========================================================================
    // 保存时校验
    // ========================================================================

    #[test]
    fn validate_rejects_official_category_when_enabled() {
        let mut provider = router_provider(
            "official-1",
            "Official",
            None,
            Some(enabled_router(vec![router_model("a", "m", "D")])),
        );
        provider.category = Some("official".to_string());
        assert!(validate_provider_config(&provider).is_err());
        // 未启用时 official 配置可保留（不构成启用）
        let disabled = router_provider(
            "official-2",
            "Official",
            None,
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![],
            }),
        );
        assert!(validate_provider_config(&disabled).is_ok());
    }

    #[test]
    fn validate_rejects_duplicate_aliases() {
        let provider = router_provider(
            "p1",
            "P1",
            None,
            Some(enabled_router(vec![
                router_model("fast", "m1", "Fast"),
                router_model("fast", "m2", "Fast 2"),
            ])),
        );
        assert!(validate_provider_config(&provider).is_err());
    }

    #[test]
    fn validate_rejects_blank_or_untrimmed_fields() {
        for model in [
            router_model("   ", "m", "D"),
            router_model(" fast", "m", "D"),
            router_model("fast", "", "D"),
            router_model("fast", "m", "\t"),
        ] {
            let case = format!("{model:?}");
            let provider = router_provider("p1", "P1", None, Some(enabled_router(vec![model])));
            assert!(
                validate_provider_config(&provider).is_err(),
                "model should be rejected: {case}"
            );
        }
    }

    #[test]
    fn validate_rejects_enabled_without_models() {
        let provider = router_provider(
            "p1",
            "P1",
            None,
            Some(ClaudeRouterConfig {
                enabled: true,
                models: vec![],
            }),
        );
        assert!(validate_provider_config(&provider).is_err());
    }

    #[test]
    fn validate_ignores_disabled_and_absent_configs() {
        // 禁用态保留表单中间态（重复/空白行）不报错
        let disabled = router_provider(
            "p1",
            "P1",
            None,
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![router_model("  ", "", ""), router_model("  ", "", "")],
            }),
        );
        assert!(validate_provider_config(&disabled).is_ok());
        // 无路由配置
        let plain = router_provider("p2", "P2", None, None);
        assert!(validate_provider_config(&plain).is_ok());
    }

    // ========================================================================
    // 路由解析（内存数据库）
    // ========================================================================

    #[test]
    fn resolve_returns_exact_target() {
        let db = Database::memory().expect("memory db");
        let provider = router_provider(
            "prov-a",
            "Provider A",
            None,
            Some(enabled_router(vec![router_model(
                "fast",
                "upstream-fast-a",
                "Fast A",
            )])),
        );
        db.save_provider("claude", &provider)
            .expect("save provider");

        let id = public_model_id("prov-a", "fast");
        let target = resolve_route(&db, &id).expect("resolve");
        assert_eq!(target.provider.id, "prov-a");
        assert_eq!(target.public_model_id, id);
        assert_eq!(target.upstream_model, "upstream-fast-a");
        // 克隆携带凭证快照
        assert_eq!(
            target.provider.settings_config["env"]["ANTHROPIC_AUTH_TOKEN"],
            json!(TEST_SECRET)
        );
    }

    #[test]
    fn resolve_rejects_unknown_disabled_and_missing_alias() {
        let db = Database::memory().expect("memory db");
        // 未知供应商（ID 形状合法）
        let unknown = public_model_id("no-such-provider", "fast");
        assert!(matches!(
            resolve_route(&db, &unknown),
            Err(ProxyError::InvalidRouterModel(_))
        ));

        // 路由禁用
        let disabled = router_provider(
            "prov-disabled",
            "Disabled",
            None,
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![router_model("fast", "m", "D")],
            }),
        );
        db.save_provider("claude", &disabled)
            .expect("save disabled");
        let disabled_id = public_model_id("prov-disabled", "fast");
        assert!(matches!(
            resolve_route(&db, &disabled_id),
            Err(ProxyError::InvalidRouterModel(_))
        ));

        // 未知别名
        let enabled = router_provider(
            "prov-b",
            "Provider B",
            None,
            Some(enabled_router(vec![router_model("fast", "m", "D")])),
        );
        db.save_provider("claude", &enabled).expect("save enabled");
        let wrong_alias = public_model_id("prov-b", "slow");
        assert!(matches!(
            resolve_route(&db, &wrong_alias),
            Err(ProxyError::InvalidRouterModel(_))
        ));
    }

    #[test]
    fn resolve_fails_closed_on_invalid_enabled_config() {
        let db = Database::memory().expect("memory db");
        // 绕过保存时校验（历史导入场景）：启用态重复别名直接落库
        let legacy = router_provider(
            "prov-legacy",
            "Legacy",
            None,
            Some(enabled_router(vec![
                router_model("fast", "m1", "D1"),
                router_model("fast", "m2", "D2"),
            ])),
        );
        db.save_provider("claude", &legacy).expect("save legacy");
        let id = public_model_id("prov-legacy", "fast");
        assert!(matches!(
            resolve_route(&db, &id),
            Err(ProxyError::InvalidRouterModel(_))
        ));
    }

    #[test]
    fn public_id_stable_across_rename_and_upstream_remap() {
        let db = Database::memory().expect("memory db");
        let provider = router_provider(
            "prov-stable",
            "Original Name",
            None,
            Some(enabled_router(vec![router_model("fast", "m-v1", "Fast")])),
        );
        db.save_provider("claude", &provider)
            .expect("save provider");
        let id_before = public_model_id("prov-stable", "fast");

        // 改名 + 上游模型重映射后重新落库
        let renamed = router_provider(
            "prov-stable",
            "Renamed Provider",
            None,
            Some(enabled_router(vec![router_model("fast", "m-v2", "Fast")])),
        );
        db.save_provider("claude", &renamed).expect("save renamed");

        let id_after = public_model_id("prov-stable", "fast");
        assert_eq!(id_before, id_after, "公开 ID 必须跨改名/重映射稳定");
        let target = resolve_route(&db, &id_after).expect("resolve renamed");
        assert_eq!(target.provider.name, "Renamed Provider");
        assert_eq!(target.upstream_model, "m-v2");
    }

    // ========================================================================
    // 模型目录
    // ========================================================================

    #[test]
    fn catalog_orders_by_sort_index_then_model_order() {
        let db = Database::memory().expect("memory db");
        let provider_a = router_provider(
            "prov-a",
            "Provider A",
            Some(1),
            Some(enabled_router(vec![
                router_model("fast", "m-a1", "Fast A"),
                router_model("pro", "m-a2", "Pro A"),
            ])),
        );
        let provider_b = router_provider(
            "prov-b",
            "Provider B",
            Some(0),
            Some(enabled_router(vec![router_model("mini", "m-b1", "Mini B")])),
        );
        let provider_c = router_provider(
            "prov-c",
            "Provider C",
            Some(2),
            Some(ClaudeRouterConfig {
                enabled: false,
                models: vec![router_model("off", "m-c1", "Off C")],
            }),
        );
        // 有意按乱序保存，验证目录仍按 sort_index 输出
        db.save_provider("claude", &provider_a).expect("save a");
        db.save_provider("claude", &provider_c).expect("save c");
        db.save_provider("claude", &provider_b).expect("save b");

        let catalog = model_catalog(&db).expect("catalog");
        let entries = catalog["data"].as_array().expect("data array");
        let ids: Vec<&str> = entries
            .iter()
            .map(|entry| entry["id"].as_str().expect("id"))
            .collect();
        assert_eq!(
            ids,
            vec![
                public_model_id("prov-b", "mini"),
                public_model_id("prov-a", "fast"),
                public_model_id("prov-a", "pro"),
            ],
            "按 sort_index 再按模型顺序输出"
        );
        assert_eq!(entries[0]["object"], json!("model"));
        assert_eq!(entries[0]["display_name"], json!("Provider B / Mini B"));
        assert_eq!(entries[2]["display_name"], json!("Provider A / Pro A"));
    }

    #[test]
    fn catalog_contains_no_credentials_or_endpoints() {
        let db = Database::memory().expect("memory db");
        let provider = router_provider(
            "prov-a",
            "Provider A",
            None,
            Some(enabled_router(vec![router_model("fast", "m-a1", "Fast A")])),
        );
        db.save_provider("claude", &provider)
            .expect("save provider");

        let serialized = serde_json::to_string(&model_catalog(&db).expect("catalog"))
            .expect("serialize catalog");
        for sentinel in [
            TEST_SECRET,
            TEST_BASE_URL,
            "ANTHROPIC_AUTH_TOKEN",
            "settingsConfig",
        ] {
            assert!(
                !serialized.contains(sentinel),
                "catalog must not contain '{sentinel}': {serialized}"
            );
        }
    }
}
