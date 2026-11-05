//! Claude Code 子代理编辑器里 `model` 字段的下拉选项。
//!
//! 子代理文件的 `model` 可以写 `inherit`、档位别名或完整模型 id，但哪种写法真能用、落到
//! 哪个模型，取决于 CC Switch 现在 Claude 的模式：
//! - 直连：Claude Code 直接拿别名查 `ANTHROPIC_DEFAULT_<档>_MODEL`，没配就把官方 id 原样
//!   发给供应商（不会回落到 `ANTHROPIC_MODEL`，见 `terminal_model_picker`），第三方会报错；
//!   完整 id 原样发出，所以供应商行里配过的模型都能直接选；
//! - 路由：别名按 `route_roles` 的回落规则映射；其余 id 会被代理的默认模型吞掉
//!   （`ModelMapping::map_model`），只有 `CLAUDE_CODE_SUBAGENT_MODEL` 例外，所以只给别名；
//! - 聚合（Stack）：四档别名都指向默认那家的第一个模型，各家发布的 `ccs-claude-*` id 直达那家。
//!
//! 选项的计算是纯函数（[`build_options`]），命令层只负责读模式、供应商和 live 配置。

use serde::Serialize;
use serde_json::{Map, Value};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::DeviceStore;
use crate::live::project::claude::{
    client_model_for_subagent, env_string, route_roles, tier_model, Tier, TierModel,
};
use crate::mode::stack::{self, Member};
use crate::mode::{current, state};
use crate::provider::Provider;
use crate::proxy::model_mapper::strip_one_m_suffix_for_upstream;
use crate::services::provider::claude_official;

const SUBAGENT_MODEL_ENV: &str = "CLAUDE_CODE_SUBAGENT_MODEL";

