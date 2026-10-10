import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { OPENCLAW_DEFAULT_USER_AGENT } from "@/components/providers/forms/hooks/useOpenclawFormState";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

// Match existing provider-form tests: mock ancillary auth/editor dependencies only.
// The actual ProviderForm, switch component, hook and onSubmit path remain real.
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea readOnly value={value} />
  ),
}));
vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      accounts: [],
    }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
  };
});

const base = {
  baseUrl: "https://example.invalid/v1",
  apiKey: "synthetic-fixture",
  api: "openai-completions",
  models: [{ id: "fixture", name: "Fixture model" }],
  authHeader: false,
  fixtureUnknown: { keep: [1, "two", false] },
};
const customHeaders = { "X-Fixture": "keep", "X-Other": "also-keep" };

async function mount(headers?: Record<string, string>) {
  const settingsConfig = { ...base, ...(headers ? { headers } : {}) };
  const onSubmit = vi.fn();
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="openclaw"
        providerId="fixture-provider"
        initialData={{ name: "Fixture provider", settingsConfig }}
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
  await screen.findByText("该供应商已添加到应用配置中，供应商标识不可修改");
  const section = screen
    .getByText("发送 User-Agent")
    .closest("div.border-l") as HTMLElement;
  const toggle = () => fireEvent.click(within(section).getByRole("switch"));
  const save = async () => {
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const actual = JSON.parse(onSubmit.mock.calls[0][0].settingsConfig);
    return actual;
  };
  return {
    settingsConfig,
    toggle,
    switchElement: within(section).getByRole("switch"),
    save,
  };
}

beforeEach(() => {
  // All Tauri calls are intercepted fixtures; any unexpected request fails closed.
  server.listen({ onUnhandledRequest: "error" });
  server.use(
    http.post("http://tauri.local/get_openclaw_live_provider_ids", () =>
      HttpResponse.json(["fixture-provider"]),
    ),
  );
});
describe("ProviderForm OpenClaw User-Agent headers", () => {
  it("enabling User-Agent preserves headers in final submission", async () => {
    const test = await mount(customHeaders);
    test.toggle();
    expect(await test.save()).toEqual({
      ...base,
      headers: { ...customHeaders, "User-Agent": OPENCLAW_DEFAULT_USER_AGENT },
    });
  });
  it("disabling User-Agent preserves headers in final submission", async () => {
    const test = await mount({
      ...customHeaders,
      "User-Agent": "fixture-agent",
    });
    test.toggle();
    expect(await test.save()).toEqual({ ...base, headers: customHeaders });
  });
  it("on-off round trip preserves headers in final submission", async () => {
    const test = await mount(customHeaders);
    test.toggle();
    test.toggle();
    expect(await test.save()).toEqual(test.settingsConfig);
  });
  it("save with no toggle preserves custom headers and User-Agent", async () => {
    const test = await mount({
      ...customHeaders,
      "User-Agent": "fixture-agent",
    });
    expect(await test.save()).toEqual(test.settingsConfig);
  });
  it("missing headers can enable User-Agent in final submission", async () => {
    const test = await mount();
    test.toggle();
    expect(await test.save()).toEqual({
      ...base,
      headers: { "User-Agent": OPENCLAW_DEFAULT_USER_AGENT },
    });
  });
  it("sole User-Agent can be removed in final submission", async () => {
    const test = await mount({ "User-Agent": "fixture-agent" });
    test.toggle();
    expect(await test.save()).toEqual(base);
  });
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
// JSON.parse creates own prototype-named properties, just like imported JSON.
const preservedHeaders: Record<string, string> = JSON.parse(
  '{"X-Fixture":"keep","x-Custom":"case-sensitive-value","__proto__":"proto-value","constructor":"ctor-value","toString":"string-value"}',
);

describe.each(caseFixtures)(
  "ProviderForm imported UA: $label",
  ({ headers }) => {
    const importedHeaders = { ...preservedHeaders, ...headers };

    it("recognizes an imported User-Agent in the initial switch", async () => {
      const test = await mount(importedHeaders);
      expect(test.switchElement).toHaveAttribute("aria-checked", "true");
    });

    it("keeps untouched imported headers and all settings on save", async () => {
      const test = await mount(importedHeaders);
      expect(await test.save()).toEqual(test.settingsConfig);
    });

    it("turning the switch off removes every UA variant from submission", async () => {
      const test = await mount(importedHeaders);
      if (test.switchElement.getAttribute("aria-checked") === "true")
        test.toggle();
      expect(await test.save()).toEqual({ ...base, headers: preservedHeaders });
    });

    it("turning the switch on submits one canonical default UA", async () => {
      const test = await mount(importedHeaders);
      if (test.switchElement.getAttribute("aria-checked") === "true")
        test.toggle();
      test.toggle();
      expect(await test.save()).toEqual({
        ...base,
        headers: {
          ...preservedHeaders,
          "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
        },
      });
    });

    it("on-off round trip removes imported UA variants from submission", async () => {
      const test = await mount(importedHeaders);
      if (test.switchElement.getAttribute("aria-checked") === "false")
        test.toggle();
      test.toggle();
      expect(await test.save()).toEqual({ ...base, headers: preservedHeaders });
    });

    it("repeated off-on toggles preserve unrelated fields and one UA", async () => {
      const test = await mount(importedHeaders);
      if (test.switchElement.getAttribute("aria-checked") === "true")
        test.toggle();
      for (let i = 0; i < 3; i++) {
        test.toggle();
        test.toggle();
      }
      test.toggle();
      expect(await test.save()).toEqual({
        ...base,
        headers: {
          ...preservedHeaders,
          "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
        },
      });
    });
  },
);
