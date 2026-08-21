import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotCliSettings } from "@/components/settings/CopilotCliSettings";
import type { CopilotByokGroup, CopilotByokState } from "@/lib/api";

const mocks = vi.hoisted(() => ({
  cliGetState: vi.fn(),
  setSelection: vi.fn(),
  disable: vi.fn(),
  upsertGroup: vi.fn(),
  deleteGroup: vi.fn(),
  reorderGroups: vi.fn(),
  checkConnection: vi.fn(),
  updateUsageScript: vi.fn(),
  openTerminal: vi.fn(),
  pickDirectory: vi.fn(),
  vscodeGetState: vi.fn(),
  usageFooter: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  copilotCliApi: {
    getState: mocks.cliGetState,
    setSelection: mocks.setSelection,
    disable: mocks.disable,
    upsertGroup: mocks.upsertGroup,
    deleteGroup: mocks.deleteGroup,
    reorderGroups: mocks.reorderGroups,
    checkConnection: mocks.checkConnection,
    updateUsageScript: mocks.updateUsageScript,
    openTerminal: mocks.openTerminal,
  },
  copilotByokApi: {
    getState: mocks.vscodeGetState,
  },
  settingsApi: {
    pickDirectory: mocks.pickDirectory,
  },
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ provider, isOpen, onSave }: any) =>
    isOpen ? (
      <button
        type="button"
        onClick={() =>
          onSave({
            enabled: true,
            language: "javascript",
            code: "return { remaining: 1 };",
          })
        }
      >
        保存 {provider.name} 用量
      </button>
    ) : null,
}));

vi.mock("@/components/UsageFooter", () => ({
  default: (props: any) => {
    mocks.usageFooter(props);
    return props.usageEnabled ? (
      <div data-testid={`usage-${props.appId}-${props.providerId}`} />
    ) : null;
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "apps.copilotCli": "Copilot CLI",
        "copilotByok.cli.provider": "供应商",
        "copilotByok.cli.model": "模型",
        "copilotByok.cli.defaultModel": "默认模型",
        "copilotByok.cli.apply": "应用到 Copilot CLI",
        "copilotByok.cli.disable": "恢复原环境",
        "copilotByok.cli.officialConfirmTitle": "清除未受管理的覆盖值？",
        "copilotByok.cli.officialConfirm": "将清除以下覆盖值",
        "copilotByok.cli.officialConfirmAction": "清除并使用官方供应商",
        "copilotByok.cli.active": "已生效",
        "copilotByok.cli.inactive": "未配置",
        "copilotByok.cli.needsApply": "需要重新应用",
        "copilotByok.cli.catalogDescription": "CLI 独立供应商目录",
        "provider.tabProvider": "供应商",
        "provider.inUse": "使用中",
        "provider.enable": "启用",
        "provider.removeFromConfig": "移除",
        "provider.addToConfig": "添加",
        "provider.dragHandle": "拖拽排序",
        "provider.duplicate": "复制",
        "provider.connectivityCheck": "检测连通",
        "provider.configureUsage": "配置用量查询",
        "provider.openTerminal": "打开终端",
        "provider.terminalOpened": "终端已打开",
        "provider.terminalOpenFailed": "打开终端失败",
        "provider.usageSaved": "用量查询配置已保存",
        "provider.usageSaveFailed": "用量查询配置保存失败",
        "common.refresh": "刷新",
        "common.edit": "编辑",
        "common.delete": "删除",
        "common.copy": "复制",
      })[key] ?? key,
    i18n: { resolvedLanguage: "zh" },
  }),
}));

vi.mock("@/components/settings/CopilotByokGroupPanel", () => ({
  CopilotByokGroupPanel: () => null,
}));

