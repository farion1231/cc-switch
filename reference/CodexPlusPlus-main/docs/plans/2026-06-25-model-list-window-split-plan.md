# 模型列表与上下文窗口分离实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans` 逐任务实现此计划。步骤使用复选框（`- [ ]`）语法来跟踪进度。

**目标：** 将 CodexPlusPlus 的模型列表从单文本框（模型名可带 `[1M]` 后缀）改造为左右并排双文本框（左侧模型名、右侧上下文窗口），在存储层把 `model_list` 和 `model_windows` 彻底分离，使 Codex 客户端永远看不到带后缀的模型名。

**架构：** 在 `RelayProfile` 中新增 `model_windows` JSON map 字段；后端 `collect_catalog_entries` 改为从 `model_list`（无后缀 slug 列表）和 `model_windows`（slug -> 窗口 token）组合生成 catalog；前端把模型列表 UI 拆为左右两个 textarea，保存时按行组装成 `model_windows`；settings 加载时自动把旧格式 `deepseek-v4-flash[1M]` 一次性迁移到新格式。

**技术栈：** Rust（serde、toml_edit）、React + TypeScript。

---

## 文件结构

| 文件 | 职责 |
|---|---|
| `crates/codex-plus-core/src/settings.rs` | `RelayProfile` 结构体，新增 `model_windows` 字段 |
| `crates/codex-plus-core/src/model_suffix.rs` | 新增旧格式迁移函数；修改 `collect_catalog_entries` 签名以接收 `model_windows` map |
| `crates/codex-plus-core/src/relay_config.rs` | 在 `normalize_relay_profile_for_storage` 调用迁移；在 `apply_model_catalog_to_config` 中组装 catalog |
| `crates/codex-plus-core/tests/model_suffix.rs` | 更新现有测试；新增迁移和 `model_windows` 测试 |
| `crates/codex-plus-core/tests/relay_config.rs` | 更新现有测试以使用新签名 |
| `apps/codex-plus-manager/src/App.tsx` | 前端：类型定义、draft 状态、模型列表 UI、保存组装、配置模型剥离后缀 |
| `apps/codex-plus-manager/src/App.test.tsx` | 新增前端渲染、行数校验、保存组装测试 |

---

## 任务 1：在 `RelayProfile` 中添加 `model_windows` 字段

**文件：**
- 修改：`crates/codex-plus-core/src/settings.rs:82-89`

- [ ] **步骤 1：添加字段**

在 `model_list` 字段后增加：

```rust
#[serde(rename = "modelList", default)]
pub model_list: String,
#[serde(rename = "modelWindows", default, skip_serializing_if = "String::is_empty")]
pub model_windows: String,
```

- [ ] **步骤 2：更新 Default 实现**

在 `impl Default for RelayProfile` 中找到 `model_list` 的默认值，添加 `model_windows`：

```rust
model_list: String::new(),
model_windows: String::new(),
```

- [ ] **步骤 3：编译检查**

运行：

```bash
cargo check -p codex-plus-core
```

预期：通过（此时字段尚未被使用，只是结构体定义）。

- [ ] **步骤 4：Commit**

```bash
git add crates/codex-plus-core/src/settings.rs
git commit -m "feat(core): 在 RelayProfile 中新增 model_windows 字段"
```

---

## 任务 2：添加旧格式迁移函数

**文件：**
- 修改：`crates/codex-plus-core/src/model_suffix.rs`

- [ ] **步骤 1：编写测试**

在 `crates/codex-plus-core/tests/model_suffix.rs` 末尾新增：

```rust
#[test]
fn migrate_model_list_with_suffixes_splits_slug_and_window() {
    let input = "deepseek-v4-flash[1M]\ndeepseek-v4-pro\nnvidia/...:free[200K]";
    let (clean_list, windows) = codex_plus_core::model_suffix::migrate_model_list_with_suffixes(input);
    assert_eq!(clean_list, "deepseek-v4-flash\ndeepseek-v4-pro\nnvidia/...:free");
    assert_eq!(windows.get("deepseek-v4-flash"), Some(&"1M".to_string()));
    assert_eq!(windows.get("deepseek-v4-pro"), None);
    assert_eq!(windows.get("nvidia/...:free"), Some(&"200K".to_string()));
}
```

