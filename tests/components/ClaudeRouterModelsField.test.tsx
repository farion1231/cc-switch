import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  ClaudeRouterModelsField,
  getClaudeRouterValidationError,
  normalizeClaudeRouterConfig,
} from "@/components/providers/forms/ClaudeRouterModelsField";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import type {
  ProviderFormProps,
  ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";
import type { ClaudeRouterConfig } from "@/types";

Element.prototype.scrollIntoView = vi.fn();
const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    info: vi.fn(),
    success: vi.fn(),
  },
}));

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => null,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: null,
      accounts: [],
    }),
    useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
    useCommonConfigSnippet: () => ({
      useCommonConfig: false,
      commonConfigSnippet: "",
      commonConfigError: null,
      isLoading: false,
      isExtracting: false,
      handleCommonConfigToggle: vi.fn(),
      handleCommonConfigSnippetChange: vi.fn(),
      handleExtract: vi.fn(),
    }),
    useCodexCommonConfig: () => ({
      useCommonConfig: false,
      commonConfigSnippet: "",
      commonConfigError: null,
      handleCommonConfigToggle: vi.fn(),
      handleCommonConfigSnippetChange: vi.fn(),
      isExtracting: false,
      handleExtract: vi.fn(),
      clearCommonConfigError: vi.fn(),
    }),
    useGeminiCommonConfig: () => ({
      useCommonConfig: false,
      commonConfigSnippet: "",
      commonConfigError: null,
      handleCommonConfigToggle: vi.fn(),
      handleCommonConfigSnippetChange: vi.fn(),
      isExtracting: false,
      handleExtract: vi.fn(),
      clearCommonConfigError: vi.fn(),
    }),
  };
});

vi.mock("@/lib/query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...actual,
    useSettingsQuery: () => ({
      data: { commonConfigConfirmed: true },
    }),
  };
});

const initialConfig: ClaudeRouterConfig = {
  enabled: false,
  models: [
    {
      alias: "sonnet",
      displayName: "Sonnet",
      upstreamModel: "claude-sonnet-4-5",
    },
  ],
};

function Harness({
  initial = initialConfig,
  fetchedModels = [],
  onFetch = vi.fn(),
}: {
  initial?: ClaudeRouterConfig;
  fetchedModels?: Array<{ id: string; ownedBy: string | null }>;
  onFetch?: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <ClaudeRouterModelsField
      value={value}
      onChange={setValue}
      fetchedModels={fetchedModels}
      isLoading={false}
      onFetch={onFetch}
    />
  );
}

describe("ClaudeRouterModelsField", () => {
  it("keeps router enablement independent and supports add, reorder, and remove", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    const enabled = screen.getByRole("switch", {
      name: "Enabled for Router",
    });
    expect(enabled).not.toBeChecked();
    await user.click(enabled);
    expect(enabled).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Add model" }));
    fireEvent.change(screen.getByLabelText("Alias 2"), {
      target: { value: "opus" },
    });
    fireEvent.change(screen.getByLabelText("Display Name 2"), {
      target: { value: "Opus" },
    });
    fireEvent.change(screen.getByLabelText("Upstream Model 2"), {
      target: { value: "claude-opus-4-1" },
    });

    await user.click(screen.getByRole("button", { name: "Move model 2 up" }));
    expect(screen.getByLabelText("Alias 1")).toHaveValue("opus");
    expect(screen.getByLabelText("Alias 2")).toHaveValue("sonnet");

    await user.click(screen.getByRole("button", { name: "Remove model 2" }));
    expect(screen.getAllByLabelText(/Alias \d/)).toHaveLength(1);
    expect(screen.getByLabelText("Alias 1")).toHaveValue("opus");
  });

  it("reuses fetched models while retaining free-form upstream input", async () => {
    const user = userEvent.setup();
    const onFetch = vi.fn();
    render(
      <Harness
        fetchedModels={[{ id: "fetched-sonnet", ownedBy: "Anthropic" }]}
        onFetch={onFetch}
      />,
    );

    const upstream = screen.getByLabelText("Upstream Model 1");
    fireEvent.change(upstream, { target: { value: "custom-model" } });
    expect(upstream).toHaveValue("custom-model");

    await user.click(screen.getByRole("button", { name: "Select model" }));
    await user.click(screen.getByText("fetched-sonnet"));
    expect(upstream).toHaveValue("fetched-sonnet");
  });

  it("reports hard validation failures and trims persisted metadata", () => {
    expect(getClaudeRouterValidationError({ enabled: true, models: [] })).toBe(
      "modelsRequired",
    );
    expect(
      getClaudeRouterValidationError({
        enabled: true,
        models: [
          { alias: "same", displayName: "One", upstreamModel: "model-a" },
          { alias: "same", displayName: "Two", upstreamModel: "model-b" },
        ],
      }),
    ).toBe("duplicateAlias");
    expect(
      getClaudeRouterValidationError({
        enabled: true,
        models: [{ alias: " ", displayName: "Name", upstreamModel: "model" }],
      }),
    ).toBe("fieldsRequired");

    expect(
      normalizeClaudeRouterConfig({
        enabled: false,
        models: [
          {
            alias: " alias ",
            displayName: " Display ",
            upstreamModel: " upstream ",
          },
        ],
      }),
    ).toEqual({
      enabled: false,
      models: [
        {
          alias: "alias",
          displayName: "Display",
          upstreamModel: "upstream",
        },
      ],
    });
  });
});

