# feat: cross-client clipboard import/export and convert to universal provider

## Summary / 概述

为 CC Switch 添加跨客户端剪贴板导入导出功能，支持供应商、统一供应商、用量脚本三种配置类型通过系统剪贴板在不同客户端、不同机器之间迁移。同时新增「转换为统一供应商」快捷操作，一键将任意客户端的供应商配置转为统一供应商格式。

**主要功能：**
- 统一的剪贴板信封格式（`$ccSwitch` magic header + version），安全识别配置内容，避免误导入
- 支持导出/导入 **供应商配置**、**统一供应商**、**用量查询脚本**
- 跨客户端导入通过 UniversalProvider 中转，支持 Claude / Codex / Gemini 三者互转
- 供应商卡片新增「转换为统一供应商」快捷操作，一键预填统一供应商表单
- Linux 下剪贴板读写优先使用 `xclip`/`xsel`/`wl-copy`，修复 arboard 在 X11 下 selection ownership 丢失导致内容消失的问题
- 完整的多语言支持（zh / zh-TW / en / ja）

## Related Issue / 关联 Issue

Fixes #

## Details / 详细说明

### 剪贴板信封格式

所有通过剪贴板传递的配置都包裹在一个 JSON 信封中，带有识别头：

```json
{
  "$ccSwitch": true,
  "version": 1,
  "kind": "provider | universal-provider | usage-script",
  "appType": "codex",
  "provider": { "...": "..." }
}
```

- `$ccSwitch` magic 标记避免把任意剪贴板内容当作配置导入
- `version` 字段为未来格式升级预留空间
- 三种 `kind` 对应三种可移植配置类型

### 跨客户端导入流程

```
源 Provider (Claude) → UniversalProvider (中转) → 目标 Provider (Codex/Gemini)
```

- 同客户端导入：原样复制，重新生成 id
- 跨客户端导入：经 UniversalProvider 提取 base_url + api_key + 模型配置后，转为目标客户端格式
- 支持目标：**Claude / Codex / Gemini**（GrokBuild / OpenCode / OpenClaw / Hermes 暂不支持跨端导入，会给出明确错误提示）

### 转换为统一供应商

在任意客户端的供应商卡片上点击「转换为统一供应商」，自动提取 base_url、api_key、模型配置等信息，跳转到统一供应商面板并预填表单，用户确认后即可保存。

- 支持从 Claude / Codex / Gemini 提取模型配置
- 其他客户端（GrokBuild / OpenCode 等）仅提取凭据，默认启用 Claude 应用

### Linux 剪贴板兼容性修复

**问题**：`arboard` 库在 X11 下通过 X selection 管理剪贴板，当 `Clipboard` 实例被 drop（`spawn_blocking` 线程退出）后，selection owner 丢失，剪贴板内容立即消失。如果系统没有运行剪贴板管理器（如 copyq、clipmenud），用户会看到「复制了但粘贴不到」的现象。

**方案**：Linux 平台优先调用系统 CLI 工具写入/读取剪贴板（按 X11/Wayland 自动选择），失败再回退到 arboard：
- X11：`xclip` → `xsel` → `wl-copy`
- Wayland：`wl-copy` → `xclip` → `xsel`

`xclip`/`wl-copy` 会 fork 后台进程持续持有 selection，确保内容持久保留。

### UI 变更

- 供应商卡片「复制」按钮改为下拉菜单：复制 / 复制配置到剪贴板 / 转换为统一供应商
- 供应商列表顶部新增「从剪贴板导入」按钮
- 统一供应商卡片新增导出到剪贴板按钮
- 统一供应商面板新增「从剪贴板导入」和「从已有配置转换」入口
- 用量脚本弹窗新增导入/导出到剪贴板按钮
- 统一供应商表单支持从供应商配置一键转换预填

## Testing / 测试

- ✅ `pnpm typecheck` — TypeScript 类型检查通过
- ✅ `pnpm test:unit` — 695 个前端测试全部通过
- ✅ `cargo test` — 2345 个 Rust 测试全部通过
- ✅ `cargo clippy` — 无新增 warning
- ✅ `pnpm format:check` — Prettier 格式检查全部通过
- ✅ 新增 `from_provider` 及模型提取的单元测试（Claude / Codex / Gemini 往返转换验证）
- ✅ Linux X11 环境下独立实验验证：arboard drop 后内容丢失，xclip 写入持久保留

## Files Changed / 修改文件

- `src/lib/providerClipboard.ts` — 新增：剪贴板信封格式定义与导入导出逻辑
- `src/lib/clipboard.ts` — 新增 `readText` 函数（与 `copyText` 对称）
- `src/lib/api/providers.ts` — 新增 3 个转换相关 API 封装
- `src/App.tsx` — 新增导出/导入/转换回调，传递给 ProviderList
- `src/components/providers/ProviderActions.tsx` — 复制按钮改为下拉菜单，新增导出/转换选项
- `src/components/providers/ProviderList.tsx` — 新增「从剪贴板导入」按钮
- `src/components/providers/ProviderCard.tsx` — 透传 onExport / onConvertToUniversal
- `src/components/universal/UniversalProviderPanel.tsx` — 新增导出/导入/从供应商转换功能
- `src/components/universal/UniversalProviderCard.tsx` — 新增导出按钮
- `src/components/universal/UniversalProviderFormModal.tsx` — 支持 initialProvider 预填
- `src/components/UsageScriptModal.tsx` — 新增用量脚本导入/导出
- `src-tauri/src/commands/misc.rs` — Linux 剪贴板 xclip/xsel/wl-copy 兜底
- `src-tauri/src/commands/provider.rs` — 新增 3 个转换命令
- `src-tauri/src/provider.rs` — 新增 `from_provider` 方法与模型提取逻辑
- `src-tauri/src/lib.rs` — 注册新命令
- `src/i18n/locales/{zh,zh-TW,en,ja}.json` — 新增剪贴板相关文案

## Checklist / 检查清单

- [x] `pnpm typecheck` passes / 通过 TypeScript 类型检查
- [x] `pnpm format:check` passes / 通过代码格式检查
- [x] `cargo clippy` passes (if Rust code changed) / 通过 Clippy 检查（如修改了 Rust 代码）
- [x] Updated i18n files if user-facing text changed / 如修改了用户可见文本，已更新国际化文件
- [x] Added tests for new functionality / 新功能已添加测试
- [x] Backwards compatible / 保持向后兼容
