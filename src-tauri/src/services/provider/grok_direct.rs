//! 写 Grok Build 的 `config.toml`：只替换 `models.default` 和 CC Switch 写的那张模型表，
//! 其余字节不碰。
//!
//! 写 Grok live 的入口（切换、新增第一个供应商、编辑当前供应商、同步、统一供应商、
//! 进入 / 退出代理）都走这里：先拿应用写锁，再经 `mode::operation` 记下 pending，文件、
//! 指针和写入记录在同一个操作里提交。不回填：用户的设置和 MCP 本来就留在 live 里。

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::grok_auth::{GrokAuthPatch, GrokCliLogin, GrokLoginStash, LiveLogin, LoginWrite};
use crate::grok_config::get_grok_config_path;

use crate::live::engine::{read_current, DeviceStore, LiveFile};
use crate::live::patch::toml::{TomlDocPatch, TomlSteps};
use crate::live::patch::WholeFile;
use crate::live::project::claude::PROXY_TOKEN_PLACEHOLDER;
use crate::live::project::grok::{GrokConfigPatch, GrokProjection};
use crate::mode::operation::{AppWrite, FileChange, OperationReport};
use crate::mode::state::{self, op, PendingTarget, Written};
use crate::provider::Provider;
use crate::store::AppState;
use serde_json::{Map, Value};

use super::editor_toml::TomlEdits;

fn app() -> &'static str {
    AppType::GrokBuild.as_str()
}

/// `~/.grok/config.toml`：模型表里有 Key，按 0600 写。
pub(crate) fn config_file() -> LiveFile {
    LiveFile::private(get_grok_config_path())
}

/// 官方卡：不写模型表，Grok 回落到内置模型和自带的 xAI 登录。
pub(crate) fn is_official(provider: &Provider) -> bool {
    provider.has_official_category()
}

/// 官方卡绑定的授权中心 xAI 账号：切到这张卡时 Grok CLI 的登录换成这个号。
pub(crate) fn bound_account(provider: &Provider) -> Option<String> {
    if !is_official(provider) {
        return None;
    }
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("xai_oauth"))
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// 切换时 Grok CLI 登录（`auth.json`）和登录暂存要做的改动，在拿写锁之前备好。
pub(crate) struct LoginChange {
    pub auth: Option<GrokAuthPatch>,
    /// 登录暂存：第一次覆盖 Grok CLI 自己的登录时写入，还原后删除
    pub stash: Option<WholeFile>,
}

/// 备好切到 `target` 时 Grok CLI 登录的改动（可能联网刷新令牌）。
///
/// - 绑了账号的官方卡：Grok CLI 登录的已是这个号就不动；否则先把 CLI 现在的登录收进授权
///   中心（可能轮换过 refresh token，或是没在授权中心的号），第一次覆盖 CLI 自己的登录时记下
///   原来是谁，再写入目标账号。出错就不切换，免得丢掉登录。
/// - 没绑账号的官方卡（跟随 Grok CLI 登录）：有暂存就还原原来的登录并删掉暂存；还原不了
///   只记日志，登录保持现状。
/// - 第三方卡：不动登录。
pub(crate) fn prepare_login(
    manager: &XaiManager,
    target: &Provider,
) -> Result<Option<LoginChange>, AppError> {
    if !is_official(target) {
        return Ok(None);
    }
    match bound_account(target) {
        Some(account_id) => prepare_bound(manager, account_id),
        None => Ok(prepare_restore(manager).unwrap_or_else(|error| {
            log::warn!("还原 Grok CLI 原来的登录失败，登录保持现状: {error}");
            None
        })),
    }
}

pub(crate) type XaiManager =
    std::sync::Arc<tokio::sync::RwLock<crate::proxy::providers::xai_oauth_auth::XaiOAuthManager>>;

