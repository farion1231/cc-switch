# Implementation Plan — Issue #8117: Custom (Uploaded) Provider Icons with Sync Support

> Issue: https://github.com/farion1231/cc-switch/issues/8117
> Title: 供应商图标支持上传任意本地图片（PNG/SVG/透明），并随云同步携带资源 / Upload arbitrary local provider icon with sync
> State: OPEN · Label: `enhancement` · Author: @hao-lin-lwh · Created: 2026-10-11
> Plan date: 2026-10-11 · Status: DRAFT (plan-first, not yet approved for implementation)

---

## Issue Evidence Summary (extracted from issue body + comments)

### Problem statement
The provider add/edit panel supports preset icons and icon search, but the icon field only accepts
**built-in icon names**. Many real-world providers — private relays (私有中转), self-hosted gateways
(自建网关), internal LLM endpoints — have no brand logo in the built-in library, so users cannot
display an appropriate icon. Third-party docs claim brand-library selection or custom icon upload is
possible, but it is not implemented.

### Proposed solution (from issue author)
- Add **upload local image** to the icon area of the add/edit provider form:
  - Support PNG / JPG / WEBP; ideally also SVG and transparent-background PNG.
  - Optionally allow **pasting an image URL** as an alternative.
- Storage: save locally under `~/.cc-switch/assets/{provider_id}.png` (or an equivalent DB field);
  record an `icon_source` in the config; must not break the existing built-in icon fallback.
- Display rules: uploaded icon takes priority; fall back to built-in icon / initials / brand color
  when missing.
- Cloud sync: when sync is enabled, offer a toggle "include icon assets / config only" so large
  images do not pollute the sync payload.
- Suggested limits: ≤256×256 px, ≤200 KB, square; separate (unstated) size limit for SVG.

### Expected behavior (derived from the proposal)
1. User opens the icon picker in the provider form → can upload a local image file or paste a URL.
2. The chosen image is persisted locally, associated with that provider, and shown everywhere the
   provider icon appears.
3. Missing image (e.g., after restore on a new machine without assets) degrades gracefully to the
   current fallback chain.
4. Sync can carry icon assets, or skip them per a user setting.

### Actual behavior (verified in code)
- `src/types.ts` — `Provider` has `icon?: string` ("图标名称（如 "openai"）") and
  `iconColor?: string`. No URL/file field, no `icon_source`.
- `src/components/IconPicker.tsx` — renders only `iconList` from `src/icons/extracted` with text
  search (`searchIcons`). No upload button, no URL input.
- `src/components/ProviderIcon.tsx` — rendering chain is: inline SVG via `hasIcon(icon)` →
  `<img>` via `isUrlIcon(icon)` (a **curated allowlist** in `iconUrls`, not arbitrary URLs) →
  initials fallback. Arbitrary URLs and local files are never rendered.
- `src/components/providers/forms/BasicFormFields.tsx` — icon dialog wraps `IconPicker` only.
- `src/lib/schemas/provider.ts` — `icon: z.string().optional()` (no format constraint).
- Rust backend — providers are persisted in SQLite (`src-tauri/src/database/`); there is **no
  command** for saving/loading per-provider image assets.
- Cloud sync exists as **WebDAV** (`src-tauri/src/services/webdav_sync.rs`,
  `webdav_sync/archive.rs`, `webdav_auto_sync.rs`, `sync_protocol.rs`) and **S3**
  (`src-tauri/src/services/s3_sync.rs`). Whether the sync archive can carry binary assets was
  **not verified in this pass** (see open questions).

### Issue comment (1 total)
@yukitakasama (2026-10-11): confirmed by code inspection that the feature is missing — icon field
stores built-in names only (`src/types.ts`), `IconPicker.tsx` supports only built-in search, no
upload/URL entry in the frontend, no Rust-side per-provider icon asset logic. Deems the request
reasonable.

### Reproduction steps
1. Open Add/Edit Provider → click the icon area.
2. Observe the picker offers only built-in icon search.
3. There is no way to upload a local image or use an arbitrary image URL.

### Environment
- App: cc-switch (General / 通用, per issue form).
- Platforms affected: all (Tauri desktop app).
- No specific versions mentioned in the issue.

### Acceptance criteria (from issue; none formally enumerated)
Derived from the proposal:
- [ ] Upload local PNG/JPG/WEBP (and SVG) as provider icon.
- [ ] Optional paste-image-URL path.
- [ ] Uploaded icon wins over built-in; graceful fallback (built-in → initials/brand color).
- [ ] Icon assets persist per provider and survive app restart.
- [ ] Sync carries icon assets, with a user-visible toggle to sync config only.
- [ ] Validation: ≤256×256, ≤200 KB, square preferred; SVG has its own size limit.
- [ ] Existing built-icon behavior and fallback are unchanged (backward compatible).

