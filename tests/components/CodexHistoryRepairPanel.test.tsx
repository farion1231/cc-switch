import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexHistoryRepairPanel } from "@/components/sessions/CodexHistoryRepairPanel";
import { proxyApi } from "@/lib/api/proxy";

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    unlockCodexModelPicker: vi.fn(),
    listCodexHistorySessions: vi.fn(),
    readCodexHistorySession: vi.fn(),
    repairCodexHistoryVisibility: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
  },
}));

// 构造最小历史列表返回值，用于验证前端参数而不依赖真实 SQLite。
function historyListFixture(overrides = {}) {
  return {
    codexHome: "C:\\Users\\sunda\\.codex",
    stateDbPath: "C:\\Users\\sunda\\.codex\\state_5.sqlite",
    activeDbKind: "codex_root",
    liveConfigModelProvider: "openai",
    targetProviderCandidates: ["openai", "custom"],
    sourceCounts: [{ value: "vscode", count: 7 }],
    providerCounts: [
      { value: "openai", count: 4 },
      { value: "custom", count: 3 },
    ],
    totalMatched: 0,
    items: [],
    skippedReason: null,
    ...overrides,
  };
}

// 构造历史修复 dry-run/apply 返回值，覆盖确认流程需要展示的统计字段。
function repairOutcomeFixture(dryRun: boolean) {
  return {
    dryRun,
    codexHome: "C:\\Users\\sunda\\.codex",
    stateDbPath: "C:\\Users\\sunda\\.codex\\state_5.sqlite",
    activeDbKind: "codex_root",
    liveConfigModelProvider: "openai",
    targetProvider: "openai",
    sourceProviderIds: ["custom"],
    sqliteThreads: 7,
    rolloutThreadsDiscovered: 9,
    threadRowsToInsert: dryRun ? 2 : 0,
    threadRowsInserted: dryRun ? 0 : 2,
    threadMetadataRowsToRebuild: dryRun ? 5 : 0,
    threadMetadataRowsRebuilt: dryRun ? 0 : 5,
    backfillStateToUpdate: true,
    backfillStateUpdated: !dryRun,
    providerRowsToUpdate: dryRun ? 3 : 0,
    providerRowsUpdated: dryRun ? 0 : 3,
    rolloutFirstLinesToUpdate: 1,
    rolloutFirstLinesUpdated: dryRun ? 0 : 1,
    userEventRowsToUpdate: 0,
    userEventRowsUpdated: 0,
    visibleCandidateRows: 7,
    sessionIndexMissingToAppend: 2,
    sessionIndexAppended: dryRun ? 0 : 2,
    projectRows: 7,
    focusSelectedCount: 0,
    balancedRecentWindowEnabled: true,
    balancedRecentWindowRows: 7,
    balancedRecentWindowProjects: 2,
    maxPerProject: 10,
    maxTotal: 300,
    sourceFilter: "all",
    sqliteFocusRowsToUpdate: 0,
    sqliteFocusRowsUpdated: 0,
    sessionIndexTitlesToUpdate: 0,
    sessionIndexTitlesUpdated: 0,
    sessionIndexRowsToMove: 0,
    sessionIndexRowsMoved: 0,
    workspaceHintsToFix: 0,
    workspaceHintsFixed: 0,
    projectlessIdsToRemove: 0,
    projectlessIdsRemoved: 0,
    savedWorkspaceRootsToAdd: 0,
    savedWorkspaceRootsAdded: 0,
    rolloutMtimesToTouch: 1,
    rolloutMtimesTouched: dryRun ? 0 : 1,
    visibleProjectRowsInWindowBefore: 0,
    backupDir: dryRun ? null : "C:\\Users\\sunda\\.codex\\backups\\history",
    skippedReason: null,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
  vi.mocked(proxyApi.listCodexHistorySessions).mockResolvedValue(
    historyListFixture(),
  );
  vi.mocked(proxyApi.readCodexHistorySession).mockResolvedValue({
    codexHome: "C:\\Users\\sunda\\.codex",
    stateDbPath: "C:\\Users\\sunda\\.codex\\state_5.sqlite",
    activeDbKind: "codex_root",
    session: null,
    messages: [],
    rolloutPath: null,
    skippedReason: null,
  });
  vi.mocked(proxyApi.repairCodexHistoryVisibility)
    .mockResolvedValueOnce(repairOutcomeFixture(true))
    .mockResolvedValueOnce(repairOutcomeFixture(false));
});

describe("CodexHistoryRepairPanel", () => {
  it("loads all projects by default even when opened from a selected session", async () => {
    render(
      <CodexHistoryRepairPanel
        initialProjectPath="C:\\Users\\sunda\\Documents\\ACPs 2\\ACPs Agent Adapter"
        showAutomationGuide
      />,
    );

    await waitFor(() => {
      expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledTimes(1);
    });

    expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledWith(
      expect.objectContaining({
        projectPath: null,
        sourceFilter: "all",
      }),
    );
    expect(
      screen.getByPlaceholderText("勾选后填写项目路径；不勾选则修复所有项目"),
    ).toHaveValue("");
    expect(
      screen.getByText(/未限制项目；会跨项目读取并修复/),
    ).toBeInTheDocument();
  });

  it("only passes projectPath after the single-project scope is enabled", async () => {
    render(
      <CodexHistoryRepairPanel initialProjectPath="C:\\Users\\sunda\\Documents\\ACPs 2" />,
    );

    await waitFor(() => {
      expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledTimes(1);
    });
    vi.mocked(proxyApi.listCodexHistorySessions).mockClear();

    fireEvent.click(screen.getByRole("checkbox", { name: "只修复单个项目" }));
    fireEvent.change(
      screen.getByPlaceholderText("勾选后填写项目路径；不勾选则修复所有项目"),
      { target: { value: "C:\\Users\\sunda\\Documents\\Project A" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "刷新记录" }));

    await waitFor(() => {
      expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledWith(
        expect.objectContaining({
          projectPath: "C:\\Users\\sunda\\Documents\\Project A",
        }),
      );
    });
  });

  it("confirms with the close-Codex warning before applying the repair", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<CodexHistoryRepairPanel />);

    await waitFor(() => {
      expect(proxyApi.listCodexHistorySessions).toHaveBeenCalledTimes(1);
    });
    fireEvent.click(screen.getByRole("button", { name: "确认修复" }));

    await waitFor(() => {
      expect(proxyApi.repairCodexHistoryVisibility).toHaveBeenCalledTimes(2);
    });
    expect(proxyApi.repairCodexHistoryVisibility).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        dryRun: true,
        projectPath: null,
        targetProvider: null,
        sourceFilter: "all",
      }),
    );
    expect(confirmSpy).toHaveBeenCalledWith(
      expect.stringContaining("请先完全退出 Codex / ChatGPT App"),
    );
    expect(proxyApi.repairCodexHistoryVisibility).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        dryRun: false,
        projectPath: null,
        targetProvider: null,
      }),
    );

    confirmSpy.mockRestore();
  });
});
