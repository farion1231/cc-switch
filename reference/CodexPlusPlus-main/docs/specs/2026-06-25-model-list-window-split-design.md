# 模型列表与上下文窗口分离设计

## 背景

当前 CodexPlusPlus 的「模型列表」使用单文本框，每行一个模型，支持在模型名后加 `[1M]`、`[200K]` 或 `[1000000]` 等后缀来声明该模型的上下文窗口。

该方案在功能上正确：后端会剥离后缀生成 catalog，Codex 客户端实际使用的是无后缀 slug。但存在以下问题：

1. **历史记录污染**：Codex 选择器会显示最近使用过的 model ID。如果用户曾经选择过带后缀的字符串（或旧版本把带后缀字符串写入了 `threads.model`），选择器里就会继续显示 `deepseek-v4-flash[1M]` 这类废弃项。
2. **用户误选报错**：选择带后缀的历史项后，Codex 会把 `config.toml` 的 `model` 字段更新为带后缀字符串，而 catalog 里只有无后缀 slug，导致 "is not a valid model ID" 报错。
3. **与 cc-switch 的体验不一致**：cc-switch 把「模型名」和「上下文窗口」在 UI 与存储层彻底分开，Codex 客户端永远看不到带后缀字符串，历史记录自然干净。

本设计将模型列表改为左右并排的两个输入框：左侧填模型名（无后缀），右侧填上下文窗口，从根上避免 Codex 接触到带后缀的 model ID。

## 设计目标

1. Codex 客户端实际使用的 model ID 永远不带后缀。
2. 用户输入方式尽量保持简单：仍然是一个模型一行，只是窗口单独放到右侧。
3. 旧数据（`deepseek-v4-flash[1M]`）自动迁移一次，之后彻底使用新格式。
4. 不引入表格、加减行按钮等复杂交互。

## 数据模型

### 新增字段

在 `RelayProfile` 中新增 `model_windows` 字段：

```rust
pub struct RelayProfile {
    // ... 现有字段 ...

    /// 每行一个无后缀模型 slug。
    pub model_list: String,

    /// JSON map：slug -> 窗口 token（如 "1M" / "200K" / "1000000"）。
    /// 空字符串或缺失表示该模型使用 Codex 默认窗口。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model_windows: String,
}
```

### 示例

旧格式 `model_list`：

```text
deepseek/deepseek-v4-flash[1M]
deepseek/deepseek-v4-pro
nvidia/nemotron-3-super-120b-a12b:free[1M]
```

迁移后：

- `model_list`：
  ```text
  deepseek/deepseek-v4-flash
  deepseek/deepseek-v4-pro
  nvidia/nemotron-3-super-120b-a12b:free
  ```

- `model_windows`（JSON）：
  ```json
  {
    "deepseek/deepseek-v4-flash": "1M",
    "nvidia/nemotron-3-super-120b-a12b:free": "1M"
  }
  ```

## UI 设计

### 模型列表区域

将当前单文本框拆为左右两个多行文本框：

```tsx
<div className="model-list-split">
  <div className="model-list-column">
    <label>模型名称</label>
    <textarea
      value={draft.modelList}
      onChange={handleModelListChange}
      placeholder="deepseek/deepseek-v4-flash"
      style={{ fontFamily: 'monospace' }}
    />
  </div>
  <div className="model-list-column">
    <label>上下文窗口</label>
    <textarea
      value={draft.modelWindowsText}
      onChange={handleModelWindowsChange}
      placeholder="1M"
      style={{ fontFamily: 'monospace' }}
    />
  </div>
</div>
```

### 对齐策略

为避免行数多时左右对应关系错位，采用以下策略：

1. **等宽字体**：左右 textarea 均使用 `monospace`，相同行高。
2. **高度绑定**：保存/渲染时让两个 textarea 高度一致。
3. **同步滚动**：左右滚动时保持 `scrollTop` 同步。
4. **行数校验**：保存时校验左右行数一致，不一致则阻止保存并提示。

### 配置模型字段

「配置模型」保持单个输入框，但只接受无后缀 slug：

```tsx
<input
  value={draft.configuredModel}
  onChange={handleConfiguredModelChange}
  placeholder="deepseek/deepseek-v4-flash"
/>
```

保存时自动剥离后缀（兼容用户手误）。

## 前端状态与保存逻辑

### draft 新增字段

```tsx
interface RelayProfileDraft {
  // ... 现有字段 ...
  modelList: string;
  modelWindowsText: string; // 新增
  configuredModel: string;
}
```

### 从后端 profile 初始化 draft