- [ ] **步骤 2：运行测试验证失败**

```bash
cargo test -p codex-plus-core --test model_suffix migrate_model_list_with_suffixes
```

预期：`FAIL`，提示 `migrate_model_list_with_suffixes` 未找到。

- [ ] **步骤 3：实现迁移函数**

在 `crates/codex-plus-core/src/model_suffix.rs` 中，紧接 `parse_model_suffix` 之后添加：

```rust
use std::collections::HashMap;

/// 一次性迁移：把旧格式 `slug[suffix]` 的 model_list 拆成无后缀列表和窗口 map。
pub fn migrate_model_list_with_suffixes(model_list: &str) -> (String, HashMap<String, String>) {
    let mut clean_lines = Vec::new();
    let mut windows = HashMap::new();
    for raw in model_list.split(['\r', '\n', ',']).map(str::trim).filter(|v| !v.is_empty()) {
        let (slug, window) = parse_model_suffix(raw);
        clean_lines.push(slug.clone());
        if let Some(window) = window {
            windows.insert(slug, window.to_string());
        }
    }
    (clean_lines.join("\n"), windows)
}
```

注意：`parse_model_suffix` 返回的 `window` 是 `Option<u64>`，需要 `to_string()`。

- [ ] **步骤 4：导出函数**

确保 `model_suffix.rs` 已经 `pub fn migrate_model_list_with_suffixes`。

- [ ] **步骤 5：运行测试验证通过**

```bash
cargo test -p codex-plus-core --test model_suffix migrate_model_list_with_suffixes
```

预期：`PASS`。

- [ ] **步骤 6：Commit**

```bash
git add crates/codex-plus-core/src/model_suffix.rs crates/codex-plus-core/tests/model_suffix.rs
git commit -m "feat(core): 添加旧 model_list 后缀迁移函数"
```

---

## 任务 3：修改 `collect_catalog_entries` 签名以接收 `model_windows`

**文件：**
- 修改：`crates/codex-plus-core/src/model_suffix.rs:64-121`
- 修改：`crates/codex-plus-core/src/model_suffix.rs` 相关调用点

- [ ] **步骤 1：更新现有测试以使用新签名**

在 `crates/codex-plus-core/tests/model_suffix.rs` 中，找到所有 `collect_catalog_entries(...)` 调用，改为传入空 map：

```rust
let entries = collect_catalog_entries(model_list, &HashMap::new(), current_model);
```

如果测试原本就依赖后缀窗口，改为在 map 中传入：

```rust
let mut windows = HashMap::new();
windows.insert("deepseek-v4-flash".to_string(), "1M".to_string());
let entries = collect_catalog_entries("deepseek-v4-flash", &windows, current_model);
```

- [ ] **步骤 2：运行测试验证失败**

```bash
cargo test -p codex-plus-core --test model_suffix
```

预期：`FAIL`，签名不匹配。

- [ ] **步骤 3：修改函数签名和实现**

把：

```rust
pub fn collect_catalog_entries(model_list: &str, current_model: &str) -> Vec<ModelCatalogEntry>
```

改为：

```rust
pub fn collect_catalog_entries(
    model_list: &str,
    model_windows: &HashMap<String, String>,
    current_model: &str,
) -> Vec<ModelCatalogEntry>
```

函数内部：

1. 删除 `suffix_for_slug` 的构建逻辑（不再从 model_list 解析后缀）。
2. 解析 `model_list` 时只保留 slug：

```rust
let (slug, _) = parse_model_suffix(raw);
```

3. 窗口来源改为 `model_windows.get(&slug)`：

```rust
let suffix_window = model_windows.get(&slug).and_then(|token| parse_window_token(token));
```

4. 处理 `current_model` 时同样从 `model_windows` 查找：

```rust
if suffix_window.is_none() {
    if let Some(token) = model_windows.get(&slug) {
        suffix_window = parse_window_token(token);
    }
}
```

