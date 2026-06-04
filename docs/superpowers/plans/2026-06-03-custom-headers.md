# Per-Provider Custom HTTP Headers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow users to configure custom HTTP headers on a per-provider basis, injected by the local proxy into upstream requests.

**Architecture:** A reusable `CustomHeadersEditor` React component is added to `ProviderForm.tsx`, reading and writing `settingsConfig.headers`. The proxy's `forwarder.rs` extracts these headers from the provider config and injects them into the final upstream request via a testable helper function.

**Tech Stack:** React 18 + TypeScript (frontend), Rust + Tauri (backend), vitest (frontend tests), cargo test (backend tests)

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/components/providers/forms/CustomHeadersEditor.tsx` | Create | Reusable key-value header editor UI |
| `tests/components/CustomHeadersEditor.test.tsx` | Create | Tests for the editor component |
| `src/components/providers/forms/ProviderForm.tsx` | Modify | Integrate CustomHeadersEditor into all provider forms |
| `src/i18n/locales/en.json` | Modify | English translations |
| `src/i18n/locales/zh.json` | Modify | Chinese translations |
| `src/i18n/locales/zh-TW.json` | Modify | Traditional Chinese translations |
| `src/i18n/locales/ja.json` | Modify | Japanese translations |
| `src-tauri/src/proxy/forwarder.rs` | Modify | Inject custom headers into upstream requests |

---

### Task 1: CustomHeadersEditor Component

**Files:**
- Create: `src/components/providers/forms/CustomHeadersEditor.tsx`
- Test: `tests/components/CustomHeadersEditor.test.tsx`

- [ ] **Step 1: Create the CustomHeadersEditor component**

Create `src/components/providers/forms/CustomHeadersEditor.tsx`:

```tsx
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Plus, Trash2 } from "lucide-react";

export interface CustomHeadersEditorProps {
  headers: Record<string, string>;
  onChange: (headers: Record<string, string>) => void;
}