```tsx
const windowsMap = safeJsonParse<Record<string, string>>(profile.model_windows, {});
const modelWindowsText = profile.model_list
  .split('\n')
  .map(line => windowsMap[line.trim()] ?? '')
  .join('\n');
```

### 保存时组装

```tsx
const models = draft.modelList
  .split('\n')
  .map(s => s.trim())
  .filter(Boolean);

const windows = draft.modelWindowsText
  .split('\n')
  .map(s => s.trim());

if (models.length !== windows.length) {
  // 阻止保存，提示行数不一致
  return;
}

const windowsMap: Record<string, string> = {};
models.forEach((model, index) => {
  if (windows[index]) {
    windowsMap[model] = windows[index];
  }
});

patch.modelList = models.join('\n');
patch.modelWindows = JSON.stringify(windowsMap);
```

## 后端 apply 流程

### 旧数据迁移（一次性）

在 settings 加载或 `normalize_relay_profile_for_storage` 中执行一次性迁移：

```rust
if profile.model_windows.is_empty() && profile.model_list.contains('[') {
    let (clean_list, windows) = migrate_model_list_with_suffixes(&profile.model_list);
    profile.model_list = clean_list;
    profile.model_windows = serde_json::to_string(&windows).unwrap_or_default();
}
```

这是一次性迁移。迁移完成后，`model_list` 不再包含 `[suffix]` 语法，后续代码也不再把 `[suffix]` 作为模型列表的合法格式来解析。新 UI 中模型名和窗口是两个独立字段。

### catalog 生成

`collect_catalog_entries` 签名改为接收 `model_list` 和 `model_windows`：

```rust
pub fn collect_catalog_entries(
    model_list: &str,
    model_windows: &HashMap<String, String>,
) -> Vec<ModelCatalogEntry>
```

逻辑：

1. 解析 `model_list` 为无后缀 slug 列表。
2. 对每个 slug，从 `model_windows` 查找窗口 token；未找到则使用 profile 顶层默认窗口。
3. 生成 catalog，slug 永远无后缀。

### config.toml 写入

`complete_relay_profile_config` 保持现有逻辑：

- `model` 字段写入前调用 `parse_model_suffix` 剥离后缀（兼容旧数据/用户手误）。
- `model_catalog_json` 指向新生成的 catalog 文件。

### 历史缓存清理

保留现有的启动时清理逻辑：

- 启动前清理 `state_5.sqlite` 的 `threads.model`。
- 启动后通过 CDP 清理 Electron `localStorage.__codexDailyTokenUsageV1`。

迁移完成后，Codex 历史记录里不会再出现新的带后缀项。

## 测试策略

### 后端测试

| 测试 | 位置 | 说明 |
|---|---|---|
| `model_windows` 解析 | `tests/model_suffix.rs` | 验证 `1M` / `200K` / `1000000` 解析为数字窗口 |
| 旧 `model_list` 迁移 | `tests/settings.rs` 或新文件 | `deepseek-v4-flash[1M]` 迁移为 `model_list` + `model_windows` |
| catalog 生成 | `tests/relay_config.rs` | 用分离后的格式生成 catalog，slug 无后缀 |
| 窗口为空使用默认值 | `tests/relay_config.rs` | 无窗口项使用 Codex 默认窗口 |
| 同名 slug 按有窗口处理 | `tests/relay_config.rs` | 已有测试需适配新签名 |

### 前端测试

| 测试 | 说明 |
|---|---|
| 渲染测试 | 两个 textarea 存在，placeholder 正确 |
| 输入同步测试 | 修改左侧不影响右侧，反之亦然 |
| 行数校验测试 | 左右行数不一致时阻止保存并提示 |
| 保存组装测试 | 输入模型列表和窗口列表后，`model_windows` JSON 正确 |
| 旧数据迁移显示测试 | 给定旧 `model_list` 含后缀，draft 初始化后显示为分离的两列 |

## 兼容性

1. **旧格式迁移**：settings 加载时自动把 `model_list` 中的 `[suffix]` 拆出到 `model_windows`。
2. **Codex 客户端**：始终使用无后缀 model ID，不会再产生带后缀历史项。
3. **手动编辑 settings 文件**：即使用户手动写回带后缀字符串，下次加载时也会重新迁移。
4. **不写窗口的模型**：保持 Codex 默认行为，与现有逻辑一致。

## 后续可扩展

- 右侧窗口输入框未来可扩展为下拉选择（常用预设）+ 自由输入的混合形态。
- 可在每行右侧增加「自动检测」按钮，根据模型名从内置表猜测默认窗口。
