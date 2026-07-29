import { createRef } from "react";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

// Radix Select calls scrollIntoView which jsdom doesn't implement.
beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();

// Mutable reference so per-describe blocks can override the installed skills.
let mockInstalledSkills: unknown[] = [];

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: mockInstalledSkills,
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
    mockInstalledSkills = [];
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
    uninstallSkillMock.mockReset();
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
});

const makeSkill = (
  overrides: Partial<{
    id: string;
    name: string;
    description: string;
    repoOwner: string;
    repoName: string;
    directory: string;
  }> = {},
) => ({
  id: overrides.id ?? "skill-alpha",
  name: overrides.name ?? "Alpha Skill",
  description: overrides.description ?? "Alpha description",
  directory: overrides.directory ?? "alpha",
  repoOwner: overrides.repoOwner ?? "octocat",
  repoName: overrides.repoName ?? "alpha-skill",
  repoBranch: "main",
  apps: { claude: true, codex: false, gemini: false },
  installedAt: 1000,
  updatedAt: 1000,
});

describe("UnifiedSkillsPanel – filters", () => {
  beforeEach(() => {
    mockInstalledSkills = [
      makeSkill({
        id: "alpha",
        name: "Alpha Skill",
        description: "Copilot helper",
        repoOwner: "octocat",
        repoName: "alpha-skill",
      }),
      makeSkill({
        id: "beta",
        name: "Beta Skill",
        description: "CI automation",
        repoOwner: "octocat",
        repoName: "beta-skill",
      }),
      makeSkill({
        id: "gamma",
        name: "Gamma Skill",
        description: "Code review bot",
        repoOwner: "another-user",
        repoName: "gamma-skill",
      }),
    ];
    scanUnmanagedMock.mockResolvedValue({ data: [] });
    toggleSkillAppMock.mockReset();
    uninstallSkillMock.mockReset();
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    restoreSkillBackupMock.mockReset();
  });

  it("shows all skills when no filter is active", async () => {
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await waitFor(() => {
      expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
    });
  });

  it("filters skills by keyword in name", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const input = await screen.findByPlaceholderText(
      "skills.searchPlaceholder",
    );
    await user.type(input, "beta");

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.queryByText("Gamma Skill")).not.toBeInTheDocument();
    });
  });

  it("filters skills by keyword in description", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const input = await screen.findByPlaceholderText(
      "skills.searchPlaceholder",
    );
    await user.type(input, "automation");

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.queryByText("Gamma Skill")).not.toBeInTheDocument();
    });
  });

  it("filters skills by keyword in repo", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const input = await screen.findByPlaceholderText(
      "skills.searchPlaceholder",
    );
    await user.type(input, "gamma-skill");

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.queryByText("Beta Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
    });
  });

  it("trims whitespace from keyword search (P2 regression)", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const input = await screen.findByPlaceholderText(
      "skills.searchPlaceholder",
    );
    await user.type(input, "  beta  ");

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.queryByText("Gamma Skill")).not.toBeInTheDocument();
    });
  });

  it("filters skills by author", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    // Open the author select — Radix SelectTrigger is role="combobox"
    const comboboxes = await screen.findAllByRole("combobox");
    const authorTrigger = comboboxes.find((el) =>
      el.textContent?.includes("skills.filter.allAuthors"),
    )!;
    await user.click(authorTrigger);
    await user.click(screen.getByText("another-user"));

    await waitFor(() => {
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
      expect(screen.queryByText("Beta Skill")).not.toBeInTheDocument();
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
    });
  });

  it("filters skills by repo", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    // Open the repo select
    const comboboxes = await screen.findAllByRole("combobox");
    const repoTrigger = comboboxes.find((el) =>
      el.textContent?.includes("skills.filter.allRepos"),
    )!;
    await user.click(repoTrigger);
    // "octocat/alpha-skill" also appears in the skill card; the SelectItem
    // is the second occurrence.
    const repoItems = screen.getAllByText("octocat/alpha-skill");
    await user.click(repoItems[1]);

    await waitFor(() => {
      expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
      expect(screen.queryByText("Beta Skill")).not.toBeInTheDocument();
      expect(screen.queryByText("Gamma Skill")).not.toBeInTheDocument();
    });
  });

  it("shows empty state when filters produce no results", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const input = await screen.findByPlaceholderText(
      "skills.searchPlaceholder",
    );
    await user.type(input, "zzz-no-match");

    await waitFor(() => {
      expect(screen.getByText("skills.noResults")).toBeInTheDocument();
    });
  });

  it("selecting 'all' option after a filter resets to show all skills", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await screen.findByText("Alpha Skill");

    // Filter to another-user
    const comboboxes = screen.getAllByRole("combobox");
    const authorTrigger = comboboxes.find((el) =>
      el.textContent?.includes("skills.filter.allAuthors"),
    )!;
    await user.click(authorTrigger);
    await user.click(screen.getByText("another-user"));

    await waitFor(() => {
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
    });

    // Reset to "all authors"
    const updatedComboboxes = screen.getAllByRole("combobox");
    const updatedAuthorTrigger = updatedComboboxes.find((el) =>
      el.textContent?.includes("another-user"),
    )!;
    await user.click(updatedAuthorTrigger);
    await user.click(screen.getByText("skills.filter.allAuthors"));

    await waitFor(() => {
      expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
    });
  });

  it("clears author filter when the selected author has no remaining skills", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await screen.findByText("Alpha Skill");

    // Filter to another-user
    const comboboxes = screen.getAllByRole("combobox");
    const authorTrigger = comboboxes.find((el) =>
      el.textContent?.includes("skills.filter.allAuthors"),
    )!;
    await user.click(authorTrigger);
    await user.click(screen.getByText("another-user"));

    await waitFor(() => {
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
    });

    // Simulate uninstalling the last skill from another-user
    mockInstalledSkills = [
      makeSkill({
        id: "alpha",
        name: "Alpha Skill",
        description: "Copilot helper",
        repoOwner: "octocat",
        repoName: "alpha-skill",
      }),
      makeSkill({
        id: "beta",
        name: "Beta Skill",
        description: "CI automation",
        repoOwner: "octocat",
        repoName: "beta-skill",
      }),
    ];

    rerender(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    // Filter should be cleared — all remaining skills visible
    await waitFor(() => {
      expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
    });
  });

  it("filter controls have accessible names", async () => {
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await screen.findByText("Alpha Skill");

    // Search input
    const searchInput = screen.getByRole("textbox", {
      name: /skills\.searchPlaceholder/i,
    });
    expect(searchInput).toHaveAttribute(
      "placeholder",
      "skills.searchPlaceholder",
    );

    // Author select (combobox) — use role + aria-label
    const authorSelect = screen.getByRole("combobox", {
      name: /skills\.filter\.author/i,
    });
    expect(authorSelect).toBeInTheDocument();

    // Repo select (combobox) — use role + aria-label
    const repoSelect = screen.getByRole("combobox", {
      name: /skills\.filter\.repo/i,
    });
    expect(repoSelect).toBeInTheDocument();
  });

  it("clears all filters when the clear button is clicked", async () => {
    const user = userEvent.setup();
    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await screen.findByText("Alpha Skill");

    // Apply keyword filter
    const searchInput = screen.getByRole("textbox", {
      name: /skills\.searchPlaceholder/i,
    });
    await user.type(searchInput, "beta");

    await waitFor(() => {
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.queryByText("Alpha Skill")).not.toBeInTheDocument();
    });

    // Click clear button
    await user.click(screen.getByText("skills.filter.clearFilter"));

    // All skills should be visible again
    await waitFor(() => {
      expect(screen.getByText("Alpha Skill")).toBeInTheDocument();
      expect(screen.getByText("Beta Skill")).toBeInTheDocument();
      expect(screen.getByText("Gamma Skill")).toBeInTheDocument();
    });

    // Input should be empty
    expect(searchInput).toHaveValue("");
  });
});