/// 联网准备好的登录，和发布时 `auth.json` 应当还是的样子
struct PreparedLogin {
    login: Option<GrokCliLogin>,
    /// 发布前的比对基准：联网前读到的 live，只换上收进授权中心时自己写回去的那条。
    /// 不在联网后重读盘面：那期间 Grok CLI 轮换的令牌、用户新登录的号都不是我们写的，
    /// 拿它们当基准就会在发布时把它们覆盖掉。
    expected: Option<LiveLogin>,
}

/// 联网前读到的 `live`，换上收进授权中心时自己写回 `auth.json` 的那条（没写回就原样）
fn expected_after_absorb(
    live: Option<LiveLogin>,
    absorbed_write_back: Option<LiveLogin>,
) -> Option<LiveLogin> {
    absorbed_write_back.or(live)
}

/// 先把 Grok CLI 现在的登录（`live`）收进授权中心（CLI 可能轮换过 refresh token，或是没在
/// 授权中心的号；收不进来就不切换，免得丢掉它），再取 `account_id` 的登录。在独立线程里跑：
/// 调用方可能已在 tokio 运行时里。
fn absorb_then_login(
    manager: &XaiManager,
    live: Option<LiveLogin>,
    account_id: Option<String>,
) -> Result<PreparedLogin, AppError> {
    let manager = manager.clone();
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            let manager = manager.read().await;
            let mut absorbed_write_back = None;
            if let Some(live) = &live {
                absorbed_write_back = manager.absorb_grok_cli_login(live).await.map_err(|e| {
                    AppError::localized(
                        "grokbuild.login.absorb_failed",
                        format!("无法确认 Grok CLI 现在的登录，为避免丢掉它，本次没有切换：{e}"),
                        format!(
                            "Could not verify Grok CLI's current login, so nothing was switched to avoid losing it: {e}"
                        ),
                    )
                })?;
            }
            let expected = expected_after_absorb(live, absorbed_write_back);
            let Some(account_id) = account_id else {
                return Ok(PreparedLogin {
                    login: None,
                    expected,
                });
            };
            let login = manager.grok_cli_login(&account_id).await.map_err(|e| {
                // 刷新令牌时网络 / 端点暂时失败不代表登录失效，别让用户白白去重新登录
                if e.is_transient() {
                    AppError::localized(
                        "grokbuild.login.refresh_unavailable",
                        format!("暂时无法刷新授权中心 xAI 账号的令牌，本次没有切换，请稍后重试：{e}"),
                        format!(
                            "Could not refresh the auth center's xAI account token right now, so nothing was switched; try again later: {e}"
                        ),
                    )
                } else {
                    AppError::localized(
                        "grokbuild.login.account_unavailable",
                        format!("授权中心的 xAI 账号不可用，请在授权中心重新登录：{e}"),
                        format!(
                            "The xAI account in the auth center is unavailable; sign in again there: {e}"
                        ),
                    )
                }
            })?;
            Ok(PreparedLogin {
                login: Some(login),
                expected,
            })
        })
    })
    .join()
    .map_err(|_| AppError::Message("Grok 登录准备线程异常退出".to_string()))?
}

/// live 里 OIDC 那一条现在的样子
struct LiveAuth {
    /// 整条（暂存时用）
    entry: Option<Map<String, Value>>,
    /// 其中的登录
    login: Option<LiveLogin>,
    /// 原来有没有 `auth.json`
    file_existed: bool,
}

fn read_live() -> Result<LiveAuth, AppError> {
    let path = crate::grok_auth::auth_path();
    let pre = read_current(&path)?;
    let entry = crate::grok_auth::read_live_entry(&path, pre.as_deref())?;
    Ok(LiveAuth {
        login: entry.as_ref().and_then(crate::grok_auth::live_login_of),
        entry,
        file_existed: pre.is_some(),
    })
}