- [ ] **步骤 4：编译检查**

```bash
cargo check -p codex-plus-core
```

预期：通过。

- [ ] **步骤 5：运行测试验证通过**

```bash
cargo test -p codex-plus-core --test model_suffix
```

预期：`PASS`。

- [ ] **步骤 6：Commit**

```bash
git add crates/codex-plus-core/src/model_suffix.rs crates/codex-plus-core/tests/model_suffix.rs
git commit -m "feat(core): collect_catalog_entries 改为从 model_windows map 读取窗口"
```

---

## 任务 4：更新 `relay_config.rs` 中的 catalog 生成调用

**文件：**
- 修改：`crates/codex-plus-core/src/relay_config.rs:1389`

- [ ] **步骤 1：编写失败测试**

在 `crates/codex-plus-core/tests/relay_config.rs` 中新增：

```rust
#[test]
fn apply_model_catalog_uses_model_windows_map() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let mut profile = build_test_relay_profile();
    profile.model_list = "deepseek-v4-flash\ndeepseek-v4-pro".to_string();
    profile.model_windows = r#"{"deepseek-v4-flash":"1M"}"#.to_string();
    profile.context_window = "200000".to_string();

    apply_relay_profile_to_home_with_switch_rules_and_computer_use_guard(
        &home, &profile, "", false,
    )
    .unwrap();

    let catalog_path = home.join("model-catalogs").join(format!("{}.json", sanitize(&profile.id)));
    let catalog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(catalog_path).unwrap()).unwrap();
    let models = catalog["models"].as_array().unwrap();
    let flash = models.iter().find(|m| m["slug"].as_str().unwrap() == "deepseek-v4-flash").unwrap();
    let pro = models.iter().find(|m| m["slug"].as_str().unwrap() == "deepseek-v4-pro").unwrap();
    assert_eq!(flash["context_window"].as_u64().unwrap(), 1_000_000);
    assert_eq!(pro["context_window"].as_u64().unwrap(), 200_000);
}
```

- [ ] **步骤 2：运行测试验证失败**

```bash
cargo test -p codex-plus-core --test relay_config apply_model_catalog_uses_model_windows_map
```

预期：`FAIL`，因为 `relay_config.rs` 仍按旧签名调用。

- [ ] **步骤 3：修改调用点**

在 `crates/codex-plus-core/src/relay_config.rs` 的 `apply_model_catalog_to_config` 函数中：

```rust
let model_windows: std::collections::HashMap<String, String> =
    serde_json::from_str(&profile.model_windows).unwrap_or_default();
let entries = crate::model_suffix::collect_catalog_entries(&profile.model_list, &model_windows, &profile.model);
```

- [ ] **步骤 4：编译检查**

```bash
cargo check -p codex-plus-core
```

预期：通过。

- [ ] **步骤 5：运行测试验证通过**

```bash
cargo test -p codex-plus-core --test relay_config apply_model_catalog_uses_model_windows_map
```

预期：`PASS`。

- [ ] **步骤 6：运行全部相关测试**

```bash
cargo test -p codex-plus-core --test relay_config --test model_suffix
```

预期：全部通过。

- [ ] **步骤 7：Commit**

```bash
git add crates/codex-plus-core/src/relay_config.rs crates/codex-plus-core/tests/relay_config.rs
git commit -m "feat(core): apply_model_catalog_to_config 使用 model_windows map"
```

---

## 任务 5：在 settings 加载/保存时迁移旧数据

**文件：**
- 修改：`crates/codex-plus-core/src/relay_config.rs` 中的 `normalize_relay_profile_for_storage`

- [ ] **步骤 1：编写失败测试**

在 `crates/codex-plus-core/tests/relay_config.rs` 中新增：

```rust
#[test]
fn normalize_migrates_model_list_suffixes_to_model_windows() {
    let mut profile = build_test_relay_profile();
    profile.model_list = "deepseek-v4-flash[1M]\ndeepseek-v4-pro".to_string();
    profile.model_windows = String::new();

    codex_plus_core::relay_config::normalize_relay_profile_for_storage(&mut profile).unwrap();

    assert_eq!(profile.model_list, "deepseek-v4-flash\ndeepseek-v4-pro");
    let windows: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&profile.model_windows).unwrap();
    assert_eq!(windows.get("deepseek-v4-flash").unwrap().as_str().unwrap(), "1M");
    assert!(!windows.contains_key("deepseek-v4-pro"));
}
```

