//! 主窗口隐藏后自动进入轻量模式的生命周期控制器。
//!
//! 这里只决定“何时进入”；窗口销毁与恢复始终复用 `lightweight` 模块，
//! 使自动路径和托盘里的手动轻量模式保持相同语义。

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::Manager;

type TimerTask = tauri::async_runtime::JoinHandle<()>;

struct TimerState<T> {
    generation: u64,
    task: Option<T>,
}

struct TimerController<T> {
    state: Mutex<TimerState<T>>,
}

impl<T> TimerController<T> {
    const fn new() -> Self {
        Self {
            state: Mutex::new(TimerState {
                generation: 0,
                task: None,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, TimerState<T>> {
        self.state.lock().unwrap_or_else(|poisoned| {
            log::warn!("自动轻量模式计时器状态锁曾发生异常，正在恢复其状态");
            poisoned.into_inner()
        })
    }

    /// 开启新一代，同时取走此前任务。调用方必须在锁外终止旧任务。
    fn begin_generation(&self) -> (u64, Option<T>) {
        let mut state = self.lock();
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        let previous_task = state.task.take();
        (generation, previous_task)
    }

    /// 将新任务安装到预留的代际。若安装前已发生取消或替换，则把任务
    /// 退还给调用方终止。
    fn install(&self, generation: u64, task: T) -> Result<(), T> {
        let mut state = self.lock();
        if state.generation != generation {
            return Err(task);
        }

        state.task = Some(task);
        Ok(())
    }

    /// 先占有到期任务的执行权，再进行有副作用的窗口销毁。
    /// 返回值是占有后的新代号；同代的第二个完成回调无法再次占有。
    fn claim(&self, generation: u64) -> Option<u64> {
        let mut state = self.lock();
        if state.generation != generation {
            return None;
        }

        let claimed = generation.wrapping_add(1);
        state.generation = claimed;

        // 当前任务正在执行，丢弃它自己的 JoinHandle 只会解除关联，不会
        // 中止任务；同时确保控制器不会长期保留已完成任务的句柄。
        let completed_task = state.task.take();
        drop(state);
        drop(completed_task);
        Some(claimed)
    }

    fn is_current(&self, generation: u64) -> bool {
        self.lock().generation == generation
    }
}

static TIMER: TimerController<TimerTask> = TimerController::new();

fn abort_task(task: Option<TimerTask>) {
    if let Some(task) = task {
        task.abort();
    }
}

/// 窗口成功隐藏（或启动时确认保持隐藏）后启动一次性倒计时。
pub(crate) fn schedule_after_hidden(app: &tauri::AppHandle, reason: &'static str) {
    // 即使设置关闭也推进代号并终止旧任务，确保上一轮不会越过新的隐藏事件。
    let (generation, previous_task) = TIMER.begin_generation();
    abort_task(previous_task);

    let config = crate::settings::get_settings().auto_lightweight;
    if !config.enabled {
        return;
    }
    let minutes = config.after_minutes;

    let delay = Duration::from_secs(u64::from(minutes) * 60);
    let app_handle = app.clone();
    log::debug!("已安排自动轻量模式: reason={reason}, generation={generation}, minutes={minutes}");

    let task = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;

        // 先取得本轮执行权并清理任务句柄。即使随后排入主线程的回调迟到，
        // 任何显示窗口、修改设置或新一轮计时仍会推进代号并阻止它执行。
        let Some(claimed_generation) = TIMER.claim(generation) else {
            log::debug!("忽略已失效的自动轻量模式任务: generation={generation}");
            return;
        };

        // WebView 的显示/销毁与托盘唤回事件统一回到主线程串行处理，避免
        // 到期回调和“刚刚重新打开窗口”在两个线程上同时操作同一窗口。
        let callback_app = app_handle.clone();
        if let Err(error) = app_handle.run_on_main_thread(move || {
            complete_if_still_hidden(&callback_app, generation, claimed_generation, minutes);
        }) {
            log::warn!("自动轻量模式回调无法切换到主线程: generation={generation}, error={error}");
        }
    });

    if let Err(stale_task) = TIMER.install(generation, task) {
        // 任务创建与句柄安装之间可能恰好发生取消或替换；这种情况下新任务
        // 从未成为当前任务，必须立即终止。
        stale_task.abort();
        log::debug!("自动轻量模式任务在安装前已失效: generation={generation}");
    }
}

/// 使当前及更早的待执行任务失效。
pub(crate) fn cancel_pending(reason: &'static str) {
    let (generation, task) = TIMER.begin_generation();
    abort_task(task);
    log::debug!("已取消待执行的自动轻量模式: reason={reason}, generation={generation}");
}

/// 设置保存成功后的运行时协调：关闭时立即取消；若通过非 UI 路径在隐藏状态
/// 开启或修改时长，则从设置生效时重新计时。
pub(crate) fn reconcile_settings(
    app: &tauri::AppHandle,
    previous: crate::settings::AutoLightweightSettings,
    current: crate::settings::AutoLightweightSettings,
) {
    if previous == current {
        return;
    }

    cancel_pending("settings-changed");
    if !current.enabled || crate::lightweight::is_lightweight_mode() {
        return;
    }

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    match window.is_visible() {
        Ok(false) => schedule_after_hidden(app, "settings-enabled-while-hidden"),
        Ok(true) => {}
        Err(error) => log::warn!("更新自动轻量模式设置时读取主窗口可见性失败: {error}"),
    }
}

fn complete_if_still_hidden(
    app: &tauri::AppHandle,
    generation: u64,
    claimed_generation: u64,
    scheduled_minutes: u32,
) {
    let settings = crate::settings::get_settings();
    if !settings.auto_lightweight.enabled
        || settings.auto_lightweight.after_minutes != scheduled_minutes
        || crate::lightweight::is_lightweight_mode()
    {
        return;
    }

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    match window.is_visible() {
        Ok(false) => {}
        Ok(true) => {
            log::debug!("自动轻量模式到期时主窗口已显示，跳过: generation={generation}");
            return;
        }
        Err(error) => {
            log::warn!("自动轻量模式到期时读取主窗口可见性失败，已跳过: {error}");
            return;
        }
    }

    // 最后一刻再次确认没有来自其它线程的设置变更或取消请求。窗口显示路径
    // 本身在主线程执行，因此不会越过本检查与下面的同步销毁操作。
    if !TIMER.is_current(claimed_generation) {
        return;
    }

    if let Err(error) = crate::lightweight::enter_lightweight_mode(app) {
        // 不自动重试：失败时保留当前窗口与非轻量状态，让用户仍可从托盘恢复，
        // 下一次明确的隐藏事件再建立新一轮计时。
        log::error!("自动进入轻量模式失败: {error}");
    } else {
        log::info!("主窗口隐藏超过 {scheduled_minutes} 分钟，已自动进入轻量模式");
    }
}

#[cfg(test)]
mod tests {
    use super::TimerController;

    #[test]
    fn cancellation_invalidates_an_older_generation_and_is_idempotent() {
        let controller = TimerController::<u8>::new();
        let (scheduled, previous) = controller.begin_generation();
        assert!(previous.is_none());
        controller.install(scheduled, 1).unwrap();

        let (_cancelled, cancelled_task) = controller.begin_generation();

        assert_eq!(cancelled_task, Some(1));
        assert!(controller.claim(scheduled).is_none());
        let (_, repeated_cancellation) = controller.begin_generation();
        assert!(repeated_cancellation.is_none());
        assert_eq!(controller.install(scheduled, 7), Err(7));
    }

    #[test]
    fn completion_is_claimed_once_and_invalidated_by_a_new_generation() {
        let controller = TimerController::<u8>::new();
        let (scheduled, _) = controller.begin_generation();
        controller.install(scheduled, 1).unwrap();
        let claimed = controller
            .claim(scheduled)
            .expect("first completion should win");

        assert!(controller.is_current(claimed));
        assert!(controller.claim(scheduled).is_none());

        let (_, completed_task) = controller.begin_generation();
        assert!(completed_task.is_none());
        assert!(!controller.is_current(claimed));
    }

    #[tokio::test]
    async fn cancelling_a_generation_allows_its_running_task_to_be_aborted() {
        let controller = TimerController::<tokio::task::JoinHandle<()>>::new();
        let (scheduled, _) = controller.begin_generation();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });
        started_rx.await.expect("task should start");
        controller.install(scheduled, task).unwrap();

        let (_cancelled, task) = controller.begin_generation();
        let task = task.expect("cancellation should take the running task");
        task.abort();
        assert!(task
            .await
            .expect_err("task should be aborted")
            .is_cancelled());
    }
}
