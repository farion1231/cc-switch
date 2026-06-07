import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import type { CodexRoutingConfig } from "@/types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/components/ui/form", () => ({
  FormLabel: ({ children }: { children: ReactNode }) => <label>{children}</label>,
}));

function renderRoutingHarness(
  initialRouting?: CodexRoutingConfig,
  options: { shouldShowSpeedTest?: boolean } = {},
) {
  const onRoutingChange = vi.fn();
  let latestRouting: CodexRoutingConfig =
    initialRouting ?? { enabled: true, defaultRouteId: "", routes: [] };

  function Harness() {
    const [routing, setRouting] = useState<CodexRoutingConfig>(latestRouting);

    // 测试壳同步保存最新 route 配置，模拟 ProviderForm 对受控字段的回写。
    const handleRoutingChange = (next: CodexRoutingConfig) => {
      latestRouting = next;
      onRoutingChange(next);
      setRouting(next);
    };

    return (
      <CodexFormFields
        codexApiKey="sk-test"
        onApiKeyChange={vi.fn()}
        category="custom"
        shouldShowApiKeyLink={false}
        websiteUrl=""
        shouldShowSpeedTest={options.shouldShowSpeedTest ?? true}
        codexBaseUrl="https://api.example.com"
        onBaseUrlChange={vi.fn()}
        isFullUrl={false}
        onFullUrlChange={vi.fn()}
        isEndpointModalOpen={false}
        onEndpointModalToggle={vi.fn()}
        autoSelect={false}
        onAutoSelectChange={vi.fn()}
        apiFormat="openai_chat"
        onApiFormatChange={vi.fn()}
        codexRouting={routing}
        onCodexRoutingChange={handleRoutingChange}
        speedTestEndpoints={[]}
      />
    );
  }

  return {
    ...render(<Harness />),
    onRoutingChange,
    latestRouting: () => latestRouting,
  };
}

describe("CodexFormFields local model routing", () => {
  it("shows local model routing even when endpoint speed tools are hidden", () => {
    renderRoutingHarness(
      { enabled: false, defaultRouteId: "", routes: [] },
      { shouldShowSpeedTest: false },
    );

    expect(screen.getByText("Local model routing")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /add route/i })).toBeInTheDocument();
  });

  it("adds and edits a route through the dialog without persisting rowId", async () => {
    const { latestRouting } = renderRoutingHarness();

    fireEvent.click(screen.getByRole("button", { name: /add route/i }));

    await waitFor(() => {
      expect(screen.getByRole("dialog")).toBeInTheDocument();
      expect(latestRouting().routes).toHaveLength(1);
    });

    fireEvent.change(screen.getByPlaceholderText("route-id"), {
      target: { value: "deepseek" },
    });
    fireEvent.change(screen.getByPlaceholderText("Route label"), {
      target: { value: "DeepSeek" },
    });
    fireEvent.change(
      screen.getByPlaceholderText("Matched models, comma separated"),
      {
        target: { value: "deepseek-v4-flash, deepseek-v4-pro" },
      },
    );
    fireEvent.change(
      screen.getByPlaceholderText("Matched prefixes, comma separated"),
      {
        target: { value: "deepseek-" },
      },
    );
    fireEvent.change(screen.getByPlaceholderText("Upstream base URL"), {
      target: { value: "https://api.deepseek.example" },
    });
    fireEvent.change(screen.getByPlaceholderText("Route API key"), {
      target: { value: "sk-route" },
    });
    fireEvent.change(screen.getByPlaceholderText("codex-model=upstream-model"), {
      target: { value: "deepseek-v4-flash=deepseek-chat" },
    });

    await waitFor(() => {
      expect(latestRouting().routes?.[0]).toMatchObject({
        id: "deepseek",
        label: "DeepSeek",
        match: {
          models: ["deepseek-v4-flash", "deepseek-v4-pro"],
          prefixes: ["deepseek-"],
        },
        upstream: {
          baseUrl: "https://api.deepseek.example",
          apiKey: "sk-route",
          modelMap: { "deepseek-v4-flash": "deepseek-chat" },
        },
      });
    });
    expect(latestRouting().routes?.[0]).not.toHaveProperty("rowId");
  });

  it("removes a route from the list and writes the shortened routing config", async () => {
    const { latestRouting, container } = renderRoutingHarness({
      enabled: true,
      defaultRouteId: "",
      routes: [
        {
          id: "deepseek",
          label: "DeepSeek",
          enabled: true,
          match: { models: ["deepseek-v4-flash"], prefixes: [] },
          upstream: {
            baseUrl: "https://api.deepseek.example",
            apiFormat: "openai_chat",
            auth: { source: "provider_config" },
          },
          capabilities: { textOnly: true, inputModalities: ["text"] },
        },
      ],
    });

    const deleteButton = container.querySelector('button[title="Delete"]');
    expect(deleteButton).not.toBeNull();
    fireEvent.click(deleteButton!);

    await waitFor(() => {
      expect(latestRouting().routes).toEqual([]);
    });
  });
});
