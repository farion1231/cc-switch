import { createRef } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  act,
} from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { InstalledSkill } from "@/lib/api/skills";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const scanUnmanagedMock = vi.fn();
const installedSkillsMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => installedSkillsMock(),
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
    installedSkillsMock.mockReturnValue({
      data: [],
      isLoading: false,
    });
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

  it("filters installed skills by the selected app and restores the all view", async () => {
    installedSkillsMock.mockReturnValue({
      data: [
        createInstalledSkill("claude-skill", { claude: true }),
        createInstalledSkill("codex-skill", { codex: true }),
        createInstalledSkill("shared-skill", { claude: true, codex: true }),
      ],
      isLoading: false,
    });

    render(
      <UnifiedSkillsPanel onOpenDiscovery={() => {}} currentApp="claude" />,
    );

    await waitFor(() => {
      expect(screen.getByText("claude-skill")).toBeInTheDocument();
      expect(screen.getByText("codex-skill")).toBeInTheDocument();
      expect(screen.getByText("shared-skill")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Filter by Claude" }));

    await waitFor(() => {
      expect(screen.getByText("claude-skill")).toBeInTheDocument();
      expect(screen.getByText("shared-skill")).toBeInTheDocument();
      expect(screen.queryByText("codex-skill")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Filter by Codex" }));

    await waitFor(() => {
      expect(screen.getByText("codex-skill")).toBeInTheDocument();
      expect(screen.getByText("shared-skill")).toBeInTheDocument();
      expect(screen.queryByText("claude-skill")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "skills.all" }));

    await waitFor(() => {
      expect(screen.getByText("claude-skill")).toBeInTheDocument();
      expect(screen.getByText("codex-skill")).toBeInTheDocument();
      expect(screen.getByText("shared-skill")).toBeInTheDocument();
    });
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

function createInstalledSkill(
  id: string,
  apps: Partial<InstalledSkill["apps"]> = {},
): InstalledSkill {
  return {
    id,
    name: id,
    directory: id,
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      ...apps,
    },
    installedAt: 0,
    updatedAt: 0,
  };
}
