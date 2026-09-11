import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useEffect } from "react";
import ReactDOM from "react-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getCurrent: vi.fn(),
  getLiveProviderSettings: vi.fn(),
  getOpenClawLiveProvider: vi.fn(),
  getHermesLiveProvider: vi.fn(),
}));
let mockFormReady = true;
let mockCodexManagedAccountSelected = false;
let submitReadyCallbacks: Array<(isReady: boolean) => void> = [];

vi.mock("@/lib/api", () => ({
  providersApi: {
    getCurrent: apiMocks.getCurrent,
  },
  vscodeApi: {
    getLiveProviderSettings: apiMocks.getLiveProviderSettings,
  },
  openclawApi: {
    getLiveProvider: apiMocks.getOpenClawLiveProvider,
  },
}));

vi.mock("@/lib/api/hermes", () => ({
  hermesApi: {
    getLiveProvider: apiMocks.getHermesLiveProvider,
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
        data-testid="provider-form"
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
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();
    apiMocks.getHermesLiveProvider.mockReset();
  });

  it("保留 Codex 数据库中的 modelCatalog，避免 live 配置缺字段时清空模型映射", async () => {
    const dbModelCatalog = {
      models: [
        {
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Flash",
          contextWindow: 1000000,
        },
      ],
    };
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "db-key",
        },
        config: 'model_provider = "custom"\nmodel = "deepseek-v4-flash"\n',
        modelCatalog: dbModelCatalog,
      },
    };
    const liveSettings = {
      auth: {
        OPENAI_API_KEY: "live-key",
      },
      config: 'model_provider = "custom"\nmodel = "deepseek-v4-pro"\n',
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(liveSettings);

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
      ).toEqual({
        ...liveSettings,
        modelCatalog: dbModelCatalog,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.settingsConfig).toEqual({
      ...liveSettings,
      modelCatalog: dbModelCatalog,
    });
  });

  it("uses the current Codex live bearer with the stored provider auth template", async () => {
    const provider: Provider = {
      id: "provider-a",
      name: "Provider A",
      category: "custom",
      settingsConfig: {
        auth: {
          OPENAI_API_KEY: "sk-db-stale",
          provider_note: "keep-me",
        },
        config:
          'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://proxy.example/v1"\n',
      },
    };
    const liveSettings = {
      // Shared auth.json belongs to another provider / official login cache.
      auth: {
        OPENAI_API_KEY: "sk-shared-other-provider",
        tokens: { account_id: "shared-account" },
      },
      config:
        'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://proxy.example/v1"\nexperimental_bearer_token = "sk-provider-a"\n',
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(liveSettings);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    const expectedSettings = {
      ...liveSettings,
      auth: {
        OPENAI_API_KEY: "sk-provider-a",
        provider_note: "keep-me",
      },
    };

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(expectedSettings);
    });

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.settingsConfig).toEqual(
      expectedSettings,
    );
  });

  it("does not convert an OAuth-only Codex provider into an API-key provider", async () => {
    const provider: Provider = {
      id: "oauth-provider",
      name: "OAuth Provider",
      category: "custom",
      settingsConfig: {
        auth: {
          auth_mode: "chatgpt",
          tokens: { account_id: "stored-account" },
        },
        config: 'model_provider = "custom"\n',
      },
    };
    const liveSettings = {
      auth: {
        auth_mode: "chatgpt",
        tokens: { account_id: "live-account" },
      },
      config:
        'model_provider = "custom"\nexperimental_bearer_token = "sk-route-only"\n',
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(liveSettings);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(liveSettings);
    });
  });

  it("does not let a stored bearer override a non-current Codex provider auth", async () => {
    const provider: Provider = {
      id: "provider-a",
      name: "Provider A",
      category: "custom",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-db-authoritative" },
        config:
          'model_provider = "custom"\nexperimental_bearer_token = "sk-leftover-live"\n',
      },
    };
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    apiMocks.getCurrent.mockResolvedValue("provider-b");

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={handleSubmit}
        appId="codex"
      />,
    );

    await waitFor(() => expect(apiMocks.getCurrent).toHaveBeenCalledTimes(1));
    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);

    fireEvent.click(screen.getByRole("button", { name: "common.save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    expect(handleSubmit.mock.calls[0][0].provider.settingsConfig).toEqual(
      provider.settingsConfig,
    );
  });

  it("代理接管中编辑 Codex 供应商时展示数据库配置而不是读取 live 代理配置", async () => {
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

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue({
      auth: {
        OPENAI_API_KEY: "PROXY_MANAGED",
      },
      config:
        'model_provider = "custom"\n[model_providers.custom]\nbase_url = "http://127.0.0.1:15721/v1"\nexperimental_bearer_token = "PROXY_MANAGED"\n',
    });

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
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
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

    // 分支保留 P2-2 live 加载 gate：表单在 live 状态就绪后才渲染，先等按钮出现。
    await screen.findByRole("button", { name: "manage-auth" });
    fireEvent.click(screen.getByRole("button", { name: "manage-auth" }));
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

    // 分支保留 P2-2 live 加载 gate：等保存按钮就绪（启用）后再点击。
    const saveButton = await screen.findByRole("button", {
      name: "common.save",
    });
    await waitFor(() => expect(saveButton).toBeEnabled());
    fireEvent.click(saveButton);

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

    // 分支保留 P2-2 live 加载 gate：等保存按钮就绪（启用）后再点击。
    const saveButton = await screen.findByRole("button", {
      name: "common.save",
    });
    await waitFor(() => expect(saveButton).toBeEnabled());
    fireEvent.click(saveButton);

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

describe("EditProviderDialog live 加载 gate 与失败处理", () => {
  beforeEach(() => {
    mockFormReady = true;
    mockCodexManagedAccountSelected = false;
    submitReadyCallbacks = [];
    apiMocks.getCurrent.mockReset();
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();
    apiMocks.getHermesLiveProvider.mockReset();
  });
  it("live 配置加载完成前不渲染表单，加载完成后出现", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
      },
    };
    let resolveLive: (value: Record<string, unknown>) => void = () => {};
    const livePromise = new Promise<Record<string, unknown>>((resolve) => {
      resolveLive = resolve;
    });

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockReturnValue(livePromise);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();

    resolveLive({});
    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
  });

  it("live 配置加载失败时禁用保存且不渲染表单", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
      },
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockRejectedValue(new Error("boom"));

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled(),
    );
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("live-load-error")).toHaveTextContent(
      /读取实时配置失败/,
    );
  });

  it("OpenClaw live 配置加载失败时禁用保存且不渲染表单", async () => {
    const provider: Provider = {
      id: "openclaw-provider",
      name: "OpenClaw Provider",
      category: "custom",
      settingsConfig: { base_url: "https://db.example.com" },
    };

    apiMocks.getOpenClawLiveProvider.mockRejectedValue(new Error("boom"));

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="openclaw"
      />,
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled(),
    );
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("live-load-error")).toHaveTextContent(
      /读取实时配置失败/,
    );
  });

  it("Hermes 编辑时用 live fragment 替换数据库快照作为表单初值", async () => {
    const provider: Provider = {
      id: "my-hermes",
      name: "My Hermes",
      category: "custom",
      settingsConfig: {
        name: "my-hermes",
        base_url: "https://db.example.com",
      },
    };
    const liveFragment = {
      name: "my-hermes",
      base_url: "https://live.example.com",
      api_key: "live-key",
      models: [{ id: "m1", displayName: "M1" }],
      _cc_source: "custom_providers",
    };

    apiMocks.getHermesLiveProvider.mockResolvedValue(liveFragment);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="hermes"
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(liveFragment);
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    // Hermes 走专用分支读取 live，不经 getCurrent（additive 应用 current() 恒为空串）
    expect(apiMocks.getCurrent).not.toHaveBeenCalled();
    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
  });

  it("Hermes live 节点不存在时用数据库快照渲染并允许保存", async () => {
    const provider: Provider = {
      id: "my-hermes",
      name: "My Hermes",
      category: "custom",
      settingsConfig: {
        name: "my-hermes",
        base_url: "https://db.example.com",
      },
    };

    apiMocks.getHermesLiveProvider.mockResolvedValue(null);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="hermes"
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
  });

  it("Hermes live 配置加载失败时禁用保存且不渲染表单", async () => {
    const provider: Provider = {
      id: "my-hermes",
      name: "My Hermes",
      category: "custom",
      settingsConfig: {
        name: "my-hermes",
        base_url: "https://db.example.com",
      },
    };

    apiMocks.getHermesLiveProvider.mockRejectedValue(new Error("boom"));

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="hermes"
      />,
    );

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "common.save" }),
      ).toBeDisabled(),
    );
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByTestId("live-load-error")).toHaveTextContent(
      /读取实时配置失败/,
    );
  });

  it("providersApi.getCurrent 失败时禁用保存并显示 live 读取失败", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
      },
    };

    apiMocks.getCurrent.mockRejectedValue(new Error("boom"));

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("live-load-error")).toHaveTextContent(
        /读取实时配置失败/,
      );
    });
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
  });

  it("切换 provider 后的首个 commit 不渲染旧 live 表单", async () => {
    const providerA: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key-a" },
      },
    };
    const providerB: Provider = {
      id: "kimi",
      name: "Kimi",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key-b" },
      },
    };
    const liveA = { auth: { OPENAI_API_KEY: "live-key-a" } };
    const liveB = { auth: { OPENAI_API_KEY: "live-key-b" } };
    let resolveA: (value: Record<string, unknown>) => void = () => {};
    const livePromiseA = new Promise<Record<string, unknown>>((resolve) => {
      resolveA = resolve;
    });

    apiMocks.getCurrent
      .mockResolvedValueOnce(providerA.id)
      .mockResolvedValueOnce(providerB.id);
    apiMocks.getLiveProviderSettings
      .mockReturnValueOnce(livePromiseA)
      .mockReturnValueOnce(liveB);

    const view = render(
      <EditProviderDialog
        open
        provider={providerA}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
      { legacyRoot: true },
    );

    resolveA(liveA);
    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual(liveA);
    });

    // 直接 ReactDOM.render 绕开 RTL 的 act，让断言落在切换后的首个 commit 上。
    // 升级债：ReactDOM.render + legacyRoot 是 React 18 遗留 API，React 19 已移除，
    // 升级时本用例需改写为 createRoot（同步 render 的语义需另行补偿）。
    ReactDOM.render(
      <EditProviderDialog
        open
        provider={providerB}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
      view.container,
    );

    expect(apiMocks.getCurrent).toHaveBeenCalledTimes(1);
    expect(screen.getByText("加载中...")).toBeInTheDocument();
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();
  });

  it("同一会话切换 provider 时重新进入加载 gate，避免短暂用旧 live 配置渲染", async () => {
    const providerA: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key-a" },
      },
    };
    const providerB: Provider = {
      id: "kimi",
      name: "Kimi",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key-b" },
      },
    };
    let resolveA: (value: Record<string, unknown>) => void = () => {};
    let resolveB: (value: Record<string, unknown>) => void = () => {};
    const liveA = new Promise<Record<string, unknown>>((resolve) => {
      resolveA = resolve;
    });
    const liveB = new Promise<Record<string, unknown>>((resolve) => {
      resolveB = resolve;
    });

    apiMocks.getCurrent
      .mockResolvedValueOnce(providerA.id)
      .mockResolvedValueOnce(providerB.id);
    apiMocks.getLiveProviderSettings
      .mockReturnValueOnce(liveA)
      .mockReturnValueOnce(liveB);

    const view = render(
      <EditProviderDialog
        open
        provider={providerA}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    resolveA({ auth: { OPENAI_API_KEY: "live-key-a" } });
    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual({ auth: { OPENAI_API_KEY: "live-key-a" } });
    });

    view.rerender(
      <EditProviderDialog
        open
        provider={providerB}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );
    expect(screen.getByText("加载中...")).toBeInTheDocument();
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.save" })).toBeDisabled();

    resolveB({ auth: { OPENAI_API_KEY: "live-key-b" } });
    await waitFor(() => {
      expect(
        JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
      ).toEqual({ auth: { OPENAI_API_KEY: "live-key-b" } });
    });
  });

  // null 是后端对"live 文件缺失"的真实契约（read_live_provider_settings 对
  // *.missing 错误码返回 Ok(null)），不再是只有测试才会构造的假分支。
  it("live 配置不存在时仍使用数据库快照渲染并允许保存", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
      },
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockResolvedValue(null);

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
  });

  it("live 读取失败后点击重试可恢复表单", async () => {
    const provider: Provider = {
      id: "deepseek",
      name: "DeepSeek",
      category: "aggregator",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "db-key" },
      },
    };

    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValue({ auth: { OPENAI_API_KEY: "live-key" } });

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="codex"
      />,
    );

    await waitFor(() =>
      expect(screen.getByTestId("live-load-error")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("provider-form")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("live-load-retry"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
    expect(screen.queryByTestId("live-load-error")).not.toBeInTheDocument();
  });

  it("claude-desktop 编辑激活供应商：不走 live 读取门，直接用数据库快照渲染表单", async () => {
    const provider: Provider = {
      id: "cd-current",
      name: "CD Current",
      settingsConfig: {
        ANTHROPIC_AUTH_TOKEN: "db-token",
        ANTHROPIC_BASE_URL: "https://db.example.com",
      },
    };

    // 后端 read_live_settings 对 claude-desktop 永远返回"不支持"错误；
    // 豁免名单生效时应压根不发起这两次调用
    apiMocks.getCurrent.mockResolvedValue(provider.id);
    apiMocks.getLiveProviderSettings.mockRejectedValue(
      new Error("Claude Desktop 3P 配置不支持作为通用 live 配置导入"),
    );

    render(
      <EditProviderDialog
        open
        provider={provider}
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        appId="claude-desktop"
      />,
    );

    expect(apiMocks.getLiveProviderSettings).not.toHaveBeenCalled();
    expect(apiMocks.getCurrent).not.toHaveBeenCalled();

    expect(screen.queryByTestId("live-load-error")).not.toBeInTheDocument();
    expect(screen.queryByTestId("live-load-retry")).not.toBeInTheDocument();

    await waitFor(() =>
      expect(screen.getByTestId("provider-form")).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: "common.save" })).toBeEnabled();
    expect(
      JSON.parse(screen.getByTestId("settings-config").textContent ?? "{}"),
    ).toEqual(provider.settingsConfig);
  });
});
