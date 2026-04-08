# CLI Sandbox E2E Harness

这个目录提供一个完全独立的 CLI 沙箱 E2E harness，用来验证 `cc-switch` 的真实命令流程。

设计边界：

- 不加入仓库根 workspace
- 不依赖仓库内任何 Rust crate
- 不复用项目现有测试 helper
- 只通过 `cc-switch` 二进制、fake HOME、fixtures、本地 loopback mock server 交互

标准入口：

```bash
qa/cli-e2e/scripts/build-cli.sh
qa/cli-e2e/scripts/run-local.sh
cargo run --manifest-path /Users/eric8810/Code/cc-switch/qa/cli-e2e/Cargo.toml -- list
cargo run --manifest-path /Users/eric8810/Code/cc-switch/qa/cli-e2e/Cargo.toml -- doctor
cargo run --manifest-path /Users/eric8810/Code/cc-switch/qa/cli-e2e/Cargo.toml -- run <scenario>
cargo run --manifest-path /Users/eric8810/Code/cc-switch/qa/cli-e2e/Cargo.toml -- run-all
```

环境变量：

- `CC_SWITCH_E2E_BIN`
  - 指向已构建的 `cc-switch` 二进制
  - 默认回退到仓库根的 `target/debug/cc-switch`
- `CC_SWITCH_E2E_KEEP_ARTIFACTS=1`
  - 成功场景也保留 `.artifacts`
- `CC_SWITCH_E2E_FILTER`
  - `list` 和 `run-all` 时按名称子串过滤场景

失败时会在 `.artifacts/<scenario>/<timestamp>/` 下留下：

- `command.log`
- `stdout.txt`
- `stderr.txt`
- `sandbox-tree.txt`
- `live-config/`
- `mock-requests.json`
- `notes.md`
