# 会话管理（Session Manager）需求文档（PRD / Markdown）

> 目标：对 **Codex / Claude Code** 的本地会话记录进行可视化管理，并提供“一键复制 / 一键终端恢复”能力。
> 范围：**v1 仅 macOS**，但必须预留多平台扩展入口。

---

## 1. 背景与问题

开发者同时使用 Codex CLI、Claude Code 时，常见痛点：
- 会话记录落在本地不同位置，**难以发现/检索**
- 找到会话后，恢复命令需要记忆或翻历史，**恢复成本高**
- 恢复时经常忘了当时的工作目录，导致命令在错误目录运行
- 希望在常用终端（macOS Terminal、kitty 等）中直接恢复，提高效率

---

## 2. 目标与非目标

### 2.1 Goals（v1 必达）
1. 扫描并展示本机所有 Codex / Claude Code 会话：列表 + 详情（会话内容）
2. 支持恢复会话：
   - 复制恢复命令（按钮）
   - 复制会话目录（按钮，若能获取/推断）
   - 可选：直接在终端执行恢复（macOS Terminal、kitty；可扩展）
3. 仅 macOS 支持，但代码结构需支持未来扩展 Windows/Linux

### 2.2 Non-Goals（v1 不做）
- 不新增/依赖云端 API；默认不上传任何内容
- 不承诺解析所有 provider 的全部内部格式（尽量兼容、可配置、可降级）
- 不做复杂的团队协作/分享/同步（后续版本再考虑）

---

## 3. 用户画像与使用场景

### 3.1 典型用户
- 高频使用多个 AI 编程工具的工程师/技术负责人/PM
- 多项目、多分支并行，频繁“中断—恢复—继续推进”

### 3.2 核心场景（Top）
1. **找回会话**：我记得一个会话讨论过某段逻辑 → 搜索关键词 → 打开详情
2. **快速恢复**：我想继续昨天的会话 → 复制恢复命令 / 一键在终端恢复
3. **回到正确目录**：恢复前先复制目录或自动 cd 到目录

---

## 4. 产品形态与信息架构

### 4.1 信息架构
- Session Manager
  - 会话列表（List）
  - 会话详情（Detail）
  - 设置（Settings）
    - Provider 配置（路径/启用禁用）
    - 终端集成（默认终端、权限提示、降级策略）
    - 索引与隐私选项（是否缓存、缓存大小、敏感信息遮罩）

---

## 5. 功能需求（Functional Requirements）

### 5.1 会话发现与索引（Discovery & Indexing）
**FR-1** 扫描本地会话数据源，生成统一的 Session 列表
- 支持 Provider：Codex、Claude Code（可扩展）
- 支持全量扫描 + 增量更新
- 支持缺失/异常文件的容错（不中断 UI）

**FR-2** 本地索引（Cache/DB）
- 用于加速列表加载与搜索
- 索引字段至少包含：sessionId、provider、lastActiveAt、projectDir(可空)、summary(可空)、filePath(可空)

**FR-3** 数据源路径探测（可配置 + 多候选）
- 默认使用常见路径；允许用户在 Settings 覆盖
- 若无法探测到 provider 安装/数据目录：在 UI 显示未启用/不可用状态，但不报错崩溃

---

### 5.2 会话列表（List）
**FR-4** 列表展示字段（建议最小集）
- Provider（Codex / Claude）
- Session 标识（id/short id）
- 最近活跃时间（lastActiveAt）
- 目录（projectDir，若未知显示 “Unknown”）
- 摘要（summary：最后一条/首条截断或规则生成）

**FR-5** 列表交互
- 搜索（跨会话，关键词匹配 transcript/summary/目录）
- 过滤：Provider、是否有目录、时间范围
- 排序：最近活跃（默认）、最早、按目录

**FR-6** 空态/异常态
- 未发现任何会话：给出“如何启用/设置路径”的指引
- 发现会话但无法解析内容：列表仍可显示基本信息，并在详情页提示“解析失败”

---

### 5.3 会话详情（Detail）
**FR-7** 会话内容展示
- 时间线展示消息（role：user/assistant/tool 等）
- 支持在当前会话内搜索 + 高亮
- 展示元信息：
  - provider、sessionId、创建/最近活跃时间
  - projectDir（可空）
  - 原始文件路径（可选显示，便于 debug）

**FR-8** 性能策略
- 默认按需加载（打开详情才加载全文）
- 对超长 transcript 支持分页/虚拟列表（防止卡顿）

---

### 5.4 恢复能力（Resume / Restore）
#### 5.4.1 复制恢复命令（必做）
**FR-9** “复制恢复命令”按钮
- 根据 provider 生成恢复命令（模板可配置）
- 点击后写入剪贴板，并 toast 提示成功

> 说明：不同版本 CLI 命令可能略有差异，建议将命令模板做成可配置项（Settings），默认提供推荐模板。

#### 5.4.2 复制会话目录（尽量做）
**FR-10** “复制会话目录”按钮
- 当 projectDir 可得时启用；不可得时置灰，并提示原因（无法推断目录）
- 复制内容为可直接 `cd` 的绝对路径（或原样）

