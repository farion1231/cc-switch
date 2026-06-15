# Batch Skill Management Design

> Date: 2025-06-15
> Status: Approved
> Author: Claude

## Overview

Add batch management capabilities to the installed skills panel, supporting:
- Batch delete installed skills (with backup)
- Batch toggle app enable/disable state for selected skills

## Requirements

### Functional Requirements

1. **Batch Delete**: Users can select multiple skills and delete them at once
   - Each deletion creates a backup (restorable)
   - Confirmation dialog before deletion
   - Toast notification with success/failure count

2. **Batch Toggle App**: Users can select multiple skills and toggle their app enable/disable state
   - Direct app icon buttons in batch operation bar
   - Supports: Claude, Codex, Gemini, OpenCode, Hermes
   - Toast notification with success/failure count

3. **Selection Mode**:
   - "Batch Manage" button to enter/exit selection mode
   - Checkboxes on each skill item in selection mode
   - Select All / Deselect All functionality
   - Selected count display
   - Stay in selection mode after operations (exit by clicking "Batch Manage" again)

### Non-Functional Requirements

- Error handling: "best effort" strategy - succeed as many as possible, report success/failure count
- Performance: Backend batch commands (single IPC call)
- Consistency: Follow existing sessions batch delete implementation pattern

## Architecture

### Data Flow

```
User Selection → Frontend State → Backend Batch Command → Loop Processing → Return Results → Update Cache
```

### Key Components

1. **Frontend**:
   - `UnifiedSkillsPanel.tsx` - Add selection mode and batch operation UI
   - `InstalledSkillListItem.tsx` - Add checkbox
   - `useSkills.ts` - Add batch operation hooks
   - `skills.ts` - Add batch API wrappers

2. **Backend**:
   - `skill.rs` (commands) - Add batch commands
   - `skill.rs` (service) - Add batch business logic
   - `skills.rs` (DAO) - Add batch database operations

## Detailed Design

### Backend Implementation

#### Data Structures

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSkillRequest {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSkillResult {
    pub id: String,
    pub success: bool,
    pub error: Option<String>,
}
```

#### Commands

```rust
#[tauri::command]
pub fn batch_uninstall_skills(
    requests: Vec<BatchSkillRequest>,
    app_state: State<'_, AppState>,
) -> Result<Vec<BatchSkillResult>, String>

#[tauri::command]
pub fn batch_toggle_skill_app(
    requests: Vec<BatchSkillRequest>,
    app: String,
    enabled: bool,
    app_state: State<'_, AppState>,
) -> Result<Vec<BatchSkillResult>, String>
```

#### Service Layer

```rust
// Follows the same pattern as sessions batch delete
fn collect_batch_results<F>(
    requests: &[BatchSkillRequest],
    mut operation: F,
) -> Vec<BatchSkillResult>
where
    F: FnMut(&str) -> Result<bool, String>,
{
    requests
        .iter()
        .map(|request| match operation(&request.id) {
            Ok(true) => BatchSkillResult {
                id: request.id.clone(),
                success: true,
                error: None,
            },
            Ok(false) => BatchSkillResult {
                id: request.id.clone(),
                success: false,
                error: Some("Operation failed".to_string()),
            },
            Err(error) => BatchSkillResult {
                id: request.id.clone(),
                success: false,
                error: Some(error),
            },
        })
        .collect()
}
```

### Frontend Implementation

#### State Management

```typescript
const [selectionMode, setSelectionMode] = useState(false);
const [selectedSkills, setSelectedSkills] = useState<Set<string>>(new Set());
```

#### API Wrappers

```typescript
async batchUninstall(requests: {id: string}[]): Promise<BatchSkillResult[]> {
    return await invoke("batch_uninstall_skills", { requests });
},

async batchToggleApp(
    requests: {id: string}[],
    app: AppId,
    enabled: boolean
): Promise<BatchSkillResult[]> {
    return await invoke("batch_toggle_skill_app", { requests, app, enabled });
},
```

#### Hooks

```typescript
export function useBatchUninstallSkills() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: (requests: {id: string}[]) => skillsApi.batchUninstall(requests),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ["skills", "installed"] });
        },
    });
}

