# Shell performance benchmark

Run the production renderer fixture (no backend or network calls):

```sh
node node_modules/vite/bin/vite.js build --config bench/shell/vite.config.ts
node node_modules/vite/bin/vite.js preview --config bench/shell/vite.config.ts
```

Open http://localhost:3011 in a foreground browser, select 300 rows and run at least three times on each revision. Keep browser, viewport, display refresh rate, row count and power settings identical. Do not resize/minimize the browser during a run.

This renders the real Sidebar and PageFrame with deterministic rows, cached settings and stubbed sidebar status. Navigation includes application pages, Apps and Settings. The resize workload applies 180 widths at animation-frame boundaries; it measures layout cost, not pointer delivery. Verify actual pointer/keyboard resizing separately.

`frameMs` measures rAF intervals; `framesOver50ms` counts visible stalls. `inputToTwoFramesMs` measures state update to two subsequent rAF callbacks, a consistent paint-opportunity proxy, **not** measured input latency or animation completion. Network/IPC, real page data loading, GPU/compositor missed frames and native WebView behavior are outside this fixture.

Use median p95 across three runs. Investigate navigation/resize p95 frame regression above 2 ms or 20% (whichever is larger), navigation paint proxy regression above 8 ms, or more than two additional >50 ms frames. These are local comparison budgets, not hardware-independent guarantees. Save raw results alongside browser/viewport metadata; run again with reduced motion enabled. No telemetry or benchmark code is included in the application bundle.
