import { createRef } from "react";
import { render, screen, waitFor, act, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";
import type {
  InstalledSkill,
  SkillBackupEntry,
  SkillUpdateInfo,
} from "@/lib/api/skills";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
const bulkToggleSkillAppMock = vi.fn();
const checkUpdatesMock = vi.fn();
const updateSkillMock = vi.fn();
const refetchSkillBackupsMock = vi.fn();
const { toastErrorMock, toastSuccessMock, toastWarningMock } = vi.hoisted(
  () => ({
    toastErrorMock: vi.fn(),
    toastSuccessMock: vi.fn(),
    toastWarningMock: vi.fn(),
  }),
);
let installedSkillsMock: InstalledSkill[] = [];
let skillBackupsMock: SkillBackupEntry[] = [];
let skillUpdatesMock: SkillUpdateInfo[] = [];
let checkUpdatesFetching = false;
let toggleSkillAppPending = false;
let toggleSkillAppVariables:
  | { id: string; app: "claude"; enabled: boolean }
  | undefined;
let bulkToggleSkillAppPending = false;
let bulkToggleSkillAppVariables:
  | { ids: string[]; app: "claude"; enabled: boolean }
  | undefined;

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    error: toastErrorMock,
    warning: toastWarningMock,
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: installedSkillsMock,
    isLoading: false,
  }),
  useSkillBackups: () => ({
    data: skillBackupsMock,
    refetch: refetchSkillBackupsMock,
    isFetching: false,
  }),
  useDeleteSkillBackup: () => ({
    mutateAsync: deleteSkillBackupMock,
    isPending: false,
  }),
  useToggleSkillApp: () => ({
    mutateAsync: toggleSkillAppMock,
    isPending: toggleSkillAppPending,
    variables: toggleSkillAppVariables,
  }),
  useBulkToggleSkillApp: () => ({
    mutateAsync: bulkToggleSkillAppMock,
    isPending: bulkToggleSkillAppPending,
    variables: bulkToggleSkillAppVariables,
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
        description: "Imported from Grok Build",
        foundIn: ["grokbuild"],
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
    data: skillUpdatesMock,
    refetch: checkUpdatesMock,
    isFetching: checkUpdatesFetching,
  }),
  useUpdateSkill: () => ({
    mutateAsync: updateSkillMock,
    isPending: false,
  }),
}));

type InstalledSkillOverrides = Omit<Partial<InstalledSkill>, "apps"> & {
  apps?: Partial<InstalledSkill["apps"]>;
};

const makeInstalledSkill = (
  overrides: InstalledSkillOverrides = {},
): InstalledSkill => {
  const defaultApps: InstalledSkill["apps"] = {
    claude: false,
    codex: false,
    gemini: false,
    grokbuild: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    pi: false,
  };
  const { apps, ...skillOverrides } = overrides;

  return {
    id: "owner/repo:alpha-skill",
    name: "Alpha Skill",
    description: "Alpha description",
    directory: "alpha-skill",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    apps: { ...defaultApps, ...apps },
    installedAt: 1,
    updatedAt: 1,
    ...skillOverrides,
  };
};

