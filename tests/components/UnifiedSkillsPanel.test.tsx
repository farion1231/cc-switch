import { createRef } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";
import type { InstalledSkill, SkillGroup } from "@/lib/api/skills";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
const createSkillGroupMock = vi.fn();
const renameSkillGroupMock = vi.fn();
const deleteSkillGroupMock = vi.fn();
const setSkillGroupMembersMock = vi.fn();
const batchToggleSkillGroupAppMock = vi.fn();

let installedSkillsMock: InstalledSkill[] = [];
let skillGroupsMock: SkillGroup[] = [];

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
  useSkillGroups: () => ({
    data: skillGroupsMock,
    isLoading: false,
  }),
  useCreateSkillGroup: () => ({
    mutateAsync: createSkillGroupMock,
    isPending: false,
  }),
  useRenameSkillGroup: () => ({
    mutateAsync: renameSkillGroupMock,
    isPending: false,
  }),
  useDeleteSkillGroup: () => ({
    mutateAsync: deleteSkillGroupMock,
    isPending: false,
  }),
  useSetSkillGroupMembers: () => ({
    mutateAsync: setSkillGroupMembersMock,
    isPending: false,
  }),
  useBatchToggleSkillGroupApp: () => ({
    mutateAsync: batchToggleSkillGroupAppMock,
    isPending: false,
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

const makeSkill = (
  id: string,
  overrides: Omit<Partial<InstalledSkill>, "apps"> & {
    apps?: Partial<InstalledSkill["apps"]>;
  } = {},
): InstalledSkill => ({
  id,
  name: id,
  description: "",
  directory: id,
  repoOwner: undefined,
  repoName: undefined,
  repoBranch: undefined,
  readmeUrl: undefined,
  installedAt: 0,
  contentHash: undefined,
  updatedAt: 0,
  ...overrides,
  apps: {
    claude: false,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    ...(overrides.apps ?? {}),
  },
});

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    installedSkillsMock = [];
    skillGroupsMock = [];
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
    createSkillGroupMock.mockReset();
    renameSkillGroupMock.mockReset();
    deleteSkillGroupMock.mockReset();
    setSkillGroupMembersMock.mockReset();
    batchToggleSkillGroupAppMock.mockReset();
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

  it("renders source groups and collapses/expands them", () => {
    installedSkillsMock = [
      makeSkill("skill-a", { name: "Source Skill A", apps: { claude: true } }),
      makeSkill("skill-b", { name: "Source Skill B", apps: { codex: true } }),
    ];
    skillGroupsMock = [
      {
        id: "source:local",
        name: "Local",
        kind: "source",
        editable: false,
        memberSkillIds: ["skill-a", "skill-b"],
        count: 2,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.source"));

    expect(screen.getByText("skills.groups.auto.local")).toBeInTheDocument();
    expect(screen.getByText("Source Skill A")).toBeInTheDocument();
    expect(screen.getByText("Source Skill B")).toBeInTheDocument();

    fireEvent.click(
      screen.getByText("skills.groups.auto.local").closest("button")!,
    );

    expect(screen.queryByText("Source Skill A")).not.toBeInTheDocument();
    expect(screen.queryByText("Source Skill B")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByText("skills.groups.auto.local").closest("button")!,
    );

    expect(screen.getByText("Source Skill A")).toBeInTheDocument();
    expect(screen.getByText("Source Skill B")).toBeInTheDocument();
  });

  it("shows mixed group app state and batch toggles a group app icon", async () => {
    installedSkillsMock = [
      makeSkill("skill-a", { name: "Skill A", apps: { claude: true } }),
      makeSkill("skill-b", { name: "Skill B", apps: { claude: false } }),
    ];
    skillGroupsMock = [
      {
        id: "source:local",
        name: "Local",
        kind: "source",
        editable: false,
        memberSkillIds: ["skill-a", "skill-b"],
        count: 2,
      },
    ];

    const { container } = render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.source"));

    expect(container.querySelector(".bg-yellow-400")).toBeTruthy();

    fireEvent.click(
      screen.getAllByLabelText("skills.groups.toggleAppForGroup")[0],
    );

    await waitFor(() => {
      expect(batchToggleSkillGroupAppMock).toHaveBeenCalledWith({
        groupId: "source:local",
        app: "claude",
        enabled: true,
        skillIds: ["skill-a", "skill-b"],
      });
    });
  });

  it("still renders grouped views even when no installed skill matches the active app filter", () => {
    installedSkillsMock = [makeSkill("skill-a", { name: "Disabled Skill" })];
    skillGroupsMock = [
      {
        id: "source:local",
        name: "Local",
        kind: "source",
        editable: false,
        memberSkillIds: ["skill-a"],
        count: 1,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.source"));

    expect(screen.getByText("skills.groups.auto.local")).toBeInTheDocument();
    expect(
      screen.queryByText("skills.appFilter.noResults"),
    ).not.toBeInTheDocument();
  });

  it("opens manual group member dialog and saves selected members", async () => {
    installedSkillsMock = [makeSkill("skill-a", { name: "Manual Skill A" })];
    skillGroupsMock = [
      {
        id: "manual:g1",
        name: "Team",
        kind: "manual",
        editable: true,
        memberSkillIds: [],
        count: 0,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.manual"));
    fireEvent.click(screen.getByText("skills.groups.members"));
    fireEvent.click(screen.getByRole("checkbox", { name: /Manual Skill A/ }));
    fireEvent.click(screen.getByText("skills.groups.saveMembers"));

    await waitFor(() => {
      expect(setSkillGroupMembersMock).toHaveBeenCalledWith({
        groupId: "manual:g1",
        skillIds: ["skill-a"],
      });
    });
  });

  it("shows ungrouped skills in the synthetic manual fallback group with shared toggle/action slots", () => {
    installedSkillsMock = [
      makeSkill("skill-a", { name: "Grouped Skill" }),
      makeSkill("skill-b", { name: "Ungrouped Skill" }),
    ];
    skillGroupsMock = [
      {
        id: "manual:g1",
        name: "Team",
        kind: "manual",
        editable: true,
        memberSkillIds: ["skill-a"],
        count: 1,
      },
      {
        id: "manual:ungrouped",
        name: "Ungrouped",
        kind: "manual",
        editable: false,
        memberSkillIds: ["skill-b"],
        count: 1,
      },
    ];

    const { container } = render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.manual"));

    expect(
      screen.getByText("skills.groups.auto.ungrouped"),
    ).toBeInTheDocument();
    expect(screen.getByText("Ungrouped Skill")).toBeInTheDocument();
    expect(screen.getAllByText("skills.groups.members")).toHaveLength(1);
    expect(screen.getAllByTestId("group-toggle-slot")).toHaveLength(2);
    expect(screen.getAllByTestId("group-action-slot")).toHaveLength(2);
    expect(screen.getAllByTestId("skill-toggle-slot")).toHaveLength(2);
    expect(screen.getAllByTestId("skill-action-slot")).toHaveLength(2);
    expect(
      container.querySelector(
        '[data-testid="group-action-slot"][data-group-id="manual:ungrouped"]',
      ),
    ).toBeTruthy();
  });

  it("bulk member selection toggles only currently filtered skills from the footer button", async () => {
    installedSkillsMock = [
      makeSkill("skill-a", { name: "Alpha Skill" }),
      makeSkill("skill-b", { name: "Beta Skill" }),
    ];
    skillGroupsMock = [
      {
        id: "manual:g1",
        name: "Team",
        kind: "manual",
        editable: true,
        memberSkillIds: [],
        count: 0,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.manual"));
    fireEvent.click(screen.getByText("skills.groups.members"));
    fireEvent.change(
      screen.getByPlaceholderText("skills.groups.searchMembers"),
      {
        target: { value: "Alpha" },
      },
    );
    fireEvent.click(screen.getByText("skills.groups.selectAll"));
    fireEvent.click(screen.getByText("skills.groups.saveMembers"));

    await waitFor(() => {
      expect(setSkillGroupMembersMock).toHaveBeenCalledWith({
        groupId: "manual:g1",
        skillIds: ["skill-a"],
      });
    });
  });

  it("shows clear-all button after every filtered skill is selected", () => {
    installedSkillsMock = [
      makeSkill("skill-a", { name: "Alpha Skill" }),
      makeSkill("skill-b", { name: "Beta Skill" }),
    ];
    skillGroupsMock = [
      {
        id: "manual:g1",
        name: "Team",
        kind: "manual",
        editable: true,
        memberSkillIds: ["skill-a"],
        count: 2,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.manual"));
    fireEvent.click(screen.getByText("skills.groups.members"));
    fireEvent.change(
      screen.getByPlaceholderText("skills.groups.searchMembers"),
      {
        target: { value: "Alpha" },
      },
    );

    expect(screen.getByText("skills.groups.clearAll")).toBeInTheDocument();
  });

  it("keeps installed skill app icons right-aligned until action buttons are shown", () => {
    installedSkillsMock = [
      makeSkill("skill-a", {
        name: "Right Aligned Skill",
        apps: { claude: true, codex: true },
      }),
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    const toggleSlot = screen.getByTestId("skill-toggle-slot");
    const actionSlot = screen.getByTestId("skill-action-slot");

    expect(toggleSlot.className).toContain("justify-end");
    expect(toggleSlot.className).toContain("group-hover:-translate-x-[40px]");
    expect(actionSlot.className).toContain("absolute");
    expect(actionSlot.className).toContain("right-4");
    expect(actionSlot.className).toContain("pointer-events-none");
  });

  it("keeps group header app icons aligned with skill rows and reveals delete on hover only for editable groups", () => {
    installedSkillsMock = [makeSkill("skill-a", { name: "Grouped Skill" })];
    skillGroupsMock = [
      {
        id: "source:local",
        name: "Local",
        kind: "source",
        editable: false,
        memberSkillIds: ["skill-a"],
        count: 1,
      },
      {
        id: "manual:g1",
        name: "Team",
        kind: "manual",
        editable: true,
        memberSkillIds: ["skill-a"],
        count: 1,
      },
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    fireEvent.click(screen.getByText("skills.groups.views.manual"));

    const manualGroupToggleSlot = screen.getByTestId("group-toggle-slot");
    const manualGroupActionSlot = screen.getByTestId("group-action-slot");
    const memberButton = screen.getByText("skills.groups.members");

    expect(manualGroupToggleSlot.className).toContain("justify-end");
    expect(manualGroupToggleSlot.className).toContain(
      "group-hover:-translate-x-[40px]",
    );
    expect(manualGroupActionSlot.className).toContain("absolute");
    expect(manualGroupActionSlot.className).toContain("right-4");
    expect(manualGroupActionSlot.className).toContain("group-hover:opacity-100");
    expect(memberButton.parentElement?.className).toContain(
      "group-hover:-translate-x-[40px]",
    );
  });
});
