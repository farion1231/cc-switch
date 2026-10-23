//! Hermes 的投影与补丁：路由供应商 → `config.yaml` 的 `custom_providers` 条目和 `model:` 段。
//!
//! Hermes 是累加式应用：供应商条目在 `custom_providers` 里共存。代理模式只多写一条指向
//! 本地代理的路由条目（名字固定 [`ROUTE_ENTRY_NAME`]，专属 CC Switch），并把顶层 `model:`
//! 段指过去；退出代理删这条、把 `model:` 段恢复原样。用户自己的其余条目、段落、注释字节
//! 不动（段级替换，见 [`crate::hermes_config::replace_yaml_section`]）。
//!
//! Hermes 的「多个 agent」是命名 profile（`<home>/profiles/<name>/`，各自独立的
//! config.yaml，不从默认 home 继承）。统一路由模式对每个 home 各跑一份本补丁；
//! `model.default` 在路由那家仍在提供的模型里就保留（各 agent 自己的模型选择不被
//! 破坏），否则换首个模型，退出时按进入时记录的原值恢复。
//!
//! 补丁以现有文件为底：解析失败报错、绝不从空文档写起（`LiveWriteError::Parse`）。

use std::path::Path;

use serde_json::{json, Value};

use crate::error::AppError;
use crate::live::patch::{KeyPath, LivePatch, LiveWriteError};

/// CC Switch 写进 `custom_providers` 的路由条目名。这个名字专属 CC Switch，退出代理
/// 按它删除；用户手工建同名条目会被代理模式接管。
pub const ROUTE_ENTRY_NAME: &str = "cc-switch-route";

/// 旧版接管写的条目名与占位 Key（识别残留用：只清这个名字 + 这个占位符的组合）。
pub const LEGACY_TAKEOVER_ENTRY: &str = "cc-switch";
pub const LEGACY_TAKEOVER_TOKEN: &str = "PROXY_MANAGED";

/// 代理条目对 Hermes 声明的协议。本地代理只完整实现 Responses（SSE、`session_id`
/// 会话粘性、`/responses/compact` 压缩），`chat_completions` 留作手改 `api_mode` 的
/// 兜底路由。
pub const PROXY_API_MODE: &str = "codex_responses";

/// `model:` 段的目标状态。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelTarget {
    /// 进入代理：`provider` 指向路由条目；`default` 为 `Some` 时显式落这个模型
    /// （聚合模式取默认那家的首个发布模型），为 `None` 时按规则：现有默认在发布
    /// 列表里就保留，否则换首个模型（没有模型就不动）。
    Route { default: Option<String> },
    /// 退出代理：`provider` 恢复记录值（`None` = 原本没有这个键，删掉，Hermes 回落
    /// 内置供应商）；`default` 有记录值就恢复，`None` = 不动（原本没有默认的 home
    /// 保留路由期的可用默认，比删掉后无默认更稳）。
    Restore {
        provider: Option<String>,
        default: Option<String>,
    },
    /// 退出代理但这个 home 没有进入时的记录（进入之后新增的 profile）：只删路由
    /// 条目，`model:` 段原样不动。
    Untouched,
}

// ============================================================================
// 角色引用接管：fallback_providers / auxiliary.* / moa.presets.*
// ============================================================================

/// 角色引用的供应商映射：引用名（`custom:X` / 裸 `X` 里的 X，即 DB 供应商 id）→
/// {原模型 id → 路由条目里发布的模型 id}。路由模式只有路由那家；聚合模式是全部成员。
pub type RoleMapping =
    std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>;

/// 角色引用（`fallback_providers` / `auxiliary.*` / `moa.presets.*`）的目标。
#[derive(Debug, Clone, PartialEq)]
pub enum RoleTarget {
    /// 进入代理：先把上一次改写过的点还原成原始引用，再按映射改写到路由条目。
    /// 不在映射里的引用保持直连；`custom:X` 悬空引用（文件里没有 X、映射里也没有）
    /// 从 fallback / moa 名单里清掉。aux 块改写时清掉 `base_url` / `api_key`（它们
    /// 会覆盖 provider，留着就绕过路由条目了），还原时写回。
    Route { mapping: RoleMapping },
    /// 退出代理：按进入时记录的改写清单逐点还原。
    Restore,
}

impl RoleTarget {
    fn mapping(&self) -> Option<&RoleMapping> {
        match self {
            RoleTarget::Route { mapping } => Some(mapping),
            RoleTarget::Restore => None,
        }
    }
}

/// 一次改写的记录（`written.extra["rewrites"][home]` 的元素）。`to_model` 为空串
/// 表示删除该项（悬空清理）；还原时按 `role` + `to_model` 匹配文件里的现值写回
/// `from_*`。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RoleRewrite {
    pub role: String,
    pub from_provider: String,
    pub from_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_api_key: Option<String>,
    pub to_model: String,
}

/// 一个 home 里三段角色区的新值（`None` = 该段没有变化，不必写）与本轮改写清单。
#[derive(Debug, Default)]
pub(crate) struct RoleOutcome {
    pub fallback: Option<serde_yaml::Value>,
    pub auxiliary: Option<serde_yaml::Value>,
    pub moa: Option<serde_yaml::Value>,
    pub rewrites: Vec<RoleRewrite>,
}

/// 解析引用名：`custom:X` / `builtin:X` / 裸 `X` → (`X`, 是否 custom 前缀)。
fn parse_provider_ref(value: &str) -> Option<(String, bool)> {
    let trimmed = value.trim();
    if let Some(name) = trimmed.strip_prefix("custom:") {
        (!name.is_empty()).then(|| (name.to_string(), true))
    } else if let Some(name) = trimmed.strip_prefix("builtin:") {
        (!name.is_empty()).then(|| (name.to_string(), false))
    } else {
        (!trimmed.is_empty()).then(|| (trimmed.to_string(), false))
    }
}

