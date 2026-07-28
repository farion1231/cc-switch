# Design

## Source of truth

- Status: Draft
- Last refreshed: 2026-07-03
- Primary product surfaces: main desktop app, system tray menu, menu-bar usage overview panel
- Evidence reviewed:
  - `README_ZH.md` product positioning and screenshots
  - `assets/screenshots/main-zh.png` current main provider-management UI
  - `src/components/usage/UsageDashboard.tsx` full usage dashboard
  - `src/components/usage/TrayUsagePanel.tsx` menu-bar usage overview
  - `/Users/getui/Documents/ajia1206/01-OpenSource/clone/tokenscope/docs/screenshot.png` menu-bar dashboard reference
  - `/Users/getui/Documents/ajia1206/01-OpenSource/clone/tokenscope/src/App.tsx` and `src/charts.tsx` compact panel information architecture

## Brand

- Personality: pragmatic, fast, developer-focused, trustworthy.
- Trust signals: clear active provider state, transparent usage/cost numbers, conservative system-native desktop behavior.
- Avoid: marketing hero layouts inside the app, decorative gradients, dense nested cards, unclear estimated cost labels, and dashboard-sized components inside menu-bar panels.

## Product goals

- Goals: make switching and monitoring multiple coding assistants simple; expose usage/cost at the point of need; keep system tray interactions fast.
- Non-goals: replace a billing system, recreate every full dashboard control inside the tray, or imitate another product pixel-for-pixel.
- Success signals: the user can scan today's usage, cost, cache mix, top app, and top model from the menu bar without opening the main window.

## Personas and jobs

- Primary personas: developers who run Claude Code, Codex, Gemini CLI, OpenCode, OpenClaw, or Hermes through multiple providers.
- User jobs: switch providers safely, confirm which tool is consuming tokens, spot unusual spend, compare recent usage by app/model.
- Key contexts of use: short menu-bar checks during active coding, full dashboard review when investigating spend or logs.

## Information architecture

- Primary navigation: assistant tabs, provider/tool actions, usage dashboard, tray quick access.
- Core routes/screens: provider list, account/config tools, usage dashboard, menu-bar usage overview.
- Content hierarchy: full app favors management and editing; tray overview favors current status, top metrics, and drill-down cues.

## Design principles

- Principle 1: Tray surfaces are glanceable status panels, not shrunken dashboard pages.
- Principle 2: Token/cost numbers must stay numerically precise enough to act on, with short labels that fit Chinese, English, Traditional Chinese, and Japanese.
- Tradeoffs: show fewer rows in the first viewport and rely on scrolling for details; prefer compact bars and dividers over bordered cards for menu-bar UI.

## Visual language

- Color: neutral app chrome; blue for app/tool activity, green for cache/cost/success, violet for output, amber for cache creation.
- Typography: system sans for labels; tabular numeric alignment for token, cost, count, and percentage values.
- Spacing/layout rhythm: 12-16 px panel padding, compact 4-8 px row gaps, dividers between sections instead of repeated outer boxes.
- Shape/radius/elevation: 8 px or smaller internal controls and row blocks; the outer popover may use the platform window radius.
- Motion: restrained state transitions only; no decorative animation in the tray overview.
- Imagery/iconography: lucide icons for actions; provider/app icons only where they aid identification.

## Components

- Existing components to reuse: `Button`, existing usage query hooks, app icon mapping, formatting helpers, i18n keys.
- New/changed components: tray overview hero, token split bar, compact metrics, trend mini-bars, app/model rank rows.
- Variants and states: loading, no usage, one trend point, long provider/model names, dark/light theme.
- Token/component ownership: keep tray-specific composition in `src/components/usage/TrayUsagePanel.tsx`; share only stable formatting helpers.

## Accessibility

- Target standard: keyboard-readable and screen-reader labeled controls for refresh, close, and range switching.
- Keyboard/focus behavior: all action controls must be real buttons with visible focus.
- Contrast/readability: muted labels must remain readable on light and dark backgrounds.
- Screen-reader semantics: title, range controls, and data rows should use semantic text rather than canvas-only charts.
- Reduced motion and sensory considerations: charts are static; no required animation.

## Responsive behavior

- Supported breakpoints/devices: fixed desktop popover window, plus renderer resilience for narrow widths.
- Layout adaptations: text truncates only on names, never on metric labels; charts cap single-bar width so one data point does not fill the chart.
- Touch/hover differences: hover titles are optional; visible labels and values remain complete enough without hover.

## Interaction states

- Loading: show placeholders while preserving layout height.
- Empty: show compact no-data rows instead of empty cards.
- Error: reuse existing query fallback behavior; do not add blocking modals in tray.
- Success: freshly loaded numbers replace placeholders without layout jump.
- Disabled: refresh/close remain available; range buttons reflect active state.
- Offline/slow network, if applicable: cached/stale query data may remain visible while refresh is pending.

## Content voice

- Tone: concise, operational, developer-facing.
- Terminology: use "来源/Sources" in tray, reserve "Provider 统计/Provider Stats" for the full dashboard.
- Microcopy rules: avoid long uppercase labels in compact panels; prefer "应用/Apps", "模型/Models", "命中/Cache", "成功/Success".

## Implementation constraints

- Framework/styling system: React, TypeScript, Tailwind, shadcn-style primitives, Tauri 2.
- Design-token constraints: use existing CSS variables and Tailwind palette; no new dependencies for tray visuals.
- Performance constraints: render SVG/CSS charts directly; avoid Recharts in the tray window.
- Compatibility constraints: keep the existing query hooks and Tauri window behavior; tray title remains separate from the webview panel.
- Test/screenshot expectations: run TypeScript and Rust checks after code changes; visually smoke-test the tray panel when possible.

## Open questions

- [ ] Whether the tray overview should eventually use a native transparent NSPanel on macOS / owner: maintainer / impact: higher visual fidelity to Tokenscope.
- [ ] Whether app/model rows should deep-link into filtered dashboard views / owner: maintainer / impact: faster investigation path.
