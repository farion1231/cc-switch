# Fork 状态跟踪

## 📌 Fork 概述

| 项目 | 信息 |
|------|------|
| **源仓库** | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) |
| **Fork 时间** | 2026-06-08 |
| **修复目标** | Codex + MiMo 多轮对话卡死问题 |
| **上游 PR** | [#3896](https://github.com/farion1231/cc-switch/pull/3896) |

## 🎯 修复列表

### 已修复

- [x] 多轮工具调用卡死（"三轮对话卡死"）
  - 原因：Responses API → Chat Completions 转换产生非法数据
  - 修复：自动合并连续 assistant 消息

- [x] 思维泄漏（thinking leak）
  - 原因：MiMo 返回的 `<think>` 标签碎片泄漏到输出
  - 修复：缓存短 delta，检测并剥离 `<think>` 标签碎片

- [x] 长推理被截断
  - 原因：两段推理间隔超过 120s 被误判为静默超时
  - 修复：流式空闲超时 120s → 300s

- [x] reasoning_content 未回传
  - 原因：多轮对话中 `reasoning_content` 丢失
  - 修复：完整透传 `reasoning_content` 字段

### 版本历史

| 版本 | 发布日期 | 状态 | 说明 |
|------|---------|------|------|
| v3.16.3 | 2026-06-10 | ✅ 最新 | 基于 v3.16.2，完整修复 |
| v3.16.1-mimo-fix | 2026-06-08 | ⚠️ 早期 | 早期修复版本 |

## 📊 PR 状态

- **上游 PR**：https://github.com/farion1231/cc-switch/pull/3896
- **状态**：⏳ 等待审核/合并
- **预期**：官方合并后将弃用此 Fork

## ⏰ 升级时间表

| 事件 | 时间 | 操作 |
|------|------|------|
| Fork 创建 | 2026-06-08 | — |
| v3.16.3 发布 | 2026-06-10 | — |
| PR 官方审核中 | — | 持续跟进 |
| **官方 v3.16.3+ 发布** | **预期 Q3** | 建议用户升级回官方版本 |

## 🔄 同步计划

### 与上游保持同步

```bash
# 添加上游远程（仅需一次）
git remote add upstream https://github.com/farion1231/cc-switch.git

# 定期同步
git fetch upstream
git rebase upstream/main
git push origin main
```

### 同步检查清单

- [ ] 每周检查上游是否有新提交
- [ ] 如有冲突，及时解决
- [ ] 更新 Fork 版本号
- [ ] 发布新的 Release

## 📈 用户数据

- **Fork Stars**：[查看实时数据](https://github.com/lssuzie/cc-switch/stargazers)
- **Fork 被引用数**：[查看网络](https://github.com/lssuzie/cc-switch/network)

## 📚 相关文档

- [FORK_README.md](../FORK_README.md) - Fork 项目简介
- [CONTRIBUTING.md](./CONTRIBUTING.md) - 贡献指南
- [官方 CONTRIBUTING.md](https://github.com/farion1231/cc-switch/blob/main/CONTRIBUTING.md) - 官方贡献指南

## 🙋 常见问题

### Q: 何时应该升级回官方版本？

**A**: 当以下任一条件满足时：
- 官方发布包含此修复的版本（v3.16.3+）
- 上游 PR #3896 被合并
- 应用内检查更新提示有新版本

### Q: 此 Fork 会继续维护吗？

**A**: 
- 在 PR 合并前：会及时修复反馈的问题
- PR 合并后：仓库标记为已弃用（archived），不再更新
- 建议用户提前计划升级时间

### Q: 可以提交其他功能吗？

**A**: 不建议。此 Fork 仅用于修复 MiMo 问题。其他功能请提交到 [官方仓库](https://github.com/farion1231/cc-switch)。

---

**最后更新**：2026-06-10

需要更新此文档？请 [创建 Issue](https://github.com/lssuzie/cc-switch/issues/new) 或 [提交 PR](https://github.com/lssuzie/cc-switch/pulls)。