fn prepare_bound(
    manager: &XaiManager,
    account_id: String,
) -> Result<Option<LoginChange>, AppError> {
    let LiveAuth {
        entry,
        login: live,
        file_existed,
    } = read_live()?;
    if live.as_ref().is_some_and(|live| live.user_id == account_id) {
        return Ok(None);
    }
    // 还没有暂存：live 里是 Grok CLI 自己的登录，记下来以便切回时还原
    let stash = match crate::grok_auth::read_stash().map_err(AppError::Message)? {
        Some(_) => None,
        None => Some(WholeFile::Write(
            GrokLoginStash::capture(entry, file_existed).to_bytes(),
        )),
    };
    let PreparedLogin { login, expected } = absorb_then_login(manager, live, Some(account_id))?;
    let Some(login) = login else {
        return Ok(None);
    };
    let write = LoginWrite::Set {
        login: Box::new(login),
        base: None,
    };
    Ok(Some(LoginChange {
        auth: Some(GrokAuthPatch { write, expected }),
        stash,
    }))
}

fn prepare_restore(manager: &XaiManager) -> Result<Option<LoginChange>, AppError> {
    let Some(stash) = crate::grok_auth::read_stash().map_err(AppError::Message)? else {
        return Ok(None);
    };
    let live = read_live()?.login;
    let auth = match (&stash.user_id, &live) {
        // 已经是原来的号（比如用户自己又 `grok login` 了）：只删暂存
        (Some(user_id), Some(live)) if live.user_id == *user_id => None,
        // 换回原来的号；原来没登录就删掉这一条。要换掉的是托管账号的登录，CLI 可能轮换过
        // 令牌，先收进授权中心。
        _ => {
            let PreparedLogin { login, expected } =
                absorb_then_login(manager, live, stash.user_id.clone())?;
            Some(GrokAuthPatch {
                write: stash.restore_write(login),
                expected,
            })
        }
    };
    Ok(Some(LoginChange {
        auth,
        stash: Some(WholeFile::Delete),
    }))
}

/// 供应商要写的模型表。写之前校验，校验不过什么都不写。
pub(crate) fn projection(provider: &Provider) -> Result<GrokProjection, AppError> {
    GrokProjection::of(&provider.settings_config, is_official(provider))
}

/// 上次写进 live 的表。有写入记录就按记录；这台设备上新版还没写过（刚从旧版升级）时，
/// 按旧版的写法推断：旧版把 `live_owner` 的行整份写进 live，它的 `models.default` 指的就是
/// CC Switch 的表。用户的其他表旧版也会原样写回，但证明不了是 CC Switch 的，不删。
pub(crate) fn retired_tables(
    store: &DeviceStore,
    live_owner: Option<&Provider>,
) -> Result<Vec<String>, AppError> {
    if let Some(written) = state::written(store, app())? {
        return Ok(written.tables);
    }
    let Some(owner) = live_owner.filter(|owner| !is_official(owner)) else {
        return Ok(Vec::new());
    };
    if let Ok(projection) = projection(owner) {
        return Ok(projection.written_tables());
    }
    // 行本身过不了校验（比如旧版回填把默认模型改成了内置的），就按行里写的名字删。
    Ok(owner
        .settings_config
        .get("config")
        .and_then(|config| config.as_str())
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|doc| {
            doc.get("models")
                .and_then(|models| models.get("default"))
                .and_then(toml_edit::Item::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
        })
        .into_iter()
        .collect())
}

/// 切到 `target`：同一个操作里写 live、改指针、记下写的表。`live_owner` 是 live 现在
/// 对应的供应商（直连指针那家），只在没有写入记录时用来推断旧版写的表。
pub(crate) fn switch_to(
    state: &AppState,
    live_owner: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    write(state, op::SWITCH, live_owner, target, Some(&target.id))
}

/// 把当前供应商重新投影到 live，不改指针。
pub(crate) fn reapply(
    state: &AppState,
    live_owner: Option<&Provider>,
    target: &Provider,
) -> Result<OperationReport, AppError> {
    write(state, op::APPLY, live_owner, target, None)
}

