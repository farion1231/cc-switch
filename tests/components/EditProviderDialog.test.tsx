import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useEffect } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getCurrent: vi.fn(),
  getEditorView: vi.fn(),
  getLiveProviderSettings: vi.fn(),
  getOpenClawLiveProvider: vi.fn(),
}));
let mockFormReady = true;
let mockCodexManagedAccountSelected = false;
let submitReadyCallbacks: Array<(isReady: boolean) => void> = [];

vi.mock("@/lib/api", () => ({
  providersApi: {
    getCurrent: apiMocks.getCurrent,
    getEditorView: apiMocks.getEditorView,
  },
  vscodeApi: {
    getLiveProviderSettings: apiMocks.getLiveProviderSettings,
  },
  openclawApi: {
    getLiveProvider: apiMocks.getOpenClawLiveProvider,
  },
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    initialData,
    onSubmit,
    onSubmitReadyChange,
    onManageAuthAccounts,
    isProxyTakeover,
  }: {
    initialData: {
      name?: string;
      websiteUrl?: string;
      notes?: string;
      settingsConfig?: Record<string, unknown>;
      meta?: Record<string, unknown>;
      icon?: string;
      iconColor?: string;
    };
    onSubmit: (values: {
      name: string;
      websiteUrl: string;
      notes?: string;
      settingsConfig: string;
      meta?: Record<string, unknown>;
      icon?: string;
      iconColor?: string;
    }) => void;
    onSubmitReadyChange?: (isReady: boolean) => void;
    onManageAuthAccounts?: (target: "codex_oauth") => void;
    isProxyTakeover?: boolean;
    appId?: string;
  }) => {
    useEffect(() => {
      if (onSubmitReadyChange) {
        submitReadyCallbacks.push(onSubmitReadyChange);
        onSubmitReadyChange(mockFormReady);
      }
    }, [onSubmitReadyChange]);
    return (
      <form
        id="provider-form"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit({
            name: initialData.name ?? "",
            websiteUrl: initialData.websiteUrl ?? "",
            notes: initialData.notes,
            settingsConfig: JSON.stringify(initialData.settingsConfig ?? {}),
            meta: mockCodexManagedAccountSelected
              ? {
                  ...(initialData.meta ?? {}),
                  providerType: "codex_oauth",
                  authBinding: {
                    source: "managed_account",
                    authProvider: "codex_oauth",
                    accountId: "acct-managed",
                  },
                }
              : initialData.meta,
            icon: initialData.icon,
            iconColor: initialData.iconColor,
          });
        }}
      >
        <output data-testid="settings-config">
          {JSON.stringify(initialData.settingsConfig ?? {})}
        </output>
        <output data-testid="is-proxy-takeover">
          {isProxyTakeover ? "true" : "false"}
        </output>
        <button
          type="button"
          onClick={() => onManageAuthAccounts?.("codex_oauth")}
        >
          manage-auth
        </button>
      </form>
    );
  },
}));

vi.mock("@/components/providers/AuthSettingsPanel", () => ({
  AuthSettingsPanel: ({ target }: { target: string | null }) =>
    target ? <div data-testid="auth-settings-panel">{target}</div> : null,
}));

import { EditProviderDialog } from "@/components/providers/EditProviderDialog";

