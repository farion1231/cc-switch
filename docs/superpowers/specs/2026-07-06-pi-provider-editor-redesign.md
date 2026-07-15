# Pi Provider 编辑器改造设计

## 背景与目标

当前 CC Switch 的 Pi 应用 provider 管理采用 `providers / edit / review` 三 tab 容器：

- providers tab：展示 provider 卡片列表，顶部有 tab 导航，列表内带「自定义提供商」分组标题。
- edit tab：填写表单后需点击「预览并审查」。
- review tab：查看差异摘要后再点击「审查并应用」才能保存。

用户希望：

1. 删除 provider 列表页顶部的 tab 导航和「自定义提供商」标题，使列表更简洁。
2. 把 provider 编辑页改成类似 OpenCode 编辑 provider 的布局：进入编辑后是独立的全屏页面，中间按字段填写，底部实时显示「配置 JSON」，最下方是「保存」按钮。不再使用「预览并审查 → 审查并应用」的两步流程。

## 范围

**在范围内**

- `PiAgentPanel`：移除 Tabs，改为列表/编辑两种状态。
- `PiProviderList`：删除「自定义提供商」分组标题，保留「内置提供商」分组标题用于区分。
- `PiProviderForm`：调整字段顺序和分组，新增底部「配置 JSON」只读预览区。
- 编辑页使用 `FullScreenPanel` 全屏面板，右下角放置「保存」按钮。
- 保存流程：前端内部先 `previewProviderPatch` 获取 file hash，再立即 `applyProviderPatch`，对外表现为一键保存。

**不在范围内**

- 不改 Pi 后端 API（`list_pi_providers`、`preview_pi_provider_patch`、`apply_pi_provider_patch`、`delete_pi_provider`、`test_pi_connectivity`）。
- 不改动 Pi provider 的数据模型或字段语义。
- 不涉及通用 provider（Claude/Codex/Gemini 等）的编辑页。
- 底部 JSON 预览本期只做只读；可编辑 JSON 的双向同步作为未来增强项，不在本次实现。

## 设计决策

### 1. 去掉 Tab 导航

当前 `PiAgentPanel` 内部用 `<Tabs>` 把 providers / edit / review 包在一起。改造后：

- `PiAgentPanel` 只负责列表视图。
- 点击卡片上的编辑按钮时，通过局部状态切换到全屏编辑视图。
- 全屏编辑视图使用 `FullScreenPanel`，关闭后回到列表视图。

这样列表页不再有任何 tab，彻底删除红框内容。

### 2. 列表页分组标题

`PiProviderList` 当前把 provider 分成「自定义提供商」和「内置提供商」两组，每组都有 `<h3>` 标题。

决策：

- 删除「自定义提供商」标题（红框内容）。
- 保留「内置提供商」标题，因为 tab 删除后用户仍需要一种方式区分内置与自定义 provider。
- 如果自定义 provider 为空，直接不渲染该分组区域，不显示空标题。

### 3. 编辑页布局

参考 OpenCode 截图，编辑页结构如下：

```
┌─────────────────────────────────────────────┐
│ ←  编辑供应商                                │  <- FullScreenPanel Header
├─────────────────────────────────────────────┤
│                                             │
│  [Provider Icon / 标识区]                   │
│                                             │
│  供应商标识 *                               │  <- 编辑时如已加入配置则只读
│  [longcat                    ]              │
│  该供应商已添加到应用配置中，标识不可修改    │
│                                             │
│  供应商名称          备注                   │
│  [Longcat        ]  [        ]              │
│                                             │
│  官网链接                                   │
│  [https://...                  ]            │
│                                             │
│  接口格式                                   │
│  [OpenAI Compatible          ▼]            │
│                                             │
│  API Key                                    │
│  [•••••••••••••              👁]            │
│  获取 API Key                               │
│                                             │
│  Base URL                                   │
│  [https://api.longcat.chat/openai]          │
│                                             │
│  额外选项                              + 添加 │
│  键名              值                       │
│  [setCacheKey]  [true]  🗑                   │
│                                             │
│  模型配置                    获取模型列表 + 添加模型 │
│  ▸ 模型 ID          显示名称                 │
│  [LongCat-2.0]  [LongCat-2.0]  🗑            │
│                                             │
│  ─────────────────────────────────────      │
│  配置 JSON                                  │
│  ┌─────────────────────────────────────┐    │
│  │ {                                   │    │
│  │   "npm": "@ai-sdk/openai-compatible",│   │
│  │   ...                               │    │
│  │ }                                   │    │
│  └─────────────────────────────────────┘    │
│                                             │
├─────────────────────────────────────────────┤
│                         [💾 保存]           │  <- FullScreenPanel Footer
└─────────────────────────────────────────────┘
```

