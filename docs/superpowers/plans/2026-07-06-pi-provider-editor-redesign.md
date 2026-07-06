# Pi Provider 编辑器改造实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 CC Switch Pi 应用的 provider 管理从三 tab 容器改为「列表页 + 全屏编辑页」结构，编辑页底部实时显示配置 JSON 预览和保存按钮。

**Architecture:** 复用现有 `FullScreenPanel` 作为编辑页容器；`PiAgentPanel` 内部维护 `view: 'list' | 'edit'` 状态；`PiProviderForm` 负责表单和底部 JSON 预览；保存时内部先 `previewProviderPatch` 获取 file hash，再立即 `applyProviderPatch`。

**Tech Stack:** React + TypeScript + Tailwind CSS + shadcn/ui + CodeMirror (`JsonEditor`) + Tauri invoke。

## Global Constraints

- 不改 Pi 后端 API：`list_pi_providers`、`preview_pi_provider_patch`、`apply_pi_provider_patch`、`delete_pi_provider`、`test_pi_connectivity`。
- 不改动 `PiProviderDraft` 数据模型。
- 底部「配置 JSON」本期只读，不开放直接编辑。
- 保留「内置提供商」分组标题，仅删除「自定义提供商」标题。
- 删除按钮保留在 provider 卡片上，删除前需二次确认。

---

## File Structure

| 文件 | 职责 |
|------|------|
| `src/components/JsonEditor.tsx` | 新增 `readOnly` prop，支持只读代码编辑器。 |
| `src/components/pi/PiProviderForm.tsx` | 调整字段顺序；新增底部「配置 JSON」只读预览区；新增 `buildConfigJsonPreview` 辅助函数。 |
| `src/components/pi/PiProviderList.tsx` | 删除「自定义提供商」分组标题；保留「内置提供商」标题。 |
| `src/components/pi/PiAgentPanel.tsx` | 移除 Tabs；改为 `view: 'list' \| 'edit'`；列表页渲染 `PiProviderList`；编辑页渲染 `FullScreenPanel` + `PiProviderForm`；保存/删除逻辑重组为一步保存。 |
| `src/i18n/locales/zh.json` (及 en/ja/zh-TW) | 新增/修改文案：编辑页标题、保存按钮、配置 JSON 标签、只读提示、删除确认等。 |

---

## Task 1: 给 `JsonEditor` 增加 `readOnly` 支持

**Files:**

- Modify: `src/components/JsonEditor.tsx`

**Interfaces:**

- Consumes: 无
- Produces: `JsonEditorProps.readOnly?: boolean`

Pi 编辑页底部的「配置 JSON」需要只读展示，但又要保留 CodeMirror 的语法高亮和格式化按钮。给 `JsonEditor` 增加 `readOnly` prop，在 `readOnly` 为 `true` 时让编辑器不可输入。

- [ ] **Step 1: 在 `JsonEditorProps` 中增加 `readOnly` 字段**

```tsx
interface JsonEditorProps {
  id?: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  darkMode?: boolean;
  rows?: number;
  showValidation?: boolean;
  language?: "json" | "javascript";
  height?: string | number;
  showMinimap?: boolean;
  readOnly?: boolean;
}
```

- [ ] **Step 2: 解构 `readOnly` 并传入 `extensions`**

```tsx
const JsonEditor: React.FC<JsonEditorProps> = ({
  value,
  onChange,
  placeholder: placeholderText = "",
  darkMode = false,
  rows = 12,
  showValidation = true,
  language = "json",
  height,
  readOnly = false,
}) => {
```

在 `extensions` 数组中追加 `EditorView.editable.of(false)`：

```tsx
const extensions = [
  basicSetup,
  language === "javascript" ? javascript() : json(),
  placeholder(placeholderText || ""),
  baseTheme,
  sizingTheme,
  jsonLinter,
  EditorView.updateListener.of((update) => {
    if (update.docChanged) {
      const newValue = update.state.doc.toString();
      onChange(newValue);
    }
  }),
  ...(readOnly ? [EditorView.editable.of(false)] : []),
];
```

- [ ] **Step 3: 格式化按钮在只读模式下仍可用**

