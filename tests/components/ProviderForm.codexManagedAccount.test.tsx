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
const providerApiMocks = vi.hoisted(() => ({
  getAll: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
  },
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      getAll: (...args: unknown[]) => providerApiMocks.getAll(...args),
    },
  };
});

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: ({
    onAccountSelect,
    allowUnboundSelection = true,
    nativeLoginOnly = false,
  }: {
    onAccountSelect?: (accountId: string | null) => void;
    allowUnboundSelection?: boolean;
    nativeLoginOnly?: boolean;
  }) => (
    <div>
      <output data-testid="native-login-only">
        {nativeLoginOnly ? "true" : "false"}
      </output>
      <button
        type="button"
        disabled={nativeLoginOnly}
        onClick={() => onAccountSelect?.("acct-managed")}
      >
        select-managed-account
      </button>
      {allowUnboundSelection && (
        <button type="button" onClick={() => onAccountSelect?.(null)}>
          select-native-login
        </button>
      )}
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
    providerApiMocks.getAll.mockReset().mockResolvedValue({
      "codex-official": {
        id: "codex-official",
        name: "OpenAI Official",
        settingsConfig: { auth: {}, config: "" },
        category: "official",
      },
    });
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

  it("requires a managed account when adding another Official card", async () => {
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    await screen.findByRole("button", { name: "select-managed-account" });
    expect(
      screen.queryByRole("button", { name: "select-native-login" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith("选择一个 ChatGPT 账号"),
    );
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("offers the current-login option only when the fixed card is missing", async () => {
    providerApiMocks.getAll.mockResolvedValueOnce({});
    const onSubmit = vi.fn();
    renderCodexForm(onSubmit);

    fireEvent.click(screen.getByRole("button", { name: /OpenAI Official/ }));
    const nativeLogin = await screen.findByRole("button", {
      name: "select-native-login",
    });
    fireEvent.click(nativeLogin);
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(submitted.presetCategory).toBe("official");
    expect(submitted.meta?.authBinding).toBeUndefined();
  });

  it("keeps the fixed Official card locked to the current Codex login", () => {
    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="codex-official"
          submitLabel="save-provider"
          onSubmit={vi.fn()}
          onCancel={vi.fn()}
          initialData={{
            name: "OpenAI Official",
            category: "official",
            settingsConfig: { auth: {}, config: "" },
          }}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByTestId("native-login-only")).toHaveTextContent("true");
    expect(
      screen.getByRole("button", { name: "select-managed-account" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "select-native-login" }),
    ).toBeInTheDocument();
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
            category: "official",
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

  it("does not offer the current-login option on a managed Official card", () => {
    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="managed-official"
          submitLabel="save-provider"
          onSubmit={vi.fn()}
          onCancel={vi.fn()}
          initialData={{
            name: "OpenAI Official (user@example.com)",
            category: "official",
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

    expect(screen.getByTestId("native-login-only")).toHaveTextContent("false");
    expect(
      screen.queryByRole("button", { name: "select-native-login" }),
    ).not.toBeInTheDocument();
  });

  it("keeps a legacy unbound Official card editable without forced migration", () => {
    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="legacy-unbound-official"
          submitLabel="save-provider"
          onSubmit={vi.fn()}
          onCancel={vi.fn()}
          initialData={{
            name: "Legacy OpenAI Official",
            category: "official",
            settingsConfig: { auth: {}, config: "" },
          }}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByTestId("native-login-only")).toHaveTextContent("false");
    expect(
      screen.getByRole("button", { name: "select-native-login" }),
    ).toBeInTheDocument();
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
