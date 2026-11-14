//! Kimi Code serializes refresh-token rotation with proper-lockfile on
//! `{home}/oauth/<name>`: the lock is the directory `<name>.lock`, it is stale
//! after 5s without an mtime update, and holders touch it while they work.
//! CC Switch takes the same lock before writing a credential, so neither side
//! can overwrite a refresh token the other has just rotated.
//!
//! Kimi disables the lock on Windows and with `KIMI_DISABLE_OAUTH_LOCK=1`.
//! A lock only Kimi ignores protects nothing, so callers must not refresh
//! tokens there (see [`refresh_supported`]).
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, SystemTime},
};

const STALE: Duration = Duration::from_secs(5);
const TOUCH_EVERY: Duration = Duration::from_millis(1000);
const RETRY_MIN: Duration = Duration::from_millis(500);
const RETRY_MAX: Duration = Duration::from_millis(1000);
const RETRIES: u32 = 120;

/// Whether Kimi itself locks around refresh on this machine.
pub(super) fn refresh_supported() -> bool {
    !cfg!(windows) && std::env::var("KIMI_DISABLE_OAUTH_LOCK").as_deref() != Ok("1")
}

/// Held lock; dropping it stops the heartbeat and removes the lock directory.
pub(super) struct CredentialLock {
    dir: PathBuf,
    stop: Arc<AtomicBool>,
    heartbeat: Option<thread::JoinHandle<()>>,
}

impl Drop for CredentialLock {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(heartbeat) = self.heartbeat.take() {
            let _ = heartbeat.join();
        }
        let _ = fs::remove_dir(&self.dir);
    }
}

fn target(home: &Path, name: &str) -> PathBuf {
    home.join("oauth").join(name)
}

fn prepare(home: &Path, name: &str) -> Result<PathBuf, String> {
    let target = target(home, name);
    let parent = target.parent().unwrap();
    fs::create_dir_all(parent).map_err(|e| format!("Cannot prepare Kimi OAuth lock: {e}"))?;
    // proper-lockfile runs with `realpath: false`, but Kimi still creates the
    // target first; mirror that so both sides agree on the same paths.
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)
        .map_err(|e| format!("Cannot prepare Kimi OAuth lock: {e}"))?;
    let mut dir = target.into_os_string();
    dir.push(".lock");
    Ok(dir.into())
}

fn touch(dir: &Path) {
    if let Ok(file) = fs::File::open(dir) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// One acquisition attempt: `Ok(true)` when the lock is ours.
fn try_acquire(dir: &Path) -> Result<bool, String> {
    match fs::create_dir(dir) {
        Ok(()) => {
            touch(dir);
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = fs::metadata(dir)
                .and_then(|meta| meta.modified())
                .map(|modified| {
                    SystemTime::now()
                        .duration_since(modified)
                        .is_ok_and(|age| age > STALE)
                })
                // Vanished between mkdir and stat: retry right away.
                .unwrap_or(true);
            if stale {
                let _ = fs::remove_dir(dir);
                return match fs::create_dir(dir) {
                    Ok(()) => {
                        touch(dir);
                        Ok(true)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
                    Err(e) => Err(format!("Cannot acquire Kimi OAuth lock: {e}")),
                };
            }
            Ok(false)
        }
        Err(e) => Err(format!("Cannot acquire Kimi OAuth lock: {e}")),
    }
}

fn hold(dir: PathBuf) -> CredentialLock {
    let stop = Arc::new(AtomicBool::new(false));
    let heartbeat = {
        let stop = stop.clone();
        let dir = dir.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if fs::metadata(&dir)
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .is_some_and(|age| age >= TOUCH_EVERY)
                {
                    touch(&dir);
                }
            }
        })
    };
    CredentialLock {
        dir,
        stop,
        heartbeat: Some(heartbeat),
    }
}

fn backoff(attempt: u32) -> Duration {
    // Same window as Kimi: 120 retries spaced 0.5–1s apart.
    let spread = (RETRY_MAX - RETRY_MIN).as_millis() as u32;
    RETRY_MIN + Duration::from_millis(u64::from(attempt * 97 % (spread + 1)))
}

fn timeout(name: &str) -> String {
    format!("Kimi Code is refreshing the \"{name}\" login; try again in a moment")
}

/// Wait for the lock without blocking the async runtime. Returns `None` where
/// Kimi does not lock either, so writes behave exactly as Kimi's own.
pub(super) async fn acquire(home: &Path, name: &str) -> Result<Option<CredentialLock>, String> {
    if !refresh_supported() {
        return Ok(None);
    }
    let dir = prepare(home, name)?;
    for attempt in 0..=RETRIES {
        if try_acquire(&dir)? {
            return Ok(Some(hold(dir)));
        }
        tokio::time::sleep(backoff(attempt)).await;
    }
    Err(timeout(name))
}

/// Blocking variant for synchronous provider operations.
pub(super) fn acquire_blocking(home: &Path, name: &str) -> Result<Option<CredentialLock>, String> {
    if !refresh_supported() {
        return Ok(None);
    }
    let dir = prepare(home, name)?;
    for attempt in 0..=RETRIES {
        if try_acquire(&dir)? {
            return Ok(Some(hold(dir)));
        }
        thread::sleep(backoff(attempt));
    }
    Err(timeout(name))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn waits_for_a_live_holder_takes_over_a_stale_one_and_cleans_up() {
        let home = tempfile::tempdir().unwrap();
        let dir = prepare(home.path(), "kimi-code").unwrap();
        assert!(home.path().join("oauth/kimi-code").is_file());
        assert_eq!(dir, home.path().join("oauth/kimi-code.lock"));

        // A live holder keeps touching its lock: we must keep waiting.
        let held = hold({
            fs::create_dir(&dir).unwrap();
            dir.clone()
        });
        assert!(!try_acquire(&dir).unwrap());
        thread::sleep(Duration::from_millis(1500));
        assert!(!try_acquire(&dir).unwrap());
        drop(held);
        assert!(!dir.exists());

        // A crashed holder stops touching: after the stale window it is ours.
        fs::create_dir(&dir).unwrap();
        let old = SystemTime::now() - Duration::from_secs(10);
        fs::File::open(&dir).unwrap().set_modified(old).unwrap();
        let lock = acquire(home.path(), "kimi-code").await.unwrap().unwrap();
        assert!(dir.exists());
        drop(lock);
        assert!(!dir.exists());

        // Waiting callers get the lock as soon as the holder releases it.
        let first = acquire(home.path(), "kimi-code").await.unwrap().unwrap();
        let path = home.path().to_path_buf();
        let waiter = tokio::spawn(async move { acquire(&path, "kimi-code").await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!waiter.is_finished());
        drop(first);
        let second = waiter.await.unwrap().unwrap().unwrap();
        drop(second);
        assert!(!dir.exists());
    }
}
