import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastInfoMock = vi.fn();
const toastWarningMock = vi.fn();

const useInstalledSkillsMock = vi.fn();
const useToggleSkillAppMock = vi.fn();
const useUninstallSkillMock = vi.fn();
const useScanUnmanagedSkillsMock = vi.fn();
const useImportSkillsFromAppsMock = vi.fn();
const useInstallSkillsFromZipMock = vi.fn();
const useSkillUpdatesMock = vi.fn();
const useBatchUpdateSkillsMock = vi.fn();

const toggleAppApiMock = vi.fn();
const uninstallUnifiedApiMock = vi.fn();
const checkInstalledUpdatesApiMock = vi.fn();
const toggleSkillMutateAsyncMock = vi.fn();
const batchUpdateMutateAsyncMock = vi.fn();
const refetchUpdatesMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: (...args: unknown[]) => useInstalledSkillsMock(...args),
  useToggleSkillApp: (...args: unknown[]) => useToggleSkillAppMock(...args),
  useUninstallSkill: (...args: unknown[]) => useUninstallSkillMock(...args),
  useScanUnmanagedSkills: (...args: unknown[]) =>
    useScanUnmanagedSkillsMock(...args),
  useImportSkillsFromApps: (...args: unknown[]) =>
    useImportSkillsFromAppsMock(...args),
  useInstallSkillsFromZip: (...args: unknown[]) =>
    useInstallSkillsFromZipMock(...args),
  useSkillUpdates: (...args: unknown[]) => useSkillUpdatesMock(...args),
  useBatchUpdateSkills: (...args: unknown[]) => useBatchUpdateSkillsMock(...args),
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<object>("@/lib/api");
  return {
    ...actual,
    settingsApi: {
      openExternal: vi.fn(),
    },
    skillsApi: {
      toggleApp: (...args: unknown[]) => toggleAppApiMock(...args),
      openZipFileDialog: vi.fn(),
      checkInstalledUpdates: (...args: unknown[]) =>
        checkInstalledUpdatesApiMock(...args),
      uninstallUnified: (...args: unknown[]) => uninstallUnifiedApiMock(...args),
    },
  };
});

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, title, message, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <div>{title}</div>
        <div>{message}</div>
        <button onClick={() => onConfirm()}>confirm-action</button>
        <button onClick={() => onCancel()}>cancel-action</button>
      </div>
    ) : null,
}));

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

