use std::sync::{Mutex, MutexGuard};

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
    assert!(
        output.status.success(),
        "isolated poison test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;

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
