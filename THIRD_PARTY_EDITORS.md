# 第三方编辑器支持（实验性）

本分支在 **cc-switch v3.13.0** 的基础上，新增了一个「第三方编辑器安装探测」能力（当前仅实现 **Windows**）。

## 当前已支持探测

- Qoder
- Trae
- CodeBuddy

## 工作方式（Windows）

探测逻辑优先级：

1. **注册表卸载项**（HKCU/HKLM）
   - `DisplayName` 包含关键字（不区分大小写）
   - 优先取 `DisplayIcon` 解析出 exe 路径，其次使用 `InstallLocation + exe 名` 组合
2. **常见安装目录扫描**
   - `%LOCALAPPDATA%\\Programs\\...`
   - `%ProgramFiles%\\...`
   - `%ProgramFiles(x86)%\\...`

## 前端展示

设置 → 通用页新增「第三方编辑器」区块，展示：

- 是否检测到安装
- 检测到的 exe 路径（如有）
- 发现来源（registry/path）

## 后续可扩展方向

- 基于已探测到的 exe 路径，加入“用某编辑器打开目录/文件”等动作
- 将 Claude/Codex/Gemini 的插件/配置联动扩展到这些编辑器（如果它们的配置存储路径不同）