`handleFormat` 已经调用 `onChange(formatted)`。只读模式下用户无法手动输入，但仍可点击「格式化」按钮整理当前 JSON。无需改动。

- [ ] **Step 4: 运行类型检查**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm tsc --noEmit`
Expected: 无新增类型错误。

- [ ] **Step 5: 提交**

```bash
git add src/components/JsonEditor.tsx
git commit -m "feat(JsonEditor): add readOnly prop"
```

---

## Task 2: 改造 `PiProviderForm` —— 调整字段顺序并新增底部 JSON 预览

**Files:**

- Modify: `src/components/pi/PiProviderForm.tsx`

**Interfaces:**

- Consumes: `JsonEditorProps.readOnly` (Task 1)
- Produces: 同原 `PiProviderFormProps`（`value?: PiProviderDraft; onChange: (value: PiProviderDraft) => void;`），渲染布局调整，新增底部配置 JSON 区域。

当前 `PiProviderForm` 的字段顺序是：Vendor Quick Select → API Template → Provider Configuration (providerId/baseUrl/apiType/apiKey) → Models → Compat → Advanced(headers)。需要调整成更接近 OpenCode 编辑页的顺序：标识 → 接口格式 → 认证与端点 → 额外选项 → 模型配置 → 兼容选项 → 配置 JSON。

- [ ] **Step 1: 导入 `JsonEditor` 和 `useDarkMode`**

```tsx
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
```

- [ ] **Step 2: 新增 `buildConfigJsonPreview` 辅助函数**

在组件外部添加：

```tsx
function buildConfigJsonPreview(draft: PiProviderDraft): string {
  const apiKeyValue =
    draft.apiKey.mode === "env"
      ? `$${draft.apiKey.value}`
      : draft.apiKey.mode === "command"
        ? draft.apiKey.value
        : draft.apiKey.value;

  const headers = draft.headers.reduce<Record<string, string>>(
    (acc, h) => {
      if (h.key.trim()) acc[h.key] = h.value;
      return acc;
    },
    {},
  );

  const models = draft.models
    .filter((m) => m.id.trim())
    .map((m) => {
      const model: Record<string, unknown> = { id: m.id };
      if (m.name) model.name = m.name;
      if (m.reasoning) model.reasoning = true;
      if (m.input && m.input.length > 0) model.input = m.input;
      if (m.contextWindow) model.contextWindow = m.contextWindow;
      if (m.maxTokens) model.maxTokens = m.maxTokens;
      return model;
    });

  const provider: Record<string, unknown> = {};
  if (draft.baseUrl?.trim()) provider.baseUrl = draft.baseUrl;
  if (draft.api) provider.api = draft.api;
  if (apiKeyValue) provider.apiKey = apiKeyValue;
  if (Object.keys(headers).length > 0) provider.headers = headers;
  if (models.length > 0) provider.models = models;
  if (draft.compat && Object.keys(draft.compat).length > 0) {
    provider.compat = draft.compat;
  }

  return JSON.stringify(provider, null, 2);
}
```

- [ ] **Step 3: 在组件内部使用 `useDarkMode` 并计算 `configJson`**

```tsx
export function PiProviderForm({ value, onChange }: PiProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const draft = value ?? emptyPiProviderDraft;
  const configJson = buildConfigJsonPreview(draft);
  // ...
}
```

- [ ] **Step 4: 重排字段顺序**

把原来的 `return (...)` 中的内容按以下顺序重组（保留原有字段行为，仅调整 JSX 顺序）：

1. **Vendor Quick Select**（保留，作为顶部快速填充，可选使用）
2. **API Template**（接口格式）
3. **Provider Configuration**（供应商标识、Base URL、API Type、API Key）
4. **额外选项 / Headers**（从 Advanced 区域提取上来）
5. **Models**
6. **Compat**
7. **配置 JSON 预览**（新增）
8. **Reset 按钮**（保留在底部 JSON 之前或之后均可，建议放在配置 JSON 上方）

具体 JSX 调整：

```tsx
return (
  <div className="space-y-6">
    {/* Vendor Quick Select */}
    <section aria-label="Vendor presets" className="space-y-3">
      {/* 保持不变 */}
    </section>

    {/* API Template */}
    <section aria-label="API template" className="space-y-3">
      {/* 保持不变 */}
    </section>

    {/* Provider Configuration */}
    <section aria-label="Provider config" className="space-y-4">
      <h3 className="text-sm font-semibold">{t("pi.form.providerConfig")}</h3>
      <div className="grid gap-3 md:grid-cols-2">
        {/* providerId */}
        {/* baseUrl */}
        {/* apiType */}
        {/* apiKey */}
      </div>
    </section>

    {/* Extra Options / Headers */}
    <section aria-label="Extra options" className="space-y-3">
      <h3 className="text-sm font-semibold">{t("pi.form.extraOptions")}</h3>
      <label className="space-y-1">
        <span className="text-xs text-muted-foreground">{t("pi.form.headersLabel")}</span>
        <Textarea
          aria-label="Headers JSON"
          placeholder='{"x-extra":"$EXTRA_TOKEN"}'
          defaultValue={/* 同原 Advanced 中的 headers textarea */}
          onBlur={(e) => updateHeadersJson(e.target.value)}
          rows={3}
        />
      </label>
    </section>

    {/* Models */}
    <section aria-label="Models" className="space-y-3">
      {/* 保持不变 */}
    </section>

    {/* Compat */}
    <section aria-label="Compatibility" className="space-y-3">
      {/* 保持不变 */}
    </section>

    {/* Config JSON Preview */}
    <section aria-label="Config JSON" className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">{t("pi.form.configJson")}</h3>
        <span className="text-xs text-muted-foreground">{t("pi.form.configJsonReadOnly")}</span>
      </div>
      <JsonEditor
        value={configJson}
        onChange={() => {}}
        rows={14}
        showValidation={false}
        language="json"
        darkMode={isDarkMode}
        readOnly
      />
    </section>

    {/* Reset */}
    <section aria-label="Actions" className="space-y-3">
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" onClick={() => onChange({ ...emptyPiProviderDraft })}>
          {t("pi.form.resetAll")}
        </Button>
      </div>
    </section>
  </div>
);
```

- [ ] **Step 5: 删除原 Advanced 区域中已迁移的 Headers 和 Reset 按钮**

原 Advanced 区域中的 `headersLabel` Textarea 和 Reset 按钮已分别迁移到「额外选项」和「Actions」区域，原 Advanced section 应删除。

- [ ] **Step 6: 添加缺失的 i18n 键**

在 `src/i18n/locales/zh.json` 的 `pi.form` 下添加：

```json
"extraOptions": "额外选项",
"configJson": "配置 JSON",
"configJsonReadOnly": "只读预览"
```

（同步到 `en.json`、`ja.json`、`zh-TW.json`，翻译可先用英文占位或沿用中文，后续找母语者校对。）

- [ ] **Step 7: 运行类型检查与 lint**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm tsc --noEmit`
Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm lint`
Expected: 无新增错误。

- [ ] **Step 8: 提交**

```bash
git add src/components/pi/PiProviderForm.tsx src/i18n/locales/zh.json src/i18n/locales/en.json src/i18n/locales/ja.json src/i18n/locales/zh-TW.json
git commit -m "feat(pi): reorder PiProviderForm fields and add config JSON preview"
```

---

## Task 3: 删除 `PiProviderList` 中的「自定义提供商」分组标题

**Files:**

- Modify: `src/components/pi/PiProviderList.tsx`

**Interfaces:**

- Consumes: 无
- Produces: `ProviderGroup` 的 `title` 变为可选；自定义 provider 分组不再渲染标题。

- [ ] **Step 1: 让 `ProviderGroup` 的 `title` 可选**

```tsx
function ProviderGroup({
  title,
  entries,
  onEdit,
  onDuplicate,
  onDelete,
  onTestConnectivity,
}: {
  title?: string;
  entries: [string, unknown][];
  onEdit: (providerId: string) => void;
  onDuplicate?: (providerId: string) => void;
  onDelete?: (providerId: string) => void;
  onTestConnectivity?: (providerId: string) => Promise<void>;
}) {
  return (
    <section className="space-y-3">
      {title && <h3 className="text-sm font-semibold text-muted-foreground">{title}</h3>}
      <div className="space-y-3">
        {entries.map(([id, config]) => (
          <PiProviderCard
            key={id}
            id={id}
            config={config}
            onEdit={onEdit}
            onDuplicate={onDuplicate}
            onDelete={onDelete}
            onTestConnectivity={onTestConnectivity}
          />
        ))}
      </div>
    </section>
  );
}
```

- [ ] **Step 2: 自定义分组不传递 `title`**

```tsx
return (
  <div className="space-y-6">
    {custom.length > 0 && (
      <div className="space-y-3">
        {custom.map(([id, config]) => (
          <PiProviderCard
            key={id}
            id={id}
            config={config}
            onEdit={onEdit}
            onDuplicate={onDuplicate}
            onDelete={onDelete}
            onTestConnectivity={onTestConnectivity}
          />
        ))}
      </div>
    )}
    {builtin.length > 0 && (
      <ProviderGroup
        title={t("pi.list.builtin")}
        entries={builtin}
        onEdit={onEdit}
        onDuplicate={onDuplicate}
        onDelete={onDelete}
        onTestConnectivity={onTestConnectivity}
      />
    )}
  </div>
);
```

- [ ] **Step 3: 运行类型检查**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm tsc --noEmit`
Expected: 无新增错误。

