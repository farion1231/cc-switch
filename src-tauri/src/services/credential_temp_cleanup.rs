//! 启动时清理凭据文件旁边残留的临时文件。
//!
//! 凭据经 [`crate::config::atomic_write_private_durable`] 写入：先写
//! `<文件名>.tmp.<pid>.<纳秒>.<计数>`，再替换目标。进程恰好在两步之间被杀或断电时，临时文件
//! 留在原地，里面是完整的凭据，之后没有任何地方会再碰它。
//!
//! 范围刻意收窄：只看凭据文件所在的目录，只认 CC Switch 自己起的临时文件名，只删这些凭据
//! 文件名的、超过 [`STALE_AFTER`] 的临时文件。`~/.codex/auth.json` 也由写入引擎经同样命名的
//! 临时文件发布，引擎崩溃恢复要靠 `live-state.json` 里记着的那份前滚：这些一律保留，读不出
//! 那份清单时这个目录整个不清。

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::remove_stale_temp_files;
use crate::live::engine::DeviceStore;
use crate::services::claude_account::layout;
use crate::services::claude_account::snapshot::store::Slot;

/// 临时文件超过这个时长还在，就当作残留删掉。
pub const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// CC Switch 数据目录里的凭据文件：OAuth 账号存储、agy 账号和登录暂存、Claude 切号日志。
const APP_DIR_FILES: [&str; 6] = [
    crate::proxy::providers::codex_oauth_auth::STORAGE_FILENAME,
    crate::proxy::providers::copilot_auth::STORAGE_FILENAME,
    crate::proxy::providers::xai_oauth_auth::STORAGE_FILENAME,
    crate::services::agy_account::STORE_FILENAME,
    crate::services::agy_account::STASH_FILENAME,
    crate::services::claude_account::snapshot::STATE_FILENAME,
];

/// 要清理的位置。单独成结构是为了测试时换成临时目录。
struct Roots {
    /// CC Switch 数据目录（`~/.cc-switch`，可被覆盖）
    app_dir: PathBuf,
    /// 用户主目录：Claude Code 默认槽位在 `~/.claude`
    home: PathBuf,
    /// Codex 的 `auth.json`
    codex_auth: PathBuf,
    /// Kimi Code 的数据目录
    kimi_dir: PathBuf,
    /// 写入引擎的状态（`live-state.json`）
    device: DeviceStore,
}

/// 启动时调用一次。做的是阻塞的磁盘 IO，调用方放到后台线程里跑。
pub fn cleanup_on_startup() {
    let roots = Roots {
        app_dir: crate::config::get_app_config_dir(),
        home: crate::config::get_home_dir(),
        codex_auth: crate::codex_config::get_codex_auth_path(),
        kimi_dir: crate::kimi_config::data_dir(),
        device: DeviceStore::for_device(),
    };
    let removed = cleanup(&roots, STALE_AFTER);
    if removed > 0 {
        log::info!("启动清理：删除了 {removed} 个残留的凭据临时文件");
    }
}

fn cleanup(roots: &Roots, max_age: Duration) -> usize {
    let none = HashSet::new();
    let mut removed = 0;

    // CC Switch 数据目录：这些文件只有 CC Switch 写，旧版 OAuth 存储的临时文件名也认。
    removed += remove_stale_temp_files(
        &roots.app_dir,
        true,
        |base| APP_DIR_FILES.contains(&base),
        &none,
        max_age,
    );

    // Claude Code：默认槽位和每个账号槽位的 `.credentials.json`。
    for slot in claude_slots(roots) {
        removed += remove_for_file(&slot.credentials_file(), &none, max_age);
    }

    // Kimi Code：`credentials/<名字>.json`。
    removed += remove_stale_temp_files(
        &roots.kimi_dir.join("credentials"),
        false,
        |base| base.ends_with(".json"),
        &none,
        max_age,
    );

    // Codex 的 `auth.json`：未完成操作还要用的临时文件保留。
    match crate::mode::state::staged_temp_files(&roots.device) {
        Ok(staged) => {
            let keep = staged
                .iter()
                .filter_map(|path| path.file_name().map(OsString::from))
                .collect();
            removed += remove_for_file(&roots.codex_auth, &keep, max_age);
        }
        Err(error) => {
            log::warn!(
                "读不出写入引擎的未完成操作（{error}），跳过 Codex auth.json 旁的临时文件清理"
            );
        }
    }

    removed
}

/// 删掉 `path` 同目录下属于它的残留临时文件。
fn remove_for_file(path: &Path, keep: &HashSet<OsString>, max_age: Duration) -> usize {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return 0;
    };
    remove_stale_temp_files(dir, false, |base| base == name, keep, max_age)
}

