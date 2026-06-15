# Batch Skill Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add batch delete and batch app-toggle capabilities to the installed skills management panel.

**Architecture:** Follow the existing sessions batch delete pattern — backend `collect_batch_results` helper drives per-item operations, frontend receives full result vectors to update cache and notify users.

**Tech Stack:** React + TypeScript + TanStack Query (frontend), Rust + Tauri 2 (backend), vitest + testing-library (tests)

**Spec:** `docs/superpowers/specs/2025-06-15-batch-skill-management-design.md`

---

### Task 1: Backend data structures

**Files:**
- Modify: `src-tauri/src/services/skill.rs` (append at end of data structures section, before `impl SkillService`)

- [ ] **Step 1: Add BatchSkillRequest and BatchSkillResult structs**

Open `src-tauri/src/services/skill.rs`. Locate the last struct defined before `impl SkillService` (currently `SkillsShSearchResult` near line 122). Append the following two structs after it:

```rust
/// 批量操作请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSkillRequest {
    pub id: String,
}

/// 批量操作结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSkillResult {
    pub id: String,
    pub success: bool,
    pub error: Option<String>,
}
```

- [ ] **Step 2: Verify the structs compile**

```bash
cd src-tauri && cargo check 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/skill.rs
git commit -m "feat(skill): add BatchSkillRequest and BatchSkillResult structs"
```

---

### Task 2: Backend service layer — batch operations

**Files:**
- Modify: `src-tauri/src/services/skill.rs` (add new methods inside `impl SkillService`)

- [ ] **Step 1: Add the `collect_batch_results` private helper**

Find the last method inside `impl SkillService` (currently `search_skills_sh`). After the closing `}` of that method, add:

```rust
    /// 批量操作结果收集器（与 sessions 的 collect_delete_session_outcomes 模式一致）
    fn collect_batch_results<F>(
        requests: &[super::BatchSkillRequest],
        mut operation: F,
    ) -> Vec<super::BatchSkillResult>
    where
        F: FnMut(&str) -> Result<bool, String>,
    {
        requests
            .iter()
            .map(|request| match operation(&request.id) {
                Ok(true) => super::BatchSkillResult {
                    id: request.id.clone(),
                    success: true,
                    error: None,
                },
                Ok(false) => super::BatchSkillResult {
                    id: request.id.clone(),
                    success: false,
                    error: Some("operation returned false".to_string()),
                },
                Err(error) => super::BatchSkillResult {
                    id: request.id.clone(),
                    success: false,
                    error: Some(error),
                },
            })
            .collect()
    }

    /// 批量卸载 Skills
    pub fn batch_uninstall(
        db: &Arc<Database>,
        requests: &[super::BatchSkillRequest],
    ) -> Vec<super::BatchSkillResult> {
        Self::collect_batch_results(requests, |id| match Self::uninstall(db, id) {
            Ok(result) => {
                let _ = result; // backup path is discarded for batch — toast shows summary only
                Ok(true)
            }
            Err(e) => Err(e.to_string()),
        })
    }

    /// 批量切换 Skills 的应用启用状态
    pub fn batch_toggle_app(
        db: &Arc<Database>,
        requests: &[super::BatchSkillRequest],
        app: &AppType,
        enabled: bool,
    ) -> Vec<super::BatchSkillResult> {
        Self::collect_batch_results(requests, |id| {
            match Self::toggle_app(db, id, app, enabled) {
                Ok(()) => Ok(true),
                Err(e) => Err(e.to_string()),
            }
        })
    }
```

Note: Since `BatchSkillRequest` and `BatchSkillResult` were added in Task 1, they are in scope. Adjust the `super::` prefix if the structs are in a different module — check the actual module structure. If the structs are defined directly in `skill.rs` (same file), use them directly without `super::`.

