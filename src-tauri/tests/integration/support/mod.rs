use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cc_switch_lib::{update_settings, AppSettings, AppState, Database, MultiAppConfig};

/// 为测试设置隔离的 HOME 目录，避免污染真实用户数据。
///
/// 每个测试进程独占一个槽位目录：cargo test 下全部集成测试在同一个进程里（靠 `test_mutex`
/// 串行），nextest 每个测试一个进程。槽位靠目录旁 `.lock` 文件上的独占锁认领，锁跟着进程走，
/// 进程退出（含崩溃）由系统释放。所以同时跑的进程（两次 cargo test、多个 nextest 进程、
/// 多个 worktree）不会清掉彼此的目录，目录数也不超过同时运行的进程数。
pub fn ensure_test_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let base = claim_test_home_slot();
        if base.exists() {
            let _ = std::fs::remove_dir_all(&base);
        }
        std::fs::create_dir_all(&base).expect("create test home");
        // Windows 上 `dirs::home_dir()` 不受 HOME/USERPROFILE 影响（走 Known Folder API），
        // 用 CC_SWITCH_TEST_HOME 显式覆盖，以确保测试不会污染真实用户目录。
        std::env::set_var("CC_SWITCH_TEST_HOME", &base);
        std::env::set_var("HOME", &base);
        #[cfg(windows)]
        std::env::set_var("USERPROFILE", &base);
        // Claude Desktop 的配置目录在 Windows 上只读 LOCALAPPDATA（见 claude_desktop_config.rs
        // 的 windows_local_app_data_dir），既不认 CC_SWITCH_TEST_HOME 也不认 HOME。不覆盖它，
        // 涉及 Claude Desktop 供应商切换的测试会写进开发者真实的桌面版配置。
        #[cfg(windows)]
        std::env::set_var("LOCALAPPDATA", base.join("AppData").join("Local"));
        // Pi 配置目录优先认 PI_CODING_AGENT_DIR：开发者 shell 里设了它，Pi 相关测试会写进真实目录
        std::env::remove_var("PI_CODING_AGENT_DIR");
        base
    })
    .as_path()
}

// `rust-version` 落后于 rust-toolchain.toml 固定的工具链，后者有 `File::try_lock`。
#[allow(clippy::incompatible_msrv)]
fn claim_test_home_slot() -> PathBuf {
    static SLOT_LOCK: OnceLock<std::fs::File> = OnceLock::new();
    let temp = std::env::temp_dir();
    for slot in 0.. {
        let lock_path = temp.join(format!("cc-switch-test-home-{slot}.lock"));
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .expect("open test home lock");
        match lock.try_lock() {
            Ok(()) => {
                SLOT_LOCK.set(lock).expect("test home slot claimed once");
                return temp.join(format!("cc-switch-test-home-{slot}"));
            }
            Err(std::fs::TryLockError::WouldBlock) => continue,
            Err(std::fs::TryLockError::Error(e)) => panic!("lock {}: {e}", lock_path.display()),
        }
    }
    unreachable!("slot numbers are unbounded")
}

/// 清理测试目录中生成的配置文件与缓存。
pub fn reset_test_fs() {
    let home = ensure_test_home();
    for sub in [
        ".claude",
        ".codex",
        ".cc-switch",
        ".gemini",
        ".grok",
        ".hermes",
        ".config",
        ".minimax",
        ".omp",
        ".openclaw",
        ".pi",
        ".workbuddy",
        "profiles",
    ] {
        let path = home.join(sub);
        if path.exists() {
            if let Err(err) = std::fs::remove_dir_all(&path) {
                eprintln!("failed to clean {}: {}", path.display(), err);
            }
        }
    }
    let claude_json = home.join(".claude.json");
    if claude_json.exists() {
        let _ = std::fs::remove_file(&claude_json);
    }

    // 重置内存中的设置缓存，确保测试环境不受上一次调用影响
    let _ = update_settings(AppSettings::default());
}

pub fn enable_codex_official_auth_preservation() {
    update_settings(AppSettings {
        preserve_codex_official_auth_on_switch: true,
        ..Default::default()
    })
    .expect("enable Codex official auth preservation");
}

/// 全局互斥锁，避免多测试并发写入相同的 HOME 目录。
pub fn test_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

/// 创建测试用的 AppState，包含一个空的数据库
pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    Ok(AppState::new(db))
}

/// 创建测试用的 AppState，并从 MultiAppConfig 迁移数据
pub fn create_test_state_with_config(
    config: &MultiAppConfig,
) -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    db.migrate_from_json(config)?;
    Ok(AppState::new(db))
}