const group: CopilotByokGroup = {
  id: "cli-provider",
  name: "CLI Provider",
  url: "https://api.example.com/v1/responses",
  apiKey: "secret",
  apiType: "responses",
  enabled: true,
  requestHeaders: {},
  extra: {},
  models: [
    {
      id: "cli-model",
      modelId: "gpt-custom",
      name: "GPT Custom",
      enabled: true,
      toolCalling: true,
      vision: false,
      thinking: true,
      streaming: true,
      contextWindow: 128_000,
      maxInputTokens: null,
      maxOutputTokens: 8_192,
      editTools: [],
      zeroDataRetentionEnabled: false,
      supportsReasoningEffort: [],
      reasoningEffortFormat: null,
      modelOptions: {},
      extra: {},
    },
  ],
};

const officialGroup: CopilotByokGroup = {
  id: "copilot-cli-official",
  name: "GitHub Copilot Official",
  url: "",
  apiKey: "",
  apiType: "chat-completions",
  websiteUrl: "https://github.com/features/copilot",
  icon: "githubcopilot",
  category: "official",
  enabled: true,
  requestHeaders: {},
  extra: {},
  models: [],
};

function cliState(active: boolean): CopilotByokState {
  return {
    groups: [officialGroup, group],
    targets: [],
    selectedTargetIds: [],
    managedModelCount: 1,
    cli: {
      supported: true,
      enabled: active,
      selectedGroupId: active ? group.id : null,
      selectedModelId: active ? group.models[0].id : null,
      selectedProviderName: active ? group.name : null,
      selectedModelName: active ? group.models[0].name : null,
      environmentMatches: true,
      environmentConflicts: [],
      officialActivationRequiresConfirmation: false,
    },
  };
}

