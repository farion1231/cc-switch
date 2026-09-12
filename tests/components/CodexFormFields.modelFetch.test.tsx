import type { ComponentProps } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { FormProvider, useForm } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import {
  copilotGetModels,
  copilotGetModelsForAccount,
  type CopilotModel,
} from "@/lib/api/copilot";
import {
  fetchModelsForConfig,
  fetchXaiOauthModels,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";

vi.mock("@/lib/api/copilot", () => ({
  copilotGetModels: vi.fn(),
  copilotGetModelsForAccount: vi.fn(),
}));
vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: vi.fn(),
  fetchXaiOauthModels: vi.fn(),
  showFetchModelsError: vi.fn(),
}));
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => null,
}));
vi.mock("@/components/providers/forms/shared", async (importOriginal) => {
  const actual =
    await importOriginal<
      typeof import("@/components/providers/forms/shared")
    >();
  return {
    ...actual,
    ModelDropdown: ({ models }: { models: FetchedModel[] }) => (
      <output data-testid="model-options">
        {models.map((model) => model.id).join(",")}
      </output>
    ),
  };
});

type Props = ComponentProps<typeof CodexFormFields>;
type ProviderKind = "copilot" | "xai" | "config";

function pendingModels() {
  let resolve!: (models: Array<CopilotModel & FetchedModel>) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<Array<CopilotModel & FetchedModel>>(
    (res, rej) => {
      resolve = res;
      reject = rej;
    },
  );
  return { promise, resolve, reject };
}

function advertisedModel(id = "model-1"): CopilotModel & FetchedModel {
  return {
    id,
    name: "Model One",
    vendor: "vendor",
    ownedBy: "vendor",
    model_picker_enabled: true,
    supported_endpoints: ["/responses"],
    context_window: 400_000,
  };
}

function makeProps(kind: ProviderKind): Props {
  return {
    isCopilotPreset: kind === "copilot",
    isCopilotAuthenticated: true,
    selectedGitHubAccountId: "github-account",
    isXaiOauthPreset: kind === "xai",
    isXaiOauthAuthenticated: true,
    selectedXaiAccountId: "xai-account",
    codexApiKey: "test-key",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://example.com/v1",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "model-1",
    onModelChange: vi.fn(),
    apiFormat: "openai_chat",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
  };
}

function Harness(props: Props) {
  const form = useForm();
  return (
    <FormProvider {...form}>
      <CodexFormFields {...props} />
    </FormProvider>
  );
}

const fetchMockFor = (kind: ProviderKind) =>
  kind === "copilot"
    ? vi.mocked(copilotGetModelsForAccount)
    : kind === "xai"
      ? vi.mocked(fetchXaiOauthModels)
      : vi.mocked(fetchModelsForConfig);

const fetchButton = () => screen.getByTitle("providerForm.fetchModels");
const providerKinds: ProviderKind[] = ["copilot", "xai", "config"];

