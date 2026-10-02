import { render, screen, waitFor, within, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import type {
  InstalledSkill,
  SkillBackupEntry,
  SkillUpdateInfo,
} from "@/lib/api/skills";

const m = vi.hoisted(() => ({
  scanUnmanaged: vi.fn(),
  toggle: vi.fn(),
  bulkToggle: vi.fn(),
  uninstall: vi.fn(),
  importSkills: vi.fn(),
  installFromZip: vi.fn(),
  deleteBackup: vi.fn(),
  restoreBackup: vi.fn(),
  checkUpdates: vi.fn(),
  updateSkill: vi.fn(),
  refetchBackups: vi.fn(),
  toastError: vi.fn(),
  toastSuccess: vi.fn(),
  toastWarning: vi.fn(),
  toastInfo: vi.fn(),
  installed: [] as InstalledSkill[],
  backups: [] as SkillBackupEntry[],
  updates: [] as SkillUpdateInfo[],
  checking: false,
  visibleApps: ["claude", "codex", "pi"] as string[],
}));

vi.mock("sonner", () => ({
  toast: {
    success: m.toastSuccess,
    error: m.toastError,
    warning: m.toastWarning,
    info: m.toastInfo,
  },
}));

vi.mock("@/components/mcp/useVisibleAppIds", () => ({
  useVisibleAppIds: (ids: string[]) =>
    ids.filter((id) => m.visibleApps.includes(id)),
}));

vi.mock("@/components/skills/SkillsStorageSheet", () => ({
  SkillsStorageSheet: ({ open }: { open: boolean }) =>
    open ? <div data-testid="storage-sheet" /> : null,
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: m.installed,
    isLoading: false,
    isError: false,
    refetch: vi.fn(),
  }),
  useSkillBackups: () => ({
    data: m.backups,
    refetch: m.refetchBackups,
    isFetching: false,
  }),
  useDeleteSkillBackup: () => ({
    mutateAsync: m.deleteBackup,
    isPending: false,
  }),
  useToggleSkillApp: () => ({ mutateAsync: m.toggle, isPending: false }),
  useBulkToggleSkillApp: () => ({
    mutateAsync: m.bulkToggle,
    isPending: false,
  }),
  useRestoreSkillBackup: () => ({
    mutateAsync: m.restoreBackup,
    isPending: false,
  }),
  useUninstallSkill: () => ({ mutateAsync: m.uninstall, isPending: false }),
  useScanUnmanagedSkills: () => ({
    data: [
      {
        directory: "shared-skill",
        name: "Shared Skill",
        foundIn: ["grokbuild", "claude"],
        path: "/tmp/shared-skill",
      },
    ],
    refetch: m.scanUnmanaged,
  }),
  useImportSkillsFromApps: () => ({
    mutateAsync: m.importSkills,
    isPending: false,
  }),
  useInstallSkillsFromZip: () => ({
    mutateAsync: m.installFromZip,
    isPending: false,
  }),
  useCheckSkillUpdates: () => ({
    data: m.updates,
    refetch: m.checkUpdates,
    isFetching: m.checking,
    dataUpdatedAt: 0,
  }),
  useUpdateSkill: () => ({
    mutateAsync: m.updateSkill,
    isPending: false,
  }),
  useDiscoverableSkills: () => ({ data: [], refetch: vi.fn() }),
  useSkillRepos: () => ({ data: [], refetch: vi.fn() }),
  useAddSkillRepo: () => ({ mutateAsync: vi.fn() }),
  useRemoveSkillRepo: () => ({ mutateAsync: vi.fn() }),
}));

type Overrides = Omit<Partial<InstalledSkill>, "apps"> & {
  apps?: Partial<InstalledSkill["apps"]>;
};

const makeSkill = (overrides: Overrides = {}): InstalledSkill => {
  const { apps, ...rest } = overrides;
  return {
    id: "owner/repo:alpha-skill",
    name: "Alpha Skill",
    description: "Alpha description",
    directory: "alpha-skill",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      grokbuild: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      pi: false,
      ...apps,
    },
    installedAt: 1,
    updatedAt: 1,
    ...rest,
  };
};

const renderPanel = (
  props: React.ComponentProps<typeof UnifiedSkillsPanel> = {},
) => render(<UnifiedSkillsPanel {...props} />);

const columns = () =>
  screen.getAllByRole("button", { name: /appMatrix.columnAria/ });

