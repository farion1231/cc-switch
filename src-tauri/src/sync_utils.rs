use std::sync::{Mutex, MutexGuard};

/// Finish an interrupted operation's rollback before resuming its original
/// panic. Callers keep their operation lock outside catch_unwind, so the lock
/// is still held during cleanup and is poisoned when the panic resumes.
/// This handles unwinding only, not process termination or an already committed
/// business transaction. Rollback must preserve the caller's conflict checks.
pub(crate) fn resume_after_rollback<T>(
    outcome: std::thread::Result<T>,
    rollback: impl FnOnce() -> Result<(), crate::error::AppError>,
) -> T {
    match outcome {
        Ok(result) => result,
        Err(payload) => {
            // Rollback (including logging an I/O failure) must not replace the
            // original panic or prevent an outer file scope from rolling back.
            let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Err(error) = rollback() {
                    log::error!("Rollback after panic failed: {error}");
                }
            }));
            if let Err(secondary) = cleanup {
                // The panic hook has reported it. An arbitrary payload's Drop
                // can itself panic, so do not drop it while preserving payload.
                std::mem::forget(secondary);
            }
            std::panic::resume_unwind(payload)
        }
    }
}

/// Serialize file operations without making one panicking caller disable the
/// file for the rest of the process. These locks contain no in-memory state;
/// callers must still reload/validate files and use their normal atomic writes
/// and rollback paths. Never use this for a lock protecting a database or cache.
pub(crate) fn lock_file_operation(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|poisoned| {
        let guard = poisoned.into_inner();
        lock.clear_poison();
        log::warn!("Recovered file operation lock after a panic");
        guard
    })
}

#[cfg(test)]
pub(crate) fn in_isolated_process(test_name: &str) -> bool {
    const CHILD: &str = "CC_SWITCH_LOCK_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test_name) {
        return true;
    }
    // Poisoning a process-wide lock must not race unrelated tests in the same
    // binary. Give the child an isolated home as well as isolated static locks.
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".cc-switch");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("cc-switch.db"), []).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, test_name)
        .env("CC_SWITCH_TEST_HOME", home.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // --exact 匹配不到测试时 libtest 也返回 0，测试改名后不能空跑通过。
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "isolated poison test failed or did not run:\n{}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_failure_preserves_original_panic_and_outer_cleanup() {
        for rollback_panics in [false, true] {
            let cleaned = std::cell::Cell::new(false);
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let inner = std::panic::catch_unwind(|| {
                    let original = std::panic::catch_unwind(|| panic!("original"));
                    resume_after_rollback(original, || {
                        if rollback_panics {
                            panic!("rollback also panicked");
                        }
                        Err(crate::error::AppError::Message("rollback failed".into()))
                    });
                });
                resume_after_rollback(inner, || {
                    cleaned.set(true);
                    Ok(())
                });
            }));
            assert!(cleaned.get());
            assert_eq!(
                caught.unwrap_err().downcast_ref::<&str>(),
                Some(&"original")
            );
        }
    }

    #[test]
    fn file_operation_lock_recovers_and_still_excludes_other_callers() {
        let lock = Mutex::new(());
        assert!(std::panic::catch_unwind(|| {
            let _guard = lock_file_operation(&lock);
            panic!("file operation interrupted");
        })
        .is_err());
        assert!(lock.is_poisoned());
        let guard = lock_file_operation(&lock);
        assert!(!lock.is_poisoned());
        assert!(matches!(
            lock.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        drop(guard);
        assert!(lock.try_lock().is_ok());
    }
}