- [ ] **Step 2: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/services/skill.rs
git commit -m "feat(skill): add batch_uninstall and batch_toggle_app service methods"
```

---

### Task 3: Backend commands

**Files:**
- Modify: `src-tauri/src/commands/skill.rs`

- [ ] **Step 1: Import new types**

Open `src-tauri/src/commands/skill.rs`. Add `BatchSkillRequest` and `BatchSkillResult` to the import block (line 8-13):

```rust
use crate::services::skill::{
    BatchSkillRequest, BatchSkillResult, DiscoverableSkill, ImportSkillSelection, MigrationResult,
    Skill, SkillBackupEntry, SkillRepo, SkillService, SkillStorageLocation,
    SkillUninstallResult, SkillUpdateInfo, SkillsShSearchResult,
};
```

- [ ] **Step 2: Add batch_uninstall_skills command**

After the `uninstall_skill_unified` command (around line 74), add:

```rust
/// 批量卸载 Skills
#[tauri::command]
pub fn batch_uninstall_skills(
    requests: Vec<BatchSkillRequest>,
    app_state: State<'_, AppState>,
) -> Result<Vec<BatchSkillResult>, String> {
    Ok(SkillService::batch_uninstall(&app_state.db, &requests))
}
```

- [ ] **Step 3: Add batch_toggle_skill_app command**

After the `toggle_skill_app` command (around line 98), add:

```rust
/// 批量切换 Skill 的应用启用状态
#[tauri::command]
pub fn batch_toggle_skill_app(
    requests: Vec<BatchSkillRequest>,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<Vec<BatchSkillResult>, String> {
    let app_type = parse_app_type(&app)?;
    Ok(SkillService::batch_toggle_app(
        &app_state.db,
        &requests,
        &app_type,
        enabled,
    ))
}
```

- [ ] **Step 4: Register commands in lib.rs**

Open `src-tauri/src/lib.rs`. Find the existing skill command registrations (search for `install_skill_unified`). Add the two new commands:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    batch_uninstall_skills,
    batch_toggle_skill_app,
])
```

- [ ] **Step 5: Verify compilation**

```bash
cd src-tauri && cargo check 2>&1 | tail -10
```

Expected: `Finished` with no errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/skill.rs src-tauri/src/lib.rs
git commit -m "feat(skill): add batch_uninstall_skills and batch_toggle_skill_app commands"
```

---

### Task 4: Backend tests

**Files:**
- Create: `src-tauri/tests/skill_batch.rs`

- [ ] **Step 1: Write backend batch operation tests**

Create `src-tauri/tests/skill_batch.rs`:

```rust
use cc_switch_lib::{
    AppType, BatchSkillRequest, BatchSkillResult, InstalledSkill, SkillApps, SkillService,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn write_skill(dir: &std::path::Path, name: &str) {
    std::fs::create_dir_all(dir).expect("create skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill\n---\n"),
    )
    .expect("write SKILL.md");
}

/// Helper: install a skill and return its id
fn install_test_skill(state: &cc_switch_lib::AppState, dir_name: &str, display_name: &str) -> String {
    use cc_switch_lib::DiscoverableSkill;
    let ssot = SkillService::get_ssot_dir().expect("get ssot dir");
    let skill_dir = ssot.join(dir_name);
    write_skill(&skill_dir, display_name);

    let discoverable = DiscoverableSkill {
        key: format!("test:{}", dir_name),
        name: display_name.to_string(),
        description: "Test".to_string(),
        directory: dir_name.to_string(),
        readme_url: None,
        repo_owner: "test".to_string(),
        repo_name: "test-repo".to_string(),
        repo_branch: "main".to_string(),
    };

    let installed = SkillService::install_local(&state.db, &discoverable).expect("install");
    installed.id
}

#[test]
fn batch_uninstall_all_succeed() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = install_test_skill(&state, "batch-test-a", "Batch A");
    let id2 = install_test_skill(&state, "batch-test-b", "Batch B");

    let results = SkillService::batch_uninstall(
        &state.db,
        &[
            BatchSkillRequest { id: id1.clone() },
            BatchSkillRequest { id: id2.clone() },
        ],
    );

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.success, "skill should be uninstalled: {:?}", r);
    }
    // Verify both are removed from DB
    let installed = SkillService::get_all_installed(&state.db).expect("get installed");
    assert!(installed.is_empty());
}

#[test]
fn batch_uninstall_partial_failure() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = install_test_skill(&state, "batch-partial-a", "Partial A");

    let results = SkillService::batch_uninstall(
        &state.db,
        &[
            BatchSkillRequest { id: id1.clone() },
            BatchSkillRequest { id: "nonexistent-id".to_string() },
        ],
    );

    assert_eq!(results.len(), 2);
    assert!(results[0].success);
    assert!(!results[1].success);
    assert!(results[1].error.is_some());
}

