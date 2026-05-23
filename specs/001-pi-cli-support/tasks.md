# Tasks: Pi CLI 配置管理支持

**Input**: Design documents from `specs/001-pi-cli-support/`

**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

**Tests**: Not explicitly requested in spec — tests omitted. Add test tasks if desired.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **Backend (Rust)**: `src-tauri/src/`
- **Frontend (React/TS)**: `src/`
- **Tests**: `tests/` (frontend), `src-tauri/tests/` (backend)
- **Pi config files** (target, not source): `~/.pi/agent/models.json`, `~/.pi/agent/settings.json`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Register Pi as a new tool type in core project infrastructure

- [x] T001 [P] Add `Pi` variant to `AppType` enum with `as_str()`, `FromStr`, `all()`, `is_additive_mode()` in `src-tauri/src/app_config.rs`
- [x] T002 [P] Add `pi: bool` field to `McpApps` struct with `is_enabled_for`, `set_enabled_for`, `enabled_apps`, `is_empty` updates in `src-tauri/src/app_config.rs`
- [x] T003 [P] Add `pi: bool` field to `SkillApps` struct with `is_enabled_for`, `set_enabled_for`, `enabled_apps`, `is_empty` updates in `src-tauri/src/app_config.rs`
- [x] T004 [P] Add `pi: Option<String>` field to `CommonConfigSnippets` with getter/setter in `src-tauri/src/app_config.rs`
- [x] T005 [P] Add `pi: bool` field (default `true`) to `VisibleApps` struct with `is_visible` match arm in `src-tauri/src/settings.rs`
- [x] T006 [P] Add `pi_config_dir: Option<String>` and `current_provider_pi: Option<String>` fields to settings struct in `src-tauri/src/settings.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core backend infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [x] T007 Create `pi_config.rs` module skeleton with path functions (`get_pi_dir`, `get_models_json_path`, `get_settings_json_path`) in `src-tauri/src/pi_config.rs`
- [x] T008 Implement `read_models_json()` function with JSON parsing and existing-provider preservation in `src-tauri/src/pi_config.rs`
- [x] T009 Implement `write_models_json()` function with atomic write (temp file → validate → rename) and `cc-switch-` prefix merge strategy in `src-tauri/src/pi_config.rs`
- [x] T010 Implement `read_settings_json()` function with field-aware parsing in `src-tauri/src/pi_config.rs`
- [x] T011 Implement `write_settings_json()` function with managed-field-only merge (preserve unknown fields) and atomic write in `src-tauri/src/pi_config.rs`
- [x] T012 Register `pi_config` module in `src-tauri/src/lib.rs`
- [x] T013 [P] Add Pi i18n keys (zh/en/ja): tab label, provider labels, settings labels, status messages in `src/i18n/zh.json`, `src/i18n/en.json`, `src/i18n/ja.json`

**Checkpoint**: Foundation ready — Pi config read/write infrastructure exists, user story implementation can now begin

---

## Phase 3: User Story 1 — 添加 Pi 作为新的 CLI 工具选项卡 (Priority: P1) 🎯 MVP

**Goal**: 用户在主界面看到 Pi 选项卡，可以切换进入 Pi 管理界面

**Independent Test**: 打开 CC Switch 设置，在"可见应用"中启用 Pi，验证主界面出现 Pi 选项卡并可切换

### Implementation for User Story 1

- [x] T014 [P] [US1] Extend `AppType` on frontend to include `'pi'` with icon and label mapping in `src/types.ts`
- [x] T015 [P] [US1] Add Pi tab entry to sidebar navigation component in `src/components/AppSwitcher.tsx`
- [x] T016 [US1] Create Pi context state (current provider, settings, providers list) in `src/contexts/AppContext.tsx` (handled via AppId extension in App.tsx)
- [ ] T017 [US1] Create Pi empty state component: prompts user to add first provider, links to quickstart info in `src/components/pi/PiEmptyState.tsx`
- [ ] T018 [US1] Create Pi main page layout (tab content area with empty state or provider list) in `src/components/pi/PiPage.tsx`
- [ ] T019 [US1] Wire Pi visibility toggle to existing settings page (VisibleApps section) in settings components

**Checkpoint**: Pi 选项卡在主界面可见，可切换进入空状态页面，设置中可控制显示/隐藏

---

## Phase 4: User Story 2 — Pi 提供商配置管理 (Priority: P1)

**Goal**: 用户可通过 CC Switch 添加、编辑、删除、切换 Pi 的提供商配置

**Independent Test**: 在 Pi 选项卡中添加 Anthropic 提供商，填入 API 密钥，点击"设为当前"，验证 `~/.pi/agent/models.json` 和 `settings.json` 正确更新

### Implementation for User Story 2

- [ ] T020 [P] [US2] Define `PiProviderConfig` Rust struct with fields (baseUrl, api, apiKey, models, compat) in `src-tauri/src/pi_config.rs`
- [ ] T021 [P] [US2] Implement `get_pi_providers()` — read models.json, extract `cc-switch-` prefixed providers, return typed structs in `src-tauri/src/pi_config.rs`
- [ ] T022 [US2] Implement `set_pi_provider(id, config)` — write single provider to models.json with merge strategy in `src-tauri/src/pi_config.rs`
- [ ] T023 [US2] Implement `remove_pi_provider(id)` — remove provider from models.json, clear defaultProvider if active in `src-tauri/src/pi_config.rs`
- [ ] T024 [US2] Implement `set_active_pi_provider(id, model_id)` — write defaultProvider/defaultModel to settings.json in `src-tauri/src/pi_config.rs`
- [ ] T025 [US2] Add Pi provider presets (Anthropic, OpenAI, Google, DeepSeek, OpenRouter, Groq, Cerebras, xAI, Mistral, Fireworks, Custom) in `src-tauri/src/database/dao/providers_seed.rs`
- [ ] T026 [US2] Create Tauri commands: `get_pi_providers`, `add_pi_provider`, `update_pi_provider`, `delete_pi_provider`, `set_active_pi_provider` in `src-tauri/src/commands/pi.rs`
- [ ] T027 [US2] Register Pi Tauri commands in `src-tauri/src/commands/mod.rs`
- [ ] T028 [P] [US2] Create Pi provider card component (name, API type badge, active indicator, actions menu) in `src/components/pi/PiProviderCard.tsx`
- [ ] T029 [P] [US2] Create Pi provider list component (sortable cards with drag-and-drop) in `src/components/pi/PiProviderList.tsx`
- [ ] T030 [US2] Create Pi provider editor form (preset selector, API key input, Base URL, model selection, API type dropdown) in `src/components/pi/PiProviderForm.tsx`
- [ ] T031 [US2] Create `usePiProviders` hook (fetch, add, update, delete, setActive) in `src/hooks/usePiProviders.ts`
- [ ] T032 [US2] Integrate provider list and form into Pi main page (T018), replacing empty state when providers exist in `src/components/pi/PiPage.tsx`

**Checkpoint**: Pi 提供商完整 CRUD + 切换功能可用，配置正确写入 models.json 和 settings.json

---

## Phase 5: User Story 3 — Pi Skills 管理集成 (Priority: P2)

**Goal**: 用户可在统一 Skills 管理面板中安装、卸载、同步 Skills 到 Pi

**Independent Test**: 在 Skills 管理面板中启用 Pi 作为目标应用，安装一个 Skill，验证 `~/.pi/agent/skills/` 目录中出现该 Skill

### Implementation for User Story 3

- [ ] T033 [P] [US3] Register Pi skills target path (`~/.pi/agent/skills/`) in skill sync service in `src-tauri/src/services/skill.rs`
- [ ] T034 [P] [US3] Add Pi skills directory detection and creation logic in `src-tauri/src/services/skill.rs`
- [ ] T035 [US3] Add "Pi" checkbox to Skills management UI (install dialog, per-skill app selector) in skills components
- [ ] T036 [US3] Wire Pi skill enable/disable to `SkillApps.pi` field in skill store operations in `src-tauri/src/services/skill.rs`

**Checkpoint**: Skills 可以安装/卸载到 Pi 目录，CC Switch 能扫描识别 Pi 目录中已有的 Skills

---

## Phase 6: User Story 4 — Pi Settings 可视化管理 (Priority: P2)

**Goal**: 用户可通过图形界面编辑 Pi 的常用设置，无需手动编辑 JSON

**Independent Test**: 修改 Pi 思考级别为 "high"，验证 `~/.pi/agent/settings.json` 中 `defaultThinkingLevel` 更新为 `"high"`

### Implementation for User Story 4

- [ ] T037 [P] [US4] Implement `get_pi_settings()` — read settings.json, extract managed fields in `src-tauri/src/pi_config.rs`
- [ ] T038 [P] [US4] Implement `update_pi_settings(fields)` — merge-managed-fields into settings.json with atomic write in `src-tauri/src/pi_config.rs`
- [ ] T039 [US4] Create Tauri commands: `get_pi_settings`, `update_pi_settings` in `src-tauri/src/commands/pi.rs`
- [ ] T040 [US4] Create Pi settings panel UI (thinkingLevel dropdown, theme dropdown, hideThinkingBlock toggle, quietStartup toggle, compaction toggle, retry toggle + maxRetries input) in `src/components/pi/PiSettings.tsx`
- [ ] T041 [US4] Integrate settings panel as a tab/sub-page in Pi main page in `src/components/pi/PiPage.tsx`

**Checkpoint**: 图形化设置面板可用，修改后 settings.json 正确更新且保留未知字段

---

## Phase 7: User Story 5 — Pi Context Files 管理 (Priority: P3)

**Goal**: 用户可通过 CC Switch 编辑 Pi 的 AGENTS.md，并与其它工具的上下文文件同步

**Independent Test**: 在 Prompt 编辑器中编辑 Pi 的 AGENTS.md，验证 `~/.pi/agent/AGENTS.md` 被正确写入

### Implementation for User Story 5

- [ ] T042 [P] [US5] Register Pi AGENTS.md path (`~/.pi/agent/AGENTS.md`) and SYSTEM.md path (`~/.pi/agent/SYSTEM.md`) in prompt file registry in `src-tauri/src/prompt_files.rs`
- [ ] T043 [US5] Add Pi as source/target option in cross-app prompt sync configuration in prompt sync components
- [ ] T044 [US5] Wire Pi prompt file read/write through existing prompt file service (reuse existing atomic write + backup) in `src-tauri/src/prompt_files.rs`

**Checkpoint**: Pi 的 AGENTS.md 可在 Prompt 编辑器中编辑和跨应用同步

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [ ] T045 [P] Add Pi CLI installation detection (check `~/.pi/agent/` directory existence, show "未检测到 Pi" banner if missing) in `src/components/pi/PiPage.tsx`
- [ ] T046 [P] Add custom config directory support for Pi (respect `pi_config_dir` override setting) in `src-tauri/src/pi_config.rs`
- [ ] T047 Code cleanup: verify all Pi code uses atomic writes, proper error handling, and i18n
- [ ] T048 Run quickstart.md validation — execute user-facing operations end-to-end
- [ ] T049 [P] Add Pi icon asset (SVG/PNG) for sidebar and provider cards in `src/assets/` and import references

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **User Story 1 (Phase 3)**: Depends on Foundational — Pi tab visible
- **User Story 2 (Phase 4)**: Depends on Foundational — can run in parallel with US1
- **User Story 3 (Phase 5)**: Depends on Foundational — can run in parallel with US1, US2
- **User Story 4 (Phase 6)**: Depends on Foundational — can run in parallel with US1-US3
- **User Story 5 (Phase 7)**: Depends on Foundational — can run in parallel with US1-US4
- **Polish (Phase 8)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: No dependencies on other stories
- **User Story 2 (P1)**: No dependencies on other stories — but shares PiPage.tsx with US1
- **User Story 3 (P2)**: No dependencies on other stories
- **User Story 4 (P2)**: No dependencies on other stories — but shares PiPage.tsx with US1-US2
- **User Story 5 (P3)**: No dependencies on other stories

### Within Each User Story

- Backend types/structs before CRUD functions
- CRUD functions before Tauri commands
- Tauri commands before frontend hooks
- Frontend hooks before UI components
- UI sub-components before page integration

### Parallel Opportunities

- All Setup tasks (T001–T006) can run in parallel
- All Foundational tasks that don't share same file can run in parallel
- US3, US4, US5 can all start in parallel after Foundational phase
- Frontend components within a story marked [P] can run in parallel

---

## Parallel Example: User Story 2

```bash
# Backend types + presets (different files, no deps):
Task: "Define PiProviderConfig Rust struct in src-tauri/src/pi_config.rs" (T020)
Task: "Add Pi provider presets in src-tauri/src/database/dao/providers_seed.rs" (T025)

