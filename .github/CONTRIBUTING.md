# 为此 Fork 做贡献

感谢你的关注！本仓库是基于 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) 的社区补丁版本。

## 📍 仓库角色定位

- ✅ **针对 MiMo 多轮对话问题的临时修复**
- ✅ **等待上游合并** (PR: https://github.com/farion1231/cc-switch/pull/3896)
- ⚠️ **不接受新功能提交**（应提交给官方仓库）

## 🔄 贡献流程

### 如果是修复 MiMo 问题

1. Fork 此仓库
2. 创建特性分支 (`git checkout -b fix/your-issue`)
3. 提交更改 (`git commit -am 'Fix: description'`)
4. 推送到分支 (`git push origin fix/your-issue`)
5. 创建 Pull Request（详见下方 PR 指南）

### 如果是其他功能/问题

**请直接提交到官方仓库**：https://github.com/farion1231/cc-switch

## 📋 Pull Request 检查清单

提交 PR 前，请确保：

- [ ] PR 标题清晰说明修复内容
- [ ] PR 描述包含「为什么需要这个修复」
- [ ] 代码通过 `pnpm format:check` 和 `pnpm typecheck`
- [ ] 运行了 `pnpm test:unit` 并确保测试通过
- [ ] Rust 代码通过 `cargo fmt` 和 `cargo clippy` 检查
- [ ] 更新了相关文档和注释
- [ ] PR 标题标注修复的问题范围（如 `[MiMo] Fix ...`）

## 🧪 本地测试

### 前置条件

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### 开发流程

```bash
# 安装依赖
pnpm install

# 启动开发模式（热重载）
pnpm dev

# 类型检查
pnpm typecheck

# 代码格式检查
pnpm format:check

# 前端单元测试
pnpm test:unit

# Rust 后端测试
cd src-tauri
cargo test

# 构建生产版本
pnpm build
```

## 📝 代码规范

- 遵循原项目的代码风格
- TypeScript：使用 `pnpm format` 格式化
- Rust：使用 `cargo fmt` 格式化
- 提交信息使用英文或中文，保持清晰

## 🔗 相关链接

- **官方仓库**：https://github.com/farion1231/cc-switch
- **官方贡献指南**：https://github.com/farion1231/cc-switch/blob/main/CONTRIBUTING.md
- **此 Fork 的 PR**：https://github.com/farion1231/cc-switch/pull/3896

---

**感谢你的贡献！** 🎉
