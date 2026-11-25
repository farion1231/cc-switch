use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn request_cancellation_handles_early_duplicate_and_finished_requests() {
    let mut requests = SearchRequests::default();
    requests.cancel("early");
    requests.cancel("early");
    assert_eq!(requests.cancelled.len(), 1);
    assert!(requests.begin("early").is_cancelled());
    assert!(requests.cancelled.is_empty());
    let other = requests.begin("other");
    requests.cancel("early");
    assert!(!other.is_cancelled());
    for i in 0..MAX_EARLY_CANCELLATIONS + 1 {
        requests.cancel(&format!("finished-{i}"));
    }
    assert_eq!(requests.cancelled.len(), MAX_EARLY_CANCELLATIONS);
    assert!(!requests.begin("finished-0").is_cancelled());
    assert!(requests.begin("finished-1").is_cancelled());

    let id = uuid::Uuid::new_v4().to_string();
    let first = begin_search(id.clone());
    let replacement = begin_search(id.clone());
    assert!(first.task.is_cancelled());
    drop(first);
    cancel_search(&id);
    assert!(replacement.task.is_cancelled());
    drop(replacement);
    assert!(!SEARCH_REQUESTS.lock_recover().active.contains_key(&id));
}

#[test]
fn early_cancellation_does_not_open_the_index_or_return_empty_hits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(INDEX_FILE);
    let task = SearchTask::default();
    task.cancel();
    assert_eq!(
        search_in_with_task(&Mutex::new(None), &path, || true, "的", None, 10, &task).unwrap_err(),
        SEARCH_CANCELLED
    );
    assert!(!path.exists());
}

#[test]
fn cancellation_interrupts_sql_and_cleans_up_its_handler() {
    let index = SessionIndex::open_in_memory().unwrap();
    let task = SearchTask::default();
    let sql = "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL
               SELECT n + 1 FROM numbers WHERE n < 10000) SELECT sum(n) FROM numbers";
    let result = task.run(&index.conn, || {
        task.cancel();
        let error = index
            .conn
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .expect_err("the progress handler must interrupt SQL before it completes");
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::OperationInterrupted)
        );
        Err::<i64, _>(error)
    });
    assert_eq!(result.unwrap(), None);
    // The old flag is still set. A leaked progress handler would interrupt this too.
    assert_eq!(
        index
            .conn
            .query_row(sql, [], |row| row.get::<_, i64>(0))
            .unwrap(),
        50_005_000
    );
}

#[test]
fn cancellation_is_checked_before_snippet_generation_without_a_progress_handler() {
    let mut index = SessionIndex::open_in_memory().unwrap();
    index
        .replace_source(
            "codex",
            "/snippet",
            fp(1),
            &rows(&["的".repeat(MAX_ROW_CHARS).as_str()]),
        )
        .unwrap();
    let task = Arc::new(SearchTask::default());
    let cancel = Arc::clone(&task);
    // Cancel when the snippet statement is prepared, after matching/aggregation.
    // No progress handler is installed: only the Rust-side checks can stop this.
    index.conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "message_text",
                column_name: "text"
            }
        ) {
            cancel.cancel();
        }
        Authorization::Allow
    }));
    let error = index.search_cancellable("的", None, 10, &task).unwrap_err();
    assert!(task.is_cancelled());
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::OperationInterrupted)
    );
}

#[test]
fn cancellation_while_waiting_for_index_initialization_is_observed_after_locking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(INDEX_FILE);
    let slot = Mutex::new(None);
    let task = SearchTask::default();
    let guard = slot.lock_recover();
    let (started, receive) = mpsc::channel();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            search_in_with_task(
                &slot,
                &path,
                || {
                    started.send(()).unwrap();
                    true
                },
                "的",
                None,
                10,
                &task,
            )
        });
        receive.recv().unwrap();
        task.cancel();
        drop(guard);
        assert_eq!(worker.join().unwrap().unwrap_err(), SEARCH_CANCELLED);
    });
}

