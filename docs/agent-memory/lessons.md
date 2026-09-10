# Lessons

## DSH 2.0.5 provider and credential surfaces

- DeepSeek Harness has two valid provider surfaces: the single `llm-deepseek` adapter and the multi-provider `llm-pi-ai.providers` catalog. Treating the whole app as a single exclusive `llm-deepseek` route hides real DSH providers such as `yuzuvalley`, `k3`, and `glm-5-3`.
- DSH 2.0.5 stores credentials in a versioned document: `version: 1`, references under `refs:`, and login records under `records:`. Writers that insert `DEEPSEEK_API_KEY` at the YAML root produce a layout the current official runtime rejects.
- Provider list synchronization should follow Pi's native-config pattern, but DSH needs source-aware writes: the official route maps to `llm-deepseek`; custom routes map by id under `llm-pi-ai.providers`; current selection maps to `agent-default-model.provider/model`.
- Manual "Import current config" must call a DSH-specific importer. The generic importer intentionally returns false for DSH, which otherwise leaves an empty page even when `~/.dsh/settings.yaml` is populated.
