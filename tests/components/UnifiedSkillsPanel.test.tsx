import { createRef } from "react";
import {
  render,
  screen,
  waitFor,
  act,
  fireEvent,
} from "@testing-library/react";
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
        description: "Imported from Claude",
        foundIn: ["claude"],
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

function makeInstalledSkill(
  overrides: Partial<InstalledSkill> = {},
): InstalledSkill {
  return {
    id: "skill-a",
    name: "Git Helper",
    description: "Helps with repository workflows",
    directory: "git-helper",
    repoOwner: "farion1231",
    repoName: "cc-switch",
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
    ...overrides,
  };
}

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    installedSkillsMock = [];
    scanUnmanagedMock.mockResolvedValue({
      data: [
        {
          directory: "shared-skill",
          name: "Shared Skill",
          description: "Imported from Claude",
          foundIn: ["claude"],
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
  });

  it("filters installed skills by searchable metadata", () => {
    installedSkillsMock = [
      makeInstalledSkill(),
      makeInstalledSkill({
        id: "skill-b",
        name: "Database Guard",
        description: "Audits database migration safety",
        directory: "db-guard",
        repoOwner: "example",
        repoName: "db-tools",
      }),
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(screen.getByText("Git Helper")).toBeInTheDocument();
    expect(screen.getByText("Database Guard")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "skills.search" }), {
      target: { value: "database" },
    });

    expect(screen.queryByText("Git Helper")).not.toBeInTheDocument();
    expect(screen.getByText("Database Guard")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "skills.search" }), {
      target: { value: "farion1231/cc-switch" },
    });

    expect(screen.getByText("Git Helper")).toBeInTheDocument();
    expect(screen.queryByText("Database Guard")).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "skills.search" }), {
      target: { value: "missing" },
    });

    expect(screen.getByText("skills.noResults")).toBeInTheDocument();
    expect(screen.queryByText("Git Helper")).not.toBeInTheDocument();
    expect(screen.queryByText("Database Guard")).not.toBeInTheDocument();
  });
});
