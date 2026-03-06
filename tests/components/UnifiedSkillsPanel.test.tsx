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
const toggleSkillMutateAsyncMock = vi.fn();

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
      checkInstalledUpdates: vi.fn(),
      uninstallUnified: vi.fn(),
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
    toggleSkillMutateAsyncMock.mockReset();

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
      refetch: vi.fn(),
    });
    useBatchUpdateSkillsMock.mockReturnValue({
      mutateAsync: vi.fn(),
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
