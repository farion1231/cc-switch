//! Test-only hooks (behind the `test-hooks` feature; zero cost in production builds).
//!
//! Two capabilities:
//! 1. **Programmable pause point** — lets a deterministic test interleave two
//!    threads at a chosen semantic point (instead of sleep/retry timing games),
//!    so the proxy-state atomicity contract can be exercised exactly at the
//!    reader/writer boundary.
//! 2. **Publish log** — records every published route-state generation together
//!    with the explicit-proxy value it was published with. Tests can verify that
//!    a reader's decision and generation correspond to one *real* published
//!    snapshot (never a torn combination of metadata + client).

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
struct GateInner {
    armed: bool,
    entered: u64,
}

pub struct PauseGate {
    state: Mutex<GateInner>,
    cv: Condvar,
}

impl PauseGate {
    /// Arm the gate and reset the "entered" counter.
    pub fn arm(&self) {
        *self.state.lock().unwrap() = GateInner {
            armed: true,
            entered: 0,
        };
    }

    /// Release all threads currently blocked in `pause_point`.
    pub fn release(&self) {
        let mut s = self.state.lock().unwrap();
        s.armed = false;
        self.cv.notify_all();
    }

    /// Block until at least one thread has entered `pause_point`.
    pub fn wait_entered(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = self.state.lock().unwrap();
        while s.entered == 0 {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, _) = self
                .cv
                .wait_timeout(s, deadline - now)
                .expect("condvar wait");
            s = guard;
        }
        true
    }
}

static PAUSE: OnceLock<Mutex<Option<std::sync::Arc<PauseGate>>>> = OnceLock::new();
static PUBLISH_LOG: OnceLock<Mutex<HashMap<u64, Option<String>>>> = OnceLock::new();

fn pause_cell() -> &'static Mutex<Option<std::sync::Arc<PauseGate>>> {
    PAUSE.get_or_init(|| Mutex::new(None))
}

fn log_cell() -> &'static Mutex<HashMap<u64, Option<String>>> {
    PUBLISH_LOG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Install / remove the active pause gate. `None` disables the pause point.
pub fn set_gate(gate: Option<std::sync::Arc<PauseGate>>) {
    *pause_cell().lock().unwrap() = gate;
}

pub fn new_gate() -> std::sync::Arc<PauseGate> {
    std::sync::Arc::new(PauseGate {
        state: Mutex::new(GateInner {
            armed: false,
            entered: 0,
        }),
        cv: Condvar::new(),
    })
}

/// The pause point itself. When a gate is installed and armed, this blocks
/// until the gate is released (or removed), after signalling `wait_entered`.
pub fn pause_point() {
    let gate = pause_cell().lock().unwrap().clone();
    if let Some(gate) = gate {
        let mut s = gate.state.lock().unwrap();
        s.entered += 1;
        gate.cv.notify_all();
        while s.armed {
            s = gate.cv.wait(s).expect("condvar wait");
        }
    }
}

/// Record a route-state publication (called by the writer inside its
/// critical section). `generation` must be unique per publication.
pub fn record_publish(generation: u64, explicit_proxy_url: Option<&str>) {
    log_cell()
        .lock()
        .unwrap()
        .insert(generation, explicit_proxy_url.map(|s| s.to_string()));
}

/// Look up what explicit-proxy value generation `generation` was published
/// with. `None` (the outer Option) means that generation was never published.
pub fn published_explicit_proxy(generation: u64) -> Option<Option<String>> {
    log_cell().lock().unwrap().get(&generation).cloned()
}

/// Clear the publish log between tests.
pub fn clear_publish_log() {
    log_cell().lock().unwrap().clear();
}

/// Records every route resolution performed by `resolve_route_for_url`:
/// `(generation, loopback_direct, url_is_loopback)`. Used by the concurrency
/// regression tests to prove that every decision is consistent with the snapshot
/// generation it actually consumed (never a torn metadata/client pairing).
static RESOLUTION_LOG: OnceLock<Mutex<Vec<(u64, bool, bool)>>> = OnceLock::new();

fn resolution_cell() -> &'static Mutex<Vec<(u64, bool, bool)>> {
    RESOLUTION_LOG.get_or_init(|| Mutex::new(Vec::new()))
}

/// Record a route resolution (called by `resolve_route_for_url`).
pub fn record_resolution(generation: u64, loopback_direct: bool, url_is_loopback: bool) {
    resolution_cell()
        .lock()
        .unwrap()
        .push((generation, loopback_direct, url_is_loopback));
}

/// Snapshot of all recorded resolutions.
pub fn resolutions() -> Vec<(u64, bool, bool)> {
    resolution_cell().lock().unwrap().clone()
}

/// Clear the resolution log between tests.
pub fn clear_resolution_log() {
    resolution_cell().lock().unwrap().clear();
}