/// 单个引用点的处置。
enum Action {
    /// 改写到路由条目（to_model 为发布 id；clear_overrides = aux 的 base_url/api_key）
    Rewrite {
        to_model: String,
        clear_overrides: bool,
    },
    /// 按记录还原（含悬空删除：to_model 空）
    Restore {
        rewrite: RoleRewrite,
    },
    Keep,
}

/// 计算一个引用点的处置。`provider` / `model` 是**还原语义后**的现值（已指路由条目
/// 的点先按记录还原成原始引用再判断）。
fn resolve_ref(
    role: &str,
    provider: &str,
    model: &str,
    has_base_url: bool,
    has_api_key: bool,
    custom_names: &std::collections::BTreeSet<String>,
    target: &RoleTarget,
    recorded: &[RoleRewrite],
    droppable: bool,
) -> Action {
    // 还原语义：现值是路由条目 → 先按 to_model 反查原始引用
    let (provider, model, restoring) = if provider.trim() == ROUTE_ENTRY_NAME {
        match recorded
            .iter()
            .find(|r| r.role == role && r.to_model == model)
        {
            Some(hit) => (hit.from_provider.as_str(), hit.from_model.as_str(), true),
            // 用户自己指到路由条目、没有我们的记录：不动
            None => return Action::Keep,
        }
    } else {
        (provider, model, false)
    };

    match target {
        RoleTarget::Restore => {
            // 只有我们改写过的点才还原（Restoring 或 from 匹配）
            if restoring {
                if let Some(hit) = recorded.iter().find(|r| {
                    r.role == role && r.from_provider == provider && r.from_model == model
                }) {
                    return Action::Restore {
                        rewrite: hit.clone(),
                    };
                }
            }
            Action::Keep
        }
        RoleTarget::Route { mapping } => {
            let Some((name, is_custom)) = parse_provider_ref(provider) else {
                return Action::Keep;
            };
            // 悬空 custom 引用：文件里没有这个条目、映射里也没有 → 清理（仅列表型角色）
            if is_custom && !custom_names.contains(&name) && !mapping.contains_key(&name) {
                if droppable {
                    return Action::Rewrite {
                        to_model: String::new(),
                        clear_overrides: false,
                    };
                }
                return Action::Keep;
            }
            let Some(models) = mapping.get(&name) else {
                return Action::Keep; // 未接管：保持直连
            };
            let to_model = if model.is_empty() {
                // model 为空 = 供应商默认 → 路由条目的默认（发布列表首个）
                models.values().next().cloned().unwrap_or_default()
            } else {
                match models.get(model) {
                    Some(published) => published.clone(),
                    // 供应商在映射里但没发布这个模型：保持直连
                    None => return Action::Keep,
                }
            };
            if to_model.is_empty() {
                return Action::Keep;
            }
            Action::Rewrite {
                to_model,
                // aux 块的 base_url / api_key 会覆盖 provider，必须清
                clear_overrides: has_base_url || has_api_key,
            }
        }
    }
}

