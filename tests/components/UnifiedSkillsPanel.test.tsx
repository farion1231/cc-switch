import { createRef } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const scanUnmanagedMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();
const bulkUpdateSkillAppsMock = vi.fn();
const saveSkillModeMock = vi.fn();
const deleteSkillModeMock = vi.fn();
const switchSkillModeMock = vi.fn();
const saveSkillCategoryMock = vi.fn();
const deleteSkillCategoryMock = vi.fn();
const deleteSkillCategoryWithSkillsMock = vi.fn();
const moveSkillToCategoryMock = vi.fn();

let installedSkillsMock: any[] = [];
let skillModesMock: any[] = [];
let skillCategoriesMock: any[] = [];
let activeSkillModeMock = "default";

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
  useSkillModes: () => ({
    data: skillModesMock,
    isLoading: false,
  }),
  useSkillCategories: () => ({
    data: skillCategoriesMock,
    isLoading: false,
  }),
  useActiveSkillMode: () => ({
    data: activeSkillModeMock,
    isLoading: false,
  }),
  useSaveSkillCategory: () => ({
    mutateAsync: saveSkillCategoryMock,
    isPending: false,
  }),
  useDeleteSkillCategory: () => ({
    mutateAsync: deleteSkillCategoryMock,
    isPending: false,
  }),
  useDeleteSkillCategoryWithSkills: () => ({
    mutateAsync: deleteSkillCategoryWithSkillsMock,
    isPending: false,
  }),
  useMoveSkillToCategory: () => ({
    mutateAsync: moveSkillToCategoryMock,
    isPending: false,
  }),
  useBulkUpdateSkillApps: () => ({
    mutateAsync: bulkUpdateSkillAppsMock,
    isPending: false,
  }),
  useSaveSkillMode: () => ({
    mutateAsync: saveSkillModeMock,
    isPending: false,
  }),
  useDeleteSkillMode: () => ({
    mutateAsync: deleteSkillModeMock,
    isPending: false,
  }),
  useSwitchSkillMode: () => ({
    mutateAsync: switchSkillModeMock,
    isPending: false,
  }),
}));

const createSkill = (overrides: Record<string, unknown>) => ({
  id: "local:skill",
  name: "Skill",
  description: "Skill description",
  directory: "skill",
  apps: {
    claude: false,
    codex: false,
    gemini: false,
    opencode: false,
    hermes: false,
    openclaw: false,
  },
  installedAt: 0,
  updatedAt: 0,
  ...overrides,
});

