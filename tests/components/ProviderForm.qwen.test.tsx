import type { ComponentProps } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, type Mock } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => <div data-testid="provider-advanced-config" />,
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div data-testid="codex-config-editor" />,
}));

vi.mock("@/components/providers/forms/GeminiConfigEditor", () => ({
  default: () => <div data-testid="gemini-config-editor" />,
}));

vi.mock("@/components/providers/forms/CommonConfigEditor", () => ({
  CommonConfigEditor: () => <div data-testid="common-config-editor" />,
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  useOpenClawLiveProviderIds: () => ({
    data: [],
    isLoading: false,
  }),
}));

vi.mock("@/components/providers/forms/hooks/useCopilotAuth", () => ({
  useCopilotAuth: () => ({
    isAuthenticated: false,
  }),
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    configApi: {
      getCommonConfigSnippet: vi.fn().mockResolvedValue(""),
      setCommonConfigSnippet: vi.fn().mockResolvedValue(true),
    },
  };
});

function renderProviderForm(
  props?: Omit<Partial<ComponentProps<typeof ProviderForm>>, "onSubmit">,
) {
  const client = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  const onSubmit: Mock = vi.fn().mockResolvedValue(undefined);

  const view = render(
    <QueryClientProvider client={client}>
      <ProviderForm
        appId="qwen"
        submitLabel="Save"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        {...props}
      />
    </QueryClientProvider>,
  );

  return {
    ...view,
    onSubmit,
  };
}

describe("ProviderForm Qwen wiring", () => {
  it("routes Qwen presets through the main provider form flow", async () => {
    const { onSubmit } = renderProviderForm();

    fireEvent.click(
      screen.getByRole("button", {
        name: /Alibaba Cloud \(DashScope\)/,
      }),
    );

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "dashscope-key" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));

    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.name).toBe("Alibaba Cloud (DashScope)");
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      env: {
        OPENAI_API_KEY: "dashscope-key",
        OPENAI_BASE_URL:
          "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        OPENAI_MODEL: "qwen3-coder-plus",
      },
    });
  });

  it("backfills and submits edited Qwen env fields", async () => {
    const { onSubmit } = renderProviderForm({
      providerId: "existing-qwen",
      initialData: {
        name: "Existing Qwen",
        websiteUrl: "https://qwen.example.com",
        settingsConfig: {
          env: {
            OPENAI_API_KEY: "old-key",
            OPENAI_BASE_URL: "https://old.example.com/v1",
            OPENAI_MODEL: "qwen-old",
          },
        },
        category: "custom",
      },
    });

    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "new-key" },
    });
    fireEvent.change(screen.getByLabelText("API 端点"), {
      target: { value: "https://new.example.com/v1" },
    });
    fireEvent.change(screen.getByLabelText("模型"), {
      target: { value: "qwen3-coder-plus" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));

    const submitted = onSubmit.mock.calls[0][0];
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      env: {
        OPENAI_API_KEY: "new-key",
        OPENAI_BASE_URL: "https://new.example.com/v1",
        OPENAI_MODEL: "qwen3-coder-plus",
      },
    });
  });
});