function renderClaudeProviderForm(
  initialData?: ProviderFormProps["initialData"],
) {
  const onSubmit = vi.fn();
  const queryClient = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId="claude"
        providerId={initialData ? "provider-a" : undefined}
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={initialData}
      />
    </QueryClientProvider>,
  );
  return { ...view, onSubmit };
}

describe("ProviderForm Claude router metadata", () => {
  it("creates trimmed router metadata for a new provider", async () => {
    const { container, onSubmit } = renderClaudeProviderForm();
    const nameInput =
      container.querySelector<HTMLInputElement>('input[name="name"]');
    const apiKeyInput = document.getElementById("apiKey");
    const baseUrlInput = document.getElementById("baseUrl");
    expect(nameInput).not.toBeNull();
    expect(apiKeyInput).not.toBeNull();
    expect(baseUrlInput).not.toBeNull();

    fireEvent.change(nameInput!, { target: { value: "Provider A" } });
    fireEvent.change(apiKeyInput!, { target: { value: "sk-test" } });
    fireEvent.change(baseUrlInput!, {
      target: { value: "https://api.example.com" },
    });
    fireEvent.click(screen.getByRole("switch", { name: "Enabled for Router" }));
    fireEvent.click(screen.getByRole("button", { name: "Add model" }));
    fireEvent.change(screen.getByLabelText("Alias 1"), {
      target: { value: " sonnet " },
    });
    fireEvent.change(screen.getByLabelText("Display Name 1"), {
      target: { value: " Sonnet 4.5 " },
    });
    fireEvent.change(screen.getByLabelText("Upstream Model 1"), {
      target: { value: " claude-sonnet-4-5 " },
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted.meta?.claudeRouter).toEqual({
      enabled: true,
      models: [
        {
          alias: "sonnet",
          displayName: "Sonnet 4.5",
          upstreamModel: "claude-sonnet-4-5",
        },
      ],
    });
  });

  it("round-trips disabled model rows when editing", async () => {
    const { onSubmit } = renderClaudeProviderForm({
      name: "Provider A",
      category: "third_party",
      settingsConfig: {
        env: {
          ANTHROPIC_AUTH_TOKEN: "sk-test",
          ANTHROPIC_BASE_URL: "https://api.example.com",
        },
      },
      meta: {
        customUserAgent: "cc-switch-test",
        claudeRouter: {
          enabled: false,
          models: [
            {
              alias: " sonnet ",
              displayName: " Sonnet ",
              upstreamModel: " upstream-sonnet ",
            },
          ],
        },
      },
    });

    expect(
      screen.getByRole("switch", { name: "Enabled for Router" }),
    ).not.toBeChecked();
    expect(screen.getByLabelText("Alias 1")).toHaveValue(" sonnet ");
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted.meta).toEqual(
      expect.objectContaining({
        customUserAgent: "cc-switch-test",
        claudeRouter: {
          enabled: false,
          models: [
            {
              alias: "sonnet",
              displayName: "Sonnet",
              upstreamModel: "upstream-sonnet",
            },
          ],
        },
      }),
    );
  });

  it("hard-blocks duplicate aliases before submitting", async () => {
    toastMocks.error.mockReset();
    const { onSubmit } = renderClaudeProviderForm({
      name: "Provider A",
      category: "third_party",
      settingsConfig: {
        env: {
          ANTHROPIC_AUTH_TOKEN: "sk-test",
          ANTHROPIC_BASE_URL: "https://api.example.com",
        },
      },
      meta: {
        claudeRouter: {
          enabled: true,
          models: [
            { alias: "same", displayName: "One", upstreamModel: "model-a" },
            { alias: "same", displayName: "Two", upstreamModel: "model-b" },
          ],
        },
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "Router model aliases must be unique.",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