#[test]
fn concurrent_cancellation_does_not_hide_index_open_or_sql_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(INDEX_FILE);
    fs::create_dir(&path).unwrap();
    let task = SearchTask::default();
    let checks = std::sync::atomic::AtomicUsize::new(0);
    let error = search_in_with_task(
        &Mutex::new(None),
        &path,
        || {
            // Cancel only after the initial check, just before the open fails.
            if checks.fetch_add(1, Ordering::Relaxed) == 1 {
                task.cancel();
            }
            true
        },
        "的",
        None,
        10,
        &task,
    )
    .unwrap_err();
    assert!(task.is_cancelled());
    assert!(!error.is_empty());
    assert_ne!(error, SEARCH_CANCELLED);

    let index = SessionIndex::open_in_memory().unwrap();
    let task = SearchTask::default();
    let error = task
        .run(&index.conn, || {
            task.cancel();
            index.conn.execute("SELECT * FROM missing_search_table", [])
        })
        .unwrap_err();
    assert!(error.to_string().contains("missing_search_table"));
    // Even SQLITE_INTERRUPT is not cancellation unless this request was cancelled.
    let task = SearchTask::default();
    assert!(task
        .run::<()>(&index.conn, || Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_INTERRUPT),
            None
        )))
        .is_err());
}

#[test]
fn search_still_reports_sql_failures_instead_of_empty_hits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(INDEX_FILE);
    let index = SessionIndex::open(&path).unwrap();
    index
        .conn
        .execute_batch("DROP TABLE indexed_sources")
        .unwrap();
    let error = search_in(&Mutex::new(Some(index)), &path, || true, "的", None, 10).unwrap_err();
    assert!(error.contains("indexed_sources"));
    assert_ne!(error, SEARCH_CANCELLED);
}

#[derive(Clone, Copy, Debug)]
enum BenchMode {
    Baseline,
    Progress,
    ProgressAndInterrupt,
}

// InterruptHandle exists only in this manual comparison, not the implementation.
#[derive(Default)]
struct BenchTask {
    search: SearchTask,
    interrupt: Mutex<Option<rusqlite::InterruptHandle>>,
}

impl BenchTask {
    fn cancel(&self, mode: BenchMode) {
        if !matches!(mode, BenchMode::Baseline) {
            self.search.cancel();
        }
        if matches!(mode, BenchMode::ProgressAndInterrupt) {
            if let Some(handle) = self.interrupt.lock_recover().as_ref() {
                handle.interrupt();
            }
        }
    }

    fn run(&self, index: &SessionIndex, query: &str, mode: BenchMode) -> Option<Vec<ContentHit>> {
        if matches!(mode, BenchMode::ProgressAndInterrupt) {
            *self.interrupt.lock_recover() = Some(index.conn.get_interrupt_handle());
        }
        let result = match mode {
            BenchMode::Baseline => index.search(query, None, DEFAULT_LIMIT).map(Some),
            _ => self.search.run(&index.conn, || {
                index.search_cancellable(query, None, DEFAULT_LIMIT, &self.search)
            }),
        };
        self.interrupt.lock_recover().take();
        result.unwrap()
    }
}

fn percentile_ms(samples: &[Duration], percentile: usize) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort();
    sorted[(sorted.len() * percentile).div_ceil(100).saturating_sub(1)].as_secs_f64() * 1000.0
}

