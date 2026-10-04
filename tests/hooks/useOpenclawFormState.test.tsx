import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  useOpenclawFormState,
  OPENCLAW_DEFAULT_USER_AGENT,
} from "@/components/providers/forms/hooks/useOpenclawFormState";

// Only ancillary provider queries are mocked; the hook is unmodified production code.
vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({ data: { providers: {} } }),
}));

const base = {
  baseUrl: "https://example.invalid/v1",
  apiKey: "synthetic-fixture",
  api: "openai-completions",
  models: [{ id: "fixture", name: "Fixture model" }],
  authHeader: false,
  fixtureUnknown: { keep: [1, "two", false] },
};
const customHeaders = { "X-Fixture": "keep", "X-Other": "also-keep" };

function mount(headers?: Record<string, string>) {
  const original = { ...base, ...(headers ? { headers } : {}) };
  let serialized = JSON.stringify(original);
  const onSettingsConfigChange = vi.fn((next: string) => {
    serialized = next;
  });
  const hook = renderHook(() =>
    useOpenclawFormState({
      appId: "openclaw",
      providerId: "fixture-provider",
      initialData: { settingsConfig: original },
      getSettingsConfig: () => serialized,
      onSettingsConfigChange,
    }),
  );
  const toggle = (enabled: boolean) =>
    act(() => hook.result.current.handleOpenclawUserAgentChange(enabled));
  return {
    ...hook,
    original,
    toggle,
    read: () => JSON.parse(serialized),
    onSettingsConfigChange,
  };
}

describe("useOpenclawFormState User-Agent headers", () => {
  it("enabling User-Agent preserves unrelated headers", () => {
    const test = mount(customHeaders);
    test.toggle(true);
    expect(test.read().headers).toEqual({
      ...customHeaders,
      "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
    });
  });
  it("disabling User-Agent preserves unrelated headers", () => {
    const test = mount({ ...customHeaders, "User-Agent": "fixture-agent" });
    test.toggle(false);
    expect(test.read().headers).toEqual(customHeaders);
  });
  it("enabling then disabling preserves unrelated headers", () => {
    const test = mount(customHeaders);
    test.toggle(true);
    test.toggle(false);
    expect(test.read()).toEqual(test.original);
  });
  it("untouched custom User-Agent and headers survive hydration", () => {
    const test = mount({ ...customHeaders, "User-Agent": "fixture-agent" });
    expect(test.result.current.openclawUserAgent).toBe(true);
    expect(test.read()).toEqual(test.original);
    expect(test.onSettingsConfigChange).not.toHaveBeenCalled();
  });
  it("missing headers can enable the default User-Agent", () => {
    const test = mount();
    expect(test.result.current.openclawUserAgent).toBe(false);
    test.toggle(true);
    expect(test.read()).toEqual({
      ...base,
      headers: { "User-Agent": OPENCLAW_DEFAULT_USER_AGENT },
    });
  });
  it("disabling sole User-Agent leaves no headers", () => {
    const test = mount({ "User-Agent": "fixture-agent" });
    test.toggle(false);
    expect(test.read()).toEqual(base);
  });
  it.each([true, false])(
    "toggle %s preserves all non-header config",
    (enabled) => {
      const test = mount({ ...customHeaders, "User-Agent": "fixture-agent" });
      test.toggle(enabled);
      const { headers: _headers, ...rest } = test.read();
      expect(rest).toEqual(base);
    },
  );
});
