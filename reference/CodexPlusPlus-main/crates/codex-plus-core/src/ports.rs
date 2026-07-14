use std::fs::File;
use std::net::{TcpListener, ToSocketAddrs};
use std::path::{Path, PathBuf};

use fs2::FileExt;

pub const LAUNCHER_GUARD_PORT_BASE: u16 = 57320;
pub const MANAGER_GUARD_PORT_BASE: u16 = 57319;

/// Offset applied to guard port base to avoid conflicts in multi-user
/// environments (Windows RDP, shared servers, etc.).
///
/// Resolution order:
/// 1. `CODEX_PLUS_GUARD_PORT` env var — exact port override
/// 2. `CODEX_PLUS_GUARD_PORT_OFFSET` env var — explicit numeric offset
/// 3. Windows: hash of `USERNAME` (mod 1000) for per-user isolation
/// 4. Other platforms: 0 (backward-compatible default)
fn guard_port_offset() -> u16 {
    // env var exact port takes priority (caller handles it via override functions below)
    #[cfg(windows)]
    {
        if let Ok(user) = std::env::var("USERNAME") {
            let hash: u16 = user.bytes().fold(0u16, |acc, b| acc.wrapping_add(b as u16));
            return hash % 1000;
        }
    }
    0
}

/// Effective launcher guard port (base + auto-offset, overridable via env var).
pub fn launcher_guard_port() -> u16 {
    if let Some(port) = std::env::var("CODEX_PLUS_GUARD_PORT")
        .or_else(|_| std::env::var("CODEX_PLUS_LAUNCHER_GUARD_PORT"))
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        return port;
    }
    if let Some(offset) = std::env::var("CODEX_PLUS_GUARD_PORT_OFFSET")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        return LAUNCHER_GUARD_PORT_BASE + offset;
    }
    LAUNCHER_GUARD_PORT_BASE + guard_port_offset()
}

/// Effective manager guard port (base + auto-offset, overridable via env var).
pub fn manager_guard_port() -> u16 {
    if let Some(port) = std::env::var("CODEX_PLUS_GUARD_PORT")
        .or_else(|_| std::env::var("CODEX_PLUS_MANAGER_GUARD_PORT"))
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        return port;
    }
    if let Some(offset) = std::env::var("CODEX_PLUS_GUARD_PORT_OFFSET")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
    {
        return MANAGER_GUARD_PORT_BASE + offset;
    }
    MANAGER_GUARD_PORT_BASE + guard_port_offset()
}

pub fn select_platform_loopback_port(requested: u16) -> u16 {
    select_platform_loopback_port_with(
        requested,
        cfg!(windows),
        can_bind_loopback_port,
        find_available_loopback_port,
    )
}

pub fn select_packaged_codex_debug_port(requested: u16) -> u16 {
    select_packaged_codex_debug_port_with(
        requested,
        cfg!(windows),
        can_bind_loopback_port,
        find_available_loopback_port,
    )
}

pub fn select_packaged_codex_debug_port_with(
    requested: u16,
    is_windows: bool,
    can_bind: impl Fn(u16) -> bool,
    find_available: impl Fn() -> u16,
) -> u16 {
    select_platform_loopback_port_with(requested, is_windows, can_bind, find_available)
}

pub fn select_platform_loopback_port_with(
    requested: u16,
    is_windows: bool,
    can_bind: impl Fn(u16) -> bool,
    find_available: impl Fn() -> u16,
) -> u16 {
    if !is_windows || can_bind(requested) {
        requested
    } else {
        find_available()
    }
}

pub fn can_bind_loopback_port(port: u16) -> bool {
    if port == 0 {
        return true;
    }
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub fn find_available_loopback_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .unwrap_or(0)
}

pub fn can_connect_loopback_port(port: u16) -> bool {
    ("127.0.0.1", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .and_then(|address| {
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200))
                .ok()
        })
        .is_some()
}

pub fn acquire_loopback_port_guard(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(("127.0.0.1", port))
}

#[derive(Debug)]
pub struct LoopbackPortGuard {
    _lock_file: Option<File>,
    lock_path: Option<PathBuf>,
    _listener: Option<TcpListener>,
    using_fallback_lock: bool,
}

impl LoopbackPortGuard {
    pub fn listener(listener: TcpListener) -> Self {
        Self {
            _lock_file: None,
            lock_path: None,
            _listener: Some(listener),
            using_fallback_lock: false,
        }
    }

    fn locked_listener(file: File, path: PathBuf, listener: TcpListener) -> Self {
        Self {
            _lock_file: Some(file),
            lock_path: Some(path),
            _listener: Some(listener),
            using_fallback_lock: false,
        }
    }

    fn fallback_lock(file: File, path: PathBuf) -> Self {
        Self {
            _lock_file: Some(file),
            lock_path: Some(path),
            _listener: None,
            using_fallback_lock: true,
        }
    }

    pub fn fallback_path(&self) -> Option<&Path> {
        self.using_fallback_lock
            .then_some(())
            .and_then(|_| self.lock_path.as_deref())
    }
}

pub fn acquire_resilient_loopback_port_guard(port: u16) -> std::io::Result<LoopbackPortGuard> {
    acquire_resilient_loopback_port_guard_at(port, &crate::paths::default_app_state_dir())
}

fn acquire_resilient_loopback_port_guard_at(
    port: u16,
    state_dir: &Path,
) -> std::io::Result<LoopbackPortGuard> {
    acquire_resilient_loopback_port_guard_with(
        port,
        state_dir,
        acquire_loopback_port_guard,
        can_connect_loopback_port,
    )
}

