# PRD: 统一供应商 API 配置同步

## Goal

在统一供应商编辑页提供显式的 API 配置更新操作，避免子行编辑反向修改统一供应商。

## Acceptance

- 子行保存不再回写统一供应商。
- 普通统一供应商编辑页显示“更新 API 配置”按钮，聚合代理不显示。
- 操作只更新已有子行的 API 地址和认证字段，保留模型及其它配置。
- 四种界面语言都有对应文案。

## Scope

包含 Rust service/command、前端 API/UI 和回归验证；不改变完整同步和聚合代理路由表保存行为。

## Repository rules

- `CONTRIBUTING.md`: 使用 Conventional Commits、运行 TypeScript/Rust 检查、更新三种语言文案。
- `SECURITY.md`: API key 属于凭据，不能进入日志或共享配置。
- `LICENSE`: MIT。
