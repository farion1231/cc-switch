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

const caseFixtures: { label: string; headers: Record<string, string> }[] = [
  { label: "lowercase", headers: { "user-agent": "lower-agent" } },
  { label: "uppercase", headers: { "USER-AGENT": "upper-agent" } },
  { label: "mixed case", headers: { "uSeR-aGeNt": "mixed-agent" } },
  {
    label: "coexisting variants",
    headers: {
      "User-Agent": "canonical-agent",
      "user-agent": "lower-agent",
      "USER-AGENT": "upper-agent",
      "uSeR-aGeNt": "mixed-agent",
    },
  },
];
const preservedHeaders: Record<string, string> = JSON.parse(
  '{"X-Fixture":"keep","x-Custom":"case-sensitive-value","__proto__":"proto-value","constructor":"ctor-value","toString":"string-value"}',
);

describe.each(caseFixtures)(
  "useOpenclawFormState imported UA: $label",
  ({ headers }) => {
    it("recognizes initial UA without rewriting the imported configuration", () => {
      const test = mount({ ...preservedHeaders, ...headers });
      expect(test.result.current.openclawUserAgent).toBe(true);
      expect(test.read()).toEqual(test.original);
      expect(test.onSettingsConfigChange).not.toHaveBeenCalled();
    });

    it("reset recognizes UA and then clears it for headers without UA", () => {
      const test = mount();
      act(() => test.result.current.resetOpenclawState({ ...base, headers }));
      expect(test.result.current.openclawUserAgent).toBe(true);
      act(() =>
        test.result.current.resetOpenclawState({
          ...base,
          headers: preservedHeaders,
        }),
      );
      expect(test.result.current.openclawUserAgent).toBe(false);
      act(() => test.result.current.resetOpenclawState());
      expect(test.result.current.openclawUserAgent).toBe(false);
      expect(test.onSettingsConfigChange).not.toHaveBeenCalled();
    });

    it("normalizes all variants on enable and removes all on disable", () => {
      const test = mount({ ...preservedHeaders, ...headers });
      test.toggle(true);
      expect(test.read()).toEqual({
        ...base,
        headers: {
          ...preservedHeaders,
          "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
        },
      });
      test.toggle(false);
      expect(test.read()).toEqual({ ...base, headers: preservedHeaders });
    });

    it("disabling sole imported UA variants removes the empty headers object", () => {
      const test = mount(headers);
      test.toggle(false);
      expect(test.read()).toEqual(base);
    });
  },
);

it("reset tolerates null headers from JSON without rewriting settings", () => {
  const test = mount({ "User-Agent": "fixture-agent" });
  const config = JSON.parse('{"headers":null}');
  act(() => test.result.current.resetOpenclawState(config));
  expect(test.result.current.openclawUserAgent).toBe(false);
  expect(test.read()).toEqual(test.original);
  expect(test.onSettingsConfigChange).not.toHaveBeenCalled();
});