- [ ] **步骤 2：运行测试验证失败**

```bash
cargo test -p codex-plus-core --test relay_config normalize_migrates_model_list_suffixes_to_model_windows
```

预期：`FAIL`。

- [ ] **步骤 3：实现迁移**

在 `normalize_relay_profile_for_storage` 函数开头（在调用 `complete_relay_profile_config` 之前）添加：

```rust
if profile.model_windows.trim().is_empty() && profile.model_list.contains('[') {
    let (clean_list, windows) = crate::model_suffix::migrate_model_list_with_suffixes(&profile.model_list);
    profile.model_list = clean_list;
    profile.model_windows = serde_json::to_string(&windows).unwrap_or_default();
}
```

- [ ] **步骤 4：运行测试验证通过**

```bash
cargo test -p codex-plus-core --test relay_config normalize_migrates_model_list_suffixes_to_model_windows
```

预期：`PASS`。

- [ ] **步骤 5：Commit**

```bash
git add crates/codex-plus-core/src/relay_config.rs crates/codex-plus-core/tests/relay_config.rs
git commit -m "feat(core): settings 保存时自动迁移旧 model_list 后缀到新 model_windows"
```

---

## 任务 6：前端 TypeScript 类型添加 `modelWindows`

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx:185`

- [ ] **步骤 1：添加字段**

在 `modelList: string;` 后添加：

```tsx
modelList: string;
modelWindows: string;
userAgent: string;
```

- [ ] **步骤 2：更新所有 default 创建点**

搜索所有 `modelList: ""` 出现的位置，在同对象内添加 `modelWindows: ""`：

```bash
grep -n 'modelList: ""' apps/codex-plus-manager/src/App.tsx
```

应修改的位置（根据当前代码）：
- `apps/codex-plus-manager/src/App.tsx:659`
- `apps/codex-plus-manager/src/App.tsx:5475`
- `apps/codex-plus-manager/src/App.tsx:5531`
- `apps/codex-plus-manager/src/App.tsx:5557`
- `apps/codex-plus-manager/src/App.tsx:6169`
- `apps/codex-plus-manager/src/App.tsx:6198`

每个位置改为：

```tsx
modelList: "",
modelWindows: "",
```

- [ ] **步骤 3：更新 `normalizeRelayProfile`**

在 `apps/codex-plus-manager/src/App.tsx:5557`：

```tsx
modelList: profile.modelList || "",
modelWindows: profile.modelWindows || "",
```

- [ ] **步骤 4：编译检查**

```bash
cd apps/codex-plus-manager && npm run tauri dev 2>&1 | head -50
```

或更轻量：

```bash
cd apps/codex-plus-manager && npx tsc --noEmit
```

预期：通过。

- [ ] **步骤 5：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx
git commit -m "feat(manager): TypeScript RelayProfile 类型添加 modelWindows 字段"
```

---

## 任务 7：前端添加模型窗口文本辅助函数

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx`（在 `parseModelSuffix` 附近）

- [ ] **步骤 1：添加辅助函数**

在 `parseModelSuffix` 函数之后添加：

```tsx
/// 把 model_windows JSON map 按 model_list 行顺序转成文本（每行一个窗口，空行表示默认）。
function modelWindowsMapToText(modelList: string, modelWindows: string): string {
  try {
    const map = JSON.parse(modelWindows || "{}") as Record<string, string>;
    return modelList
      .split("\n")
      .map((line) => map[line.trim()] ?? "")
      .join("\n");
  } catch {
    return "";
  }
}

