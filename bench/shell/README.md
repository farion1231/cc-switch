# Native shell performance benchmark

The benchmark script is `main.tsx`. It renders the real Sidebar and PageFrame with deterministic rows, cached settings and stubbed sidebar status. It does not import or change provider configuration. Use native Tauri for acceptance: macOS WKWebView and Windows WebView2 must be measured separately. A Chromium browser is only useful for developing the fixture.

## Run on macOS

Install the repository's normal frontend and Rust/Tauri prerequisites. Quit the existing CC Switch instance first: its single-instance handler would otherwise bring that window forward instead of opening the benchmark. From the repository root:

```sh
node node_modules/vite/bin/vite.js build --config bench/shell/vite.config.ts
node node_modules/@tauri-apps/cli/tauri.js dev --no-watch --config bench/shell/native.config.json
```

The config starts a local production-renderer preview on port 3011 and opens it in the actual Tauri shell. The Rust shell runs in development mode; use the same shell/build mode on both revisions. No benchmark code ships in the regular application bundle.

Keep the native window foreground and unobscured. Select 300 rows, click **Run benchmark**, and leave the window alone for approximately 20 seconds. Repeat at least three times per revision. Runs with `validForegroundRun: false` are invalid for comparison; keep them locally with the reason for exclusion. Focus checks cannot detect every form of OS occlusion or scheduling interference.

To compare with the pre-motion baseline, use frontend sources from commit `340da34d`, but copy the current `bench/shell/` harness/config into that checkout so measurement and foreground validation are identical. Keep the same native binary, machine, viewport, display refresh rate, power settings and row count. The local macOS run used the existing native preview app pointed at port 3000, serving each production fixture in turn and restarting the app between revisions. When reusing an HTTP server, avoid stale index caching when switching builds.

## What it measures

Navigation includes application pages, Apps and Settings. The resize workload applies 180 widths at animation-frame boundaries; it measures layout cost, not pointer delivery. Verify actual pointer/keyboard resizing and snapping separately in the real application.

- `frameMs`: rAF intervals, not compositor/GPU frame delivery.
- `framesOver50ms`: observed long frame intervals.
- `inputToTwoFramesMs`: state update to two subsequent rAF callbacks. This is a paint-opportunity proxy, **not** actual input latency or animation completion.
- `backgroundFrames`: samples where the document was hidden or unfocused.

Compare the median of per-run p95 values. Investigate navigation/resize p95 regression above 2 ms or 20% (whichever is larger), paint-proxy regression above 8 ms, or more than two additional >50 ms frames. These are local budgets, not hardware-independent guarantees. Repeat with reduced motion enabled if measuring that configuration.

The fixture isolates shell rendering. Real page loading, network/IPC latency and backend work are outside the measured workload; manually check real pages too. Record OS/WebView version, hardware, viewport, build mode and motion preference alongside each result. Keep raw data locally and list the comparison in the PR description; do not commit result files.

Windows should use the same harness and native config with the repository's Windows Tauri prerequisites. Compare before/after on that machine, rather than comparing absolute Windows numbers against macOS. Windows measurements have not yet been performed.