fn acquire_resilient_loopback_port_guard_with(
    port: u16,
    state_dir: &Path,
    bind: impl Fn(u16) -> std::io::Result<TcpListener>,
    can_connect: impl Fn(u16) -> bool,
) -> std::io::Result<LoopbackPortGuard> {
    if port == 0 {
        return bind(port).map(LoopbackPortGuard::listener);
    }

    let (file, path) = acquire_lock_guard(port, state_dir)?;
    match bind(port) {
        Ok(listener) => Ok(LoopbackPortGuard::locked_listener(file, path, listener)),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && can_connect(port) => {
            Err(error)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            Ok(LoopbackPortGuard::fallback_lock(file, path))
        }
        Err(error) => Err(error),
    }
}

fn acquire_lock_guard(port: u16, state_dir: &Path) -> std::io::Result<(File, PathBuf)> {
    let dir = state_dir.join("locks");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("loopback-port-{port}.lock"));
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.try_lock_exclusive().map_err(normalize_lock_error)?;
    Ok((file, path))
}

fn normalize_lock_error(error: std::io::Error) -> std::io::Error {
    match error.raw_os_error() {
        Some(33) => std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "loopback port guard lock is already held",
        ),
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static GUARD_PORT_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn resilient_guard_holds_lock_and_listener_when_requested_port_is_available() {
        let temp = tempfile::tempdir().unwrap();
        let port = find_available_loopback_port();

        let guard = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap();

        assert!(guard.lock_path.is_some());
        assert!(guard._listener.is_some());
        assert!(guard.fallback_path().is_none());
    }

    #[test]
    fn resilient_guard_reports_lock_conflict_when_instance_lock_is_held() {
        let temp = tempfile::tempdir().unwrap();
        let port = find_available_loopback_port();
        let _guard = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap();

        let second = acquire_resilient_loopback_port_guard_at(port, temp.path()).unwrap_err();

        assert_eq!(second.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn resilient_guard_reports_conflict_when_requested_port_is_connectable() {
        let temp = tempfile::tempdir().unwrap();
        let error = acquire_resilient_loopback_port_guard_with(
            57319,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "port busy",
                ))
            },
            |_| true,
        )
        .unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    }

    #[test]
    fn resilient_guard_uses_lock_fallback_when_requested_port_is_not_connectable() {
        let temp = tempfile::tempdir().unwrap();
        let guard = acquire_resilient_loopback_port_guard_with(
            57319,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "stale port",
                ))
            },
            |_| false,
        )
        .unwrap();

        assert!(guard._listener.is_none());
        assert!(guard.fallback_path().is_some());

        let second = acquire_resilient_loopback_port_guard_with(
            57319,
            temp.path(),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "stale port",
                ))
            },
            |_| false,
        )
        .unwrap_err();
        assert_eq!(second.kind(), std::io::ErrorKind::WouldBlock);
    }

    #[test]
    fn launcher_guard_port_returns_base_when_no_env_override() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        let port = launcher_guard_port();
        // On non-Windows: LAUNCHER_GUARD_PORT_BASE + 0
        // On Windows: LAUNCHER_GUARD_PORT_BASE + USERNAME hash mod 1000
        assert!(port >= LAUNCHER_GUARD_PORT_BASE);
        assert!(port < LAUNCHER_GUARD_PORT_BASE + 1000);
    }

    #[test]
    fn manager_guard_port_returns_base_when_no_env_override() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        let port = manager_guard_port();
        assert!(port >= MANAGER_GUARD_PORT_BASE);
        assert!(port < MANAGER_GUARD_PORT_BASE + 1000);
    }

    #[test]
    fn launcher_guard_port_honors_env_override() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        unsafe { std::env::set_var("CODEX_PLUS_GUARD_PORT", "9999") };
        let port = launcher_guard_port();
        unsafe { std::env::remove_var("CODEX_PLUS_GUARD_PORT") };
        assert_eq!(port, 9999);
    }

    #[test]
    fn launcher_guard_port_honors_specific_env_override() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        unsafe { std::env::set_var("CODEX_PLUS_LAUNCHER_GUARD_PORT", "8888") };
        let port = launcher_guard_port();
        unsafe { std::env::remove_var("CODEX_PLUS_LAUNCHER_GUARD_PORT") };
        assert_eq!(port, 8888);
    }

    #[test]
    fn manager_guard_port_honors_specific_env_override() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        unsafe { std::env::set_var("CODEX_PLUS_MANAGER_GUARD_PORT", "7777") };
        let port = manager_guard_port();
        unsafe { std::env::remove_var("CODEX_PLUS_MANAGER_GUARD_PORT") };
        assert_eq!(port, 7777);
    }

    #[test]
    fn launcher_guard_port_honors_offset_env() {
        let _guard = guard_port_env_lock();
        _clear_guard_port_env_vars();
        unsafe { std::env::set_var("CODEX_PLUS_GUARD_PORT_OFFSET", "50") };
        let port = launcher_guard_port();
        unsafe { std::env::remove_var("CODEX_PLUS_GUARD_PORT_OFFSET") };
        assert_eq!(port, LAUNCHER_GUARD_PORT_BASE + 50);
    }

    fn guard_port_env_lock() -> MutexGuard<'static, ()> {
        GUARD_PORT_ENV_LOCK
            .lock()
            .expect("guard port env lock should not be poisoned")
    }
}

/// Clear all guard-port env vars to prevent cross-test contamination
/// when cargo runs tests in parallel threads.
fn _clear_guard_port_env_vars() {
    unsafe {
        let _ = std::env::remove_var("CODEX_PLUS_GUARD_PORT");
        let _ = std::env::remove_var("CODEX_PLUS_LAUNCHER_GUARD_PORT");
        let _ = std::env::remove_var("CODEX_PLUS_MANAGER_GUARD_PORT");
        let _ = std::env::remove_var("CODEX_PLUS_GUARD_PORT_OFFSET");
    }
}