#[test]
fn batch_toggle_app_all_succeed() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();
    let state = create_test_state().expect("create test state");

    let id1 = install_test_skill(&state, "batch-toggle-a", "Toggle A");
    let id2 = install_test_skill(&state, "batch-toggle-b", "Toggle B");

    // Disable claude for both
    let results = SkillService::batch_toggle_app(
        &state.db,
        &[
            BatchSkillRequest { id: id1.clone() },
            BatchSkillRequest { id: id2.clone() },
        ],
        &AppType::Claude,
        false,
    );

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.success, "toggle should succeed: {:?}", r);
    }
    // Verify claude is disabled
    let installed = SkillService::get_all_installed(&state.db).expect("get installed");
    for (_, skill) in &installed {
        assert!(!skill.apps.claude);
    }
}

#[test]
fn collect_batch_results_maps_errors() {
    let requests = vec![
        BatchSkillRequest { id: "a".to_string() },
        BatchSkillRequest { id: "b".to_string() },
    ];
    let results = SkillService::collect_batch_results_for_test(&requests, |id| {
        if id == "a" { Ok(true) }
        else { Err("simulated failure".to_string()) }
    });
    assert!(results[0].success);
    assert!(!results[1].success);
    assert_eq!(results[1].error.as_deref(), Some("simulated failure"));
}
```

Note: The `collect_batch_results_for_test` function may need to be added as a test-only helper or you can test it indirectly through `batch_uninstall`. Adjust based on what compiles. The helper `install_local` in `SkillService` may not exist — use the actual `install` method or `import_from_apps` as appropriate. Check the existing `skill_sync.rs` test file for the actual API signatures and adjust accordingly.

- [ ] **Step 2: Run backend tests**

```bash
cd src-tauri && cargo test skill_batch 2>&1 | tail -20
```

Expected: All tests pass.

- [ ] **Step 3: Run clippy**

```bash
cd src-tauri && cargo clippy -- -D warnings 2>&1 | tail -10
```

Expected: No warnings.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/skill_batch.rs
git commit -m "test(skill): add batch operation unit tests"
```

---

### Task 5: Frontend API wrappers

**Files:**
- Modify: `src/lib/api/skills.ts`

- [ ] **Step 1: Add BatchSkillResult type and API methods**

Open `src/lib/api/skills.ts`. After the `MigrationResult` interface (around line 105), add:

```typescript
/** 批量操作结果 */
export interface BatchSkillResult {
  id: string;
  success: boolean;
  error?: string;
}
```

Then find the `skillsApi` object. After the `updateSkill` method (around line 204), add:

```typescript
  /** 批量卸载 Skills */
  async batchUninstall(ids: string[]): Promise<BatchSkillResult[]> {
    const requests = ids.map((id) => ({ id }));
    return await invoke("batch_uninstall_skills", { requests });
  },

  /** 批量切换 Skill 的应用启用状态 */
  async batchToggleApp(
    ids: string[],
    app: AppId,
    enabled: boolean,
  ): Promise<BatchSkillResult[]> {
    const requests = ids.map((id) => ({ id }));
    return await invoke("batch_toggle_skill_app", { requests, app, enabled });
  },
```

- [ ] **Step 2: Run typecheck**

```bash
pnpm typecheck 2>&1 | tail -10
```

Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/api/skills.ts
git commit -m "feat(skill): add batchUninstall and batchToggleApp API wrappers"
```

---

### Task 6: Frontend hooks

**Files:**
- Modify: `src/hooks/useSkills.ts`

- [ ] **Step 1: Add batch mutation hooks**

Open `src/hooks/useSkills.ts`. Import the new type at the top (line 13, add `BatchSkillResult`):

```typescript
import {
  skillsApi,
  type SkillBackupEntry,
  type BatchSkillResult,
  type DiscoverableSkill,
  type ImportSkillSelection,
  type InstalledSkill,
  type SkillUpdateInfo,
  type SkillsShSearchResult,
} from "@/lib/api/skills";
```

After the `useUpdateSkill` hook definition (around line 326), add:

```typescript
// ========== 批量操作 ==========

/**
 * 批量卸载 Skills
 * 成功后直接更新缓存，移除已卸载的 skill
 */
export function useBatchUninstallSkills() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (ids: string[]) => skillsApi.batchUninstall(ids),
    onSuccess: (results, ids) => {
      const succeededIds = new Set(
        results.filter((r) => r.success).map((r) => r.id),
      );
      // 从 installed 缓存中移除成功删除的 skill
      queryClient.setQueryData<InstalledSkill[]>(
        ["skills", "installed"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.filter((s) => !succeededIds.has(s.id));
        },
      );
      // 刷新 discoverable 缓存（已删除的 skill 应标记为未安装）
      queryClient.invalidateQueries({ queryKey: ["skills", "discoverable"] });
    },
  });
}

