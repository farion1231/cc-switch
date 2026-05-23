# Feature Specification: Pi CLI 配置管理支持

**Feature Branch**: `001-pi-cli-support`

**Created**: 2026-05-23

**Status**: Draft

**Input**: User description: "当前的项目，不支持pi项目的配置切换，我需要在这个项目中添加pi的配置，https://pi.dev/官方地址在这里，讲述了如何配置pi这个agent cli"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - 添加 Pi 作为新的 CLI 工具选项卡 (Priority: P1)

作为一名同时使用 Pi 和其他 AI CLI 工具的开发者，我希望在 CC Switch 主界面中看到 Pi 作为一个独立选项卡，与现有的 Claude Code、Codex、Gemini CLI、OpenCode、OpenClaw、Hermes 并列显示，以便我可以在统一界面中管理 Pi 的配置。

**Why this priority**: 这是整个功能的基础入口。没有 Pi 选项卡，用户无法访问任何 Pi 相关的配置管理功能。

**Independent Test**: 打开 CC Switch 设置，在"可见应用"中启用 Pi，验证主界面出现 Pi 选项卡并可切换。

**Acceptance Scenarios**:

1. **Given** CC Switch 已安装且运行中，**When** 用户在设置中启用 Pi 可见性，**Then** 主界面侧边栏/选项卡中出现 Pi 图标和标签
2. **Given** Pi 选项卡已启用，**When** 用户点击 Pi 选项卡，**Then** 显示 Pi 的提供商管理界面（初始为空状态，提示添加提供商）
3. **Given** Pi 选项卡已启用，**When** 用户在设置中禁用 Pi 可见性，**Then** Pi 选项卡从主界面隐藏

---

### User Story 2 - Pi 提供商配置管理 (Priority: P1)

作为一名 Pi 用户，我希望通过 CC Switch 管理 Pi 的 API 提供商配置（API 密钥、Base URL、模型选择），并能一键切换当前使用的提供商，就像管理 Claude Code 或 Codex 的提供商一样。

**Why this priority**: 提供商配置是 CC Switch 的核心价值——消除手动编辑配置文件的需求。这是用户最主要的使用场景。

**Independent Test**: 在 Pi 选项卡中添加一个 Anthropic 提供商，填入 API 密钥，点击"设为当前"，验证 Pi CLI 可以正常使用该配置。

**Acceptance Scenarios**:

1. **Given** 用户在 Pi 选项卡中，**When** 点击"添加提供商"，**Then** 显示提供商预设列表（Anthropic、OpenAI、Google Gemini 等内置提供商），用户可选择预设或手动配置
2. **Given** 用户选择 Anthropic 预设，**When** 填入 API 密钥并保存，**Then** CC Switch 将配置写入 `~/.pi/agent/models.json`（如使用自定义 API）或通过环境变量方式配置
3. **Given** 已添加多个 Pi 提供商，**When** 用户点击某个提供商的"设为当前"按钮，**Then** Pi 的当前提供商切换为该选择，`settings.json` 中的 `defaultProvider` 和 `defaultModel` 字段相应更新
4. **Given** 用户编辑已有提供商的 API 密钥或 Base URL，**When** 保存修改，**Then** 对应的 `models.json` 配置立即更新

---

### User Story 3 - Pi Skills 管理集成 (Priority: P2)

作为一名 Pi 用户，我希望通过 CC Switch 统一管理 Skills（Agent Skills 标准），包括从 GitHub 仓库安装、启用/禁用、同步到 Pi 的 skills 目录。

**Why this priority**: Skills 是 Pi 的核心扩展机制之一。CC Switch 已有统一的 Skills 管理面板，扩展到 Pi 可以复用现有基础设施。

**Independent Test**: 在 CC Switch 的 Skills 管理面板中启用 Pi 作为目标应用，安装一个 Skill，验证 Pi 的 `~/.pi/agent/skills/` 目录中出现该 Skill。