const installedSkills = [
  {
    id: "skill-1",
    name: "Skill One",
    description: "First skill",
    directory: "skill-one",
    repoOwner: "owner",
    repoName: "repo",
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
    },
    installedAt: 1,
  },
  {
    id: "skill-2",
    name: "Skill Two",
    description: "Second skill",
    directory: "skill-two",
    repoOwner: "owner",
    repoName: "repo",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
    },
    installedAt: 2,
  },
];

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
    toastWarningMock.mockReset();
    toggleAppApiMock.mockReset();
    uninstallUnifiedApiMock.mockReset();
    checkInstalledUpdatesApiMock.mockReset();
    toggleSkillMutateAsyncMock.mockReset();
    batchUpdateMutateAsyncMock.mockReset();
    refetchUpdatesMock.mockReset();

    useInstalledSkillsMock.mockReturnValue({
      data: installedSkills,
      isLoading: false,
    });
    useToggleSkillAppMock.mockReturnValue({
      mutateAsync: toggleSkillMutateAsyncMock,
    });
    useUninstallSkillMock.mockReturnValue({
      mutateAsync: vi.fn(),
    });
    useScanUnmanagedSkillsMock.mockReturnValue({
      data: [],
      refetch: vi.fn(),
    });
    useImportSkillsFromAppsMock.mockReturnValue({
      mutateAsync: vi.fn(),
    });
    useInstallSkillsFromZipMock.mockReturnValue({
      mutateAsync: vi.fn(),
    });
    useSkillUpdatesMock.mockReturnValue({
      data: [],
      refetch: refetchUpdatesMock,
    });
    useBatchUpdateSkillsMock.mockReturnValue({
      mutateAsync: batchUpdateMutateAsyncMock,
      isPending: false,
    });
  });

  it("defaults to single mode and keeps apply changes disabled", () => {
    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    expect(
      screen.getByRole("switch", { name: "Batch mode" }),
    ).not.toBeChecked();
    expect(screen.getByText("Apply changes (0)")).toBeDisabled();
  });

  it("applies single-app toggles immediately in single mode", async () => {
    toggleSkillMutateAsyncMock.mockResolvedValue(true);

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getAllByRole("button", { name: "Claude" })[0]);

    await waitFor(() => {
      expect(toggleSkillMutateAsyncMock).toHaveBeenCalledWith({
        id: "skill-1",
        app: "claude",
        enabled: true,
      });
    });
  });

  it("stages changes for all selected skills in batch mode and shows pending dots", async () => {
    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Gemini" })[0]);

    expect(toggleSkillMutateAsyncMock).not.toHaveBeenCalled();
    expect(screen.getByText("Apply changes (2)")).not.toBeDisabled();
    expect(screen.getAllByLabelText("Gemini pending")).toHaveLength(2);
  });

  it("auto-selects the clicked skill when staging without a prior selection", async () => {
    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("button", { name: "OpenCode" })[0]);

    expect(toastInfoMock).not.toHaveBeenCalled();
    expect(screen.getByText("Apply changes (1)")).not.toBeDisabled();
    expect(screen.getByLabelText("Select skill Skill One")).toBeChecked();
    expect(screen.getAllByLabelText("OpenCode pending")).toHaveLength(1);
  });

  it("confirms and submits staged changes for all selected skills", async () => {
    toggleAppApiMock.mockResolvedValue(true);

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Codex" })[0]);
    fireEvent.click(screen.getByText("Apply changes (2)"));

    expect(screen.getByTestId("confirm-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(toggleAppApiMock).toHaveBeenCalledTimes(2);
      expect(toggleAppApiMock).toHaveBeenCalledWith(
        "skill-1",
        "codex",
        true,
      );
      expect(toggleAppApiMock).toHaveBeenCalledWith(
        "skill-2",
        "codex",
        true,
      );
    });
  });

  it("keeps failed pending changes after partial apply failure", async () => {
    let callCount = 0;
    toggleAppApiMock.mockImplementation(async () => {
      callCount += 1;
      if (callCount === 2) {
        throw new Error("toggle failed");
      }
      return true;
    });

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Gemini" })[0]);
    fireEvent.click(screen.getByText("Apply changes (2)"));
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(toastWarningMock).toHaveBeenCalled();
    });

    expect(screen.getByText("Apply changes (1)")).not.toBeDisabled();
    expect(screen.getAllByLabelText("Gemini pending")).toHaveLength(1);
  });

  it("discards pending changes when leaving batch mode after confirmation", async () => {
    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("button", { name: "OpenCode" })[0]);

    expect(screen.getByText("Apply changes (1)")).not.toBeDisabled();

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    expect(screen.getByTestId("confirm-dialog")).toBeInTheDocument();

    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(
        screen.getByRole("switch", { name: "Batch mode" }),
      ).not.toBeChecked();
    });

    expect(screen.queryByLabelText("OpenCode pending")).not.toBeInTheDocument();
    expect(screen.getByText("Apply changes (0)")).toBeDisabled();
  });

  it("batch uninstalls selected skills and reports partial failures", async () => {
    uninstallUnifiedApiMock
      .mockResolvedValueOnce(true)
      .mockRejectedValueOnce(new Error("uninstall failed"));

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(
      screen.getByRole("button", { name: /Uninstall selected|卸载已选/ }),
    );
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(uninstallUnifiedApiMock).toHaveBeenCalledTimes(2);
      expect(toastWarningMock).toHaveBeenCalled();
    });
  });

  it("updates only selected skills with available updates", async () => {
    useSkillUpdatesMock.mockReturnValue({
      data: [
        { id: "skill-1", state: "update_available" },
        { id: "skill-2", state: "up_to_date" },
      ],
      refetch: refetchUpdatesMock,
    });
    batchUpdateMutateAsyncMock.mockResolvedValue({
      installed: [installedSkills[0]],
      failed: [],
    });

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);

    expect(
      screen.getByRole("button", { name: /Update selected|更新已选/ }),
    ).not.toBeDisabled();

    fireEvent.click(
      screen.getByRole("button", { name: /Update selected|更新已选/ }),
    );

    await waitFor(() => {
      expect(batchUpdateMutateAsyncMock).toHaveBeenCalledWith({
        ids: ["skill-1"],
        forceRefresh: true,
      });
      expect(refetchUpdatesMock).toHaveBeenCalled();
    });
  });

  it("checks updates and shows success when nothing is available", async () => {
    checkInstalledUpdatesApiMock.mockResolvedValue([
      { id: "skill-1", state: "up_to_date" },
      { id: "skill-2", state: "unknown" },
    ]);

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: /Check updates|skills\.checkUpdates/ }),
    );

    await waitFor(() => {
      expect(checkInstalledUpdatesApiMock).toHaveBeenCalledWith(true);
      expect(toastSuccessMock).toHaveBeenCalled();
    });
  });

  it("disables apply changes while staged updates are being submitted", async () => {
    let resolveToggle!: () => void;
    toggleAppApiMock.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          resolveToggle = () => resolve(true);
        }),
    );

    renderWithQueryClient(
      <UnifiedSkillsPanel onOpenDiscovery={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("switch", { name: "Batch mode" }));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("button", { name: "OpenCode" })[0]);
    fireEvent.click(screen.getByText("Apply changes (1)"));
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Apply changes (1)" }),
      ).toBeDisabled();
    });

    resolveToggle();

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Apply changes (0)" }),
      ).toBeDisabled();
    });
  });
});
