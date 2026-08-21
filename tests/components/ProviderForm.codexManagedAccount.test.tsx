import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

const authState = vi.hoisted(() => ({
  codexReauthRequired: false,
}));
const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
  },
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: ({
    onAccountSelect,
    onSelectionConfirmed,
    onSelectionInvalidated,
    allowUnboundSelection = true,
    allowUnboundSelectionWithoutStatus = false,
  }: {
    onAccountSelect?: (accountId: string | null) => void;
    onSelectionConfirmed?: () => void;
    onSelectionInvalidated?: () => void;
    allowUnboundSelection?: boolean;
    allowUnboundSelectionWithoutStatus?: boolean;
  }) => (
    <div>
      <output data-testid="allow-unbound-selection">
        {allowUnboundSelection ? "true" : "false"}
      </output>
      <output data-testid="allow-unbound-without-status">
        {allowUnboundSelectionWithoutStatus ? "true" : "false"}
      </output>
      <button
        type="button"
        onClick={() => {
          onSelectionConfirmed?.();
          onAccountSelect?.("acct-managed");
        }}
      >
        select-managed-account
      </button>
      {allowUnboundSelection && (
        <button
          type="button"
          onClick={() => {
            onSelectionConfirmed?.();
            onAccountSelect?.(null);
          }}
        >
          select-native-login
        </button>
      )}
      <button
        type="button"
        onClick={() => {
          onSelectionInvalidated?.();
          onAccountSelect?.(null);
        }}
      >
        invalidate-selected-account
      </button>
    </div>
  ),
}));

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div data-testid="codex-config-editor" />,
}));

vi.mock("@/components/providers/forms/ProviderAdvancedConfig", () => ({
  ProviderAdvancedConfig: () => <div data-testid="advanced-config" />,
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
      isAuthenticated: true,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: "acct-managed",
      accounts: [
        {
          id: "acct-managed",
          login: "user@example.com",
          is_default: true,
          reauth_required: authState.codexReauthRequired,
          requires_reauth: false,
        },
      ],
    }),
    useXaiOauth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
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

function renderCodexForm(onSubmit: (values: ProviderFormValues) => void) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId="codex"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

function renderClaudeCodexForm(onSubmit: (values: ProviderFormValues) => void) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId="claude"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Claude via Codex OAuth",
          category: "third_party",
          settingsConfig: { env: {} },
          meta: { providerType: "codex_oauth" },
        }}
      />
    </QueryClientProvider>,
  );
}

