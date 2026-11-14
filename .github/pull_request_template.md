## Human Note / 本人说明

<!-- Required. In your own words, say what you want to do. No AI-generated text. -->
<!-- 必填。用你自己的话说清楚你想要做什么，不允许 AI 生成。 -->

## Summary / 概述

<!-- Briefly describe what this PR does and why. / 简要描述这个 PR 做了什么以及为什么。 -->

## Related Issue / 关联 Issue

<!-- Link the related issue. Use "Fixes #123" to auto-close it when merged. -->
<!-- 关联相关 Issue。使用 "Fixes #123" 可在合并时自动关闭。 -->

Fixes #

## Screenshots / 截图

<!-- If applicable, add before/after screenshots. / 如有需要，请添加修改前后的截图。 -->

| Before / 修改前 | After / 修改后 |
|-----------------|---------------|
|                 |               |

## Checklist / 检查清单

<!-- 逐项真实勾选：做过才勾，不适用写 N/A 及理由。 -->
<!-- Check each item only after actually running it; write N/A with a reason if not applicable. -->

- [ ] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [ ] `pnpm format:check` passes / 通过代码格式检查
- [ ] `pnpm test:unit` passes（如修改了前端代码）/ 通过前端单元测试（如修改了前端代码）
- [ ] `cargo fmt --check` passes（如修改了 Rust 代码）/ 通过格式检查（如修改了 Rust 代码）
- [ ] `cargo clippy -- -D warnings` passes（如修改了 Rust 代码；口径为全 workspace，勿只跑 `--lib`）/ 通过 Clippy 检查（如修改了 Rust 代码）
- [ ] `cargo test` passes（如修改了 Rust 代码）/ 通过 Rust 测试（如修改了 Rust 代码）
- [ ] Updated i18n files if user-facing text changed (zh / zh-TW / en / ja / tr required; ko / ru may fall back to en) / 如修改了用户可见文本，已更新国际化文件（zh / zh-TW / en / ja / tr 必须同步；ko / ru 可回退英文）
