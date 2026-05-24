import { createRef } from "react";
import {
  render,
  screen,
  waitFor,
  act,
  fireEvent,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
const openZipFileDialogMock = vi.hoisted(() => vi.fn());
const toastErrorMock = vi.hoisted(() => vi.fn());
const toastSuccessMock = vi.hoisted(() => vi.fn());
const toastInfoMock = vi.hoisted(() => vi.fn());
let unmanagedSkillsData = [
  {
    directory: "shared-skill",
    name: "Shared Skill",
    description: "Imported from Claude",
    foundIn: ["claude"],
    path: "/tmp/shared-skill",
  },
];
let skillBackupsData: any[] = [];

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: [],
    isLoading: false,
  }),
  useSkillBackups: () => ({
    data: skillBackupsData,
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
  useRestoreSkillBackup: () => ({
    mutateAsync: restoreSkillBackupMock,
    isPending: false,
  }),
  useUninstallSkill: () => ({
    mutateAsync: uninstallSkillMock,
  }),
  useScanUnmanagedSkills: () => ({
    data: unmanagedSkillsData,
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

vi.mock("@/lib/api", () => ({
  settingsApi: {
    openExternal: vi.fn(),
  },
  skillsApi: {
    openZipFileDialog: openZipFileDialogMock,
  },
}));

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    unmanagedSkillsData = [
      {
        directory: "shared-skill",
        name: "Shared Skill",
        description: "Imported from Claude",
        foundIn: ["claude"],
        path: "/tmp/shared-skill",
      },
    ];
    skillBackupsData = [];
    scanUnmanagedMock.mockResolvedValue({
      data: unmanagedSkillsData,
    });
    toggleSkillAppMock.mockReset();
    uninstallSkillMock.mockReset();
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    openZipFileDialogMock.mockReset();
    openZipFileDialogMock.mockResolvedValue(null);
    toastErrorMock.mockReset();
    toastSuccessMock.mockReset();
    toastInfoMock.mockReset();
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

  it("does not enable hidden apps when importing unmanaged skills", async () => {
    unmanagedSkillsData = [
      {
        directory: "hermes-only-skill",
        name: "Hermes Only Skill",
        description: "Imported from Hermes",
        foundIn: ["hermes"],
        path: "/tmp/hermes-only-skill",
      },
    ];
    scanUnmanagedMock.mockResolvedValue({ data: unmanagedSkillsData });
    importSkillsMock.mockResolvedValue([]);

    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
        visibleApps={{
          claude: true,
          "claude-desktop": false,
          codex: true,
          gemini: true,
          opencode: true,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openImport();
    });

    await waitFor(() => {
      expect(screen.getByText("Hermes Only Skill")).toBeInTheDocument();
      expect(screen.queryByText("Hermes")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("skills.importSelected"));

    await waitFor(() => expect(importSkillsMock).toHaveBeenCalledTimes(1));
    expect(importSkillsMock).toHaveBeenCalledWith([
      {
        directory: "hermes-only-skill",
        apps: {
          claude: false,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      },
    ]);
  });

  it("uses the first visible skills app for ZIP install when current app cannot host skills", async () => {
    installFromZipMock.mockResolvedValue([]);
    openZipFileDialogMock.mockResolvedValue("/tmp/skills.zip");
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="openclaw"
        visibleApps={{
          claude: false,
          "claude-desktop": false,
          codex: true,
          gemini: true,
          opencode: true,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openInstallFromZip();
    });

    await waitFor(() => expect(installFromZipMock).toHaveBeenCalledTimes(1));
    expect(installFromZipMock).toHaveBeenCalledWith({
      filePath: "/tmp/skills.zip",
      currentApp: "codex",
    });
  });

  it("uses the first visible skills app for backup restore when current app cannot host skills", async () => {
    skillBackupsData = [
      {
        backupId: "backup-1",
        backupPath: "/tmp/backup-1",
        createdAt: 1,
        skill: {
          id: "restored-skill",
          name: "Restored Skill",
          directory: "restored-skill",
          apps: {
            claude: true,
            "claude-desktop": false,
            codex: false,
            gemini: false,
            opencode: false,
            openclaw: false,
            hermes: false,
          },
          installedAt: 1,
          updatedAt: 0,
        },
      },
    ];
    restoreSkillBackupMock.mockResolvedValue({
      name: "Restored Skill",
    });
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
        visibleApps={{
          claude: false,
          "claude-desktop": false,
          codex: true,
          gemini: true,
          opencode: true,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openRestoreFromBackup();
    });

    fireEvent.click(
      await screen.findByText("skills.restoreFromBackup.restore"),
    );

    await waitFor(() =>
      expect(restoreSkillBackupMock).toHaveBeenCalledTimes(1),
    );
    expect(restoreSkillBackupMock).toHaveBeenCalledWith({
      backupId: "backup-1",
      currentApp: "codex",
    });
  });

  it("does not install from ZIP when only OpenClaw is visible", async () => {
    openZipFileDialogMock.mockResolvedValue("/tmp/skills.zip");
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp={null}
        visibleApps={{
          claude: false,
          "claude-desktop": false,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openInstallFromZip();
    });

    expect(openZipFileDialogMock).not.toHaveBeenCalled();
    expect(installFromZipMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith("skills.noVisibleTargetApp");
  });

  it("does not restore a backup when only OpenClaw is visible", async () => {
    skillBackupsData = [
      {
        backupId: "backup-1",
        backupPath: "/tmp/backup-1",
        createdAt: 1,
        skill: {
          id: "restored-skill",
          name: "Restored Skill",
          directory: "restored-skill",
          apps: {
            claude: true,
            "claude-desktop": false,
            codex: false,
            gemini: false,
            opencode: false,
            openclaw: false,
            hermes: false,
          },
          installedAt: 1,
          updatedAt: 0,
        },
      },
    ];
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp={null}
        visibleApps={{
          claude: false,
          "claude-desktop": false,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openRestoreFromBackup();
    });

    fireEvent.click(
      await screen.findByText("skills.restoreFromBackup.restore"),
    );

    expect(restoreSkillBackupMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith("skills.noVisibleTargetApp");
  });
});
