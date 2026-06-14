import { createRef, type ReactNode } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import {
  SkillsPage,
  type SkillsPageHandle,
} from "@/components/skills/SkillsPage";
import type {
  DiscoverableSkill,
  SkillsShDiscoverableSkill,
  SkillsShSearchResult,
} from "@/lib/api/skills";
import {
  applySkillDiscoveryProgress,
  beginSkillDiscovery,
  getSkillDiscoveryTaskSnapshot,
  resetSkillDiscoveryTask,
} from "@/stores/skillDiscoveryTask";

const installMutateAsyncMock = vi.fn();
const addRepoMutateAsyncMock = vi.fn();
const retryRepoMutateAsyncMock = vi.fn();
const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
let discoverableSkillsMock: DiscoverableSkill[] = [];
let discoveryFailuresMock: Array<{
  owner: string;
  name: string;
  branch: string;
  error: string;
}> = [];
let skillReposMock: Array<{
  owner: string;
  name: string;
  branch: string;
  enabled: boolean;
}> = [];
let discoveryLoadingMock = false;
let discoveryErrorMock: Error | null = null;
let reposLoadingMock = false;
const refetchDiscoverableMock = vi.fn();
const forceRefetchDiscoverableMock = vi.fn();
const selectValueRenderMock = vi.fn();
// Stable cache so repeated renders see referentially-equal data.
// SkillsPage has `useEffect([skillsShResult, ...])` that calls setState — a
// fresh object every render would loop forever.
const searchCache = new Map<
  string,
  {
    data: SkillsShSearchResult | undefined;
    isLoading: boolean;
    isFetching: boolean;
  }
>();

const setSearchResult = (
  query: string,
  offset: number,
  result: SkillsShSearchResult | undefined,
) => {
  searchCache.set(`${query}:${offset}`, {
    data: result,
    isLoading: false,
    isFetching: false,
  });
};

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: vi.fn(),
  },
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({
    children,
    value,
    onValueChange,
  }: {
    children: ReactNode;
    value?: string;
    onValueChange?: (value: string) => void;
  }) => (
    <select
      value={value}
      onChange={(event) => onValueChange?.(event.target.value)}
    >
      {children}
    </select>
  ),
  SelectTrigger: ({ children }: { children: ReactNode }) => <>{children}</>,
  SelectValue: (props: { children?: ReactNode; placeholder?: string }) => {
    selectValueRenderMock(props);
    return null;
  },
  SelectContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  SelectItem: ({ value }: { value: string; children: ReactNode }) => (
    <option value={value}>{value}</option>
  ),
}));

vi.mock("@/hooks/useSkills", () => ({
  useDiscoverableSkills: () => ({
    data: { skills: discoverableSkillsMock, failures: discoveryFailuresMock },
    isLoading: discoveryLoadingMock,
    isFetching: discoveryLoadingMock,
    isError: discoveryErrorMock !== null,
    error: discoveryErrorMock,
    refetch: refetchDiscoverableMock,
    forceRefetch: forceRefetchDiscoverableMock,
  }),
  useInstalledSkills: () => ({
    data: [],
    isLoading: false,
  }),
  useInstallSkill: () => ({
    mutateAsync: installMutateAsyncMock,
  }),
  useSkillRepos: () => ({
    data: skillReposMock,
    isLoading: reposLoadingMock,
    refetch: vi.fn(),
  }),
  useAddSkillRepo: () => ({
    mutateAsync: addRepoMutateAsyncMock,
  }),
  useRemoveSkillRepo: () => ({
    mutateAsync: vi.fn(),
  }),
  useRetrySkillRepo: () => ({
    mutateAsync: retryRepoMutateAsyncMock,
    isPending: false,
    variables: undefined,
  }),
  useSearchSkillsSh: (query: string, _limit: number, offset: number) => {
    const cached = searchCache.get(`${query}:${offset}`);
    if (cached) return cached;
    return { data: undefined, isLoading: false, isFetching: false };
  },
}));

const makeSkillsShSkill = (
  overrides: Partial<SkillsShDiscoverableSkill> = {},
): SkillsShDiscoverableSkill => ({
  key: "agent-browser:owner-a:repo-a",
  name: "Agent Browser",
  directory: "agent-browser",
  repoOwner: "owner-a",
  repoName: "repo-a",
  repoBranch: "main",
  installs: 100,
  readmeUrl: "https://example.com/a",
  ...overrides,
});