**Acceptance Scenarios**:

1. **Given** CC Switch Skills 管理面板已打开，**When** 用户安装一个 Skill 并勾选 Pi 作为目标应用，**Then** Skill 被同步到 `~/.pi/agent/skills/<skill-name>/` 目录
2. **Given** 用户已有 Skills 安装在 Pi 目录中，**When** CC Switch 扫描 Skills 时，**Then** 识别并显示这些 Skills（标记为"未管理"或自动纳入管理）
3. **Given** 用户卸载一个 Skill，**When** 该 Skill 之前已同步到 Pi 目录，**Then** Pi 目录中的对应文件被移除

---

### User Story 4 - Pi Settings 可视化管理 (Priority: P2)

作为一名 Pi 用户，我希望通过 CC Switch 图形化编辑 Pi 的全局设置（settings.json），而不是手动编辑 JSON 文件。

**Why this priority**: Settings 管理是提升用户体验的重要功能，减少手动编辑 JSON 文件的出错风险。

**Independent Test**: 在 Pi 选项卡的设置子页面中修改默认思考级别（thinking level），验证 `~/.pi/agent/settings.json` 中对应字段被正确更新。

**Acceptance Scenarios**:

1. **Given** 用户在 Pi 选项卡中打开"设置"子页面，**When** 查看设置项，**Then** 显示常用设置：默认模型、思考级别（off/minimal/low/medium/high/xhigh）、主题（dark/light）、压缩配置
2. **Given** 用户修改思考级别为 "high"，**When** 点击保存，**Then** `~/.pi/agent/settings.json` 中 `defaultThinkingLevel` 字段更新为 `"high"`
3. **Given** Pi 设置文件不存在（首次使用），**When** 用户首次保存设置，**Then** CC Switch 自动创建 `~/.pi/agent/settings.json` 并写入配置

---

### User Story 5 - Pi Context Files 管理 (Priority: P3)

作为一名 Pi 用户，我希望通过 CC Switch 编辑 Pi 的上下文文件（AGENTS.md、SYSTEM.md），实现跨工具的提示词同步（如当前已支持的 CLAUDE.md / AGENTS.md 互相同步）。

**Why this priority**: Context files 管理是锦上添花的功能，利用 CC Switch 已有的 Prompt 编辑器基础设施。

**Independent Test**: 在 CC Switch 的 Prompt 编辑器中，将 Pi 的 AGENTS.md 与 Claude Code 的 CLAUDE.md 同步，验证 Pi 启动时能加载正确的上下文文件。

**Acceptance Scenarios**:

1. **Given** 用户在 Prompt 管理面板中，**When** 选择 Pi 的 AGENTS.md 进行编辑，**Then** 编辑器显示 `~/.pi/agent/AGENTS.md` 的内容
2. **Given** 用户启用了跨应用同步，**When** 修改 Pi 的 AGENTS.md 并保存，**Then** 同步目标应用的上下文文件也更新（如 Claude Code 的 CLAUDE.md）
3. **Given** Pi 上下文文件不存在，**When** 用户首次编辑并保存，**Then** CC Switch 自动创建对应文件

---

### Edge Cases