fn write(
    state: &AppState,
    op: &str,
    live_owner: Option<&Provider>,
    target: &Provider,
    pointer: Option<&str>,
) -> Result<OperationReport, AppError> {
    let projection = projection(target)?;
    let login = prepare_login(&state.xai_oauth_manager, target)?;
    run(
        &state.db,
        op,
        live_owner,
        Some(&projection),
        PendingTarget::pointer(pointer.map(str::to_string)),
        None,
        login.as_ref(),
    )
}

/// 把 `projection` 写进 `config.toml`，和 `target` 在同一个操作里提交，并记下写的表；
/// `projection` 为空时只落定状态、不读也不写文件。`edits` 是编辑器保存的全局改动（先改动，
/// 后模型表），`login` 是 Grok CLI 登录和登录暂存的改动，都在同一次写入里提交。
pub(crate) fn run(
    db: &Database,
    op: &str,
    live_owner: Option<&Provider>,
    projection: Option<&GrokProjection>,
    mut target: PendingTarget,
    edits: Option<&TomlEdits>,
    login: Option<&LoginChange>,
) -> Result<OperationReport, AppError> {
    // 先补完上一次没做完的操作：它可能改了写入记录和指针，下面要按最新的记录删表。
    let app_write = AppWrite::begin(db, app())?;
    let patch = match projection {
        Some(projection) => {
            target.written = Some(Written {
                tables: projection.written_tables(),
                ..Written::default()
            });
            Some(GrokConfigPatch::direct(
                projection,
                retired_tables(&app_write.store, live_owner)?,
                PROXY_TOKEN_PLACEHOLDER,
            ))
        }
        None => None,
    };
    let edits = edits.filter(|edits| !edits.is_empty());
    // 先按三方比较应用编辑器的改动，再换模型表。
    let mut steps: Vec<&dyn TomlDocPatch> = Vec::new();
    if let Some(edits) = edits {
        steps.push(edits);
    }
    if let Some(patch) = &patch {
        steps.push(patch);
    }
    let write = TomlSteps(steps);
    let mut changes: Vec<FileChange<'_>> = (!write.0.is_empty())
        .then(|| FileChange {
            file: config_file(),
            patch: &write,
        })
        .into_iter()
        .collect();
    if let Some(auth) = login.and_then(|login| login.auth.as_ref()) {
        changes.push(FileChange {
            file: LiveFile::private(crate::grok_auth::auth_path()),
            patch: auth,
        });
    }
    if let Some(stash) = login.and_then(|login| login.stash.as_ref()) {
        changes.push(FileChange {
            file: LiveFile::private(app_write.store.file(crate::grok_auth::STASH_FILENAME)),
            patch: stash,
        });
    }
    app_write.run(op, &changes, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::patch::{LivePatch, LiveWriteError};
    use crate::proxy::providers::xai_oauth_auth::XaiOAuthManager;
    use crate::test_support::TestHome;
    use serial_test::serial;
    use std::io::{Read, Write};

    fn auth_json(user: &str, refresh: &str) -> String {
        serde_json::json!({
            (crate::grok_auth::oidc_scope()): {"user_id": user, "refresh_token": refresh}
        })
        .to_string()
    }

    fn write_auth(user: &str, refresh: &str) {
        let path = crate::grok_auth::auth_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, auth_json(user, refresh)).unwrap();
    }

    fn disk_auth() -> Vec<u8> {
        std::fs::read(crate::grok_auth::auth_path()).unwrap()
    }

    /// 本机令牌端点：依次应答 `responses`。每条应答前先跑它的 `before`（模拟联网期间
    /// Grok CLI 改了 `auth.json`）。
    type BeforeReply = Box<dyn FnOnce() + Send>;
    fn serve_token_responses(responses: Vec<(Option<BeforeReply>, serde_json::Value)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (before, body) in responses {
                let Ok((mut socket, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 8192];
                let _ = socket.read(&mut buf);
                if let Some(before) = before {
                    before();
                }
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}/oauth2/token")
    }

    fn tokens(access: &str, refresh: &str) -> serde_json::Value {
        serde_json::json!({"access_token": access, "refresh_token": refresh, "expires_in": 3600})
    }

    /// 授权中心里有一个绑定用的号 `bound`，令牌端点按 `responses` 应答：
    /// 第一次是收 CLI 登录（`cli-user`）时的刷新，第二次是取 `bound` 的登录时的刷新。
    fn manager_with(
        home: &TestHome,
        responses: Vec<(Option<BeforeReply>, serde_json::Value)>,
    ) -> XaiManager {
        let endpoint = serve_token_responses(responses);
        let manager = XaiOAuthManager::new(home.path().join("xai-data"));
        let manager = std::sync::Arc::new(tokio::sync::RwLock::new(manager));
        let seeded = manager.clone();
        std::thread::spawn(move || {
            tauri::async_runtime::block_on(async move {
                seeded
                    .read()
                    .await
                    .seed_test_account_with_token_endpoint("bound", "rt-bound", endpoint)
                    .await;
            })
        })
        .join()
        .unwrap();
        manager
    }

    fn conflict(result: Result<Vec<u8>, LiveWriteError>) -> bool {
        matches!(result, Err(LiveWriteError::Conflict { .. }))
    }

    /// 收进授权中心联网期间 Grok CLI 改了 `auth.json`（轮换了令牌 / 用户换号登录）：基准是
    /// 联网前读到的 live，发布时比对不上，拒绝写入，不把新登录覆盖掉。
    #[test]
    #[serial]
    fn concurrent_change_during_absorb_fails_the_guard() {
        for (user, refresh) in [
            ("cli-user", "rt-rotated-by-cli"),
            ("someone-else", "rt-other"),
        ] {
            let home = TestHome::new();
            write_auth("cli-user", "rt-1");
            let manager = manager_with(
                &home,
                vec![
                    (
                        Some(Box::new(move || write_auth(user, refresh))),
                        tokens("at-cli", "rt-absorbed"),
                    ),
                    (None, tokens("at-bound", "rt-bound-2")),
                ],
            );

            let LiveAuth { login: live, .. } = read_live().unwrap();
            let PreparedLogin { login, expected } =
                absorb_then_login(&manager, live, Some("bound".to_string())).unwrap();
            let patch = GrokAuthPatch {
                write: LoginWrite::Set {
                    login: Box::new(login.unwrap()),
                    base: None,
                },
                expected,
            };

            let path = crate::grok_auth::auth_path();
            let disk = disk_auth();
            assert_eq!(disk, auth_json(user, refresh).into_bytes());
            assert!(conflict(patch.apply(&path, Some(&disk))), "{user}");
        }
    }

    /// 收进授权中心时自己把轮换出的令牌写回了 `auth.json`：基准换成自己写的那条，照常写入。
    #[test]
    #[serial]
    fn own_write_back_during_absorb_becomes_the_baseline() {
        let home = TestHome::new();
        write_auth("cli-user", "rt-1");
        let manager = manager_with(
            &home,
            vec![
                (None, tokens("at-cli", "rt-absorbed")),
                (None, tokens("at-bound", "rt-bound-2")),
            ],
        );

        let LiveAuth { login: live, .. } = read_live().unwrap();
        let PreparedLogin { login, expected } =
            absorb_then_login(&manager, live, Some("bound".to_string())).unwrap();
        assert_eq!(
            expected.as_ref().and_then(|l| l.refresh_token.as_deref()),
            Some("rt-absorbed")
        );
        let patch = GrokAuthPatch {
            write: LoginWrite::Set {
                login: Box::new(login.unwrap()),
                base: None,
            },
            expected,
        };

        let path = crate::grok_auth::auth_path();
        let written = patch.apply(&path, Some(&disk_auth())).unwrap();
        let after = crate::grok_auth::read_live_login(&path, Some(&written))
            .unwrap()
            .unwrap();
        assert_eq!(after.user_id, "bound");
        assert_eq!(after.refresh_token.as_deref(), Some("rt-bound-2"));
    }
}