describe("EditProviderDialog", () => {
  beforeEach(() => {
    mockFormReady = true;
    mockCodexManagedAccountSelected = false;
    submitReadyCallbacks = [];
    apiMocks.getCurrent.mockReset();
    apiMocks.getEditorView.mockReset();
    apiMocks.getEditorView.mockImplementation(
      async (_app: string, settingsConfig: Record<string, unknown>) => ({
        settings: settingsConfig,
        inactive: [],
      }),
    );
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();
  });

  it("Codex 显示后端算出的切换投影，并把它作为保存时三方比较的基准", async () => {
    const modelCatalog = {
      models: [{ model: "deepseek-v4-flash", contextWindow: 1000000 }],
    };
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config: 'model_provider = "custom"\nmodel = "deepseek-v4-flash"\n',
        modelCatalog,
      },
    };
    const view = {
      auth: { OPENAI_API_KEY: "db-key" },
      config:
        'approval_policy = "never"\nmodel_provider = "custom"\nmodel = "deepseek-v4-flash"\n',
      modelCatalog,
    };
    apiMocks.getEditorView.mockResolvedValue({ settings: view, inactive: [] });
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(view);
    });
    expect(apiMocks.getEditorView).toHaveBeenCalledWith(
      "codex",
      provider.settingsConfig,
      "aggregator",
      provider.id,
      undefined,
    );
    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    const payload = handleSubmit.mock.calls[0][0];
    expect(payload.provider.settingsConfig).toEqual(view);
    expect(payload.editorSave).toEqual({ base: view, onConflict: "refuse" });
  });

  it.each([undefined, "openai_responses"] as const)(
    "passes stored Copilot metadata to the edit projection (%s)",
    async (format) => {
      const provider: Provider = {
        id: "copilot-card",
        name: "GitHub Copilot",
        category: "third_party",
        settingsConfig: {
          auth: {},
          config:
            'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://api.githubcopilot.com"\nrequires_openai_auth = true\n',
        },
        meta: {
          providerType: "github_copilot",
          apiFormat: format ?? "openai_chat",
          ...(format ? { codexCopilotApiFormat: format } : {}),
          authBinding: {
            source: "managed_account",
            authProvider: "github_copilot",
            accountId: "copilot-account",
          },
        },
      };
      const handleSubmit = vi.fn().mockResolvedValue(undefined);
      render(
        <EditProviderDialog
          open
          provider={provider}
          onOpenChange={vi.fn()}
          onSubmit={handleSubmit}
          appId="codex"
        />,
      );
      await waitFor(() =>
        expect(apiMocks.getEditorView).toHaveBeenCalledWith(
          "codex",
          provider.settingsConfig,
          provider.category,
          provider.id,
          provider.meta,
        ),
      );
      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
      const payload = handleSubmit.mock.calls[0][0];
      expect(payload.originalId).toBe(provider.id);
      expect(payload.provider.meta).toEqual(provider.meta);
      expect(payload.editorSave.base).toEqual(provider.settingsConfig);
    },
  );

  it.each([
    [
      "gemini",
      {
        env: { GEMINI_API_KEY: "db-key", GEMINI_MODEL: "m" },
        config: {},
      },
      {
        env: { GEMINI_SANDBOX: "docker", GEMINI_API_KEY: "db-key" },
        config: { ui: { theme: "dark" } },
      },
    ],
    [
      "grokbuild",
      { config: '[models]\ndefault = "grok-4.5"\n' },
      { config: '[ui]\ntheme = "dark"\n\n[models]\ndefault = "grok-4.5"\n' },
    ],
  ] as const)(
    "%s 也显示切换投影，并把它作为保存时三方比较的基准",
    async (appId, settingsConfig, view) => {
      const provider: Provider = {
        id: "p",
        name: "P",
        category: "custom",
        settingsConfig: settingsConfig as Record<string, unknown>,
      };
      apiMocks.getEditorView.mockResolvedValue({
        settings: view,
        inactive: [],
      });
      const handleSubmit = vi.fn().mockResolvedValue(undefined);

      render(
        <EditProviderDialog
          open
          provider={provider}
          onOpenChange={vi.fn()}
          onSubmit={handleSubmit}
          appId={appId}
        />,
      );

      await waitFor(() => {
        expect(
          JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
        ).toEqual(view);
      });
      expect(apiMocks.getEditorView).toHaveBeenCalledWith(
        appId,
        provider.settingsConfig,
        "custom",
        provider.id,
        undefined,
      );
      expect(apiMocks.getCurrent).not.toHaveBeenCalled();
      expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole("button", { name: "common.save" }));
      await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
      expect(handleSubmit.mock.calls[0][0].editorSave).toEqual({
        base: view,
        onConflict: "refuse",
      });
    },
  );

  it("Codex 读不了配置文件时退回显示保存的供应商配置", async () => {
    const provider: Provider = {
      id: "relay",
      name: "Relay",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
        config: 'model_provider = "custom"\n',
      },
    };
    apiMocks.getEditorView.mockRejectedValue(new Error("broken config.toml"));
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(provider.settingsConfig);
    });
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));
    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].editorSave).toBeUndefined();
  });

  it("代理模式下编辑 Codex 供应商也显示它自己的关键字段，不读 live 里的代理契约", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "custom",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config:
          'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://api.deepseek.com/v1"\n',
      },
    };

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
        isProxyTakeover
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("is-proxy-takeover").textContent).toBe("true");
    });

    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(provider.settingsConfig);
    });
    expect(apiMocks.getEditorView).toHaveBeenCalledWith(
      "codex",
      provider.settingsConfig,
      "custom",
      provider.id,
      undefined,
    );
  });

  it("clears the nested auth panel before the dialog reopens", async () => {
    const provider: Provider = {
      id: "official",
      name: "OpenAI Official",
      settingsConfig: { auth: {}, config: "" },
    };
    const props = {
      provider,
      onOpenChange: vi.fn(),
      onSubmit: vi.fn(),
      appId: "codex" as const,
    };
    const { rerender } = render(<EditProviderDialog open {...props} />);

    fireEvent.click(await screen.findByRole("button", { name: "manage-auth" }));
    expect(screen.getByTestId("auth-settings-panel")).toHaveTextContent(
      "codex_oauth",
    );

    rerender(<EditProviderDialog open={false} {...props} />);
    rerender(<EditProviderDialog open {...props} />);

    await waitFor(() => {
      expect(
        screen.queryByTestId("auth-settings-panel"),
      ).not.toBeInTheDocument();
    });
  });

  it("keeps an unbound Codex Official provider ID unchanged", async () => {
    apiMocks.getCurrent.mockResolvedValue(null);
    const onSubmit = vi.fn();
    const provider: Provider = {
      id: "legacy-unbound-official",
      name: "Legacy OpenAI Official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    };

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={onSubmit}
        appId="codex"
      />,
    );

    await screen.findByTestId("settings-config");
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        originalId: "legacy-unbound-official",
        provider: expect.objectContaining({ id: "legacy-unbound-official" }),
      }),
    );
  });

  it("keeps the fixed Codex provider ID when an account is bound", async () => {
    mockCodexManagedAccountSelected = true;
    apiMocks.getCurrent.mockResolvedValue(null);
    const onSubmit = vi.fn();
    const provider: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      category: "official",
      settingsConfig: { auth: {}, config: "" },
    };

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={onSubmit}
        appId="codex"
      />,
    );

    await screen.findByTestId("settings-config");
    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0];
    expect(submitted.originalId).toBe("codex-official");
    expect(submitted.provider.id).toBe("codex-official");
    expect(submitted.provider.meta?.authBinding).toEqual({
      source: "managed_account",
      authProvider: "codex_oauth",
      accountId: "acct-managed",
    });
  });

  it("编辑 Pi 供应商时保留通用元数据", async () => {
    const provider: Provider = {
      id: "pi-provider",
      name: "Pi Provider",
      settingsConfig: {
        baseUrl: "https://api.example.com/v1",
        models: [{ id: "model" }],
      },
      meta: {
        isPartner: true,
        endpointAutoSelect: true,
        custom_endpoints: {
          "https://failover.example.com/v1": {
            url: "https://failover.example.com/v1",
            addedAt: 1,
          },
        },
      },
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="pi"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.meta).toMatchObject({
      isPartner: true,
    });
    expect(handleSubmit.mock.calls[0][0]).not.toHaveProperty(
      "expectedSettingsConfig",
    );
  });

  it("重新打开 Pi 编辑表单后忽略上一轮的就绪回调", async () => {
    const provider: Provider = {
      id: "pi-provider",
      name: "Pi Provider",
      settingsConfig: { models: [{ id: "model" }] },
    };
    const props = {
      provider,
      onOpenChange: vi.fn(),
      onSubmit: vi.fn(),
      appId: "pi" as const,
    };
    const { rerender } = render(<EditProviderDialog open {...props} />);

    const saveButton = await screen.findByRole("button", {
      name: "common.save",
    });
    await waitFor(() => expect(saveButton).toBeEnabled());
    const staleCallback = submitReadyCallbacks.at(-1);
    expect(staleCallback).toBeDefined();

    rerender(<EditProviderDialog open={false} {...props} />);
    mockFormReady = false;
    rerender(<EditProviderDialog open {...props} />);
    const reopenedButton = await screen.findByRole("button", {
      name: "common.save",
    });
    await waitFor(() => expect(reopenedButton).toBeDisabled());

    act(() => staleCallback?.(true));
    expect(reopenedButton).toBeDisabled();
  });
});
