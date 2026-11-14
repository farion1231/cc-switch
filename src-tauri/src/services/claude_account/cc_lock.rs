//! 和 Claude Code 共用的目录锁（与它用的 proper-lockfile 兼容）。
//!
//! proper-lockfile 的锁是一个目录：`mkdir` 成功就拿到锁，删目录就释放；别人看到目录已经在、
//! 而且 mtime 比 `now - stale` 还旧，就当持有者已经死了，删掉重抢。持有者每隔 `update`
//! 毫秒 touch 一次 mtime 续命。
//!
//! 我们每次只持有很短的时间（远小于 Claude Code 给每把锁定的 `stale`），所以不续命：拿到锁
//! 之后记下目录的 mtime，释放时只删 mtime 还是自己那一个的目录（被别人当成过期接管了就
//! 不碰）。也不写 `.owner` 文件：Claude Code 对没有 owner 记录的锁只按过期规则处理。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::error::AppError;

/// 一把锁的参数：Claude Code 判过期的时长，和我们愿意等多久。
#[derive(Debug, Clone, Copy)]
pub struct LockSpec {
    pub stale: Duration,
    pub wait: Duration,
}

/// 拿到的锁，`drop` 时释放。
#[derive(Debug)]
pub struct CcLock {
    path: PathBuf,
    mtime: Option<SystemTime>,
}

const POLL: Duration = Duration::from_millis(100);

impl CcLock {
    /// 拿锁：目录已在且没过期就每 100ms 再试，最多等 `spec.wait`。
    pub fn acquire(path: &Path, spec: LockSpec) -> Result<CcLock, AppError> {
        let started = Instant::now();
        let mut parent_created = false;
        loop {
            match std::fs::create_dir(path) {
                Ok(()) => {
                    return Ok(CcLock {
                        path: path.to_path_buf(),
                        mtime: mtime_of(path),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // 和 proper-lockfile 一样：过期的锁删掉马上重抢。删不掉（里面有东西、没权限，
                    // 或别人刚抢走）就和没过期一样等着，受下面的时限约束
                    if let Some(seen) = stale_mtime(path, spec.stale) {
                        if take_over(path, seen) {
                            continue;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // 父目录不存在（全新安装）：建好马上重抢一次；之后还找不到（被别人删了）就按时限等
                    let parent = path.parent().ok_or_else(|| AppError::io(path, error))?;
                    std::fs::create_dir_all(parent)
                        .map_err(|source| AppError::io(parent, source))?;
                    if !parent_created {
                        parent_created = true;
                        continue;
                    }
                }
                Err(source) => return Err(AppError::io(path, source)),
            }
            if started.elapsed() >= spec.wait {
                return Err(AppError::localized(
                    "claude.snapshot.lock_busy",
                    "Claude Code 正在使用登录信息（刷新或登录中），请稍后再切换",
                    "Claude Code is using its sign-in right now (refreshing or signing in). Try switching again in a moment.",
                ));
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for CcLock {
    fn drop(&mut self) {
        let current = mtime_of(&self.path);
        if current.is_some() && current == self.mtime {
            let _ = std::fs::remove_dir(&self.path);
        } else {
            log::warn!(
                "[ClaudeAccount] 锁已被别人接管，不删除: {}",
                self.path.display()
            );
        }
    }
}

/// 锁过期了就返回它的 mtime。
fn stale_mtime(path: &Path, stale: Duration) -> Option<SystemTime> {
    let modified = mtime_of(path)?;
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age > stale)
        .then_some(modified)
}

/// 删掉一把看到过期的锁。删之前再看一次 mtime：另一个等锁的进程可能已经先删了它、建了自己
/// 的新锁（mtime 变了），这时删就会让两边同时进锁（claude-swap #420）。两步之间仍有极小的
/// 窗口，目录锁协议本身消除不了。
fn take_over(path: &Path, seen: SystemTime) -> bool {
    mtime_of(path) == Some(seen) && std::fs::remove_dir(path).is_ok()
}

fn mtime_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(wait_ms: u64) -> LockSpec {
        LockSpec {
            stale: Duration::from_secs(10),
            wait: Duration::from_millis(wait_ms),
        }
    }

    #[test]
    fn second_holder_waits_then_gives_up() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".storage-write.lock");
        let first = CcLock::acquire(&path, spec(0)).unwrap();
        assert!(path.is_dir());
        assert!(CcLock::acquire(&path, spec(250)).is_err());
        drop(first);
        assert!(!path.exists());
        let _again = CcLock::acquire(&path, spec(0)).unwrap();
    }

    #[test]
    fn stale_lock_is_taken_over() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.lock");
        std::fs::create_dir(&path).unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        let _ = std::fs::File::open(&path)
            .and_then(|dir| dir.set_times(std::fs::FileTimes::new().set_modified(old)));
        // 打不开目录的平台上（Windows）跳过：mtime 改不了就构造不出过期锁
        if stale_mtime(&path, Duration::from_secs(10)).is_some() {
            let lock = CcLock::acquire(&path, spec(0)).unwrap();
            assert!(path.is_dir());
            drop(lock);
            assert!(!path.exists());
        }
    }

    #[test]
    fn stale_lock_renewed_meanwhile_is_not_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.lock");
        std::fs::create_dir(&path).unwrap();
        let set_mtime = |time: SystemTime| {
            std::fs::File::open(&path)
                .and_then(|dir| dir.set_times(std::fs::FileTimes::new().set_modified(time)))
                .is_ok()
        };
        // 打不开目录的平台上（Windows）跳过：mtime 改不了
        if !set_mtime(SystemTime::now() - Duration::from_secs(60)) {
            return;
        }
        let seen = stale_mtime(&path, Duration::from_secs(10)).unwrap();
        // 看到过期之后，另一个进程删掉它、建了自己的新锁
        assert!(set_mtime(SystemTime::now()));
        assert!(!take_over(&path, seen));
        assert!(path.is_dir());
    }

    #[test]
    fn stale_lock_that_cannot_be_removed_times_out() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.lock");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("junk"), "x").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        let _ = std::fs::File::open(&path)
            .and_then(|dir| dir.set_times(std::fs::FileTimes::new().set_modified(old)));
        // 过期但删不掉（非空）：到时限报忙，不能死循环
        assert!(CcLock::acquire(&path, spec(300)).is_err());
    }

    #[test]
    fn missing_parent_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("new").join(".oauth_refresh.lock");
        let _lock = CcLock::acquire(&path, spec(0)).unwrap();
        assert!(path.is_dir());
    }
}