- [ ] **Step 4: 提交**

```bash
git add src/components/pi/PiProviderList.tsx
git commit -m "feat(pi): hide custom provider group title in PiProviderList"
```

---

## Task 4: 改造 `PiAgentPanel` —— 移除 Tabs，改为列表/编辑分屏

**Files:**

- Modify: `src/components/pi/PiAgentPanel.tsx`

**Interfaces:**

- Consumes: `PiProviderForm`（Task 2）、`PiProviderList`（Task 3）、`FullScreenPanel`（现有）、`JsonEditor.readOnly`（Task 1）
- Produces: 仍通过 `forwardRef` 暴露 `openAdd()`；新增局部 `view: 'list' | 'edit'` 状态。

当前 `PiAgentPanel` 用 `Tabs` 把 providers/edit/review 包在一起。改造后：

- 维护 `view: 'list' | 'edit'`。
- `view === 'list'` 时渲染 `PiProviderList`。
- `view === 'edit'` 时用 `FullScreenPanel` 渲染 `PiProviderForm`，footer 放保存按钮。
- 保存时内部先 `previewProviderPatch` 获取 `currentFileHash`，再立即 `applyProviderPatch`。
- 删除 provider 时先 preview 获取 hash，再 `deleteProvider`。

- [ ] **Step 1: 导入 `FullScreenPanel` 和 `Save` 图标，移除 Tabs 和 `PiProviderDiffPreview` 导入**

