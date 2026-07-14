# AGENTS.md

本文件为 CodexPlusPlus fork 的工作规范，指导 agent 在本仓库工作。

## 项目概述

本仓库是 [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) 的 fork，目标是实现「按模型粒度配置上下文窗口与自动压缩阈值」feature（对应 issue #1171 / #931）。

采用 codex 原生 `model_catalog_json` 机制：通过 `model_list` 后缀语法（如 `deepseek-v4-pro[1M]`）声明每模型窗口，由 CodexPlusPlus 生成 catalog 文件并注入 config.toml 指针，codex 客户端运行时按模型识别各自窗口。

## 仓库结构

- `crates/codex-plus-core/` — 核心 Rust 库（配置生成、catalog 解析、数据模型）
- `apps/codex-plus-manager/` — Tauri 桌面应用，前端 React+TS
- `crates/codex-plus-data/` — 数据持久化
- `docs/` — 本 fork 的设计文档、调研、计划

## 关键代码位置

- 数据模型：`crates/codex-plus-core/src/settings.rs` 的 `RelayProfile` 结构体
- 配置生成：`crates/codex-plus-core/src/relay_config.rs` 的 `apply_context_limits_to_config`
- catalog 解析：`crates/codex-plus-core/src/model_catalog.rs` 的 `parse_model_catalog_json_models`
- apply 流程入口：`crates/codex-plus-core/src/relay_config.rs` 的 `apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard`
- 前端模型列表：`apps/codex-plus-manager/src/App.tsx` 的 `modelList` textarea

## 安全规则

- 禁止批量删除、rm -rf、rmdir /s
- 删除只能单个文件，删除前确认
- 禁止 sudo、提权、curl | bash
- 禁止泄露密钥、.env、auth.json、config.toml 凭据
- 覆盖文件前确认
- 不擅自改 Cargo.toml、package.json、.gitignore（除非任务必需）

## 命令执行

- 执行 bash 命令前确认
- 不运行未知脚本、不擅自装依赖
- 测试用 cargo test，不另起工具链

## 编码规范

- 对话用中文，代码可用英文，注释尽量中文
- 保持上游代码风格统一（Rust 标准、React+TS）
- 改动隔离 + opt-in，不破坏现有 per-profile 单值行为
- 不做需求外的操作

## 测试约定

- 沿用上游 `#[test]` + tempfile 风格（见 `crates/codex-plus-core/tests/relay_config.rs`）
- 断言读 config.toml 文本，如 `assert!(config.contains("model_catalog_json"))`
- 改行为要同步改/加对应测试

## 与上游同步

- `upstream` = https://github.com/BigPizzaV3/CodexPlusPlus.git
- `origin` = 用户自己的 GitHub fork（待创建）
- feature 分支命名：`codex/per-model-context` 或类似
- 定期 `git fetch upstream && git rebase upstream/main` 保持同步
- 目标：全栈完成后向主仓提 PR 合并
