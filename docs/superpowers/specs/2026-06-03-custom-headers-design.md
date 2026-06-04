# Per-Provider Custom HTTP Headers

## Problem

KimiCode restricts access based on request headers, returning 403 for non-whitelisted user agents:

> Kimi For Coding is currently only available for Coding Agents such as Kimi CLI, Claude Code, Roo Code, Kilo Code, etc.

When CC Switch's local proxy forwards Codex requests to a Kimi provider, Codex's default `User-Agent` is rejected. Users need a way to override request headers on a per-provider basis.

## Goals

- Allow users to configure custom HTTP headers for any provider
- Custom headers are injected into upstream requests by the local proxy
- Follow existing codebase patterns (OpenClaw already stores headers in `settingsConfig`)
- Minimal blast radius — additive change, no breaking changes

## Non-Goals

- Global proxy-level custom headers (out of scope; per-provider is sufficient)
- Header templating or dynamic values (e.g., timestamps, random strings)
- Per-request header overrides from the UI

## Design

### Data Model

Custom headers are stored in `Provider.settings_config["headers"]` as a JSON object:

```json
{
  "baseUrl": "https://api.example.com",
  "apiKey": "sk-xxx",
  "headers": {
    "User-Agent": "Claude Code",
    "X-Custom-Header": "value"
  }
}
```

- **No database schema migration** — `settings_config` is already a JSON `Value` column.
- OpenClaw's existing `headers` field uses the same key; this design generalizes that pattern.

### Backend — Proxy Forwarder

Injection point: `src-tauri/src/proxy/forwarder.rs`, inside `Forwarder::forward()`, after `ordered_headers` is fully assembled and before the request is sent.

**Algorithm**:

1. Extract `headers` from `provider.settings_config`:
   ```rust
   let custom_headers = provider
       .settings_config
       .get("headers")
       .and_then(|v| v.as_object())
       .map(|obj| {
           obj.iter()
               .filter_map(|(k, v)| {
                   let name = http::HeaderName::from_bytes(k.as_bytes()).ok()?;
                   let value = http::HeaderValue::from_str(v.as_str()?).ok()?;
                   Some((name, value))
               })
               .collect::<Vec<_>>()
       })
       .unwrap_or_default();
   ```

2. For each `(name, value)` in `custom_headers`, call `ordered_headers.insert(name, value)`.
   - `insert` replaces any existing header of the same name.
   - This ensures user-configured headers override client defaults and proxy-injected defaults.

3. Invalid header names or values are silently skipped (logged at `debug` level).

**Why this point?** `ordered_headers` is the final `HeaderMap` sent upstream. All auth, host, transform, and rectifier headers are already set. Provider custom headers act as the last override layer.

**Edge case — Authorization override**: If a user sets `Authorization` as a custom header, it replaces the proxy's injected auth. This is intentional — it gives power users full control. No special guard needed.

### Frontend — CustomHeadersEditor

New component: `src/components/providers/forms/CustomHeadersEditor.tsx`

**Props**:
```typescript
interface CustomHeadersEditorProps {
  headers: Record<string, string>;
  onChange: (headers: Record<string, string>) => void;
}
```

**Behavior**:
- Displays key-value rows (name input + value input + delete button)
- "Add header" button appends a new empty row
- Empty rows are filtered out on change
- Placed inside all provider form field components (ClaudeFormFields, CodexFormFields, GeminiFormFields, OpenCodeFormFields, HermesFormFields, OpenClawFormFields)
- For OpenClaw, placed below the existing User-Agent toggle. Both can coexist.

**Schema update**:
In `src/lib/schemas/provider.ts`, add to the provider form schema:
```typescript
customHeaders: z.record(z.string()).default({}),
```

On form save, `customHeaders` is serialized into `settingsConfig.headers`.

### i18n

Add translation keys to all four locale files:

```json
{
  "provider": {
    "customHeaders": {
      "title": "Custom Headers",
      "add": "Add Header",
      "name": "Header Name",
      "value": "Header Value",
      "remove": "Remove",
      "empty": "No custom headers configured"
    }
  }
}
```

Files to update:
- `src/i18n/locales/en.json`
- `src/i18n/locales/zh.json`
- `src/i18n/locales/zh-TW.json`
- `src/i18n/locales/ja.json`

### Testing

**Frontend**:
- Test `CustomHeadersEditor` in `tests/components/CustomHeadersEditor.test.tsx`:
  - Renders empty state
  - Adds a row
  - Removes a row
  - Calls `onChange` with correct object

**Backend**:
- Add test in `src-tauri/src/proxy/forwarder.rs` (test module at bottom):
  - Mock provider with `settings_config.headers`
  - Verify custom header is present in upstream request
  - Verify custom header overrides an existing header of same name

## Rollout

This is a pure additive feature:
- Existing providers without `settingsConfig.headers` behave exactly as before
- OpenClaw's existing `User-Agent` toggle continues to work
- No migration scripts needed