```tsx
import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save } from "lucide-react";
import { piApi } from "@/lib/api";
import type {
  PiProviderDraft,
  PiProviderPatchPreview,
  PiProvidersMap,
} from "@/types/pi";
import { emptyPiProviderDraft, PiProviderForm } from "@/components/pi/PiProviderForm";
import { PiProviderList } from "@/components/pi/PiProviderList";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";
```

- [ ] **Step 2: 替换状态**

把：

```tsx
const [activeTab, setActiveTab] = useState("providers");
```

改为：

```tsx
const [view, setView] = useState<"list" | "edit">("list");
const [isSaving, setIsSaving] = useState(false);
const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
```

- [ ] **Step 3: 修改 `startNew` 和 `editProvider`**

```tsx
const startNew = () => {
  setDraft({ ...emptyPiProviderDraft });
  setPreview(null);
  setView("edit");
};
```

```tsx
const editProvider = (providerId: string) => {
  const provider = providers[providerId] as Record<string, unknown> | undefined;
  if (!provider) {
    startNew();
    return;
  }
  // ... 原有解析逻辑 ...
  setDraft({
    mode: "custom",
    providerId,
    template: "custom",
    baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl : "",
    api: typeof provider.api === "string" ? provider.api : "openai-completions",
    apiKey,
    headers,
    models,
    compat: rawCompat ? { ... } : null,
    advancedJson: null,
  });
  setPreview(null);
  setView("edit");
};
```

- [ ] **Step 4: 重组保存逻辑**

新增一个统一的 `saveProvider` 方法：