/**
 * 批量切换 Skills 的应用启用状态
 * 成功后直接更新缓存
 */
export function useBatchToggleSkillApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      ids,
      app,
      enabled,
    }: {
      ids: string[];
      app: AppId;
      enabled: boolean;
    }) => skillsApi.batchToggleApp(ids, app, enabled),
    onSuccess: (results) => {
      // 批量切换后刷新缓存
      queryClient.invalidateQueries({ queryKey: ["skills", "installed"] });
    },
  });
}
```

- [ ] **Step 2: Run typecheck**

```bash
pnpm typecheck 2>&1 | tail -10
```

Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/hooks/useSkills.ts
git commit -m "feat(skill): add useBatchUninstallSkills and useBatchToggleSkillApp hooks"
```

---

### Task 7: Frontend hooks tests

**Files:**
- Create: `tests/hooks/useBatchSkillOperations.test.ts`

- [ ] **Step 1: Write hook logic tests**

Since the mutation hooks use TanStack Query with `invoke` calls, we test the cache update logic by testing the helper functions or by testing the component behavior. For the hooks themselves, test the core data transformation logic.

Create `tests/hooks/useBatchSkillOperations.test.ts`:

```typescript
import { describe, it, expect } from "vitest";
import type {
  InstalledSkill,
  BatchSkillResult,
} from "@/lib/api/skills";

function makeSkill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "skill-a",
    name: "Skill A",
    directory: "skill-a",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
    installedAt: 0,
    updatedAt: 0,
    ...overrides,
  };
}

describe("batch uninstall cache update", () => {
  it("removes successfully uninstalled skills from the cache", () => {
    const existing: InstalledSkill[] = [
      makeSkill({ id: "skill-a" }),
      makeSkill({ id: "skill-b", name: "Skill B", directory: "skill-b" }),
      makeSkill({ id: "skill-c", name: "Skill C", directory: "skill-c" }),
    ];

    // Simulate the cache update: remove skills whose id is in the success set
    const succeededIds = new Set(["skill-a", "skill-c"]);
    const updated = existing.filter((s) => !succeededIds.has(s.id));

    expect(updated).toHaveLength(1);
    expect(updated[0].id).toBe("skill-b");
  });

  it("keeps all skills when none succeed", () => {
    const existing: InstalledSkill[] = [
      makeSkill({ id: "skill-a" }),
      makeSkill({ id: "skill-b", name: "Skill B", directory: "skill-b" }),
    ];
    const succeededIds = new Set<string>();
    const updated = existing.filter((s) => !succeededIds.has(s.id));

    expect(updated).toHaveLength(2);
  });

  it("returns existing cache unchanged when no cache exists", () => {
    const succeededIds = new Set(["skill-a"]);
    // oldData is undefined → should return undefined
    const oldData: InstalledSkill[] | undefined = undefined;
    const updated = oldData ? oldData.filter((s) => !succeededIds.has(s.id)) : oldData;
    expect(updated).toBeUndefined();
  });
});

describe("batch results parsing", () => {
  it("correctly separates successes and failures", () => {
    const results: BatchSkillResult[] = [
      { id: "a", success: true },
      { id: "b", success: false, error: "not found" },
      { id: "c", success: true },
    ];

    const succeeded = results.filter((r) => r.success).map((r) => r.id);
    const failed = results.filter((r) => !r.success).map((r) => r.id);

    expect(succeeded).toEqual(["a", "c"]);
    expect(failed).toEqual(["b"]);
  });

  it("handles empty results", () => {
    const results: BatchSkillResult[] = [];
    const succeeded = results.filter((r) => r.success);
    const failed = results.filter((r) => !r.success);
    expect(succeeded).toHaveLength(0);
    expect(failed).toHaveLength(0);
  });
});
```

- [ ] **Step 2: Run tests**