#### 5.4.3 一键终端恢复（可选但强烈建议）
**FR-11** “在终端恢复”按钮（或下拉菜单）
- 默认目标：macOS Terminal
- 支持 kitty（v1 要求）
- 执行策略：
  - `cd "<projectDir>" && <resumeCommand>`（若 projectDir 为空则仅执行 resumeCommand）
- 失败降级：
  - 无权限/终端不可用 → 自动降级为“仅复制命令”，并提示用户如何修复（例如开启 Automation 权限、kitty remote control）

**FR-12** 终端目标选择与记忆
- 下拉选择：Terminal / kitty /（预留 iTerm2）/ 仅复制
- 记住上次选择作为默认

---

## 6. 平台与扩展性设计（macOS v1 + Future-proof）

### 6.1 Provider Adapter 抽象（必须）
统一接口（示例）：
- `detect(): boolean`
- `scanSessions(): SessionMeta[]`
- `loadTranscript(sessionId): Message[]`
- `getResumeCommand(sessionId): string`
- `getProjectDir(sessionId): string | null`

### 6.2 Terminal Launcher 抽象（必须）
- `launch(command: string, cwd?: string, targetTerminal: TerminalKind): Result`
- macOS v1 实现：TerminalLauncherMac
- Future：TerminalLauncherWindows / TerminalLauncherLinux

### 6.3 Path Resolver（必须）
- `resolveProviderDataPaths(providerId): string[]`
- v1 返回 macOS 默认候选；允许 Settings 覆盖

---

## 7. 隐私与安全（Privacy & Security）

**默认原则：全本地、只读、不上传。**
- transcript 默认不出网
- 本地索引默认仅存必要字段（可选：是否缓存全文内容）
- 提供“敏感信息遮罩”（可选）：
  - 简单正则：token/key/password 等
- 提示用户：会话内容可能包含敏感信息，导出/复制时注意

---

## 8. 非功能需求（Non-Functional Requirements）

### 8.1 性能
- 首次打开：列表可在 1s 内展示（允许先展示缓存，再后台增量刷新）
- 搜索：在 1k 会话量级可用（建立索引或增量缓存）
- 详情页：打开后 300ms 内渲染骨架屏，内容流式/分段加载

### 8.2 稳定性
- 任一 provider 数据源损坏不影响整体（隔离失败）
- 扫描过程可中断/可重试

### 8.3 可观测性（可选）
- 本地日志：扫描耗时、解析失败原因、终端启动失败原因（便于 debug）

---

## 9. 关键数据结构（建议）

### 9.1 SessionMeta
- `providerId: "codex" | "claude" | string`
- `sessionId: string`
- `title?: string`
- `summary?: string`
- `projectDir?: string | null`
- `createdAt?: number`
- `lastActiveAt?: number`
- `sourcePath?: string`

### 9.2 Message
- `role: "user" | "assistant" | "tool" | "system" | string`
- `content: string`
- `ts?: number`
- `raw?: any`（保留原始字段，便于兼容未来格式）

---

## 10. 交互流程（UX Flows）

### 10.1 Flow A：搜索并查看
1) 打开 Session Manager → 看到列表
2) 输入关键词搜索 → 命中会话
3) 点击会话 → 进入详情 → 浏览内容 / 在会话内搜索

### 10.2 Flow B：复制恢复命令
1) 列表或详情页点击“复制恢复命令”
2) toast 成功 → 用户粘贴到终端执行

### 10.3 Flow C：一键终端恢复
1) 详情页点击“在终端恢复”（默认 Terminal）
2) 系统打开终端新窗口/新 tab
3) 自动执行：`cd projectDir && resumeCommand`
4) 失败 → toast 提示，并提供“复制命令”降级路径

---

## 11. 边界情况与降级策略

- 无法获取 projectDir：仍可恢复（只执行 resume），目录按钮置灰
- 无法解析 transcript：列表仍显示，详情提示“无法解析”，可提供“打开原始文件路径”
- CLI 命令模板不匹配：允许 Settings 自定义模板；默认模板可更新
- 终端权限问题（Automation）：提示用户在系统设置中开启对应权限，并允许降级为复制命令
- kitty 未开启 remote control：提示如何配置，降级为复制命令

---

## 12. 里程碑与交付（建议）

### M1（核心可用）
- Provider 扫描：Codex / Claude
- 列表 + 详情（可读）
- 复制恢复命令
- 复制目录（若可得）

### M2（效率提升）
- 跨会话搜索、过滤/排序
- 增量索引与文件监听（可选）
- “在 macOS Terminal 恢复”

### M3（终端覆盖与可扩展）
- “在 kitty 恢复”
- 终端目标下拉与记忆
- 插件化接口/扩展点文档

---

## 13. 后续功能候选（Backlog / Ideas）

- 收藏/Pin 会话
- 会话标签（项目/主题/状态）
- 会话摘要（本地生成）
- Fork 会话继续（避免污染原会话）
- 导出 Markdown/JSONL
- 按项目聚合（Repo 视图）
- 会话清理/归档（磁盘管理）

---