---

## 1. Objective and Scope

**Objective:** Allow users to attach an arbitrary local image (or pasted image URL) as a provider's
icon, persisted locally, rendered ahead of the built-in fallback chain, and optionally included in
cloud (WebDAV/S3) sync.

**In scope:**
- Frontend: icon picker extension (upload + URL paste + preview + removal).
- Data model: extension of the provider icon representation (`icon_source` semantics).
- Backend (Rust/Tauri): commands to store/read/delete image assets under the app data dir;
  validation of type/size/dimensions.
- Rendering: `ProviderIcon` / `ProviderIconBox` support for local asset icons.
- Sync: inclusion of icon assets in the sync archive behind a settings toggle.
- Deeplink/import-export paths that copy providers (asset duplication or referencing decision —
  see §7).

**Out of scope (unless requested later):**
- Automatic icon fetching from provider base URL (favicon scraping).
- Image editing (crop/resize UI); we only validate dimensions, not transform them.
- Server-side hosting of icons.

## 2. Requirements and Acceptance Criteria

R1. In the provider form icon dialog, user can (a) pick a built-in icon (unchanged), (b) upload a
    local file (PNG/JPG/WEBP/SVG), (c) paste an http(s) image URL, (d) remove a custom icon.
R2. Uploaded files are validated: extension/MIME whitelist, ≤200 KB (raster), separate SVG cap
    (proposal: **assumption** — suggest 100 KB; issue does not state it), dimensions ≤256×256 for
    raster (SVG exempt from pixel check). Violations produce a clear, translatable error.
R3. Assets are stored in the app data dir (proposal names `~/.cc-switch/assets/{provider_id}.png`;
    actual dir must resolve via the existing app-config path helper, not a hardcoded tilde path),
    with a deterministic, extension-preserving filename per provider id + content hash.
R4. Provider record gains an `icon_source` discriminator (`builtin` | `local` | `url`) so the
    renderer can resolve correctly and the fallback chain survives a missing file.
R5. Rendering precedence everywhere `ProviderIcon` is used: `local` asset → `url` image → built-in
    SVG/URL allowlist → initials + brand color.
R6. Missing local asset (file deleted, fresh restore) → silent, safe fallback; no broken-image
    glyphs; provider switching unaffected.
R7. Sync: icon assets are included in WebDAV/S3 archives only when the user enables "include icon
    assets" (default **assumption**: off, per the issue's "avoid polluting sync with large images");
    config-only sync keeps icon references but assets may be absent on the peer (fallback covers it).
R8. All new UI strings are wired through i18n (zh + en at minimum).
R9. Existing providers with only `icon` names behave exactly as before; no migration required for
    them (additive schema change).

## 3. Proposed Technical Approach

### 3.1 Data model (frontend + backend)
Keep `icon?: string` for built-in names (zero migration) and add:
- `iconSource?: "builtin" | "local" | "url"` on the provider record.
- When `iconSource === "local"`, `icon` stores the asset key (e.g. `local/<providerId>-<hash8>` or
  the stored filename); when `"url"`, `icon` stores the raw http(s) URL.

Alternative considered: a separate `iconUrl?: string` field. Rejected — two fields invite
inconsistent states; one discriminator + one value field is easier to validate in zod and in the
Rust model. **Open question:** whether upstream prefers a new column or reusing `icon` (see §7 Q1).

### 3.2 Backend (Rust, src-tauri)
New module `src-tauri/src/services/provider_icons.rs` (or `commands/provider_icons.rs`) with Tauri
commands:
- `save_provider_icon(providerId: String, sourcePath: String, kind: "file") -> IconMeta` — copies
  (not moves) the picked file into `<app_data>/assets/icons/`, validates, computes hash, returns
  the stored key + dimensions.
- `save_provider_icon_from_url(providerId, url)` — optional phase-2; fetching URLs in the backend
  avoids CSP issues and lets us validate size before accepting.
- `read_provider_icon(key) -> String` — returns a `data:` URL or an `asset:` protocol URL for
  `<img src>`; prefer Tauri's asset protocol conversion for performance (no base64 inflation).
  **Assumption:** asset protocol scope must be added to `tauri.conf.json` (`assetProtocol.scope`).
- `delete_provider_icon(providerId)` — invoked on provider delete and on icon replacement.

Validation (shared Rust function, unit-testable): extension + magic-byte sniff (PNG `\x89PNG`,
JPEG `\xFF\xD8`, WEBP `RIFF....WEBP`, SVG `<svg` with XML sanity), size caps, and dimension read
via the existing image crate **if already a dependency — to verify**; otherwise decode dimensions
with a minimal header parser to avoid adding a heavy dependency.