```bash
pnpm test:unit -- tests/hooks/useBatchSkillOperations 2>&1 | tail -20
```

Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/hooks/useBatchSkillOperations.test.ts
git commit -m "test(skill): add batch operation cache update and result parsing tests"
```

---

### Task 8: i18n strings

**Files:**
- Modify: `src/locales/en.json`
- Modify: `src/locales/zh.json`
- Modify: `src/locales/ja.json`

- [ ] **Step 1: Add English strings**

Open `src/locales/en.json`. Find the `skills` section. Add the following keys (adjust JSON formatting to match surrounding key order):

```json
"batchManage": "Batch Manage",
"batchManageExit": "Exit Batch Mode",
"batchSelectAll": "Select All",
"batchDeselectAll": "Deselect All",
"batchSelectedCount": "{{count}} selected",
"batchDelete": "Delete Selected",
"batchDeleting": "Deleting...",
"batchDeleteConfirmTitle": "Batch Delete Skills",
"batchDeleteConfirmMessage": "This will delete {{count}} selected skills. Backups will be created for each skill.\n\nThis action can be undone by restoring from backups.",
"batchDeleteConfirmAction": "Delete Selected",
"batchDeleteSuccess": "Successfully deleted {{count}} skills",
"batchDeletePartial": "Deleted {{success}} skills, {{failed}} failed",
"batchDeleteFailed": "Batch delete failed. Please try again later.",
"batchToggleSuccess": "Successfully updated {{count}} skills",
"batchTogglePartial": "Updated {{success}} skills, {{failed}} failed",
"batchToggleFailed": "Batch update failed. Please try again later.",
```

- [ ] **Step 2: Add Chinese strings**

Open `src/locales/zh.json`. Add the same keys:

```json
"batchManage": "批量管理",
"batchManageExit": "退出批量管理",
"batchSelectAll": "全选",
"batchDeselectAll": "取消全选",
"batchSelectedCount": "已选 {{count}} 个",
"batchDelete": "删除所选",
"batchDeleting": "删除中...",
"batchDeleteConfirmTitle": "批量删除 Skill",
"batchDeleteConfirmMessage": "将删除选中的 {{count}} 个 Skill，每个 Skill 都会创建备份。\n\n此操作可通过恢复备份撤销。",
"batchDeleteConfirmAction": "删除所选",
"batchDeleteSuccess": "已成功删除 {{count}} 个 Skill",
"batchDeletePartial": "已删除 {{success}} 个 Skill，{{failed}} 个失败",
"batchDeleteFailed": "批量删除失败，请稍后重试",
"batchToggleSuccess": "已成功更新 {{count}} 个 Skill",
"batchTogglePartial": "已更新 {{success}} 个 Skill，{{failed}} 个失败",
"batchToggleFailed": "批量更新失败，请稍后重试",
```

- [ ] **Step 3: Add Japanese strings**

Open `src/locales/ja.json`. Add the same keys:

```json
"batchManage": "一括管理",
"batchManageExit": "一括管理を終了",
"batchSelectAll": "すべて選択",
"batchDeselectAll": "選択解除",
"batchSelectedCount": "{{count}} 件選択中",
"batchDelete": "選択を削除",
"batchDeleting": "削除中...",
"batchDeleteConfirmTitle": "Skill を一括削除",
"batchDeleteConfirmMessage": "選択した {{count}} 個の Skill を削除します。各 Skill のバックアップが作成されます。\n\nこの操作はバックアップから復元することで取り消せます。",
"batchDeleteConfirmAction": "選択を削除",
"batchDeleteSuccess": "{{count}} 個の Skill を削除しました",
"batchDeletePartial": "{{success}} 個の Skill を削除しましたが、{{failed}} 個が失敗しました",
"batchDeleteFailed": "一括削除に失敗しました。後でもう一度お試しください。",
"batchToggleSuccess": "{{count}} 個の Skill を更新しました",
"batchTogglePartial": "{{success}} 個の Skill を更新しましたが、{{failed}} 個が失敗しました",
"batchToggleFailed": "一括更新に失敗しました。後でもう一度お試しください。",
```

- [ ] **Step 4: Commit**

```bash
git add src/locales/en.json src/locales/zh.json src/locales/ja.json
git commit -m "feat(i18n): add batch skill management strings"
```

---

### Task 9: Frontend UI — selection mode state and batch operation bar

**Files:**
- Modify: `src/components/skills/UnifiedSkillsPanel.tsx`

- [ ] **Step 1: Add imports**

At the top of `UnifiedSkillsPanel.tsx`, add the missing imports. The file already imports `ConfirmDialog`, `toast`, `SKILLS_APP_IDS`, etc. Add `Checkbox` from shadcn/ui if not already available. First check if a Checkbox component exists:

```bash
ls src/components/ui/checkbox.tsx 2>/dev/null
```

If it doesn't exist, we'll use a plain `<input type="checkbox">` instead (simpler, avoids adding a dependency). Decide based on the result. For this plan, we'll use the native input approach.

Add these imports (adjust based on existing imports — the file already has `Button`, `Badge`, `TooltipProvider`, etc.):

```typescript
import { Check, Minus } from "lucide-react";
```

- [ ] **Step 2: Add state variables**

Inside the `UnifiedSkillsPanel` component function body, right after the existing state declarations (around line 80, after `restoreDialogOpen`), add:

```typescript
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
```

- [ ] **Step 3: Add batch mutation hook usage**

After the existing mutation hooks (around line 102, after `updateSkillMutation`), add:

```typescript
  const batchUninstallMutation = useBatchUninstallSkills();
  const batchToggleAppMutation = useBatchToggleSkillApp();
