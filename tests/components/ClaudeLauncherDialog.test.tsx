import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import type { Provider } from "@/types";
import type { ClaudeShortcutCommandResult } from "@/lib/api/providers";
import { ClaudeLauncherDialog } from "@/components/providers/ClaudeLauncherDialog";

vi.mock("@/components/ui/select", async () => {
  const React = await vi.importActual<typeof import("react")>("react");
  const SelectContext = React.createContext<{
    onValueChange: (value: string) => void;
  }>({ onValueChange: () => undefined });

  return {
    Select: ({ onValueChange, children }: any) => (
      <SelectContext.Provider value={{ onValueChange }}>
        <div>{children}</div>
      </SelectContext.Provider>
    ),
    SelectTrigger: ({ children }: any) => <div>{children}</div>,
    SelectValue: () => null,
    SelectContent: ({ children }: any) => <div>{children}</div>,
    SelectItem: ({ value, children }: any) => {
      const context = React.useContext(SelectContext);
      return (
        <button type="button" onClick={() => context.onValueChange(value)}>
          {children}
        </button>
      );
    },
  };
});

const provider: Provider = {
  id: "kimi",
  name: "Kimi",
  settingsConfig: {},
  category: "custom",
  meta: {
    parallelConfigEnabled: false,
  },
};

const enabledProvider: Provider = {
  ...provider,
  meta: {
    parallelConfigEnabled: true,
    shortcutName: "claude-kimi",
    managedProfilePath: "/Users/test/.cc-switch/claude/kimi",
  },
};

const launcherResult: ClaudeShortcutCommandResult = {
  info: {
    name: "claude-kimi",
    targetPath: "/Users/test/.local/bin/claude-kimi",
    status: "missing",
    currentProfileDir: null,
  },
  targetKind: "user",
  userBinDir: "/Users/test/.local/bin",
  pathOnPath: false,
  pathExportSnippet: 'export PATH="$HOME/.local/bin:$PATH"',
  launchCommand: "CLAUDE_CONFIG_DIR=/Users/test/.cc-switch/claude/kimi claude",
  installed: false,
  removed: false,
  error: null,
};

const installedResult: ClaudeShortcutCommandResult = {
  ...launcherResult,
  info: {
    ...launcherResult.info,
    status: "installed",
  },
  installed: true,
};

function renderDialog(
  overrides: Partial<ComponentProps<typeof ClaudeLauncherDialog>> = {},
) {
  const props: ComponentProps<typeof ClaudeLauncherDialog> = {
    open: true,
    onOpenChange: vi.fn(),
    provider,
    onSaveLauncherSettings: vi.fn().mockResolvedValue(provider),
    onSyncProfile: vi.fn().mockResolvedValue("/Users/test/profile"),
    onOpenProfileDir: vi.fn(),
    onGetLauncherStatus: vi.fn().mockResolvedValue(launcherResult),
    onInstallLauncher: vi.fn().mockResolvedValue(installedResult),
    onRemoveLauncher: vi.fn().mockResolvedValue(launcherResult),
    ...overrides,
  };

  render(<ClaudeLauncherDialog {...props} />);
  return props;
}