### 3.3 Frontend
- `src/lib/api/providers.ts` (or new `src/lib/api/providerIcons.ts`): typed wrappers for the new
  commands.
- `src/lib/schemas/provider.ts`: extend zod schema — `iconSource` enum, conditional validation
  (URL shape for `url`, asset-key shape for `local`).
- `src/components/IconPicker.tsx`: add a top section "Custom icon" with Upload button (native file
  dialog via the save command's dialog, or `@tauri-apps/plugin-dialog` if already used — **verify
  current dialog plugin usage**) and a URL input; keep the built-in grid below. Show the current
  custom icon with a "remove" affordance.
- `src/components/ProviderIcon.tsx`: before the existing chain, resolve
  `iconSource === "local"` via `read_provider_icon` (React state, cached per key) and render `<img>`;
  for `iconSource === "url"` render `<img src={icon}>` with `onError` → fallback. Add a
  `useProviderIconSrc` hook so ProviderIconBox and tray reuse the same resolution.
- **Tray/menu icons (Rust side)**: if the tray renders provider icons, local assets must be loaded
  there too — **verify whether the tray currently uses `icon`** (open question Q4).

### 3.4 Sync
- `src-tauri/src/services/webdav_sync/archive.rs` (and the S3 equivalent): when building the sync
  archive, include `assets/icons/**` if the setting allows; on restore, unpack them. Archive
  format change must remain readable by older versions (**verify** how tolerant the reader is to
  extra entries — assumption: additive entries are ignored by old versions).
- New setting in `src-tauri/src/settings.rs` + Settings UI (`include_icon_assets_in_sync`, default
  off) surfaced in the sync settings panel.

## 4. Step-by-Step Implementation Tasks

| # | Task | Likely files |
|---|------|--------------|
| 1 | Verify tray/deeplink/import-export paths that consume `provider.icon`; list all render sites | `src-tauri/src/tray.rs`, `src-tauri/src/commands/deeplink.rs`, `src-tauri/src/commands/import_export.rs`, grep `<ProviderIcon` |
| 2 | Extend provider data model: `iconSource` on TS type + Rust struct (serde default for old rows) | `src/types.ts`, provider model under `src-tauri/src/database/` (schema/struct) |
| 3 | Implement `provider_icons` service: save/read/delete + validation (magic bytes, size, dims) + unit tests | new `src-tauri/src/services/provider_icons.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs` (command registration) |
| 4 | Tauri asset-protocol scope + CSP review for `<app_data>/assets/icons` | `src-tauri/tauri.conf.json` |
| 5 | Frontend API wrappers + zod schema extension | `src/lib/api/providers.ts`, `src/lib/schemas/provider.ts` |
| 6 | IconPicker extension: upload button, URL input, custom-icon preview + remove | `src/components/IconPicker.tsx`, `src/components/providers/forms/BasicFormFields.tsx` |
| 7 | Rendering: local/URL resolution with fallback + caching hook | `src/components/ProviderIcon.tsx`, `src/components/ProviderIconBox.tsx`, new `src/hooks/useProviderIconSrc.ts` |
| 8 | Deletion lifecycle: remove asset when provider deleted or icon replaced | provider delete service in `src-tauri/src/services/provider/` |
| 9 | Sync: archive inclusion toggle + restore path (WebDAV + S3) + setting UI | `src-tauri/src/services/webdav_sync/archive.rs`, `s3_sync.rs`, `settings.rs`, `src/components/settings/*`, `src/lib/api/settings.ts` |
| 10 | i18n strings (zh/en) for all new UI | `src/i18n/` locales |
| 11 | Tests (see §5) + docs touch-up if icon behavior is documented in README FAQ | `README*.md` FAQ sections (verify) |

Recommended commit split: (2,3,4) backend core → (5,6,7,10) frontend → (8) lifecycle → (9) sync.
Per repo convention: commits are drafted and reviewed by the user before any `git commit`.

## 5. Testing and Validation Strategy

**Unit (Rust, `cargo test -j 2 --no-fail-fast` per machine constraints):**
- `provider_icons` validation: accept/reject each magic-byte class; size caps (exactly at/over
  limit); dimension cap; SVG content sniffing; path-traversal attempts on the asset key
  (`../`, absolute paths, provider-id spoofing).
- Save/delete round-trip; orphan cleanup when provider deleted.

**Unit (frontend, vitest):**
- Zod schema: `iconSource` combinations; legacy records without `iconSource` still validate.
- `useProviderIconSrc` fallback: missing asset → fallback chain (assert initials rendered, no
  `<img>` with dead src).
