# CLI 产品化阶段

当前状态：

- `core + CLI` 的业务能力主线已经补齐
- 剩余差距主要是壳层交互，不阻塞后续阶段
- 这一轮产品化主线已经落地完成

## 已完成项

1. `completions`
2. `install / update`
3. `诊断强化`

## 1. Completions

- 已新增 `cc-switch completions <shell>`
- 已支持 `bash / zsh / fish`
- `zsh / fish` 走标准生成器
- `bash` 使用仓库内稳定脚本生成，避开上游生成器在复杂命令树上的 panic
- 已新增 `cc-switch install completions <shell> [--dir ...]`
- 已补 CLI 黑盒测试和 `qa/cli-e2e` 场景

## 2. Install / Update

- 已新增 `cc-switch install guide`
- 已新增 `cc-switch update guide`
- 安装建议已经收口到可执行命令：
  - `cargo install --git ... cc-switch-cli --bin cc-switch --locked --force`
  - 本地源码 checkout 更新命令
- `about / update check / release-notes` 继续保留
- 当前没有做真正的二进制 `self-update`
  - 原因是当前发布链路仍以桌面端 bundle 为主，不存在稳定的独立 CLI 发布资产
  - 现阶段更可靠的方案是输出明确、可复制的安装与升级命令
- 已补 CLI 黑盒测试

## 3. 诊断强化

- 已新增 `DoctorService`
- 已新增 `cc-switch doctor`
- 当前聚合输出已覆盖：
  - tool detection
  - runtime / config / database / settings path
  - per-app current provider
  - live config path
  - env conflict
  - 常见 warning
  - 可选 update check
- 已补 CLI 黑盒测试和 `qa/cli-e2e` 场景

## 我们是否需要 TUI

当前结论：

- 现在不需要把 `TUI` 作为下一阶段主线
- 也不建议现在立刻开做

原因：

1. 当前最缺的不是“更炫的交互”，而是 CLI 产品化能力
2. `completions / install-update / diagnosis` 的收益更直接
3. TUI 会显著增加维护面和交互复杂度
4. 如果命令面和状态模型还在继续变化，TUI 很容易反复返工

## 什么时候再评估 TUI

只有当下面条件基本成立时，才建议重开 TUI 评估：

1. 命令面已经稳定
2. `install / update / completions / doctor` 已经齐
3. 我们确认真的需要一个“低门槛但强交互”的 CLI 入口
4. 只做一套主交互层，不同时维护多套 interactive 方案

## 如果将来做 TUI，边界应该是什么

建议边界：

- `core`
  - 只放业务能力和数据结构
- `CLI 命令层`
  - 继续保留脚本友好入口
- `TUI`
  - 只作为交互壳层
  - 承接：
    - 表单
    - 预览
    - 确认
    - 状态查看
  - 不承接新的业务逻辑

## 结论

这轮 CLI 产品化主线已经完成。后续如果继续推进，优先级应该转向：

1. 发布与分发链路是否要补独立 CLI 资产
2. 是否要为 `doctor` 增加更多自动修复建议
3. 等命令面稳定后，再重新评估 `TUI`
