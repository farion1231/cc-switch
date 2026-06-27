import { createRef } from "react";
import {
  render,
  screen,
  waitFor,
  act,
  fireEvent,
  within,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const installedSkillsDataMock: Array<{
  id: string;
  name: string;
  description?: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  readmeUrl?: string;
  apps: {
    claude: boolean;
    codex: boolean;
    gemini: boolean;
    opencode: boolean;
    openclaw: boolean;
  };
  installedAt: number;
}> = [];
const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const batchToggleSkillAppMock = vi.fn();
const batchUninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: installedSkillsDataMock,
    isLoading: false,
  }),
  useSkillBackups: () => ({
    data: [],
    refetch: vi.fn(),
    isFetching: false,
  }),
  useDeleteSkillBackup: () => ({
    mutateAsync: deleteSkillBackupMock,
    isPending: false,
  }),
  useToggleSkillApp: () => ({
    mutateAsync: toggleSkillAppMock,
  }),
  useBatchToggleSkillApp: () => ({
    mutateAsync: batchToggleSkillAppMock,
    isPending: false,
  }),
  useRestoreSkillBackup: () => ({
    mutateAsync: restoreSkillBackupMock,
    isPending: false,
  }),
  useUninstallSkill: () => ({
    mutateAsync: uninstallSkillMock,
  }),
  useBatchUninstallSkill: () => ({
    mutateAsync: batchUninstallSkillMock,
    isPending: false,
  }),
  useScanUnmanagedSkills: () => ({
    data: [
      {
        directory: "shared-skill",
        name: "Shared Skill",
        description: "Imported from Claude",
        foundIn: ["claude"],
        path: "/tmp/shared-skill",
      },
    ],
    refetch: scanUnmanagedMock,
  }),
  useImportSkillsFromApps: () => ({
    mutateAsync: importSkillsMock,
  }),
  useInstallSkillsFromZip: () => ({
    mutateAsync: installFromZipMock,
  }),
  useCheckSkillUpdates: () => ({
    data: [],
    refetch: vi.fn(),
    isFetching: false,
  }),
  useUpdateSkill: () => ({
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    installedSkillsDataMock.splice(0, installedSkillsDataMock.length);
    scanUnmanagedMock.mockResolvedValue({
      data: [
        {
          directory: "shared-skill",
          name: "Shared Skill",
          description: "Imported from Claude",
          foundIn: ["claude"],
          path: "/tmp/shared-skill",
        },
      ],
    });
    toggleSkillAppMock.mockReset();
    uninstallSkillMock.mockReset();
    batchToggleSkillAppMock.mockResolvedValue({
      successIds: [],
      failed: [],
    });
    batchUninstallSkillMock.mockResolvedValue({
      successIds: [],
      failed: [],
    });
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    restoreSkillBackupMock.mockReset();
  });

  it("opens the import dialog without crashing when app toggles render", async () => {
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.openImport();
    });

    await waitFor(() => {
      expect(screen.getByText("skills.import")).toBeInTheDocument();
      expect(screen.getByText("Shared Skill")).toBeInTheDocument();
      expect(screen.getByText("/tmp/shared-skill")).toBeInTheDocument();
    });
  });

  it("filters installed skills by search query", async () => {
    installedSkillsDataMock.push(
      {
        id: "owner/repo:alpha",
        name: "Alpha Skill",
        description: "Handle alpha workflow",
        directory: "alpha-skill",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
        },
        installedAt: Date.now(),
      },
      {
        id: "local:beta",
        name: "Beta Toolkit",
        description: "Local beta helper",
        directory: "beta-toolkit",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
        },
        installedAt: Date.now(),
      },
    );

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
    expect(screen.getByText("Beta Toolkit")).toBeInTheDocument();

    fireEvent.change(
      screen.getByPlaceholderText("skills.batch.searchInstalledPlaceholder"),
      {
        target: { value: "beta-toolkit" },
      },
    );

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Beta Toolkit")).toBeInTheDocument();
    });
  });

  it("selects only filtered skills in batch mode", async () => {
    installedSkillsDataMock.push(
      {
        id: "owner/repo:alpha",
        name: "Alpha Skill",
        description: "Handle alpha workflow",
        directory: "alpha-skill",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
        },
        installedAt: Date.now(),
      },
      {
        id: "owner/repo:beta",
        name: "Beta Skill",
        description: "Handle beta workflow",
        directory: "beta-skill",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
        },
        installedAt: Date.now(),
      },
    );

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "skills.batch.enterMode" }),
    );

    fireEvent.change(
      screen.getByPlaceholderText("skills.batch.searchInstalledPlaceholder"),
      {
        target: { value: "beta" },
      },
    );

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
    });

    fireEvent.click(
      screen.getByRole("button", { name: "skills.batch.selectAllFiltered" }),
    );

    expect(screen.getByText("skills.batch.selectedCount")).toBeInTheDocument();

    const listContainer = screen.getByText("Beta Skill").closest(".group");
    expect(listContainer).not.toBeNull();
    const checkbox = within(listContainer as HTMLElement).getByRole("checkbox");
    expect(checkbox).toHaveAttribute("data-state", "checked");
  });

  it("applies batch toggle to the selected target app instead of defaulting to claude", async () => {
    installedSkillsDataMock.push({
      id: "owner/repo:alpha",
      name: "Alpha Skill",
      description: "Handle alpha workflow",
      directory: "alpha-skill",
      repoOwner: "owner",
      repoName: "repo",
      repoBranch: "main",
      apps: {
        claude: false,
        codex: false,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
      installedAt: Date.now(),
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="openclaw" />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "skills.batch.enterMode" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "skills.batch.selectAllFiltered" }),
    );

    fireEvent.click(screen.getByTestId("skills-batch-target-opencode"));
    fireEvent.click(
      screen.getByRole("button", { name: "skills.batch.enableSelectedFor" }),
    );

    await waitFor(() => {
      expect(batchToggleSkillAppMock).toHaveBeenCalledWith({
        items: [
          {
            id: "owner/repo:alpha",
            app: "opencode",
            enabled: true,
          },
        ],
      });
    });
  });
});