describe("ClaudeLauncherDialog", () => {
  it("shows draft fields and keeps script details collapsed by default", async () => {
    const onGetLauncherStatus = vi.fn().mockResolvedValue(launcherResult);

    renderDialog({ onGetLauncherStatus });

    expect(await screen.findByDisplayValue("claude-kimi")).toBeInTheDocument();
    expect(
      screen.getByText("provider.launcherPermissionMode"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("provider.launcherPermissionModes.inherit"),
    ).toBeInTheDocument();
    expect(screen.queryByText("/Users/test/.local/bin/claude-kimi")).toBeNull();
    expect(screen.queryByText(/\/usr\/local\/bin/)).not.toBeInTheDocument();
    expect(screen.queryByText(/sudo/i)).not.toBeInTheDocument();
    expect(onGetLauncherStatus).toHaveBeenCalledWith("kimi");
  });

  it("does not save draft edits until the primary action is used", async () => {
    const onSaveLauncherSettings = vi.fn().mockResolvedValue({
      ...provider,
      meta: { ...provider.meta, parallelConfigEnabled: true },
    });
    const onInstallLauncher = vi.fn().mockResolvedValue({
      ...installedResult,
      info: {
        ...installedResult.info,
        name: "claude-kimi-fast",
        targetPath: "/Users/test/.local/bin/claude-kimi-fast",
      },
    });

    renderDialog({ onSaveLauncherSettings, onInstallLauncher });

    const aliasInput = await screen.findByDisplayValue("claude-kimi");
    fireEvent.change(aliasInput, { target: { value: "claude-kimi-fast" } });
    fireEvent.click(
      screen.getByText("provider.launcherPermissionModes.acceptEdits"),
    );
    expect(onSaveLauncherSettings).not.toHaveBeenCalled();
    expect(onInstallLauncher).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByText("Enable and Install"));

    await waitFor(() => {
      expect(onSaveLauncherSettings).toHaveBeenCalledWith("kimi", {
        enabled: true,
        launcherPermissionMode: "acceptEdits",
      });
      expect(onInstallLauncher).toHaveBeenCalledWith(
        "kimi",
        "claude-kimi-fast",
        "acceptEdits",
      );
    });
  });

  it("confirms alias changes before deleting the old managed command", async () => {
    const onSaveLauncherSettings = vi.fn().mockResolvedValue(enabledProvider);
    const onInstallLauncher = vi.fn().mockResolvedValue({
      ...installedResult,
      info: {
        ...installedResult.info,
        name: "claude-kimi-fast",
        targetPath: "/Users/test/.local/bin/claude-kimi-fast",
      },
    });

    renderDialog({
      provider: enabledProvider,
      onSaveLauncherSettings,
      onInstallLauncher,
      onGetLauncherStatus: vi.fn().mockResolvedValue(installedResult),
    });

    const aliasInput = await screen.findByDisplayValue("claude-kimi");
    fireEvent.change(aliasInput, { target: { value: "claude-kimi-fast" } });
    fireEvent.click(screen.getByText("Save and Update Command"));

    expect(
      await screen.findByText("provider.launcherAliasChangeTitle"),
    ).toBeInTheDocument();
    expect(onSaveLauncherSettings).not.toHaveBeenCalled();
    expect(onInstallLauncher).not.toHaveBeenCalled();

    fireEvent.click(screen.getByText("provider.launcherAliasChangeConfirm"));

    await waitFor(() => {
      expect(onSaveLauncherSettings).toHaveBeenCalledWith("kimi", {
        enabled: true,
        launcherPermissionMode: null,
      });
      expect(onInstallLauncher).toHaveBeenCalledWith(
        "kimi",
        "claude-kimi-fast",
        null,
        true,
      );
    });
  });

  it("keeps the dialog open when launcher install returns an error result", async () => {
    const onOpenChange = vi.fn();
    const onInstallLauncher = vi.fn().mockResolvedValue({
      ...launcherResult,
      error: "Cannot overwrite unmanaged command",
    });

    renderDialog({ onOpenChange, onInstallLauncher });

    await screen.findByDisplayValue("claude-kimi");
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByText("Enable and Install"));

    expect(
      await screen.findByText("Cannot overwrite unmanaged command"),
    ).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("discards draft changes when canceled", async () => {
    const onOpenChange = vi.fn();
    const onSaveLauncherSettings = vi.fn().mockResolvedValue(provider);

    renderDialog({ onOpenChange, onSaveLauncherSettings });

    const aliasInput = await screen.findByDisplayValue("claude-kimi");
    fireEvent.change(aliasInput, { target: { value: "claude-discarded" } });
    const cancelButtons = screen.getAllByText("common.cancel");
    fireEvent.click(cancelButtons[cancelButtons.length - 1]);

    expect(onSaveLauncherSettings).not.toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("uses install command as the primary action for enabled launchers with missing scripts", async () => {
    const onSaveLauncherSettings = vi.fn().mockResolvedValue(enabledProvider);
    const onInstallLauncher = vi.fn().mockResolvedValue(installedResult);

    renderDialog({
      provider: enabledProvider,
      onSaveLauncherSettings,
      onInstallLauncher,
      onGetLauncherStatus: vi.fn().mockResolvedValue(launcherResult),
    });

    await screen.findByDisplayValue("claude-kimi");
    fireEvent.click(screen.getByText("Install Command"));

    await waitFor(() => {
      expect(onSaveLauncherSettings).toHaveBeenCalledWith("kimi", {
        enabled: true,
        shortcutName: "claude-kimi",
        launcherPermissionMode: null,
      });
      expect(onInstallLauncher).toHaveBeenCalledWith(
        "kimi",
        "claude-kimi",
        null,
      );
    });
  });

  it("requires confirmation before saving bypass permission mode", async () => {
    const onSaveLauncherSettings = vi.fn().mockResolvedValue(provider);

    renderDialog({ onSaveLauncherSettings });

    await screen.findByDisplayValue("claude-kimi");
    fireEvent.click(
      screen.getByText("provider.launcherPermissionModes.bypassPermissions"),
    );
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByText("Enable and Install"));

    expect(
      await screen.findByText("provider.launcherPermissionBypassTitle"),
    ).toBeInTheDocument();
    expect(onSaveLauncherSettings).not.toHaveBeenCalled();

    const cancelButtons = screen.getAllByText("common.cancel");
    fireEvent.click(cancelButtons[cancelButtons.length - 1]);

    await waitFor(() => {
      expect(
        screen.queryByText("provider.launcherPermissionBypassTitle"),
      ).not.toBeInTheDocument();
    });
    expect(onSaveLauncherSettings).not.toHaveBeenCalled();
  });

  it("saves bypass permission mode after confirmation", async () => {
    const onSaveLauncherSettings = vi.fn().mockResolvedValue(provider);
    const onInstallLauncher = vi.fn().mockResolvedValue(launcherResult);

    renderDialog({ onSaveLauncherSettings, onInstallLauncher });

    await screen.findByDisplayValue("claude-kimi");
    fireEvent.click(
      screen.getByText("provider.launcherPermissionModes.bypassPermissions"),
    );
    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getByText("Enable and Install"));
    fireEvent.click(
      await screen.findByText("provider.launcherPermissionBypassConfirm"),
    );

    await waitFor(() => {
      expect(onSaveLauncherSettings).toHaveBeenCalledWith("kimi", {
        enabled: true,
        shortcutName: "claude-kimi",
        launcherPermissionMode: "bypassPermissions",
      });
      expect(onInstallLauncher).toHaveBeenCalledWith(
        "kimi",
        "claude-kimi",
        "bypassPermissions",
      );
    });
  });

  it("reveals advanced details and repair actions on demand", async () => {
    const onSyncProfile = vi.fn().mockResolvedValue("/Users/test/profile");
    const onInstallLauncher = vi.fn().mockResolvedValue(installedResult);

    renderDialog({
      provider: enabledProvider,
      onSyncProfile,
      onInstallLauncher,
      onGetLauncherStatus: vi.fn().mockResolvedValue(installedResult),
    });

    await screen.findByDisplayValue("claude-kimi");
    expect(screen.queryByText("/Users/test/.local/bin/claude-kimi")).toBeNull();

    fireEvent.click(screen.getByText("provider.launcherAdvancedDetails"));
    expect(
      screen.getByText("/Users/test/.local/bin/claude-kimi"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "CLAUDE_CONFIG_DIR=/Users/test/.cc-switch/claude/kimi claude",
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByText("provider.launcherRepairSync"));
    await waitFor(() => {
      expect(onSyncProfile).toHaveBeenCalledWith("kimi");
    });

    await waitFor(() => {
      expect(
        screen.getByText("provider.launcherRepairReinstall"),
      ).not.toBeDisabled();
    });
    fireEvent.click(screen.getByText("provider.launcherRepairReinstall"));
    await waitFor(() => {
      expect(onInstallLauncher).toHaveBeenCalledWith(
        "kimi",
        "claude-kimi",
        null,
      );
    });
  });
});