describe("SkillsPage - skills.sh install (regression)", () => {
  beforeEach(() => {
    installMutateAsyncMock.mockReset();
    installMutateAsyncMock.mockResolvedValue({});
    addRepoMutateAsyncMock.mockReset();
    addRepoMutateAsyncMock.mockResolvedValue(true);
    retryRepoMutateAsyncMock.mockReset();
    retryRepoMutateAsyncMock.mockResolvedValue({ skills: [], failures: [] });
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    discoverableSkillsMock = [
      {
        key: "owner-a/repo-a:skill-a",
        name: "Skill A",
        description: "",
        directory: "skill-a",
        repoOwner: "owner-a",
        repoName: "repo-a",
        repoBranch: "main",
      },
    ];
    discoveryFailuresMock = [];
    discoveryLoadingMock = false;
    discoveryErrorMock = null;
    reposLoadingMock = false;
    refetchDiscoverableMock.mockReset();
    refetchDiscoverableMock.mockResolvedValue({
      data: { skills: discoverableSkillsMock, failures: [] },
    });
    forceRefetchDiscoverableMock.mockReset();
    forceRefetchDiscoverableMock.mockResolvedValue({
      data: { skills: discoverableSkillsMock, failures: [] },
    });
    selectValueRenderMock.mockClear();
    resetSkillDiscoveryTask();
    skillReposMock = [];
    searchCache.clear();
  });

  it("installs the second skill when two results share the same directory", async () => {
    const first = makeSkillsShSkill({
      key: "agent-browser:owner-a:repo-a",
      name: "Agent Browser A",
      repoOwner: "owner-a",
      repoName: "repo-a",
    });
    const second = makeSkillsShSkill({
      key: "agent-browser:owner-b:repo-b",
      name: "Agent Browser B",
      repoOwner: "owner-b",
      repoName: "repo-b",
    });

    setSearchResult("agent", 0, {
      skills: [first, second],
      totalCount: 2,
      query: "agent",
    });

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    const user = userEvent.setup();

    // Switch to skills.sh source
    await user.click(screen.getByRole("button", { name: /skills\.sh/i }));

    // Type a query and submit
    const input = screen.getByPlaceholderText(
      "skills.skillssh.searchPlaceholder",
    );
    await user.type(input, "agent");
    await user.click(screen.getByRole("button", { name: "skills.search" }));

    // Wait for both cards to render
    await waitFor(() => {
      expect(screen.getByText("Agent Browser A")).toBeInTheDocument();
      expect(screen.getByText("Agent Browser B")).toBeInTheDocument();
    });

    // Click install on the SECOND card (Agent Browser B)
    const secondCard = screen
      .getByText("Agent Browser B")
      .closest("div.glass-card");
    expect(secondCard).not.toBeNull();
    const installButton = secondCard!.querySelector(
      "button:last-of-type",
    ) as HTMLButtonElement;
    expect(installButton).not.toBeNull();
    await user.click(installButton);

    // Verify the SECOND skill was passed to the install mutation, not the first
    await waitFor(() => {
      expect(installMutateAsyncMock).toHaveBeenCalledTimes(1);
    });
    const callArgs = installMutateAsyncMock.mock.calls[0][0];
    expect(callArgs.skill.repoOwner).toBe("owner-b");
    expect(callArgs.skill.repoName).toBe("repo-b");
    expect(callArgs.skill.name).toBe("Agent Browser B");
  });

  it("shows configured repositories in the repository filter even when discovery returned no skills", async () => {
    skillReposMock = [
      {
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];
    discoverableSkillsMock = [];

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    expect(screen.getByText("JimLiu/baoyu-skills")).toBeInTheDocument();
    expect(screen.getByText("anthropics/skills")).toBeInTheDocument();
  });

  it("shows an independent icon-only retry button for every repository", async () => {
    skillReposMock = [
      {
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];
    discoveryFailuresMock = [
      {
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
        error: '{"code":"DOWNLOAD_TIMEOUT","context":{}}',
      },
    ];

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    act(() => ref.current?.openRepoManager());
    const user = userEvent.setup();
    const retryButtons = await screen.findAllByRole("button", {
      name: "skills.repo.retry",
    });
    expect(retryButtons).toHaveLength(2);
    expect(screen.queryByText("common.retry")).not.toBeInTheDocument();

    await user.click(retryButtons[0]);

    expect(retryRepoMutateAsyncMock).toHaveBeenCalledTimes(1);
    expect(retryRepoMutateAsyncMock).toHaveBeenCalledWith(
      expect.objectContaining({
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
      }),
    );
  });

  it("shows repository connection information on the very first render", () => {
    discoverableSkillsMock = [];
    discoveryLoadingMock = false;
    reposLoadingMock = true;
    skillReposMock = [];

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    expect(
      screen.getByText("skills.discoveryInitialConnecting"),
    ).toBeInTheDocument();
  });

  it("does not keep the initial connection spinner after the initial query fails", async () => {
    discoverableSkillsMock = [];
    discoveryErrorMock = new Error("backend unavailable");
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];

    render(<SkillsPage initialApp="claude" />);

    await waitFor(() =>
      expect(
        screen.queryByText("skills.discoveryInitialConnectingCount"),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByText("skills.loadFailed")).toBeInTheDocument();
  });

  it("shows the repository count after repository configuration loads", () => {
    discoverableSkillsMock = [];
    discoveryLoadingMock = true;
    beginSkillDiscovery();
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
        enabled: true,
      },
    ];

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    expect(
      screen.getByText("skills.discoveryInitialConnectingCount"),
    ).toBeInTheDocument();
  });

  it("keeps the initial connection message briefly before revealing the first cards", async () => {
    vi.useFakeTimers();
    discoverableSkillsMock = [];
    discoveryLoadingMock = true;
    beginSkillDiscovery();
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "obra",
        name: "superpowers",
        branch: "main",
        enabled: true,
      },
    ];

    render(<SkillsPage initialApp="claude" />);

    act(() => {
      applySkillDiscoveryProgress({
        phase: "completed",
        completed: 1,
        total: 2,
        repo: "anthropics/skills",
        skillCount: 1,
        skills: [
          {
            key: "anthropics/skills:frontend-design",
            name: "Frontend Design",
            description: "Design interfaces",
            directory: "frontend-design",
            repoOwner: "anthropics",
            repoName: "skills",
            repoBranch: "main",
          },
        ],
      });
    });

    expect(
      screen.getByText("skills.discoveryInitialConnectingCount"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Frontend Design")).not.toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(800);
    });

    expect(screen.getByText("Frontend Design")).toBeInTheDocument();
    vi.useRealTimers();
  });

  it("marks a manual refresh active synchronously before the query refetch starts", () => {
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];
    refetchDiscoverableMock.mockReturnValue(new Promise(() => {}));

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    act(() => {
      ref.current?.refresh();
    });

    expect(getSkillDiscoveryTaskSnapshot().active).toBe(true);
  });

  it("does not start a second refresh before the first click rerenders the page", () => {
    refetchDiscoverableMock.mockReturnValue(new Promise(() => {}));

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    act(() => {
      void ref.current?.refresh();
      void ref.current?.refresh();
    });

    expect(refetchDiscoverableMock).toHaveBeenCalledTimes(1);
  });

  it("shows a persistent error when a manual refresh fails", async () => {
    refetchDiscoverableMock.mockRejectedValueOnce(
      new Error('{"code":"NETWORK_ERROR","context":{}}'),
    );

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    await act(async () => {
      await ref.current?.refresh();
    });

    expect(toastErrorMock).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({
        closeButton: true,
        duration: Infinity,
      }),
    );
  });

  it("can install a skill card revealed by repository progress before the final result", async () => {
    vi.useFakeTimers();
    discoverableSkillsMock = [];
    discoveryLoadingMock = true;
    beginSkillDiscovery();
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];

    render(<SkillsPage initialApp="claude" />);
    act(() => {
      applySkillDiscoveryProgress({
        phase: "completed",
        completed: 1,
        total: 1,
        repo: "anthropics/skills",
        skillCount: 1,
        skills: [
          {
            key: "anthropics/skills:frontend-design",
            name: "Frontend Design",
            description: "Design interfaces",
            directory: "frontend-design",
            repoOwner: "anthropics",
            repoName: "skills",
            repoBranch: "main",
          },
        ],
      });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(800);
    });
    vi.useRealTimers();

    const card = screen.getByText("Frontend Design").closest("div.glass-card");
    const installButton = card!.querySelector(
      "button:last-of-type",
    ) as HTMLButtonElement;
    await userEvent.click(installButton);

    expect(installMutateAsyncMock).toHaveBeenCalledWith(
      expect.objectContaining({
        skill: expect.objectContaining({
          repoOwner: "anthropics",
          name: "Frontend Design",
        }),
      }),
    );
  });

  it("does not restore skills from repositories that are no longer configured", () => {
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];
    discoverableSkillsMock = [];

    beginSkillDiscovery();
    applySkillDiscoveryProgress({
      phase: "completed",
      completed: 1,
      total: 2,
      repo: "removed/repo",
      skillCount: 1,
      skills: [
        {
          key: "removed/repo:stale-skill",
          name: "Stale Skill",
          description: "Must not return after deletion",
          directory: "stale-skill",
          repoOwner: "removed",
          repoName: "repo",
          repoBranch: "main",
        },
      ],
    });

    render(<SkillsPage initialApp="claude" />);

    expect(screen.queryByText("Stale Skill")).not.toBeInTheDocument();
  });

  it("keeps cached skill cards visible while repository configuration is loading", () => {
    reposLoadingMock = true;
    skillReposMock = [];
    discoverableSkillsMock = [
      {
        key: "anthropics/skills:frontend-design",
        name: "Frontend Design",
        description: "Cached skill",
        directory: "frontend-design",
        repoOwner: "anthropics",
        repoName: "skills",
        repoBranch: "main",
      },
    ];

    render(<SkillsPage initialApp="claude" />);

    expect(screen.getByText("Frontend Design")).toBeInTheDocument();
  });

  it("resets the repository filter when the selected repository is removed", async () => {
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "removed",
        name: "repo",
        branch: "main",
        enabled: true,
      },
    ];

    const { rerender } = render(<SkillsPage initialApp="claude" />);
    const user = userEvent.setup();
    const repoFilter = screen.getAllByRole("combobox")[0];

    await user.selectOptions(repoFilter, "removed/repo");
    expect(repoFilter).toHaveValue("removed/repo");

    skillReposMock = skillReposMock.filter(
      (repo) => `${repo.owner}/${repo.name}` !== "removed/repo",
    );
    rerender(<SkillsPage initialApp="claude" />);

    await waitFor(() => expect(repoFilter).toHaveValue("all"));
  });

  it("shows repository names inside the initial connection message", () => {
    discoverableSkillsMock = [];
    discoveryLoadingMock = true;
    beginSkillDiscovery();
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "JimLiu",
        name: "baoyu-skills",
        branch: "main",
        enabled: true,
      },
      {
        owner: "obra",
        name: "superpowers",
        branch: "main",
        enabled: true,
      },
      {
        owner: "cexll",
        name: "myclaude",
        branch: "master",
        enabled: true,
      },
    ];

    render(<SkillsPage initialApp="claude" />);

    const connection = screen.getByRole("status");
    expect(
      within(connection).getByText("anthropics/skills"),
    ).toBeInTheDocument();
    expect(
      within(connection).getByText("JimLiu/baoyu-skills"),
    ).toBeInTheDocument();
    expect(within(connection).getByText("cexll/myclaude")).toBeInTheDocument();
    expect(within(connection).queryByText("obra/superpowers")).toBeNull();
  });

  it("keeps repository status icons out of the selected filter value", () => {
    skillReposMock = [
      {
        owner: "anthropics",
        name: "skills",
        branch: "main",
        enabled: true,
      },
    ];

    render(<SkillsPage initialApp="claude" />);

    const selectedValueProps = selectValueRenderMock.mock.calls
      .map(([props]) => props)
      .find((props) => props.placeholder === "skills.filter.repo");
    expect(selectedValueProps?.children).toBe("skills.filter.allRepos");
  });

  it("renders large repository results in batches to keep the first frame responsive", async () => {
    skillReposMock = [
      {
        owner: "large",
        name: "repo",
        branch: "main",
        enabled: true,
      },
    ];
    discoverableSkillsMock = Array.from({ length: 75 }, (_, index) => ({
      key: `large/repo:skill-${index}`,
      name: `Large Skill ${index}`,
      description: "",
      directory: `skill-${index}`,
      repoOwner: "large",
      repoName: "repo",
      repoBranch: "main",
    }));

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(48);
    const loadMore = screen.getByRole("button", {
      name: "skills.discoveryLoadMore",
    });

    await userEvent.click(loadMore);

    expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(75);
    expect(
      screen.queryByRole("button", { name: "skills.discoveryLoadMore" }),
    ).toBeNull();

    await act(async () => {
      await ref.current?.refresh();
    });

    expect(screen.getAllByRole("heading", { level: 3 })).toHaveLength(48);
  });

  it("shows repository add discovery failures instead of a zero-count success", async () => {
    retryRepoMutateAsyncMock.mockResolvedValue({
      skills: [],
      failures: [
        {
          owner: "missing",
          name: "repo",
          branch: "main",
          error: '{"code":"NO_SKILLS_IN_ZIP","context":{}}',
        },
      ],
    });

    const ref = createRef<SkillsPageHandle>();
    render(<SkillsPage ref={ref} initialApp="claude" />);

    act(() => ref.current?.openRepoManager());
    const user = userEvent.setup();
    await user.type(screen.getByLabelText("skills.repo.url"), "missing/repo");
    await user.click(screen.getByRole("button", { name: "skills.repo.add" }));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "skills.repo.addDiscoveryFailed",
        expect.objectContaining({
          description: expect.anything(),
        }),
      ),
    );
    expect(toastSuccessMock).not.toHaveBeenCalledWith(
      "skills.repo.addSuccess",
      expect.anything(),
      expect.anything(),
    );
  });
});
