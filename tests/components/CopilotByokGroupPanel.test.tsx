import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotByokGroupPanel } from "@/components/settings/CopilotByokGroupPanel";

const mocks = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  fetchModelsDevPricing: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: mocks.fetchModelsForConfig,
  showFetchModelsError: mocks.showFetchModelsError,
}));

vi.mock("@/lib/modelsDevPricing", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/lib/modelsDevPricing")>();
  return {
    ...actual,
    fetchModelsDevPricing: mocks.fetchModelsDevPricing,
  };
});

vi.mock("@/components/providers/forms/shared", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/providers/forms/shared")
    >();
  return {
    ...actual,
    ModelDropdown: ({
      models,
      onSelect,
    }: {
      models: Array<{ id: string }>;
      onSelect: (id: string) => void;
    }) => (
      <button
        type="button"
        aria-label="select fetched model"
        onClick={() => onSelect(models[0].id)}
      />
    ),
  };
});

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { resolvedLanguage: "zh" },
  }),
}));

describe("CopilotByokGroupPanel", () => {
  beforeEach(() => {
    mocks.fetchModelsForConfig.mockReset();
    mocks.fetchModelsDevPricing.mockReset();
    mocks.showFetchModelsError.mockReset();
    mocks.fetchModelsForConfig.mockResolvedValue([
      { id: "kimi-k2", name: "Kimi K2", ownedBy: "Moonshot" },
    ]);
    mocks.fetchModelsDevPricing.mockResolvedValue({});
  });

  it("saves multiple models under one shared provider connection", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <CopilotByokGroupPanel
        open
        group={null}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );

    expect(screen.getByText("providerPreset.label")).toBeInTheDocument();
    expect(screen.getByText("providerPreset.custom")).toBeInTheDocument();
    expect(
      screen.getByText("providerForm.customApiKeyHint"),
    ).toBeInTheDocument();
    expect(screen.getByText("copilotByok.securityTitle")).toBeInTheDocument();
    expect(screen.getByText("copilotByok.security")).toBeInTheDocument();
    expect(
      screen.queryByText("providerPreset.noSearchResults"),
    ).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Moonshot" },
    });
    fireEvent.change(screen.getByLabelText("provider.notes"), {
      target: { value: "Coding plan" },
    });
    fireEvent.change(screen.getByLabelText("provider.websiteUrl"), {
      target: { value: "https://platform.moonshot.cn" },
    });
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://api.example.com/v1/chat/completions" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() =>
      expect(mocks.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.example.com/v1/chat/completions",
        "",
        false,
        undefined,
        undefined,
        {
          apiFormat: "chat-completions",
          requestHeaders: {},
        },
      ),
    );

    await user.click(
      await screen.findByRole("button", { name: "select fetched model" }),
    );

    const firstModelId = screen.getByPlaceholderText(
      "copilotByok.form.modelIdPlaceholder",
    );
    const firstModelName = screen.getByPlaceholderText(
      "copilotByok.form.modelNamePlaceholder",
    );
    expect(firstModelId).toHaveValue("kimi-k2");
    expect(firstModelName).toHaveValue("Kimi K2");
    expect(screen.queryByText("opencode.modelLimits")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "opencode.toggleModelDetails" }),
    );
    expect(screen.getByText("opencode.modelLimits")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "opencode.addHeader" }));
    fireEvent.change(
      screen.getByPlaceholderText("opencode.headerNamePlaceholder"),
      {
        target: { value: "X-Title" },
      },
    );
    fireEvent.blur(
      screen.getByPlaceholderText("opencode.headerNamePlaceholder"),
    );
    fireEvent.change(
      screen.getByPlaceholderText("opencode.headerValuePlaceholder"),
      {
        target: { value: "CC Switch BYOK" },
      },
    );

    fireEvent.click(screen.getByRole("button", { name: "opencode.addModel" }));

    const modelIds = screen.getAllByPlaceholderText(
      "copilotByok.form.modelIdPlaceholder",
    );
    const modelNames = screen.getAllByPlaceholderText(
      "copilotByok.form.modelNamePlaceholder",
    );
    fireEvent.change(modelIds[1], { target: { value: "kimi-k3" } });
    fireEvent.change(modelNames[1], { target: { value: "Kimi K3" } });

    fireEvent.click(
      screen.getByRole("button", { name: "provider.addToConfig" }),
    );

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Moonshot",
        url: "https://api.example.com/v1/chat/completions",
        apiKey: "",
        apiType: "chat-completions",
        notes: "Coding plan",
        websiteUrl: "https://platform.moonshot.cn",
        requestHeaders: { "X-Title": "CC Switch BYOK" },
        models: [
          expect.objectContaining({
            modelId: "kimi-k2",
            name: "Kimi K2",
            contextWindow: null,
            maxOutputTokens: null,
            toolCalling: true,
            streaming: null,
          }),
          expect.objectContaining({
            modelId: "kimi-k3",
            name: "Kimi K3",
            toolCalling: true,
            vision: true,
            thinking: true,
            streaming: true,
            contextWindow: 1_000_000,
            maxInputTokens: null,
            maxOutputTokens: null,
            supportsReasoningEffort: ["low", "high", "max"],
            reasoningEffortFormat: "chat-completions",
            modelOptions: { temperature: 1, top_p: 0.95 },
          }),
        ],
      }),
    );
    expect(screen.getAllByLabelText("opencode.baseUrl")).toHaveLength(1);
    expect(screen.getAllByLabelText("copilotByok.form.apiKey")).toHaveLength(1);
  });

  it("hydrates a fetched Kimi coding alias from models.dev capabilities", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn().mockResolvedValue(undefined);
    mocks.fetchModelsForConfig.mockResolvedValue([
      {
        id: "kimi-for-coding",
        name: "K2.7 Coding",
        ownedBy: "Moonshot",
      },
    ]);
    mocks.fetchModelsDevPricing.mockResolvedValue({
      moonshotai: {
        name: "Moonshot AI",
        models: {
          "kimi-k2.7-code": {
            name: "Kimi K2.7 Code",
            tool_call: true,
            reasoning: true,
            modalities: {
              input: ["text", "image", "video"],
              output: ["text"],
            },
            limit: { context: 262_144, output: 262_144 },
          },
        },
      },
    });

    render(
      <CopilotByokGroupPanel
        open
        group={null}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );
    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "Kimi" },
    });
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://api.kimi.com/coding/v1" },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(
      await screen.findByRole("button", { name: "select fetched model" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "provider.addToConfig" }),
    );

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        models: [
          expect.objectContaining({
            modelId: "kimi-for-coding",
            name: "K2.7 Coding",
            toolCalling: true,
            vision: true,
            thinking: true,
            streaming: true,
            contextWindow: 262_144,
            maxOutputTokens: 262_144,
            supportsReasoningEffort: [],
            modelOptions: { temperature: 1, top_p: 0.95 },
          }),
        ],
      }),
    );
  });

  it("shows Copilot CLI provider controls and CLI-specific credential guidance", () => {
    render(
      <CopilotByokGroupPanel
        catalogApp="copilot-cli"
        open
        group={null}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(
      screen.getByText("copilotByok.cli.form.providerType"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("copilotByok.cli.form.bearerToken"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.transport"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("copilotByok.cli.form.azureApiVersion"),
    ).toBeDisabled();
    expect(
      screen.getByText("copilotByok.cli.form.securityTitle"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.security"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("copilotByok.securityTitle"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.defaultModel"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.defaultModelHint"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "opencode.addModel" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "opencode.toggleModelDetails" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.advanced"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("copilotByok.cli.form.modelRouting"),
    ).toBeInTheDocument();
  });

  it("saves exactly one Copilot CLI default model", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <CopilotByokGroupPanel
        catalogApp="copilot-cli"
        open
        group={null}
        saving={false}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );

    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: { value: "MiniMax" },
    });
    fireEvent.change(screen.getByLabelText("opencode.baseUrl"), {
      target: { value: "https://api.minimax.io/v1" },
    });
    fireEvent.change(
      screen.getByLabelText("copilotByok.cli.form.defaultModel"),
      { target: { value: "MiniMax-M3" } },
    );
    fireEvent.click(
      screen.getByRole("button", { name: "provider.addToConfig" }),
    );

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "MiniMax",
        enabled: true,
        models: [
          expect.objectContaining({
            modelId: "MiniMax-M3",
            name: "MiniMax-M3",
            enabled: true,
          }),
        ],
      }),
    );
  });
});