/// Manual, deterministic dense-hit workload; no machine-dependent speed assertions.
/// Run: cargo test --lib benchmark_search_cancellation -- --ignored --nocapture
/// Uses the current cargo test profile, a temporary WAL database and warm page cache.
/// Times the Rust search/concurrent readers after a cancellation signal, excluding
/// webview IPC transport and the frontend's 250 ms debounce. Rapid dispatch at 20 ms
/// deliberately stresses the backend more than ordinary debounced typing.
#[test]
#[ignore = "manual dense single-character / rapid-input cancellation benchmark"]
fn benchmark_search_cancellation() {
    const SOURCES: usize = 240;
    const ROWS_PER_SOURCE: usize = 100;
    const SAMPLES: usize = 12;
    const CANCEL_AFTER: Duration = Duration::from_millis(5);
    const INPUT_INTERVAL: Duration = Duration::from_millis(20);
    const TYPING: [&str; 10] = [
        "的",
        "的会",
        "的会话",
        "的会话搜",
        "的会话搜索",
        "的",
        "的会",
        "的会话",
        "的会话搜",
        "的会话搜索",
    ];
    const MODES: [BenchMode; 3] = [
        BenchMode::Baseline,
        BenchMode::Progress,
        BenchMode::ProgressAndInterrupt,
    ];

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(INDEX_FILE);
    let mut index = SessionIndex::open(&path).unwrap();
    let text = truncate_chars(
        &"a cancellation benchmark 的会话搜索正文 repeated dense matches. ".repeat(40),
        1024,
    );
    let fixture: Vec<_> = (0..ROWS_PER_SOURCE).map(|i| (i, text.clone())).collect();
    let build = Instant::now();
    for source in 0..SOURCES {
        index
            .replace_source("codex", &format!("/benchmark/{source}"), fp(1), &fixture)
            .unwrap();
    }
    index.compact().unwrap();
    println!("fixture: {SOURCES} sessions x {ROWS_PER_SOURCE} rows x {} chars; {} dense matching rows; cap={MAX_HIT_ROWS}; limit={DEFAULT_LIMIT}; progress_ops={SEARCH_PROGRESS_OPS}; samples={SAMPLES}; build={:.3}s; debug_assertions={}",
        text.chars().count(), SOURCES * ROWS_PER_SOURCE, build.elapsed().as_secs_f64(), cfg!(debug_assertions));
    drop(index);
    let mut index = SessionIndex::open_reader(&path).unwrap();

    for query in ["a", "的"] {
        let expected = index.search(query, None, DEFAULT_LIMIT).unwrap();
        assert_eq!(expected.len(), DEFAULT_LIMIT);
        assert_eq!(
            expected.iter().map(|hit| hit.snippets.len()).sum::<usize>(),
            DEFAULT_LIMIT * MAX_SNIPPETS_PER_SESSION
        );
        // Rotate mode order on every sample to reduce warm-cache/order bias.
        let mut durations: [Vec<Duration>; 3] = Default::default();
        for sample in 0..SAMPLES {
            for offset in 0..MODES.len() {
                let arm = (sample + offset) % MODES.len();
                let task = BenchTask::default();
                let start = Instant::now();
                let hits = task.run(&index, query, MODES[arm]).unwrap();
                durations[arm].push(start.elapsed());
                assert_eq!(hits, expected);
            }
        }
        for (arm, mode) in MODES.iter().enumerate() {
            println!("complete query={query:?} mode={mode:?}: p50={:.3}ms p95={:.3}ms throughput={:.3} completed/s",
                percentile_ms(&durations[arm], 50), percentile_ms(&durations[arm], 95),
                SAMPLES as f64 / durations[arm].iter().sum::<Duration>().as_secs_f64());
        }

        for phase in ["sql", "snippets"] {
            let mut latencies: [Vec<Duration>; 3] = Default::default();
            let mut cancelled = [0; 3];
            for sample in 0..SAMPLES {
                for offset in 0..MODES.len() {
                    let arm = (sample + offset) % MODES.len();
                    let mode = MODES[arm];
                    let task = BenchTask::default();
                    let (ready, receive) = mpsc::sync_channel(0);
                    // Move the connection to the worker (Connection is Send, not Sync).
                    let (returned, latency, stopped) = std::thread::scope(|scope| {
                        let task_ref = &task;
                        let worker = scope.spawn(move || {
                            if phase == "snippets" {
                                let mut ready = Some(ready);
                                index.conn.authorizer(Some(move |context: AuthContext<'_>| {
                                    if matches!(
                                        context.action,
                                        AuthAction::Read {
                                            table_name: "message_text",
                                            column_name: "text"
                                        }
                                    ) {
                                        if let Some(ready) = ready.take() {
                                            ready.send(()).unwrap();
                                        }
                                    }
                                    Authorization::Allow
                                }));
                            } else {
                                ready.send(()).unwrap();
                            }
                            let result = task_ref.run(&index, query, mode);
                            let stopped = result.is_none();
                            drop(result);
                            index
                                .conn
                                .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
                            (index, Instant::now(), stopped)
                        });
                        receive.recv().unwrap();
                        std::thread::sleep(CANCEL_AFTER);
                        let cancel_at = Instant::now();
                        task.cancel(mode);
                        let (index, finished, stopped) = worker.join().unwrap();
                        (
                            index,
                            finished.saturating_duration_since(cancel_at),
                            stopped,
                        )
                    });
                    index = returned;
                    latencies[arm].push(latency);
                    cancelled[arm] += usize::from(stopped);
                }
            }
            for (arm, mode) in MODES.iter().enumerate() {
                println!("cancel query={query:?} phase={phase} after={}ms mode={mode:?}: p50={:.3}ms p95={:.3}ms max={:.3}ms cancelled={}/{SAMPLES}",
                    CANCEL_AFTER.as_millis(), percentile_ms(&latencies[arm], 50), percentile_ms(&latencies[arm], 95),
                    percentile_ms(&latencies[arm], 100), cancelled[arm]);
            }
        }
    }

    // Match the independent WAL-reader implementation: each dispatch launches
    // its own reader while previous searches may still be running. Cancel the
    // predecessor on each input; the final request must complete in every arm.
    for mode in MODES {
        let mut elapsed_samples = Vec::new();
        let mut latest_samples = Vec::new();
        let mut work_samples = Vec::new();
        let mut cancelled_total = 0;
        for _ in 0..3 {
            let start = Instant::now();
            let (elapsed, latest, work, cancelled) = std::thread::scope(|scope| {
                let mut workers = Vec::new();
                let mut previous: Option<Arc<BenchTask>> = None;
                for (i, query) in TYPING.iter().enumerate() {
                    if i > 0 {
                        let deadline = start + INPUT_INTERVAL * i as u32;
                        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                    }
                    if let Some(task) = previous.take() {
                        task.cancel(mode);
                    }
                    let task = Arc::new(BenchTask::default());
                    previous = Some(Arc::clone(&task));
                    let submitted = Instant::now();
                    let db_path = &path;
                    workers.push(scope.spawn(move || {
                        let started = Instant::now();
                        let reader = SessionIndex::open_reader(db_path).unwrap();
                        let result = task.run(&reader, query, mode);
                        if i == TYPING.len() - 1 {
                            assert_eq!(result.as_ref().unwrap().len(), DEFAULT_LIMIT);
                        }
                        let stopped = result.is_none();
                        drop(result);
                        drop(reader);
                        (submitted.elapsed(), started.elapsed(), stopped)
                    }));
                }
                let mut latest = Duration::ZERO;
                let mut work = Duration::ZERO;
                let mut cancelled = 0;
                for worker in workers {
                    let (latency, duration, stopped) = worker.join().unwrap();
                    latest = latency;
                    work += duration;
                    cancelled += usize::from(stopped);
                }
                (start.elapsed(), latest, work, cancelled)
            });
            elapsed_samples.push(elapsed);
            latest_samples.push(latest);
            work_samples.push(work);
            cancelled_total += cancelled;
        }
        println!("typing mode={mode:?}: {} independent readers x {}ms, 3 rounds; drain_p50={:.3}ms latest_p50={:.3}ms summed_worker_p50={:.3}ms throughput={:.3} settled/s cancelled={}/{}",
            TYPING.len(), INPUT_INTERVAL.as_millis(), percentile_ms(&elapsed_samples, 50),
            percentile_ms(&latest_samples, 50), percentile_ms(&work_samples, 50),
            (3 * TYPING.len()) as f64 / elapsed_samples.iter().sum::<Duration>().as_secs_f64(),
            cancelled_total, 3 * TYPING.len());
    }
}
