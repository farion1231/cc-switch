import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CopilotByokSettings } from "@/components/settings/CopilotByokSettings";
import type {
  CopilotByokGroup,
  CopilotByokState,
  CopilotByokTargetState,
} from "@/lib/api";

const mocks = vi.hoisted(() => ({
  getState: vi.fn(),
  setTargets: vi.fn(),
  importModels: vi.fn(),
  upsertGroup: vi.fn(),
  deleteGroup: vi.fn(),
  reorderGroups: vi.fn(),
  checkConnection: vi.fn(),
  updateUsageScript: vi.fn(),
  usageFooter: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  copilotByokApi: {
    getState: mocks.getState,
    setTargets: mocks.setTargets,
    importModels: mocks.importModels,
    upsertGroup: mocks.upsertGroup,
    deleteGroup: mocks.deleteGroup,
    reorderGroups: mocks.reorderGroups,
    checkConnection: mocks.checkConnection,
    updateUsageScript: mocks.updateUsageScript,
  },
  copilotCliApi: {},
  settingsApi: { pickDirectory: vi.fn() },
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
        "provider.noProviders": "还没有添加任何供应商",
        "provider.noProvidersDescription":
          '如果你已有配置，请点击"导入当前配置"，所有数据将安全保存在 default 供应商中',
        "provider.importCurrent": "导入当前配置",
        "provider.importCurrentDescription":
          "将当前正在使用的配置导入为默认供应商",
        "provider.addProvider": "添加供应商",
        "provider.dragHandle": "拖拽排序",
        "provider.removeFromConfig": "移除",
        "provider.addToConfig": "添加",
        "provider.duplicate": "复制",
        "provider.connectivityCheck": "检测连通",
        "provider.configureUsage": "配置用量查询",
        "provider.usageSaved": "用量查询配置已保存",
        "provider.usageSaveFailed": "用量查询配置保存失败",
        "usage.title": "使用统计",
        "apps.copilotByok": "VS Code Copilot",
        "common.refresh": "刷新",
        "common.confirm": "确认",
        "common.cancel": "取消",
        "confirm.deleteProvider": "删除供应商",
        "copilotByok.targets": "同步目标",
        "copilotByok.optInTitle": "当前未管理任何 Profile",
        "copilotByok.stopManaging": "停止管理所选 Profile",
        "copilotByok.securityTitle": "API Key 存储提示",
        "copilotByok.repairSync": "重新同步",
        "common.edit": "编辑",
        "common.delete": "删除",
      })[key] ?? key,
    i18n: { resolvedLanguage: "zh" },
  }),
}));

vi.mock("@/components/settings/CopilotByokGroupPanel", () => ({
  CopilotByokGroupPanel: () => null,
}));

const target: CopilotByokTargetState = {
  id: "stable:default",
  source: "detected",
  edition: "stable" as const,
  editionName: "Visual Studio Code",
  profileId: null,
  profileName: "Default",
  isDefault: true,
  languageModelsPath: "C:\\Code\\User\\chatLanguageModels.json",
  configExists: false,
  backupExists: false,
  selected: false,
  managedGroupCount: 0,
  readError: null,
};

function state(selected: boolean): CopilotByokState {
  return {
    groups: [],
    targets: [{ ...target, selected }],
    selectedTargetIds: selected ? [target.id] : [],
    managedModelCount: 0,
    cli: {
      supported: true,
      enabled: false,
      selectedGroupId: null,
      selectedModelId: null,
      selectedProviderName: null,
      selectedModelName: null,
      environmentMatches: false,
      environmentConflicts: [],
      officialActivationRequiresConfirmation: false,
    },
  };
}

const group: CopilotByokGroup = {
  id: "moonshot",
  name: "Moonshot",
  url: "https://api.example.com/v1/responses",
  apiKey: "secret",
  apiType: "responses",
  enabled: true,
  requestHeaders: {},
  extra: {},
  models: [
    {
      id: "kimi-k3",
      modelId: "kimi-k3",
      name: "Kimi K3",
      enabled: true,
      toolCalling: true,
      vision: false,
      thinking: true,
      streaming: true,
      contextWindow: 128000,
      maxInputTokens: null,
      maxOutputTokens: 8192,
      editTools: [],
      zeroDataRetentionEnabled: false,
      supportsReasoningEffort: [],
      reasoningEffortFormat: null,
      modelOptions: {},
      extra: {},
    },
  ],
};

