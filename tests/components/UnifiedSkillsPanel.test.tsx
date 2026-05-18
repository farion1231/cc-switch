import { createRef } from "react";
import { render, screen, waitFor, act, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";
import type { InstalledSkill, SkillUpdateInfo } from "@/lib/api/skills";

// Hoisted mock state so vi.mock factories can reference it.
const mocks = vi.hoisted(() => {
  return {
    installed: [] as InstalledSkill[],
    updates: [] as SkillUpdateInfo[],
    scanUnmanagedMock: vi.fn(),
    toggleSkillAppMock: vi.fn(),
    uninstallSkillMock: vi.fn(),
    importSkillsMock: vi.fn(),
    installFromZipMock: vi.fn(),
    deleteSkillBackupMock: vi.fn(),
    restoreSkillBackupMock: vi.fn(),
    setPinMutateMock: vi.fn(),
    updateSkillMock: vi.fn(),
  };
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
    data: mocks.installed,
    isLoading: false,
  }),
  useSkillBackups: () => ({
    data: [],
    refetch: vi.fn(),
    isFetching: false,
  }),
  useDeleteSkillBackup: () => ({
    mutateAsync: mocks.deleteSkillBackupMock,
    isPending: false,
  }),
  useToggleSkillApp: () => ({
    mutateAsync: mocks.toggleSkillAppMock,
  }),
  useRestoreSkillBackup: () => ({
    mutateAsync: mocks.restoreSkillBackupMock,
    isPending: false,
  }),
  useUninstallSkill: () => ({
    mutateAsync: mocks.uninstallSkillMock,
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
    refetch: mocks.scanUnmanagedMock,
  }),
  useImportSkillsFromApps: () => ({
    mutateAsync: mocks.importSkillsMock,
  }),
  useInstallSkillsFromZip: () => ({
    mutateAsync: mocks.installFromZipMock,
  }),
  useCheckSkillUpdates: () => ({
    data: mocks.updates,
    refetch: vi.fn(),
    isFetching: false,
  }),
  useUpdateSkill: () => ({
    mutateAsync: mocks.updateSkillMock,
    isPending: false,
  }),
  useSetSkillPin: () => ({
    mutate: mocks.setPinMutateMock,
    mutateAsync: vi.fn(),
    isPending: false,
  }),
}));

function skill(overrides: Partial<InstalledSkill> = {}): InstalledSkill {
  return {
    id: "s1",
    name: "Skill 1",
    directory: "skill-1",
    repoOwner: "forrest",
    repoName: "kit",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
    installedAt: 1000,
    updatedAt: 0,
    ...overrides,
  };
}

