//! 写 Hermes 的 `config.yaml`：只动 CC Switch 自己的路由条目和顶层 `model:` 段，其余
//! 字节不碰（投影与补丁见 `live::project::hermes`）。
//!
//! 写 Hermes live 的入口（进入 / 退出代理、换路由、同步、退出 CC Switch 分离）都走这里：
//! 和 `hermes_config` 的其它写路径共用 `hermes_write_lock`，再经 `mode::operation` 记下
//! pending，文件、模式和写入记录在同一个操作里提交，崩溃后可前滚或丢弃。
//!
//! Hermes 的「多个 agent」是命名 profile（`<home>/profiles/<name>/config.yaml`，自包含、
//! 不从默认 home 继承）。统一路由模式把同一条路由条目写进默认 home 和全部 profile home：
//! 每个 home 的 `model.default` 在路由那家提供的模型里就保留；退出时默认 home 按直连
//! 指针恢复，profile home 按进入时记录的原值恢复（记录在 `written.extra["homes"]`）。

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::live::engine::{DeviceStore, LiveFile};
use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;
use crate::live::project::hermes::{HermesConfigPatch, HermesProjection, ModelTarget};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{self, PendingTarget, Written};
use crate::provider::Provider;

fn app() -> &'static str {
    AppType::Hermes.as_str()
}

/// 一个要写的 Hermes home。`name` 为空串是默认 home（`~/.hermes/config.yaml`），
/// 其余是 `profiles/<name>/config.yaml`。
#[derive(Debug, Clone)]
pub(crate) struct HermesHome {
    pub name: String,
    pub file: LiveFile,
}

/// 默认 home（`~/.hermes/config.yaml`，供应商条目里带 Key，按 0600 写）。
pub(crate) fn default_home() -> HermesHome {
    HermesHome {
        name: String::new(),
        file: LiveFile::private(crate::hermes_config::get_hermes_config_path()),
    }
}

/// 全部候选 home：默认 home + 全部命名 profile。只收目录名不带点、且有 `config.yaml`
/// 的 profile（见 [`crate::hermes_config::list_profile_config_paths`]）。
pub(crate) fn homes() -> Vec<HermesHome> {
    let mut homes = vec![default_home()];
    homes.extend(
        crate::hermes_config::list_profile_config_paths()
            .into_iter()
            .map(|(name, path)| HermesHome {
                name,
                file: LiveFile::private(path),
            }),
    );
    homes
}

/// 契约摘要里的 home 名单（与实际写入的名单一致：解析不了的 home 不写也不进契约，
/// 否则 unchanged 判定会说谎）。
pub(crate) fn home_names() -> Vec<String> {
    writable_homes().into_iter().map(|home| home.name).collect()
}

/// 能安全改写的 home：文件存在且解析得动。解析失败的不碰（补丁对坏文件只会报错），
/// 警告后跳过——一个损坏的 profile 不该挡住其它所有 agent 进代理。
fn writable_homes() -> Vec<HermesHome> {
    homes()
        .into_iter()
        .filter(|home| match read_model_state(&home.file.path) {
            Ok(_) => true,
            Err(error) => {
                log::warn!(
                    "Hermes home '{}' 的 config.yaml 读不了（{error}），本次跳过",
                    home.name
                );
                false
            }
        })
        .collect()
}