```tsx
const saveProvider = async () => {
  if (!draft.providerId.trim()) {
    toast.error(t("pi.save.providerIdRequired", { defaultValue: "请填写供应商标识" }));
    return;
  }

  setIsSaving(true);
  try {
    const preview = await piApi.previewProviderPatch(draft);
    const result = await piApi.applyProviderPatch(draft, preview.currentFileHash);
    toast.success(t("pi.toast.saved"), {
      description: t("pi.toast.savedDesc", { path: result.backupPath }),
    });
    setPreview(null);
    await refresh();
    setView("list");
  } catch (error) {
    toast.error(t("pi.toast.applyFailed"), {
      description: String(error),
    });
  } finally {
    setIsSaving(false);
  }
};
```

注意：这里仍用 `preview` 结果，但不再渲染 `PiProviderDiffPreview`；preview 仅用于获取 file hash。

- [ ] **Step 5: 重组删除逻辑**

新增 `confirmDeleteProvider` 和 `executeDelete`：

```tsx
const confirmDeleteProvider = (providerId: string) => {
  setDeleteTarget(providerId);
};

const executeDelete = async () => {
  if (!deleteTarget) return;
  setIsSaving(true);
  try {
    const tempDraft: PiProviderDraft = { ...emptyPiProviderDraft, providerId: deleteTarget };
    const preview = await piApi.previewProviderPatch(tempDraft);
    const result = await piApi.deleteProvider(deleteTarget, preview.currentFileHash);
    toast.success(t("pi.toast.deleted"), {
      description: t("pi.toast.savedDesc", { path: result.backupPath }),
    });
    setDraft({ ...emptyPiProviderDraft });
    setPreview(null);
    await refresh();
  } catch (error) {
    toast.error(t("pi.toast.deleteFailed"), {
      description: String(error),
    });
  } finally {
    setIsSaving(false);
    setDeleteTarget(null);
  }
};
```

- [ ] **Step 6: 修改 `duplicateProvider` 和 `testConnectivity`**

`duplicateProvider` 应进入编辑视图：

```tsx
const duplicateProvider = (providerId: string) => {
  editProvider(providerId);
  setDraft((prev) => ({
    ...prev,
    providerId: `${prev.providerId}-copy`,
  }));
};
```

`testConnectivity` 保持不变，但注意 toast 文案仍可复用。

- [ ] **Step 7: 重写 render 部分**

```tsx
return (
  <div className="px-6 pt-4 pb-12">
    {view === "list" ? (
      <PiProviderList
        providers={providers}
        onEdit={editProvider}
        onDuplicate={duplicateProvider}
        onDelete={confirmDeleteProvider}
        onTestConnectivity={testConnectivity}
      />
    ) : (
      <FullScreenPanel
        isOpen={view === "edit"}
        title={t("pi.editor.title", { defaultValue: "编辑供应商" })}
        onClose={() => setView("list")}
        footer={
          <Button
            type="button"
            onClick={() => void saveProvider()}
            disabled={isSaving}
            className="bg-primary text-primary-foreground hover:bg-primary/90"
          >
            <Save className="h-4 w-4 mr-2" />
            {t("common.save")}
          </Button>
        }
      >
        <div className="max-w-4xl mx-auto">
          <PiProviderForm value={draft} onChange={setDraft} />
        </div>
      </FullScreenPanel>
    )}

    <ConfirmDialog
      isOpen={deleteTarget !== null}
      title={t("pi.deleteConfirm.title", { defaultValue: "删除供应商" })}
      message={t("pi.deleteConfirm.message", {
        id: deleteTarget ?? "",
        defaultValue: `确定要删除供应商 "${deleteTarget}" 吗？此操作不可撤销。`,
      })}
      confirmText={t("common.delete")}
      variant="destructive"
      onConfirm={() => void executeDelete()}
      onCancel={() => setDeleteTarget(null)}
    />
  </div>
);
```

- [ ] **Step 8: 清理未使用的 `preview` 状态和 `PiProviderDiffPreview` 相关逻辑**

如果 `preview` 状态在 Task 4 Step 4 中仅用于获取 file hash，可以保留用于调试，也可以移除。建议保留 `preview` 局部变量在 `saveProvider` 内部，不再使用 `useState<PiProviderPatchPreview | null>`。

