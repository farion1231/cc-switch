//! 落盘放在单独的线程上：往 App Group 容器写文件时，macOS 可能先做一次文件访问权限检查
//! （重新签名后的第一次写入会弹窗询问，用户点了才继续），这期间写入会一直卡着。
//! 调用方只登记「最新要写成什么」就返回，不堵后面的导出和刷新；卡住期间的多次导出只落盘最新一份。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::reload;

pub enum Job {
    /// 写快照；`content` 是去掉写入时间后的内容（判断要不要通知小组件重读）
    Write {
        path: PathBuf,
        bytes: Vec<u8>,
        content: Vec<u8>,
        summary: String,
    },
    /// 关掉小组件：删快照
    Delete { path: PathBuf },
}

static PENDING: Mutex<Option<Job>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

fn take() -> Option<Job> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// 登记最新的落盘任务（覆盖还没执行的那个），需要时起写线程。
pub fn submit(job: Job) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(job);
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(|| loop {
        match take() {
            Some(job) => run(job),
            None => {
                BUSY.store(false, Ordering::Release);
                // 放下标记的同时有新任务进来、却看到 BUSY 还在而没起线程：接着干
                let more = PENDING.lock().unwrap_or_else(|e| e.into_inner()).is_some();
                if !more || BUSY.swap(true, Ordering::AcqRel) {
                    break;
                }
            }
        }
    });
}

fn run(job: Job) {
    let started = Instant::now();
    match job {
        Job::Write {
            path,
            bytes,
            content,
            summary,
        } => {
            let result = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|e| e.to_string())
                .and_then(|()| {
                    crate::config::atomic_write(&path, &bytes).map_err(|e| e.to_string())
                });
            match result {
                Ok(()) => {
                    log::debug!(
                        "[Widget] 已写快照：{summary}（落盘 {} ms）",
                        started.elapsed().as_millis()
                    );
                    reload::snapshot_written(&content);
                }
                Err(e) => log::warn!("[Widget] 写入小组件快照失败 {}: {e}", path.display()),
            }
        }
        Job::Delete { path } => {
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("[Widget] 删除小组件快照失败 {}: {e}", path.display());
                }
            }
            // 让小组件马上换成「未开启」
            reload::forget();
            reload::snapshot_written(&[]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_job_wins_and_writer_finishes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("group").join("snap.json");
        for i in 0..20 {
            submit(Job::Write {
                path: path.clone(),
                bytes: format!("{{\"n\":{i}}}").into_bytes(),
                content: vec![],
                summary: String::new(),
            });
        }
        // 等写线程把最后一份落盘
        for _ in 0..200 {
            if !BUSY.load(Ordering::Acquire)
                && std::fs::read_to_string(&path).ok().as_deref() == Some("{\"n\":19}")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"n\":19}");
        submit(Job::Delete { path: path.clone() });
        for _ in 0..200 {
            if !path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!path.exists());
    }
}