/// 按目标处理一个 home 的三段角色区（纯函数：capture 与 patch.apply 共用，输入
/// 相同则输出相同）。`doc` 是该 home 解析后的整份配置；`recorded` 是这个 home 已
/// 有的改写记录（第一轮为空）。
pub(crate) fn apply_role_target(
    doc: &serde_yaml::Value,
    custom_names: &std::collections::BTreeSet<String>,
    target: &RoleTarget,
    recorded: &[RoleRewrite],
) -> RoleOutcome {
    use serde_yaml::Value as Yaml;
    let mut out = RoleOutcome::default();
    let text = |v: &Yaml, key: &str| {
        v.get(key)
            .and_then(Yaml::as_str)
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    };
    let has = |v: &Yaml, key: &str| v.get(key).is_some_and(|x| !x.is_null());

    // ---- fallback_providers ----
    if let Some(list) = doc.get("fallback_providers").and_then(Yaml::as_sequence) {
        let mut new_list = Vec::with_capacity(list.len());
        let mut changed = false;
        for item in list {
            let mut item = item.clone();
            let provider = text(&item, "provider");
            let model = text(&item, "model");
            let action = resolve_ref(
                "fallback",
                &provider,
                &model,
                false,
                false,
                custom_names,
                target,
                recorded,
                true,
            );
            match action {
                Action::Rewrite { to_model, .. } if to_model.is_empty() => {
                    changed = true; // 悬空：删除
                    out.rewrites.push(RoleRewrite {
                        role: "fallback".to_string(),
                        from_provider: provider,
                        from_model: model,
                        from_base_url: None,
                        from_api_key: None,
                        to_model: String::new(),
                    });
                    continue;
                }
                Action::Rewrite { to_model, .. } => {
                    let mapping = item.as_mapping_mut().expect("fallback 项是 mapping");
                    mapping.insert(
                        Yaml::String("provider".into()),
                        Yaml::String(ROUTE_ENTRY_NAME.into()),
                    );
                    mapping.insert(Yaml::String("model".into()), Yaml::String(to_model.clone()));
                    out.rewrites.push(RoleRewrite {
                        role: "fallback".to_string(),
                        from_provider: provider,
                        from_model: model,
                        from_base_url: None,
                        from_api_key: None,
                        to_model,
                    });
                    changed = true;
                }
                Action::Restore { rewrite } => {
                    let mapping = item.as_mapping_mut().expect("fallback 项是 mapping");
                    mapping.insert(
                        Yaml::String("provider".into()),
                        Yaml::String(rewrite.from_provider.clone()),
                    );
                    mapping.insert(
                        Yaml::String("model".into()),
                        Yaml::String(rewrite.from_model.clone()),
                    );
                    changed = true;
                }
                Action::Keep => {}
            }
            new_list.push(item);
        }
        // 还原时补回被清掉的悬空项（to_model 为空的记录是删除，文件里已没有它们）
        if matches!(target, RoleTarget::Restore) {
            for rewrite in recorded
                .iter()
                .filter(|r| r.role == "fallback" && r.to_model.is_empty())
            {
                let mut item = serde_yaml::Mapping::new();
                item.insert(
                    Yaml::String("provider".into()),
                    Yaml::String(rewrite.from_provider.clone()),
                );
                item.insert(
                    Yaml::String("model".into()),
                    Yaml::String(rewrite.from_model.clone()),
                );
                new_list.push(Yaml::Mapping(item));
                changed = true;
            }
        }
        if changed {
            out.fallback = Some(Yaml::Sequence(new_list));
        }
    }

    // ---- auxiliary.<task> ----
    if let Some(aux) = doc.get("auxiliary").and_then(Yaml::as_mapping) {
        let mut new_aux = aux.clone();
        let mut changed = false;
        for (task, block) in aux.iter() {
            let Some(block_map) = block.as_mapping() else {
                continue;
            };
            if !block_map.contains_key(&Yaml::String("provider".into())) {
                continue;
            }
            let task_name = task.as_str().unwrap_or_default().to_string();
            let role = format!("aux:{task_name}");
            let provider = block
                .get("provider")
                .and_then(Yaml::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if provider == "auto" || provider.is_empty() {
                continue; // 继承主模型（已接管）
            }
            let model = text(block, "model");
            let h_base = has(block, "base_url");
            let h_key = has(block, "api_key");
            let action = resolve_ref(
                &role,
                &provider,
                &model,
                h_base,
                h_key,
                custom_names,
                target,
                recorded,
                false,
            );
            let mut new_block = block.clone();
            match action {
                Action::Rewrite {
                    to_model,
                    clear_overrides,
                } => {
                    let mapping = new_block.as_mapping_mut().expect("aux 块是 mapping");
                    mapping.insert(
                        Yaml::String("provider".into()),
                        Yaml::String(ROUTE_ENTRY_NAME.into()),
                    );
                    mapping.insert(Yaml::String("model".into()), Yaml::String(to_model.clone()));
                    if clear_overrides {
                        mapping.remove(Yaml::String("base_url".into()));
                        mapping.remove(Yaml::String("api_key".into()));
                    }
                    out.rewrites.push(RoleRewrite {
                        role: role.clone(),
                        from_provider: provider,
                        from_model: model,
                        from_base_url: h_base.then(|| text(block, "base_url")),
                        from_api_key: h_key.then(|| text(block, "api_key")),
                        to_model,
                    });
                    changed = true;
                }
                Action::Restore { rewrite } => {
                    let mapping = new_block.as_mapping_mut().expect("aux 块是 mapping");
                    mapping.insert(
                        Yaml::String("provider".into()),
                        Yaml::String(rewrite.from_provider.clone()),
                    );
                    mapping.insert(
                        Yaml::String("model".into()),
                        Yaml::String(rewrite.from_model.clone()),
                    );
                    if let Some(value) = rewrite.from_base_url.as_deref() {
                        mapping.insert(
                            Yaml::String("base_url".into()),
                            Yaml::String(value.to_string()),
                        );
                    }
                    if let Some(value) = rewrite.from_api_key.as_deref() {
                        mapping.insert(
                            Yaml::String("api_key".into()),
                            Yaml::String(value.to_string()),
                        );
                    }
                    changed = true;
                }
                Action::Keep => {}
            }
            if changed_out(&new_block, block) {
                new_aux.insert(Yaml::String(task_name), new_block);
            }
        }
        if changed {
            out.auxiliary = Some(Yaml::Mapping(new_aux));
        }
    }

    // ---- moa.presets.<p> ----
    if let Some(moa) = doc.get("moa").and_then(Yaml::as_mapping) {
        let mut new_moa = moa.clone();
        let mut changed = false;
        if let Some(presets) = moa
            .get(&Yaml::String("presets".into()))
            .and_then(Yaml::as_mapping)
        {
            let mut new_presets = presets.clone();
            for (preset, block) in presets.iter() {
                let preset_name = preset.as_str().unwrap_or_default().to_string();
                let mut new_block = block.clone();
                // reference_models[]
                if let Some(list) = block
                    .get(&Yaml::String("reference_models".into()))
                    .and_then(Yaml::as_sequence)
                {
                    let mut new_list = Vec::with_capacity(list.len());
                    for item in list.iter() {
                        let mut item = item.clone();
                        let role = format!("moa:{preset_name}:ref");
                        let provider = text(&item, "provider");
                        let model = text(&item, "model");
                        let action = resolve_ref(
                            &role,
                            &provider,
                            &model,
                            false,
                            false,
                            custom_names,
                            target,
                            recorded,
                            true,
                        );
                        match action {
                            Action::Rewrite { to_model, .. } if to_model.is_empty() => {
                                changed = true;
                                out.rewrites.push(RoleRewrite {
                                    role,
                                    from_provider: provider,
                                    from_model: model,
                                    from_base_url: None,
                                    from_api_key: None,
                                    to_model: String::new(),
                                });
                                continue;
                            }
                            Action::Rewrite { to_model, .. } => {
                                let mapping = item.as_mapping_mut().expect("moa 项是 mapping");
                                mapping.insert(
                                    Yaml::String("provider".into()),
                                    Yaml::String(ROUTE_ENTRY_NAME.into()),
                                );
                                mapping.insert(
                                    Yaml::String("model".into()),
                                    Yaml::String(to_model.clone()),
                                );
                                out.rewrites.push(RoleRewrite {
                                    role,
                                    from_provider: provider,
                                    from_model: model,
                                    from_base_url: None,
                                    from_api_key: None,
                                    to_model,
                                });
                                changed = true;
                            }
                            Action::Restore { rewrite } => {
                                let mapping = item.as_mapping_mut().expect("moa 项是 mapping");
                                mapping.insert(
                                    Yaml::String("provider".into()),
                                    Yaml::String(rewrite.from_provider.clone()),
                                );
                                mapping.insert(
                                    Yaml::String("model".into()),
                                    Yaml::String(rewrite.from_model.clone()),
                                );
                                changed = true;
                            }
                            Action::Keep => {}
                        }
                        new_list.push(item);
                    }
                    // 还原时补回被清掉的悬空成员
                    if matches!(target, RoleTarget::Restore) {
                        let role = format!("moa:{preset_name}:ref");
                        for rewrite in recorded
                            .iter()
                            .filter(|r| r.role == role && r.to_model.is_empty())
                        {
                            let mut item = serde_yaml::Mapping::new();
                            item.insert(
                                Yaml::String("provider".into()),
                                Yaml::String(rewrite.from_provider.clone()),
                            );
                            item.insert(
                                Yaml::String("model".into()),
                                Yaml::String(rewrite.from_model.clone()),
                            );
                            new_list.push(Yaml::Mapping(item));
                            changed = true;
                        }
                    }
                    new_block.as_mapping_mut().expect("moa 块").insert(
                        Yaml::String("reference_models".into()),
                        Yaml::Sequence(new_list),
                    );
                }
                // aggregator
                if let Some(agg) = block.get(&Yaml::String("aggregator".into())) {
                    let mut new_agg = agg.clone();
                    let role = format!("moa:{preset_name}:agg");
                    let provider = text(agg, "provider");
                    let model = text(agg, "model");
                    let action = resolve_ref(
                        &role,
                        &provider,
                        &model,
                        false,
                        false,
                        custom_names,
                        target,
                        recorded,
                        false,
                    );
                    match action {
                        Action::Rewrite { to_model, .. } => {
                            let mapping = new_agg.as_mapping_mut().expect("agg 是 mapping");
                            mapping.insert(
                                Yaml::String("provider".into()),
                                Yaml::String(ROUTE_ENTRY_NAME.into()),
                            );
                            mapping.insert(
                                Yaml::String("model".into()),
                                Yaml::String(to_model.clone()),
                            );
                            out.rewrites.push(RoleRewrite {
                                role,
                                from_provider: provider,
                                from_model: model,
                                from_base_url: None,
                                from_api_key: None,
                                to_model,
                            });
                            changed = true;
                        }
                        Action::Restore { rewrite } => {
                            let mapping = new_agg.as_mapping_mut().expect("agg 是 mapping");
                            mapping.insert(
                                Yaml::String("provider".into()),
                                Yaml::String(rewrite.from_provider.clone()),
                            );
                            mapping.insert(
                                Yaml::String("model".into()),
                                Yaml::String(rewrite.from_model.clone()),
                            );
                            changed = true;
                        }
                        Action::Keep => {}
                    }
                    new_block
                        .as_mapping_mut()
                        .expect("moa 块")
                        .insert(Yaml::String("aggregator".into()), new_agg);
                }
                new_presets.insert(Yaml::String(preset_name), new_block);
            }
            new_moa.insert(Yaml::String("presets".into()), Yaml::Mapping(new_presets));
        }
        if changed {
            out.moa = Some(Yaml::Mapping(new_moa));
        }
    }

    out
}

/// 块内容是否真的变了（serde_yaml::Value 的 PartialEq 对 mapping 顺序敏感——
/// 用序列化文本比较）。
fn changed_out(new: &serde_yaml::Value, old: &serde_yaml::Value) -> bool {
    serde_yaml::to_string(new).ok() != serde_yaml::to_string(old).ok()
}

/// 要写进 live 的目标状态：`custom_providers` 里的路由条目 + `model:` 段的指向。
///
/// 投影是纯函数（不读盘）；和现有文件合并发生在补丁 [`HermesConfigPatch`] 里：
/// `model:` 段的其余键（`context_length` / `max_tokens` / 未知键）以现有文件为底保留。
#[derive(Debug, Clone, PartialEq)]
pub struct HermesProjection {
    /// 写进 `custom_providers` 的路由条目（JSON，`models` 用 UI 数组形态，落盘前由
    /// [`crate::hermes_config::build_custom_provider_entry`] 规范化）；`None` = 删掉
    /// CC Switch 写的那条（退出代理）。
    pub entry: Option<Value>,
    /// `model:` 段的目标。
    pub model: ModelTarget,
    /// 角色引用（fallback / auxiliary / moa）的目标。
    pub roles: RoleTarget,
}

impl HermesProjection {
    /// 路由模式的角色映射：{原模型 id → 原模型 id}（条目按原样发布）。
    fn identity_mapping(provider: &str, models: &Value) -> RoleMapping {
        let ids: Vec<String> = models
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        std::iter::once((
            provider.to_string(),
            ids.into_iter().map(|id| (id.clone(), id)).collect(),
        ))
        .collect()
    }

    /// 代理契约（路由模式）：发布路由供应商声明的模型列表；角色映射只有路由那家
    /// （模型 id 原样发布）。
    pub fn proxy_contract(
        route_settings: &Value,
        route_id: &str,
        proxy_base_url: &str,
        placeholder: &str,
    ) -> Result<Self, AppError> {
        let models = route_settings
            .get("models")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let mapping = Self::identity_mapping(route_id, &models);
        Self::proxy_contract_with_models(models, None, mapping, proxy_base_url, placeholder)
    }

    /// 代理契约（聚合模式）：发布 Stack 成员的模型（`models` 是带 `ccs-h-<key>/` 前缀的
    /// UI 数组形态，由 `mode::stack::hermes_published` 转换而来）。
    pub fn proxy_contract_with_models(
        models: Value,
        default_model: Option<String>,
        roles: RoleMapping,
        proxy_base_url: &str,
        placeholder: &str,
    ) -> Result<Self, AppError> {
        let entry = json!({
            "name": ROUTE_ENTRY_NAME,
            "base_url": proxy_base_url,
            "api_key": placeholder,
            "api_mode": PROXY_API_MODE,
            // Hermes 对自定义供应商默认不带任何会话标识（#86241：端点没要就不发）。
            // 声明这个 opt-in 字段后，每轮请求（含压缩/标题等辅助调用）都会带
            // `session_id: <会话根 id>` 头，本地代理据此做会话粘性负载均衡；
            // 不认识该字段的旧版 Hermes 会忽略它，无副作用。
            "session_affinity_header": "session_id",
            "models": models,
        });
        Ok(Self {
            entry: Some(entry),
            model: ModelTarget::Route {
                default: default_model,
            },
            roles: RoleTarget::Route { mapping: roles },
        })
    }

    /// 默认 home 的直连投影：删路由条目，`model:` 段指回直连指针那家（没有就删掉
    /// 指向）。默认 home 是 CC Switch 托管的面（切换供应商就写它），按直连指针恢复。
    pub fn direct(direct: Option<(&str, &Value)>) -> Self {
        match direct {
            Some((id, settings)) => Self {
                entry: None,
                model: ModelTarget::Restore {
                    provider: Some(id.to_string()),
                    default: first_model_id(settings),
                },
                roles: RoleTarget::Restore,
            },
            None => Self {
                entry: None,
                model: ModelTarget::Restore {
                    provider: None,
                    default: None,
                },
                roles: RoleTarget::Restore,
            },
        }
    }

    /// 契约摘要用的规范形式（只反映进入代理的目标状态）。
    pub fn to_value(&self) -> Value {
        json!({
            "entry": self.entry,
            "model": match &self.model {
                ModelTarget::Route { default } => json!({ "route": default }),
                ModelTarget::Restore { .. } => json!("restore"),
                ModelTarget::Untouched => json!("untouched"),
            },
            "roles": match &self.roles {
                RoleTarget::Route { mapping } => {
                    serde_json::to_value(mapping).unwrap_or(Value::Null)
                }
                RoleTarget::Restore => json!("restore"),
            },
        })
    }
}

/// 行里声明的第一个模型 id（UI 数组形态）；没有声明模型时为 `None`。
fn first_model_id(settings: &Value) -> Option<String> {
    settings
        .get("models")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|m| m.get("id"))
        .and_then(|id| id.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// 按投影改写 `config.yaml`：路由条目 upsert / 删除（顺带清旧接管残留），`model:` 段
/// 换指向，角色引用（fallback / auxiliary / moa）按 [`RoleTarget`] 接管或还原。
/// `recorded` 是这个 home 已有的改写记录（`written.extra["rewrites"][home]`）。
pub struct HermesConfigPatch<'a> {
    projection: &'a HermesProjection,
    recorded: Vec<RoleRewrite>,
}

impl<'a> HermesConfigPatch<'a> {
    pub fn new(projection: &'a HermesProjection) -> Self {
        Self {
            projection,
            recorded: Vec::new(),
        }
    }

    pub fn with_recorded(projection: &'a HermesProjection, recorded: Vec<RoleRewrite>) -> Self {
        Self {
            projection,
            recorded,
        }
    }
}

impl LivePatch for HermesConfigPatch<'_> {
    fn apply(&self, path: &Path, pre: Option<&[u8]>) -> Result<Vec<u8>, LiveWriteError> {
        use serde_yaml::Value as Yaml;

        let raw = pre
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        let config: Yaml =
            serde_yaml::from_str(&raw).map_err(|error| yaml_parse_error(path, &error))?;

        // 1) custom_providers：路由条目 upsert（按 name 匹配）或删除；进入代理时顺带
        //    清掉旧版接管的残留条目（同名 cc-switch + PROXY_MANAGED 占位 Key）。
        let mut providers: Vec<Yaml> = config
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .cloned()
            .unwrap_or_default();
        let removing = self.projection.entry.is_none();
        match &self.projection.entry {
            Some(entry) => {
                let yaml_entry = crate::hermes_config::build_custom_provider_entry(
                    ROUTE_ENTRY_NAME,
                    entry.clone(),
                )
                .map_err(|_| LiveWriteError::Shape {
                    path: path.to_path_buf(),
                    key_path: KeyPath::new(&["custom_providers", ROUTE_ENTRY_NAME]),
                    expected: "合法的供应商条目",
                })?;
                let published: Vec<String> = entry
                    .get("models")
                    .and_then(|v| v.as_array())
                    .map(|models| {
                        models
                            .iter()
                            .filter_map(|m| m.get("id").and_then(Value::as_str))
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                // 先清两代接管残留，再落现行条目——顺序反了会把刚写入的条目清掉
                providers.retain(|p| !is_legacy_takeover_entry(p));
                match providers
                    .iter_mut()
                    .find(|p| p.get("name").and_then(Yaml::as_str) == Some(ROUTE_ENTRY_NAME))
                {
                    Some(existing) => *existing = yaml_entry,
                    None => providers.push(yaml_entry),
                }
                let _ = published;
            }
            None => {
                providers.retain(|p| p.get("name").and_then(Yaml::as_str) != Some(ROUTE_ENTRY_NAME))
            }
        }
        if providers.is_empty() && config.get("custom_providers").is_none() && !removing {
            // 原本没有这一段且要写条目：照常写。
        }
        let mut raw = crate::hermes_config::replace_yaml_section(
            &raw,
            "custom_providers",
            &Yaml::Sequence(providers),
        )
        .map_err(|_| LiveWriteError::Shape {
            path: path.to_path_buf(),
            key_path: KeyPath::new(&["custom_providers"]),
            expected: "可序列化的 YAML 段",
        })?;

        // 2) model 段：以现有内容为底，按目标换 provider / default。
        let had_model = config.get("model").is_some_and(|v| !v.is_null());
        match &self.projection.model {
            ModelTarget::Untouched => {}
            model_target => {
                let mut model = config
                    .get("model")
                    .and_then(|v| v.as_mapping())
                    .cloned()
                    .unwrap_or_default();
                let (provider, default) = match model_target {
                    ModelTarget::Route { default: explicit } => {
                        let provider = Some(ROUTE_ENTRY_NAME.to_string());
                        match explicit {
                            Some(value) => (provider, Some(value.clone())),
                            None => {
                                let published: Vec<String> = self
                                    .projection
                                    .entry
                                    .as_ref()
                                    .and_then(|entry| entry.get("models"))
                                    .and_then(|v| v.as_array())
                                    .map(|models| {
                                        models
                                            .iter()
                                            .filter_map(|m| m.get("id").and_then(Value::as_str))
                                            .map(String::from)
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let keep = model
                                    .get(&Yaml::String("default".to_string()))
                                    .and_then(Yaml::as_str)
                                    .is_some_and(|current| published.iter().any(|m| m == current));
                                let fallback = published.first().cloned();
                                (provider, if keep { None } else { fallback })
                            }
                        }
                    }
                    ModelTarget::Restore { provider, default } => {
                        (provider.clone(), default.clone())
                    }
                    ModelTarget::Untouched => unreachable!(),
                };
                if !had_model && provider.is_none() && default.is_none() {
                    return Ok(raw.into_bytes());
                }
                let key = Yaml::String("provider".to_string());
                match provider {
                    Some(value) => {
                        model.insert(key, Yaml::String(value));
                    }
                    None => {
                        model.remove(&key);
                    }
                }
                if let Some(value) = default {
                    model.insert(Yaml::String("default".to_string()), Yaml::String(value));
                }
                raw = crate::hermes_config::replace_yaml_section(
                    &raw,
                    "model",
                    &Yaml::Mapping(model),
                )
                .map_err(|_| LiveWriteError::Shape {
                    path: path.to_path_buf(),
                    key_path: KeyPath::new(&["model"]),
                    expected: "可序列化的 YAML 段",
                })?;
            }
        }

        // 3) 角色引用（fallback_providers / auxiliary / moa）：按目标接管或还原。
        let custom_names: std::collections::BTreeSet<String> = config
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(Yaml::as_str))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let outcome = apply_role_target(
            &config,
            &custom_names,
            &self.projection.roles,
            &self.recorded,
        );
        for (section, value) in [
            ("fallback_providers", outcome.fallback),
            ("auxiliary", outcome.auxiliary),
            ("moa", outcome.moa),
        ] {
            if let Some(value) = value {
                raw = crate::hermes_config::replace_yaml_section(&raw, section, &value).map_err(
                    |_| LiveWriteError::Shape {
                        path: path.to_path_buf(),
                        key_path: KeyPath::new(&[section]),
                        expected: "可序列化的 YAML 段",
                    },
                )?;
            }
        }

        Ok(raw.into_bytes())
    }
}

/// 旧版接管残留：占位 Key 是 `PROXY_MANAGED` 的路由条目。条目名有两代——
/// 官方旧接管的 `cc-switch` 与现行路由的 `cc-switch-route`（profile 里上次
/// 接管留下的同名条目）。调用方先清后写，现行条目由本次 upsert 重新落位。
fn is_legacy_takeover_entry(entry: &serde_yaml::Value) -> bool {
    let name = entry.get("name").and_then(|v| v.as_str());
    (name == Some(LEGACY_TAKEOVER_ENTRY) || name == Some(ROUTE_ENTRY_NAME))
        && entry.get("api_key").and_then(|v| v.as_str()) == Some(LEGACY_TAKEOVER_TOKEN)
}

/// serde_yaml 解析错误 → 补丁层的 `Parse`（带行列），绝不退回空文档。
fn yaml_parse_error(path: &Path, error: &serde_yaml::Error) -> LiveWriteError {
    let (line, column) = error
        .location()
        .map(|location| (location.line(), location.column()))
        .unwrap_or((0, 0));
    LiveWriteError::Parse {
        path: path.to_path_buf(),
        line,
        column,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route_settings() -> Value {
        json!({
            "name": "kimi",
            "base_url": "https://api.kimi.com/coding/",
            "api_key": "sk-live",
            "api_mode": "codex_responses",
            "models": [
                { "id": "kimi-k2-0905", "context_length": 262144 },
                { "id": "kimi-k2-thinking", "context_length": 262144 },
            ],
        })
    }

    fn apply_on(raw: &str, projection: &HermesProjection) -> String {
        let patch = HermesConfigPatch::new(projection);
        let bytes = patch
            .apply(Path::new("/tmp/config.yaml"), Some(raw.as_bytes()))
            .expect("patch applies");
        String::from_utf8(bytes).expect("utf-8")
    }

    fn proxy_projection() -> HermesProjection {
        HermesProjection::proxy_contract(
            &route_settings(),
            "kimi",
            "http://127.0.0.1:15721/hermes/v1",
            "sk-cc-switch-managed",
        )
        .expect("contract")
    }

    #[test]
    fn proxy_patch_upserts_route_entry_and_points_model_at_it() {
        let raw = "model:\n  default: kimi-k2-0905\n  provider: kimi\ncustom_providers:\n  - name: kimi\n    base_url: https://api.kimi.com/coding/\n    api_key: sk-live\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        let entry = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .find(|p| p.get("name").and_then(|n| n.as_str()) == Some(ROUTE_ENTRY_NAME))
            .expect("route entry present");
        assert_eq!(
            entry.get("base_url").and_then(|v| v.as_str()),
            Some("http://127.0.0.1:15721/hermes/v1")
        );
        assert_eq!(
            entry.get("api_mode").and_then(|v| v.as_str()),
            Some("codex_responses")
        );
        assert_eq!(
            entry
                .get("session_affinity_header")
                .and_then(|v| v.as_str()),
            Some("session_id")
        );
        assert!(entry.get("models").and_then(|v| v.as_mapping()).is_some());
        assert_eq!(
            entry.get("model").and_then(|v| v.as_str()),
            Some("kimi-k2-0905")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some(ROUTE_ENTRY_NAME)
        );
        assert!(yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .any(|p| p.get("name").and_then(|n| n.as_str()) == Some("kimi")));
    }

    #[test]
    fn proxy_patch_keeps_default_when_route_still_serves_it() {
        // profile home 的默认模型在路由那家的列表里：保留，不换首模型。
        let raw = "model:\n  default: kimi-k2-thinking\n  provider: kimi\ncustom_providers: []\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-thinking")
        );
    }

    #[test]
    fn proxy_patch_swaps_default_route_does_not_serve() {
        let raw = "model:\n  default: glm-5.3-flash\n  provider: kimi\ncustom_providers: []\n";
        let out = apply_on(raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-0905"),
            "路由不提供的默认模型换成首个发布模型"
        );
    }

    #[test]
    fn proxy_patch_removes_legacy_takeover_entry() {
        let raw = format!(
            "model:\n  default: aim\n  provider: custom:{LEGACY_TAKEOVER_ENTRY}\ncustom_providers:\n  - name: {LEGACY_TAKEOVER_ENTRY}\n    api_key: {LEGACY_TAKEOVER_TOKEN}\n    base_url: http://127.0.0.1:15721\n  - name: mine\n"
        );
        let out = apply_on(&raw, &proxy_projection());
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        let names: Vec<&str> = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
            .collect();
        assert!(!names.contains(&LEGACY_TAKEOVER_ENTRY), "旧接管残留被清掉");
        assert!(names.contains(&"mine") && names.contains(&ROUTE_ENTRY_NAME));
    }

    #[test]
    fn direct_patch_removes_route_entry_and_restores_model_provider() {
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: kimi-k2-0905\ncustom_providers:\n  - name: kimi\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection::direct(Some(("kimi", &route_settings())));
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        let providers = yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers");
        assert_eq!(providers.len(), 1, "路由条目被删，用户条目保留");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("kimi")
        );
    }

    #[test]
    fn direct_patch_restores_recorded_profile_state() {
        // profile home 的恢复：按进入时记录的原值，而不是直连指针。
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: glm-5.3-flash\n  context_length: 131072\ncustom_providers:\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection {
            entry: None,
            model: ModelTarget::Restore {
                provider: Some("custom:cc-switch".to_string()),
                default: Some("aim".to_string()),
            },
            roles: RoleTarget::Restore,
        };
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("custom:cc-switch")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("aim")
        );
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("context_length"))
                .and_then(|v| v.as_i64()),
            Some(131072),
            "model 段其余键保留"
        );
    }

    #[test]
    fn direct_patch_untouched_leaves_model_section_alone() {
        let raw = format!(
            "model:\n  provider: other\n  default: whatever\ncustom_providers:\n  - name: other\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection {
            entry: None,
            model: ModelTarget::Untouched,
            roles: RoleTarget::Restore,
        };
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("provider"))
                .and_then(|v| v.as_str()),
            Some("other"),
            "没有记录的 home 只删条目，model 段不动"
        );
        assert!(yaml
            .get("custom_providers")
            .and_then(|v| v.as_sequence())
            .expect("custom_providers")
            .iter()
            .all(|p| p.get("name").and_then(|n| n.as_str()) != Some(ROUTE_ENTRY_NAME)));
    }

    #[test]
    fn direct_patch_without_provider_clears_model_provider() {
        let raw = format!(
            "model:\n  provider: {ROUTE_ENTRY_NAME}\n  default: kimi-k2-0905\ncustom_providers:\n  - name: {ROUTE_ENTRY_NAME}\n"
        );
        let projection = HermesProjection::direct(None);
        let out = apply_on(&raw, &projection);
        let yaml: serde_yaml::Value = serde_yaml::from_str(&out).expect("parses");
        assert!(yaml.get("model").and_then(|m| m.get("provider")).is_none());
        assert_eq!(
            yaml.get("model")
                .and_then(|m| m.get("default"))
                .and_then(|v| v.as_str()),
            Some("kimi-k2-0905")
        );
    }

    fn role_mapping() -> RoleMapping {
        // 聚合形态：kimi 的模型带 ccs-h- 前缀发布
        [(
            "kimi".to_string(),
            [("kimi-k2".to_string(), "ccs-h-kimi/kimi-k2".to_string())]
                .into_iter()
                .collect(),
        )]
        .into_iter()
        .collect()
    }

    fn role_raw() -> String {
        format!(
            "model:\n  default: kimi-k2\n  provider: cc-switch-route\ncustom_providers:\n  - name: kimi\n  - name: moa-mglm\nfallback_providers:\n  - provider: custom:kimi\n    model: kimi-k2\n  - provider: custom:moa-mglm\n    model: glm-5.3\n  - provider: custom:cc-switch\n    model: aim\nauxiliary:\n  vision:\n    provider: custom:kimi\n    model: kimi-k2\n    base_url: https://direct.example/v1\n    api_key: sk-direct\n  title_generation:\n    provider: auto\nmoa:\n  presets:\n    heavy:\n      reference_models:\n        - provider: custom:kimi\n          model: kimi-k2\n          enabled: true\n        - provider: custom:moa-mglm\n          model: glm-5.3\n      aggregator:\n        provider: custom:kimi\n        model: kimi-k2\n"
        )
    }

    fn role_patch(target: RoleTarget, recorded: &[RoleRewrite]) -> HermesConfigPatch<'static> {
        // 投影只承载 roles；entry 用 None（角色逻辑独立于条目写盘）
        let projection = Box::leak(Box::new(HermesProjection {
            entry: None,
            model: ModelTarget::Untouched,
            roles: target,
        }));
        HermesConfigPatch::with_recorded(projection, recorded.to_vec())
    }

    #[test]
    fn roles_route_rewrites_known_refs_and_cleans_dangling() {
        let patch = role_patch(
            RoleTarget::Route {
                mapping: role_mapping(),
            },
            &[],
        );
        let out = patch
            .apply(Path::new("/tmp/config.yaml"), Some(role_raw().as_bytes()))
            .expect("applies");
        let yaml: serde_yaml::Value = serde_yaml::from_slice(&out).expect("parses");

        // fallback：kimi 接管（发布 id 带前缀），moa-mglm 不在映射保持直连，悬空被删
        let fallback = yaml["fallback_providers"].as_sequence().unwrap();
        assert_eq!(
            (
                fallback[0]["provider"].as_str(),
                fallback[0]["model"].as_str()
            ),
            (Some(ROUTE_ENTRY_NAME), Some("ccs-h-kimi/kimi-k2")),
            "映射内的引用接管"
        );
        assert_eq!(
            (
                fallback[1]["provider"].as_str(),
                fallback[1]["model"].as_str()
            ),
            (Some("custom:moa-mglm"), Some("glm-5.3")),
            "未接管的保持直连"
        );
        assert_eq!(fallback.len(), 2, "悬空的 custom:cc-switch 被清掉");

        // aux：接管 + base_url/api_key 清空（它们会覆盖 provider 绕过路由）
        let vision = &yaml["auxiliary"]["vision"];
        assert_eq!(
            (vision["provider"].as_str(), vision["model"].as_str()),
            (Some(ROUTE_ENTRY_NAME), Some("ccs-h-kimi/kimi-k2"))
        );
        assert!(vision.get("base_url").is_none() && vision.get("api_key").is_none());
        assert_eq!(
            yaml["auxiliary"]["title_generation"]["provider"].as_str(),
            Some("auto"),
            "auto（继承主模型）不动"
        );

        // moa：引用与聚合器接管，moa-mglm 成员保持
        let refs = yaml["moa"]["presets"]["heavy"]["reference_models"]
            .as_sequence()
            .unwrap();
        assert_eq!(
            (refs[0]["provider"].as_str(), refs[0]["model"].as_str()),
            (Some(ROUTE_ENTRY_NAME), Some("ccs-h-kimi/kimi-k2"))
        );
        assert_eq!(refs[1]["provider"].as_str(), Some("custom:moa-mglm"));
        assert_eq!(
            yaml["moa"]["presets"]["heavy"]["aggregator"]["model"].as_str(),
            Some("ccs-h-kimi/kimi-k2")
        );
    }

    #[test]
    fn roles_restore_round_trips_everything() {
        let enter = role_patch(
            RoleTarget::Route {
                mapping: role_mapping(),
            },
            &[],
        );
        let rewritten = enter
            .apply(Path::new("/tmp/config.yaml"), Some(role_raw().as_bytes()))
            .expect("enter");

        // 记录 = 引擎对本轮的改写清单（capture 与 apply 同一纯函数）
        let doc: serde_yaml::Value = serde_yaml::from_str(&role_raw()).expect("parses");
        let names = ["kimi", "moa-mglm"].into_iter().map(String::from).collect();
        let outcome = apply_role_target(
            &doc,
            &names,
            &RoleTarget::Route {
                mapping: role_mapping(),
            },
            &[],
        );
        assert!(!outcome.rewrites.is_empty());

        let restore = role_patch(RoleTarget::Restore, &outcome.rewrites);
        let out = restore
            .apply(Path::new("/tmp/config.yaml"), Some(&rewritten))
            .expect("restore");
        let yaml: serde_yaml::Value = serde_yaml::from_slice(&out).expect("parses");

        assert_eq!(
            yaml["fallback_providers"][0]["provider"].as_str(),
            Some("custom:kimi")
        );
        assert_eq!(
            yaml["fallback_providers"][0]["model"].as_str(),
            Some("kimi-k2")
        );
        assert_eq!(
            yaml["fallback_providers"].as_sequence().unwrap().len(),
            3,
            "悬空删除项也按记录恢复"
        );
        let vision = &yaml["auxiliary"]["vision"];
        assert_eq!(
            (vision["provider"].as_str(), vision["model"].as_str()),
            (Some("custom:kimi"), Some("kimi-k2"))
        );
        assert_eq!(
            vision["base_url"].as_str(),
            Some("https://direct.example/v1")
        );
        assert_eq!(vision["api_key"].as_str(), Some("sk-direct"));
        assert_eq!(
            yaml["moa"]["presets"]["heavy"]["aggregator"]["model"].as_str(),
            Some("kimi-k2")
        );
    }

    #[test]
    fn roles_user_pinned_route_entry_without_record_is_untouched() {
        let raw = "fallback_providers:\n  - provider: cc-switch-route\n    model: whatever\n";
        let patch = role_patch(
            RoleTarget::Route {
                mapping: role_mapping(),
            },
            &[],
        );
        let out = patch
            .apply(Path::new("/tmp/config.yaml"), Some(raw.as_bytes()))
            .expect("applies");
        let yaml: serde_yaml::Value = serde_yaml::from_slice(&out).expect("parses");
        assert_eq!(
            yaml["fallback_providers"][0]["provider"].as_str(),
            Some("cc-switch-route"),
            "用户自己指路由条目、无记录 → 不动"
        );
    }

    #[test]
    fn unparseable_config_is_never_rewritten_from_scratch() {
        let raw = "custom_providers: [ not closed";
        let projection = HermesProjection::direct(None);
        let patch = HermesConfigPatch::new(&projection);
        let result = patch.apply(Path::new("/tmp/config.yaml"), Some(raw.as_bytes()));
        assert!(matches!(result, Err(LiveWriteError::Parse { .. })));
    }
}
