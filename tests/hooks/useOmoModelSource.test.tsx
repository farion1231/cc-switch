import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import type { PropsWithChildren } from "react";
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { useOmoModelSource } from "@/components/providers/forms/hooks/useOmoModelSource";

import { OmoFormFields } from "@/components/providers/forms/OmoFormFields";

const mocks = vi.hoisted(() => ({
  providers: {} as Record<string, unknown>,
  getLiveIds: vi.fn(),
  getModels: vi.fn(),
}));

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({ data: { providers: mocks.providers } }),
}));
vi.mock("@/lib/api", () => ({
  providersApi: { getOpenCodeLiveProviderIds: mocks.getLiveIds },
}));
vi.mock("@/lib/api/model-fetch", () => ({
  getOpenCodeModels: mocks.getModels,
}));
vi.mock("@/config/opencodeProviderPresets", () => ({
  OPENCODE_PRESET_MODEL_VARIANTS: {
    "test-sdk": [
      {
        id: "model",
        variants: {
          low: {},
          medium: { disabled: true },
          high: { disabled: false },
        },
        options: { reasoningEffort: "high" },
        contextLimit: 1000,
      },
    ],
  },
}));

function setModel(model: Record<string, unknown>) {
  mocks.providers = {
    provider: {
      name: "Provider",
      settingsConfig: { npm: "test-sdk", models: { model } },
    },
  };
}

function renderSource() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return renderHook(
    ({ enabled }) => useOmoModelSource({ isOmoCategory: enabled }),
    {
      initialProps: { enabled: true },
      wrapper: ({ children }: PropsWithChildren) => (
        <QueryClientProvider client={client}>{children}</QueryClientProvider>
      ),
    },
  );
}

async function waitForConfiguredModel(
  result: ReturnType<typeof renderSource>["result"],
) {
  await waitFor(() =>
    expect(result.current.omoModelOptions).toContainEqual({
      value: "provider/model",
      label: "Provider / model (model)",
    }),
  );
}

describe("useOmoModelSource variants", () => {
  beforeEach(() => {
    mocks.getLiveIds.mockResolvedValue(["provider"]);
    mocks.getModels.mockResolvedValue([]);
  });

  it("excludes only explicitly disabled config variants", async () => {
    setModel({
      variants: {
        low: {},
        medium: { disabled: true },
        high: { disabled: false },
        "": {},
      },
    });
    const { result } = renderSource();
    await waitForConfiguredModel(result);
    expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([
      "low",
      "high",
    ]);
  });

  it.each([{ low: { disabled: true }, high: { disabled: true } }])(
    "does not resurrect preset variants when all configured variants are disabled: %j",
    async (variants) => {
      setModel({ variants });
      const { result } = renderSource();
      await waitForConfiguredModel(result);
      expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([]);
      expect(result.current.omoPresetMetaMap["provider/model"]).toEqual({
        options: { reasoningEffort: "high" },
        limit: { context: 1000 },
      });
    },
  );

  it.each([{}, { variants: {} }])(
    "falls back to enabled preset variants when no variants are defined: %j",
    async (model) => {
      setModel(model);
      const { result } = renderSource();
      await waitForConfiguredModel(result);
      expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([
        "low",
        "high",
      ]);
    },
  );

  it("recomputes variants after config updates and leaving/reentering OMO", async () => {
    setModel({ variants: { low: {}, medium: {} } });
    const { result, rerender } = renderSource();
    await waitForConfiguredModel(result);
    expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([
      "low",
      "medium",
    ]);
    setModel({ variants: { low: {}, medium: { disabled: true } } });
    rerender({ enabled: true });
    expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([
      "low",
    ]);
    rerender({ enabled: false });
    expect(result.current.omoModelVariantsMap).toEqual({});
    rerender({ enabled: true });
    await waitForConfiguredModel(result);
    expect(result.current.omoModelVariantsMap["provider/model"]).toEqual([
      "low",
    ]);
  });
});

const originalScrollIntoView = Element.prototype.scrollIntoView;
beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});
afterAll(() => {
  Element.prototype.scrollIntoView = originalScrollIntoView;
});

describe.each([false, true])("OMO form variants (slim=%s)", (isSlim) => {
  it("offers enabled variants and keeps a disabled saved selection marked unavailable", async () => {
    mocks.getLiveIds.mockResolvedValue(["provider"]);
    mocks.getModels.mockResolvedValue([]);
    setModel({
      variants: {
        low: {},
        medium: { disabled: true },
        high: { disabled: false },
      },
    });
    const agents = {
      [isSlim ? "orchestrator" : "sisyphus"]: { model: "provider/model" },
    };
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const noop = () => {};
    const categories = {};
    function Form({ savedVariant }: { savedVariant?: string }) {
      const source = useOmoModelSource({ isOmoCategory: true });
      return (
        <OmoFormFields
          modelOptions={source.omoModelOptions}
          modelVariantsMap={source.omoModelVariantsMap}
          categories={categories}
          agents={
            savedVariant
              ? {
                  [isSlim ? "orchestrator" : "sisyphus"]: {
                    model: "provider/model",
                    variant: savedVariant,
                  },
                }
              : agents
          }
          onAgentsChange={noop}
          otherFieldsStr="{}"
          onOtherFieldsStrChange={noop}
          isSlim={isSlim}
        />
      );
    }
    const { rerender } = render(
      <QueryClientProvider client={client}>
        <Form />
      </QueryClientProvider>,
    );
    const trigger = await screen.findByTitle("(Default)");
    fireEvent.keyDown(trigger, { key: "Enter" });
    expect(
      await screen.findByRole("option", { name: "low" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "high" })).toBeInTheDocument();
    expect(
      screen.queryByRole("option", { name: "medium" }),
    ).not.toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    rerender(
      <QueryClientProvider client={client}>
        <Form savedVariant="medium" />
      </QueryClientProvider>,
    );
    expect(
      await screen.findByTitle("medium (current value, unavailable)"),
    ).toBeInTheDocument();
  });
});