const renderPanel = () =>
  render(<UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />);

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    installedSkillsMock = [];
    skillBackupsMock = [];
    skillUpdatesMock = [];
    checkUpdatesFetching = false;
    toggleSkillAppPending = false;
    toggleSkillAppVariables = undefined;
    bulkToggleSkillAppPending = false;
    bulkToggleSkillAppVariables = undefined;
    scanUnmanagedMock.mockReset();
    scanUnmanagedMock.mockResolvedValue({
      data: [
        {
          directory: "shared-skill",
          name: "Shared Skill",
          description: "Imported from Grok Build",
          foundIn: ["grokbuild"],
          path: "/tmp/shared-skill",
        },
      ],
    });
    toggleSkillAppMock.mockReset();
    toggleSkillAppMock.mockResolvedValue(true);
    bulkToggleSkillAppMock.mockReset();
    bulkToggleSkillAppMock.mockResolvedValue({ succeeded: [], failed: [] });
    toastErrorMock.mockReset();
    toastSuccessMock.mockReset();
    toastWarningMock.mockReset();
    uninstallSkillMock.mockReset();
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    refetchSkillBackupsMock.mockReset();
    refetchSkillBackupsMock.mockResolvedValue({ data: skillBackupsMock });
    restoreSkillBackupMock.mockReset();
    checkUpdatesMock.mockReset();
    checkUpdatesMock.mockResolvedValue({ data: [] });
    updateSkillMock.mockReset();
    updateSkillMock.mockImplementation(async (id: string) =>
      makeInstalledSkill({ id }),
    );
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

    await act(async () => {
      screen.getByText("skills.importSelected").click();
    });

    await waitFor(() => {
      expect(importSkillsMock).toHaveBeenCalledWith([
        {
          directory: "shared-skill",
          apps: expect.objectContaining({ grokbuild: true }),
        },
      ]);
    });
  });

  it("passes only the installed Skill ID to uninstall", async () => {
    installedSkillsMock = [
      makeInstalledSkill({
        id: "owner/repo:skill-id",
        directory: "nested/skill-directory",
        repoOwner: "owner",
        repoName: "repo",
      }),
    ];
    uninstallSkillMock.mockResolvedValueOnce({ backupPath: undefined });
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByTitle("skills.uninstall"));
    await user.click(
      screen.getByRole("button", {
        name: "common.confirm",
      }),
    );

    await waitFor(() => {
      expect(uninstallSkillMock).toHaveBeenCalledWith("owner/repo:skill-id");
    });
  });

  it("warns when uninstall preserves an unverified Pi directory", async () => {
    installedSkillsMock = [makeInstalledSkill({ name: "Pi Skill" })];
    uninstallSkillMock.mockResolvedValueOnce({
      backupPath: "/tmp/backup",
      preservedPiPath: "/tmp/pi/skills/pi-skill",
    });
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByTitle("skills.uninstall"));
    await user.click(screen.getByRole("button", { name: "common.confirm" }));

    await waitFor(() => {
      expect(toastWarningMock).toHaveBeenCalledWith("skills.uninstallSuccess", {
        description: "skills.uninstallPiPreserved",
        closeButton: true,
      });
    });
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("warns when the Pi Skills directory could not be resolved", async () => {
    installedSkillsMock = [makeInstalledSkill({ name: "Pi Skill" })];
    uninstallSkillMock.mockResolvedValueOnce({ piCleanupIncomplete: true });
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByTitle("skills.uninstall"));
    await user.click(screen.getByRole("button", { name: "common.confirm" }));

    await waitFor(() => {
      expect(toastWarningMock).toHaveBeenCalledWith("skills.uninstallSuccess", {
        description: "skills.uninstallPiCleanupIncomplete",
        closeButton: true,
      });
    });
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it.each([
    ["name", "searchable name"],
    ["id", "opaque-id-token"],
    ["description", "descriptive-token"],
    ["directory", "directory-token"],
    ["repo owner", "owner-token"],
    ["repo name", "repository-token"],
  ])("filters installed Skills by %s", async (_field, query) => {
    installedSkillsMock = [
      makeInstalledSkill({
        id: "opaque-id-token",
        name: "Searchable Name",
        description: "Contains descriptive-token",
        directory: "nested/directory-token",
        repoOwner: "owner-token",
        repoName: "repository-token",
      }),
      makeInstalledSkill({
        id: "unrelated-id",
        name: "Unrelated Skill",
        description: "Nothing to match",
        directory: "other-directory",
        repoOwner: "another-owner",
        repoName: "another-repo",
      }),
    ];
    renderPanel();

    const user = userEvent.setup();
    await user.type(
      screen.getByRole("textbox", {
        name: "skills.installedSearchAriaLabel",
      }),
      `  ${query.toUpperCase()}  `,
    );

    expect(screen.getByText("Searchable Name")).toBeInTheDocument();
    expect(screen.queryByText("Unrelated Skill")).not.toBeInTheDocument();
  });

  it("distinguishes an empty list from an installed-Skill search miss", async () => {
    const { rerender } = renderPanel();

    expect(screen.getByText("skills.noInstalled")).toBeInTheDocument();
    expect(
      screen.queryByText("skills.noInstalledSearchResults"),
    ).not.toBeInTheDocument();

    installedSkillsMock = [makeInstalledSkill()];
    rerender(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );
    const user = userEvent.setup();
    await user.type(
      screen.getByRole("textbox", {
        name: "skills.installedSearchAriaLabel",
      }),
      "missing",
    );

    expect(
      screen.getByText("skills.noInstalledSearchResults"),
    ).toBeInTheDocument();
    expect(screen.queryByText("skills.noInstalled")).not.toBeInTheDocument();
  });

  it("keeps the search control outside the visible scroll viewport", () => {
    installedSkillsMock = [makeInstalledSkill()];
    const { container } = renderPanel();

    const searchInput = screen.getByRole("textbox", {
      name: "skills.installedSearchAriaLabel",
    });
    const viewport = container.querySelector(
      "[data-radix-scroll-area-viewport]",
    );

    expect(viewport).not.toBeNull();
    expect(viewport).not.toContainElement(searchInput);
  });

  it("enables only disabled Skills from the full list when the app state is mixed", async () => {
    installedSkillsMock = [
      makeInstalledSkill({
        id: "enabled-id",
        name: "Visible Skill",
        apps: { claude: true },
      }),
      makeInstalledSkill({ id: "disabled-id-1", name: "Hidden Skill One" }),
      makeInstalledSkill({ id: "disabled-id-2", name: "Hidden Skill Two" }),
    ];
    bulkToggleSkillAppMock.mockResolvedValue({
      succeeded: ["disabled-id-1", "disabled-id-2"],
      failed: [],
    });
    renderPanel();

    const user = userEvent.setup();
    await user.type(
      screen.getByRole("textbox", {
        name: "skills.installedSearchAriaLabel",
      }),
      "Visible Skill",
    );
    await user.click(screen.getByText("Claude:").closest("button")!);

    await waitFor(() => {
      expect(bulkToggleSkillAppMock).toHaveBeenCalledWith({
        ids: ["disabled-id-1", "disabled-id-2"],
        app: "claude",
        enabled: true,
      });
    });
  });

  it("enables all Skills when none are enabled for an app", async () => {
    installedSkillsMock = [
      makeInstalledSkill({ id: "first-id" }),
      makeInstalledSkill({ id: "second-id" }),
    ];
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByText("Claude:").closest("button")!);

    await waitFor(() => {
      expect(bulkToggleSkillAppMock).toHaveBeenCalledWith({
        ids: ["first-id", "second-id"],
        app: "claude",
        enabled: true,
      });
    });
  });

  it("disables all Skills when every Skill is enabled for an app", async () => {
    installedSkillsMock = [
      makeInstalledSkill({ id: "first-id", apps: { claude: true } }),
      makeInstalledSkill({ id: "second-id", apps: { claude: true } }),
    ];
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByText("Claude:").closest("button")!);

    await waitFor(() => {
      expect(bulkToggleSkillAppMock).toHaveBeenCalledWith({
        ids: ["first-id", "second-id"],
        app: "claude",
        enabled: false,
      });
    });
  });

  it("reports partial bulk-toggle failures", async () => {
    installedSkillsMock = [
      makeInstalledSkill({ id: "first-id" }),
      makeInstalledSkill({ id: "second-id" }),
    ];
    bulkToggleSkillAppMock.mockResolvedValue({
      succeeded: ["first-id"],
      failed: [{ item: "second-id", error: new Error("permission denied") }],
    });
    renderPanel();

    const user = userEvent.setup();
    await user.click(screen.getByText("Claude:").closest("button")!);

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith("common.bulkToggleFailed", {
        description: "Error: permission denied",
      });
    });
  });

  it.each(["single", "bulk"] as const)(
    "disables row app toggles while a %s toggle is pending",
    async (pendingKind) => {
      installedSkillsMock = [makeInstalledSkill()];
      if (pendingKind === "single") {
        toggleSkillAppPending = true;
        toggleSkillAppVariables = {
          id: "owner/repo:alpha-skill",
          app: "claude",
          enabled: true,
        };
      } else {
        bulkToggleSkillAppPending = true;
        bulkToggleSkillAppVariables = {
          ids: ["owner/repo:alpha-skill"],
          app: "claude",
          enabled: true,
        };
      }
      renderPanel();

      const row = screen.getByText("Alpha Skill").closest(".group");
      const appToggleButtons = Array.from(
        row!.querySelectorAll<HTMLButtonElement>("button"),
      ).slice(0, 7);

      expect(appToggleButtons).toHaveLength(7);
      appToggleButtons.forEach((button) => expect(button).toBeDisabled());
      expect(screen.getByTitle("skills.uninstall")).toBeDisabled();
      await userEvent.setup().click(appToggleButtons[0]);
      expect(toggleSkillAppMock).not.toHaveBeenCalled();
    },
  );

  it("reports check-update availability and clears it on unmount", async () => {
    installedSkillsMock = [makeInstalledSkill()];
    const onCheckUpdatesStateChange = vi.fn();

    const { unmount } = render(
      <UnifiedSkillsPanel
        onOpenDiscovery={() => {}}
        currentApp="claude"
        onCheckUpdatesStateChange={onCheckUpdatesStateChange}
      />,
    );

    await waitFor(() => {
      expect(onCheckUpdatesStateChange).toHaveBeenLastCalledWith({
        isChecking: false,
        hasSkills: true,
      });
    });
    expect(screen.queryByText("skills.checkUpdates")).not.toBeInTheDocument();

    unmount();
    expect(onCheckUpdatesStateChange).toHaveBeenLastCalledWith({
      isChecking: false,
      hasSkills: false,
    });
  });

  it("ignores rapid duplicate check-update ref calls", async () => {
    installedSkillsMock = [makeInstalledSkill()];
    let resolveCheck!: (value: { data: never[] }) => void;
    checkUpdatesMock.mockReturnValue(
      new Promise((resolve) => {
        resolveCheck = resolve;
      }),
    );
    const ref = createRef<UnifiedSkillsPanelHandle>();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    act(() => {
      ref.current?.checkUpdates();
      ref.current?.checkUpdates();
    });
    expect(checkUpdatesMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveCheck({ data: [] });
      await Promise.resolve();
    });
  });

  it("blocks actions but not navigation while checking updates", async () => {
    installedSkillsMock = [makeInstalledSkill()];
    checkUpdatesFetching = true;
    const ref = createRef<UnifiedSkillsPanelHandle>();
    const onInteractionBlockedChange = vi.fn();
    const onNavigationBlockedChange = vi.fn();

    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
        onInteractionBlockedChange={onInteractionBlockedChange}
        onNavigationBlockedChange={onNavigationBlockedChange}
      />,
    );

    await waitFor(() => {
      expect(onInteractionBlockedChange).toHaveBeenLastCalledWith(true);
      expect(onNavigationBlockedChange).toHaveBeenLastCalledWith(false);
    });
    expect(screen.getByText("Claude:").closest("button")).toBeDisabled();
    expect(screen.getByTitle("skills.uninstall")).toBeDisabled();

    await act(async () => {
      await ref.current?.openImport();
    });
    expect(scanUnmanagedMock).not.toHaveBeenCalled();
  });

  it("closes the backup dialog and reports an explicit refresh failure", async () => {
    refetchSkillBackupsMock.mockRejectedValueOnce(new Error("refresh failed"));
    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.openRestoreFromBackup();
    });

    expect(refetchSkillBackupsMock).toHaveBeenCalledWith({
      throwOnError: true,
    });
    expect(toastErrorMock).toHaveBeenCalledWith("common.error", {
      description: "Error: refresh failed",
    });
    expect(
      screen.queryByText("skills.restoreFromBackup.title"),
    ).not.toBeInTheDocument();
  });

  it("blocks writes immediately when an update check starts", async () => {
    installedSkillsMock = [makeInstalledSkill()];
    let resolveCheck!: (value: { data: never[] }) => void;
    checkUpdatesMock.mockReturnValue(
      new Promise((resolve) => {
        resolveCheck = resolve;
      }),
    );
    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    act(() => {
      ref.current?.checkUpdates();
    });
    expect(checkUpdatesMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      await ref.current?.openImport();
    });
    await userEvent.setup().click(screen.getByTitle("skills.uninstall"));
    await userEvent
      .setup()
      .click(screen.getByText("Claude:").closest("button")!);

    expect(scanUnmanagedMock).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(bulkToggleSkillAppMock).not.toHaveBeenCalled();

    await act(async () => {
      resolveCheck({ data: [] });
      await Promise.resolve();
    });
  });

  it("ignores stale update entries for uninstalled Skills", async () => {
    installedSkillsMock = [makeInstalledSkill({ id: "installed-id" })];
    skillUpdatesMock = [
      { id: "removed-id", name: "Removed Skill", remoteHash: "removed" },
      { id: "installed-id", name: "Alpha Skill", remoteHash: "current" },
    ];
    renderPanel();

    expect(screen.getAllByText("skills.updateAvailable")).toHaveLength(1);
    await userEvent.setup().click(
      screen.getByRole("button", {
        name: "skills.updateAll",
      }),
    );

    await waitFor(() => {
      expect(updateSkillMock).toHaveBeenCalledTimes(1);
      expect(updateSkillMock).toHaveBeenCalledWith("installed-id");
    });
  });

  it("waits for an explicit backup refresh before reporting deletion failure", async () => {
    skillBackupsMock = [
      {
        backupId: "backup-1",
        backupPath: "C:\\backups\\backup-1",
        createdAt: 1,
        skill: makeInstalledSkill({ name: "Backup Skill" }),
      },
    ];
    deleteSkillBackupMock.mockRejectedValueOnce(undefined);
    let releaseRefresh: (() => void) | undefined;
    const refreshPending = new Promise((resolve) => {
      releaseRefresh = () => resolve({ data: [] });
    });
    refetchSkillBackupsMock
      .mockResolvedValueOnce({ data: skillBackupsMock })
      .mockReturnValueOnce(refreshPending);
    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.openRestoreFromBackup();
    });
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );
    const confirmDialog = screen
      .getByText("skills.restoreFromBackup.deleteConfirmTitle")
      .closest<HTMLElement>('[role="dialog"]');
    expect(confirmDialog).not.toBeNull();
    await user.click(
      within(confirmDialog!).getByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );

    await waitFor(() => {
      expect(deleteSkillBackupMock).toHaveBeenCalledWith("backup-1");
      expect(refetchSkillBackupsMock).toHaveBeenCalledTimes(2);
    });
    expect(toastErrorMock).not.toHaveBeenCalled();

    releaseRefresh?.();
    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledTimes(1);
      expect(toastErrorMock).toHaveBeenCalledWith(
        "skills.restoreFromBackup.deleteFailed",
        { description: "undefined" },
      );
    });
    expect(toastSuccessMock).not.toHaveBeenCalled();
    expect(
      screen.queryByText("skills.restoreFromBackup.deleteConfirmTitle"),
    ).not.toBeInTheDocument();
  });

  it("does not report a completed deletion as failed when refresh rejects", async () => {
    const consoleErrorSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    skillBackupsMock = [
      {
        backupId: "backup-1",
        backupPath: "C:\\backups\\backup-1",
        createdAt: 1,
        skill: makeInstalledSkill({ name: "Backup Skill" }),
      },
    ];
    deleteSkillBackupMock.mockResolvedValueOnce(true);
    refetchSkillBackupsMock
      .mockResolvedValueOnce({ data: skillBackupsMock })
      .mockRejectedValueOnce(new Error("refresh failed"));
    const ref = createRef<UnifiedSkillsPanelHandle>();
    render(
      <UnifiedSkillsPanel
        ref={ref}
        onOpenDiscovery={() => {}}
        currentApp="claude"
      />,
    );

    await act(async () => {
      await ref.current?.openRestoreFromBackup();
    });
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );
    const confirmDialog = screen
      .getByText("skills.restoreFromBackup.deleteConfirmTitle")
      .closest<HTMLElement>('[role="dialog"]');
    expect(confirmDialog).not.toBeNull();
    await user.click(
      within(confirmDialog!).getByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );

    await waitFor(() => {
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "skills.restoreFromBackup.deleteSuccess",
        { closeButton: true },
      );
    });
    expect(refetchSkillBackupsMock).toHaveBeenCalledTimes(2);
    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(consoleErrorSpy).toHaveBeenCalledWith(
      "Failed to refresh Skill backups after deletion:",
      expect.any(Error),
    );
    consoleErrorSpy.mockRestore();
  });

  it("renders and toggles the Pi app state like the other apps", async () => {
    installedSkillsMock = [
      makeInstalledSkill({
        id: "skill-1",
        name: "Pi Skill",
        directory: "pi-skill",
        apps: { pi: true },
      }),
    ];

    render(<UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="pi" />);

    const piToggle = screen.getByRole("button", { name: "Pi" });
    expect(piToggle).toHaveAttribute("aria-pressed", "true");

    await userEvent.setup().click(piToggle);

    await waitFor(() => {
      expect(toggleSkillAppMock).toHaveBeenCalledWith({
        id: "skill-1",
        app: "pi",
        enabled: false,
      });
    });
  });

  it("renders an inactive Pi state like the other apps", () => {
    installedSkillsMock = [
      makeInstalledSkill({
        id: "skill-1",
        name: "Claude Skill",
        directory: "claude-skill",
        apps: { claude: true, pi: false },
      }),
    ];

    render(<UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="pi" />);

    expect(screen.getByRole("button", { name: "Pi" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("does not add an inactive Pi toggle outside the Pi context", () => {
    installedSkillsMock = [
      makeInstalledSkill({
        name: "Claude Skill",
        apps: { claude: true, pi: false },
      }),
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.queryByRole("button", { name: "Pi" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Claude" })).toBeInTheDocument();
  });
});