```

And add the import for these hooks at the top of the file (line 18, extend the import from `@/hooks/useSkills`):

```typescript
  useBatchUninstallSkills,
  useBatchToggleSkillApp,
```

- [ ] **Step 4: Add selection mode handlers**

After the `handleUpdateAll` function (around line 277), add:

```typescript
  const handleToggleSelection = (skillId: string) => {
    setSelectedSkills((prev) => {
      const next = new Set(prev);
      if (next.has(skillId)) {
        next.delete(skillId);
      } else {
        next.add(skillId);
      }
      return next;
    });
  };

  const handleSelectAll = () => {
    if (!skills) return;
    setSelectedSkills(new Set(skills.map((s) => s.id)));
  };

  const handleDeselectAll = () => {
    setSelectedSkills(new Set());
  };

  const handleBatchDelete = () => {
    const ids = Array.from(selectedSkills);
    if (ids.length === 0) return;
    setConfirmDialog({
      isOpen: true,
      title: t("skills.batchDeleteConfirmTitle"),
      message: t("skills.batchDeleteConfirmMessage", {
        count: ids.length,
      }),
      confirmText: t("skills.batchDeleteConfirmAction"),
      variant: "destructive",
      onConfirm: async () => {
        try {
          const results = await batchUninstallMutation.mutateAsync(ids);
          setConfirmDialog(null);
          const succeeded = results.filter((r) => r.success).length;
          const failed = results.filter((r) => !r.success).length;
          if (failed === 0) {
            toast.success(
              t("skills.batchDeleteSuccess", { count: succeeded }),
              { closeButton: true },
            );
          } else if (succeeded > 0) {
            toast.warning(
              t("skills.batchDeletePartial", { success: succeeded, failed }),
              { closeButton: true },
            );
          } else {
            toast.error(t("skills.batchDeleteFailed"), { closeButton: true });
          }
        } catch (error) {
          toast.error(t("skills.batchDeleteFailed"), {
            description: String(error),
          });
        }
      },
    });
  };

  const handleBatchToggleApp = async (app: AppId) => {
    const ids = Array.from(selectedSkills);
    if (ids.length === 0) return;
    // Determine the target state: if ANY selected skill has the app disabled, set enabled=true (majority-wins: enable)
    // Actually check: if MOST skills have it disabled, enable. Otherwise disable.
    let enabledCount = 0;
    for (const skill of skills ?? []) {
      if (selectedSkills.has(skill.id) && skill.apps[app]) {
        enabledCount++;
      }
    }
    const enabled = enabledCount < ids.length / 2; // enable if minority has it on

    try {
      const results = await batchToggleAppMutation.mutateAsync({
        ids,
        app,
        enabled,
      });
      const succeeded = results.filter((r) => r.success).length;
      const failed = results.filter((r) => !r.success).length;
      if (failed === 0) {
        toast.success(
          t("skills.batchToggleSuccess", { count: succeeded }),
          { closeButton: true },
        );
      } else if (succeeded > 0) {
        toast.warning(
          t("skills.batchTogglePartial", { success: succeeded, failed }),
          { closeButton: true },
        );
      } else {
        toast.error(t("skills.batchToggleFailed"), { closeButton: true });
      }
    } catch (error) {
      toast.error(t("skills.batchToggleFailed"), {
        description: String(error),
      });
    }
  };
```

- [ ] **Step 5: Add batch operation bar to JSX**

Find the existing header section in the JSX (around line 349, the `<div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">`). Right after the `<AppCountBar ... />` block (which is inside a `<div className="flex items-center justify-between">`), add the batch operation bar.

Insert after the closing `</div>` of the header row (the one containing `AppCountBar` and the update buttons), and before `<div className="flex-1 overflow-y-auto...">`:

