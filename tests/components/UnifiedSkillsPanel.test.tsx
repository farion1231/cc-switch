import { createRef } from "react";
import { render, screen, waitFor, act, within } from "@testing-library/react";
import { fireEvent } from "@testing-library/react";
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
const bulkUninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
const bulkToggleSkillAppMock = vi.fn();
const checkUpdatesMock = vi.fn();
const updateSkillMock = vi.fn();
const refetchSkillBackupsMock = vi.fn();
const { toastErrorMock, toastSuccessMock, toastWarningMock, toastInfoMock } =
  vi.hoisted(() => ({
    toastErrorMock: vi.fn(),
    toastSuccessMock: vi.fn(),
    toastWarningMock: vi.fn(),
    toastInfoMock: vi.fn(),
  }));
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
    info: toastInfoMock,
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
  useBulkUninstallSkill: () => ({
    mutateAsync: bulkUninstallSkillMock,
    isPending: false,
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
    toastInfoMock.mockReset();
    uninstallSkillMock.mockReset();
    bulkUninstallSkillMock.mockReset();
    bulkUninstallSkillMock.mockResolvedValue({ succeeded: [], failed: [] });
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
      expect(screen.getByText("Shared Skill")).toBeInTheDocument();
      expect(screen.getByText("/tmp/shared-skill")).toBeInTheDocument();
    });
    // The overlay is hand-rolled (no role="dialog"); the heading carries the key.
    expect(screen.getByRole("heading", { name: "skills.import" })).toBeTruthy();

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

  describe("management filters and bulk selection", () => {
    const bulkCountText = () => {
      const toolbar = screen.getByRole("toolbar");
      return toolbar.querySelector("span")?.textContent?.trim() ?? "";
    };

    /**
     * Row checkboxes only. AppCountBar renders its app counters with
     * role="checkbox" too, so a bare getAllByRole("checkbox") picks those up first.
     */
    const rowCheckboxes = () =>
      Array.from(
        document.querySelectorAll<HTMLInputElement>(
          'input[type="checkbox"][aria-label="skills.manage.selectSkill"]',
        ),
      );

    const threeSkills = () => {
      installedSkillsMock = [
        makeInstalledSkill({
          id: "repo/enabled-claude",
          name: "Repo Claude Skill",
          description: "from a repo",
          repoOwner: "owner",
          repoName: "one",
          apps: { claude: true },
        }),
        makeInstalledSkill({
          id: "repo/local-only",
          name: "Local Only Skill",
          description: "imported manually",
          repoOwner: undefined,
          repoName: undefined,
        }),
        makeInstalledSkill({
          id: "repo/updated",
          name: "Updatable Skill",
          description: "has a pending update",
          repoOwner: "owner",
          repoName: "two",
          apps: { claude: true, codex: true },
        }),
      ];
      skillUpdatesMock = [
        { id: "repo/updated", name: "Updatable Skill", remoteHash: "next" },
      ];
    };

    it("narrows the list with app chips and switches any/all matching", () => {
      threeSkills();
      renderPanel();

      // Chip text is the raw app id: the test i18n catalog is empty, so
      // t("skills.apps.codex") falls back to the key's last segment.
      const chipButton = (label: string) =>
        screen
          .getAllByRole("button")
          .find((button) => button.textContent?.includes(label))!;

      // "any" (default): Claude OR Codex → both app-enabled skills, not local-only.
      fireEvent.click(chipButton("claude"));
      fireEvent.click(chipButton("codex"));
      expect(screen.getByText("Repo Claude Skill")).toBeInTheDocument();
      expect(screen.getByText("Updatable Skill")).toBeInTheDocument();
      expect(screen.queryByText("Local Only Skill")).not.toBeInTheDocument();

      // Selecting two apps reveals the mode switch; "all" keeps only the
      // skill enabled for both.
      fireEvent.click(screen.getByText("skills.manage.filterAppModeAll"));
      expect(screen.queryByText("Repo Claude Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Updatable Skill")).toBeInTheDocument();
      expect(screen.queryByText("Local Only Skill")).not.toBeInTheDocument();

      fireEvent.click(screen.getByText("skills.manage.filterAppModeAny"));
      expect(screen.getByText("Repo Claude Skill")).toBeInTheDocument();
    });

    it("combines source and update filters with the search text", () => {
      threeSkills();
      renderPanel();

      const chipButton = (label: string) =>
        screen
          .getAllByRole("button")
          .find((button) => button.textContent?.includes(label))!;

      fireEvent.click(chipButton("skills.manage.sourceLocal"));
      expect(screen.getByText("Local Only Skill")).toBeInTheDocument();
      expect(screen.queryByText("Repo Claude Skill")).not.toBeInTheDocument();

      fireEvent.click(chipButton("skills.manage.updatedAvailable"));
      expect(
        screen.getByText("skills.noInstalledSearchResults"),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByText("skills.manage.sourceRepo"));
      expect(screen.getByText("Updatable Skill")).toBeInTheDocument();
      expect(screen.queryByText("Local Only Skill")).not.toBeInTheDocument();

      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Updatable" } },
      );
      expect(screen.getByText("Updatable Skill")).toBeInTheDocument();
      // The real key with its interpolation stripped by the empty test catalog.
      expect(
        screen.getByText("skills.manage.visibleCount"),
      ).toBeInTheDocument();
    });

    it("uninstalls exactly the selected rows through the bulk action", async () => {
      threeSkills();
      bulkUninstallSkillMock.mockResolvedValue({
        succeeded: [
          { item: "repo/enabled-claude", result: {} },
          { item: "repo/local-only", result: {} },
        ],
        failed: [],
      });
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(rowCheckboxes()[1]);

      // The bulk bar renders a plain "selected / total" figure across three
      // text nodes, so compare the normalized span text.
      await waitFor(() => expect(bulkCountText()).toBe("2 / 3"));

      await user.click(screen.getByText("skills.manage.bulkUninstall"));
      // The confirm dialog carries the bulk label as its confirm text.
      await user.click(
        screen.getByRole("button", { name: "skills.manage.bulkUninstall" }),
      );

      await waitFor(() => {
        expect(bulkUninstallSkillMock).toHaveBeenCalledWith([
          "repo/enabled-claude",
          "repo/local-only",
        ]);
      });
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "skills.manage.bulkUninstallSuccess",
        { closeButton: true },
      );
    });

    it("keeps earlier ticks when the filter changes and comes back", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(rowCheckboxes()[1]);
      await waitFor(() => expect(bulkCountText()).toBe("2 / 3"));

      // Narrowing the filter hides the first tick but must not drop it: the
      // numerator follows the screen ("1 / 1") and the hidden tick is
      // disclosed separately instead of silently removed.
      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Local Only" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));
      expect(
        screen.getByText("skills.manage.hiddenSelected"),
      ).toBeInTheDocument();

      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("2 / 3"));
      // Both ticks survived the round trip.
      expect(rowCheckboxes()[0]).toBeChecked();
      expect(rowCheckboxes()[1]).toBeChecked();
      expect(
        screen.queryByText("skills.manage.hiddenSelected"),
      ).not.toBeInTheDocument();
    });

    it("does not disclose hidden selections when every selected row is on screen", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[1]);
      await waitFor(() => expect(bulkCountText()).toBe("1 / 3"));

      // Filtering down to the selected row itself: numerator and denominator
      // agree, nothing is hidden, so no disclosure may appear.
      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Local Only" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));
      expect(
        screen.queryByText("skills.manage.hiddenSelected"),
      ).not.toBeInTheDocument();
    });

    it("switches clear and uninstall labels to scope-declaring keys when ticks are hidden", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(rowCheckboxes()[1]);

      // Nothing hidden: the plain labels describe the screen set, and that is
      // exactly what the buttons act on.
      await waitFor(() => expect(bulkCountText()).toBe("2 / 3"));
      expect(
        screen.getByRole("button", { name: "skills.manage.clearSelection" }),
      ).toBeInTheDocument();
      expect(
        screen.getByText("skills.manage.bulkUninstall"),
      ).toBeInTheDocument();

      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Local Only" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));

      // One tick hidden: both controls must self-declare that they act on the
      // whole selection, not just the "1 / 1" on screen.
      expect(
        screen.getByRole("button", {
          name: "skills.manage.clearSelectionAll",
        }),
      ).toBeInTheDocument();
      expect(
        screen.getByText("skills.manage.bulkUninstallAll"),
      ).toBeInTheDocument();
    });

    it("names the skills hidden by the filter in the bulk uninstall confirm", async () => {
      threeSkills();
      bulkUninstallSkillMock.mockResolvedValue({ succeeded: [], failed: [] });
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(rowCheckboxes()[1]);
      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Local Only" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));

      // With a hidden tick the destructive button must self-declare its scope
      // instead of showing a bare number next to the screen-scoped fraction.
      await user.click(screen.getByText("skills.manage.bulkUninstallAll"));
      // The description carries the count sentence, the hidden lead-in and the
      // name list as ONE text node, so match by substring.
      const dialog = screen
        .getByText(/skills\.manage\.bulkUninstallConfirm/)
        .closest<HTMLElement>('[role="dialog"]');
      expect(dialog).not.toBeNull();
      // The count sentence stays, and the tick the search hid is named
      // verbatim instead of hiding behind the total. Only the hidden ones are
      // named — the visible tick can be cross-checked on screen.
      expect(dialog!.textContent).toContain(
        "skills.manage.bulkUninstallHidden",
      );
      expect(dialog!.textContent).toContain("Repo Claude Skill");
      expect(dialog!.textContent).not.toContain("Local Only Skill");

      await user.click(
        within(dialog!).getByRole("button", {
          name: "skills.manage.bulkUninstall",
        }),
      );
      // The uninstall still targets the whole accumulated selection, including
      // the row that is off screen right now.
      await waitFor(() => {
        expect(bulkUninstallSkillMock).toHaveBeenCalledWith([
          "repo/enabled-claude",
          "repo/local-only",
        ]);
      });
    });

    it("states how many hidden names the confirm dialog left out past the cap", async () => {
      installedSkillsMock = [
        makeInstalledSkill({ id: "shown/visible", name: "Shown Visible" }),
        ...Array.from({ length: 10 }, (_, index) =>
          makeInstalledSkill({
            id: `hidden/skill-${index}`,
            name: `Hidden Skill ${index}`,
          }),
        ),
      ];
      renderPanel();

      const user = userEvent.setup();
      // Tick all eleven rows, then hide ten of them behind the search.
      for (let index = 0; index < 11; index++) {
        await user.click(rowCheckboxes()[index]);
      }
      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Shown Visible" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));

      await user.click(screen.getByText("skills.manage.bulkUninstallAll"));
      const dialog = screen
        .getByText(/skills\.manage\.bulkUninstallConfirm/)
        .closest<HTMLElement>('[role="dialog"]');
      expect(dialog).not.toBeNull();
      // The cap stays at 8 names, but the ellipsis must not read as "that was
      // everything": the leftover count is spelled out, and the two names past
      // the cap really are absent.
      expect(dialog!.textContent).toContain("…");
      expect(dialog!.textContent).toContain("Hidden Skill 7");
      expect(dialog!.textContent).toContain(
        "skills.manage.hiddenSelectedUnlisted",
      );
      expect(dialog!.textContent).not.toContain("Hidden Skill 8");
      expect(dialog!.textContent).not.toContain("Hidden Skill 9");
    });

    it("merges the visible rows on select-all without dropping hidden ticks", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[1]);

      const chipButton = (label: string) =>
        screen
          .getAllByRole("button")
          .find((button) => button.textContent?.includes(label))!;

      // The app filter hides the ticked local skill while narrowing to the
      // two claude-enabled rows.
      fireEvent.click(chipButton("claude"));
      expect(screen.queryByText("Local Only Skill")).not.toBeInTheDocument();
      await user.click(rowCheckboxes()[1]);
      await waitFor(() => expect(bulkCountText()).toBe("1 / 2"));
      expect(
        screen.getByText("skills.manage.hiddenSelected"),
      ).toBeInTheDocument();

      // Select-all merges what is on screen and leaves the hidden tick alone.
      await user.click(
        screen.getByRole("button", { name: "skills.manage.selectAll" }),
      );
      fireEvent.click(chipButton("claude"));
      await waitFor(() => expect(bulkCountText()).toBe("3 / 3"));
      expect(rowCheckboxes()[1]).toBeChecked();
      expect(
        screen.queryByText("skills.manage.hiddenSelected"),
      ).not.toBeInTheDocument();
    });

    it("offers select-all while the visible rows are only partly selected", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);

      // Two visible rows, one selected: select-all must stay available. The bug
      // this guards was a hidden selection inflating the count so the list
      // looked fully selected and the button vanished.
      await waitFor(() => expect(bulkCountText()).toBe("1 / 3"));
      expect(
        screen.getByRole("button", { name: "skills.manage.selectAll" }),
      ).toBeInTheDocument();

      await user.click(
        screen.getByRole("button", { name: "skills.manage.selectAll" }),
      );
      await waitFor(() => expect(bulkCountText()).toBe("3 / 3"));
    });

    it("hides the bulk bar while every selected row is filtered out, and restores it after", async () => {
      threeSkills();
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await waitFor(() => expect(bulkCountText()).toBe("1 / 3"));

      // With every tick hidden the visible selection is empty, so the shared
      // bulk bar renders nothing — but the selection itself survives the
      // detour and comes back with the tick intact.
      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "Local Only" } },
      );
      await waitFor(() => expect(screen.queryByRole("toolbar")).toBeNull());

      fireEvent.change(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        { target: { value: "" } },
      );
      await waitFor(() => expect(bulkCountText()).toBe("1 / 3"));
      expect(rowCheckboxes()[0]).toBeChecked();
      expect(
        screen.queryByText("skills.manage.hiddenSelected"),
      ).not.toBeInTheDocument();
    });

    it("prunes selections whose skill no longer exists after a refresh", async () => {
      threeSkills();
      bulkUninstallSkillMock.mockResolvedValue({ succeeded: [], failed: [] });
      const { rerender } = renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await waitFor(() => expect(bulkCountText()).toBe("1 / 3"));

      // The ticked skill disappears from the installed list (uninstalled
      // elsewhere); the next refresh must drop it from the selection instead
      // of keeping a ghost target around for the next bulk action.
      installedSkillsMock = [
        makeInstalledSkill({
          id: "repo/local-only",
          name: "Local Only Skill",
          description: "imported manually",
          repoOwner: undefined,
          repoName: undefined,
        }),
      ];
      rerender(
        <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
      );

      await user.click(rowCheckboxes()[0]);
      await waitFor(() => expect(bulkCountText()).toBe("1 / 1"));
      // No disclosure: the ghost id was pruned, not silently kept hidden.
      expect(
        screen.queryByText("skills.manage.hiddenSelected"),
      ).not.toBeInTheDocument();

      await user.click(screen.getByText("skills.manage.bulkUninstall"));
      await user.click(
        screen.getByRole("button", { name: "skills.manage.bulkUninstall" }),
      );
      await waitFor(() => {
        expect(bulkUninstallSkillMock).toHaveBeenCalledWith([
          "repo/local-only",
        ]);
      });
    });

    it("surfaces the Pi cleanup warning a successful bulk uninstall returns", async () => {
      threeSkills();
      // No rejection at all: the backend removed the managed record but says in
      // the body that a directory was preserved. Reporting plain success here
      // would hide a directory the user has to clean up by hand.
      bulkUninstallSkillMock.mockResolvedValue({
        succeeded: [
          {
            item: "repo/enabled-claude",
            result: {
              backupPath: "/tmp/backup",
              preservedPiPath: "/tmp/pi/skills/repo",
            },
          },
        ],
        failed: [],
      });
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(screen.getByText("skills.manage.bulkUninstall"));
      await user.click(
        screen.getByRole("button", { name: "skills.manage.bulkUninstall" }),
      );

      await waitFor(() => {
        expect(toastWarningMock).toHaveBeenCalledWith(
          "skills.manage.bulkUninstallSuccess",
          expect.objectContaining({
            description: "skills.uninstallPiPreserved",
          }),
        );
      });
      expect(toastSuccessMock).not.toHaveBeenCalled();
    });

    it("reports the extra count when several skills left files behind", async () => {
      threeSkills();
      bulkUninstallSkillMock.mockResolvedValue({
        succeeded: [
          {
            item: "repo/enabled-claude",
            result: { piCleanupIncomplete: true },
          },
          {
            item: "repo/local-only",
            result: { piCleanupIncomplete: true },
          },
        ],
        failed: [],
      });
      renderPanel();

      const user = userEvent.setup();
      await user.click(rowCheckboxes()[0]);
      await user.click(rowCheckboxes()[1]);
      await user.click(screen.getByText("skills.manage.bulkUninstall"));
      await user.click(
        screen.getByRole("button", { name: "skills.manage.bulkUninstall" }),
      );

      await waitFor(() => {
        expect(toastInfoMock).toHaveBeenCalledWith(
          "skills.manage.bulkUninstallCleanupMore",
          { closeButton: true },
        );
      });
    });

    it("shows a Load more control once the list exceeds one page", () => {
      installedSkillsMock = Array.from({ length: 65 }, (_, index) =>
        makeInstalledSkill({
          id: `page/skill-${index}`,
          name: `Paged Skill ${index}`,
        }),
      );
      renderPanel();

      // First page only: 60 rows rendered, the rest waits behind Load more.
      expect(screen.getAllByText(/Paged Skill \d+/).length).toBe(60);

      fireEvent.click(screen.getByText("skills.manage.loadMore"));
      expect(screen.getAllByText(/Paged Skill \d+/).length).toBe(65);
    });
  });
});
