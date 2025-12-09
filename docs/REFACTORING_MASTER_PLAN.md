# CC Switch 现代化重构完整方案

> Breaking Change 提醒（后续示例如仍出现 `app_type/appType` 字样，请按本规范理解与替换）：
>
> - 后端 Tauri 命令统一仅接受 `app` 参数（值：`claude` 或 `codex`），不再接受 `app_type`/`appType`。
> - 传入未知 `app` 会返回本地化错误，并提示“可选值: claude, codex”。
> - 前端与文档中的旧示例如包含 `app_type`，一律替换为 `{ app }`。

## 📋 目录

- [第一部分: 战略规划](#第一部分-战略规划)
  - [重构背景与目标](#重构背景与目标)
  - [当前问题全面分析](#当前问题全面分析)
  - [技术选型与理由](#技术选型与理由)
- [第二部分: 架构设计](#第二部分-架构设计)
  - [新的目录结构](#新的目录结构)
  - [数据流架构](#数据流架构)
  - [组件拆分详细方案](#组件拆分详细方案)
- [第三部分: 实施计划](#第三部分-实施计划)
  - [分阶段实施路线图](#分阶段实施路线图)
  - [详细实施步骤](#详细实施步骤)
- [第四部分: 质量保障](#第四部分-质量保障)
  - [测试策略](#测试策略)
  - [风险控制](#风险控制)
  - [回滚方案](#回滚方案)

---

# 第一部分: 战略规划

## 🎯 重构背景与目标

### 为什么要重构？

当前代码库存在以下核心问题：

1. **状态管理混乱**
   - 手动管理 20+ `useState`
   - 大量复杂的 `useEffect` 依赖链
   - 数据同步逻辑分散

2. **组件过于臃肿**
   - `SettingsModal.tsx`: **1046 行** 😱
   - `ProviderList.tsx`: **418 行**
   - `ProviderForm.tsx`: **271 行**

3. **代码重复严重**
   - 相似的数据获取逻辑在多个组件重复
   - 表单验证逻辑手动编写
   - 错误处理不统一

4. **UI 缺乏统一性**
   - 自定义样式分散
   - 缺乏设计系统
   - 响应式支持不足

5. **可维护性差**
   - 组件职责不清晰
   - 耦合度高
   - 难以测试

### 重构目标

| 维度           | 目标                 | 衡量标准       |
| -------------- | -------------------- | -------------- |
| **代码质量**   | 减少 40-60% 样板代码 | 代码行数统计   |
| **开发效率**   | 提升 50%+ 开发速度   | 新功能开发时间 |
| **用户体验**   | 统一设计系统         | UI 一致性检查  |
| **可维护性**   | 清晰的架构分层       | 代码审查时间   |
| **功能完整性** | 100% 功能无回归      | 全量测试通过   |

---

## 🔍 当前问题全面分析

### 问题 1: App.tsx - 状态管理混乱 (412行)

**现状**:

```typescript
// 10+ 个 useState，状态管理混乱
const [providers, setProviders] = useState<Record<string, Provider>>({})
const [currentProviderId, setCurrentProviderId] = useState<string>("")
const [notification, setNotification] = useState<{...} | null>(null)
const [isNotificationVisible, setIsNotificationVisible] = useState(false)
const [confirmDialog, setConfirmDialog] = useState<{...} | null>(null)
const [isSettingsOpen, setIsSettingsOpen] = useState(false)
const [isMcpOpen, setIsMcpOpen] = useState(false)
// ... 更多

// 手动数据加载，缺少 loading/error 状态
const loadProviders = async () => {
  const loadedProviders = await window.api.getProviders(activeApp)
  const currentId = await window.api.getCurrentProvider(activeApp)
  setProviders(loadedProviders)
  setCurrentProviderId(currentId)
}

// 复杂的 useEffect 依赖
useEffect(() => {
  loadProviders()
}, [activeApp])
```

**核心问题**:

- ❌ 状态同步困难
- ❌ 没有 loading/error 处理
- ❌ 错误处理不统一
- ❌ 组件责任过重

**目标**:

```typescript
// React Query: 3 行搞定
const { data, isLoading, error } = useProvidersQuery(activeApp);
const providers = data?.providers || {};
const currentProviderId = data?.currentProviderId || "";
```

---

### 问题 2: SettingsModal.tsx - 超级巨无霸组件 (1046行)

**现状结构**:

```
SettingsModal.tsx (1046 行)
├── 20+ useState (settings, configPath, version, isChecking...)
├── 15+ 处理函数
│   ├── loadSettings()
│   ├── saveSettings()
│   ├── handleLanguageChange()
│   ├── handleCheckUpdate()
│   ├── handleExportConfig()
│   ├── handleImportConfig()
│   ├── handleBrowseConfigDir()
│   └── ... 更多
├── 语言设置 UI
├── 窗口行为设置 UI
├── 配置文件位置 UI
├── 配置目录覆盖 UI (3个输入框)
├── 导入导出 UI
├── 关于和更新 UI
└── 2个子对话框 (ImportProgress, RestartConfirm)
```

**核心问题**:

- ❌ 单个文件超过 1000 行
- ❌ 多种职责混杂
- ❌ 难以理解和维护
- ❌ 无法并行开发
- ❌ 难以测试

**目标**: 拆分为 **7 个小组件** (~470 行总计)

---

### 问题 3: ProviderList.tsx - 内嵌组件和逻辑混杂 (418行)

**现状结构**:

```
ProviderList.tsx (418 行)
├── SortableProviderItem (内嵌子组件, ~100行)
├── 拖拽排序逻辑
├── 用量配置逻辑
├── URL 处理逻辑
├── Claude 插件同步逻辑
└── 空状态 UI
```

**核心问题**:

- ❌ 内嵌组件导致代码难读
- ❌ 拖拽逻辑和 UI 混在一起
- ❌ 业务逻辑分散

**目标**: 拆分为 **4 个独立组件** + **1 个自定义 Hook**

---

### 问题 4: tauri-api.ts - 全局污染 (712行)

**现状**:

```typescript
// 问题 1: 污染全局命名空间
if (typeof window !== "undefined") {
  (window as any).api = tauriAPI;
}

// 问题 2: 无缓存机制
getProviders: async (app?: AppId) => {
  try {
    return await invoke("get_providers", { app });
  } catch (error) {
    console.error("获取供应商列表失败:", error);
    return {}; // 错误被吞掉
  }
};
```

**核心问题**:

- ❌ 全局 `window.api` 污染命名空间
- ❌ 无缓存，重复请求
- ❌ 无自动重试
- ❌ 错误处理不统一

**目标**:

- 封装为 API 层 (`lib/api/`)
- React Query 管理缓存和状态

---

### 问题 5: 表单验证 - 手动编写 (ProviderForm.tsx)

**现状**:

```typescript
const [name, setName] = useState("");
const [nameError, setNameError] = useState("");
const [apiKey, setApiKey] = useState("");
const [apiKeyError, setApiKeyError] = useState("");

const validate = () => {
  let valid = true;
  if (!name) {
    setNameError("请填写名称");
    valid = false;
  } else {
    setNameError("");
  }
  if (!apiKey) {
    setApiKeyError("请填写 API Key");
    valid = false;
  } else if (apiKey.length < 10) {
    setApiKeyError("API Key 长度不足");
    valid = false;
  } else {
    setApiKeyError("");
  }
  return valid;
};
```

**核心问题**:

- ❌ 每个字段需要 2 个 state (值 + 错误)
- ❌ 验证逻辑手动编写
- ❌ 代码冗长

**目标**: 使用 `react-hook-form` + `zod`

```typescript
const schema = z.object({
  name: z.string().min(1, "请填写名称"),
  apiKey: z.string().min(10, "API Key 长度不足"),
});

const form = useForm({ resolver: zodResolver(schema) });
```

---

## 🛠 技术选型与理由

### 核心技术栈

| 技术                      | 版本    | 用途           | 替代方案        | 为何选它？           |
| ------------------------- | ------- | -------------- | --------------- | -------------------- |
| **@tanstack/react-query** | ^5.90.2 | 服务端状态管理 | SWR, RTK Query  | 功能最全，生态最好   |
| **react-hook-form**       | ^7.63.0 | 表单管理       | Formik          | 性能更好，API 更简洁 |
| **zod**                   | ^4.1.11 | 运行时类型验证 | yup, joi        | TypeScript 原生支持  |
| **shadcn/ui**             | latest  | UI 组件库      | Radix UI 原生   | 可定制，代码归属权   |
| **sonner**                | ^2.0.7  | Toast 通知     | react-hot-toast | 更现代，动画更好     |
| **next-themes**           | ^0.4.6  | 主题管理       | 自定义实现      | 开箱即用，SSR 友好   |

---

# 第二部分: 架构设计

## 📁 新的目录结构

### 完整目录树

```
src/
├── components/
│   ├── ui/                           # shadcn/ui 基础组件 (由 CLI 生成)
│   │   ├── button.tsx
│   │   ├── dialog.tsx
│   │   ├── input.tsx
│   │   ├── label.tsx
│   │   ├── form.tsx
│   │   ├── select.tsx
│   │   ├── switch.tsx
│   │   ├── tabs.tsx
│   │   ├── card.tsx
│   │   ├── badge.tsx
│   │   └── sonner.tsx               # Toast 组件
│   │
│   ├── providers/                    # 供应商管理模块
│   │   ├── ProviderList.tsx         # 列表容器 (~100行)
│   │   ├── ProviderCard.tsx         # 供应商卡片 (~120行)
│   │   ├── ProviderActions.tsx      # 操作按钮组 (~80行)
│   │   ├── ProviderEmptyState.tsx   # 空状态 (~30行)
│   │   ├── AddProviderDialog.tsx    # 添加对话框 (~60行)
│   │   ├── EditProviderDialog.tsx   # 编辑对话框 (~60行)
│   │   └── forms/                   # 表单子模块
│   │       ├── ProviderForm.tsx     # 主表单 (~150行)
│   │       ├── PresetSelector.tsx   # 预设选择器 (~60行)
│   │       ├── ApiKeyInput.tsx      # API Key 输入 (~40行)
│   │       ├── ConfigEditor.tsx     # 配置编辑器 (~80行)
│   │       └── KimiModelSelector.tsx # Kimi 模型选择器 (~40行)
│   │
│   ├── settings/                     # 设置管理模块 (拆分自 SettingsModal)
│   │   ├── SettingsDialog.tsx       # 设置对话框容器 (~80行)
│   │   ├── LanguageSettings.tsx     # 语言设置 (~40行)
│   │   ├── WindowSettings.tsx       # 窗口行为设置 (~50行)
│   │   ├── ConfigPathDisplay.tsx    # 配置路径显示 (~40行)
│   │   ├── DirectorySettings/       # 目录设置子模块
│   │   │   ├── index.tsx            # 目录设置容器 (~60行)
│   │   │   └── DirectoryInput.tsx   # 单个目录输入组件 (~50行)
│   │   ├── ImportExportSection.tsx  # 导入导出 (~120行)
│   │   ├── AboutSection.tsx         # 关于和更新 (~100行)
│   │   └── RestartDialog.tsx        # 重启确认对话框 (~40行)
│   │
│   ├── usage/                        # 用量查询模块
│   │   ├── UsageFooter.tsx          # 用量信息展示
│   │   ├── UsageScriptModal.tsx     # 用量脚本配置
│   │   └── UsageEditor.tsx          # 脚本编辑器
│   │
│   ├── mcp/                          # MCP 管理模块
│   │   ├── McpPanel.tsx             # MCP 管理面板
│   │   ├── McpList.tsx              # MCP 列表
│   │   ├── McpForm.tsx              # MCP 表单
│   │   └── McpTemplates.tsx         # MCP 模板选择
│   │
│   ├── shared/                       # 共享组件
│   │   ├── AppSwitcher.tsx          # Claude/Codex 切换器
│   │   ├── ConfirmDialog.tsx        # 确认对话框
│   │   ├── UpdateBadge.tsx          # 更新徽章
│   │   ├── JsonEditor.tsx           # JSON 编辑器
│   │   ├── BrandIcons.tsx           # 品牌图标
│   │   └── ImportProgressModal.tsx  # 导入进度
│   │
│   ├── theme-provider.tsx           # 主题 Provider
│   └── mode-toggle.tsx              # 主题切换按钮
│
├── hooks/                            # 自定义 Hooks (业务逻辑层)
│   ├── useSettings.ts               # 设置管理逻辑
│   ├── useImportExport.ts           # 导入导出逻辑
│   ├── useDragSort.ts               # 拖拽排序逻辑
│   ├── useProviderActions.ts        # 供应商操作 (可选)
│   ├── useVSCodeSync.ts             # VS Code 同步
│   ├── useClaudePlugin.ts           # Claude 插件管理
│   └── useAppVersion.ts             # 版本信息
│
├── lib/
│   ├── query/                        # React Query 层
│   │   ├── index.ts                 # 导出所有 hooks
│   │   ├── queryClient.ts           # QueryClient 配置
│   │   ├── queries.ts               # 所有查询 hooks
│   │   └── mutations.ts             # 所有变更 hooks
│   │
│   ├── api/                          # API 调用层 (封装 Tauri invoke)
│   │   ├── providers.ts             # 供应商 API
│   │   ├── settings.ts              # 设置 API
│   │   ├── mcp.ts                   # MCP API
│   │   ├── usage.ts                 # 用量查询 API
│   │   ├── vscode.ts                # VS Code API
│   │   └── index.ts                 # 聚合导出
│   │
│   ├── schemas/                      # Zod 验证 Schemas
│   │   ├── provider.ts              # 供应商验证规则
│   │   ├── settings.ts              # 设置验证规则
│   │   └── mcp.ts                   # MCP 验证规则
│   │
│   ├── utils/                        # 工具函数
│   │   ├── errorHandling.ts         # 错误处理
│   │   ├── providerUtils.ts         # 供应商工具
│   │   └── configUtils.ts           # 配置工具
│   │
│   └── utils.ts                      # shadcn/ui 工具函数 (cn)
│
├── types/                            # TypeScript 类型定义
│   └── index.ts
│
├── contexts/                         # React Contexts (保留现有)
│   └── UpdateContext.tsx            # 更新管理 Context
│
├── i18n/                             # 国际化 (保留现有)
│   ├── index.ts
│   └── locales/
│
├── App.tsx                           # 主应用组件 (简化到 ~100行)
├── main.tsx                          # 入口文件 (添加 Providers)
└── index.css                         # 全局样式
```

### 目录结构设计原则

1. **按功能模块分组** (providers/, settings/, mcp/)
2. **按技术层次分层** (components/, hooks/, lib/)
3. **UI 组件独立** (ui/ 目录)
4. **业务逻辑提取** (hooks/ 目录)
5. **数据层封装** (api/ 目录)

---

## 🏗 数据流架构

### 分层架构图

```
┌─────────────────────────────────────────┐
│           UI 层 (Components)            │
│  ProviderList, SettingsDialog, etc.   │
└────────────────┬────────────────────────┘
                 │ 使用
                 ↓
┌─────────────────────────────────────────┐
│      业务逻辑层 (Custom Hooks)          │
│  useSettings, useDragSort, etc.        │
└────────────────┬────────────────────────┘
                 │ 调用
                 ↓
┌─────────────────────────────────────────┐
│    数据管理层 (React Query Hooks)      │
│  useProvidersQuery, useMutation, etc.  │
└────────────────┬────────────────────────┘
                 │ 调用
                 ↓
┌─────────────────────────────────────────┐
│        API 层 (API Functions)          │
│  providersApi, settingsApi, etc.       │
└────────────────┬────────────────────────┘
                 │ invoke
                 ↓
┌─────────────────────────────────────────┐
│      Tauri Backend (Rust)              │
│  Commands, State, File System          │
└─────────────────────────────────────────┘
```

### 数据流示例

**场景**: 切换供应商

```
1. 用户点击按钮
   ↓
2. ProviderCard 调用 onClick={() => switchMutation.mutate(id)}
   ↓
3. useSwitchProviderMutation (lib/query/mutations.ts)
   - mutationFn: 调用 providersApi.switch(id, appType)
   ↓
4. providersApi.switch (lib/api/providers.ts)
   - 调用 invoke('switch_provider', { id, app })
   ↓
5. Tauri Backend (Rust)
   - 执行切换逻辑
   - 更新配置文件
   - 返回结果
   ↓
6. useSwitchProviderMutation
   - onSuccess: invalidateQueries(['providers', appType])
   - onSuccess: updateTrayMenu()
   - onSuccess: toast.success('切换成功')
   ↓
7. useProvidersQuery 自动重新获取数据
   ↓
8. UI 自动更新
```

### 关键设计原则

1. **单一职责**: 每层只做一件事
2. **依赖倒置**: UI 依赖抽象 (hooks)，不依赖具体实现
3. **开闭原则**: 易于扩展，无需修改现有代码
4. **状态分离**:
   - 服务端状态 → React Query
   - 客户端 UI 状态 → useState
   - 全局状态 → Context

---

## 🔧 组件拆分详细方案

### 拆分策略: SettingsModal (1046行 → 7个组件)

#### 拆分前后对比

```
┌───────────────────────────────────┐
│   SettingsModal.tsx (1046 行)    │  ❌ 过于臃肿
│                                   │
│  - 20+ useState                   │
│  - 15+ 函数                       │
│  - 600+ 行 JSX                    │
│  - 难以理解和维护                  │
└───────────────────────────────────┘

                ↓ 重构

┌─────────────────────────────────────────────────┐
│       settings/ 模块 (7个组件, ~470行)          │
│                                                 │
│  ├── SettingsDialog.tsx (容器, ~80行)          │
│  │   └── 使用 useSettings hook                 │
│  │                                              │
│  ├── LanguageSettings.tsx (~40行)              │
│  ├── WindowSettings.tsx (~50行)                │
│  ├── ConfigPathDisplay.tsx (~40行)             │
│  ├── DirectorySettings/ (~110行)               │
│  │   ├── index.tsx (~60行)                     │
│  │   └── DirectoryInput.tsx (~50行)            │
│  ├── ImportExportSection.tsx (~120行)          │
│  │   └── 使用 useImportExport hook             │
│  └── AboutSection.tsx (~100行)                 │
│      └── 使用 useAppVersion, useUpdate hooks  │
└─────────────────────────────────────────────────┘

✅ 每个组件 30-120 行
✅ 职责清晰
✅ 易于测试
✅ 可独立开发
```

#### 拆分详细方案

**1. SettingsDialog.tsx (容器组件, ~80行)**

职责: 组织整体布局，协调子组件

```typescript
import { LanguageSettings } from './LanguageSettings'
import { WindowSettings } from './WindowSettings'
import { DirectorySettings } from './DirectorySettings'
import { ImportExportSection } from './ImportExportSection'
import { AboutSection } from './AboutSection'
import { useSettings } from '@/hooks/useSettings'

export function SettingsDialog({ open, onOpenChange }) {
  const { settings, updateSettings, saveSettings, isPending } = useSettings()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh]">
        <DialogHeader>
          <DialogTitle>设置</DialogTitle>
        </DialogHeader>

        <Tabs defaultValue="general">
          <TabsList className="grid w-full grid-cols-3">
            <TabsTrigger value="general">通用</TabsTrigger>
            <TabsTrigger value="advanced">高级</TabsTrigger>
            <TabsTrigger value="about">关于</TabsTrigger>
          </TabsList>

          <TabsContent value="general" className="space-y-4">
            <LanguageSettings
              value={settings.language}
              onChange={(lang) => updateSettings({ language: lang })}
            />
            <WindowSettings settings={settings} onChange={updateSettings} />
            <ConfigPathDisplay />
          </TabsContent>

          <TabsContent value="advanced" className="space-y-4">
            <DirectorySettings settings={settings} onChange={updateSettings} />
            <ImportExportSection />
          </TabsContent>

          <TabsContent value="about">
            <AboutSection />
          </TabsContent>
        </Tabs>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={saveSettings} disabled={isPending}>
            {isPending ? '保存中...' : '保存'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

**2. LanguageSettings.tsx (~40行)**

职责: 语言切换 UI

```typescript
interface LanguageSettingsProps {
  value: 'zh' | 'en'
  onChange: (lang: 'zh' | 'en') => void
}

export function LanguageSettings({ value, onChange }: LanguageSettingsProps) {
  return (
    <div>
      <h3 className="text-sm font-medium mb-3">语言设置</h3>
      <div className="inline-flex p-0.5 bg-gray-100 dark:bg-gray-800 rounded-lg">
        <Button
          variant={value === 'zh' ? 'default' : 'ghost'}
          size="sm"
          onClick={() => onChange('zh')}
        >
          中文
        </Button>
        <Button
          variant={value === 'en' ? 'default' : 'ghost'}
          size="sm"
          onClick={() => onChange('en')}
        >
          English
        </Button>
      </div>
    </div>
  )
}
```

**3. DirectoryInput.tsx (~50行)**

职责: 可复用的目录选择输入框

```typescript
import { FolderSearch, Undo2 } from 'lucide-react'

interface DirectoryInputProps {
  label: string
  description?: string
  value?: string
  onChange: (value: string | undefined) => void
  type: 'app' | 'claude' | 'codex'
}

export function DirectoryInput({ label, description, value, onChange }: DirectoryInputProps) {
  const handleBrowse = async () => {
    const selected = await window.api.selectConfigDirectory(value)
    if (selected) onChange(selected)
  }

  const handleReset = () => {
    onChange(undefined)
  }

  return (
    <div>
      <Label className="text-xs">{label}</Label>
      {description && <p className="text-xs text-muted-foreground mb-1">{description}</p>}
      <div className="flex gap-2">
        <Input
          value={value || ''}
          onChange={(e) => onChange(e.target.value)}
          className="flex-1 font-mono text-xs"
        />
        <Button variant="outline" size="icon" onClick={handleBrowse}>
          <FolderSearch className="h-4 w-4" />
        </Button>
        <Button variant="outline" size="icon" onClick={handleReset}>
          <Undo2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  )
}
```

**4. useSettings Hook (业务逻辑提取)**

```typescript
export function useSettings() {
  const queryClient = useQueryClient();

  // 获取设置
  const { data: settings, isLoading } = useQuery({
    queryKey: ["settings"],
    queryFn: async () => await settingsApi.get(),
  });

  // 保存设置
  const saveMutation = useMutation({
    mutationFn: async (newSettings: Settings) =>
      await settingsApi.save(newSettings),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      toast.success("设置已保存");
    },
  });

  // 本地临时状态 (保存前)
  const [localSettings, setLocalSettings] = useState<Settings | null>(null);
  const currentSettings = localSettings || settings || {};

  return {
    settings: currentSettings,
    updateSettings: (updates: Partial<Settings>) => {
      setLocalSettings((prev) => ({ ...prev, ...updates }));
    },
    saveSettings: () => {
      if (localSettings) saveMutation.mutate(localSettings);
    },
    resetSettings: () => setLocalSettings(null),
    isPending: saveMutation.isPending,
    isLoading,
  };
}
```

---

### 拆分策略: ProviderList (418行 → 4个组件 + 1个Hook)

#### 拆分方案

```
ProviderList.tsx (418 行)  ❌ 内嵌组件、逻辑混杂

        ↓ 重构

providers/ 模块 (4个组件 + 1个Hook, ~330行)

├── ProviderList.tsx (容器, ~100行)
│   └── 使用 useDragSort hook
│
├── ProviderCard.tsx (~120行)
│   └── 显示单个供应商信息
│
├── ProviderActions.tsx (~80行)
│   └── 操作按钮组 (switch, edit, delete, usage)
│
├── ProviderEmptyState.tsx (~30行)
│   └── 空状态提示
│
└── hooks/useDragSort.ts (~100行)
    └── 拖拽排序逻辑
```

#### 代码示例

**ProviderList.tsx (容器)**

```typescript
import { ProviderCard } from './ProviderCard'
import { ProviderEmptyState } from './ProviderEmptyState'
import { useDragSort } from '@/hooks/useDragSort'

export function ProviderList({ providers, currentProviderId, appType }) {
  const { sortedProviders, handleDragEnd, sensors } = useDragSort(providers, appType)

  if (sortedProviders.length === 0) {
    return <ProviderEmptyState />
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={handleDragEnd}
    >
      <SortableContext
        items={sortedProviders.map(p => p.id)}
        strategy={verticalListSortingStrategy}
      >
        <div className="space-y-3">
          {sortedProviders.map(provider => (
            <ProviderCard
              key={provider.id}
              provider={provider}
              isCurrent={provider.id === currentProviderId}
              appType={appType}
            />
          ))}
        </div>
      </SortableContext>
    </DndContext>
  )
}
```

**useDragSort.ts (逻辑提取)**

```typescript
export function useDragSort(
  providers: Record<string, Provider>,
  appType: AppId
) {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // 排序逻辑
  const sortedProviders = useMemo(() => {
    return Object.values(providers).sort((a, b) => {
      if (a.sortIndex !== undefined && b.sortIndex !== undefined) {
        return a.sortIndex - b.sortIndex;
      }
      const timeA = a.createdAt || 0;
      const timeB = b.createdAt || 0;
      if (timeA === 0 && timeB === 0) {
        return a.name.localeCompare(b.name, "zh-CN");
      }
      return timeA === 0 ? -1 : timeB === 0 ? 1 : timeA - timeB;
    });
  }, [providers]);

  // 拖拽传感器
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor)
  );

  // 拖拽结束处理
  const handleDragEnd = useCallback(
    async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;

      const oldIndex = sortedProviders.findIndex((p) => p.id === active.id);
      const newIndex = sortedProviders.findIndex((p) => p.id === over.id);

      const reordered = arrayMove(sortedProviders, oldIndex, newIndex);
      const updates = reordered.map((p, i) => ({ id: p.id, sortIndex: i }));

      try {
        await providersApi.updateSortOrder(updates, appType);
        queryClient.invalidateQueries({ queryKey: ["providers", appType] });
        toast.success(t("provider.sortUpdated"));
      } catch (error) {
        toast.error(t("provider.sortUpdateFailed"));
      }
    },
    [sortedProviders, appType, queryClient, t]
  );

  return { sortedProviders, sensors, handleDragEnd };
}
```

---

### 代码量对比总结

| 组件                 | 重构前     | 重构后           | 变化     |
| -------------------- | ---------- | ---------------- | -------- |
| **SettingsModal**    | 1046 行    | 7个组件 ~470行   | **-55%** |
| **ProviderList**     | 418 行     | 4个组件 ~330行   | **-21%** |
| **业务逻辑 (Hooks)** | 混在组件中 | 5个 hooks ~400行 | 提取独立 |
| **总计**             | 1464 行    | ~1200 行         | **-18%** |

**注意**: 代码总量略有减少，但**可维护性大幅提升**：

- ✅ 每个文件 30-120 行，易于理解
- ✅ 关注点分离，职责清晰
- ✅ 业务逻辑可复用
- ✅ 易于测试和调试

---

# 第三部分: 实施计划

## 📅 分阶段实施路线图

### 总览

| 阶段       | 目标           | 工期         | 产出                         |
| ---------- | -------------- | ------------ | ---------------------------- |
| **阶段 0** | 准备环境       | 1 天         | 依赖安装、配置完成           |
| **阶段 1** | 搭建基础设施（✅ 已完成） | 2-3 天       | API 层、Query Hooks 完成     |
| **阶段 2** | 重构核心功能（✅ 已完成） | 3-4 天       | App.tsx、ProviderList 完成   |
| **阶段 3** | 重构设置和辅助（✅ 已完成） | 2-3 天       | SettingsDialog、通知系统完成 |
| **阶段 4** | 清理和优化     | 1-2 天       | 旧代码删除、优化完成         |
| **阶段 5** | 测试和修复     | 2-3 天       | 测试通过、Bug 修复           |
| **总计**   | -              | **11-16 天** | v4.0.0 发布                  |

---

### 阶段 0: 准备阶段 (1天)

**目标**: 环境准备和依赖安装

#### 任务清单

- [ ] 创建新分支 `refactor/modernization`
- [ ] 创建备份标签 `git tag backup-before-refactor`
- [ ] 安装核心依赖
- [ ] 配置 shadcn/ui
- [ ] 配置 TypeScript 路径别名
- [ ] 配置 Vite 路径解析
- [ ] 验证开发服务器启动

#### 详细步骤

**1. 创建分支和备份**

```bash
# 创建新分支
git checkout -b refactor/modernization

# 创建备份标签
git tag backup-before-refactor

# 推送标签到远程 (可选)
git push origin backup-before-refactor
```

**2. 安装依赖**

```bash
# 核心依赖
pnpm add @tanstack/react-query
pnpm add react-hook-form @hookform/resolvers
pnpm add zod
pnpm add sonner
pnpm add next-themes

# Radix UI 组件 (shadcn/ui 依赖)
pnpm add @radix-ui/react-dialog
pnpm add @radix-ui/react-dropdown-menu
pnpm add @radix-ui/react-label
pnpm add @radix-ui/react-select
pnpm add @radix-ui/react-slot
pnpm add @radix-ui/react-switch
pnpm add @radix-ui/react-tabs
pnpm add @radix-ui/react-checkbox

# 样式工具
pnpm add class-variance-authority
pnpm add clsx
pnpm add tailwind-merge
```

**3. 创建 `components.json`**

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "default",
  "rsc": false,
  "tsx": true,
  "tailwind": {
    "config": "tailwind.config.js",
    "css": "src/index.css",
    "baseColor": "neutral",
    "cssVariables": true,
    "prefix": ""
  },
  "iconLibrary": "lucide",
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils",
    "ui": "@/components/ui",
    "lib": "@/lib",
    "hooks": "@/hooks"
  }
}
```

**4. 更新 `tsconfig.json`**

```json
{
  "compilerOptions": {
    // ... 现有配置
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"]
    }
  }
}
```

**5. 更新 `vite.config.mts`**

```typescript
import path from "path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
```

**6. 验证**

```bash
pnpm dev  # 确保开发服务器正常启动
pnpm typecheck  # 确保类型检查通过
```

---

### 阶段 1: 基础设施 (2-3天)

**目标**: 搭建新架构的基础层

#### 任务清单

- [x] 创建工具函数 (`lib/utils.ts`)
- [x] 添加基础 UI 组件 (Button, Dialog, Input, Form 等)
- [x] 创建 Query Client 配置
- [x] 封装 API 层 (providers, settings, mcp)
- [x] 创建 Query Hooks (queries, mutations)
- [x] 创建 Zod Schemas

#### 详细步骤

**Step 1.1: 创建 `src/lib/utils.ts`**

```typescript
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
```

**Step 1.2: 添加 shadcn/ui 基础组件**

创建 `src/components/ui/button.tsx`:

```typescript
import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground shadow hover:bg-primary/90",
        destructive: "bg-destructive text-destructive-foreground shadow-sm hover:bg-destructive/90",
        outline: "border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground",
        secondary: "bg-secondary text-secondary-foreground shadow-sm hover:bg-secondary/80",
        ghost: "hover:bg-accent hover:text-accent-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-9 px-4 py-2",
        sm: "h-8 rounded-md px-3 text-xs",
        lg: "h-10 rounded-md px-8",
        icon: "h-9 w-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button"
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    )
  }
)
Button.displayName = "Button"

export { Button, buttonVariants }
```

类似地创建:

- `dialog.tsx`
- `input.tsx`
- `label.tsx`
- `form.tsx`
- `select.tsx`
- `switch.tsx`
- `tabs.tsx`
- `textarea.tsx`
- `sonner.tsx`

**参考**: https://ui.shadcn.com/docs/components

**Step 1.3: 创建 Query Client**

`src/lib/query/queryClient.ts`:

```typescript
import { QueryClient } from "@tanstack/react-query";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
      staleTime: 1000 * 60 * 5, // 5 分钟
    },
    mutations: {
      retry: false,
    },
  },
});
```

**Step 1.4: 封装 API 层**

`src/lib/api/providers.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { Provider } from "@/types";
import type { AppId } from "@/lib/api";

export const providersApi = {
  getAll: async (appId: AppId): Promise<Record<string, Provider>> => {
    return await invoke("get_providers", { app: appId });
  },

  getCurrent: async (appId: AppId): Promise<string> => {
    return await invoke("get_current_provider", { app: appId });
  },

  add: async (provider: Provider, appId: AppId): Promise<boolean> => {
    return await invoke("add_provider", { provider, app: appId });
  },

  update: async (provider: Provider, appId: AppId): Promise<boolean> => {
    return await invoke("update_provider", { provider, app: appId });
  },

  delete: async (id: string, appId: AppId): Promise<boolean> => {
    return await invoke("delete_provider", { id, app: appId });
  },

  switch: async (id: string, appId: AppId): Promise<boolean> => {
    return await invoke("switch_provider", { id, app: appId });
  },

  importDefault: async (appId: AppId): Promise<boolean> => {
    return await invoke("import_default_config", { app: appId });
  },

  updateTrayMenu: async (): Promise<boolean> => {
    return await invoke("update_tray_menu");
  },

  updateSortOrder: async (
    updates: Array<{ id: string; sortIndex: number }>,
    appId: AppId
  ): Promise<boolean> => {
    return await invoke("update_providers_sort_order", { updates, app: appId });
  },
};
```

类似地创建:

- `src/lib/api/settings.ts`
- `src/lib/api/mcp.ts`
- `src/lib/api/index.ts` (聚合导出)

**Step 1.5: 创建 Query Hooks**

`src/lib/query/queries.ts`:

```typescript
import { useQuery } from "@tanstack/react-query";
import { providersApi, type AppId } from "@/lib/api";
import { Provider } from "@/types";

// 排序辅助函数
const sortProviders = (
  providers: Record<string, Provider>
): Record<string, Provider> => {
  return Object.fromEntries(
    Object.values(providers)
      .sort((a, b) => {
        const timeA = a.createdAt || 0;
        const timeB = b.createdAt || 0;
        if (timeA === 0 && timeB === 0) {
          return a.name.localeCompare(b.name, "zh-CN");
        }
        if (timeA === 0) return -1;
        if (timeB === 0) return 1;
        return timeA - timeB;
      })
      .map((provider) => [provider.id, provider])
  );
};

export const useProvidersQuery = (appType: AppId) => {
  return useQuery({
    queryKey: ["providers", appType],
    queryFn: async () => {
      let providers: Record<string, Provider> = {};
      let currentProviderId = "";

      try {
        providers = await providersApi.getAll(appType);
      } catch (error) {
        console.error("获取供应商列表失败:", error);
      }

      try {
        currentProviderId = await providersApi.getCurrent(appType);
      } catch (error) {
        console.error("获取当前供应商失败:", error);
      }

      // 自动导入默认配置
      if (Object.keys(providers).length === 0) {
        try {
          const success = await providersApi.importDefault(appType);
          if (success) {
            providers = await providersApi.getAll(appType);
            currentProviderId = await providersApi.getCurrent(appType);
          }
        } catch (error) {
          console.error("导入默认配置失败:", error);
        }
      }

      return { providers: sortProviders(providers), currentProviderId };
    },
  });
};
```

`src/lib/query/mutations.ts`:

```typescript
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { providersApi, type AppId } from "@/lib/api";
import { Provider } from "@/types";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

export const useAddProviderMutation = (appType: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (provider: Omit<Provider, "id">) => {
      const newProvider: Provider = {
        ...provider,
        id: crypto.randomUUID(),
        createdAt: Date.now(),
      };
      await providersApi.add(newProvider, appType);
      return newProvider;
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", appType] });
      await providersApi.updateTrayMenu();
      toast.success(t("notifications.providerAdded"));
    },
    onError: (error: Error) => {
      toast.error(t("notifications.addFailed", { error: error.message }));
    },
  });
};

export const useSwitchProviderMutation = (appType: AppId) => {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  return useMutation({
    mutationFn: async (providerId: string) => {
      return await providersApi.switch(providerId, appType);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["providers", appType] });
      await providersApi.updateTrayMenu();
      toast.success(
        t("notifications.switchSuccess", { appName: t(`apps.${appType}`) })
      );
    },
    onError: (error: Error) => {
      toast.error(t("notifications.switchFailed") + ": " + error.message);
    },
  });
};

// 类似地创建: useDeleteProviderMutation, useUpdateProviderMutation
```

**Step 1.6: 创建 Zod Schemas**

`src/lib/schemas/provider.ts`:

```typescript
import { z } from "zod";

export const providerSchema = z.object({
  name: z.string().min(1, "请填写供应商名称"),
  websiteUrl: z.string().url("请输入有效的网址").optional().or(z.literal("")),
  settingsConfig: z
    .string()
    .min(1, "请填写配置内容")
    .refine(
      (val) => {
        try {
          JSON.parse(val);
          return true;
        } catch {
          return false;
        }
      },
      { message: "配置 JSON 格式错误" }
    ),
});

export type ProviderFormData = z.infer<typeof providerSchema>;
```

---

### 阶段 2: 核心功能重构 (3-4天)

**目标**: 重构 App.tsx 和供应商管理

#### 任务清单

- [x] 更新 `main.tsx` (添加 Providers)
- [x] 创建主题 Provider
- [x] 重构 `App.tsx` (412行 → ~100行)
- [x] 拆分 ProviderList (4个组件)
- [x] 创建 `useDragSort` Hook
- [x] 重构表单组件 (使用 react-hook-form)
- [x] 创建 AddProvider / EditProvider Dialog

#### 详细步骤

**Step 2.1: 更新 `main.tsx`**

```typescript
import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { UpdateProvider } from './contexts/UpdateContext'
import './index.css'
import './i18n'
import { QueryClientProvider } from '@tanstack/react-query'
import { queryClient } from '@/lib/query'
import { ThemeProvider } from '@/components/theme-provider'
import { Toaster } from '@/components/ui/sonner'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
        <UpdateProvider>
          <App />
          <Toaster />
        </UpdateProvider>
      </ThemeProvider>
    </QueryClientProvider>
  </React.StrictMode>
)
```

**Step 2.2: 创建 `theme-provider.tsx`**

```typescript
import { createContext, useContext, useEffect, useState } from 'react'

type Theme = 'dark' | 'light' | 'system'

type ThemeProviderProps = {
  children: React.ReactNode
  defaultTheme?: Theme
  storageKey?: string
}

type ThemeProviderState = {
  theme: Theme
  setTheme: (theme: Theme) => void
}

const ThemeProviderContext = createContext<ThemeProviderState>({
  theme: 'system',
  setTheme: () => null,
})

export function ThemeProvider({
  children,
  defaultTheme = 'system',
  storageKey = 'ui-theme',
  ...props
}: ThemeProviderProps) {
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem(storageKey) as Theme) || defaultTheme
  )

  useEffect(() => {
    const root = window.document.documentElement
    root.classList.remove('light', 'dark')

    if (theme === 'system') {
      const systemTheme = window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      root.classList.add(systemTheme)
      return
    }

    root.classList.add(theme)
  }, [theme])

  const value = {
    theme,
    setTheme: (theme: Theme) => {
      localStorage.setItem(storageKey, theme)
      setTheme(theme)
    },
  }

  return (
    <ThemeProviderContext.Provider {...props} value={value}>
      {children}
    </ThemeProviderContext.Provider>
  )
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext)
  if (context === undefined) {
    throw new Error('useTheme must be used within a ThemeProvider')
  }
  return context
}
```

**Step 2.3: 重构 `App.tsx`**

(参考前面的代码示例，从 412 行简化到 ~100 行)

**Step 2.4-2.7: 拆分 ProviderList**

(参考前面的组件拆分详细方案)

---

### 阶段 3: 设置和辅助功能 (2-3天)

**目标**: 重构设置模块和通知系统

#### 任务清单

- [x] 拆分 SettingsDialog (7个组件)
- [x] 创建 `useSettings` Hook
- [x] 创建 `useImportExport` Hook
- [x] 替换通知系统为 Sonner
- [x] 重构 ConfirmDialog

#### 详细步骤

(参考前面的组件拆分详细方案)

---

### 阶段 4: 清理和优化 (1-2天)

**目标**: 清理旧代码，优化性能

#### 任务清单

- [x] 删除 `lib/styles.ts`
- [x] 删除旧的 Modal 组件
- [x] 移除 `window.api` 全局绑定
- [x] 清理无用的 state 和函数
- [x] 更新类型定义
- [x] 代码格式化
- [x] TypeScript 检查

---

### 阶段 5: 测试和修复 (2-3天)

**目标**: 全面测试，修复 Bug

#### 功能测试清单

- [ ] 添加供应商 (Claude/Codex)
- [ ] 编辑供应商
- [ ] 删除供应商
- [ ] 切换供应商
- [ ] 拖拽排序
- [ ] 设置保存
- [ ] 导入导出配置
- [ ] 主题切换
- [ ] MCP 管理
- [ ] 用量查询
- [ ] 托盘菜单同步

#### 边界情况测试

- [ ] 空供应商列表
- [ ] 网络错误
- [ ] 表单验证
- [ ] 并发操作
- [ ] 大量数据 (100+ 供应商)

---

# 第四部分: 质量保障

## 🧪 测试策略

### 手动测试

每完成一个阶段后进行全量功能测试。

### 自动化测试 (可选)

可以考虑添加:

- Vitest 单元测试 (hooks, utils)
- Testing Library 组件测试

---

## 🚨 风险控制

### 潜在风险

1. **功能回归**: 重构可能引入 bug
2. **用户数据丢失**: 配置文件操作失败
3. **性能下降**: 新架构可能影响性能
4. **兼容性问题**: 依赖库平台兼容性

### 缓解措施

1. **逐步重构**: 按阶段进行，每阶段后测试
2. **保留备份**: Git tag + 配置文件备份
3. **Beta 测试**: 先发布 beta 版本
4. **回滚方案**: 准备快速回滚机制

---

## ⏪ 回滚方案

### 如果需要回滚

```bash
# 方案 1: 回到重构前
git reset --hard backup-before-refactor

# 方案 2: 创建回滚分支
git checkout -b rollback-refactor
git revert <commit-range>
```

### 用户数据保护

在重构前自动备份配置:

```rust
// Rust 后端
fn backup_config_before_refactor() -> Result<()> {
    let config_path = get_app_config_path()?;
    let backup_path = config_path.with_extension("backup.json");
    fs::copy(config_path, backup_path)?;
    Ok(())
}
```

---

## 🎯 成功标准

### 必须达成 (Must Have)

- ✅ 所有现有功能正常工作
- ✅ 无用户数据丢失
- ✅ 性能不下降
- ✅ TypeScript 检查通过

### 期望达成 (Should Have)

- ✅ 代码量减少 40%+
- ✅ 用户反馈积极
- ✅ 开发体验提升明显

### 可选达成 (Nice to Have)

- ⭕ 添加自动化测试
- ⭕ 性能优化 20%+

---

## 📊 预期成果

### 代码质量

- **代码行数**: 减少 40-60%
- **文件数量**: UI 组件增加，但单文件更小
- **可维护性**: 大幅提升

### 开发效率

- **新功能开发**: 提升 50%+
- **Bug 修复**: 提升 30%+
- **代码审查**: 提升 40%+

### 用户体验

- **界面一致性**: 统一的设计语言
- **响应速度**: 更好的加载反馈
- **错误提示**: 更友好的错误信息

---

## 📚 参考资料

- [TanStack Query 文档](https://tanstack.com/query/latest)
- [react-hook-form 文档](https://react-hook-form.com/)
- [shadcn/ui 文档](https://ui.shadcn.com/)
- [Zod 文档](https://zod.dev/)
- [原始 PR #76](https://github.com/farion1231/cc-switch/pull/76)

---

## 📝 注意事项

1. **分支管理**: 在新分支进行，不要直接在 main 上修改
2. **提交粒度**: 每完成一小步就提交，便于回滚
3. **文档更新**: 同步更新 CLAUDE.md
4. **依赖锁定**: 锁定依赖版本
5. **沟通协作**: 定期同步进度

---

**祝重构顺利! 🚀**
