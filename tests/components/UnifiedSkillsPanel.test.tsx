import { createRef } from "react";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedSkillsPanel, {
  type UnifiedSkillsPanelHandle,
} from "@/components/skills/UnifiedSkillsPanel";

const scanUnmanagedMock = vi.fn();
const toggleSkillAppMock = vi.fn();
const uninstallSkillMock = vi.fn();
const importSkillsMock = vi.fn();
const installFromZipMock = vi.fn();
const deleteSkillBackupMock = vi.fn();
const restoreSkillBackupMock = vi.fn();

const unmanagedSkills = [
  {
    directory: "shared-skill",
    name: "Shared Skill",
    description: "Imported from Claude",
    foundIn: ["claude"],
    path: "/tmp/shared-skill",
  },
  {
    directory: "codex-helper",
    name: "Codex Helper",
    description: "Imported from Codex",
    foundIn: ["codex"],
    path: "/tmp/codex-helper",
  },
  {
    directory: "gemini-runner",
    name: "Gemini Runner",
    description: "Imported from Gemini",
    foundIn: ["gemini"],
    path: "/tmp/gemini-runner",
  },
];

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({
    data: [],
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
    data: unmanagedSkills,
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
    scanUnmanagedMock.mockResolvedValue({
      data: unmanagedSkills,
    });
    toggleSkillAppMock.mockReset();
    uninstallSkillMock.mockReset();
    importSkillsMock.mockReset();
    installFromZipMock.mockReset();
    deleteSkillBackupMock.mockReset();
    restoreSkillBackupMock.mockReset();
  });

  const openImportDialog = async () => {
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

    return ref;
  };

  it("opens the import dialog without crashing when app toggles render", async () => {
    await openImportDialog();

    await waitFor(() => {
      expect(screen.getByText("skills.import")).toBeInTheDocument();
      expect(screen.getByText("Shared Skill")).toBeInTheDocument();
      expect(screen.getByText("/tmp/shared-skill")).toBeInTheDocument();
    });
  });

  it("does not select scanned skills by default", async () => {
    await openImportDialog();

    await waitFor(() => {
      expect(screen.getByText("Shared Skill")).toBeInTheDocument();
    });

    const checkboxes = screen.getAllByRole("checkbox");
    expect(checkboxes).toHaveLength(unmanagedSkills.length);
    checkboxes.forEach((checkbox) => {
      expect(checkbox).not.toBeChecked();
    });
    expect(
      screen.getByRole("button", { name: "skills.importSelected" }),
    ).toBeDisabled();
  });

  it("filters import candidates by search query", async () => {
    const user = userEvent.setup();
    await openImportDialog();

    await user.type(
      screen.getByLabelText("skills.importSearchAriaLabel"),
      "codex",
    );

    expect(screen.getByText("Codex Helper")).toBeInTheDocument();
    expect(screen.queryByText("Shared Skill")).not.toBeInTheDocument();
    expect(screen.queryByText("Gemini Runner")).not.toBeInTheDocument();
  });

  it("selects and deselects the current filtered results", async () => {
    const user = userEvent.setup();
    await openImportDialog();

    await user.type(
      screen.getByLabelText("skills.importSearchAriaLabel"),
      "codex",
    );

    await user.click(
      screen.getByRole("button", { name: "skills.importSelectAll" }),
    );
    expect(screen.getByRole("checkbox")).toBeChecked();

    await user.click(
      screen.getByRole("button", { name: "skills.importDeselectAll" }),
    );
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });

  it("imports only selected skills after selecting filtered results", async () => {
    const user = userEvent.setup();
    importSkillsMock.mockResolvedValue([
      {
        id: "codex-helper",
        name: "Codex Helper",
      },
    ]);
    await openImportDialog();

    await user.type(
      screen.getByLabelText("skills.importSearchAriaLabel"),
      "codex",
    );
    await user.click(
      screen.getByRole("button", { name: "skills.importSelectAll" }),
    );
    await user.click(
      screen.getByRole("button", { name: "skills.importSelected" }),
    );

    await waitFor(() => {
      expect(importSkillsMock).toHaveBeenCalledWith([
        {
          directory: "codex-helper",
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
});
