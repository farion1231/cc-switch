//! 全 crate 共享的测试基础设施。
//!
//! ## 跨模块 env 竞态（第五轮根因）
//!
//! 历史上各模块各自维护 `OnceLock<Mutex<()>>` 来串行化本模块改全局 env 的测试，但
//! 这把锁只锁本模块——一个 `#[serial]` 的 codex 测试与一个用本地 mutex 的
//! openclaw 测试可以真并发，因为 `serial_test::#[serial]` 只协调**同分组**的测试。
//!
//! 后果：
//! - `openclaw_config::tests::default_model_noop_write_skips_backup` 间歇失败
//!   （HEAD 1/4），且失败时会把开发者真实的 `~/.openclaw/openclaw.json`（含各
//!   provider 的 API key）打进 panic 输出——CI 日志泄露风险。
//! - 其他模块（hermes_config、provider_bundle、codex_config）改
//!   `CC_SWITCH_TEST_HOME` / `HOME` / `CODEX_HOME` 同样会与 openclaw 互相串。
//!
//! ## 用法
//!
//! 所有改进程全局 env（`CC_SWITCH_TEST_HOME` / `HOME` / `CODEX_HOME` 及其等效
//! 触发路径）且会跨测试调用**路径解析**的测试，都应在测试体开头
//! `let _guard = test_support::env_lock().lock().unwrap_or_else(|e| e.into_inner());`
//! 持有这把共享锁。各模块旧的 `test_guard()` 实现已经统一改为委托到此锁。
//!
//! ## 不要做的事
//!
//! - 不要在这把锁内做 IO 或 sleep——它保护的是「切换 HOME 等 env」与「读
//!   `get_home_dir()`」之间的一致性，不是资源互斥。
//! - 不要在 production code 引用本模块（`#[cfg(test)]` 守住）。

use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// 全 crate 共享的 env 锁。第一次调用时初始化，之后每次调用都返回同一把锁。
///
/// 返回 `&'static Mutex<()>`，调用方 `.lock()` 后持锁到本测试 scope 结束。
/// 锁中毒时（持有线程 panic）通过 `into_inner()` 自愈——测试间互不阻断。
pub fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// **可重入**的 env 锁守卫。同一线程重复获取只加计数，真正加锁只发生在
/// 第一次（计数 0→1）；最后一个守卫 drop 时（计数 1→0）才释放底层 mutex。
///
/// ## 为什么必须是可重入的
///
/// 一个测试里很容易有多个「设 env 再还原」的 RAII 包装（例如
/// `provider_bundle::tests::EnvRestore` 在同一个测试里先包 HOME/CC_SWITCH_TEST_HOME、
/// 后包 KAIXUAN_TEST_KEY2）。若每次构造都去 lock 同一把**非重入**的
/// `std::sync::Mutex`，第二个包装会永久阻塞在自己已经持有的锁上——
/// `install_bundle_preserves_user_prefilled_settings` 就是这样挂死的：
/// 栈停在 `EnvRestore::new → std::sync::Mutex::lock`，而它自己外层已经持着锁。
///
/// `#[serial]` 挡不住这个：它是测试**之间**的串行化，同一个测试内部的两次获取
/// 它看不见。
pub struct EnvGuard {
    /// 仅在计数 1→0 时有值；重入期间是 None。
    inner: Option<MutexGuard<'static, ()>>,
}

thread_local! {
    /// 当前线程持有的 env 锁嵌套深度。
    static ENV_LOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl EnvGuard {
    /// 获取（可重入）。
    pub fn acquire() -> Self {
        let already_held = ENV_LOCK_DEPTH.with(|d| d.get()) > 0;
        ENV_LOCK_DEPTH.with(|d| d.set(d.get() + 1));
        let inner = if already_held {
            // 同一线程已持有：只加深度，不重复加锁。
            None
        } else {
            Some(
                env_lock()
                    .lock()
                    // 中毒自愈：持有线程 panic 不应该让后续所有测试永久卡死。
                    .unwrap_or_else(|err| err.into_inner()),
            )
        };
        Self { inner }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        ENV_LOCK_DEPTH.with(|d| {
            let depth = d.get();
            if depth > 0 {
                d.set(depth - 1);
            }
        });
        // 只有最外层（inner 为 Some）才真正释放锁。
        drop(self.inner.take());
    }
}

/// 便捷 API：拿到可重入的 env 锁守卫。优先用这个而不是 `env_lock().lock()`。
pub fn env_guard() -> EnvGuard {
    EnvGuard::acquire()
}

/// 便捷宏：把 `let _guard = test_support::env_lock().lock();` 浓缩成
/// `env_locked!();`。与 `#[serial]` 不冲突但更强（也覆盖没标 `#[serial]` 的测试）。
#[macro_export]
macro_rules! env_locked {
    () => {
        let _env_guard = $crate::test_support::env_guard();
    };
}

/// 把含敏感凭据的配置内容打码后再用于断言，避免 panic 信息把 API key 打进
/// CI 日志。具体规则：把任意形如 `"<key>": "<value>"` 或 `<key>: '<value>'`（json5）
/// 的 value 替换成 `"<REDACTED len=N>"`，长度仍保留便于 debug 大小变化。
///
/// 这是兜底——根因是跨模块 env 竞态让测试读到**真实** `~/.openclaw/openclaw.json`，
/// 那份文件含各 provider 的 API key。env_lock 已尽量杜绝这条路径，但作为
/// 「即使偶发命中也不会泄漏」的二道闸，所有在 panic 里出现 openclaw/codex/auth.json
/// 内容的断言，都应走本函数。
pub fn redact_secrets_in_config(content: &str) -> String {
    // 简易：匹配 `"...: "..."` 或 `...: '...'`，value 用 REDACTED 替换。
    // 这是粗粒度的——JSON5 字符串字面量、双引号 / 单引号都能命中；用户自定义的非
    // KV 写法不会被改写，且即便误改写也只会让断言失败信息变模糊，不会泄漏。
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 找冒号
        if bytes[i] == b':' && i + 1 < bytes.len() {
            out.push(':');
            i += 1;
            // 跳空白
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                out.push(bytes[i] as char);
                i += 1;
            }
            // 引号开始
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                out.push(quote as char);
                i += 1;
                let value_start = i;
                // 找匹配的结束引号（不处理转义——这是粗粒度脱敏）
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                let value_len = i - value_start;
                out.push_str(&format!("<REDACTED len={value_len}>"));
                if i < bytes.len() {
                    out.push(quote as char);
                    i += 1;
                }
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_lock_returns_same_instance() {
        let a = env_lock();
        let b = env_lock();
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn env_lock_recovers_from_poison() {
        // 锁中毒后下一次 lock() 不应阻塞（unwrap_or_else 走 into_inner）
        let _ = std::panic::catch_unwind(|| {
            let _g = env_lock().lock().unwrap();
            panic!("intentional poison");
        });
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
    }
}