describe("CopilotByokSettings", () => {
  beforeEach(() => {
    mocks.updateUsageScript.mockReset();
    mocks.usageFooter.mockReset();
    mocks.getState.mockResolvedValue(state(true));
    mocks.setTargets.mockResolvedValue(state(false));
    mocks.importModels.mockResolvedValue({
      targetId: target.id,
      importedGroupCount: 0,
      importedModelCount: 0,
      reusedModelCount: 0,
      skippedGroupCount: 0,
      changedTargetCount: 0,
      warnings: [],
    });
    mocks.upsertGroup.mockResolvedValue(state(true));
    mocks.deleteGroup.mockResolvedValue(state(true));
    mocks.reorderGroups.mockResolvedValue(state(true));
    mocks.updateUsageScript.mockResolvedValue({
      ...state(true),
      groups: [group],
    });
  });

  it("loads with the VS Code default profile selected", async () => {
    render(<CopilotByokSettings mode="targets" />);

    expect(await screen.findByRole("checkbox")).toBeChecked();
    expect(
      screen.queryByText("当前未管理任何 Profile"),
    ).not.toBeInTheDocument();
    expect(mocks.setTargets).not.toHaveBeenCalled();
  });

  it("clears target selection when stopping management", async () => {
    mocks.getState.mockResolvedValue(state(true));
    render(<CopilotByokSettings mode="targets" />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "停止管理所选 Profile",
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "确认" }));

    await waitFor(() => expect(mocks.setTargets).toHaveBeenCalledWith([]));
    expect(screen.getByText("当前未管理任何 Profile")).toBeInTheDocument();
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });

  it("shows only provider cards in catalog mode", async () => {
    mocks.getState.mockResolvedValue({ ...state(true), groups: [group] });
    render(<CopilotByokSettings mode="catalog" />);

    expect(await screen.findByText("Moonshot")).toBeInTheDocument();
    expect(
      screen.getByText("https://api.example.com/v1/responses"),
    ).toBeInTheDocument();
    expect(screen.queryByText("1/1 个模型")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "拖拽排序" }),
    ).toBeInTheDocument();
    expect(screen.getByText("移除")).toBeInTheDocument();
    expect(screen.getByTitle("编辑")).toBeInTheDocument();
    expect(screen.getByTitle("复制")).toBeInTheDocument();
    expect(screen.getByTitle("检测连通")).toBeInTheDocument();
    expect(screen.queryByTitle("使用统计")).not.toBeInTheDocument();
    expect(screen.getByTitle("删除")).toBeInTheDocument();
    expect(screen.queryByText("同步目标")).not.toBeInTheDocument();
    expect(screen.queryByText("API Key 存储提示")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "重新同步" }),
    ).not.toBeInTheDocument();
  });

  it("shows sync targets without provider catalog actions", async () => {
    mocks.getState.mockResolvedValue({ ...state(true), groups: [group] });
    render(<CopilotByokSettings mode="targets" />);

    expect(await screen.findByText("同步目标")).toBeInTheDocument();
    expect(screen.queryByText("API Key 存储提示")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "添加供应商" }),
    ).not.toBeInTheDocument();
  });

  it("opens and saves usage query configuration for VS Code Copilot providers", async () => {
    mocks.getState.mockResolvedValue({ ...state(true), groups: [group] });
    render(<CopilotByokSettings mode="catalog" />);

    fireEvent.click(await screen.findByTitle("配置用量查询"));
    fireEvent.click(await screen.findByText("保存 Moonshot 用量"));

    await waitFor(() =>
      expect(mocks.updateUsageScript).toHaveBeenCalledWith(
        "moonshot",
        expect.objectContaining({ enabled: true }),
      ),
    );
  });

  it("queries enabled usage on VS Code Copilot provider cards", async () => {
    const usageGroup: CopilotByokGroup = {
      ...group,
      usageScript: {
        enabled: true,
        language: "javascript",
        code: "return { remaining: 1 };",
        autoQueryInterval: 5,
      },
    };
    mocks.getState.mockResolvedValue({
      ...state(true),
      groups: [usageGroup],
    });

    render(<CopilotByokSettings mode="catalog" />);

    expect(
      await screen.findByTestId("usage-copilot-byok-moonshot"),
    ).toBeInTheDocument();
    expect(mocks.usageFooter).toHaveBeenCalledWith(
      expect.objectContaining({
        appId: "copilot-byok",
        providerId: "moonshot",
        usageEnabled: true,
        isCurrent: true,
        isInConfig: true,
        inline: true,
      }),
    );
  });

  it("reuses the standard empty state and imports the selected VS Code config", async () => {
    render(<CopilotByokSettings mode="catalog" />);

    expect(await screen.findByText("还没有添加任何供应商")).toBeInTheDocument();
    expect(
      screen.getByText(
        '如果你已有配置，请点击"导入当前配置"，所有数据将安全保存在 default 供应商中',
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "导入当前配置" }));
    await waitFor(() =>
      expect(mocks.importModels).toHaveBeenCalledWith(target.id),
    );
    expect(screen.getByRole("button", { name: "添加供应商" })).toBeVisible();
  });
});