/// 读一个 home 的 `model:` 段现状（provider / default），文件缺失或没有 model 段时
/// 各字段为 `None`。
fn read_model_state(path: &Path) -> Result<(Option<String>, Option<String>), AppError> {
    let raw = std::fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&raw).map_err(|e| AppError::Config(format!("YAML 解析失败: {e}")))?;
    let text = |key: &str| {
        yaml.get("model")
            .and_then(|model| model.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    Ok((text("provider"), text("default")))
}

/// 代理契约：`{proxy_base}/hermes/v1` 上的 Responses 路由条目 + `model:` 段指向它。
pub(crate) fn proxy_contract(
    route: &Provider,
    proxy_base_url: &str,
) -> Result<HermesProjection, AppError> {
    HermesProjection::proxy_contract(
        &route.settings_config,
        proxy_base_url,
        PROXY_TOKEN_PLACEHOLDER,
    )
}

/// 默认 home 的直连投影（`direct` 为 `None` 时清掉 `model.provider`）。
fn direct_projection(direct: Option<&Provider>) -> HermesProjection {
    HermesProjection::direct(
        direct.map(|provider| (provider.id.as_str(), &provider.settings_config)),
    )
}

/// 把 `projection` 写进全部 home，和目标状态在同一个操作里提交；`None` 时只落定状态、
/// 不读也不写文件（契约没变 / 未接上）。
pub(crate) fn run_enter(
    db: &Database,
    op: &str,
    projection: Option<&HermesProjection>,
    mut target: PendingTarget,
) -> Result<OperationReport, AppError> {
    // 与 hermes_config 的其它写路径（set_provider 等）互斥，避免两条路径交错的半截写。
    let _hermes_guard = crate::hermes_config::hermes_write_lock()
        .lock()
        .map_err(|e| AppError::Lock(e.to_string()))?;

    let app_write = AppWrite::begin(db, app())?;
    let Some(projection) = projection else {
        return app_write.run(op, &[], target);
    };

    let homes = writable_homes();
    // 进入代理时逐 home 记下 model: 段原值，退出时按它恢复。代理内换路由会再次
    // 走到这里：已记录的 home 保留第一次进入时的原值（此时 live 已是代理契约，
    // 重采会把代理期的值当原值），只补记上次之后新增的 home。
    let recorded = recorded_home_states(&app_write.store)?;
    let mut restore = Map::new();
    for home in &homes {
        let state = match recorded.get(&home.name) {
            Some(state) => state.clone(),
            None => {
                let (provider, default) = read_model_state(&home.file.path)?;
                json!({ "provider": provider, "default": default })
            }
        };
        restore.insert(home.name.clone(), state);
    }
    target.written = Some(Written {
        tables: vec![crate::live::project::hermes::ROUTE_ENTRY_NAME.to_string()],
        extra: {
            let mut extra = Map::new();
            extra.insert("homes".to_string(), Value::Object(restore));
            extra
        },
    });

    let patch = HermesConfigPatch::new(projection);
    let changes: Vec<FileChange<'_>> = homes
        .iter()
        .map(|home| FileChange {
            file: home.file.clone(),
            patch: &patch,
        })
        .collect();
    app_write.run(op, &changes, target)
}

/// 退出代理：默认 home 按直连指针恢复，profile home 按进入时记录的原值恢复（没有
/// 记录的 home 只删条目、`model:` 段不动）；`attached` 为假时只落定状态、不写文件。
pub(crate) fn run_exit(
    db: &Database,
    op: &str,
    attached: bool,
    direct: Option<&Provider>,
    mut target: PendingTarget,
) -> Result<OperationReport, AppError> {
    let _hermes_guard = crate::hermes_config::hermes_write_lock()
        .lock()
        .map_err(|e| AppError::Lock(e.to_string()))?;

    let app_write = AppWrite::begin(db, app())?;
    if !attached {
        return app_write.run(op, &[], target);
    }

    let homes = writable_homes();
    let recorded = recorded_home_states(&app_write.store)?;
    let projections: Vec<HermesProjection> = homes
        .iter()
        .map(|home| {
            if home.name.is_empty() {
                return direct_projection(direct);
            }
            match recorded.get(&home.name) {
                Some(state) => HermesProjection {
                    entry: None,
                    model: ModelTarget::Restore {
                        provider: state
                            .get("provider")
                            .and_then(Value::as_str)
                            .map(String::from),
                        default: state
                            .get("default")
                            .and_then(Value::as_str)
                            .map(String::from),
                    },
                },
                None => HermesProjection {
                    entry: None,
                    model: ModelTarget::Untouched,
                },
            }
        })
        .collect();
    let patches: Vec<HermesConfigPatch<'_>> =
        projections.iter().map(HermesConfigPatch::new).collect();
    let changes: Vec<FileChange<'_>> = homes
        .iter()
        .zip(&patches)
        .map(|(home, patch)| FileChange {
            file: home.file.clone(),
            patch,
        })
        .collect();
    // 没有 CC Switch 写的条目了，清掉写入记录。
    target.written = Some(Written::default());
    app_write.run(op, &changes, target)
}

/// 进入代理时记录的各 home `model:` 段原值（`written.extra["homes"]`）。
fn recorded_home_states(store: &DeviceStore) -> Result<BTreeMap<String, Value>, AppError> {
    let recorded = state::written(store, app())?
        .and_then(|written| written.extra.get("homes").cloned())
        .unwrap_or_else(|| json!({}));
    Ok(serde_json::from_value(recorded).unwrap_or_default())
}