describe("CopilotCliSettings", () => {
  beforeEach(() => {
    mocks.cliGetState.mockReset();
    mocks.setSelection.mockReset();
    mocks.disable.mockReset();
    mocks.upsertGroup.mockReset();
    mocks.deleteGroup.mockReset();
    mocks.reorderGroups.mockReset();
    mocks.checkConnection.mockReset();
    mocks.updateUsageScript.mockReset();
    mocks.openTerminal.mockReset();
    mocks.pickDirectory.mockReset();
    mocks.vscodeGetState.mockReset();
    mocks.usageFooter.mockReset();

    mocks.cliGetState.mockResolvedValue(cliState(false));
    mocks.setSelection.mockResolvedValue(cliState(true));
    mocks.disable.mockResolvedValue(cliState(false));
    mocks.upsertGroup.mockResolvedValue(cliState(false));
    mocks.deleteGroup.mockResolvedValue(cliState(false));
    mocks.reorderGroups.mockResolvedValue(cliState(false));
    mocks.updateUsageScript.mockResolvedValue(cliState(false));
    mocks.openTerminal.mockResolvedValue(true);
    mocks.pickDirectory.mockResolvedValue("C:\\Work");
  });

  it("shows a compact provider list and switches the provider directly", async () => {
    render(<CopilotCliSettings />);

    expect(await screen.findByText("CLI Provider")).toBeInTheDocument();
    expect(screen.getByText("GitHub Copilot Official")).toBeInTheDocument();
    expect(screen.queryByText(/默认模型:/)).not.toBeInTheDocument();
    expect(screen.queryByText("供应商")).not.toBeInTheDocument();
    expect(screen.queryByText("CLI 独立供应商目录")).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "启用" }));

    await waitFor(() =>
      expect(mocks.setSelection).toHaveBeenCalledWith(
        "cli-provider",
        "CLI Provider",
      ),
    );
    expect(mocks.cliGetState).toHaveBeenCalled();
    expect(mocks.vscodeGetState).not.toHaveBeenCalled();
  });

  it("keeps the fixed in-use action slot for the active provider and switches through Official", async () => {
    mocks.cliGetState.mockResolvedValue(cliState(true));
    render(<CopilotCliSettings />);

    const activeProvider = await screen.findByRole("group", {
      name: "CLI Provider",
    });
    expect(activeProvider).toHaveClass("border-blue-500/60");
    const inUseButton = within(activeProvider).getByRole("button", {
      name: "使用中",
    });
    expect(inUseButton).toBeDisabled();
    expect(inUseButton).toHaveClass("w-[4.5rem]");
    expect(screen.getByRole("button", { name: "删除" })).toBeDisabled();

    const officialProvider = screen.getByRole("group", {
      name: "GitHub Copilot Official",
    });
    fireEvent.click(
      within(officialProvider).getByRole("button", { name: "启用" }),
    );
    await waitFor(() =>
      expect(mocks.setSelection).toHaveBeenCalledWith(
        "copilot-cli-official",
        "GitHub Copilot Official",
      ),
    );
    expect(mocks.disable).not.toHaveBeenCalled();
  });

  it("requires confirmation before the official provider clears an unmanaged environment", async () => {
    const unmanaged = cliState(false);
    unmanaged.cli.environmentMatches = false;
    unmanaged.cli.environmentConflicts = [
      "COPILOT_PROVIDER_BASE_URL",
      "COPILOT_PROVIDER_API_KEY",
    ];
    unmanaged.cli.officialActivationRequiresConfirmation = true;
    mocks.cliGetState.mockResolvedValue(unmanaged);
    render(<CopilotCliSettings />);

    const officialProvider = await screen.findByRole("group", {
      name: "GitHub Copilot Official",
    });
    fireEvent.click(
      within(officialProvider).getByRole("button", { name: "启用" }),
    );

    expect(mocks.setSelection).not.toHaveBeenCalled();
    expect(
      await screen.findByText("清除未受管理的覆盖值？"),
    ).toBeInTheDocument();
    expect(screen.getByText(/COPILOT_PROVIDER_API_KEY/)).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "清除并使用官方供应商" }),
    );

    await waitFor(() =>
      expect(mocks.setSelection).toHaveBeenCalledWith(
        "copilot-cli-official",
        "GitHub Copilot Official",
        true,
      ),
    );
  });

  it("keeps Official sortable and exposes no previous-environment restore action", async () => {
    render(<CopilotCliSettings />);

    const officialProvider = await screen.findByRole("group", {
      name: "GitHub Copilot Official",
    });
    const inUseButton = within(officialProvider).getByRole("button", {
      name: "使用中",
    });
    expect(inUseButton).toBeDisabled();
    expect(inUseButton).toHaveClass("w-[4.5rem]");
    expect(
      within(officialProvider).getByRole("button", { name: "拖拽排序" }),
    ).toBeEnabled();
    expect(
      within(officialProvider).queryByText("恢复之前的环境"),
    ).not.toBeInTheDocument();
    expect(mocks.disable).not.toHaveBeenCalled();
  });

  it("configures usage queries and opens a provider-scoped CLI terminal", async () => {
    render(<CopilotCliSettings />);

    const provider = await screen.findByRole("group", { name: "CLI Provider" });
    fireEvent.click(within(provider).getByTitle("配置用量查询"));
    fireEvent.click(await screen.findByText("保存 CLI Provider 用量"));
    await waitFor(() =>
      expect(mocks.updateUsageScript).toHaveBeenCalledWith(
        "cli-provider",
        expect.objectContaining({ enabled: true }),
      ),
    );

    fireEvent.click(within(provider).getByTitle("打开终端"));
    await waitFor(() =>
      expect(mocks.openTerminal).toHaveBeenCalledWith(
        "cli-provider",
        "C:\\Work",
      ),
    );
  });

  it("queries enabled usage on the selected Copilot CLI provider card", async () => {
    const next = cliState(true);
    next.groups = next.groups.map((candidate) =>
      candidate.id === group.id
        ? {
            ...candidate,
            usageScript: {
              enabled: true,
              language: "javascript",
              code: "return { remaining: 1 };",
              autoQueryInterval: 5,
            },
          }
        : candidate,
    );
    mocks.cliGetState.mockResolvedValue(next);

    render(<CopilotCliSettings />);

    expect(
      await screen.findByTestId("usage-copilot-cli-cli-provider"),
    ).toBeInTheDocument();
    expect(mocks.usageFooter).toHaveBeenCalledWith(
      expect.objectContaining({
        appId: "copilot-cli",
        providerId: "cli-provider",
        usageEnabled: true,
        isCurrent: true,
        isInConfig: false,
        inline: true,
      }),
    );
  });
});
