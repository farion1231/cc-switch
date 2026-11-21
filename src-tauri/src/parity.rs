//! Test seams for the parity suite (parity/), compiled only with the `parity` feature.
//!
//! The page clock: when the harness pins the app's time (see [`crate::clock`]), every webview's
//! `Date` reads the same pinned instant. Each new webview starts at the instant pinned when it is
//! created, every page that finishes loading gets the current instant, and a change of the clock
//! file reaches every open webview within a poll interval. After a change of the time or of the
//! set of windows the instant is pushed again for a second, because a page still loading has no
//! `__parityClock` to receive it yet. Timers and `performance.now()` keep running in real time.

use std::time::Duration;
use tauri::plugin::Plugin;
use tauri::webview::{PageLoadEvent, PageLoadPayload, Webview};
use tauri::{AppHandle, Manager, Runtime};

const POLL: Duration = Duration::from_millis(50);
/// Polls to keep pushing after a change, for pages that were still loading when it happened.
const REPEAT: u32 = 20;

/// Replaces `Date` and the clock reads of `Intl.DateTimeFormat`. `window.__parityClock(ms)` moves
/// the page's time; the latest value survives a reload of the same webview.
const PAGE_CLOCK: &str = r#"
const Real = globalThis.Date;
const KEY = "__parityClock";
let pinned = __PINNED_MS__;
try {
  const kept = Number(sessionStorage.getItem(KEY));
  if (kept) pinned = kept;
} catch {}
function Date(...args) {
  if (!new.target) return new Real(pinned).toString();
  return Reflect.construct(Real, args.length ? args : [pinned], new.target);
}
Object.setPrototypeOf(Date, Real);
Date.prototype = Real.prototype;
Date.now = () => pinned;
Object.defineProperty(Real.prototype, "constructor", { value: Date, writable: true, configurable: true });
globalThis.Date = Date;
const dtf = Intl.DateTimeFormat.prototype;
const format = Object.getOwnPropertyDescriptor(dtf, "format").get;
Object.defineProperty(dtf, "format", {
  configurable: true,
  get() {
    const f = format.call(this);
    return (d) => f(d === undefined ? pinned : d);
  },
});
const formatToParts = dtf.formatToParts;
dtf.formatToParts = function (d) {
  return formatToParts.call(this, d === undefined ? pinned : d);
};
Object.defineProperty(globalThis, "__parityClock", {
  value: (ms) => {
    pinned = ms;
    try { sessionStorage.setItem(KEY, String(ms)); } catch {}
  },
});
"#;

pub struct ParityPlugin;

pub fn plugin() -> ParityPlugin {
    ParityPlugin
}

impl<R: Runtime> Plugin<R> for ParityPlugin {
    fn name(&self) -> &'static str {
        "parity"
    }

    /// Called for every webview as it is created, so each one starts at the current instant.
    fn initialization_script(&self) -> Option<String> {
        crate::clock::pinned::path()?;
        let ms = crate::clock::now().timestamp_millis();
        Some(PAGE_CLOCK.replace("__PINNED_MS__", &ms.to_string()))
    }

    /// A page that loaded after the instant changed started from the instant baked into its
    /// webview or kept in its sessionStorage; give it the current one.
    fn on_page_load(&mut self, webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
        if payload.event() != PageLoadEvent::Finished || crate::clock::pinned::path().is_none() {
            return;
        }
        let ms = crate::clock::now().timestamp_millis();
        let _ = webview.eval(format!(
            "window.__parityClock && window.__parityClock({ms})"
        ));
    }

    fn initialize(
        &mut self,
        app: &AppHandle<R>,
        _config: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if crate::clock::pinned::path().is_none() {
            return Ok(());
        }
        let app = app.clone();
        std::thread::Builder::new()
            .name("parity-clock".into())
            .spawn(move || {
                let labels = |app: &AppHandle<R>| {
                    let mut l: Vec<String> = app.webview_windows().into_keys().collect();
                    l.sort();
                    l
                };
                let mut sent = crate::clock::now().timestamp_millis();
                let mut windows = labels(&app);
                let mut repeat = 0;
                loop {
                    std::thread::sleep(POLL);
                    let ms = crate::clock::now().timestamp_millis();
                    let now_windows = labels(&app);
                    if ms != sent || now_windows != windows {
                        sent = ms;
                        windows = now_windows;
                        repeat = REPEAT;
                    }
                    if repeat == 0 {
                        continue;
                    }
                    repeat -= 1;
                    for webview in app.webview_windows().values() {
                        let _ = webview.eval(format!(
                            "window.__parityClock && window.__parityClock({ms})"
                        ));
                    }
                }
            })?;
        Ok(())
    }
}
