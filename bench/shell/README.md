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

## Local results (2026-10-21)

Raw samples are in `results/baseline.json` (340da34d, before motion) and `results/final.json` (1e5dcdf6, final implementation). The earlier motion measurements remain in `results/animated.json`. All used 300 rows, 1280×720, the same in-app Chromium browser and normal motion.

| Metric (median of run p95 values) | Baseline, 4 runs | Animated, 3 runs |
| --- | ---: | ---: |
| Navigation frame interval | 17.5 ms | 17.4 ms |
| Resize frame interval | 17.3 ms | 17.3 ms |
| Navigation two-frame proxy | 51.35 ms | 43.2 ms |

Two baseline runs had stalls (47 and 10 navigation frames over 50 ms); their cause was not established. All samples are retained. The other two baseline navigation p95 intervals were 17.3 ms and the two-frame proxy was 37.8–40.7 ms. Final animated runs measured 17.2–17.4 ms and 35.9–45.3 ms respectively, with no >50 ms frames in either phase. This supports no observed regression in this fixture, **not** a speedup or a guarantee for native WebView/heavy real pages. Reduced-motion behavior is covered by a component test; no reduced-motion performance run was collected.

Manual pointer verification: expand from 200 to 273 px, drag inward to snap into the 84 px macOS rail, drag outward to restore 273 px. Keyboard, cancellation, interrupted dragging and remembered width are covered by interaction tests.
