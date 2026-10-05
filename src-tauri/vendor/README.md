# Tao Windows taskbar deadlock backport

`tao-0.34.6` is the exact published Tao crate locked by this repository, with
one Windows-only backport of [tauri-apps/tao#1264](https://github.com/tauri-apps/tao/pull/1264).
Its original Apache-2.0 license and SPDX metadata are retained.

Source archive: https://static.crates.io/crates/tao/tao-0.34.6.crate

Archive SHA256:
`6e06d52c379e63da659a483a958110bbde891695a0ecb53e48cc7786d5eda7bb`

The sole source change is in
`src/platform_impl/windows/event_loop.rs`, in the `S_U_TASKBAR_RESTART`
(`TaskbarCreated`) handler. Copy `skip_taskbar` and release `window_state` before
calling `set_skip_taskbar`. Explorer's COM call can pump messages and re-enter
the window procedure; retaining the non-reentrant lock across that call can
permanently block the UI thread. See [tao#1263](https://github.com/tauri-apps/tao/issues/1263).

The upstream fix is in the newer Tao release line, while Tauri's currently
locked dependency uses 0.34.x. This local Cargo patch keeps the remaining
dependency versions and all non-Windows platform implementations unchanged.
Remove the patch and this snapshot once Tauri resolves to a compatible Tao
version containing the fix.