- Reverse-validation discipline (repo convention): revert the `ProviderIcon` local-branch and
  confirm the new test FAILS before restoring.

**Integration:**
- Existing `tests/integration/App.test.tsx` icon dialog flow still passes (5s-timeout flakiness on
  this machine is environmental — note in PR if hit, per three-step flake triage).
- New integration test: open form → mock save command → form submits → provider list renders the
  custom icon.

**Manual:**
- Upload PNG with transparency / JPG / oversized / wrong-type-renamed (e.g. `.txt`→`.png`) / huge
  SVG with scripts (must be rejected or sanitized — SVG served as `<img>` does not execute scripts,
  which is the chosen mitigation).
- Restart app → icon persists. Delete provider → asset gone from disk.
- WebDAV sync with toggle on/off between two profiles; restore on "fresh" profile shows fallback
  when assets excluded.
- Built-in icon selection unchanged; old config file loads without migration.

## 6. Migration and Backward Compatibility

- **Additive only.** `iconSource` is optional with serde/zod defaults; rows lacking it resolve to
  `builtin` semantics (current behavior). No DB schema migration expected **if** providers are
  stored as JSON blobs in SQLite (verify `database/schema.rs` — if it is a typed column set, one
  `ALTER TABLE`/column-add step is needed).
- Existing `icon` values keep working; the URL-allowlist path (`isUrlIcon`) is untouched.
- Sync archives: new entries are additive; readers on older versions **assume** ignore unknown
  entries (verify in `archive.rs` unpack logic; if it errors on unknown entries, gate asset entries
  behind a manifest flag old readers skip).
- Import/export and deeplink flows that copy provider JSON: `iconSource: local` references an
  asset key that may not exist on the target — acceptable because rendering falls back; optionally
  embed assets in export archives later (out of scope this round unless trivial).

## 7. Risks, Trade-offs, and Unresolved Questions

**Risks**
- SVG XSS surface: mitigated by rendering via `<img>` (no script execution) and by magic-byte
  sniffing; do **not** inline user SVG with `dangerouslySetInnerHTML`.
- Sync payload growth: capped by per-file limits but N providers × 200 KB can bloat archives —
  hence the default-off toggle.
- Asset-protocol scope changes touch app security config — keep scope narrow to the icons dir.

**Trade-offs**
- Copy-on-save (asset duplicated outside user's file) vs reference original path: copy chosen for
  robustness (user may move/delete the source file).
- One discriminator field vs separate `iconUrl` field (§3.1).

**Open questions (to resolve with upstream maintainer / code verification before implementation)**
- Q1: Data model — reuse `icon` + `iconSource`, or add a dedicated field? (Upstream taste.)
- Q2: Does the WebDAV/S3 archive reader tolerate extra entries? (Read `archive.rs` first.)
- Q3: Is an image-dimension crate already in `Cargo.toml`? If not, header-parser to avoid a heavy dep.
- Q4: Does the tray (Rust) render provider icons today? If yes, local-asset support is required
  there too, else the tray shows fallback inconsistently with the UI.
- Q5: Exact SVG size cap — issue does not state one (suggested 100 KB).
- Q6: Default state of the "include icon assets" sync toggle (plan assumes off; issue implies
  concern about bloat but doesn't mandate).
- Q7: Whether URL-paste is phase 1 or phase 2 (issue lists it as "可选/optional").

## 8. Dependencies and Recommended Sequencing

- No external npm deps anticipated (file input handled by the dialog plugin already in the stack —
  verify; otherwise `@tauri-apps/plugin-dialog`).
- Rust: no new crates anticipated (see Q3).
- Sequencing: backend storage (tasks 2–4) → frontend render pipeline (5–7) must land **together**
  (a stored asset with no renderer is invisible; a renderer with no storage is dead code) →
  lifecycle (8) → sync (9) → i18n/docs (10–11). Sync is separable and could ship as a follow-up PR
  if review scope is a concern.

## 9. Estimated Effort / Complexity

**Overall: Medium (M).**

| Area | Estimate | Notes |
|------|----------|-------|
| Rust service + commands + validation tests | 1.5–2 days | Includes asset-protocol config |
| Frontend picker + rendering + fallback | 1–1.5 days | Fallback chain and caching are the fiddly parts |
| Sync integration + setting | 1–1.5 days | Contingent on archive.rs tolerance (Q2) |
| i18n, manual QA, PR prep | 0.5 day | zh/en strings, screenshots for PR |

Upstream CI risk is low (no new native deps); local `cargo test` must use the documented
`-j 2 --no-fail-fast` workaround.

---

*All citations above reference files inspected in the working tree at commit time of this plan;
upstream `main` moves fast — re-verify touched files before implementation.*