- Pi 尚未安装在用户机器上时，CC Switch 如何处理？应显示"未检测到 Pi 安装"的提示，但仍允许配置（配置将在 Pi 安装后自动生效）
- 用户的 `models.json` 已有手动添加的自定义模型，CC Switch 写入时是否会覆盖？应保留用户手动添加的配置（合并策略而非覆盖）
- Pi 的 settings.json 与 CC Switch 管理的 settings.json 格式不一致时如何处理？应进行格式校验，无效 JSON 时提示用户（不覆盖损坏的文件）
- 多个 CC Switch 管理的提供商写入 models.json 时如何确保数据完整性？应使用原子写入策略

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: 系统 MUST 在 AppType 枚举中新增 `Pi` 类型，并在 VisibleApps 中添加 Pi 的可见性控制（默认可见）
- **FR-002**: 系统 MUST 在 Provider 系统中支持 Pi 类型的提供商，包括内置预设（Anthropic、OpenAI、Google Gemini 等主流提供商）
- **FR-003**: 系统 MUST 将 Pi 的提供商配置写入 `~/.pi/agent/models.json`，使用 `openai-completions`、`anthropic-messages` 或 `google-generative-ai` API 类型
- **FR-004**: 系统 MUST 支持通过环境变量方式配置 Pi 的 API 密钥（`ANTHROPIC_API_KEY`、`OPENAI_API_KEY` 等），写入 Pi 的 `.env` 文件或系统环境
- **FR-005**: 系统 MUST 向 Pi 的 `settings.json` 写入 `defaultProvider` 和 `defaultModel` 字段以完成提供商切换
- **FR-006**: 系统 MUST 将 Pi 集成到现有的 Skills 管理系统中，支持将 Skills 同步到 `~/.pi/agent/skills/` 目录
- **FR-007**: 系统 MUST 提供 Pi 常用设置的可视化编辑界面（思考级别、主题、压缩配置等）
- **FR-008**: 系统 MUST 支持 Pi 的 AGENTS.md 上下文文件编辑和跨应用同步
- **FR-009**: 系统 MUST 在写入 Pi 配置时采用原子写入策略（先写临时文件再 rename）
- **FR-010**: 系统 MUST 保留用户已在 `models.json` 中手动添加的自定义模型（合并而非覆盖）
- **FR-011**: 系统 MUST 在 Pi 未安装时仍允许配置管理，并给出适当的提示信息
- **FR-012**: 系统 MUST 支持 Pi 的配置目录自定义（类似其他工具的 override_dir 设置）

### Key Entities *(include if feature involves data)*

- **Pi Provider**: 表示 Pi 使用的一个 API 提供商配置。关键属性：提供商名称、API 类型（openai-completions/anthropic-messages/google-generative-ai）、Base URL、API 密钥、模型列表
- **Pi Settings**: 表示 Pi 的全局设置集合。关键属性：默认提供商、默认模型、思考级别、主题、压缩参数、重试参数
- **Pi Skills 关联**: 将 CC Switch 已有的 Skill 实体关联到 Pi 应用的目标同步目录

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 用户可以在 30 秒内完成 Pi 提供商的新增和切换操作（从打开 CC Switch 到切换完成）
- **SC-002**: 提供商切换后，Pi CLI 在下次启动时能正确使用新配置（无需额外的手动编辑）
- **SC-003**: 用户已手动添加的自定义模型在 CC Switch 写入配置后 100% 保留（零数据丢失）
- **SC-004**: Pi 选项卡的 UI 与现有工具选项卡（Claude Code、Codex 等）视觉风格和行为一致
- **SC-005**: 90% 的常用 Pi 设置项能通过 CC Switch 图形界面完成，无需手动编辑 JSON

## Assumptions

- Pi 的配置目录遵循官方默认路径：`~/.pi/agent/`（全局）和 `.pi/`（项目级）。CC Switch 主要管理全局配置
- Pi 使用 Agent Skills 标准（agentskills.io），与 CC Switch 现有的 Skills 管理系统兼容
- Pi 的提供商配置遵循 `models.json` 格式规范，支持 `openai-completions`、`anthropic-messages`、`google-generative-ai`、`openai-responses` 四种 API 类型
- Pi 的 settings.json 格式保持向后兼容，CC Switch 只写入已知字段，未知字段保留不变
- 用户的 Pi 安装由用户自行完成（CC Switch 不负责安装 Pi CLI 本身）
- 上下文文件同步沿用现有的 Prompt 同步机制（source-target sync 模式），Pi 使用 AGENTS.md 作为上下文文件名