/// 下拉里别名的顺序：和 Claude Code 文档里子代理 `model` 的写法顺序一致，fable 最后
/// （第三方大多没配这一档）。
const ALIAS_ORDER: [Tier; 4] = [Tier::Sonnet, Tier::Opus, Tier::Haiku, Tier::Fable];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentRoutingMode {
    Direct,
    Route,
    Stack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentModelKind {
    Inherit,
    Alias,
    Model,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentModelWarning {
    /// 直连第三方（或路由那家什么模型都没配）时这一档没有映射：Claude Code 会把官方 id
    /// 原样发给第三方，多半直接报错。
    AliasUnmapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentModelNotice {
    /// 路由模式只列别名：完整 id 会被代理的默认模型吞掉。
    RouteAliasesOnly,
    /// 聚合模式下四档别名都指向默认那家的同一个模型。
    StackAliasesShareDefault,
    /// 读不到当前供应商（直连指针或路由那家）。
    NoProvider,
    /// live 里设了 `CLAUDE_CODE_SUBAGENT_MODEL`：它比子代理文件里的 `model` 优先。
    SubagentModelOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentModelOption {
    /// 写进子代理文件 `model` 的值。
    pub value: String,
    pub kind: SubagentModelKind,
    /// 别名和 `inherit` 就是值本身，由前端本地化；完整 id 是显示名。
    pub label: String,
    /// 实际发往上游的模型（知道时）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolves_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<SubagentModelWarning>,
}

impl SubagentModelOption {
    fn new(value: impl Into<String>, kind: SubagentModelKind, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            kind,
            label: label.into(),
            resolves_to: None,
            provider_name: None,
            description: None,
            warning: None,
        }
    }

    fn inherit() -> Self {
        Self::new("inherit", SubagentModelKind::Inherit, "inherit")
    }

    fn alias(tier: Tier) -> Self {
        Self::new(tier.alias(), SubagentModelKind::Alias, tier.alias())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentModelOptions {
    pub mode: SubagentRoutingMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_name: Option<String>,
    pub is_official: bool,
    pub options: Vec<SubagentModelOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_model_override: Option<String>,
    pub notices: Vec<SubagentModelNotice>,
}

/// [`build_options`] 的输入：现在的模式和它用到的供应商。
#[derive(Debug, Clone, Copy)]
pub enum RoutingInput<'a> {
    /// 直连指针那家。
    Direct(Option<&'a Provider>),
    /// 路由那家。
    Route(Option<&'a Provider>),
    /// 默认那家（代理路由）和发布 Stack 模型的成员（`stack::published_members`）。
    Stack {
        route: Option<&'a Provider>,
        members: &'a [Member],
    },
}

/// 按模式算出下拉选项。`subagent_override` 是 live `settings.json` 里的
/// `CLAUDE_CODE_SUBAGENT_MODEL`。
pub fn build_options(
    input: RoutingInput<'_>,
    subagent_override: Option<&str>,
) -> SubagentModelOptions {
    let (mode, provider) = match input {
        RoutingInput::Direct(provider) => (SubagentRoutingMode::Direct, provider),
        RoutingInput::Route(provider) => (SubagentRoutingMode::Route, provider),
        RoutingInput::Stack { route, .. } => (SubagentRoutingMode::Stack, route),
    };
    let mut out = SubagentModelOptions {
        mode,
        provider_id: provider.map(|provider| provider.id.clone()),
        provider_name: provider.map(|provider| provider.name.clone()),
        is_official: provider.is_some_and(claude_official::is_official),
        options: Vec::new(),
        subagent_model_override: subagent_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        notices: Vec::new(),
    };

    match input {
        RoutingInput::Direct(Some(provider)) => direct_options(provider, &mut out.options),
        RoutingInput::Route(Some(provider)) => {
            route_options(provider, &mut out.options);
            out.notices.push(SubagentModelNotice::RouteAliasesOnly);
        }
        RoutingInput::Stack { route, members } => {
            match (stack::claude_route_default(members), route) {
                (Some(default), _) => {
                    out.options.push(SubagentModelOption::inherit());
                    for tier in ALIAS_ORDER {
                        let mut option = SubagentModelOption::alias(tier);
                        option.resolves_to = Some(default.upstream.clone());
                        option.provider_name = route.map(|route| route.name.clone());
                        option.description = Some(default.description.clone());
                        out.options.push(option);
                    }
                    out.notices
                        .push(SubagentModelNotice::StackAliasesShareDefault);
                }
                // 默认那家没有可发布的模型时，契约的四档按路由模式写（`proxy_projection`
                // 的 `stack_default` 为空），别名也就按路由的回落规则走。
                (None, Some(route)) => route_options(route, &mut out.options),
                (None, None) => bare_options(&mut out.options),
            }
            // 默认那家读不到时各家发布的 id 仍然直达那家，照样列出。
            for member in members {
                for model in stack::claude_models(&member.key, &member.provider) {
                    out.options.push(SubagentModelOption {
                        value: model.id,
                        kind: SubagentModelKind::Model,
                        label: model.name,
                        resolves_to: Some(model.upstream),
                        provider_name: Some(member.provider.name.clone()),
                        description: Some(model.description),
                        warning: None,
                    });
                }
            }
        }
        RoutingInput::Direct(None) | RoutingInput::Route(None) => bare_options(&mut out.options),
    }
    if provider.is_none() {
        out.notices.push(SubagentModelNotice::NoProvider);
    }
    if out.subagent_model_override.is_some() {
        out.notices.push(SubagentModelNotice::SubagentModelOverride);
    }
    out
}

/// 不知道供应商时只给 `inherit` 和四个别名，不猜它们落到哪。
fn bare_options(options: &mut Vec<SubagentModelOption>) {
    options.push(SubagentModelOption::inherit());
    options.extend(ALIAS_ORDER.map(SubagentModelOption::alias));
}

fn provider_env(provider: &Provider) -> Map<String, Value> {
    provider
        .settings_config
        .get("env")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// 直连：别名只认这张卡显式配的档位；第三方还列出行里配过的完整模型 id。
fn direct_options(provider: &Provider, options: &mut Vec<SubagentModelOption>) {
    let env = provider_env(provider);
    let official = claude_official::is_official(provider);

    let mut inherit = SubagentModelOption::inherit();
    inherit.resolves_to = env_string(&env, "ANTHROPIC_MODEL").map(str::to_string);
    options.push(inherit);

    for tier in ALIAS_ORDER {
        let mut option = SubagentModelOption::alias(tier);
        match env_string(&env, tier.model_key()) {
            Some(upstream) => describe_tier(
                &mut option,
                &tier_model(&env, tier, upstream),
                &env,
                provider,
            ),
            // 官方：别名就是官方模型，不用映射。
            None if official => {}
            // 第三方没配 fable 档的很常见，列出来只会多一个必然报错的选项，干脆不列。
            None if tier == Tier::Fable => continue,
            None => option.warning = Some(SubagentModelWarning::AliasUnmapped),
        }
        options.push(option);
    }

    // 官方卡上写完整 id 和选别名没区别，不列。
    if official {
        return;
    }
    let mut found = Vec::new();
    let mut push_env = |model_key: &str, name_key: Option<&str>| {
        if let Some(upstream) = env_string(&env, model_key) {
            let name = name_key.and_then(|key| env_string(&env, key).map(str::to_string));
            stack::push_found(&mut found, upstream, name);
        }
    };
    push_env("ANTHROPIC_MODEL", None);
    for tier in ALIAS_ORDER {
        push_env(tier.model_key(), Some(tier.name_key()));
    }
    push_env("ANTHROPIC_SMALL_FAST_MODEL", None);
    push_env(SUBAGENT_MODEL_ENV, None);
    if let Some(list) = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.stack_models.as_deref())
    {
        stack::push_listed(&mut found, list);
    }
    for model in found {
        let window = if model.one_m { 1_000_000 } else { 0 };
        options.push(SubagentModelOption {
            // 带着 1M 标记写进文件：Claude Code 才按 1M 窗口跑这个子代理（发请求时自己剥掉）。
            value: model.upstream,
            kind: SubagentModelKind::Model,
            label: model.name.unwrap_or_else(|| model.model.clone()),
            resolves_to: None,
            provider_name: Some(provider.name.clone()),
            description: Some(stack::provider_model_description(
                &provider.name,
                &model.model,
                window,
            )),
            warning: None,
        });
    }
}

/// 路由：别名按 `route_roles` 的回落规则落到上游模型。fable 没配时代理按 opus 档映射
/// （`map_model` 的 fable→opus 降级）。
fn route_options(provider: &Provider, options: &mut Vec<SubagentModelOption>) {
    let env = provider_env(provider);
    let official = claude_official::is_official(provider);
    let roles = route_roles(&env);
    let role_of = |tier: Tier| roles.iter().find(|role| role.tier == tier);

    options.push(SubagentModelOption::inherit());
    for tier in ALIAS_ORDER {
        let mut option = SubagentModelOption::alias(tier);
        let role = match tier {
            Tier::Fable => role_of(Tier::Fable).or_else(|| role_of(Tier::Opus)),
            _ => role_of(tier),
        };
        match role {
            Some(role) => describe_tier(&mut option, role, &env, provider),
            // 路由那家一个模型都没配：代理把官方 id 原样转给它。
            None if !official => option.warning = Some(SubagentModelWarning::AliasUnmapped),
            None => {}
        }
        options.push(option);
    }

    // 代理认得 `CLAUDE_CODE_SUBAGENT_MODEL`（`map_model` 把它原样送回上游），它是路由模式下
    // 唯一可靠的完整 id。写客户端认得的形态，和接管写进 live 的一致。
    if let Some(subagent) = env_string(&env, SUBAGENT_MODEL_ENV) {
        options.push(SubagentModelOption {
            value: client_model_for_subagent(subagent),
            kind: SubagentModelKind::Model,
            label: strip_one_m_suffix_for_upstream(subagent).trim().to_string(),
            resolves_to: Some(subagent.to_string()),
            provider_name: Some(provider.name.clone()),
            description: None,
            warning: None,
        });
    }
}

fn describe_tier(
    option: &mut SubagentModelOption,
    role: &TierModel<'_>,
    env: &Map<String, Value>,
    provider: &Provider,
) {
    option.resolves_to = Some(role.upstream.to_string());
    option.provider_name = Some(provider.name.clone());
    option.description = Some(role.description(env, &provider.name));
}

/// 读现在的模式、供应商和 live 配置，算出下拉选项。要读数据库，命令层放到阻塞线程池里调。
pub fn get_subagent_model_options(db: &Database) -> Result<SubagentModelOptions, AppError> {
    let app = AppType::Claude;
    let subagent_override = live_subagent_override();
    let mode = current::mode_state(&app);
    if !mode.is_proxy() {
        let provider = current::direct_provider(db, &app)?;
        return Ok(build_options(
            RoutingInput::Direct(provider.as_ref()),
            subagent_override.as_deref(),
        ));
    }

    let route = match mode.proxy_route.as_deref() {
        Some(id) => db.get_provider_by_id(id, app.as_str())?,
        None => None,
    };
    // 和 `controller::app_mode_view` 同一个判定：代理模式下 Stack 开关开着就是聚合模式。
    let stack_state = state::stack(&DeviceStore::for_device(), app.as_str())?;
    if !stack_state.enabled {
        return Ok(build_options(
            RoutingInput::Route(route.as_ref()),
            subagent_override.as_deref(),
        ));
    }
    let members = stack::published_members(db, &app, &stack_state, mode.proxy_route.as_deref())?;
    Ok(build_options(
        RoutingInput::Stack {
            route: route.as_ref(),
            members: &members,
        },
        subagent_override.as_deref(),
    ))
}

/// Claude Code 实际读的 `settings.json`（含配置目录覆盖）里的 `CLAUDE_CODE_SUBAGENT_MODEL`。
/// 读不到文件或解析失败时当作没设：只影响一条提示，不该让整个下拉失败。
fn live_subagent_override() -> Option<String> {
    let path = crate::config::get_claude_settings_path();
    if !path.exists() {
        return None;
    }
    let live: Value = crate::config::read_json_file(&path)
        .map_err(|error| log::warn!("读取 Claude Code 配置失败，忽略子代理模型覆盖: {error}"))
        .ok()?;
    let env = live.get("env")?.as_object()?;
    env_string(env, SUBAGENT_MODEL_ENV).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(id: &str, name: &str, env: Value) -> Provider {
        Provider::with_id(
            id.to_string(),
            name.to_string(),
            json!({ "env": env }),
            None,
        )
    }

    fn official() -> Provider {
        let mut provider = provider("claude-official", "Claude Official", json!({}));
        provider.category = Some("official".to_string());
        provider
    }

    fn with_stack_models(mut provider: Provider, models: Value) -> Provider {
        provider.meta = Some(crate::provider::ProviderMeta {
            stack_models: serde_json::from_value(models).unwrap(),
            ..Default::default()
        });
        provider
    }

    fn find<'a>(out: &'a SubagentModelOptions, value: &str) -> Option<&'a SubagentModelOption> {
        out.options.iter().find(|option| option.value == value)
    }

    fn resolves<'a>(out: &'a SubagentModelOptions, value: &str) -> Option<&'a str> {
        find(out, value)
            .unwrap_or_else(|| panic!("{value} not listed"))
            .resolves_to
            .as_deref()
    }

    fn values(out: &SubagentModelOptions, kind: SubagentModelKind) -> Vec<&str> {
        out.options
            .iter()
            .filter(|option| option.kind == kind)
            .map(|option| option.value.as_str())
            .collect()
    }

    #[test]
    fn direct_third_party_flags_unmapped_aliases_and_hides_unmapped_fable() {
        let row = provider(
            "kimi",
            "Kimi",
            json!({
                "ANTHROPIC_MODEL": "kimi-k3",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "kimi-k3",
                "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME": "Kimi K3"
            }),
        );
        let out = build_options(RoutingInput::Direct(Some(&row)), None);
        assert_eq!(out.mode, SubagentRoutingMode::Direct);
        assert_eq!(out.provider_id.as_deref(), Some("kimi"));
        assert!(!out.is_official);
        assert!(out.notices.is_empty());

        assert_eq!(resolves(&out, "inherit"), Some("kimi-k3"));
        let sonnet = find(&out, "sonnet").unwrap();
        assert_eq!(sonnet.resolves_to.as_deref(), Some("kimi-k3"));
        assert_eq!(sonnet.warning, None);
        assert_eq!(sonnet.description.as_deref(), Some("Kimi · kimi-k3"));
        // 直连不回落到 ANTHROPIC_MODEL：没配的档会把官方 id 发给第三方。
        for alias in ["opus", "haiku"] {
            let option = find(&out, alias).unwrap();
            assert_eq!(option.resolves_to, None, "{alias}");
            assert_eq!(
                option.warning,
                Some(SubagentModelWarning::AliasUnmapped),
                "{alias}"
            );
        }
        assert!(find(&out, "fable").is_none());
        // 同一个模型只列一次，显示名取档位配的。
        let models: Vec<_> = out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Model)
            .map(|option| (option.value.as_str(), option.label.as_str()))
            .collect();
        assert_eq!(models, vec![("kimi-k3", "Kimi K3")]);

        // 配了 fable 档才列。
        let mut env = row.settings_config["env"].clone();
        env["ANTHROPIC_DEFAULT_FABLE_MODEL"] = json!("kimi-k3-max");
        let row = provider("kimi", "Kimi", env);
        let out = build_options(RoutingInput::Direct(Some(&row)), None);
        assert_eq!(resolves(&out, "fable"), Some("kimi-k3-max"));
    }

    #[test]
    fn direct_official_lists_plain_aliases_and_no_models() {
        let row = official();
        let out = build_options(RoutingInput::Direct(Some(&row)), None);
        assert!(out.is_official);
        assert_eq!(
            values(&out, SubagentModelKind::Alias),
            vec!["sonnet", "opus", "haiku", "fable"]
        );
        assert!(out
            .options
            .iter()
            .all(|option| option.warning.is_none() && option.resolves_to.is_none()));
        assert!(values(&out, SubagentModelKind::Model).is_empty());
        assert_eq!(values(&out, SubagentModelKind::Inherit), vec!["inherit"]);
    }

    #[test]
    fn direct_models_merge_env_and_stack_models_once_keeping_the_1m_marker() {
        let row = with_stack_models(
            provider(
                "zhipu",
                "Zhipu",
                json!({
                    "ANTHROPIC_MODEL": "glm-5.2",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL": "glm-5.2[1M]",
                    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME": "GLM 5.2",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "glm-4.7-air",
                    "ANTHROPIC_SMALL_FAST_MODEL": "glm-4.7-air",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "glm-4.7"
                }),
            ),
            json!([
                { "model": "glm-4.7-air", "displayName": "GLM Air" },
                { "model": "glm-6", "displayName": "GLM 6", "oneM": true }
            ]),
        );
        let out = build_options(RoutingInput::Direct(Some(&row)), None);
        let models: Vec<_> = out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Model)
            .map(|option| {
                (
                    option.value.as_str(),
                    option.label.as_str(),
                    option.description.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            models,
            vec![
                ("glm-5.2[1M]", "GLM 5.2", "Zhipu · glm-5.2 · 1M"),
                ("glm-4.7-air", "GLM Air", "Zhipu · glm-4.7-air"),
                ("glm-4.7", "glm-4.7", "Zhipu · glm-4.7"),
                ("glm-6[1M]", "GLM 6", "Zhipu · glm-6 · 1M"),
            ]
        );
        assert!(out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Model)
            .all(|option| option.provider_name.as_deref() == Some("Zhipu")));
        assert_eq!(resolves(&out, "opus"), Some("glm-5.2[1M]"));
        assert_eq!(
            find(&out, "sonnet").unwrap().warning,
            Some(SubagentModelWarning::AliasUnmapped)
        );
    }

    #[test]
    fn route_aliases_follow_the_route_fallbacks() {
        // haiku：自己的 → SMALL_FAST → MODEL；sonnet/opus：自己的 → MODEL → SMALL_FAST；
        // fable 没配时归入 opus 档。
        let both = provider(
            "relay",
            "Relay",
            json!({
                "ANTHROPIC_MODEL": "main-model",
                "ANTHROPIC_SMALL_FAST_MODEL": "fast-model",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "opus-model"
            }),
        );
        let out = build_options(RoutingInput::Route(Some(&both)), None);
        assert_eq!(out.mode, SubagentRoutingMode::Route);
        assert_eq!(out.notices, vec![SubagentModelNotice::RouteAliasesOnly]);
        assert_eq!(resolves(&out, "haiku"), Some("fast-model"));
        assert_eq!(resolves(&out, "sonnet"), Some("main-model"));
        assert_eq!(resolves(&out, "opus"), Some("opus-model"));
        assert_eq!(resolves(&out, "fable"), Some("opus-model"));
        assert_eq!(resolves(&out, "inherit"), None);
        assert!(values(&out, SubagentModelKind::Model).is_empty());
        assert!(out.options.iter().all(|option| option.warning.is_none()));

        let main_only = provider("m", "M", json!({ "ANTHROPIC_MODEL": "main-model" }));
        let out = build_options(RoutingInput::Route(Some(&main_only)), None);
        for alias in ["haiku", "sonnet", "opus", "fable"] {
            assert_eq!(resolves(&out, alias), Some("main-model"), "{alias}");
        }

        let fast_only = provider(
            "f",
            "F",
            json!({ "ANTHROPIC_SMALL_FAST_MODEL": "fast-model" }),
        );
        let out = build_options(RoutingInput::Route(Some(&fast_only)), None);
        assert_eq!(resolves(&out, "sonnet"), Some("fast-model"));
        assert_eq!(resolves(&out, "haiku"), Some("fast-model"));

        let own_fable = provider(
            "o",
            "O",
            json!({
                "ANTHROPIC_MODEL": "main-model",
                "ANTHROPIC_DEFAULT_FABLE_MODEL": "fable-model[1M]"
            }),
        );
        let out = build_options(RoutingInput::Route(Some(&own_fable)), None);
        let fable = find(&out, "fable").unwrap();
        assert_eq!(fable.resolves_to.as_deref(), Some("fable-model[1M]"));
        assert_eq!(fable.description.as_deref(), Some("O · fable-model · 1M"));

        // 一个模型都没配的第三方：别名原样转给它。
        let bare = provider("b", "B", json!({ "ANTHROPIC_AUTH_TOKEN": "sk" }));
        let out = build_options(RoutingInput::Route(Some(&bare)), None);
        assert!(out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Alias)
            .all(|option| option.warning == Some(SubagentModelWarning::AliasUnmapped)));
    }

    #[test]
    fn route_lists_the_subagent_model_in_its_client_form() {
        let row = provider(
            "relay",
            "Relay",
            json!({
                "ANTHROPIC_MODEL": "glm-5",
                "CLAUDE_CODE_SUBAGENT_MODEL": "claude-opus-5.5[1M]"
            }),
        );
        let out = build_options(RoutingInput::Route(Some(&row)), None);
        let models: Vec<_> = out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Model)
            .collect();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].value, "claude-opus-5-5[1M]");
        assert_eq!(models[0].label, "claude-opus-5.5");
        assert_eq!(
            models[0].resolves_to.as_deref(),
            Some("claude-opus-5.5[1M]")
        );
    }

    fn member(provider: Provider, key: &str, route: bool) -> Member {
        let model_ids = stack::claude_models(key, &provider)
            .into_iter()
            .map(|model| model.id)
            .collect();
        Member {
            provider,
            key: key.to_string(),
            route,
            model_ids,
        }
    }

    #[test]
    fn stack_aliases_share_the_default_and_every_member_publishes() {
        let kimi = provider(
            "kimi",
            "Kimi",
            json!({
                "ANTHROPIC_MODEL": "kimi-k3",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "kimi-k3-turbo"
            }),
        );
        let zhipu = provider(
            "zhipu",
            "Zhipu",
            json!({ "ANTHROPIC_MODEL": "glm-5.2[1M]" }),
        );
        let members = vec![
            member(kimi.clone(), "kimi", true),
            member(zhipu, "zhipu", false),
        ];
        let out = build_options(
            RoutingInput::Stack {
                route: Some(&kimi),
                members: &members,
            },
            None,
        );
        assert_eq!(out.mode, SubagentRoutingMode::Stack);
        assert_eq!(out.provider_id.as_deref(), Some("kimi"));
        assert_eq!(
            out.notices,
            vec![SubagentModelNotice::StackAliasesShareDefault]
        );
        for alias in ["sonnet", "opus", "haiku", "fable"] {
            assert_eq!(resolves(&out, alias), Some("kimi-k3"), "{alias}");
        }
        let models: Vec<_> = out
            .options
            .iter()
            .filter(|option| option.kind == SubagentModelKind::Model)
            .map(|option| {
                (
                    option.value.as_str(),
                    option.label.as_str(),
                    option.provider_name.as_deref().unwrap(),
                    option.resolves_to.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            models,
            vec![
                ("ccs-claude-kimi--kimi-k3", "kimi-k3", "Kimi", "kimi-k3"),
                (
                    "ccs-claude-kimi--kimi-k3-turbo",
                    "kimi-k3-turbo",
                    "Kimi",
                    "kimi-k3-turbo"
                ),
                (
                    "ccs-claude-zhipu--glm-5.2[1M]",
                    "glm-5.2",
                    "Zhipu",
                    "glm-5.2[1M]"
                ),
            ]
        );
        assert_eq!(
            find(&out, "ccs-claude-zhipu--glm-5.2[1M]")
                .unwrap()
                .description
                .as_deref(),
            Some("Zhipu · glm-5.2 · 1M")
        );
    }

    #[test]
    fn stack_without_a_default_model_falls_back_to_route_aliases() {
        // 默认那家没有模型（不在发布的成员里）：契约四档按路由模式写。
        let route = provider("relay", "Relay", json!({ "ANTHROPIC_AUTH_TOKEN": "sk" }));
        let zhipu = provider("zhipu", "Zhipu", json!({ "ANTHROPIC_MODEL": "glm-5.2" }));
        let members = vec![member(zhipu, "zhipu", false)];
        let out = build_options(
            RoutingInput::Stack {
                route: Some(&route),
                members: &members,
            },
            None,
        );
        assert!(out.notices.is_empty());
        assert_eq!(
            find(&out, "sonnet").unwrap().warning,
            Some(SubagentModelWarning::AliasUnmapped)
        );
        assert_eq!(
            values(&out, SubagentModelKind::Model),
            vec!["ccs-claude-zhipu--glm-5.2"]
        );
    }

    #[test]
    fn a_missing_provider_offers_only_inherit_and_aliases() {
        for input in [RoutingInput::Direct(None), RoutingInput::Route(None)] {
            let out = build_options(input, None);
            assert_eq!(
                out.options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect::<Vec<_>>(),
                vec!["inherit", "sonnet", "opus", "haiku", "fable"]
            );
            assert!(out
                .options
                .iter()
                .all(|option| option.warning.is_none() && option.resolves_to.is_none()));
            assert!(out.notices.contains(&SubagentModelNotice::NoProvider));
            assert_eq!(out.provider_id, None);
            assert!(!out.is_official);
        }
        let out = build_options(
            RoutingInput::Stack {
                route: None,
                members: &[],
            },
            None,
        );
        assert_eq!(out.notices, vec![SubagentModelNotice::NoProvider]);
        assert_eq!(out.options.len(), 5);
    }

    #[test]
    fn a_live_subagent_override_is_reported() {
        let row = official();
        let out = build_options(RoutingInput::Direct(Some(&row)), Some(" glm-4.7 "));
        assert_eq!(out.subagent_model_override.as_deref(), Some("glm-4.7"));
        assert_eq!(
            out.notices,
            vec![SubagentModelNotice::SubagentModelOverride]
        );
        let out = build_options(RoutingInput::Direct(Some(&row)), Some("  "));
        assert_eq!(out.subagent_model_override, None);
        assert!(out.notices.is_empty());
    }

    #[test]
    fn serializes_to_the_frontend_contract() {
        let row = provider("kimi", "Kimi", json!({ "ANTHROPIC_MODEL": "kimi-k3" }));
        let value = serde_json::to_value(build_options(
            RoutingInput::Route(Some(&row)),
            Some("kimi-k3"),
        ))
        .unwrap();
        assert_eq!(value["mode"], "route");
        assert_eq!(value["providerId"], "kimi");
        assert_eq!(value["providerName"], "Kimi");
        assert_eq!(value["isOfficial"], false);
        assert_eq!(value["subagentModelOverride"], "kimi-k3");
        assert_eq!(
            value["notices"],
            json!(["route_aliases_only", "subagent_model_override"])
        );
        assert_eq!(
            value["options"][0],
            json!({ "value": "inherit", "kind": "inherit", "label": "inherit" })
        );
        assert_eq!(
            value["options"][1],
            json!({
                "value": "sonnet",
                "kind": "alias",
                "label": "sonnet",
                "resolvesTo": "kimi-k3",
                "providerName": "Kimi",
                "description": "Kimi · kimi-k3"
            })
        );

        let bare = provider("b", "B", json!({}));
        let value =
            serde_json::to_value(build_options(RoutingInput::Direct(Some(&bare)), None)).unwrap();
        assert_eq!(value["options"][1]["warning"], "alias_unmapped");
        assert_eq!(value["options"][1]["kind"], "alias");
        assert!(value.get("subagentModelOverride").is_none());
        assert_eq!(value["notices"], json!([]));
    }
}
