import { createRef } from "react";
import { render, screen, waitFor, act } from "@testing-library/react";
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
const checkUpdatesMock = vi.fn();
const updateSkillMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastInfoMock = vi.fn();
let updateProgressHandler:
  | ((progress: {
      phase: "connecting" | "checking" | "downloading" | "scanning";
      current: number;
      total: number;
      repo: string;
    }) => void)
  | undefined;
let installedSkillsMock: Array<{
  id: string;
  name: string;
  directory: string;
  repoOwner?: string;
  repoName?: string;
  repoBranch?: string;
  apps: Record<string, boolean>;
  installedAt: number;
  updatedAt: number;
}> = [];
let isCheckingUpdatesMock = false;
let updateCheckResultMock: {
  updates: Array<{
    skillId: string;
    skillName: string;
    status: string;
  }>;
  failures: unknown[];
} = { updates: [], failures: [] };

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
  },
}));

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: (
    _eventName: string,
    handler: typeof updateProgressHandler,
  ) => {
    updateProgressHandler = handler;
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: installedSkillsMock,
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
  useRestoreSkillBackup: () => ({
    mutateAsync: restoreSkillBackupMock,
    isPending: false,
  }),
  useUninstallSkill: () => ({
    mutateAsync: uninstallSkillMock,
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
    data: updateCheckResultMock,
    refetch: checkUpdatesMock,
    forceRefetch: checkUpdatesMock,
    isFetching: isCheckingUpdatesMock,
  }),
  useUpdateSkill: () => ({
    mutateAsync: updateSkillMock,
    isPending: false,
  }),
}));

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
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
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    restoreSkillBackupMock.mockReset();
    checkUpdatesMock.mockReset();
    updateSkillMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
    installedSkillsMock = [];
    isCheckingUpdatesMock = false;
    updateCheckResultMock = { updates: [], failures: [] };
    updateProgressHandler = undefined;
    checkUpdatesMock.mockResolvedValue({ data: { updates: [], failures: [] } });
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

  it("renders repository failures as separate lines", async () => {
    installedSkillsMock = [
      {
        id: "one",
        name: "One",
        directory: "one",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    checkUpdatesMock.mockResolvedValue({
      data: {
        updates: [],
        failures: [
          {
            owner: "owner",
            name: "repo",
            branch: "main",
            error: '{"code":"DOWNLOAD_TIMEOUT","context":{}}',
          },
          {
            owner: "other",
            name: "repo",
            branch: "main",
            error: '{"code":"DOWNLOAD_TIMEOUT","context":{}}',
          },
        ],
      },
    });
    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.checkUpdates();
    });

    const options = toastErrorMock.mock.calls[0][1] as {
      description: React.ReactNode;
    };
    const { container } = render(<>{options.description}</>);
    expect(container.querySelectorAll("[data-repo-failure]")).toHaveLength(2);
  });

  it("does not report all skills up to date when update checks have repository failures", async () => {
    checkUpdatesMock.mockResolvedValue({
      data: {
        updates: [],
        failures: [
          {
            owner: "JimLiu",
            name: "baoyu-skills",
            branch: "main",
            error: "download timeout",
          },
        ],
      },
    });

    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.checkUpdates();
    });

    expect(toastSuccessMock).not.toHaveBeenCalledWith(
      "skills.noUpdates",
      expect.anything(),
    );
    expect(toastErrorMock).toHaveBeenCalledWith(
      "skills.updateCheckIncomplete",
      expect.objectContaining({ duration: Infinity }),
    );
    const options = toastErrorMock.mock.calls[0][1] as {
      description: React.ReactNode;
    };
    const { container } = render(<>{options.description}</>);
    expect(container).toHaveTextContent("JimLiu/baoyu-skills");
  });

  it("shows repository connection progress immediately before SHA requests finish", () => {
    installedSkillsMock = [
      {
        id: "skill-a",
        name: "Skill A",
        directory: "skill-a",
        repoOwner: "anthropics",
        repoName: "skills",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
      {
        id: "skill-b",
        name: "Skill B",
        directory: "skill-b",
        repoOwner: "JimLiu",
        repoName: "baoyu-skills",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    isCheckingUpdatesMock = true;

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.getByText("skills.connectingRepositoriesProgress"),
    ).toBeInTheDocument();
  });

  it("shows the latest meaningful repository phase", () => {
    installedSkillsMock = [
      {
        id: "skill-a",
        name: "Skill A",
        directory: "skill-a",
        repoOwner: "anthropics",
        repoName: "skills",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    isCheckingUpdatesMock = true;

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    act(() => {
      updateProgressHandler?.({
        phase: "downloading",
        current: 1,
        total: 1,
        repo: "anthropics/skills",
      });
    });
    const progressButton = screen.getByRole("button", {
      name: /skills\.downloadingRepository.*anthropics\/skills/,
    });
    expect(progressButton).toBeInTheDocument();

    expect(progressButton).toHaveTextContent("anthropics/skills");
  });

  it("reports successful updates together with repositories that did not finish", async () => {
    installedSkillsMock = [
      {
        id: "frontend-design",
        name: "Frontend Design",
        directory: "frontend-design",
        repoOwner: "anthropics",
        repoName: "skills",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    checkUpdatesMock.mockResolvedValue({
      data: {
        updates: [
          {
            id: "frontend-design",
            name: "Frontend Design",
            status: "updateAvailable",
            remoteHash: "remote",
          },
        ],
        failures: [
          {
            owner: "JimLiu",
            name: "baoyu-skills",
            branch: "main",
            error: "download timeout",
          },
        ],
      },
    });

    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.checkUpdates();
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      "skills.updateCheckPartialWithUpdates",
      expect.objectContaining({ duration: Infinity }),
    );
    expect(toastInfoMock).not.toHaveBeenCalled();
  });

  it("does not call a local modification a remote update", async () => {
    checkUpdatesMock.mockResolvedValue({
      data: {
        updates: [
          {
            id: "local-skill",
            name: "Local Skill",
            status: "localModified",
            remoteHash: "same",
          },
        ],
        failures: [],
      },
    });

    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.checkUpdates();
    });

    expect(toastInfoMock).toHaveBeenCalledWith(
      "skills.updateCheckNeedsAttention",
      expect.objectContaining({ closeButton: true }),
    );
    expect(toastInfoMock).not.toHaveBeenCalledWith(
      "skills.updatesFound",
      expect.anything(),
    );
  });

  it("allows a legacy unverified skill to be updated with confirmation", () => {
    installedSkillsMock = [
      {
        id: "legacy-skill",
        name: "Legacy Skill",
        directory: "legacy-skill",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];
    updateCheckResultMock = {
      updates: [
        {
          id: "legacy-skill",
          name: "Legacy Skill",
          status: "notChecked",
          remoteHash: "remote",
        },
      ] as never,
      failures: [],
    };

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(screen.getByTitle("skills.update")).toBeInTheDocument();
  });

  it("disables update checks when installed skills have no repository source", () => {
    installedSkillsMock = [
      {
        id: "local-skill",
        name: "Local Skill",
        directory: "local-skill",
        apps: {},
        installedAt: 0,
        updatedAt: 0,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.getByRole("button", { name: "skills.checkUpdates" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "skills.checkUpdates" }),
    ).toHaveAttribute("title", "skills.noCheckableUpdates");
  });

  it("visually hides update all while preserving its layout space", () => {
    updateCheckResultMock = {
      updates: [
        {
          skillId: "frontend-design",
          skillName: "frontend-design",
          status: "updateAvailable",
        },
      ],
      failures: [],
    };
    isCheckingUpdatesMock = true;

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.getByRole("button", { name: "skills.updateAll" }),
    ).toHaveClass("invisible");
  });
});
