# CC Switch - MiMo 修复版（社区 Fork）

> 这是基于 [farion1231/cc-switch](https://github.com/farion1231/cc-switch) v3.16.2 的社区补丁版本

## 📍 快速导航

- **官方仓库**：https://github.com/farion1231/cc-switch
- **官方网站**：https://ccswitch.io
- **上游 PR**：https://github.com/farion1231/cc-switch/pull/3896

## 🔧 修复内容

本 Fork 针对 **Codex + MiMo 多轮对话卡死问题** 进行了修复。

### 修复了 4 个问题

1. **多轮工具调用卡死**（"三轮对话卡死"）
   - Codex → Chat Completions 转换产生非法数据
   - 自动合并连续 assistant 消息，空 content 改为空字符串

2. **思维泄漏**（thinking leak）
   - MiMo 返回的 `<think>` 标签碎片泄漏到输出
   - 缓存短 delta，检测并剥离 `<think>` 标签碎片

3. **长推理被截断**
   - 两段推理间隔超过 120s 被误判为静默超时
   - 流式空闲超时 120s → **300s**

4. **reasoning_content 未回传**
   - 多轮对话中 `reasoning_content` 丢失
   - 完整透传 `reasoning_content` 字段

## 📦 下载和使用

### 当前发布版本

- **v3.16.3**：最新修复版本，基于官方 v3.16.2
- **v3.16.1-mimo-fix**：早期修复版本

所有版本均支持 Windows、macOS、Linux 平台，见 [Releases](https://github.com/lssuzie/cc-switch/releases) 页面。

### 安装方式

1. 前往 [Releases](https://github.com/lssuzie/cc-switch/releases)
2. 选择对应平台的安装包下载
3. 按照官方方式安装（与官方版本相同）

## ⚠️ 重要提示

### 这是临时解决方案

- ✅ PR 已提交至上游：https://github.com/farion1231/cc-switch/pull/3896
- 📅 一旦上游合并，**建议升级到官方版本**
- 不推荐长期依赖此 Fork 版本

### 应用内更新提示

此 Fork 的应用内「检查更新」会指向本仓库。如需切换回官方版本的自动更新，请：

1. 卸载此版本
2. 从官方渠道重新安装
3. 按照官方登录流程重新配置

## 🔄 何时升级回官方版本

- ✅ 当原仓库发布 v3.16.3+ 版本时
- ✅ 当 PR #3896 被官方合并时
- 📧 关注此仓库的 [Releases](https://github.com/lssuzie/cc-switch/releases) 获取更新通知

## 🐛 问题反馈

- **此 Fork 的问题**：请在 [Issues](https://github.com/lssuzie/cc-switch/issues) 反馈
- **官方功能建议**：请到 [官方仓库](https://github.com/farion1231/cc-switch) 提交

## 📝 代码变更

```
修改文件数：6 个 Rust 文件
代码增删：+788 / −10 行

核心修改文件：
- src-tauri/src/core/providers/codex/transform_codex_chat.rs
- src-tauri/src/core/providers/codex/streaming_codex_chat.rs
- src-tauri/src/core/types.rs
```

详见各 Release 的 Changelog。

## 🙏 致谢

- 感谢 [farion1231](https://github.com/farion1231) 开发的优秀项目
- 感谢 MiMo 用户们的反馈

## 📄 许可证

MIT © Jason Young（原作者）

此 Fork 遵循原项目相同的 MIT 许可证。

---

**最后更新**：2026-06-10

如有疑问，欢迎 [开启 Issue](https://github.com/lssuzie/cc-switch/issues/new) 讨论。