把：

```tsx
const [preview, setPreview] = useState<PiProviderPatchPreview | null>(null);
```

删除（或保留但不再 `setPreview`）。为简化，直接删除该 state，并在 `saveProvider` 中用局部变量。

同时删除 `buildPreview`、`applyPreview`、`deletePreview` 三个旧方法。

- [ ] **Step 9: 添加 i18n 键**

在 `src/i18n/locales/zh.json` 的 `pi` 下添加：

```json
"editor": {
  "title": "编辑供应商"
},
"deleteConfirm": {
  "title": "删除供应商",
  "message": "确定要删除供应商 \"{{id}}\" 吗？此操作不可撤销。"
},
"save": {
  "providerIdRequired": "请填写供应商标识"
}
```

同步到其他语言文件。

- [ ] **Step 10: 运行类型检查与 lint**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm tsc --noEmit`
Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm lint`
Expected: 无新增错误。

- [ ] **Step 11: 提交**

```bash
git add src/components/pi/PiAgentPanel.tsx src/i18n/locales/zh.json src/i18n/locales/en.json src/i18n/locales/ja.json src/i18n/locales/zh-TW.json
git commit -m "feat(pi): replace Pi provider tabs with list/edit split and one-click save"
```

---

## Task 5: 端到端验证

**Files:**

- 无文件修改

- [ ] **Step 1: 启动开发服务器并切换到 Pi 应用**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm dev`

在应用内切换到 Pi 应用（`pi`）。

- [ ] **Step 2: 验证列表页**

- 确认顶部没有 `providers / edit / review` tab。
- 确认「自定义提供商」标题已隐藏。
- 确认「内置提供商」标题仍存在（如果有内置 provider）。
- 确认卡片上的编辑、复制、测试、删除按钮可点击。

- [ ] **Step 3: 验证编辑页**

- 点击某个 provider 的编辑按钮。
- 确认进入全屏页面，顶部显示「编辑供应商」和返回按钮。
- 确认字段顺序为：Vendor Quick Select → API Template → Provider Configuration → Extra Options → Models → Compat → Config JSON。
- 修改字段，确认底部「配置 JSON」实时更新。
- 点击保存，确认 toast 成功，页面返回列表，列表数据已刷新。

- [ ] **Step 4: 验证新建 provider**

- 点击 Pi 应用右上角的橙色 + 按钮。
- 确认进入空的全屏编辑页。
- 填写信息并保存，确认新 provider 出现在列表中。

- [ ] **Step 5: 验证删除 provider**

- 点击某个 provider 卡片的删除按钮。
- 确认弹出二次确认弹窗。
- 确认删除后列表刷新，该 provider 消失。

- [ ] **Step 6: 运行测试**

Run: `cd /Users/linmaogui/VSCodeProjects/VSCodeProjects/LLM/Codex/cc-switch-pi/cc-switch && pnpm test`
Expected: 所有现有测试通过（Pi 相关测试在 `src-tauri/tests/pi_config.rs`，前端测试如有则通过）。

- [ ] **Step 7: 提交验证结果或修复**

如有问题，修复后提交：

```bash
git add <files>
git commit -m "fix(pi): address provider editor redesign issues"
```

---

## Self-Review

**1. Spec coverage:**

- 删除 tab 导航 → Task 4。
- 删除「自定义提供商」标题 → Task 3。
- 编辑页布局参考 OpenCode → Task 2 + Task 4。
- 底部显示配置 JSON → Task 2。
- 保存按钮 → Task 4。
- 一步保存替代 preview/apply 两步 → Task 4。

**2. Placeholder scan:**

- 无 TBD/TODO。
- 所有步骤包含具体代码或命令。
- i18n 键已给出默认值。

**3. Type consistency:**

- `JsonEditorProps.readOnly` 在 Task 1 定义，Task 2 使用。
- `PiProviderFormProps` 未变，Task 2 内部计算 `configJson`。
- `PiAgentPanel` 的 `view` 状态在 Task 4 定义并使用。

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-07-06-pi-provider-editor-redesign.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using `executing-plans`, batch execution with checkpoints

**Which approach?**