export function useBatchToggleSkillApp() {
    const queryClient = useQueryClient();
    return useMutation({
        mutationFn: ({ requests, app, enabled }: {
            requests: {id: string}[];
            app: AppId;
            enabled: boolean;
        }) => skillsApi.batchToggleApp(requests, app, enabled),
        onSuccess: () => {
            queryClient.invalidateQueries({ queryKey: ["skills", "installed"] });
        },
    });
}
```

#### UI Components

1. **UnifiedSkillsPanel** - Batch operation bar:
   - Batch Manage button (enter/exit selection mode)
   - Select All / Deselect All checkbox
   - Batch Delete button
   - App toggle icon group (Claude/Codex/Gemini/OpenCode/Hermes)
   - Selected count display

2. **InstalledSkillListItem** - Checkbox:
   - Show checkbox in selection mode
   - Hide checkbox in normal mode

### User Interaction Flow

1. Click "Batch Manage" → Enter selection mode
2. Check skills → Selected count updates
3. Click Batch Delete → Confirmation dialog → Call API → Show result
4. Click App Icon → Call API → Show result
5. Click "Batch Manage" again → Exit selection mode

### Error Handling & User Feedback

**Confirmation Dialog** (ConfirmDialog component):
- Title: `skills.batchDeleteConfirmTitle`
- Message: `skills.batchDeleteConfirmMessage`
- Confirm: `skills.batchDeleteConfirmAction`

**Success Toast** (toast.success):
- Title: `skills.batchDeleteSuccess`
- Description: `skills.backup.location` (if backup path available)

**Partial Success Toast** (toast.warning):
- Title: `skills.batchDeletePartial`
- Description: First failure error message

**Failure Toast** (toast.error):
- Title: `skills.batchDeleteFailed`
- Description: Error message

**Cache Update Strategy**:
- Directly update `installed` cache on success (optimistic update)
- Remove/update corresponding skills from cache
- Keep original data for failed operations

### i18n Text

```json
{
  "skills": {
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
    "batchToggleFailed": "批量更新失败，请稍后重试"
  }
}
```

## Testing Strategy

### PR Requirements (from CLAUDE.md)

- `pnpm typecheck` must pass
- `pnpm format:check` must pass
- `pnpm test:unit` must pass
- `cargo clippy` must pass (Rust code changed)
- Update i18n files (user-facing text changed)

### Frontend Tests

- `useBatchUninstallSkills` hook tests
- `useBatchToggleSkillApp` hook tests
- Selection mode state management tests
- Select All / Deselect All logic tests

### Backend Tests

- `batch_uninstall_skills` command tests
- `batch_toggle_skill_app` command tests
- `collect_batch_results` helper function tests
- Partial failure scenario tests

## Implementation Plan

### Phase 1: Backend

1. Add data structures (`BatchSkillRequest`, `BatchSkillResult`)
2. Add service layer methods (`batch_uninstall`, `batch_toggle_app`)
3. Add commands (`batch_uninstall_skills`, `batch_toggle_skill_app`)
4. Write backend tests
5. Run `cargo clippy` and `cargo test`

### Phase 2: Frontend

1. Add API wrappers in `skills.ts`
2. Add hooks in `useSkills.ts`
3. Modify `UnifiedSkillsPanel` - add selection mode and batch operation UI
4. Modify `InstalledSkillListItem` - add checkbox
5. Write frontend tests
6. Run `pnpm typecheck` and `pnpm test:unit`

### Phase 3: i18n & Polish

1. Update `src/locales/en.json`
2. Update `src/locales/zh.json`
3. Update `src/locales/ja.json`
4. Run `pnpm format:check`
5. Final integration testing

## References

- Existing sessions batch delete implementation: `src-tauri/src/session_manager/mod.rs`
- Existing skill management: `src/components/skills/UnifiedSkillsPanel.tsx`
- PR Requirements: `CLAUDE.md`
