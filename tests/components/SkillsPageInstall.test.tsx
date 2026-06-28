import { createRef } from "react";
import { render, screen, waitFor } from "@testing-library/react";
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

const installMutateAsyncMock = vi.fn();

// Stable cache so repeated renders see referentially-equal data.
// SkillsPage has `useEffect([skillsShResult, ...])` that calls setState — a
// fresh object every render would loop forever.
const searchCache = new Map<
  string,
  { data: SkillsShSearchResult | undefined; isLoading: boolean; isFetching: boolean }
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
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useDiscoverableSkills: () => ({
    data: [] as DiscoverableSkill[],
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  }),
  useInstalledSkills: () => ({
    data: [],
    isLoading: false,
  }),
  useInstallSkill: () => ({
    mutateAsync: installMutateAsyncMock,
  }),
  useSkillRepos: () => ({
    data: [],
    refetch: vi.fn(),
  }),
  useAddSkillRepo: () => ({
    mutateAsync: vi.fn(),
  }),
  useRemoveSkillRepo: () => ({
    mutateAsync: vi.fn(),
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
});
