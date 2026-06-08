import { createRef } from "react";
import { render, screen, waitFor, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  getSkillDropTargetTagIds,
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";
import type { InstalledSkill, SkillTag } from "@/hooks/useSkills";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
let installedSkillsMock: InstalledSkill[] = [];
let skillTagsMock: SkillTag[] = [];
let tagAssignmentsMock: [string, number][] = [];

const createSkill = (overrides: Partial<InstalledSkill>): InstalledSkill => ({
  id: "skill-1",
  name: "Skill One",
  directory: "skill-one",
  apps: {
    claude: false,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
  },
  installedAt: 1,
  updatedAt: 1,
  ...overrides,
});

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
  useSkillTags: () => ({
    data: skillTagsMock,
  }),
  useAllTagAssignments: () => ({
    data: tagAssignmentsMock,
  }),
  useSetSkillTags: () => ({
    mutateAsync: vi.fn(),
  }),
  useUpdateTag: () => ({
    mutateAsync: vi.fn(),
  }),
  useCreateTag: () => ({
    mutateAsync: vi.fn(),
  }),
  useReorderTags: () => ({
    mutateAsync: vi.fn(),
  }),
}));

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    installedSkillsMock = [];
    skillTagsMock = [];
    tagAssignmentsMock = [];
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

  it("keeps empty tags visible in grouped view", async () => {
    installedSkillsMock = [
      createSkill({ id: "skill-1", name: "Grouped Skill" }),
      createSkill({ id: "skill-2", name: "Ungrouped Skill" }),
    ];
    skillTagsMock = [
      { id: 1, name: "密码", sort_index: 0, created_at: 1 },
      { id: 2, name: "空标签", sort_index: 1, created_at: 1 },
    ];
    tagAssignmentsMock = [["skill-1", 1]];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await act(async () => {
      screen.getByTitle("skills.tags.groupedView").click();
    });

    expect(screen.getByText("密码")).toBeInTheDocument();
    expect(screen.getByText("空标签")).toBeInTheDocument();
    expect(screen.getByText("Grouped Skill")).toBeInTheDocument();
    expect(screen.getByText("Ungrouped Skill")).toBeInTheDocument();
  });

  it("resolves ungrouped drop targets to an empty tag assignment", () => {
    const assignments: [string, number][] = [["grouped-skill", 1]];

    expect(getSkillDropTargetTagIds("drop-untagged", assignments)).toEqual([]);
    expect(getSkillDropTargetTagIds("drop-group:-1", assignments)).toEqual([]);
    expect(
      getSkillDropTargetTagIds("skill:ungrouped-skill", assignments),
    ).toEqual([]);
    expect(getSkillDropTargetTagIds("drop-group:2", assignments)).toEqual([2]);
    expect(
      getSkillDropTargetTagIds("skill:grouped-skill", assignments),
    ).toEqual([1]);
  });
});
