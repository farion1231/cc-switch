import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useOpencodeFormState } from "@/components/providers/forms/hooks/useOpencodeFormState";

const renderOpencodeFormState = (
  initialSettingsConfig: Record<string, unknown>,
) => {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((nextConfig: string) => {
    settingsConfig = nextConfig;
  });

  const hook = renderHook(() =>
    useOpencodeFormState({
      appId: "opencode",
      initialData: { settingsConfig: initialSettingsConfig },
      onSettingsConfigChange,
      getSettingsConfig: () => settingsConfig,
    }),
  );

  const setSettingsConfig = (nextConfig: Record<string, unknown>) => {
    settingsConfig = JSON.stringify(nextConfig);
    hook.rerender();
  };

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
    setSettingsConfig,
  };
};

describe("useOpencodeFormState", () => {
  it("edits an existing built-in override without inventing a package or models", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      name: "OpenCode Go",
      options: { apiKey: "test-key" },
    });

    expect(result.current.opencodeNpm).toBe("");
    expect(result.current.opencodeModels).toEqual({});
    act(() => result.current.handleOpencodeApiKeyChange("edited-test-key"));
    expect(JSON.parse(getSettingsConfig())).toEqual({
      name: "OpenCode Go",
      options: { apiKey: "edited-test-key" },
    });
  });

  it("hydrates provider headers from options", () => {
    const { result } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "HTTP-Referer": "https://cc-switch.app",
          "X-Title": "CC Switch",
        },
      },
      models: {},
    });

    expect(result.current.opencodeHeaders).toEqual({
      "HTTP-Referer": "https://cc-switch.app",
      "X-Title": "CC Switch",
    });
  });

  it("writes provider headers to options", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {},
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({
        "X-Title": "CC Switch",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toEqual({
      "X-Title": "CC Switch",
    });
  });

  it("removes options.headers when all provider headers are removed", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "X-Title": "CC Switch",
        },
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({});
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toBeUndefined();
  });

  it("preserves legitimate headers whose names start with header-", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        headers: {
          "header-version": "v1",
          "X-Title": "Old",
        },
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeHeadersChange({
        "header-version": "v1",
        "X-Title": "New",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options.headers).toEqual({
      "header-version": "v1",
      "X-Title": "New",
    });
  });

  it("preserves legitimate options whose names start with option-", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: {
        "option-mode": "legacy",
        timeout: 100,
      },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeExtraOptionsChange({
        "option-mode": "legacy",
        timeout: "200",
        "draft-option:123": "",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      "option-mode": "legacy",
      timeout: 200,
    });
  });

  it("keeps untouched extra option values verbatim when another option is edited", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: {
        name: "123",
        flag: "false",
        missing: "null",
        list: "[]",
        map: "{}",
        timeout: 100,
      },
      models: { fixture: { name: "Fixture model" } },
    });

    act(() => {
      result.current.handleOpencodeExtraOptionsChange({
        ...result.current.opencodeExtraOptions,
        timeout: "200",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      name: "123",
      flag: "false",
      missing: "null",
      list: "[]",
      map: "{}",
      timeout: 200,
    });
  });

  it("keeps untouched extra option values verbatim when an option is added", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: { name: "123", timeout: 100 },
      models: {},
    });

    act(() => {
      // Mirrors handleAddExtraOption: a draft row is appended to the map.
      result.current.handleOpencodeExtraOptionsChange({
        ...result.current.opencodeExtraOptions,
        "draft-option:123": "",
      });
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      name: "123",
      timeout: 100,
    });
  });

  it("keeps untouched extra option values verbatim when a different option is removed", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: { name: "123", timeout: 100, legacy: "false" },
      models: {},
    });

    const rows = { ...result.current.opencodeExtraOptions };
    delete rows.legacy;
    act(() => {
      result.current.handleOpencodeExtraOptionsChange(rows);
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      name: "123",
      timeout: 100,
    });
  });

  it("removes extra options whose keys collide with Object.prototype properties", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: {
        constructor: "custom-value",
        toString: "false",
        timeout: 100,
      },
      models: {},
    });

    const rows = { ...result.current.opencodeExtraOptions };
    // Mirrors handleRemoveExtraOption, which deletes by a runtime key.
    for (const key of ["constructor", "toString"]) delete rows[key];
    act(() => {
      result.current.handleOpencodeExtraOptionsChange(rows);
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      timeout: 100,
    });
  });

  it("drops the old key when an extra option is renamed away from a prototype-named key", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: { valueOf: "3", timeout: 100 },
      models: {},
    });

    // Mirrors handleExtraOptionKeyChange: rebuild the row map with the new key.
    const rows: Record<string, string> = {};
    for (const [k, v] of Object.entries(result.current.opencodeExtraOptions)) {
      rows[k === "valueOf" ? "count" : k] = v;
    }
    act(() => {
      result.current.handleOpencodeExtraOptionsChange(rows);
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      count: "3",
      timeout: 100,
    });
  });

  it("keeps a renamed extra option's stored value type when only the key changes", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: { name: "123", timeout: 100 },
      models: {},
    });

    // Mirrors handleExtraOptionKeyChange: the row's display text moves to
    // the new key untouched.
    const rows: Record<string, string> = {};
    for (const [k, v] of Object.entries(result.current.opencodeExtraOptions)) {
      rows[k === "name" ? "label" : k] = v;
    }
    act(() => {
      result.current.handleOpencodeExtraOptionsChange(rows);
    });

    const options = JSON.parse(getSettingsConfig()).options;
    expect(options.label).toBe("123");
    expect(options).not.toHaveProperty("name");
  });

  it("does not mistake a kept key with the same display text for a rename source", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai",
      options: { name: "100", timeout: 100 },
      models: {},
    });

    // Rename name -> label while timeout (also displayed as "100") stays.
    const rows: Record<string, string> = {};
    for (const [k, v] of Object.entries(result.current.opencodeExtraOptions)) {
      rows[k === "name" ? "label" : k] = v;
    }
    act(() => {
      result.current.handleOpencodeExtraOptionsChange(rows);
    });

    expect(JSON.parse(getSettingsConfig()).options).toEqual({
      label: "100",
      timeout: 100,
    });
  });

  it("syncs a known MiniMax base URL when switching to Anthropic", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://api.minimax.cn/v1" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/anthropic");
    expect(result.current.opencodeBaseUrl).toBe(
      "https://api.minimax.cn/anthropic",
    );
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://api.minimax.cn/anthropic" },
    });
  });

  it("syncs a known MiniMax base URL back to the OpenAI path", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://api.minimax.io/anthropic" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/openai-compatible");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/openai-compatible");
    expect(result.current.opencodeBaseUrl).toBe("https://api.minimax.io/v1");
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://api.minimax.io/v1" },
    });
  });

  it("never overwrites a custom base URL when changing API format", () => {
    const { result, getSettingsConfig } = renderOpencodeFormState({
      npm: "@ai-sdk/openai-compatible",
      options: { baseURL: "https://relay.example.com/v1" },
      models: {},
    });

    act(() => {
      result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
    });

    expect(result.current.opencodeNpm).toBe("@ai-sdk/anthropic");
    expect(result.current.opencodeBaseUrl).toBe("https://relay.example.com/v1");
    expect(JSON.parse(getSettingsConfig())).toMatchObject({
      npm: "@ai-sdk/anthropic",
      options: { baseURL: "https://relay.example.com/v1" },
    });
  });

  it.each([
    [
      "custom relay",
      "https://relay.example.com/anthropic",
      "https://relay.example.com/anthropic",
    ],
    [
      "URL with query",
      "https://api.minimax.cn/v1?tenant=example",
      "https://api.minimax.cn/v1?tenant=example",
    ],
    [
      "international MiniMax endpoint",
      "https://api.minimax.io/anthropic",
      "https://api.minimax.io/anthropic",
    ],
  ] as const)(
    "uses the latest JSON-edited base URL for %s",
    (_caseName, editedBaseUrl, expectedBaseUrl) => {
      const { result, getSettingsConfig, setSettingsConfig } =
        renderOpencodeFormState({
          npm: "@ai-sdk/openai-compatible",
          options: { baseURL: "https://api.minimax.cn/v1" },
          models: {},
        });

      act(() => {
        setSettingsConfig({
          npm: "@ai-sdk/openai-compatible",
          options: { baseURL: editedBaseUrl },
          models: {},
        });
      });

      act(() => {
        result.current.handleOpencodeNpmChange("@ai-sdk/anthropic");
      });

      expect(result.current.opencodeBaseUrl).toBe(expectedBaseUrl);
      expect(JSON.parse(getSettingsConfig())).toMatchObject({
        npm: "@ai-sdk/anthropic",
        options: { baseURL: expectedBaseUrl },
      });
    },
  );
});