/// 默认槽位加上 `claude-accounts/` 下每个账号的槽位。
fn claude_slots(roots: &Roots) -> Vec<Slot> {
    let mut slots = vec![Slot::Default {
        home: roots.home.clone(),
    }];
    if let Ok(entries) = std::fs::read_dir(layout::accounts_root(&roots.app_dir)) {
        for entry in entries.flatten() {
            let Some(local_id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if layout::valid_local_id(&local_id) {
                slots.push(Slot::Account(layout::config_dir(&roots.app_dir, &local_id)));
            }
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    const HOUR: Duration = Duration::from_secs(60 * 60);

    struct Fixture {
        _dir: tempfile::TempDir,
        roots: Roots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let roots = Roots {
            app_dir: root.join("cc-switch"),
            home: root.join("home"),
            codex_auth: root.join("home").join(".codex").join("auth.json"),
            kimi_dir: root.join("kimi"),
            device: DeviceStore::at(root.join("device")),
        };
        fs::create_dir_all(&roots.app_dir).unwrap();
        fs::create_dir_all(roots.codex_auth.parent().unwrap()).unwrap();
        fs::create_dir_all(roots.home.join(".claude")).unwrap();
        fs::create_dir_all(roots.kimi_dir.join("credentials")).unwrap();
        fs::create_dir_all(root.join("device")).unwrap();
        Fixture { _dir: dir, roots }
    }

    /// 建一个文件，修改时间设成 `age` 之前。
    fn touch(path: &Path, age: Duration) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"secret").unwrap();
        let file = fs::File::options().write(true).open(path).unwrap();
        file.set_modified(SystemTime::now() - age).unwrap();
        path.to_path_buf()
    }

    #[test]
    fn removes_stale_credential_temp_files_in_app_dir() {
        let fx = fixture();
        let app = &fx.roots.app_dir;
        let stale = touch(&app.join("codex_oauth_auth.json.tmp.12.345.0"), 2 * HOUR);
        let legacy = touch(
            &app.join("copilot_auth.json.tmp.1700000000000000000"),
            2 * HOUR,
        );
        let journal = touch(
            &app.join("claude-account-snapshot.json.tmp.7.8.9"),
            2 * HOUR,
        );
        let fresh = touch(
            &app.join("xai_oauth_auth.json.tmp.12.346.0"),
            Duration::ZERO,
        );
        let unrelated = touch(&app.join("settings.json.tmp.12.347.0"), 2 * HOUR);
        let not_temp = touch(&app.join("agy_accounts.json.bak"), 2 * HOUR);
        let store = touch(&app.join("agy_accounts.json"), 2 * HOUR);

        assert_eq!(cleanup(&fx.roots, HOUR), 3);

        assert!(!stale.exists());
        assert!(!legacy.exists());
        assert!(!journal.exists());
        assert!(fresh.exists(), "a write in progress must not be touched");
        assert!(unrelated.exists(), "only credential files are cleaned");
        assert!(not_temp.exists());
        assert!(store.exists(), "the credential file itself stays");
    }

    #[test]
    fn removes_stale_temp_files_of_claude_and_kimi_credentials() {
        let fx = fixture();
        let local_id = "0123456789abcdef0123456789abcdef";
        let account_dir = layout::config_dir(&fx.roots.app_dir, local_id);
        let default_slot = touch(
            &fx.roots
                .home
                .join(".claude")
                .join(".credentials.json.tmp.1.2.3"),
            2 * HOUR,
        );
        let account_slot = touch(&account_dir.join(".credentials.json.tmp.1.2.3"), 2 * HOUR);
        let kimi = touch(
            &fx.roots
                .kimi_dir
                .join("credentials")
                .join("kimi-code.json.tmp.1.2.3"),
            2 * HOUR,
        );
        // Claude Code 自己的临时文件命名不同：不认，不删。
        let claude_code_own = touch(
            &fx.roots
                .home
                .join(".claude")
                .join(".credentials.json.tmp.1.1700000000000"),
            2 * HOUR,
        );

        assert_eq!(cleanup(&fx.roots, HOUR), 3);

        assert!(!default_slot.exists());
        assert!(!account_slot.exists());
        assert!(!kimi.exists());
        assert!(claude_code_own.exists());
    }

    #[test]
    fn keeps_codex_temp_files_the_write_engine_still_needs() {
        let fx = fixture();
        let codex_dir = fx.roots.codex_auth.parent().unwrap();
        let staged = touch(&codex_dir.join("auth.json.tmp.1.2.3"), 2 * HOUR);
        let orphan = touch(&codex_dir.join("auth.json.tmp.4.5.6"), 2 * HOUR);
        let state = serde_json::json!({
            "version": 1,
            "apps": {
                "codex": {
                    "pending": {
                        "op": "switch",
                        "files": [{
                            "path": fx.roots.codex_auth,
                            "pre": null,
                            "planned": "abc",
                            "staged": staged,
                        }],
                    },
                },
            },
        });
        fs::write(
            fx.roots.device.state_path(),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();

        assert_eq!(cleanup(&fx.roots, HOUR), 1);

        assert!(staged.exists(), "crash recovery rolls forward from it");
        assert!(!orphan.exists());
    }

    #[test]
    fn skips_codex_dir_when_engine_state_is_unreadable() {
        let fx = fixture();
        let codex_dir = fx.roots.codex_auth.parent().unwrap();
        let temp = touch(&codex_dir.join("auth.json.tmp.1.2.3"), 2 * HOUR);
        let state_path = fx.roots.device.state_path();
        fs::write(&state_path, b"{not json").unwrap();

        assert_eq!(cleanup(&fx.roots, HOUR), 0);

        assert!(temp.exists());
        assert_eq!(
            fs::read(&state_path).unwrap(),
            b"{not json",
            "the state file must not be moved aside by the cleanup"
        );
    }
}
