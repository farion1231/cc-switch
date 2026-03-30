import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import { SkillsPage } from "@/components/skills/SkillsPage";

let discoverableSkillsData: any[] = [];
let installedSkillsData: any[] = [];
const installSkillMock = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, ...props }: any) => <button {...props}>{children}</button>,
}));

vi.mock("@/components/ui/input", () => ({
  Input: (props: any) => <input {...props} />,
}));

vi.mock("@/components/ui/select", () => ({
  Select: ({ children }: any) => <div>{children}</div>,
  SelectContent: ({ children }: any) => <div>{children}</div>,
  SelectItem: ({ children }: any) => <div>{children}</div>,
  SelectTrigger: ({ children }: any) => <div>{children}</div>,
  SelectValue: () => null,
}));

vi.mock("@/components/skills/RepoManagerPanel", () => ({
  RepoManagerPanel: () => null,
}));

vi.mock("@/components/skills/SkillCard", () => ({
  SkillCard: ({ skill, onInstall }: any) => (
    <div>
      <span>{skill.name}</span>
      <span>{skill.installed ? "installed" : "uninstalled"}</span>
      <button onClick={() => onInstall(skill)}>{`install-${skill.repoName}`}</button>
    </div>
  ),
}));

vi.mock("@/hooks/useSkills", () => ({
  useDiscoverableSkills: () => ({
    data: discoverableSkillsData,
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  }),
  useInstalledSkills: () => ({
    data: installedSkillsData,
  }),
  useInstallSkill: () => ({
    mutateAsync: installSkillMock,
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
}));

describe("SkillsPage", () => {
  beforeEach(() => {
    installSkillMock.mockReset();
    discoverableSkillsData = [
      {
        key: "owner/repo:superpowers/using-superpowers",
        name: "using-superpowers",
        description: "Nested skill",
        directory: "superpowers/using-superpowers",
        repoOwner: "owner",
        repoName: "repo",
        repoBranch: "main",
      },
    ];
    installedSkillsData = [
      {
        id: "local:superpowers/using-superpowers",
        name: "using-superpowers",
        description: "Nested skill",
        directory: "superpowers/using-superpowers",
        repoOwner: "owner",
        repoName: "repo",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
        },
        installedAt: 1,
      },
    ];
  });

  it("marks nested discoverable skills as installed using the full directory key", () => {
    render(<SkillsPage initialApp="claude" />);

    expect(screen.getByText("using-superpowers")).toBeInTheDocument();
    expect(screen.getByText("installed")).toBeInTheDocument();
  });

  it("installs the exact discoverable skill even when directories are duplicated across repos", async () => {
    installSkillMock.mockResolvedValue({});
    discoverableSkillsData = [
      {
        key: "owner-a/repo-a:shared/skill",
        name: "shared-skill-a",
        description: "Repo A",
        directory: "shared/skill",
        repoOwner: "owner-a",
        repoName: "repo-a",
        repoBranch: "main",
      },
      {
        key: "owner-b/repo-b:shared/skill",
        name: "shared-skill-b",
        description: "Repo B",
        directory: "shared/skill",
        repoOwner: "owner-b",
        repoName: "repo-b",
        repoBranch: "main",
      },
    ];
    installedSkillsData = [];

    render(<SkillsPage initialApp="claude" />);

    fireEvent.click(screen.getByRole("button", { name: "install-repo-b" }));

    await waitFor(() => {
      expect(installSkillMock).toHaveBeenCalledWith({
        skill: expect.objectContaining({
          key: "owner-b/repo-b:shared/skill",
          repoOwner: "owner-b",
          repoName: "repo-b",
        }),
        currentApp: "claude",
      });
    });
  });
});