### 4. 字段顺序调整

为对齐 OpenCode 截图，`PiProviderForm` 的字段顺序和分组调整为：

1. 标识区（图标 + 供应商标识）
2. 基础信息（名称 / 备注 / 官网链接）
3. 接口格式（API Template）
4. 认证与端点（API Key / Base URL）
5. 额外选项（key-value 列表）
6. 模型配置（模型列表）
7. 配置 JSON（只读预览）

当前 `PiProviderForm` 里已有这些字段，主要是顺序和视觉分组需要调整。

### 5. 配置 JSON 预览

- 在 `PiProviderForm` 底部新增一个 `JsonEditor`（只读，`readOnly` 或 `onChange={() => {}}`）。
- 预览内容根据当前 `draft` 实时生成，与后端实际会写入 `models.json` 的结构保持一致。
- 提供一个「格式化」按钮，方便用户查看。
- 该 JSON 仅用于展示和校验，本期不开放直接编辑。

### 6. 保存流程

当前流程：

1. 用户在 edit tab 填表。
2. 点击「预览并审查」→ `previewProviderPatch` → 拿到 `currentFileHash` 和 `nextModelsJson`。
3. 跳转到 review tab。
4. 点击「审查并应用」→ `applyProviderPatch(draft, fileHash)`。

新流程：

1. 用户在全屏编辑页填表。
2. 点击「保存」。
3. 组件内部：
   - 先校验 `providerId` 非空。
   - 调用 `previewProviderPatch(draft)` 获取 `currentFileHash` 和 `nextModelsJson`。
   - 如果 preview 成功，立即调用 `applyProviderPatch(draft, currentFileHash)`。
   - 成功后关闭编辑页，刷新列表，toast 提示保存成功。
4. 如果 preview 或 apply 失败，显示错误 toast，停留在编辑页，用户可修改后重试。

这样仍使用后端乐观锁，但用户感知是一键保存。

### 7. 删除流程

- 删除按钮保留在 provider 卡片上。
- 点击删除：
  1. 用临时 draft（只带 `providerId`）调用 `previewProviderPatch` 获取 file hash。
  2. 调用 `deleteProvider(providerId, fileHash)`。
  3. 成功后刷新列表。
- 增加二次确认弹窗，避免误删。

### 8. 新建 provider 流程

- 列表页保留「添加 provider」入口（当前 PiAgentPanel 通过 `openAdd` 暴露给 App.tsx 的橙色 + 按钮）。
- 点击后进入全屏编辑页，`draft` 初始化为 `emptyPiProviderDraft`。
- 保存逻辑同上。

## 组件改动

### `PiAgentPanel`

- 移除 `<Tabs>`、`<TabsList>`、`<TabsContent>`。
- 新增局部状态 `view: 'list' | 'edit'`。
- `view === 'list'` 时渲染 `PiProviderList`。
- `view === 'edit'` 时渲染 `PiProviderEditor`（新组件或直接在 `FullScreenPanel` 内渲染 `PiProviderForm`）。
- 把 `buildPreview`、`applyPreview`、`deletePreview` 等逻辑重组成统一的保存/删除方法。
- 仍通过 `forwardRef` 暴露 `openAdd()` 给 App.tsx。