async function openMenu(trigger: string, item: string) {
  await userEvent.click(screen.getByRole("button", { name: trigger }));
  await userEvent.click(await screen.findByRole("menuitem", { name: item }));
}

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    m.installed = [];
    m.backups = [];
    m.updates = [];
    m.checking = false;
    m.visibleApps = ["claude", "codex", "pi"];
    m.scanUnmanaged.mockReset().mockResolvedValue({
      data: [
        {
          directory: "shared-skill",
          name: "Shared Skill",
          foundIn: ["grokbuild", "claude"],
          path: "/tmp/shared-skill",
        },
      ],
    });
    m.toggle.mockReset().mockResolvedValue(true);
    m.bulkToggle
      .mockReset()
      .mockImplementation(async ({ ids }) => ({ succeeded: ids, failed: [] }));
    m.uninstall.mockReset().mockResolvedValue({});
    m.importSkills.mockReset().mockResolvedValue([]);
    m.deleteBackup.mockReset();
    m.restoreBackup.mockReset();
    m.refetchBackups.mockReset().mockResolvedValue({ data: [] });
    m.checkUpdates.mockReset().mockResolvedValue({ data: [] });
    m.updateSkill
      .mockReset()
      .mockImplementation(async (id: string) => makeSkill({ id }));
    for (const fn of [
      m.toastError,
      m.toastSuccess,
      m.toastWarning,
      m.toastInfo,
    ]) {
      fn.mockReset();
    }
  });

  it("always shows the Pi column and toggles it like the other apps", async () => {
    m.installed = [makeSkill({ apps: { claude: true, pi: true } })];
    renderPanel();
    expect(columns()).toHaveLength(3);
    expect(columns()[2]).toHaveAttribute("title", "skillsPage.piColumnTitle");

    const cells = screen.getAllByRole("button", { name: /appMatrix.cell/ });
    expect(cells[2]).toHaveAttribute("aria-pressed", "true");
    await userEvent.click(cells[2]);
    await waitFor(() =>
      expect(m.toggle).toHaveBeenCalledWith({
        id: "owner/repo:alpha-skill",
        app: "pi",
        enabled: false,
      }),
    );
  });

  it("hides the Pi column only when Pi is hidden on the Apps page", () => {
    m.visibleApps = ["claude", "codex"];
    m.installed = [makeSkill()];
    renderPanel();
    expect(columns()).toHaveLength(2);
  });

  it("distinguishes an empty list from a search miss", async () => {
    renderPanel();
    expect(screen.getByText("skillsPage.emptyTitle")).toBeInTheDocument();
  });

  it("shows a search miss with a way into Discover", async () => {
    m.installed = [makeSkill()];
    renderPanel();
    await userEvent.type(
      screen.getByRole("textbox", { name: "skills.installedSearchAriaLabel" }),
      "zzz",
    );
    expect(screen.getByText("skillsPage.noMatch")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skillsPage.searchInDiscover" }),
    ).toBeInTheDocument();
  });

  it("bulk-enables only the disabled Skills in a column and offers undo", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A", apps: { codex: true } }),
      makeSkill({ id: "b", name: "B" }),
      makeSkill({ id: "c", name: "C" }),
    ];
    renderPanel();
    await userEvent.click(columns()[1]);
    await userEvent.click(
      screen.getByRole("button", { name: "appMatrix.pop.enableRest" }),
    );
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["b", "c"],
        app: "codex",
        enabled: true,
      }),
    );
    await waitFor(() => expect(m.toastSuccess).toHaveBeenCalled());
    const [, options] = m.toastSuccess.mock.calls[0];
    act(() => options.action.onClick());
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenLastCalledWith({
        ids: ["b", "c"],
        app: "codex",
        enabled: false,
      }),
    );
  });

  it("marks partial bulk failures in the matrix", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
    ];
    m.bulkToggle.mockResolvedValueOnce({
      succeeded: ["a"],
      failed: [{ item: "b", error: new Error("symlink failed") }],
    });
    renderPanel();
    await userEvent.click(columns()[0]);
    await userEvent.click(
      screen.getByRole("button", { name: "appMatrix.pop.enableRest" }),
    );
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "appMatrix.cell.fail" }),
      ).toBeInTheDocument(),
    );
    expect(screen.getByText(/skillsPage.rowFail/)).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.fixSync" }),
    );
    expect(screen.getByTestId("storage-sheet")).toBeInTheDocument();
  });

  it("enables the selected Skills in one app from the selection bar", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
      makeSkill({ id: "c", name: "C" }),
    ];
    renderPanel();
    const picks = screen.getAllByRole("checkbox", {
      name: "skillsPage.selectAria",
    });
    await userEvent.click(picks[0]);
    await userEvent.click(picks[2]);
    expect(screen.getByText("skillsPage.bulk.selected")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /skillsPage.bulk.enableTo/ }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: /Codex/ }),
    );
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["a", "c"],
        app: "codex",
        enabled: true,
      }),
    );
  });

  it("uninstalls through the row menu after confirming", async () => {
    m.installed = [makeSkill()];
    m.uninstall.mockResolvedValueOnce({
      preservedPiPath: "/tmp/.pi/agent/skills/alpha-skill",
    });
    renderPanel();
    await openMenu("skillsPage.rowMoreAria", "skillsPage.uninstallEllipsis");
    expect(
      screen.getByText("skillsPage.confirm.uninstallBody"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "skills.uninstall" }),
    );
    await waitFor(() =>
      expect(m.uninstall).toHaveBeenCalledWith("owner/repo:alpha-skill"),
    );
    await waitFor(() =>
      expect(m.toastWarning).toHaveBeenCalledWith(
        "skills.uninstallSuccess",
        expect.objectContaining({ description: "skills.uninstallPiPreserved" }),
      ),
    );
  });

  it("filters to Skills with updates and updates them after confirming", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
    ];
    m.updates = [
      { id: "b", name: "B", remoteHash: "x" },
      { id: "gone", name: "Gone", remoteHash: "y" },
    ];
    renderPanel();
    expect(screen.getAllByText("skills.updateAvailable")).toHaveLength(1);
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.banner.updateAll" }),
    );
    await userEvent.click(
      screen.getByRole("button", {
        name: "skillsPage.confirm.updateAllButton",
      }),
    );
    await waitFor(() => expect(m.updateSkill).toHaveBeenCalledTimes(1));
    expect(m.updateSkill).toHaveBeenCalledWith("b");
  });

  it("ignores a second check-update click while one is running", async () => {
    m.installed = [makeSkill()];
    let resolve!: (value: { data: never[] }) => void;
    m.checkUpdates.mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );
    renderPanel();
    const button = screen.getByRole("button", { name: "skills.checkUpdates" });
    await userEvent.click(button);
    await userEvent.click(button);
    expect(m.checkUpdates).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolve({ data: [] });
    });
  });

  it("blocks actions but not navigation while checking updates", async () => {
    m.installed = [makeSkill()];
    m.checking = true;
    const onInteraction = vi.fn();
    const onNavigation = vi.fn();
    renderPanel({
      onInteractionBlockedChange: onInteraction,
      onNavigationBlockedChange: onNavigation,
    });
    await waitFor(() => {
      expect(onInteraction).toHaveBeenLastCalledWith(true);
      expect(onNavigation).toHaveBeenLastCalledWith(false);
    });
    expect(
      screen.getAllByRole("button", { name: /appMatrix.cell/ })[0],
    ).toBeDisabled();
  });

  it("imports with apps chosen from where each Skill was found", async () => {
    m.installed = [makeSkill()];
    m.visibleApps = ["claude", "codex", "grokbuild", "pi"];
    m.importSkills.mockResolvedValueOnce([makeSkill({ id: "shared" })]);
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.banner.reviewImport" }),
    );
    await waitFor(() =>
      expect(screen.getByText("skillsPage.import.title")).toBeInTheDocument(),
    );
    // Pi 按目录判断，导入时不提供勾选
    const dialog = screen.getByRole("dialog");
    expect(within(dialog).queryByText("Pi")).not.toBeInTheDocument();
    await userEvent.click(
      within(dialog).getByRole("button", { name: "skillsPage.import.submit" }),
    );
    await waitFor(() => expect(m.importSkills).toHaveBeenCalled());
    expect(m.importSkills.mock.calls[0][0]).toEqual([
      {
        directory: "shared-skill",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          grokbuild: true,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
          mcode: false,
        },
      },
    ]);
  });

  it("closes the backup dialog and reports an explicit refresh failure", async () => {
    m.refetchBackups.mockRejectedValueOnce(new Error("refresh failed"));
    renderPanel();
    await openMenu("skills.moreActions", "skillsPage.moreMenu.restore");
    await waitFor(() =>
      expect(m.toastError).toHaveBeenCalledWith("common.error", {
        description: "Error: refresh failed",
      }),
    );
    expect(
      screen.queryByText("skills.restoreFromBackup.title"),
    ).not.toBeInTheDocument();
  });

  it("does not report a completed backup deletion as failed when refresh rejects", async () => {
    const consoleErrorSpy = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    m.backups = [
      {
        backupId: "backup-1",
        backupPath: "/backups/backup-1",
        createdAt: 1,
        skill: makeSkill({ name: "Backup Skill" }),
      },
    ];
    m.deleteBackup.mockResolvedValueOnce(true);
    m.refetchBackups
      .mockResolvedValueOnce({ data: m.backups })
      .mockRejectedValueOnce(new Error("refresh failed"));
    renderPanel();
    await openMenu("skills.moreActions", "skillsPage.moreMenu.restore");
    await userEvent.click(
      await screen.findByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );
    const confirmDialog = screen
      .getByText("skills.restoreFromBackup.deleteConfirmTitle")
      .closest<HTMLElement>('[role="dialog"]');
    await userEvent.click(
      within(confirmDialog!).getByRole("button", {
        name: "skills.restoreFromBackup.delete",
      }),
    );
    await waitFor(() =>
      expect(m.toastSuccess).toHaveBeenCalledWith(
        "skills.restoreFromBackup.deleteSuccess",
        { closeButton: true },
      ),
    );
    expect(m.toastError).not.toHaveBeenCalled();
    consoleErrorSpy.mockRestore();
  });
});
