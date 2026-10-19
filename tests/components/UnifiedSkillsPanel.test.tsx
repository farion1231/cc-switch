import { render, screen, waitFor, within, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";

import UnifiedSkillsPanel from "@/components/skills/UnifiedSkillsPanel";
import { settingsApi, skillsApi } from "@/lib/api";
import type {
  InstalledSkill,
  SkillBackupEntry,
  SkillRepoFailure,
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
  repoFailures: [] as SkillRepoFailure[],
  checking: false,
  projects: [] as import("@/lib/api/skills").SkillProjectGroup[],
  discoverable: [] as import("@/lib/api/skills").DiscoverableSkill[],
  repos: [] as import("@/lib/api/skills").SkillRepo[],
  saveGroup: vi.fn(),
  deleteGroup: vi.fn(),
  visibleApps: ["claude", "codex", "pi"] as string[],
}));

vi.mock("@/hooks/useSkillProjectGroups", () => ({
  useSkillProjectGroups: () => ({ data: m.projects, isError: false }),
  useSaveSkillProjectGroup: () => ({
    mutateAsync: m.saveGroup,
    isPending: false,
  }),
  useDeleteSkillProjectGroup: () => ({
    mutateAsync: m.deleteGroup,
    isPending: false,
  }),
}));

// jsdom has no layout; expose all Discover rows for the navigation regression.
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 56,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: index,
        start: index * 56,
      })),
  }),
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
    data: { updates: m.updates, failures: m.repoFailures },
    refetch: m.checkUpdates,
    isFetching: m.checking,
    dataUpdatedAt: 0,
  }),
  useUpdateSkill: () => ({
    mutateAsync: m.updateSkill,
    isPending: false,
  }),
  useDiscoverableSkills: () => ({ data: m.discoverable, refetch: vi.fn() }),
  useDiscoverableSkillsFailures: () => ({ data: [] }),
  useSkillRepos: () => ({ data: m.repos, refetch: vi.fn() }),
  useAddSkillRepo: () => ({ mutateAsync: vi.fn() }),
  useRemoveSkillRepo: () => ({ mutateAsync: vi.fn() }),
  useSearchSkillsSh: () => ({ data: undefined, isLoading: false }),
  useInstallSkill: () => ({ mutateAsync: vi.fn() }),
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

async function expandGroup(name: string) {
  const group = screen.getByRole("region", { name });
  const toggle = within(group).getByRole("button", {
    name: "skillsPage.groups.expand",
  });
  if (toggle.getAttribute("aria-expanded") === "false") {
    await userEvent.click(toggle);
  }
  return group;
}

