import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { SkillsPage } from "@/components/skills/SkillsPage";
import type {
  DiscoverableSkill,
  InstalledSkill,
  SkillRepo,
  SkillRepoFailure,
  SkillsShDiscoverableSkill,
  SkillsShSearchResult,
} from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { settingsApi } from "@/lib/api";

const m = vi.hoisted(() => ({
  install: vi.fn(),
  addRepo: vi.fn(),
  refetchDiscoverable: vi.fn(),
  discoverable: [] as DiscoverableSkill[],
  failures: [] as SkillRepoFailure[],
  installed: [] as InstalledSkill[],
  repos: [] as SkillRepo[],
}));

// 同一个 query 返回同一个对象，否则 useEffect([skillsShResult]) 会无限 setState
const searchCache = new Map<
  string,
  {
    data: SkillsShSearchResult | undefined;
    isLoading: boolean;
    isFetching: boolean;
    isPlaceholderData?: boolean;
    isError?: boolean;
    dataUpdatedAt?: number;
    refetch?: () => void;
  }
>();

const setSearchResult = (
  query: string,
  offset: number,
  result: SkillsShSearchResult | undefined,
  state: Partial<{
    isLoading: boolean;
    isFetching: boolean;
    isError: boolean;
    dataUpdatedAt: number;
    refetch: () => void;
  }> = {},
) => {
  searchCache.set(`${query}:${offset}`, {
    data: result,
    isLoading: false,
    isFetching: false,
    dataUpdatedAt: 1,
    ...state,
  });
};

// jsdom 没有布局，虚拟列表量不出可视高度：全部渲染
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
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn() },
}));

vi.mock("@/hooks/useSkills", () => ({
  useDiscoverableSkills: () => ({
    data: m.discoverable,
    isLoading: false,
    isFetching: false,
    isError: false,
  }),
  useRefreshDiscoverableSkills: () => m.refetchDiscoverable,
  useDiscoverableSkillsFailures: () => ({ data: m.failures }),
  useInstalledSkills: () => ({ data: m.installed, isLoading: false }),
  useInstallSkill: () => ({ mutateAsync: m.install }),
  useSkillRepos: () => ({ data: m.repos, isLoading: false, refetch: vi.fn() }),
  useAddSkillRepo: () => ({ mutateAsync: m.addRepo }),
  useRemoveSkillRepo: () => ({ mutateAsync: vi.fn() }),
  useSearchSkillsSh: (query: string, _limit: number, offset: number) =>
    searchCache.get(`${query}:${offset}`) ?? {
      data: undefined,
      isLoading: false,
      isFetching: false,
    },
}));

const makeSkillsSh = (
  overrides: Partial<SkillsShDiscoverableSkill> = {},
): SkillsShDiscoverableSkill => ({
  key: "agent-browser:owner-a:repo-a",
  name: "Agent Browser",
  directory: "agent-browser",
  repoOwner: "owner-a",
  repoName: "repo-a",
  repoBranch: "main",
  installs: 12300,
  readmeUrl: "https://example.com/a",
  ...overrides,
});

const makeDiscoverable = (
  overrides: Partial<DiscoverableSkill> = {},
): DiscoverableSkill => ({
  key: "repo-skill:owner-a:repo-a",
  name: "repo-skill",
  description: "Skill from a configured repository",
  directory: "repo-skill",
  repoOwner: "owner-a",
  repoName: "repo-a",
  repoBranch: "main",
  ...overrides,
});

const makeRepo = (overrides: Partial<SkillRepo> = {}): SkillRepo => ({
  owner: "owner-a",
  name: "repo-a",
  branch: "main",
  enabled: true,
  ...overrides,
});

function renderPage(
  overrides: Partial<React.ComponentProps<typeof SkillsPage>> = {},
) {
  const installTo = vi.fn(
    async (install: (first: AppId) => Promise<Array<{ id: string }>>) => ({
      installed: await install("codex"),
      failures: [],
    }),
  );
  const onShowInstalled = vi.fn();
  render(
    <SkillsPage
      source={m.repos.length > 0 ? "repos" : "skillssh"}
      visibleAppIds={["claude", "codex", "pi"]}
      installTargets={["codex", "claude"]}
      onInstallTargetsChange={vi.fn()}
      installTo={installTo as never}
      onShowInstalled={onShowInstalled}
      onOpenRepoManager={vi.fn()}
      onEnableFailures={vi.fn()}
      {...overrides}
    />,
  );
  return { installTo, onShowInstalled };
}

