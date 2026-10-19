# XSEC Agent Shield

面向 AI Agent 的本地优先安全防护与可逆数据脱敏平台。

Local-first security and reversible data redaction for AI agents.

本项目以 CC Switch 的 Tauri、React、Rust 和本地代理能力为基础，计划增加请求脱敏、作用域令牌、加密映射、响应恢复及严格失败策略。

**当前状态：仓库整理与开发规划阶段。安全能力尚未实现或验证；保留的界面、包名、图标、更新源和数据目录仍属于上游技术基线。现有代理可用不代表已提供敏感信息保护。**

## 开发文档

- [项目总结与需求基线](docs/xsec-agent-shield-project-summary.md)
- [开发计划与阶段验收](docs/development-plan.md)
- [架构与接口设计](docs/architecture.md)
- [威胁模型与覆盖边界](docs/threat-model.md)
- [测试与验收策略](docs/testing.md)
- [仓库基线与清理记录](docs/repository-baseline.md)
- [来源与第三方审查状态](THIRD_PARTY_NOTICES.md)

## 本地检查

Node 版本见 `.node-version`，pnpm 版本见 `package.json#packageManager`，Rust 工具链见 `rust-toolchain.toml`。

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm build:renderer
pnpm test:unit
cargo fmt --check --manifest-path src-tauri/Cargo.toml
```

运行桌面应用会涉及真实的 Agent 配置与上游数据目录。完成开发计划 P0 的应用身份和数据隔离前，仅在独立测试账户或隔离环境启动 `pnpm dev`，不要用日常 Agent 配置进行验证。

## 保护范围

规划中的保护只适用于明确接入且经过验证的请求路径。Agent 直连、工具外网请求、独立子进程和不支持的多模态路径需要分别验证。MCP 是辅助入口，不能作为所有模型请求的统一拦截保证。

首期计划验证一个 Agent 的文本请求、JSON 响应和 SSE 流；详细支持矩阵以测试结果为准。目前没有已验收的受保护 Agent。

## 项目来源

本项目独立于 [CC Switch](https://github.com/farion1231/cc-switch)，本地基线提交为 `adc223af322adaeca878e82bf78323865b0db768`，上游版本标识为 `4.0.15`。来源记录以本地 Git 和代码为依据，未另行核实远端分支状态。

保留原始 [LICENSE](LICENSE) 和版权声明。依赖及第三方资源的逐项审查尚未完成，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