describe("Codex model-fetch lifecycle", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("fetches only models supporting the selected Copilot protocol", async () => {
    const responsesModel = advertisedModel("responses-model");
    const chatModel = {
      ...advertisedModel("chat-model"),
      supported_endpoints: ["/chat/completions"],
    };
    vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
      responsesModel,
      chatModel,
    ]);
    const props = {
      ...makeProps("copilot"),
      copilotApiFormat: "openai_chat" as const,
    };
    render(<Harness {...props} />);
    fireEvent.click(fetchButton());
    await waitFor(() => expect(fetchButton()).toBeEnabled());
    expect(screen.getByTestId("model-options")).toHaveTextContent("chat-model");
    expect(screen.getByTestId("model-options")).not.toHaveTextContent(
      "responses-model",
    );
    expect(props.onModelChange).toHaveBeenCalledWith("chat-model");
    expect(props.onCatalogModelsChange).toHaveBeenCalledWith([
      expect.objectContaining({ model: "chat-model" }),
    ]);
  });

  it("discards a pending model list after the Copilot protocol changes", async () => {
    const pending = pendingModels();
    vi.mocked(copilotGetModelsForAccount).mockReturnValue(pending.promise);
    const props = makeProps("copilot");
    const { rerender } = render(<Harness {...props} />);
    fireEvent.click(fetchButton());
    rerender(<Harness {...props} copilotApiFormat="openai_chat" />);
    await act(async () => pending.resolve([advertisedModel()]));
    expect(props.onCatalogModelsChange).not.toHaveBeenCalled();
    expect(props.onModelChange).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it.each(["gpt-6-astra", "claude-model"])(
    "prefers an available GPT model without replacing a valid selection (%s)",
    async (currentModel) => {
      vi.mocked(copilotGetModelsForAccount).mockResolvedValue([
        advertisedModel("claude-model"),
        advertisedModel("gpt-available"),
      ]);
      const props = { ...makeProps("copilot"), codexModel: currentModel };
      render(<Harness {...props} />);
      fireEvent.click(fetchButton());
      await waitFor(() => expect(fetchButton()).toBeEnabled());
      if (currentModel === "claude-model") {
        expect(props.onModelChange).not.toHaveBeenCalled();
      } else {
        expect(props.onModelChange).toHaveBeenCalledWith("gpt-available");
      }
    },
  );

  it.each(providerKinds)(
    "loads %s models through the existing provider route",
    async (kind) => {
      const pending = pendingModels();
      fetchMockFor(kind).mockReturnValue(pending.promise);
      const props = makeProps(kind);
      render(<Harness {...props} />);

      fireEvent.click(fetchButton());
      expect(fetchButton()).toBeDisabled();
      expect(fetchMockFor(kind)).toHaveBeenCalledTimes(1);
      if (kind === "copilot") {
        expect(copilotGetModelsForAccount).toHaveBeenCalledWith(
          "github-account",
        );
      } else if (kind === "xai") {
        expect(fetchXaiOauthModels).toHaveBeenCalledWith("xai-account");
      } else {
        expect(fetchModelsForConfig).toHaveBeenCalledWith(
          props.codexBaseUrl,
          "test-key",
          false,
          undefined,
          "",
        );
      }

      await act(async () => pending.resolve([advertisedModel()]));
      await waitFor(() => expect(fetchButton()).toBeEnabled());
      expect(screen.getByTestId("model-options")).toHaveTextContent("model-1");
      expect(toast.success).toHaveBeenCalledTimes(1);
      expect(showFetchModelsError).not.toHaveBeenCalled();
      if (kind === "copilot") {
        expect(props.onCatalogModelsChange).toHaveBeenCalledWith([
          {
            model: "model-1",
            displayName: "Model One",
            contextWindow: 400_000,
            supportsParallelToolCalls: false,
            inputModalities: ["text"],
          },
        ]);
      } else {
        expect(props.onCatalogModelsChange).not.toHaveBeenCalled();
      }
    },
  );

  it.each(providerKinds)(
    "keeps the empty-list notification for %s",
    async (kind) => {
      fetchMockFor(kind).mockResolvedValue([]);
      render(<Harness {...makeProps(kind)} />);
      fireEvent.click(fetchButton());

      await waitFor(() =>
        expect(toast.info).toHaveBeenCalledWith(
          "providerForm.fetchModelsEmpty",
        ),
      );
      expect(fetchButton()).toBeEnabled();
      expect(toast.success).not.toHaveBeenCalled();
    },
  );

  it.each(providerKinds)(
    "reports %s failures and releases loading state",
    async (kind) => {
      const error = new Error("model-list unavailable");
      fetchMockFor(kind).mockRejectedValue(error);
      render(<Harness {...makeProps(kind)} />);
      fireEvent.click(fetchButton());

      await waitFor(() =>
        expect(showFetchModelsError).toHaveBeenCalledWith(
          error,
          expect.any(Function),
        ),
      );
      expect(fetchButton()).toBeEnabled();
      expect(toast.success).not.toHaveBeenCalled();
      expect(console.warn).toHaveBeenCalledWith(
        kind === "copilot"
          ? "[Copilot] Failed to fetch models:"
          : kind === "xai"
            ? "[XaiOAuth] Failed to fetch models:"
            : "[ModelFetch] Failed:",
        error,
      );
    },
  );

  it.each(providerKinds)(
    "ignores stale %s results after the request identity changes",
    async (kind) => {
      const pending = pendingModels();
      fetchMockFor(kind).mockReturnValue(pending.promise);
      const props = makeProps(kind);
      const view = render(<Harness {...props} />);
      fireEvent.click(fetchButton());
      view.rerender(
        <Harness
          {...props}
          selectedGitHubAccountId="another-github-account"
          selectedXaiAccountId="another-xai-account"
          codexBaseUrl="https://other.example/v1"
        />,
      );

      await act(async () => pending.resolve([advertisedModel()]));
      await waitFor(() => expect(fetchButton()).toBeEnabled());
      expect(screen.queryByTestId("model-options")).not.toBeInTheDocument();
      expect(props.onCatalogModelsChange).not.toHaveBeenCalled();
      expect(toast.success).not.toHaveBeenCalled();
    },
  );

  it("keeps Copilot filtering, explicit context values and default-model selection", async () => {
    vi.mocked(copilotGetModels).mockResolvedValue([
      advertisedModel(),
      {
        ...advertisedModel("messages-only"),
        supported_endpoints: ["/v1/messages"],
      },
    ]);
    const props = {
      ...makeProps("copilot"),
      selectedGitHubAccountId: null,
      codexModel: "unavailable",
      catalogModels: [{ model: "model-1", contextWindow: 200_000 }],
    };
    render(<Harness {...props} />);
    fireEvent.click(fetchButton());

    await waitFor(() =>
      expect(props.onModelChange).toHaveBeenCalledWith("model-1"),
    );
    expect(copilotGetModels).toHaveBeenCalledTimes(1);
    expect(copilotGetModelsForAccount).not.toHaveBeenCalled();
    expect(props.onCatalogModelsChange).toHaveBeenCalledWith([
      expect.objectContaining({ model: "model-1", contextWindow: 200_000 }),
    ]);
    expect(screen.getAllByTestId("model-options")[0]).not.toHaveTextContent(
      "messages-only",
    );
  });

  it.each(providerKinds)("preserves %s preflight validation", (kind) => {
    render(
      <Harness
        {...makeProps(kind)}
        isCopilotAuthenticated={false}
        isXaiOauthAuthenticated={false}
        codexApiKey=""
        codexBaseUrl=""
      />,
    );
    fireEvent.click(fetchButton());

    expect(fetchMockFor(kind)).not.toHaveBeenCalled();
    expect(fetchButton()).toBeEnabled();
    if (kind === "config") {
      expect(showFetchModelsError).toHaveBeenCalledWith(
        null,
        expect.any(Function),
        { hasApiKey: false, hasBaseUrl: false },
      );
    } else {
      expect(toast.error).toHaveBeenCalledTimes(1);
    }
  });
});