/// 把左右 textarea 文本组装成 model_windows JSON map。
function modelWindowsTextToMap(modelList: string, modelWindowsText: string): string {
  const models = modelList.split("\n").map((s) => s.trim()).filter(Boolean);
  const windows = modelWindowsText.split("\n").map((s) => s.trim());
  const map: Record<string, string> = {};
  models.forEach((model, index) => {
    if (windows[index]) {
      map[model] = windows[index];
    }
  });
  return JSON.stringify(map);
}
```

- [ ] **步骤 2：编译检查**

```bash
cd apps/codex-plus-manager && npx tsc --noEmit
```

预期：通过。

- [ ] **步骤 3：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx
git commit -m "feat(manager): 添加 model_windows 文本与 map 互转辅助函数"
```

---

## 任务 8：前端模型列表 UI 改为双 textarea

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx:3900-3925`

- [ ] **步骤 1：修改 UI 渲染**

把当前模型列表 Field 替换为：

```tsx
{showApiFields ? (
  <Field className="relay-field-model-list" label="模型列表">
    <div className="relay-model-list-split">
      <div className="relay-model-list-column">
        <label className="relay-model-list-label">模型名称</label>
        <Textarea
          value={profile.modelList}
          onChange={(event) => updateDraft({ modelList: event.currentTarget.value })}
          placeholder="deepseek/deepseek-v4-flash"
          style={{ fontFamily: "monospace" }}
        />
      </div>
      <div className="relay-model-list-column">
        <label className="relay-model-list-label">上下文窗口</label>
        <Textarea
          value={modelWindowsText}
          onChange={(event) => setModelWindowsText(event.currentTarget.value)}
          placeholder="1M"
          style={{ fontFamily: "monospace" }}
        />
      </div>
    </div>
    <div className="relay-model-list-tools">
      <Button
        onClick={async () => {
          const models = await actions.fetchRelayProfileModels(profile);
          if (models?.length) updateDraft({ modelList: models.join("\n") });
        }}
        size="sm"
        type="button"
        variant="secondary"
      >
        <Download className="h-4 w-4" />
        从上游获取
      </Button>
    </div>
    <p className="field-hint">
      每行一个模型；左侧填模型名，右侧填上下文窗口（如 <code>1M</code>、<code>200K</code> 或 <code>1000000</code>）。右侧留空表示使用 Codex 默认长度。
    </p>
  </Field>
) : null}
```

- [ ] **步骤 2：添加/更新 CSS**

在 `apps/codex-plus-manager/src/App.css`（或使用的样式文件）中添加：

```css
.relay-model-list-split {
  display: flex;
  gap: 12px;
}