# Frontend components (different files, no deps):
Task: "Create Pi provider card component in src/components/pi/PiProviderCard.tsx" (T028)
Task: "Create Pi provider list component in src/components/pi/PiProviderList.tsx" (T029)

# After hooks + components ready:
Task: "Create Pi provider editor form in src/components/pi/PiProviderForm.tsx" (T030)
Task: "Integrate into PiPage.tsx" (T032)
```

---

## Implementation Strategy

### MVP First (User Story 1 + 2)

1. Complete Phase 1: Setup (T001–T006)
2. Complete Phase 2: Foundational (T007–T013)
3. Complete Phase 3: User Story 1 — Pi tab (T014–T019)
4. Complete Phase 4: User Story 2 — Provider management (T020–T032)
5. **STOP and VALIDATE**: Add Anthropic provider → switch → verify `pi` works
6. Deploy/demo if ready

### Incremental Delivery

1. Setup + Foundational → Foundation ready
2. Add US1 + US2 → Core Pi provider management works → **MVP!**
3. Add US3 → Skills sync → Test independently
4. Add US4 → Settings UI → Test independently
5. Add US5 → Context files → Test independently
6. Polish → Production ready

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: US1 (Pi tab) → then US2 (providers)
   - Developer B: US3 (Skills) → then US5 (context files)
   - Developer C: US4 (Settings)
3. Stories complete and integrate independently

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Pi uses additive mode — all providers coexist in models.json
- CC Switch providers use `cc-switch-` prefix namespace to avoid conflicts
- All config writes MUST use atomic operations (per constitution Principle III)
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