export function CustomHeadersEditor({
  headers,
  onChange,
}: CustomHeadersEditorProps) {
  const { t } = useTranslation();
  const entries = Object.entries(headers);

  const handleAdd = () => {
    onChange({ ...headers, "": "" });
  };

  const handleRemove = (index: number) => {
    const newEntries = entries.filter((_, i) => i !== index);
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  const handleKeyChange = (index: number, newKey: string) => {
    const newEntries = entries.map(([k, v], i) => (i === index ? [newKey, v] : [k, v]));
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  const handleValueChange = (index: number, newValue: string) => {
    const newEntries = entries.map(([k, v], i) => (i === index ? [k, newValue] : [k, v]));
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">
          {t("customHeaders.title", { defaultValue: "自定义请求头" })}
        </Label>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleAdd}
          className="h-8 gap-1"
        >
          <Plus className="h-3.5 w-3.5" />
          {t("customHeaders.add", { defaultValue: "添加" })}
        </Button>
      </div>

      {entries.length === 0 && (
        <p className="text-sm text-muted-foreground">
          {t("customHeaders.empty", { defaultValue: "未配置自定义请求头" })}
        </p>
      )}

      {entries.map(([key, value], index) => (
        <div key={index} className="flex items-center gap-2">
          <Input
            placeholder={t("customHeaders.name", { defaultValue: "Header 名称" })}
            value={key}
            onChange={(e) => handleKeyChange(index, e.target.value)}
            className="flex-1"
          />
          <Input
            placeholder={t("customHeaders.value", { defaultValue: "Header 值" })}
            value={value}
            onChange={(e) => handleValueChange(index, e.target.value)}
            className="flex-1"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => handleRemove(index)}
            className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Run TypeScript type check**

Run: `pnpm typecheck`
Expected: PASS (no errors from the new component)

- [ ] **Step 3: Create the component test**

Create `tests/components/CustomHeadersEditor.test.tsx`:

```tsx
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { CustomHeadersEditor } from "@/components/providers/forms/CustomHeadersEditor";

describe("CustomHeadersEditor", () => {
  it("renders empty state", () => {
    render(<CustomHeadersEditor headers={{}} onChange={vi.fn()} />);
    expect(screen.getByText("未配置自定义请求头")).toBeInTheDocument();
  });

  it("adds a header row", () => {
    const onChange = vi.fn();
    render(<CustomHeadersEditor headers={{}} onChange={onChange} />);

    fireEvent.click(screen.getByText("添加"));

    expect(onChange).toHaveBeenCalledWith({ "": "" });
  });

  it("removes a header row", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "" }));

    expect(onChange).toHaveBeenCalledWith({});
  });

  it("calls onChange when key is updated", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    const keyInput = screen.getAllByRole("textbox")[0];
    fireEvent.change(keyInput, { target: { value: "X-Custom" } });

    expect(onChange).toHaveBeenCalledWith({ "X-Custom": "Test" });
  });

  it("calls onChange when value is updated", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    const valueInput = screen.getAllByRole("textbox")[1];
    fireEvent.change(valueInput, { target: { value: "Claude Code" } });

    expect(onChange).toHaveBeenCalledWith({ "User-Agent": "Claude Code" });
  });
});
```

- [ ] **Step 4: Run the component test**

Run: `pnpm test:unit tests/components/CustomHeadersEditor.test.tsx`
Expected: All 5 tests PASS

---

### Task 2: Integrate CustomHeadersEditor into ProviderForm

**Files:**
- Modify: `src/components/providers/forms/ProviderForm.tsx`

- [ ] **Step 1: Add helper functions and import**

At the top of `ProviderForm.tsx`, add the import:

```tsx
import { CustomHeadersEditor } from "./CustomHeadersEditor";
```

Add these helper functions near the other helpers in the file (before `ProviderForm` component):

```tsx
function parseCustomHeaders(settingsConfig: string): Record<string, string> {
  try {
    const config = JSON.parse(settingsConfig || "{}") as Record<string, unknown>;
    const headers = config.headers;
    if (headers && typeof headers === "object" && !Array.isArray(headers)) {
      return headers as Record<string, string>;
    }
  } catch {
    // ignore parse errors
  }
  return {};
}

function updateCustomHeaders(
  settingsConfig: string,
  headers: Record<string, string>,
): string {
  try {
    const config = JSON.parse(settingsConfig || "{}") as Record<string, unknown>;
    if (Object.keys(headers).length > 0) {
      config.headers = headers;
    } else {
      delete config.headers;
    }
    return JSON.stringify(config, null, 2);
  } catch {
    return settingsConfig;
  }
}
```

- [ ] **Step 2: Render CustomHeadersEditor in the form**

In `ProviderForm.tsx`, find the block that renders `ProviderAdvancedConfig` (around line 2306-2316). Insert `CustomHeadersEditor` right before it:

```tsx
          {/* 自定义请求头 */}
          <CustomHeadersEditor
            headers={parseCustomHeaders(form.getValues("settingsConfig"))}
            onChange={(headers) => {
              const updated = updateCustomHeaders(
                form.getValues("settingsConfig"),
                headers,
              );
              form.setValue("settingsConfig", updated);
            }}
          />

          {!isAnyOmoCategory && ...
```

This places the custom headers editor between the app-specific config editors and the advanced config section, making it available for all provider types.

- [ ] **Step 3: Run TypeScript type check**

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 4: Run frontend tests**

Run: `pnpm test:unit`
Expected: All existing tests still PASS (new component tests + existing tests)

---

### Task 3: i18n Translations

**Files:**
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`

- [ ] **Step 1: Add English translations**

In `src/i18n/locales/en.json`, add a new top-level key (place it alphabetically near other top-level keys):

```json
  "customHeaders": {
    "title": "Custom Headers",
    "add": "Add Header",
    "name": "Header Name",
    "value": "Header Value",
    "empty": "No custom headers configured"
  },
```

- [ ] **Step 2: Add Chinese (Simplified) translations**

In `src/i18n/locales/zh.json`, add:

```json
  "customHeaders": {
    "title": "自定义请求头",
    "add": "添加",
    "name": "Header 名称",
    "value": "Header 值",
    "empty": "未配置自定义请求头"
  },
```

- [ ] **Step 3: Add Chinese (Traditional) translations**

In `src/i18n/locales/zh-TW.json`, add:

```json
  "customHeaders": {
    "title": "自訂請求頭",
    "add": "新增",
    "name": "Header 名稱",
    "value": "Header 值",
    "empty": "未設定自訂請求頭"
  },
```

- [ ] **Step 4: Add Japanese translations**

In `src/i18n/locales/ja.json`, add:

```json
  "customHeaders": {
    "title": "カスタムヘッダー",
    "add": "追加",
    "name": "ヘッダー名",
    "value": "ヘッダー値",
    "empty": "カスタムヘッダーが設定されていません"
  },
```

- [ ] **Step 5: Verify i18n compiles**

Run: `pnpm typecheck`
Expected: PASS

---

### Task 4: Backend — Inject Custom Headers in Proxy Forwarder

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs`

- [ ] **Step 1: Add the helper function**

In `src-tauri/src/proxy/forwarder.rs`, add a new helper function near the other helper functions (before the `RequestForwarder` impl block, around line 270):

```rust
/// Extract custom headers from provider.settings_config["headers"] and inject them
/// into the upstream request HeaderMap. Custom headers override existing headers.
fn inject_provider_custom_headers(
    ordered_headers: &mut http::HeaderMap,
    provider: &Provider,
) {
    let custom = provider
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

    for (name, value) in custom {
        ordered_headers.insert(name, value);
    }
}
```

- [ ] **Step 2: Call the helper in the forward method**

In `forwarder.rs`, inside `Forwarder::forward()`, find the block after content-type enforcement (around line 1600-1605). Insert the call right after the content-type `if` block and before `reject_proxy_placeholder_for_managed_account_upstream`:

```rust
        // 确保 content-type 存在
        if !ordered_headers.contains_key(http::header::CONTENT_TYPE) {
            ordered_headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("application/json"),
            );
        }

        // 注入供应商自定义请求头（覆盖已有同名头）
        inject_provider_custom_headers(&mut ordered_headers, provider);

        reject_proxy_placeholder_for_managed_account_upstream(&url, &ordered_headers)?;
```

- [ ] **Step 3: Verify Rust compiles**

Run:
```bash
cd src-tauri
cargo check
```
Expected: `Finished dev [unoptimized + debuginfo] target(s) in Xs`

---

### Task 5: Backend Test for Custom Header Injection

**Files:**
- Modify: `src-tauri/src/proxy/forwarder.rs` (test module)

- [ ] **Step 1: Add the Rust test**

In `src-tauri/src/proxy/forwarder.rs`, inside the `#[cfg(test)] mod tests` block (after the existing tests, around line 2476), add:

```rust
    #[test]
    fn inject_provider_custom_headers_adds_new_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-existing", HeaderValue::from_static("keep"));

        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "headers": {
                    "User-Agent": "Claude Code",
                    "X-Custom": "value"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        inject_provider_custom_headers(&mut headers, &provider);

        assert_eq!(
            headers.get("x-existing").unwrap().to_str().unwrap(),
            "keep"
        );
        assert_eq!(
            headers.get("User-Agent").unwrap().to_str().unwrap(),
            "Claude Code"
        );
        assert_eq!(
            headers.get("X-Custom").unwrap().to_str().unwrap(),
            "value"
        );
    }

    #[test]
    fn inject_provider_custom_headers_overrides_existing() {
        let mut headers = HeaderMap::new();
        headers.insert("User-Agent", HeaderValue::from_static("Original"));

        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "headers": {
                    "User-Agent": "Claude Code"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        inject_provider_custom_headers(&mut headers, &provider);

        assert_eq!(
            headers.get("User-Agent").unwrap().to_str().unwrap(),
            "Claude Code"
        );
    }

    #[test]
    fn inject_provider_custom_headers_skips_invalid_names() {
        let mut headers = HeaderMap::new();

        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({
                "headers": {
                    "Valid-Header": "ok",
                    "": "empty-key",
                    "Also\nInvalid": "bad"
                }
            }),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        inject_provider_custom_headers(&mut headers, &provider);

        assert_eq!(
            headers.get("Valid-Header").unwrap().to_str().unwrap(),
            "ok"
        );
        assert!(!headers.contains_key(""));
    }

    #[test]
    fn inject_provider_custom_headers_noop_when_missing() {
        let mut headers = HeaderMap::new();
        headers.insert("x-existing", HeaderValue::from_static("keep"));

        let provider = Provider {
            id: "test".to_string(),
            name: "Test".to_string(),
            settings_config: json!({}),
            website_url: None,
            category: None,
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        };

        inject_provider_custom_headers(&mut headers, &provider);

        assert_eq!(
            headers.get("x-existing").unwrap().to_str().unwrap(),
            "keep"
        );
        assert_eq!(headers.len(), 1);
    }
```

- [ ] **Step 2: Run the Rust tests**

Run:
```bash
cd src-tauri
cargo test inject_provider_custom_headers
```
Expected: All 4 tests PASS

---

### Task 6: Full Verification

- [ ] **Step 1: Run frontend type check**

Run: `pnpm typecheck`
Expected: PASS

- [ ] **Step 2: Run frontend unit tests**

Run: `pnpm test:unit`
Expected: All tests PASS (including new CustomHeadersEditor tests)

- [ ] **Step 3: Run Rust checks**

Run:
```bash
cd src-tauri
cargo clippy
cargo test
```
Expected: clippy passes with no warnings; all tests PASS

- [ ] **Step 4: Format check**

Run: `pnpm format:check`
Expected: PASS (no unformatted files)

Run:
```bash
cd src-tauri
cargo fmt --check
```
Expected: PASS

---

## Self-Review

### Spec Coverage Checklist

| Spec Requirement | Plan Task |
|---|---|
| Custom headers stored in `settingsConfig.headers` | Task 2 (helpers), Task 4 (Rust extraction) |
| Backend injection in forwarder.rs | Task 4 |
| Reusable CustomHeadersEditor component | Task 1 |
| Added to all provider forms | Task 2 |
| i18n for 4 languages | Task 3 |
| Frontend component tests | Task 1 |
| Backend Rust tests | Task 5 |
| Override semantics (insert replaces existing) | Task 5 test #2 |
| No breaking changes / additive only | All tasks |

### Placeholder Scan

- No "TBD", "TODO", or "implement later" found.
- All code blocks contain complete, runnable code.
- All test assertions are concrete.
- No vague instructions like "add appropriate error handling".

### Type Consistency

- `CustomHeadersEditorProps.headers` is `Record<string, string>` everywhere.
- `parseCustomHeaders` and `updateCustomHeaders` use the same shape.
- Rust `inject_provider_custom_headers` extracts `settings_config["headers"]` as an object, consistent with the JSON structure.
