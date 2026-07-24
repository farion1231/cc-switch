import { createRef } from "react";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";
import type { InstalledSkill } from "@/lib/api/skills";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
let installedSkillsMock: InstalledSkill[] = [];
let installedSkillContentsMock: Record<string, string> = {};

function createInstalledSkill(
  id: string,
  name: string,
  description: string,
  repoOwner?: string,
): InstalledSkill {
  return {
    id,
    name,
    description,
    directory: id,
    repoOwner,
    repoName: repoOwner ? "skills-repo" : undefined,
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
    installedAt: 0,
    updatedAt: 0,
  };
}

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: installedSkillsMock,
    isLoading: false,
  }),
  useInstalledSkillContents: () => ({
    data: installedSkillContentsMock,
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
    installedSkillsMock = [];
    installedSkillContentsMock = {};
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

  it("filters installed skills across all supported fields but not repository", async () => {
    installedSkillsMock = [
      createInstalledSkill(
        "alpha",
        "Alpha Helper",
        "General utilities",
        "owner-a",
      ),
      createInstalledSkill(
        "beta",
        "Beta Helper",
        "Database cleanup",
        "owner-b",
      ),
      createInstalledSkill(
        "gamma",
        "Gamma Helper",
        "Deployment utilities",
        "exclusive-owner",
      ),
    ];
    installedSkillContentsMock = {
      alpha: "Use this skill for frontend reviews.",
      beta: "Use this skill for database maintenance.",
      gamma: "Run a Kubernetes rollout safely.",
    };

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const user = userEvent.setup();
    const search = screen.getByPlaceholderText(
      "skills.installedSearchPlaceholder.all",
    );

    await user.type(search, "alpha");
    expect(screen.getByText("Alpha Helper")).toBeInTheDocument();
    expect(screen.queryByText("Beta Helper")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "cleanup");
    expect(screen.getByText("Beta Helper")).toBeInTheDocument();
    expect(screen.queryByText("Alpha Helper")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "kubernetes rollout");
    expect(screen.getByText("Gamma Helper")).toBeInTheDocument();
    expect(screen.queryByText("Beta Helper")).not.toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "exclusive-owner");
    expect(screen.getByText("skills.noResults")).toBeInTheDocument();
    expect(screen.queryByText("Gamma Helper")).not.toBeInTheDocument();
  });

  it("matches English names independently of the system locale", async () => {
    installedSkillsMock = [
      createInstalledSkill(
        "installed-helper",
        "Installed Helper",
        "General utilities",
      ),
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const originalToLocaleLowerCase = String.prototype.toLocaleLowerCase;
    const localeLowerCaseSpy = vi
      .spyOn(String.prototype, "toLocaleLowerCase")
      .mockImplementation(function (this: string) {
        return originalToLocaleLowerCase.call(this, "tr-TR");
      });

    try {
      const user = userEvent.setup();
      const search = screen.getByPlaceholderText(
        "skills.installedSearchPlaceholder.all",
      );
      await user.type(search, "installed");

      expect(screen.getByText("Installed Helper")).toBeInTheDocument();
    } finally {
      localeLowerCaseSpy.mockRestore();
    }
  });

  it("lets users limit installed skill search to name or content", async () => {
    installedSkillsMock = [
      createInstalledSkill("alpha", "Alpha Helper", "General utilities"),
      createInstalledSkill("beta", "Beta Helper", "Alpha database cleanup"),
    ];
    installedSkillContentsMock = {
      alpha: "Review frontend code.",
      beta: "Run maintenance safely.",
    };

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const user = userEvent.setup();
    const search = screen.getByPlaceholderText(
      "skills.installedSearchPlaceholder.all",
    );

    await user.click(screen.getByText("skills.installedSearchScope.name"));
    expect(search).toHaveAttribute(
      "placeholder",
      "skills.installedSearchPlaceholder.name",
    );
    await user.type(search, "alpha");
    expect(screen.getByText("Alpha Helper")).toBeInTheDocument();
    expect(screen.queryByText("Beta Helper")).not.toBeInTheDocument();

    await user.click(screen.getByText("skills.installedSearchScope.content"));
    expect(search).toHaveAttribute(
      "placeholder",
      "skills.installedSearchPlaceholder.content",
    );
    expect(screen.queryByText("Alpha Helper")).not.toBeInTheDocument();
    expect(screen.getByText("Beta Helper")).toBeInTheDocument();

    await user.clear(search);
    await user.type(search, "frontend code");
    expect(screen.getByText("Alpha Helper")).toBeInTheDocument();
    expect(screen.queryByText("Beta Helper")).not.toBeInTheDocument();
  });
});