```tsx
      {/* 批量操作栏 */}
      {selectionMode && (
        <div className="flex items-center gap-2 py-2 px-3 mt-2 rounded-lg border border-border-default bg-muted/30">
          <button
            type="button"
            className="flex items-center gap-1 text-xs"
            onClick={() => {
              if (selectedSkills.size === (skills?.length ?? 0)) {
                handleDeselectAll();
              } else {
                handleSelectAll();
              }
            }}
          >
            <div className={`w-4 h-4 rounded border flex items-center justify-center ${
              selectedSkills.size === (skills?.length ?? 0) && (skills?.length ?? 0) > 0
                ? "bg-primary border-primary"
                : "border-border-default"
            }`}>
              {selectedSkills.size === (skills?.length ?? 0) && (skills?.length ?? 0) > 0 && (
                <Check size={12} className="text-primary-foreground" />
              )}
            </div>
            <span className="text-muted-foreground">
              {selectedSkills.size === (skills?.length ?? 0)
                ? t("skills.batchDeselectAll")
                : t("skills.batchSelectAll")}
            </span>
          </button>

          <span className="text-xs text-muted-foreground">
            {t("skills.batchSelectedCount", { count: selectedSkills.size })}
          </span>

          <div className="flex-1" />

          {/* 应用切换图标组 */}
          <div className="flex items-center gap-1">
            {SKILLS_APP_IDS.map((app) => {
              const { label, icon, activeClass } = APP_ICON_MAP[app];
              return (
                <button
                  key={app}
                  type="button"
                  className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all hover:opacity-100 opacity-70`}
                  onClick={() => handleBatchToggleApp(app)}
                  disabled={
                    batchToggleAppMutation.isPending ||
                    selectedSkills.size === 0
                  }
                  title={`${t("skills.batchToggleApp")} ${label}`}
                >
                  {icon}
                </button>
              );
            })}
          </div>

          {/* 批量删除按钮 */}
          <Button
            type="button"
            variant="destructive"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={handleBatchDelete}
            disabled={
              batchUninstallMutation.isPending ||
              selectedSkills.size === 0
            }
          >
            {batchUninstallMutation.isPending ? (
              <Loader2 size={12} className="animate-spin" />
            ) : (
              <Trash2 size={12} />
            )}
            {batchUninstallMutation.isPending
              ? t("skills.batchDeleting")
              : t("skills.batchDelete")}
          </Button>
        </div>
      )}
```

- [ ] **Step 6: Add Batch Manage toggle button**

Find the existing button row with the check-updates and update-all buttons (around line 355). Add a Batch Manage button before them. Modify the `div` with `className="flex items-center gap-1.5"` to include:

```tsx
          <Button
            type="button"
            variant={selectionMode ? "default" : "ghost"}
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={() => {
              setSelectionMode(!selectionMode);
              setSelectedSkills(new Set());
            }}
          >
            {selectionMode
              ? t("skills.batchManageExit")
              : t("skills.batchManage")}
          </Button>
```

- [ ] **Step 7: Pass selection props to InstalledSkillListItem**

Find the `<InstalledSkillListItem ... />` usage in the JSX (around line 423). Add the new props:

```tsx
                  <InstalledSkillListItem
                    key={skill.id}
                    skill={skill}
                    hasUpdate={!!updatesMap[skill.id]}
                    isUpdating={
                      updateSkillMutation.isPending &&
                      updateSkillMutation.variables === skill.id
                    }
                    selectionMode={selectionMode}
                    isSelected={selectedSkills.has(skill.id)}
                    onToggleSelection={handleToggleSelection}
                    onToggleApp={handleToggleApp}
                    onUninstall={() => handleUninstall(skill)}
                    onUpdate={() => handleUpdateSkill(skill)}
                    isLast={index === skills.length - 1}
                  />
```

- [ ] **Step 8: Update InstalledSkillListItem component**

Find the `InstalledSkillListItem` component definition (around line 490). Update its props interface and component body:

```tsx
interface InstalledSkillListItemProps {
  skill: InstalledSkill;
  hasUpdate?: boolean;
  isUpdating?: boolean;
  selectionMode?: boolean;
  isSelected?: boolean;
  onToggleSelection?: (skillId: string) => void;
  onToggleApp: (id: string, app: AppId, enabled: boolean) => void;
  onUninstall: () => void;
  onUpdate?: () => void;
  isLast?: boolean;
}

