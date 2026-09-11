# Lessons

## DSH 2.0.5 provider and credential surfaces

- DeepSeek Harness has two valid provider surfaces: the single `llm-deepseek` adapter and the multi-provider `llm-pi-ai.providers` catalog. Treating the whole app as a single exclusive `llm-deepseek` route hides real DSH providers such as `yuzuvalley`, `k3`, and `glm-5-3`.
- DSH 2.0.5 stores credentials in a versioned document: `version: 1`, references under `refs:`, and login records under `records:`. Writers that insert `DEEPSEEK_API_KEY` at the YAML root produce a layout the current official runtime rejects.
- Provider list synchronization should follow Pi's native-config pattern, but DSH needs source-aware writes: the official route maps to `llm-deepseek`; custom routes map by id under `llm-pi-ai.providers`; current selection maps to `agent-default-model.provider/model`.
- Manual "Import current config" must call a DSH-specific importer. The generic importer intentionally returns false for DSH, which otherwise leaves an empty page even when `~/.dsh/settings.yaml` is populated.

## DSH parity iteration (2026-09-11)

- DSH session files (`~/.dsh/sessions/<encoded-cwd>/<id>/session.jsonl.zstd`) contain no token usage fields; usage statistics require a different data source and must not be promised.
- `app_config.rs` PromptRoot is a legacy path for apps that predate the SQLite prompt store. New apps (Pi, DSH) belong in `services/prompt.rs` `first_launch_import_apps()`; editing PromptRoot adds dead state.
- `settings.rs` SETTINGS_STORE reads the real user settings file even in tests. Any per-app directory override consulted by path resolution must be bypassed under `cfg(test)` (pi's TEST_AGENT_DIR / dsh's `settings_override()` shim), or every env-isolated test breaks once the developer sets the override.
- `agent-default-model.provider/model` is coupled in DSH: setting a default model on a non-current provider switches the current provider. UI copy and reviewers should treat them as one action.
- Removing the currently selected DSH provider leaves a dangling `agent-default-model` pointer (pre-existing in both delete and remove-from-live paths).