### `PiProviderList`

- 删除「自定义提供商」分组的 `<h3>` 标题。
- 保留「内置提供商」分组标题。
- 自定义 provider 为空时不渲染该分组。

### `PiProviderForm`

- 调整字段顺序：标识 → 基础信息 → 接口格式 → API Key → Base URL → 额外选项 → 模型配置 → 配置 JSON。
- 新增 `readOnlyConfigJson` prop 或内部派生状态，用于底部 JSON 预览。
- 移除当前与保存相关的按钮（当前 edit tab 里的「预览并审查」按钮不在 `PiProviderForm` 内，而是在 `PiAgentPanel` 里，所以 `PiProviderForm` 本身不需要删按钮）。

### 新增/复用 `FullScreenPanel`

- 复用 `src/components/common/FullScreenPanel.tsx`。
- 编辑页通过 `FullScreenPanel` 包裹 `PiProviderForm`，footer 放「保存」按钮。

### 删除 `PiProviderDiffPreview` 的使用

- 改造后不再需要 review tab，因此 `PiProviderDiffPreview` 不再被 `PiAgentPanel` 使用。
- 该组件文件可以保留，未来如需高级 diff 查看可复用；本期不删除文件，仅移除引用。

## 数据流

```
App.tsx
  │ 点击 Pi 应用 + 按钮 → piPanelRef.current?.openAdd()
  │ 或 PiAgentPanel 内部点击卡片编辑
  ▼
PiAgentPanel
  │ view: 'list' / 'edit'
  │
  ├─ list ──► PiProviderList ──► 卡片点击 onEdit / onDelete / onDuplicate / onTestConnectivity
  │
  └─ edit ──► FullScreenPanel
                │
                ├─ Content: PiProviderForm (draft ↔ setDraft)
                │            └─ 实时生成 configJson 预览
                │
                └─ Footer: 保存按钮
                              │
                              ▼
                         previewProviderPatch(draft)
                              │
                              ▼
                         applyProviderPatch(draft, fileHash)
                              │
                              ▼
                         刷新列表 → view = 'list'
```

## 错误处理

| 场景 | 处理 |
|------|------|
| `providerId` 为空 | 保存按钮禁用或点击时 toast 提示 |
| `previewProviderPatch` 失败 | toast 显示错误，停留在编辑页 |
| `applyProviderPatch` 失败（如 file hash 冲突） | toast 显示错误，建议用户刷新后重试 |
| 删除时获取 hash 失败 | toast 显示错误 |
| JSON 预览生成失败 | 显示空对象 `{}` 或错误占位，不影响表单编辑 |

## 样式与交互

- 全屏编辑页背景使用 `hsl(var(--background))`，与现有 `FullScreenPanel` 一致。
- 保存按钮固定在右下角，使用主色（蓝色）。
- 返回按钮在左上角，点击关闭编辑页并回到列表。
- 表单字段使用项目现有 `Input`、`Select`、`Button`、`Switch` 等 shadcn/ui 组件。
- 配置 JSON 使用现有 `JsonEditor` 组件，只读模式。

## 测试要点

- 列表页不再显示 tab 导航和「自定义提供商」标题。
- 点击编辑进入全屏编辑页，字段顺序与 OpenCode 截图一致。
- 修改表单后，底部 JSON 预览实时更新。
- 点击保存后内部先 preview 再 apply，成功后返回列表并刷新。
- 删除 provider 仍能通过 preview+delete 正确移除。
- 新建 provider 流程正常。
- 网络失败或 hash 冲突时正确提示错误。

## 后续可增强项

- 底部「配置 JSON」支持直接编辑，并实现表单 ↔ JSON 双向同步。
- 在编辑页增加「连通性测试」按钮。
- 保存前增加软校验提示（如 baseUrl 为空等）。