const InstalledSkillListItem: React.FC<InstalledSkillListItemProps> = ({
  skill,
  hasUpdate,
  isUpdating,
  selectionMode,
  isSelected,
  onToggleSelection,
  onToggleApp,
  onUninstall,
  onUpdate,
  isLast,
}) => {
```

Then inside the `ListItemRow`, add a checkbox at the beginning when in selection mode. After `<ListItemRow isLast={isLast}>` and before `<div className="flex-1 min-w-0">`:

```tsx
      {selectionMode && onToggleSelection && (
        <div className="flex-shrink-0 mr-2">
          <button
            type="button"
            className={`w-4 h-4 rounded border flex items-center justify-center ${
              isSelected
                ? "bg-primary border-primary"
                : "border-border-default hover:border-primary/50"
            }`}
            onClick={() => onToggleSelection(skill.id)}
          >
            {isSelected && <Check size={12} className="text-primary-foreground" />}
          </button>
        </div>
      )}
```

- [ ] **Step 9: Run typecheck**

```bash
pnpm typecheck 2>&1 | tail -10
```

Expected: No errors.

- [ ] **Step 10: Commit**

```bash
git add src/components/skills/UnifiedSkillsPanel.tsx
git commit -m "feat(skill): add batch selection mode and batch operation bar"
```

---

### Task 10: Frontend component tests

**Files:**
- Modify: `tests/components/UnifiedSkillsPanel.test.tsx`

- [ ] **Step 1: Add batch operation hook mocks**

Open `tests/components/UnifiedSkillsPanel.test.tsx`. After the existing mock definitions (around line 15), add:

```typescript
const batchUninstallMock = vi.fn();
const batchToggleAppMock = vi.fn();
```

Then inside the `vi.mock("@/hooks/useSkills", () => ({...}))` block, add the new hooks:

```typescript
  useBatchUninstallSkills: () => ({
    mutateAsync: batchUninstallMock,
    isPending: false,
  }),
  useBatchToggleSkillApp: () => ({
    mutateAsync: batchToggleAppMock,
    isPending: false,
  }),
```

- [ ] **Step 2: Write selection mode tests**

After the existing test cases, add:

```typescript
describe("batch management", () => {
  it("shows batch manage button", async () => {
    render(
      <UnifiedSkillsPanel
        onOpenDiscovery={vi.fn()}
        currentApp="claude"
      />,
    );
    await waitFor(() => {
      expect(screen.getByText("skills.batchManage")).toBeInTheDocument();
    });
  });

  it("enters selection mode when batch manage button is clicked", async () => {
    render(
      <UnifiedSkillsPanel
        onOpenDiscovery={vi.fn()}
        currentApp="claude"
      />,
    );
    const batchBtn = screen.getByText("skills.batchManage");
    await act(async () => {
      batchBtn.click();
    });
    await waitFor(() => {
      expect(screen.getByText("skills.batchManageExit")).toBeInTheDocument();
    });
  });
});
```

- [ ] **Step 3: Run tests**

```bash
pnpm test:unit -- tests/components/UnifiedSkillsPanel 2>&1 | tail -20
```

Expected: All tests pass (existing + new).

- [ ] **Step 4: Commit**

```bash
git add tests/components/UnifiedSkillsPanel.test.tsx
git commit -m "test(skill): add batch management UI tests"
```

---

### Task 11: Final verification

**Files:**
- All modified files

- [ ] **Step 1: Run all pre-submission checks**

```bash
pnpm typecheck && pnpm format:check && pnpm test:unit
```

Expected: All pass.

- [ ] **Step 2: Run Rust checks**

```bash
cd src-tauri && cargo fmt --check && cargo clippy -- -D warnings && cargo test
```

Expected: All pass.

- [ ] **Step 3: Run Prettier format**

```bash
pnpm format
```

Expected: Files formatted successfully.

- [ ] **Step 4: Commit (if any format changes)**

```bash
git add -A
git commit -m "chore: apply formatting"
```

---

### Post-Implementation Checklist

- [ ] `pnpm typecheck` passes
- [ ] `pnpm format:check` passes
- [ ] `pnpm test:unit` passes
- [ ] `cargo clippy` passes
- [ ] `cargo test` passes
- [ ] i18n files updated (en, zh, ja)
- [ ] Batch delete creates backups (verifiable via restore dialog)
- [ ] Batch toggle app updates all selected skills
- [ ] Selection mode persists after operations
- [ ] Error handling shows correct success/failure counts
