# DSH Native Providers Design

## Goal

Make the DeepSeek Harness page reflect and manage the real DSH 2.0.5 provider state instead of showing an empty CC Switch-only catalog.

## Native ownership

DSH owns three related settings surfaces:

- `llm-deepseek`: the single `deepseek-official` route.
- `llm-pi-ai.providers`: a dictionary of custom and catalog-backed routes.
- `agent-default-model`: the current provider/model pair.

Secrets remain in `$DSH_HOME/.credentials.yaml` using the version-1 `refs` and `records` document. CC Switch may update one `refs.<NAME>` value but must preserve all other references, records, comments when possible, and the owner-only Unix mode.

## Provider model

Each native route is mirrored to the existing `providers` table under app type `deepseek-harness`:

- `deepseek-official` mirrors `llm-deepseek`.
- Every `llm-pi-ai.providers.<key>` mirrors one provider whose id is `<key>`.
- `meta.providerType` records `dsh_deepseek` or `dsh_pi_ai` so writes return to the correct namespace.
- The provider whose id matches `agent-default-model.provider` is current. Its selected model is persisted in provider metadata for display and switching.

Native files remain authoritative. Listing/importing DSH providers refreshes the mirrored DB rows using Pi's idempotent sync pattern. Missing native routes are not deleted automatically from the DB; they remain reusable saved entries, while current/live state comes only from the native settings.

## UI

- Remove DSH from frontend additive-app classification because current state is explicit.
- The empty-state import button calls a DSH-specific native importer.
- Provider cards display `baseURL`, current state, and the configured model list.
- Add/edit uses a DSH-specific form derived from Pi's provider form but constrained to DSH fields: route id, display name, API format, base URL, credential reference/API key, defaults, and models.
- Switching asks for a model when a provider has multiple models, then updates `agent-default-model`.

## Safety

- Parse YAML structurally and bound file size.
- Serialize settings atomically while preserving unrelated namespaces.
- Use the official version-1 credential structure and preserve `records`.
- Never copy credential values into logs, errors, project records, or UI responses beyond the existing masked form behavior.
- Validate route names, credential references, API formats, non-empty model ids, and provider/model membership before writing.

## Testing

- Rust tests cover DSH home resolution, version-1 credential read/write, both provider namespaces, current state, native-to-DB sync, switch, add/update/delete, unrelated YAML preservation, and rollback on DB failure where applicable.
- Frontend tests cover non-additive classification, DSH-specific import, URL extraction from `baseURL`, provider cards/current state, and DSH form serialization.
- Run focused suites, full TypeScript checks, full Vitest, Rust format, focused Rust tests, full Rust tests, and package/install visual verification.
