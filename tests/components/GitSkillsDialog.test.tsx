import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { GitSkillsDialog } from "@/components/skills/GitSkillsDialog";
import type { InstalledSkill } from "@/lib/api/skills";

const mocks = vi.hoisted(() => ({
  add: vi.fn(),
  saved: vi.fn(),
  batch: vi.fn(),
  getRepos: vi.fn(),
  installed: [] as Partial<InstalledSkill>[],
}));
vi.mock("@/hooks/useSkills", () => ({
  useDiscoverAndAddSkillRepo: () => ({ mutateAsync: mocks.add }),
  useDiscoverSkillRepo: () => ({ mutateAsync: mocks.saved }),
  useSkillRepos: () => ({
    data: [],
    refetch: async () => ({ data: await mocks.getRepos() }),
  }),
  useInstallRepoSkills: () => ({ mutateAsync: mocks.batch }),
  useInstalledSkills: () => ({ data: mocks.installed }),
}));
vi.mock("@/lib/api/skills", () => ({
  skillsApi: { getRepos: mocks.getRepos },
}));

function fetchRepo(url: string) {
  render(
    <GitSkillsDialog
      appIds={["codex"]}
      targets={["codex"]}
      setTargets={vi.fn()}
      installTo={async (install) => ({
        installed: await install("codex"),
        failures: [],
      })}
      onClose={vi.fn()}
    />,
  );
  const input = screen.getByLabelText("skills.repo.url");
  fireEvent.change(input, { target: { value: url } });
  fireEvent.keyDown(input, { key: "Enter" });
}

describe("Git repository picker", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.getRepos.mockResolvedValue([]);
    mocks.installed = [];
  });
  it("rejects a saved owner/name on another host before fetching", async () => {
    mocks.getRepos.mockResolvedValue([
      { owner: "team", name: "skills", host: "gitlab.com" },
    ]);
    fetchRepo("https://github.com/team/skills");
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "skills.github.hostConflict",
    );
    expect(mocks.add).not.toHaveBeenCalled();
    expect(mocks.saved).not.toHaveBeenCalled();
  });
  it("sends one batch, reserves colliding destinations and shows individual outcomes", async () => {
    const skills = ["one/demo", "two/demo", "broken"].map((directory) => ({
      directory,
      key: directory,
      name: directory,
      repoOwner: "team",
      repoName: "skills",
      repoBranch: "main",
      description: "",
    }));
    mocks.add.mockResolvedValue(["main", { skills, failures: [] }]);
    mocks.batch.mockResolvedValue([
      { Ok: { id: "one/demo" } },
      { Err: "download failed" },
    ]);
    fetchRepo("https://github.com/team/skills");
    fireEvent.click(await screen.findByText("skills.github.selectAll"));
    fireEvent.click(screen.getByText("skills.github.install"));
    expect(
      await screen.findByText("skillsPage.discover.installed"),
    ).toBeVisible();
    expect(screen.getByText("download failed")).toBeVisible();
    expect(screen.getByText("skills.github.occupied")).toBeVisible();
    expect(mocks.batch.mock.calls).toEqual([
      [{ skills: [skills[0], skills[2]], currentApp: "codex" }],
    ]);
  });
  it("re-fetches a saved fallback using its configured branch", async () => {
    const repo = {
      owner: "team",
      name: "skills",
      branch: "main",
      enabled: true,
    };
    mocks.getRepos.mockResolvedValue([repo]);
    mocks.saved.mockResolvedValue({
      discovery: {
        skills: [
          {
            key: "demo",
            name: "demo",
            directory: "demo",
            repoOwner: "team",
            repoName: "skills",
            repoBranch: "master",
            description: "",
          },
        ],
        failures: [],
      },
    });
    fetchRepo("https://github.com/team/skills");
    await screen.findByText("skills.github.branchFallback");
    fireEvent.click(screen.getByText("skills.github.fetch"));
    await vi.waitFor(() => expect(mocks.saved).toHaveBeenCalledTimes(2));
    expect(mocks.saved.mock.calls.map(([value]) => value.branch)).toEqual([
      "main",
      "main",
    ]);
  });
  it.each([
    ["github.com", "blob"],
    ["gitlab.com", "-/blob"],
    ["codeberg.org", "src/branch"],
  ])(
    "shows the resolved branch and distinguishes identical destination names on %s",
    async (host, marker) => {
      const url = "https://" + host + "/team/skills";
      const doc = (branch: string, folder: string) =>
        url + "/" + marker + "/" + branch + "/" + folder + "/demo/SKILL.md";
      mocks.installed = [
        {
          id: "team/skills:demo",
          directory: "demo",
          repoHost: host,
          repoOwner: "team",
          repoName: "skills",
          repoBranch: "old",
          readmeUrl: doc("old", "one"),
        },
      ];
      const skills = ["one", "two"].map((folder) => ({
        key: "team/skills:" + folder + "/demo",
        name: folder,
        directory: folder + "/demo",
        description: "",
        repoHost: host,
        repoOwner: "team",
        repoName: "skills",
        repoBranch: "master",
        readmeUrl: doc("master", folder),
      }));
      mocks.add.mockResolvedValue(["master", { skills, failures: [] }]);
      fetchRepo(url);
      const rows = await screen.findAllByRole("listitem");
      expect(screen.getByLabelText("skills.repo.branch")).toHaveValue("master");
      expect(
        within(rows[0]).getByText("skillsPage.discover.installed"),
      ).toBeVisible();
      expect(within(rows[1]).getByText("skills.github.occupied")).toBeVisible();
      expect(mocks.add).toHaveBeenCalledTimes(1);
    },
  );
});