describe("SkillsPage (Discover)", () => {
  beforeEach(() => {
    m.install.mockReset().mockImplementation(async ({ skill }) => ({
      id: skill.key,
    }));
    m.addRepo.mockReset().mockResolvedValue(true);
    m.refetchDiscoverable.mockReset();
    m.discoverable = [];
    m.failures = [];
    m.installed = [];
    m.repos = [];
    searchCache.clear();
  });

  it("installs the chosen skills.sh result into the first install-to app", async () => {
    setSearchResult("agent", 0, {
      skills: [
        makeSkillsSh({ key: "a", name: "Agent Browser A" }),
        makeSkillsSh({
          key: "b",
          name: "Agent Browser B",
          repoOwner: "owner-b",
          repoName: "repo-b",
        }),
      ],
      totalCount: 2,
      query: "agent",
    });
    const { installTo } = renderPage();
    // 没配置仓库时直接是 skills.sh
    const input = screen.getByRole("textbox", {
      name: "skillsPage.discover.skillsShAria",
    });
    await userEvent.type(input, "agent{Enter}");
    await screen.findByText("Agent Browser B");

    const rows = screen.getAllByRole("listitem");
    await userEvent.click(
      within(rows[1]).getByRole("button", {
        name: "skillsPage.discover.installAria",
      }),
    );
    await waitFor(() => expect(m.install).toHaveBeenCalledTimes(1));
    expect(installTo).toHaveBeenCalledTimes(1);
    const args = m.install.mock.calls[0][0];
    expect(args.currentApp).toBe("codex");
    expect(args.skill.repoOwner).toBe("owner-b");
    expect(args.skill.name).toBe("Agent Browser B");
  });

  it("keeps skills.sh results when the same query is submitted again", async () => {
    setSearchResult("figma", 0, {
      skills: [makeSkillsSh({ key: "f", name: "figma-use" })],
      totalCount: 1,
      query: "figma",
    });
    renderPage();
    const input = screen.getByRole("textbox", {
      name: "skillsPage.discover.skillsShAria",
    });
    await userEvent.type(input, "figma{Enter}");
    await screen.findByText("figma-use");
    await userEvent.type(input, "{Enter}");
    expect(screen.getByText("figma-use")).toBeInTheDocument();
  });

  it("refetches the first skills.sh page when retrying after a failure", async () => {
    const refetch = vi.fn();
    setSearchResult("boom", 0, undefined, { isError: true, refetch });
    renderPage();
    await userEvent.type(
      screen.getByRole("textbox", { name: "skillsPage.discover.skillsShAria" }),
      "boom{Enter}",
    );
    expect(screen.getByText("skills.skillssh.error")).toBeInTheDocument();
    // 第一页失败时 query key 没变，重试必须显式 refetch，不能只把 offset 设回 0
    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it("shows the loading state while a skills.sh query is fetching", async () => {
    setSearchResult("x1", 0, undefined, { isLoading: true, isFetching: true });
    renderPage();
    await userEvent.type(
      screen.getByRole("textbox", { name: "skillsPage.discover.skillsShAria" }),
      "x1{Enter}",
    );
    expect(screen.getByText("skills.skillssh.loading")).toBeInTheDocument();
  });

  it("stays on repositories when they return no skills, so a retry is possible", () => {
    m.repos = [makeRepo()];
    renderPage({ source: "repos" });
    expect(
      screen.getByText("skillsPage.discover.noneRead"),
    ).toBeInTheDocument();
  });

  it("says which repositories could not be read when only some failed", async () => {
    m.repos = [makeRepo(), makeRepo({ owner: "composio", name: "awesome" })];
    m.discoverable = [makeDiscoverable()];
    m.failures = [
      {
        owner: "composio",
        name: "awesome",
        branch: "main",
        error: JSON.stringify({
          code: "DOWNLOAD_FAILED",
          context: { status: "403" },
          suggestion: "http403",
        }),
      },
    ];
    renderPage({ source: "repos" });

    // 读到的照常列出，横幅说哪些没读到
    expect(screen.getByText("repo-skill")).toBeInTheDocument();
    expect(screen.getByText("skillsPage.repoFail.title")).toBeInTheDocument();
    expect(
      screen.getByText("skillsPage.repoFail.discoverBody"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(m.refetchDiscoverable).toHaveBeenCalled();

    // 筛选弹层里的仓库行写「读取失败」
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.filter.button" }),
    );
    expect(
      await screen.findByText("skillsPage.repos.readFailed"),
    ).toBeInTheDocument();
  });

  it("calls it a read failure, not an empty repository, when every repository failed", () => {
    m.repos = [makeRepo()];
    m.failures = [
      { owner: "owner-a", name: "repo-a", branch: "main", error: "timeout" },
    ];
    renderPage();
    expect(
      screen.getByText("skillsPage.discover.loadFailed"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("skillsPage.discover.noneRead"),
    ).not.toBeInTheDocument();
    // 全部没读到时由空状态说明，不再叠一条横幅
    expect(
      screen.queryByText("skillsPage.repoFail.discoverBody"),
    ).not.toBeInTheDocument();
  });

  it("links an installed result to its row instead of offering uninstall", async () => {
    m.repos = [makeRepo()];
    m.discoverable = [
      makeDiscoverable(),
      makeDiscoverable({
        key: "other",
        name: "other-skill",
        directory: "other-skill",
      }),
    ];
    m.installed = [
      {
        id: "owner-a/repo-a:repo-skill",
        name: "repo-skill",
        directory: "repo-skill",
        repoOwner: "owner-a",
        repoName: "repo-a",
        apps: {
          claude: true,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
        },
        installedAt: 1,
        updatedAt: 1,
      },
    ];
    const { onShowInstalled } = renderPage();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.discover.installedAria" }),
    );
    expect(onShowInstalled).toHaveBeenCalledWith("owner-a/repo-a:repo-skill");
    expect(screen.queryByText("skills.uninstall")).not.toBeInTheDocument();

    // 「只看未安装」收在统一筛选弹层里
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.filter.button" }),
    );
    await userEvent.click(
      await screen.findByRole("checkbox", {
        name: "skillsPage.discover.onlyUninstalled",
      }),
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("other-skill")).toBeInTheDocument();
  });

  it("does not treat the same path on another host as the installed skill", () => {
    // 仓库只按 owner/name 定位：GitHub 的 owner-a/repo-a 被换成 GitLab 上的同名仓库后，
    // 之前从 GitHub 装的 repo-skill 不是这一个，只能算目录被占用
    m.repos = [makeRepo({ host: "gitlab.example.com" })];
    m.discoverable = [makeDiscoverable({ repoHost: "gitlab.example.com" })];
    m.installed = [
      {
        id: "owner-a/repo-a:repo-skill",
        name: "repo-skill",
        directory: "repo-skill",
        repoOwner: "owner-a",
        repoName: "repo-a",
        apps: {
          claude: true,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
          pi: false,
        },
        installedAt: 1,
        updatedAt: 1,
      },
    ];
    renderPage();
    expect(
      screen.queryByRole("button", {
        name: "skillsPage.discover.installedAria",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("skillsPage.discover.dirTaken"),
    ).toBeInTheDocument();
  });

  it("shows an install failure on the row and lets the user retry", async () => {
    m.repos = [makeRepo()];
    m.discoverable = [makeDiscoverable()];
    m.install.mockRejectedValueOnce(new Error("timeout"));
    renderPage();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.discover.installAria" }),
    );
    await waitFor(() =>
      expect(
        screen.getByText("skillsPage.discover.installFailed"),
      ).toBeInTheDocument(),
    );
    expect(
      screen.getByRole("button", { name: "skillsPage.discover.retryAria" }),
    ).toBeInTheDocument();
  });

  it("opens a discovered skill's docs in the browser", async () => {
    const open = vi
      .spyOn(settingsApi, "openExternal")
      .mockResolvedValue(undefined as never);
    m.repos = [makeRepo()];
    m.discoverable = [
      makeDiscoverable({
        readmeUrl: "https://github.com/owner-a/repo-a/tree/main/repo-skill",
      }),
    ];
    renderPage();
    await userEvent.click(
      screen.getByRole("button", { name: "mcpPage.openDocs" }),
    );
    expect(open).toHaveBeenLastCalledWith(
      "https://github.com/owner-a/repo-a/tree/main/repo-skill",
    );
    open.mockRestore();
  });

  it("turns a repository off through add_skill_repo with enabled=false", async () => {
    m.repos = [makeRepo(), makeRepo({ owner: "b", name: "c" })];
    m.discoverable = [makeDiscoverable()];
    renderPage();
    await userEvent.click(
      screen.getByRole("button", { name: "skillsPage.filter.button" }),
    );
    const boxes = await screen.findAllByRole("checkbox", {
      name: "skillsPage.repos.enableAria",
    });
    await userEvent.click(boxes[1]);
    await waitFor(() =>
      expect(m.addRepo).toHaveBeenCalledWith({
        owner: "b",
        name: "c",
        branch: "main",
        enabled: false,
      }),
    );
  });

  it("shows empty repositories state with skills.sh shortcut when zero repos are configured", async () => {
    const onOpenSkillsSh = vi.fn();
    renderPage({ source: "repos", onOpenSkillsSh });
    expect(screen.getByText("skills.repo.empty")).toBeInTheDocument();
    expect(
      screen.queryByText("skillsPage.discover.noneRead"),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "skills.sh" }));
    expect(onOpenSkillsSh).toHaveBeenCalledTimes(1);
  });

  it("keeps skills.sh results when Refresh returns identical data and forces page-0 refetch after Load more", async () => {
    const refetchPage0 = vi.fn();
    const refetchPage1 = vi.fn();
    setSearchResult(
      "react",
      0,
      {
        skills: [makeSkillsSh({ key: "p0", name: "Page 0 Skill" })],
        totalCount: 25,
        query: "react",
      },
      { refetch: refetchPage0 },
    );
    setSearchResult(
      "react",
      20,
      {
        skills: [makeSkillsSh({ key: "p1", name: "Page 1 Skill" })],
        totalCount: 25,
        query: "react",
      },
      { refetch: refetchPage1 },
    );

    let handle: {
      canRefresh: boolean;
      refreshing: boolean;
      refresh: () => void;
    } = { canRefresh: false, refreshing: false, refresh: () => undefined };
    renderPage({
      source: "skillssh",
      onSkillsShRefreshChange: (next) => {
        handle = next;
      },
    });

    await userEvent.type(
      screen.getByRole("textbox", { name: "skillsPage.discover.skillsShAria" }),
      "react{Enter}",
    );
    await screen.findByText("Page 0 Skill");
    expect(handle.canRefresh).toBe(true);

    // Refresh on page 0: refetches without wiping results even if data reference is unchanged
    handle.refresh();
    expect(refetchPage0).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Page 0 Skill")).toBeInTheDocument();

    // Load more -> page 1
    await userEvent.click(
      screen.getByRole("button", { name: "skills.skillssh.loadMore" }),
    );
    await screen.findByText("Page 1 Skill");

    // Refresh after Load more: drops back to page 0 and forces a fresh refetch
    handle.refresh();
    await waitFor(() => expect(refetchPage0).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Page 0 Skill")).toBeInTheDocument();
  });

  it("does not duplicate page 2 rows on a same-page background refetch after Load more", async () => {
    const page0 = {
      skills: [makeSkillsSh({ key: "p0", name: "Page 0 Skill" })],
      totalCount: 3,
      query: "bg",
    };
    const page1 = {
      skills: [makeSkillsSh({ key: "p1", name: "Page 1 Skill" })],
      totalCount: 3,
      query: "bg",
    };
    setSearchResult("bg", 0, page0, { dataUpdatedAt: 100 });
    setSearchResult("bg", 20, page1, { dataUpdatedAt: 100 });

    const installTo = vi.fn(
      async (install: (first: AppId) => Promise<Array<{ id: string }>>) => ({
        installed: await install("codex"),
        failures: [],
      }),
    );
    const props: React.ComponentProps<typeof SkillsPage> = {
      source: "skillssh",
      visibleAppIds: ["claude", "codex", "pi"],
      installTargets: ["codex", "claude"],
      onInstallTargetsChange: vi.fn(),
      installTo: installTo as never,
      onShowInstalled: vi.fn(),
      onOpenRepoManager: vi.fn(),
      onEnableFailures: vi.fn(),
    };
    const { rerender } = render(<SkillsPage {...props} />);

    await userEvent.type(
      screen.getByRole("textbox", { name: "skillsPage.discover.skillsShAria" }),
      "bg{Enter}",
    );
    await screen.findByText("Page 0 Skill");

    await userEvent.click(
      screen.getByRole("button", { name: "skills.skillssh.loadMore" }),
    );
    await screen.findByText("Page 1 Skill");
    expect(screen.getAllByText("Page 1 Skill")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "skills.skillssh.loadMore" }),
    ).toBeInTheDocument();

    // Simulate TanStack Query background refetch of page 1 (same data, newer dataUpdatedAt)
    setSearchResult("bg", 20, page1, { dataUpdatedAt: 200 });
    rerender(<SkillsPage {...props} />);

    expect(screen.getAllByText("Page 1 Skill")).toHaveLength(1);
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(
      screen.getByRole("button", { name: "skills.skillssh.loadMore" }),
    ).toBeInTheDocument();
  });
});