describe("ProviderForm Codex Official managed account", () => {
  beforeEach(() => {
    authState.codexReauthRequired = false;
    toastMocks.error.mockReset();
  });

  it("persists the selected managed account while stripping OAuth secrets", async () => {
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "select-managed-account" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted).toEqual(
      expect.objectContaining({
        name: "OpenAI Official (user@example.com)",
        presetId: "codex-0",
        presetCategory: "official",
        meta: expect.objectContaining({
          providerType: "codex_oauth",
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acct-managed",
          },
        }),
      }),
    );
    expect(JSON.parse(submitted.settingsConfig)).toEqual({
      auth: {},
      config: "",
    });
  });

  it("defaults every new Official card to the current Codex login", async () => {
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    expect(
      await screen.findByRole("button", { name: "select-native-login" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("allow-unbound-selection")).toHaveTextContent(
      "true",
    );
    expect(
      screen.getByTestId("allow-unbound-without-status"),
    ).toHaveTextContent("true");
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted.presetCategory).toBe("official");
    expect(submitted.meta?.providerType).toBeUndefined();
    expect(submitted.meta?.authBinding).toBeUndefined();
    expect(submitted).not.toHaveProperty("codexNativeLoginSelected");
  });

  it("requires confirmation before falling back when a selected account disappears", async () => {
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "select-managed-account" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "invalidate-selected-account" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith("请先选择登录方式"),
    );
    expect(onSubmit).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "select-native-login" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta?.providerType).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].meta?.authBinding).toBeUndefined();
  });

  it("allows the fixed Official card to switch to a managed account", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="codex-official"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "OpenAI Official",
            settingsConfig: { auth: {}, config: "" },
          }}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("button", { name: "select-managed-account" }),
    ).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "select-native-login" }),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "select-managed-account" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta?.authBinding).toEqual({
      source: "managed_account",
      authProvider: "codex_oauth",
      accountId: "acct-managed",
    });
    expect(onSubmit.mock.calls[0][0].meta?.providerType).toBe("codex_oauth");
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty(
      "codexNativeLoginSelected",
    );
  });

  it("does not silently strip a legacy binding from the fixed card", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="codex-official"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "OpenAI Official",
            settingsConfig: { auth: {}, config: "" },
            meta: {
              providerType: "codex_oauth",
              authBinding: {
                source: "managed_account",
                authProvider: "codex_oauth",
                accountId: "acct-managed",
              },
            },
          }}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta?.authBinding).toEqual({
      source: "managed_account",
      authProvider: "codex_oauth",
      accountId: "acct-managed",
    });
  });

  it("keeps a category-less managed card Official when it is unbound", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="managed-official"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "OpenAI Official (user@example.com)",
            settingsConfig: { auth: {}, config: "" },
            meta: {
              providerType: "codex_oauth",
              authBinding: {
                source: "managed_account",
                authProvider: "codex_oauth",
                accountId: "acct-managed",
              },
            },
          }}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(
      await screen.findByRole("button", { name: "select-native-login" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta).not.toEqual(
      expect.objectContaining({ authBinding: expect.anything() }),
    );
    expect(onSubmit.mock.calls[0][0].meta?.providerType).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].presetCategory).toBe("official");
    expect(onSubmit.mock.calls[0][0]).not.toHaveProperty(
      "codexNativeLoginSelected",
    );
  });

  it("keeps an unmarked legacy Official row on follow-login", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="legacy-unbound-official"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Legacy OpenAI Official",
            category: "official",
            settingsConfig: {
              auth: {
                auth_mode: "chatgpt",
                tokens: { refresh_token: "legacy-refresh-token" },
              },
              config: "",
            },
          }}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByRole("button", { name: "select-native-login" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit.mock.calls[0][0].meta?.providerType).toBeUndefined();
    expect(onSubmit.mock.calls[0][0].meta?.authBinding).toBeUndefined();
    expect(JSON.parse(onSubmit.mock.calls[0][0].settingsConfig).auth).toEqual({
      auth_mode: "chatgpt",
      tokens: { refresh_token: "legacy-refresh-token" },
    });
  });

  it("does not add OAuth binding behavior to a third-party route with a stale Official category", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="legacy-unbound-official"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Legacy OpenAI Official",
            category: "official",
            settingsConfig: {
              auth: { tokens: { refresh_token: "stale-secret" } },
              config:
                'model_provider = "custom"\nbase_url = "https://example.com/v1"\nexperimental_bearer_token = "stale-key"\n[model_providers.custom]\nrequires_openai_auth = true',
            },
          }}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.queryByRole("button", { name: "select-native-login" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted.meta?.providerType).toBeUndefined();
    const submittedSettings = JSON.parse(submitted.settingsConfig);
    expect(submittedSettings.auth).toEqual({
      tokens: { refresh_token: "stale-secret" },
    });
    expect(submittedSettings.config).toContain('model_provider = "custom"');
    expect(submittedSettings.config).toContain(
      'base_url = "https://example.com/v1"',
    );
  });

  it("blocks saving a managed account that requires reauthentication", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    fireEvent.click(
      await screen.findByRole("button", { name: "select-managed-account" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("blocks the reauth-required default account when no account is selected", async () => {
    authState.codexReauthRequired = true;
    const onSubmit = vi.fn();
    renderClaudeCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        "已绑定账号不存在或需要重新登录",
      ),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });
});
