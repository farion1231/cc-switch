import type { ReactElement } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

const { fetchModelsForConfigMock } = vi.hoisted(() => ({
  fetchModelsForConfigMock: vi.fn<
    (baseUrl: string, apiKey: string, isFullUrl?: boolean) => Promise<[]>
  >(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: fetchModelsForConfigMock,
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="settings config"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => null,
}));

vi.mock("@/components/providers/forms/GeminiConfigEditor", () => ({
  default: () => null,
}));

vi.mock("@/components/providers/forms/CommonConfigEditor", () => ({
  CommonConfigEditor: () => null,
}));

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({
    isOpen,
    confirmText,
    onConfirm,
  }: {
    isOpen?: boolean;
    confirmText?: string;
    onConfirm?: () => void | Promise<void>;
  }) =>
    isOpen ? (
      <button type="button" onClick={() => void onConfirm?.()}>
        {confirmText ?? "确认"}
      </button>
    ) : null,
}));

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function renderProviderForm(
  appId: "opencode" | "openclaw",
  initialData: {
    name: string;
    category: any;
    settingsConfig: Record<string, unknown>;
    meta?: Record<string, unknown>;
  },
) {
  return renderWithQueryClient(
    <ProviderForm
      appId={appId}
      submitLabel="保存"
      onSubmit={vi.fn()}
      onCancel={vi.fn()}
      initialData={initialData}
    />,
  );
}

describe("Provider endpoint full URL wiring", () => {
  it("OpenCode submit does not persist full URL after JSON editor changes npm to unsupported value", async () => {
    const handleSubmit = vi.fn();

    renderWithQueryClient(
      <ProviderForm
        appId="opencode"
        submitLabel="保存"
        onSubmit={handleSubmit}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.change(screen.getByLabelText("provider.name"), {
      target: {
        value: "OpenCode Submit",
      },
    });
    fireEvent.change(
      screen.getByPlaceholderText("opencode.providerKeyPlaceholder"),
      {
        target: {
          value: "opencode-test-key",
        },
      },
    );
    fireEvent.click(screen.getByLabelText("完整 URL"));

    fireEvent.change(screen.getByLabelText("settings config"), {
      target: {
        value: JSON.stringify(
          {
            npm: "@ai-sdk/google",
            options: {
              baseURL: "https://example.com/v1beta",
              apiKey: "sk-test",
            },
            models: {
              "gemini-2.5-pro": {},
            },
          },
          null,
          2,
        ),
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    const saveAnywayButton = await screen
      .findByRole("button", {
        name: /providerForm\.softValidation\.saveAnyway|仍要保存/,
      })
      .catch(() => null);
    if (saveAnywayButton) {
      fireEvent.click(saveAnywayButton);
    }

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));

    expect(handleSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        meta: expect.not.objectContaining({
          isFullUrl: true,
        }),
      }),
    );
  });

  it("OpenCode shows full URL toggle and forwards isFullUrl=true for supported npm packages", async () => {
    fetchModelsForConfigMock.mockResolvedValueOnce([]);

    renderProviderForm("opencode", {
      name: "OpenCode Supported",
      category: "custom",
      settingsConfig: {
        npm: "@ai-sdk/openai-compatible",
        options: {
          baseURL: "https://example.com/v1",
          apiKey: "sk-test",
        },
        models: {},
      },
      meta: {
        isFullUrl: true,
      },
    });

    expect(screen.getByLabelText("完整 URL")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() =>
      expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
        "https://example.com/v1",
        "sk-test",
        true,
      ),
    );
  });

  it("OpenCode hides the full URL toggle for unsupported npm packages and fetches with full URL disabled", async () => {
    fetchModelsForConfigMock.mockResolvedValueOnce([]);

    renderProviderForm("opencode", {
      name: "OpenCode Unsupported",
      category: "custom",
      settingsConfig: {
        npm: "@ai-sdk/google",
        options: {
          baseURL: "https://example.com/v1beta",
          apiKey: "sk-test",
        },
        models: {},
      },
      meta: {
        isFullUrl: true,
      },
    });

    expect(screen.queryByLabelText("完整 URL")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() =>
      expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
        "https://example.com/v1beta",
        "sk-test",
        false,
      ),
    );
  });

  it("OpenClaw shows full URL toggle and forwards isFullUrl=true for supported protocols", async () => {
    fetchModelsForConfigMock.mockResolvedValueOnce([]);

    renderProviderForm("openclaw", {
      name: "OpenClaw Supported",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://example.com/anthropic",
        apiKey: "sk-openclaw",
        api: "anthropic-messages",
        models: [],
      },
      meta: {
        isFullUrl: true,
      },
    });

    expect(screen.getByLabelText("完整 URL")).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() =>
      expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
        "https://example.com/anthropic",
        "sk-openclaw",
        true,
      ),
    );
  });

  it("OpenClaw hides the full URL toggle for unsupported protocols and fetches with full URL disabled", async () => {
    fetchModelsForConfigMock.mockResolvedValueOnce([]);

    renderProviderForm("openclaw", {
      name: "OpenClaw Unsupported",
      category: "custom",
      settingsConfig: {
        baseUrl: "https://example.com/google",
        apiKey: "sk-openclaw",
        api: "google-generative-ai",
        models: [],
      },
      meta: {
        isFullUrl: true,
      },
    });

    expect(screen.queryByLabelText("完整 URL")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", {
        name: "providerForm.fetchModels",
      }),
    );

    await waitFor(() =>
      expect(fetchModelsForConfigMock).toHaveBeenCalledWith(
        "https://example.com/google",
        "sk-openclaw",
        false,
      ),
    );
  });
});