describe("UnifiedSkillsPanel", () => {
  it("groups repositories case-insensitively without combining unrelated local projects", async () => {
    m.installed = [
      makeSkill({ id: "repo-a", name: "Repo A" }),
      makeSkill({
        id: "repo-b",
        name: "Repo B",
        repoOwner: "OWNER",
        repoName: "REPO",
      }),
      makeSkill({
        id: "local-a",
        name: "Local A",
        repoOwner: undefined,
        repoName: undefined,
      }),
      makeSkill({
        id: "local-b",
        name: "Local B",
        repoOwner: undefined,
        repoName: undefined,
      }),
      makeSkill({
        id: "local-c",
        name: "Local C",
        repoOwner: undefined,
        repoName: undefined,
      }),
    ];
    m.projects = [
      { id: "a", name: "Project A", skillIds: ["local-a"] },
      { id: "b", name: "Project B", skillIds: ["local-b"] },
    ];
    renderPanel();
    expect(
      within(screen.getByRole("region", { name: "owner/repo" })).getByRole(
        "button",
        { name: "skillsPage.groups.expand" },
      ),
    ).toHaveAttribute("aria-expanded", "false");
    await expandGroup("owner/repo");
    await expandGroup("Project A");
    expect(
      within(screen.getByRole("region", { name: "owner/repo" })).getAllByRole(
        "listitem",
      ),
    ).toHaveLength(2);
    expect(
      within(screen.getByRole("region", { name: "Project A" })).getByText(
        "Local A",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("region", { name: "Project A" })).queryByText(
        "Local B",
      ),
    ).not.toBeInTheDocument();
    expect(
      within(
        screen.getByRole("region", { name: "skillsPage.groups.ungrouped" }),
      ).queryByRole("checkbox", { name: "skillsPage.groups.toggle" }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      within(screen.getByRole("region", { name: "Project A" })).getAllByRole(
        "checkbox",
      )[0],
    );
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["local-a"],
        app: "claude",
        enabled: true,
      }),
    );
  });

  it("keeps group scope complete when search hides a sibling", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "Alpha" }),
      makeSkill({
        id: "b",
        name: "Beta",
        directory: "beta",
        description: "Beta",
      }),
    ];
    renderPanel();
    await userEvent.type(
      screen.getByRole("textbox", { name: "skills.installedSearchAriaLabel" }),
      "Alpha",
    );
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();
    const group = screen.getByRole("region", { name: "owner/repo" });
    await userEvent.click(within(group).getAllByRole("checkbox")[0]);
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["a", "b"],
        app: "claude",
        enabled: true,
      }),
    );
  });

  it("keeps a filtered group's mixed state and allows re-enabling after disabling its last visible member", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A", apps: { claude: true } }),
      makeSkill({ id: "b", name: "B" }),
    ];
    const { rerender } = renderPanel();
    await userEvent.click(columns()[0]);
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.pop.onlyApp" }),
    );
    let group = screen.getByRole("region", { name: "owner/repo" });
    expect(within(group).getAllByRole("checkbox")[0]).toHaveAttribute(
      "aria-checked",
      "mixed",
    );
    await userEvent.click(within(group).getAllByRole("checkbox")[0]);
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["b"],
        app: "claude",
        enabled: true,
      }),
    );
    m.installed = m.installed.map((skill) => ({
      ...skill,
      apps: { ...skill.apps, claude: true },
    }));
    rerender(<UnifiedSkillsPanel />);
    await userEvent.click(
      within(screen.getByRole("region", { name: "owner/repo" })).getAllByRole(
        "checkbox",
      )[0],
    );
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenLastCalledWith({
        ids: ["a", "b"],
        app: "claude",
        enabled: false,
      }),
    );
    m.installed = m.installed.map((skill) => ({
      ...skill,
      apps: { ...skill.apps, claude: false },
    }));
    rerender(<UnifiedSkillsPanel />);
    group = screen.getByRole("region", { name: "owner/repo" });
    expect(within(group).queryByText("A")).not.toBeInTheDocument();
    const toggle = within(group).getAllByRole("checkbox")[0];
    expect(toggle).not.toBeDisabled();
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenLastCalledWith({
        ids: ["a", "b"],
        app: "claude",
        enabled: true,
      }),
    );
  });

  it("groups Skills with the same recorded installer source and toggles the full source group", async () => {
    m.installed = [
      makeSkill({
        id: "local:lark-a",
        name: "Lark A",
        repoOwner: undefined,
        repoName: undefined,
        sourceType: "well-known",
        sourceId: "open.feishu.cn",
      }),
      makeSkill({
        id: "local:lark-b",
        name: "Lark B",
        repoOwner: undefined,
        repoName: undefined,
        sourceType: "well-known",
        sourceId: "open.feishu.cn",
      }),
      makeSkill({
        id: "local:unknown",
        name: "Unknown",
        repoOwner: undefined,
        repoName: undefined,
      }),
    ];
    renderPanel();

    const group = await expandGroup("open.feishu.cn");
    await expandGroup("skillsPage.groups.ungrouped");
    expect(within(group).getByText("Lark A")).toBeInTheDocument();
    expect(within(group).getByText("Lark B")).toBeInTheDocument();
    expect(screen.getByText("Unknown")).toBeInTheDocument();

    await userEvent.click(within(group).getAllByRole("checkbox")[0]);
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["local:lark-a", "local:lark-b"],
        app: "claude",
        enabled: true,
      }),
    );
  });

  it("searches source IDs consistently for rows, counts, selection and column actions", async () => {
    m.installed = ["a", "b"].map((id) =>
      makeSkill({
        id: `local:${id}`,
        name: `Lark ${id}`,
        repoOwner: undefined,
        repoName: undefined,
        sourceType: "well-known",
        sourceId: "open.feishu.cn",
      }),
    );
    m.installed.push(makeSkill({ id: "unrelated", name: "Unrelated" }));
    i18n.addResourceBundle("zh", "translation", {
      appMatrix: { found: "Found {{count}}" },
    });
    try {
      renderPanel();
      await userEvent.type(
        screen.getByRole("textbox", {
          name: "skills.installedSearchAriaLabel",
        }),
        "OPEN.FEISHU.CN",
      );
      const group = screen.getByRole("region", { name: "open.feishu.cn" });
      expect(within(group).getByText("Lark a")).toBeInTheDocument();
      expect(within(group).getByText("Lark b")).toBeInTheDocument();
      expect(screen.getByText("Found 2")).toBeInTheDocument();
      expect(screen.queryByText("Unrelated")).not.toBeInTheDocument();
      await userEvent.click(columns()[1]);
      await userEvent.click(
        screen.getByRole("button", { name: "appMatrix.pop.enableRest" }),
      );
      await waitFor(() =>
        expect(m.bulkToggle).toHaveBeenCalledWith({
          ids: ["local:a", "local:b"],
          app: "codex",
          enabled: true,
        }),
      );
      await userEvent.click(
        screen.getByRole("checkbox", { name: "skillsPage.selectAllAria" }),
      );
      const selectedRows = screen.getAllByRole("checkbox", {
        name: "skillsPage.selectAria",
      });
      expect(selectedRows).toHaveLength(2);
      selectedRows.forEach((row) => expect(row).toBeChecked());
    } finally {
      i18n.removeResourceBundle("zh", "translation");
    }
  });

  it("gives a manual project group priority over an automatic source group", async () => {
    m.installed = [
      makeSkill({
        id: "local:lark-a",
        name: "Lark A",
        repoOwner: undefined,
        repoName: undefined,
        sourceType: "well-known",
        sourceId: "open.feishu.cn",
      }),
      makeSkill({
        id: "local:lark-b",
        name: "Lark B",
        repoOwner: undefined,
        repoName: undefined,
        sourceType: "well-known",
        sourceId: "open.feishu.cn",
      }),
    ];
    m.projects = [
      { id: "project", name: "My Lark", skillIds: ["local:lark-a"] },
    ];
    renderPanel();
    await expandGroup("My Lark");
    await expandGroup("skillsPage.groups.ungrouped");

    expect(
      within(screen.getByRole("region", { name: "My Lark" })).getByText(
        "Lark A",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: "open.feishu.cn" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("Lark B")).toBeInTheDocument();
  });

  it("creates and edits a named local project without losing hidden member selections", async () => {
    m.installed = [
      makeSkill({
        id: "a",
        name: "Local A",
        repoOwner: undefined,
        repoName: undefined,
      }),
      makeSkill({
        id: "b",
        name: "Local B",
        repoOwner: undefined,
        repoName: undefined,
      }),
      makeSkill({ id: "repo", name: "Repository Skill" }),
    ];
    renderPanel();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.groups.create" }),
    );
    let dialog = screen.getByRole("dialog");
    expect(
      within(dialog).queryByText("Repository Skill"),
    ).not.toBeInTheDocument();
    await userEvent.type(
      within(dialog).getByRole("textbox", { name: "skillsPage.groups.name" }),
      "Tools",
    );
    await userEvent.click(within(dialog).getAllByRole("checkbox")[0]);
    await userEvent.type(
      within(dialog).getByRole("textbox", { name: "skillsPage.groups.search" }),
      "Local B",
    );
    await userEvent.click(within(dialog).getAllByRole("checkbox")[0]);
    await userEvent.click(
      within(dialog).getByRole("button", { name: "common.save" }),
    );
    await waitFor(() =>
      expect(m.saveGroup).toHaveBeenCalledWith({
        id: undefined,
        name: "Tools",
        skillIds: ["a", "b"],
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("dissolves a project without uninstalling Skills", async () => {
    m.installed = [
      makeSkill({ id: "a", repoOwner: undefined, repoName: undefined }),
    ];
    m.projects = [{ id: "p", name: "Tools", skillIds: ["a"] }];
    renderPanel();
    await userEvent.click(
      within(screen.getByRole("region", { name: "Tools" })).getByRole(
        "button",
        { name: "skillsPage.groups.editNamed" },
      ),
    );
    await userEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "skillsPage.groups.dissolve",
      }),
    );
    await waitFor(() => expect(m.deleteGroup).toHaveBeenCalledWith("p"));
    expect(m.uninstall).not.toHaveBeenCalled();
    expect(m.toggle).not.toHaveBeenCalled();
  });

  it("keeps a manually grouped Skill selectable after source backfill", async () => {
    m.installed = [makeSkill({ id: "repo", name: "Repository Skill" })];
    m.projects = [{ id: "p", name: "Tools", skillIds: ["repo"] }];
    renderPanel();

    await userEvent.click(
      within(screen.getByRole("region", { name: "Tools" })).getByRole(
        "button",
        { name: "skillsPage.groups.editNamed" },
      ),
    );
    expect(
      within(screen.getByRole("dialog")).getByText("Repository Skill"),
    ).toBeInTheDocument();
  });

  it("search reveals a collapsed group and still lets the user collapse it", async () => {
    m.installed = [makeSkill({ name: "Alpha" })];
    renderPanel();
    const group = screen.getByRole("region", { name: "owner/repo" });
    expect(within(group).queryByText("Alpha")).not.toBeInTheDocument();
    const search = screen.getByRole("textbox", {
      name: "skills.installedSearchAriaLabel",
    });
    await userEvent.type(search, "Alpha");
    expect(within(group).getByText("Alpha")).toBeInTheDocument();
    await userEvent.click(
      within(group).getByRole("button", { name: "skillsPage.groups.expand" }),
    );
    expect(within(group).queryByText("Alpha")).not.toBeInTheDocument();
    await userEvent.clear(search);
    expect(within(group).queryByText("Alpha")).not.toBeInTheDocument();
  });

  it("reveals a collapsed group from Discover and still lets the user collapse it again", async () => {
    m.installed = [makeSkill({ name: "Alpha" })];
    m.repos = [{ owner: "owner", name: "repo", branch: "main", enabled: true }];
    m.discoverable = [
      {
        key: "owner/repo:alpha-skill",
        name: "Alpha",
        directory: "alpha-skill",
        description: "Test",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
    ];
    renderPanel();
    await userEvent.click(
      within(screen.getByRole("region", { name: "owner/repo" })).getByRole(
        "button",
        { name: "skillsPage.groups.expand" },
      ),
    );
    await userEvent.click(
      screen.getByRole("tab", { name: "skillsPage.viewDiscover" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.discover.installedAria" }),
    );
    const group = screen.getByRole("region", { name: "owner/repo" });
    expect(within(group).getByText("Alpha")).toBeInTheDocument();
    await userEvent.click(
      within(group).getByRole("button", { name: "skillsPage.groups.expand" }),
    );
    expect(within(group).queryByText("Alpha")).not.toBeInTheDocument();
  });

  beforeEach(() => {
    m.installed = [];
    m.discoverable = [];
    m.repos = [];
    m.projects = [];
    m.saveGroup.mockReset().mockResolvedValue({});
    m.deleteGroup.mockReset().mockResolvedValue(true);
    m.backups = [];
    m.updates = [];
    m.repoFailures = [];
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
    m.checkUpdates
      .mockReset()
      .mockResolvedValue({ data: { updates: [], failures: [] } });
    m.installFromZip.mockReset();
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
    await expandGroup("owner/repo");
    expect(columns()).toHaveLength(3);
    // Pi 列头的说明走 HoverTip（悬停即显），不再用原生 title
    expect(columns()[2]).not.toHaveAttribute("title");
    await userEvent.hover(columns()[2]);
    expect(
      (await screen.findAllByText("skillsPage.piColumnTitle")).length,
    ).toBeGreaterThan(0);
    await userEvent.unhover(columns()[2]);

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
    await expandGroup("owner/repo");
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

  it.each([true, false])(
    "shows an error toast when every bulk toggle fails (enabled=%s)",
    async (enabled) => {
      m.visibleApps = ["hermes"];
      m.installed = [
        makeSkill({ id: "a", name: "A", apps: { hermes: !enabled } }),
        makeSkill({ id: "b", name: "B", apps: { hermes: !enabled } }),
      ];
      m.bulkToggle.mockResolvedValueOnce({
        succeeded: [],
        failed: ["a", "b"].map((item) => ({
          item,
          error: new Error("Invalid skill directory"),
        })),
      });
      renderPanel();
      await expandGroup("owner/repo");
      await userEvent.click(columns()[0]);
      await userEvent.click(
        screen.getByRole("button", {
          name: enabled
            ? "appMatrix.pop.enableRest"
            : "appMatrix.pop.disableAll",
        }),
      );
      await waitFor(() =>
        expect(m.toastError).toHaveBeenCalledWith(
          expect.stringContaining("appMatrix.toast.partialFail"),
          { closeButton: true },
        ),
      );
      expect(m.bulkToggle).toHaveBeenCalledWith({
        ids: ["a", "b"],
        app: "hermes",
        enabled,
      });
      expect(
        screen.getAllByRole("button", { name: "appMatrix.cell.fail" }),
      ).toHaveLength(2);
      expect(m.toastSuccess).not.toHaveBeenCalled();
      expect(m.toastWarning).not.toHaveBeenCalled();
    },
  );

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
    await expandGroup("owner/repo");
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
    expect(m.toastWarning).toHaveBeenCalledWith(
      expect.stringContaining("appMatrix.toast.partialFail"),
      expect.objectContaining({
        closeButton: true,
        duration: 8000,
        action: expect.anything(),
      }),
    );
    expect(m.toastSuccess).not.toHaveBeenCalled();
    expect(m.toastError).not.toHaveBeenCalled();
    const [, options] = m.toastWarning.mock.calls[0];
    act(() => options.action.onClick());
    await waitFor(() =>
      expect(m.bulkToggle).toHaveBeenLastCalledWith({
        ids: ["a"],
        app: "claude",
        enabled: false,
      }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.fixSync" }),
    );
    expect(screen.getByTestId("storage-sheet")).toBeInTheDocument();
  });

  describe.each([true, false])("bulk undo feedback (enabled=%s)", (enabled) => {
    beforeEach(() => {
      m.visibleApps = ["hermes"];
      m.installed = ["a", "b"].map((id) =>
        makeSkill({ id, name: id, apps: { hermes: !enabled } }),
      );
      // Check the actual outcome text and count without changing panel labels.
      i18n.addResourceBundle("zh", "translation", {
        common: { bulkToggleFailed: en.common.bulkToggleFailed },
        appMatrix: { toast: { undone: en.appMatrix.toast.undone } },
      });
    });

    afterEach(() => {
      i18n.removeResourceBundle("zh", "translation");
    });

    it.each(["success", "partial", "total", "rejected"] as const)(
      "reports %s undo results and preserves failed-cell retries",
      async (outcome) => {
        renderPanel();
        await expandGroup("owner/repo");
        await userEvent.click(columns()[0]);
        await userEvent.click(
          screen.getByRole("button", {
            name: enabled
              ? "appMatrix.pop.enableRest"
              : "appMatrix.pop.disableAll",
          }),
        );
        await waitFor(() => expect(m.toastSuccess).toHaveBeenCalledTimes(1));
        const [, options] = m.toastSuccess.mock.calls[0];
        m.toastSuccess.mockClear();

        const failedIds =
          outcome === "success"
            ? []
            : outcome === "partial"
              ? ["b"]
              : ["a", "b"];
        if (outcome === "rejected") {
          m.bulkToggle.mockRejectedValueOnce(new Error("undo failed"));
        } else {
          m.bulkToggle.mockResolvedValueOnce({
            succeeded: ["a", "b"].filter((id) => !failedIds.includes(id)),
            failed: failedIds.map((item) => ({
              item,
              error: new Error("undo failed"),
            })),
          });
        }
        act(() => options.action.onClick());

        const expectedToast =
          outcome === "success"
            ? m.toastSuccess
            : outcome === "partial"
              ? m.toastWarning
              : m.toastError;
        await waitFor(() =>
          expect(expectedToast).toHaveBeenCalledWith(
            outcome === "success"
              ? "Undone"
              : `Failed to update ${failedIds.length} item(s)`,
            { closeButton: true },
          ),
        );
        for (const other of [m.toastSuccess, m.toastWarning, m.toastError]) {
          if (other !== expectedToast) expect(other).not.toHaveBeenCalled();
        }
        expect(m.bulkToggle).toHaveBeenLastCalledWith({
          ids: ["a", "b"],
          app: "hermes",
          enabled: !enabled,
        });
        const failedCells = screen.queryAllByRole("button", {
          name: "appMatrix.cell.fail",
        });
        expect(failedCells).toHaveLength(failedIds.length);

        if (failedIds.length > 0) {
          // Undo releases the write lock and retry keeps the inverse target.
          await userEvent.click(failedCells[0]);
          await waitFor(() =>
            expect(m.toggle).toHaveBeenCalledWith({
              id: failedIds[0],
              app: "hermes",
              enabled: !enabled,
            }),
          );
          await waitFor(() =>
            expect(
              screen.queryAllByRole("button", { name: "appMatrix.cell.fail" }),
            ).toHaveLength(failedIds.length - 1),
          );
        }
      },
    );
  });

  it("enables the selected Skills in one app from the selection bar", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
      makeSkill({ id: "c", name: "C" }),
    ];
    renderPanel();
    await expandGroup("owner/repo");
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

  it("selects every listed Skill from the header checkbox", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
    ];
    renderPanel();
    await expandGroup("owner/repo");
    const all = screen.getByRole("checkbox", {
      name: "skillsPage.selectAllAria",
    }) as HTMLInputElement;
    const picks = screen.getAllByRole("checkbox", {
      name: "skillsPage.selectAria",
    });

    await userEvent.click(picks[0]);
    expect(all.indeterminate).toBe(true);

    await userEvent.click(all);
    expect(all.checked).toBe(true);
    for (const pick of picks) expect(pick).toBeChecked();

    await userEvent.click(all);
    expect(all.checked).toBe(false);
    for (const pick of picks) expect(pick).not.toBeChecked();
  });

  it("uninstalls through the row menu after confirming", async () => {
    m.installed = [makeSkill()];
    m.uninstall.mockResolvedValueOnce({
      preservedPiPath: "/tmp/.pi/agent/skills/alpha-skill",
    });
    renderPanel();
    await expandGroup("owner/repo");
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

  it("filters to Skills with updates from the header count and updates them after confirming", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
    ];
    m.updates = [
      { id: "b", name: "B", remoteHash: "x" },
      { id: "gone", name: "Gone", remoteHash: "y" },
    ];
    renderPanel();
    await expandGroup("owner/repo");
    // 更新横幅已去掉：页头的「N 个可更新」、行上的徽标、「检查更新」按钮说的是同一件事
    expect(screen.getAllByText("skills.updateAvailable")).toHaveLength(1);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    // 已知有更新时「检查更新」直接变成「全部更新」，不用先筛选
    expect(
      screen.getAllByRole("button", { name: "skillsPage.updateAllCount" }),
    ).toHaveLength(1);
    expect(
      screen.queryByRole("button", { name: "skills.checkUpdates" }),
    ).not.toBeInTheDocument();

    const chip = screen.getByRole("button", {
      name: "skillsPage.headerUpdates",
    });
    expect(chip).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(chip);
    expect(chip).toHaveAttribute("aria-pressed", "true");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);

    // 再点一次恢复全部
    await userEvent.click(chip);
    expect(chip).toHaveAttribute("aria-pressed", "false");
    expect(screen.getAllByRole("listitem")).toHaveLength(2);

    await userEvent.click(chip);
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.updateAllCount" }),
    );
    // 确认框列出要更新哪些；已经不在本机的「Gone」不列
    const list = screen.getByRole("list", {
      name: "skillsPage.confirm.updateListAria",
    });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(1);
    expect(within(rows[0]).getByText("B")).toBeInTheDocument();
    // 仓库名可点，打开的地址和列表行的来源链接一致
    const openExternal = vi
      .spyOn(settingsApi, "openExternal")
      .mockResolvedValue(undefined);
    await userEvent.click(
      within(rows[0]).getByRole("button", { name: "owner/repo" }),
    );
    expect(openExternal).toHaveBeenCalledWith("https://github.com/owner/repo");
    openExternal.mockRestore();
    expect(within(list).queryByText("Gone")).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", {
        name: "skillsPage.confirm.updateAllButton",
      }),
    );
    await waitFor(() => expect(m.updateSkill).toHaveBeenCalledTimes(1));
    expect(m.updateSkill).toHaveBeenCalledWith("b");
  });

  it("switches Installed / Discover with page tabs", async () => {
    m.installed = [makeSkill()];
    renderPanel();
    const tabs = screen.getByRole("tablist", { name: "skillsPage.viewAria" });
    const [installed, discover] = within(tabs).getAllByRole("tab");
    expect(installed).toHaveAttribute("aria-selected", "true");
    await userEvent.click(discover);
    expect(
      within(
        screen.getByRole("tablist", { name: "skillsPage.viewAria" }),
      ).getAllByRole("tab")[1],
    ).toHaveAttribute("aria-selected", "true");
  });

  it("lets the unmanaged notice be ignored with one control", async () => {
    m.installed = [makeSkill()];
    renderPanel();
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "skillsPage.banner.ignore" }),
      ).toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: "skillsPage.banner.close" }),
    ).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.banner.ignore" }),
    );
    expect(
      screen.queryByText("skillsPage.banner.unmanaged"),
    ).not.toBeInTheDocument();
  });

  it("highlights a column header and shows the app name while hovering its cells", async () => {
    m.installed = [makeSkill({ apps: { claude: true } })];
    renderPanel();
    await expandGroup("owner/repo");
    const cells = screen.getAllByRole("button", { name: /appMatrix.cell/ });
    // 开 / 关是同一个方框，只换填充
    expect(cells[0]).toHaveAttribute("data-state", "on");
    expect(cells[1]).toHaveAttribute("data-state", "off");
    expect(cells[0].className).toBe(cells[1].className);

    expect(screen.queryByTestId("matrix-column-name")).not.toBeInTheDocument();
    await userEvent.hover(cells[1]);
    expect(columns()[1]).toHaveAttribute("data-highlighted");
    expect(columns()[0]).not.toHaveAttribute("data-highlighted");
    expect(screen.getByTestId("matrix-column-name")).toHaveTextContent("Codex");
    await userEvent.unhover(cells[1]);
    expect(columns()[1]).not.toHaveAttribute("data-highlighted");
    expect(screen.queryByTestId("matrix-column-name")).not.toBeInTheDocument();

    await userEvent.hover(columns()[2]);
    expect(screen.getByTestId("matrix-column-name")).toHaveTextContent("Pi");
  });

  it("ignores a second check-update click while one is running", async () => {
    m.installed = [makeSkill()];
    let resolve!: (value: {
      data: { updates: never[]; failures: never[] };
    }) => void;
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
      resolve({ data: { updates: [], failures: [] } });
    });
  });

  it("closes the confirm dialog right away and shows progress while updating", async () => {
    m.installed = [
      makeSkill({ id: "a", name: "A" }),
      makeSkill({ id: "b", name: "B" }),
    ];
    m.updates = [
      { id: "a", name: "A", remoteHash: "x" },
      { id: "b", name: "B", remoteHash: "y" },
    ];
    const pending: Array<(value: InstalledSkill) => void> = [];
    m.updateSkill.mockImplementation(
      (id: string) =>
        new Promise<InstalledSkill>((resolve) => {
          pending.push(() => resolve(makeSkill({ id, name: id })));
        }),
    );
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.updateAllCount" }),
    );
    await userEvent.click(
      screen.getByRole("button", {
        name: "skillsPage.confirm.updateAllButton",
      }),
    );
    // 第一个还没更新完，确认框已经关了，进度在按钮上
    await waitFor(() => expect(m.updateSkill).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByRole("button", {
        name: "skillsPage.confirm.updateAllButton",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "skillsPage.updatingProgress" }),
    ).toBeDisabled();

    await act(async () => pending[0]?.(makeSkill()));
    await waitFor(() => expect(m.updateSkill).toHaveBeenCalledTimes(2));
    await act(async () => pending[1]?.(makeSkill()));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "skillsPage.updatingProgress" }),
      ).not.toBeInTheDocument(),
    );
    expect(m.toastSuccess).toHaveBeenCalledWith(
      "skills.updateAllSuccess",
      expect.anything(),
    );
  });

  it("asks to update everything right after a check finds updates", async () => {
    m.installed = [makeSkill({ id: "a", name: "A" })];
    const updates = [
      { id: "a", name: "A", remoteHash: "x" },
      { id: "gone", name: "Gone", remoteHash: "y" },
    ];
    m.checkUpdates.mockImplementationOnce(async () => {
      // 真实的 refetch 会把结果写进查询缓存
      m.updates = updates;
      return { data: { updates, failures: [] } };
    });
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: "skills.checkUpdates" }),
    );
    await userEvent.click(
      await screen.findByRole("button", {
        name: "skillsPage.confirm.updateAllButton",
      }),
    );
    await waitFor(() => expect(m.updateSkill).toHaveBeenCalledTimes(1));
    expect(m.updateSkill).toHaveBeenCalledWith("a");
  });

  it("does not call everything up to date when a repository could not be read", async () => {
    m.installed = [makeSkill()];
    const failure = {
      owner: "owner",
      name: "repo",
      branch: "main",
      error: "network down",
    };
    m.checkUpdates.mockResolvedValueOnce({
      data: { updates: [], failures: [failure] },
    });
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: "skills.checkUpdates" }),
    );

    await waitFor(() =>
      expect(m.toastWarning).toHaveBeenCalledWith(
        "skills.updatesIncomplete",
        expect.anything(),
      ),
    );
    expect(m.toastSuccess).not.toHaveBeenCalledWith(
      "skills.noUpdates",
      expect.anything(),
    );
  });

  it("shows which repositories the update check could not read", () => {
    m.installed = [makeSkill()];
    m.repoFailures = [
      { owner: "owner", name: "repo", branch: "main", error: "network down" },
    ];
    renderPanel();
    expect(screen.getByText("skillsPage.repoFail.title")).toBeInTheDocument();
    expect(
      screen.getByText("skillsPage.repoFail.updatesBody"),
    ).toBeInTheDocument();
  });

  it("scans only the workspace explicitly picked by the user", async () => {
    const pickDirectory = vi
      .spyOn(settingsApi, "pickDirectory")
      .mockResolvedValue("/tmp/local-projects");
    const scanLocalSources = vi
      .spyOn(skillsApi, "scanLocalSources")
      .mockResolvedValue({
        updated: 2,
        ambiguous: 1,
        scannedRepositories: 3,
        scannedSkills: 8,
      });
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: "skills.moreActions" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", {
        name: "skillsPage.moreMenu.scanLocalSources",
      }),
    );

    await waitFor(() =>
      expect(scanLocalSources).toHaveBeenCalledWith("/tmp/local-projects"),
    );
    expect(pickDirectory).toHaveBeenCalledOnce();
    expect(m.toastSuccess).toHaveBeenCalledWith(
      "skills.localSourceScan.completed",
      expect.objectContaining({
        description: "skills.localSourceScan.ambiguous",
      }),
    );
    pickDirectory.mockRestore();
    scanLocalSources.mockRestore();
  });

  it("translates structured local source scan errors in the toast", async () => {
    const pick = vi
      .spyOn(settingsApi, "pickDirectory")
      .mockResolvedValue("/tmp/projects");
    const scan = vi.spyOn(skillsApi, "scanLocalSources").mockRejectedValue(
      JSON.stringify({
        code: "LOCAL_SOURCE_SCAN_REPOSITORIES",
        context: { limit: "100" },
      }),
    );
    i18n.addResourceBundle("zh", "translation", {
      skills: { localSourceScan: en.skills.localSourceScan },
    });
    try {
      renderPanel();
      await openMenu(
        "skills.moreActions",
        "skillsPage.moreMenu.scanLocalSources",
      );
      await waitFor(() =>
        expect(m.toastError).toHaveBeenCalledWith("common.error", {
          description:
            "The scan exceeds 100 Git projects. Choose a smaller workspace and try again.",
        }),
      );
    } finally {
      pick.mockRestore();
      scan.mockRestore();
      i18n.removeResourceBundle("zh", "translation");
    }
  });

  it("says which ZIP Skills were skipped because the folder name is taken", async () => {
    m.installed = [makeSkill()];
    const dialog = vi
      .spyOn(skillsApi, "openZipFileDialog")
      .mockResolvedValue("/tmp/team.zip");
    m.installFromZip.mockResolvedValueOnce({
      installed: [],
      skipped: [
        {
          directory: "my-helper",
          existingId: "local:my-helper",
          existingName: "My Helper",
        },
      ],
    });
    renderPanel();

    await userEvent.click(
      screen.getByRole("button", { name: /skillsPage.add/ }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "skillsPage.addMenu.zip" }),
    );

    await waitFor(() =>
      expect(m.toastWarning).toHaveBeenCalledWith(
        "skills.installFromZip.allSkipped",
        expect.objectContaining({
          description: "skills.installFromZip.skippedBody",
        }),
      ),
    );
    expect(m.toastInfo).not.toHaveBeenCalledWith(
      "skills.installFromZip.noSkillsFound",
      expect.anything(),
    );
    dialog.mockRestore();
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
    await expandGroup("owner/repo");
    await waitFor(() => {
      expect(onInteraction).toHaveBeenLastCalledWith(true);
      expect(onNavigation).toHaveBeenLastCalledWith(false);
    });
    // 外观上的禁用晚 300ms 出现（useDelayedFlag），拦截本身是立即的
    await waitFor(() =>
      expect(
        screen.getAllByRole("button", { name: /appMatrix.cell/ })[0],
      ).toBeDisabled(),
    );
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