function renderPanel() {
  const ref = createRef<UnifiedSkillsPanelHandle>();
  const utils = render(
    <UnifiedSkillsPanel
      ref={ref}
      onOpenDiscovery={() => {}}
      currentApp="claude"
    />,
  );
  return { ref, ...utils };
}

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
    mocks.installed = [];
    mocks.updates = [];
    mocks.scanUnmanagedMock.mockResolvedValue({
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
    mocks.toggleSkillAppMock.mockReset();
    mocks.uninstallSkillMock.mockReset();
    mocks.importSkillsMock.mockReset();
    mocks.installFromZipMock.mockReset();
    mocks.deleteSkillBackupMock.mockReset();
    mocks.restoreSkillBackupMock.mockReset();
    mocks.setPinMutateMock.mockReset();
    mocks.updateSkillMock.mockReset();
  });

  it("opens the import dialog without crashing when app toggles render", async () => {
    const { ref } = renderPanel();

    await act(async () => {
      await ref.current?.openImport();
    });

    await waitFor(() => {
      expect(screen.getByText("skills.import")).toBeInTheDocument();
      expect(screen.getByText("Shared Skill")).toBeInTheDocument();
      expect(screen.getByText("/tmp/shared-skill")).toBeInTheDocument();
    });
  });

  it("renders the empty state when no skills are installed", () => {
    mocks.installed = [];
    renderPanel();
    expect(screen.getByText("skills.noInstalled")).toBeInTheDocument();
  });

  it("renders installed skills in the list", () => {
    mocks.installed = [
      skill({ id: "a", name: "Apple", description: "First skill" }),
      skill({ id: "b", name: "Banana", description: "Second skill" }),
    ];
    renderPanel();
    expect(screen.getByText("Apple")).toBeInTheDocument();
    expect(screen.getByText("Banana")).toBeInTheDocument();
  });

  it("filters skills by search query", () => {
    mocks.installed = [
      skill({ id: "a", name: "Apple" }),
      skill({ id: "b", name: "Banana" }),
    ];
    renderPanel();
    const input = screen.getByPlaceholderText(
      "skills.toolbar.searchPlaceholder",
    );
    fireEvent.change(input, { target: { value: "Apple" } });
    expect(screen.getByText("Apple")).toBeInTheDocument();
    expect(screen.queryByText("Banana")).not.toBeInTheDocument();
  });

  it("clicking the Star button calls setPin via the mocked hook", () => {
    mocks.installed = [skill({ id: "a", name: "Apple" })];
    renderPanel();
    const pinBtn = screen.getByTitle("skills.pin.pin");
    fireEvent.click(pinBtn);
    expect(mocks.setPinMutateMock).toHaveBeenCalledWith({
      id: "a",
      pinned: true,
    });
  });

  it("when pinned, clicking the star unpins", () => {
    mocks.installed = [
      skill({ id: "a", name: "Apple", pinnedAt: 1234 }),
    ];
    renderPanel();
    const unpinBtn = screen.getByTitle("skills.pin.unpin");
    fireEvent.click(unpinBtn);
    expect(mocks.setPinMutateMock).toHaveBeenCalledWith({
      id: "a",
      pinned: false,
    });
  });

  it("enters selection mode and renders checkboxes", () => {
    mocks.installed = [
      skill({ id: "a", name: "Apple" }),
      skill({ id: "b", name: "Banana" }),
    ];
    renderPanel();
    const multiSelectBtn = screen.getByTitle("skills.toolbar.multiSelectMode");
    fireEvent.click(multiSelectBtn);
    // After entering selection mode, two checkboxes (one per row) appear with
    // aria-label = "skills.bulk.select".
    const checkboxes = screen.getAllByLabelText("skills.bulk.select");
    expect(checkboxes).toHaveLength(2);
  });

  it("App chip in header acts as a filter (toggles filterApps)", () => {
    mocks.installed = [
      skill({
        id: "claude-only",
        name: "ClaudeOnly",
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      }),
      skill({
        id: "codex-only",
        name: "CodexOnly",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      }),
    ];
    renderPanel();
    // Initially both visible.
    expect(screen.getByText("ClaudeOnly")).toBeInTheDocument();
    expect(screen.getByText("CodexOnly")).toBeInTheDocument();

    // Find the App chip "Claude" in the header (aria-pressed="false" initially).
    // Multiple "Claude" texts may exist (header chip + app toggle row icons),
    // pick the chip by its aria-pressed attribute.
    const claudeChip = screen
      .getAllByRole("button")
      .find(
        (b) =>
          b.getAttribute("aria-pressed") === "false" &&
          b.textContent?.includes("Claude"),
      );
    expect(claudeChip).toBeDefined();
    fireEvent.click(claudeChip!);

    // After clicking, only Claude-enabled skill remains.
    expect(screen.getByText("ClaudeOnly")).toBeInTheDocument();
    expect(screen.queryByText("CodexOnly")).not.toBeInTheDocument();
  });

  it("noResults state shows when filters yield empty list", () => {
    mocks.installed = [skill({ id: "a", name: "Apple" })];
    renderPanel();
    const input = screen.getByPlaceholderText(
      "skills.toolbar.searchPlaceholder",
    );
    fireEvent.change(input, { target: { value: "ZZZNotMatching" } });
    expect(screen.getByText("skills.noResults")).toBeInTheDocument();
  });
});