describe("UnifiedSkillsPanel", () => {
  beforeEach(() => {
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
    installedSkillsMock = [];
    skillCategoriesMock = [
      {
        id: "default",
        name: "skills.categories.default",
        createdAt: 0,
        updatedAt: 0,
      },
    ];
    skillModesMock = [
      {
        id: "default",
        name: "skills.modes.default",
        matrix: {},
        createdAt: 0,
        updatedAt: 0,
      },
    ];
    activeSkillModeMock = "default";
    uninstallSkillMock.mockReset();
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    restoreSkillBackupMock.mockReset();
    bulkUpdateSkillAppsMock.mockReset();
    saveSkillModeMock.mockReset();
    deleteSkillModeMock.mockReset();
    switchSkillModeMock.mockReset();
    saveSkillCategoryMock.mockReset();
    deleteSkillCategoryMock.mockReset();
    deleteSkillCategoryWithSkillsMock.mockReset();
    moveSkillToCategoryMock.mockReset();
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

  it("puts initially uncategorized skills in the default category", () => {
    installedSkillsMock = [
      createSkill({
        id: "local:loose",
        name: "Loose Skill",
        directory: "loose",
        category: undefined,
      }),
    ];

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(screen.getAllByText("skills.categories.default").length).toBeGreaterThan(
      0,
    );
    expect(screen.getByText("Loose Skill")).toBeInTheDocument();
  });

  it("opens a dialog to create a custom category group", async () => {
    const user = userEvent.setup();
    saveSkillCategoryMock.mockResolvedValue({
      id: "writing",
      name: "Writing",
      createdAt: 1,
      updatedAt: 1,
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.queryByPlaceholderText("skills.categories.namePlaceholder"),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.categories.create" }),
    );
    await user.type(
      screen.getByPlaceholderText("skills.categories.namePlaceholder"),
      "Writing",
    );
    await user.click(
      screen.getByRole("button", { name: "skills.categories.create" }),
    );

    expect(saveSkillCategoryMock).toHaveBeenCalledWith({
      id: expect.stringContaining("writing"),
      name: "Writing",
      createdAt: 0,
      updatedAt: 0,
    });
  });

  it("creates a custom category with a generated id for non-latin names", async () => {
    const user = userEvent.setup();
    saveSkillCategoryMock.mockResolvedValue({
      id: "category-1",
      name: "写作",
      createdAt: 1,
      updatedAt: 1,
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.categories.create" }),
    );
    await user.type(
      screen.getByPlaceholderText("skills.categories.namePlaceholder"),
      "写作",
    );
    await user.click(
      screen.getByRole("button", { name: "skills.categories.create" }),
    );

    expect(saveSkillCategoryMock).toHaveBeenCalledWith({
      id: expect.stringMatching(/^category-\d+$/),
      name: "写作",
      createdAt: 0,
      updatedAt: 0,
    });
  });

  it("opens a dialog to create a mode", async () => {
    const user = userEvent.setup();
    installedSkillsMock = [
      createSkill({
        id: "local:one",
        name: "One",
        directory: "one",
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
    saveSkillModeMock.mockResolvedValue({
      id: "focus",
      name: "Focus",
      matrix: {},
      createdAt: 1,
      updatedAt: 1,
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.queryByPlaceholderText("skills.modes.namePlaceholder"),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: "skills.modes.create" }),
    );
    await user.type(
      screen.getByPlaceholderText("skills.modes.namePlaceholder"),
      "Focus",
    );
    await user.click(
      screen.getByRole("button", { name: "skills.modes.create" }),
    );

    expect(saveSkillModeMock).toHaveBeenCalledWith({
      id: expect.stringContaining("focus"),
      name: "Focus",
      matrix: {
        "local:one": {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      },
      createdAt: 0,
      updatedAt: 0,
    });
    expect(switchSkillModeMock).toHaveBeenCalledWith("focus");
  });

  it("creates an empty mode when no skills are installed", async () => {
    const user = userEvent.setup();
    saveSkillModeMock.mockResolvedValue({
      id: "mode-1",
      name: "空模式",
      matrix: {},
      createdAt: 1,
      updatedAt: 1,
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.modes.create" }),
    );
    await user.type(
      screen.getByPlaceholderText("skills.modes.namePlaceholder"),
      "空模式",
    );
    await user.click(
      screen.getByRole("button", { name: "skills.modes.create" }),
    );

    expect(saveSkillModeMock).toHaveBeenCalledWith({
      id: expect.stringMatching(/^mode-\d+$/),
      name: "空模式",
      matrix: {},
      createdAt: 0,
      updatedAt: 0,
    });
    expect(switchSkillModeMock).toHaveBeenCalledWith("mode-1");
  });

  it("confirms deleting only the category without deleting skills", async () => {
    const user = userEvent.setup();
    skillCategoriesMock = [
      ...skillCategoriesMock,
      { id: "writing", name: "Writing", createdAt: 1, updatedAt: 1 },
    ];
    deleteSkillCategoryMock.mockResolvedValue(true);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.categories.delete Writing" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.categories.deleteOnlyCategory",
      }),
    );

    expect(deleteSkillCategoryMock).toHaveBeenCalledWith("writing");
  });

  it("can delete a category together with its skills", async () => {
    const user = userEvent.setup();
    skillCategoriesMock = [
      ...skillCategoriesMock,
      { id: "writing", name: "Writing", createdAt: 1, updatedAt: 1 },
    ];
    deleteSkillCategoryWithSkillsMock.mockResolvedValue(true);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("button", { name: "skills.categories.delete Writing" }),
    );
    await user.click(
      screen.getByRole("button", {
        name: "skills.categories.deleteCategoryAndSkills",
      }),
    );

    expect(deleteSkillCategoryWithSkillsMock).toHaveBeenCalledWith("writing");
    expect(deleteSkillCategoryMock).not.toHaveBeenCalled();
  });

  it("moves a skill to the selected category from its row", async () => {
    const user = userEvent.setup();
    installedSkillsMock = [
      createSkill({
        id: "local:loose",
        name: "Loose Skill",
        directory: "loose",
        category: undefined,
      }),
    ];
    skillCategoriesMock = [
      ...skillCategoriesMock,
      { id: "writing", name: "Writing", createdAt: 1, updatedAt: 1 },
    ];
    moveSkillToCategoryMock.mockResolvedValue(undefined);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("combobox", {
        name: "skills.categories.assign Loose Skill",
      }),
    );
    await user.click(screen.getByRole("option", { name: "Writing" }));

    expect(moveSkillToCategoryMock).toHaveBeenCalledWith({
      id: "local:loose",
      category: "writing",
    });
  });

  it("bulk updates every skill in a category for one app", async () => {
    const user = userEvent.setup();
    installedSkillsMock = [
      createSkill({
        id: "local:one",
        name: "One",
        directory: "one",
        category: "writing",
      }),
      createSkill({
        id: "local:two",
        name: "Two",
        directory: "two",
        category: "writing",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          hermes: false,
        },
      }),
      createSkill({
        id: "local:three",
        name: "Three",
        directory: "three",
        category: "coding",
      }),
    ];
    skillCategoriesMock = [
      ...skillCategoriesMock,
      { id: "writing", name: "Writing", createdAt: 1, updatedAt: 1 },
      { id: "coding", name: "Coding", createdAt: 1, updatedAt: 1 },
    ];
    bulkUpdateSkillAppsMock.mockResolvedValue(2);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("button", {
        name: "skills.categories.bulkToggle Writing Codex",
      }),
    );

    expect(bulkUpdateSkillAppsMock).toHaveBeenCalledWith([
      {
        id: "local:one",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      },
      {
        id: "local:two",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      },
    ]);
  });

  it("switches modes immediately from the mode select", async () => {
    const user = userEvent.setup();
    installedSkillsMock = [
      createSkill({
        id: "local:one",
        name: "One",
        directory: "one",
        category: "Writing",
      }),
    ];
    skillModesMock = [
      ...skillModesMock,
      {
        id: "focus",
        name: "Focus",
        matrix: {
          "local:one": {
            claude: false,
            codex: true,
            gemini: false,
            opencode: false,
            hermes: false,
          },
        },
        createdAt: 1,
        updatedAt: 2,
      },
    ];
    switchSkillModeMock.mockResolvedValue(installedSkillsMock);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(
      screen.getByRole("combobox", { name: "skills.modes.title" }),
    );
    await user.click(screen.getByRole("option", { name: "Focus" }));

    expect(switchSkillModeMock).toHaveBeenCalledWith("focus");
  });

  it("confirms deleting a custom mode from the mode select", async () => {
    const user = userEvent.setup();
    installedSkillsMock = [
      createSkill({
        id: "local:one",
        name: "One",
        directory: "one",
      }),
    ];
    skillModesMock = [
      ...skillModesMock,
      {
        id: "focus",
        name: "Focus",
        matrix: {},
        createdAt: 1,
        updatedAt: 2,
      },
    ];
    deleteSkillModeMock.mockResolvedValue(true);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    expect(
      screen.queryByRole("button", { name: "skills.modes.delete" }),
    ).not.toBeInTheDocument();

    await user.click(
      screen.getByRole("combobox", { name: "skills.modes.title" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.modes.delete Focus" }),
    );

    expect(deleteSkillModeMock).not.toHaveBeenCalled();

    await user.click(
      screen.getByRole("button", { name: "skills.modes.delete" }),
    );

    expect(deleteSkillModeMock).toHaveBeenCalledWith("focus");
    expect(switchSkillModeMock).not.toHaveBeenCalledWith("focus");
  });

  it("updates app toggles in the active mode only", async () => {
    const user = userEvent.setup();
    activeSkillModeMock = "focus";
    installedSkillsMock = [
      createSkill({
        id: "local:one",
        name: "One",
        directory: "one",
      }),
    ];
    bulkUpdateSkillAppsMock.mockResolvedValue(1);

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await user.click(screen.getByRole("button", { name: "Codex" }));

    expect(bulkUpdateSkillAppsMock).toHaveBeenCalledWith([
      {
        id: "local:one",
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
      },
    ]);
  });
});
