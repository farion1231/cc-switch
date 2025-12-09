# 前端测试开发计划

## 1. 背景与目标
- **背景**：v3.5.0 起前端功能快速扩张（供应商管理、MCP、导入导出、端点测速、国际化），缺失系统化测试导致回归风险与人工验证成本攀升。
- **目标**：在 3 个迭代内建立覆盖关键业务的自动化测试体系，形成稳定的手动冒烟流程，并将测试执行纳入 CI/CD。

## 2. 范围与优先级
| 范围 | 内容 | 优先级 |
| --- | --- | --- |
| 供应商管理 | 列表、排序、预设/自定义表单、切换、复制、删除 | P0 |
| 配置导入导出 | JSON 校验、备份、进度反馈、失败回滚 | P0 |
| MCP 管理 | 列表、启停、模板、命令校验 | P1 |
| 设置面板 | 主题/语言切换、目录设置、关于、更新检查 | P1 |
| 端点速度测试 & 使用脚本 | 启动测试、状态指示、脚本保存 | P2 |
| 国际化 | 中英切换、缺省文案回退 | P2 |

## 3. 测试分层策略
- **单元测试（Vitest）**：纯函数与 Hook（`useProviderActions`、`useSettingsForm`、`useDragSort`、`useImportExport` 等）验证数据处理、错误分支、排序逻辑。
- **组件测试（React Testing Library）**：关键组件（`ProviderList`、`AddProviderDialog`、`SettingsDialog`、`McpPanel`）模拟交互、校验、提示；结合 MSW 模拟 API。
- **集成测试（App 级别）**：挂载 `App.tsx`，覆盖应用切换、编辑模式、导入导出回调、语言切换，验证状态同步与 toast 提示。
- **端到端测试（Playwright）**：依赖 `pnpm dev:renderer`，串联供应商 CRUD、排序拖拽、MCP 启停、语言切换即时刷新、更新检查跳转。
- **手动冒烟**：Tauri 桌面包 + dev server 双通道，验证托盘、系统权限、真实文件写入。

## 4. 环境与工具
- 依赖：Node 18+、pnpm 8+、Vitest、React Testing Library、MSW、Playwright、Testing Library User Event、Playwright Trace Viewer。
- 配置要点：
  - 在 `tsconfig` 中共享别名，Vitest 配合 `vite.config.mts`。
  - `setupTests.ts` 统一注册 MSW/RTL、自定义 matcher。
  - Playwright 使用多浏览器矩阵（Chromium 必选，WebKit 可选），并共享 `.env.test`。
  - Mock `@tauri-apps/api` 与 `providersApi`/`settingsApi`，隔离 Rust 层。

## 5. 自动化建设里程碑
| 周期 | 目标 | 交付 |
| --- | --- | --- |
| Sprint 1 | Vitest 基础设施、核心 Hook 单测（P0） | `pnpm test:unit`、覆盖率报告、10+ 用例 |
| Sprint 2 | 组件/集成测试、MSW Mock 层 | `pnpm test:component`、App 主流程用例 |
| Sprint 3 | Playwright E2E、CI 接入 | `pnpm test:e2e`、CI job、冒烟脚本 |
| 持续 | 回归用例补齐、视觉比对探索 | Playwright Trace、截图基线 |

## 6. 用例规划概览
- **供应商管理**：新增（预设+自定义）、编辑校验、复制排序、切换失败回退、删除确认、使用脚本保存。
- **导入导出**：成功、重复导入、校验失败、备份失败提示、导入后托盘刷新。
- **MCP**：模板应用、协议切换（stdio/http）、命令校验、启停状态持久化。
- **设置**：主题/语言即时生效、目录路径更新、更新检查按钮外链、关于信息渲染。
- **端点速度测试**：触发测试、loading/成功/失败状态、指示器颜色、测速数据排序。
- **国际化**：默认中文、切换英文后主界面/对话框文案变化、缺失 key fallback。

## 7. 数据与 Mock 策略
- 在 `tests/fixtures/` 维护标准供应商、MCP、设置数据集。
- 使用 MSW 拦截 `providersApi`、`settingsApi`、`providersApi.onSwitched` 等调用；提供延迟/错误注入接口以覆盖异常分支。
- Playwright 端提供临时用户目录（`TMP_CC_SWITCH_HOME`）+ 伪配置文件，以验证真实文件交互路径。

## 8. 质量门禁与指标
- 覆盖率目标：单元 ≥75%，分支 ≥70%，逐步提升至 80%+。
- CI 阶段：`pnpm typecheck` → `pnpm format:check` → `pnpm test:unit` → `pnpm test:component` → `pnpm test:e2e`（可在 nightly 执行）。
- 缺陷处理：修复前补充最小复现测试；E2E 冒烟必须陪跑重大功能发布。

## 9. 工作流与职责
- **测试负责人**：前端工程师轮值；负责测试计划维护、PR 流水线健康。
- **开发者职责**：提交功能需附新增/更新测试、列出手动验证步骤、如涉及 UI 提交截图。
- **Code Review 检查**：测试覆盖说明、mock 合理性、易读性。

## 10. 风险与缓解
| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| Tauri API Mock 难度高 | 单测无法稳定 | 抽象 API 适配层 + MSW 统一模拟 |
| Playwright 运行时间长 | CI 变慢 | 拆分冒烟/完整版，冒烟只跑关键路径 |
| 国际化文案频繁变化 | 用例脆弱 | 优先断言 data-testid/结构，文案使用翻译 key |

## 11. 输出与维护
- 文档维护者：前端团队；每个版本更新后检查测试覆盖清单。
- 交付物：测试报告（CI artifact）、Playwright Trace、覆盖率摘要。
- 复盘：每次发布后召开 30 分钟测试复盘，记录缺陷、补齐用例。