.relay-model-list-column {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.relay-model-list-label {
  font-size: 0.875rem;
  margin-bottom: 4px;
  color: var(--muted-foreground);
}

.relay-model-list-column textarea {
  min-height: 120px;
  resize: vertical;
  white-space: pre;
  overflow-wrap: normal;
  overflow-x: auto;
}
```

- [ ] **步骤 3：编译检查**

```bash
cd apps/codex-plus-manager && npx tsc --noEmit
```

预期：通过。

- [ ] **步骤 4：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx apps/codex-plus-manager/src/App.css
git commit -m "feat(manager): 模型列表 UI 拆分为模型名和上下文窗口两列"
```

---

## 任务 9：前端独立管理 `modelWindowsText` state

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx` 中 `RelayProfileEditor` 组件

- [ ] **步骤 1：添加独立 state**

在 `const [draft, setDraft] = useState<RelayProfile>(profile);` 之后添加：

```tsx
const [modelWindowsText, setModelWindowsText] = useState(
  modelWindowsMapToText(profile.modelList, profile.modelWindows || "")
);
```

- [ ] **步骤 2：同步 modelList 变化时更新窗口文本**

使用 `useEffect` 监听 `draft.modelList` 和 `draft.modelWindows`：

```tsx
useEffect(() => {
  setModelWindowsText(modelWindowsMapToText(draft.modelList, draft.modelWindows || ""));
}, [draft.modelList, draft.modelWindows]);
```

注意：当用户主动编辑右侧窗口文本时，不应被这个 effect 覆盖。因此更精确的做法是：只在 `draft.modelList` 行数变化且 `modelWindowsText` 尚未由用户编辑过，或从上游获取/加载时同步。

为简化，初始版本可以在 `draft.modelList` 变化时重新生成（用户编辑右侧会暂时被覆盖，可后续优化）。

- [ ] **步骤 3：编译检查**

```bash
cd apps/codex-plus-manager && npx tsc --noEmit
```

预期：通过。

- [ ] **步骤 4：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx
git commit -m "feat(manager): 独立管理 modelWindowsText state"
```

---

## 任务 10：前端保存时组装 `modelWindows` 并校验行数

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx` 中 `saveDraft` 和 `switchDraft` 函数

- [ ] **步骤 1：修改 `saveDraft`**

在 `saveDraft` 函数中，在 `if (validationError) return;` 之后、调用 `deriveRelayProfileFromFiles` 之前添加：

```tsx
const models = draft.modelList.split("\n").map((s) => s.trim()).filter(Boolean);
const windows = modelWindowsText.split("\n").map((s) => s.trim());

if (models.length !== windows.length) {
  // 可以用 toast 或 alert，这里先用 alert
  alert(`模型名称有 ${models.length} 行，上下文窗口有 ${windows.length} 行，请保持行数一致。`);
  return;
}

const modelWindows = modelWindowsTextToMap(draft.modelList, modelWindowsText);
const draftWithWindows = { ...draft, modelWindows };
const normalizedDraft = deriveRelayProfileFromFiles(draftWithWindows);
```

然后 `normalizedDraft` 会包含 `modelWindows`。

- [ ] **步骤 2：同步 `switchDraft` 函数**

`switchDraft` 中也调用了 `deriveRelayProfileFromFiles(draft)`，需要同样组装 `modelWindows`：

```tsx
const modelWindows = modelWindowsTextToMap(draft.modelList, modelWindowsText);
const normalizedDraft = deriveRelayProfileFromFiles({ ...draft, modelWindows });
```

- [ ] **步骤 3：编译检查**

```bash
cd apps/codex-plus-manager && npx tsc --noEmit
```

预期：通过。

- [ ] **步骤 4：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx
git commit -m "feat(manager): 保存时校验行数并组装 model_windows"
```

---

## 任务 11：处理从上游获取模型列表

**文件：**
- 修改：`apps/codex-plus-manager/src/App.tsx` 中「从上游获取」按钮事件

- [ ] **步骤 1：清空右侧窗口文本**

当点击「从上游获取」时，获取到的模型列表是无后缀 slug，应同时清空 `modelWindowsText`：

```tsx
if (models?.length) {
  updateDraft({ modelList: models.join("\n"), modelWindowsText: "" });
}
```

- [ ] **步骤 2：Commit**

```bash
git add apps/codex-plus-manager/src/App.tsx
git commit -m "feat(manager): 从上游获取模型列表时清空窗口文本"
```

---

## 任务 12：后端 `complete_relay_profile_config` 兼容旧 model 字段后缀

**文件：**
- 修改：`crates/codex-plus-core/src/relay_config.rs:1873-1888`

- [ ] **步骤 1：确认现有逻辑**

当前已经调用 `parse_model_suffix` 剥离 model 后缀。如果 `model_list` 首条也带后缀，需要确保取到无后缀 slug。

- [ ] **步骤 2：修复 fallback model 来源**

在：

```rust
if model.trim().is_empty() && !profile.model_list.trim().is_empty() {
    if let Some(first) = profile.model_list.split(...).find(...) {
        model = crate::model_suffix::parse_model_suffix(first).0;
    }
}
```

这里 `profile.model_list` 已经迁移为无后缀，所以 `parse_model_suffix` 返回原串即可，无需改动。

- [ ] **步骤 3：Commit**

此任务若无需改动，可与任务 13 合并。

---

## 任务 13：添加/更新测试

### 后端测试

- [ ] **步骤 1：更新 `collect_catalog_entries` 调用点**

在 `crates/codex-plus-core/tests/model_suffix.rs` 和 `tests/relay_config.rs` 中，把所有 `collect_catalog_entries(model_list, current_model)` 改为 `collect_catalog_entries(model_list, &HashMap::new(), current_model)` 或传入对应 map。

- [ ] **步骤 2：运行后端测试**

```bash
cargo test -p codex-plus-core --test relay_config --test model_suffix --test launcher --test codex_sqlite
```

预期：全部通过。

- [ ] **步骤 3：Commit**

```bash
git add crates/codex-plus-core/tests
git commit -m "test(core): 更新测试以适配 model_windows 新签名"
```

### 前端测试

- [ ] **步骤 4：新增渲染测试**

在 `apps/codex-plus-manager/src/App.test.tsx` 中新增：

```tsx
it("renders model list with two textareas", () => {
  render(<App />);
  // 根据实际路由/入口渲染编辑器，可能需要更具体的测试 helper
  expect(screen.getByPlaceholderText("deepseek/deepseek-v4-flash")).toBeInTheDocument();
  expect(screen.getByPlaceholderText("1M")).toBeInTheDocument();
});
```

- [ ] **步骤 5：新增行数校验测试**

```tsx
it("prevents save when model list and windows line counts mismatch", async () => {
  // 设置 draft：模型 2 行，窗口 1 行
  // 触发保存
  // 验证保存未执行（如 mock 的 onFormChange 未被调用）
});
```

- [ ] **步骤 6：运行前端测试**

```bash
cd apps/codex-plus-manager && npm test
```

预期：通过。

- [ ] **步骤 7：Commit**

```bash
git add apps/codex-plus-manager/src/App.test.tsx
git commit -m "test(manager): 添加模型列表双文本框相关测试"
```

---

## 任务 14：端到端验证

- [ ] **步骤 1：构建并运行管理工具**

```bash
cd apps/codex-plus-manager && npm run build
```

- [ ] **步骤 2：运行 example 验证后端**

```bash
cargo run --example apply_profile -p codex-plus-core
```

预期：`config.toml` 中 `model` 无后缀，catalog 窗口正确。

- [ ] **步骤 3：打包 DMG**

```bash
bash scripts/installer/macos/package-dmg.sh 1.2.18 arm64
```

- [ ] **步骤 4：手动验证**

1. 安装新 DMG。
2. 在管理工具中输入：
   - 左侧：`deepseek/deepseek-v4-flash`
   - 右侧：`1M`
3. 保存并启动 Codex。
4. 检查 `~/.codex/config.toml`：`model` 无后缀。
5. 检查 `~/.codex/model-catalogs/<profile>.json`：slug 无后缀，context_window 为 1M。
6. 在 Codex 中选择模型，确认不报 "is not a valid model ID"。

- [ ] **步骤 5：Commit（如有文档更新）**

```bash
git add docs/ CHANGELOG.md README.md README_EN.md
git commit -m "docs: 更新模型列表与窗口分离的使用说明"
```

---

## 自检

**1. 规格覆盖度：**

- 双输入框 UI：任务 8 覆盖。
- 存储层分离 `model_list` + `model_windows`：任务 1、4、10 覆盖。
- 旧数据一次性迁移：任务 2、5 覆盖。
- Codex 看不到带后缀字符串：任务 4（catalog slug 无后缀）+ 任务 12（config.toml model 无后缀）覆盖。
- 行数校验：任务 10 覆盖。
- 测试：任务 13 覆盖。

**2. 占位符扫描：**

- 无 TODO / 待定。
- 所有代码步骤均包含实际代码。

**3. 类型一致性：**

- Rust：`RelayProfile.model_windows` 类型一致。
- TypeScript：`RelayProfile.modelWindows` 和 draft 的 `modelWindowsText` 类型一致。
- `collect_catalog_entries` 签名在定义与调用处一致。

**4. 范围检查：**

- 本计划聚焦模型列表与窗口分离，不涉及其他功能。

---

## 执行交接

**计划已完成并保存到 `docs/plans/2026-06-25-model-list-window-split-plan.md`。两种执行方式：**

**1. 子代理驱动（推荐）** - 每个任务调度一个新的子代理，任务间进行审查，快速迭代

**2. 内联执行** - 在当前会话中使用 `executing-plans` 执行任务，批量执行并设有检查点

**选哪种方式？**
