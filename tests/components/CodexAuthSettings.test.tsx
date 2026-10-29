import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.skippedFiles !== undefined
        ? `${key}:${options.files}/${options.rows}/${options.skippedFiles}/${options.skippedRows}`
        : options?.size
          ? `${key}:${options.size}`
          : key,
  }),
}));

const forcesMultiAgentV2Mock = vi.fn();
const getCompressionMock = vi.fn();
const setCompressionMock = vi.fn();
const diskUsageMock = vi.fn();
const migrateManagedMock = vi.fn();
const repairLegacyOfficialMock = vi.fn();
const restoreHistoryMock = vi.fn();
const hasBackupMock = vi.fn().mockResolvedValue(false);
const historyToastError = vi.fn();
const historyToastWarning = vi.fn();
const historyToastSuccess = vi.fn();
vi.mock("@/lib/toast", () => ({
  toast: {
    success: (...args: unknown[]) => historyToastSuccess(...args),
    warning: (...args: unknown[]) => historyToastWarning(...args),
    error: (...args: unknown[]) => historyToastError(...args),
  },
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: () => forcesMultiAgentV2Mock(),
    hasCodexUnifyHistoryBackup: () => hasBackupMock(),
    restoreCodexUnifiedHistory: () => restoreHistoryMock(),
    migrateManagedCodexHistory: () => migrateManagedMock(),
    repairLegacyOfficialCodexHistory: () => repairLegacyOfficialMock(),
    getCodexSessionCompression: () => getCompressionMock(),
    setCodexSessionCompression: (enabled: boolean) =>
      setCompressionMock(enabled),
    getCodexSessionsDiskUsage: () => diskUsageMock(),
  },
}));

const LABEL = "settings.codexStackClassicSubagents";

/** 挂载后把读取 config.toml / 会话占用的异步更新一并冲刷掉 */
async function renderWith(classic: boolean, onChange = vi.fn()) {
  await act(async () => {
    render(
      <CodexAuthSettings
        settings={{ codexStackClassicTools: classic } as SettingsFormState}
        onChange={onChange}
      />,
    );
  });
  return onChange;
}

describe("CodexAuthSettings classic sub-agent toggle", () => {
  beforeEach(() => {
    forcesMultiAgentV2Mock.mockReset();
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  // 开关挪到了 Codex 聚合页签的「聚合设置」（StackSettingsSheet），设置页不再显示
  it("is no longer on the settings page", async () => {
    await renderWith(true);

    expect(
      screen.queryByRole("switch", { name: LABEL }),
    ).not.toBeInTheDocument();
    expect(forcesMultiAgentV2Mock).not.toHaveBeenCalled();
  });
});

describe("Codex unified history desktop compatibility", () => {
  it.each([false, true])(
    "shows the desktop compatibility warning before changing the setting (enabled: %s)",
    async (enabled) => {
      getCompressionMock.mockResolvedValue(false);
      diskUsageMock.mockResolvedValue(0);
      hasBackupMock.mockResolvedValue(false);
      const onChange = vi.fn();
      await act(async () => {
        render(
          <CodexAuthSettings
            settings={
              { unifyCodexSessionHistory: enabled } as SettingsFormState
            }
            onChange={onChange}
          />,
        );
      });
      expect(screen.getByRole("note")).toHaveTextContent(
        "settings.unifyCodexDesktopCompatibility",
      );
      fireEvent.click(
        screen.getByRole("switch", {
          name: "settings.unifyCodexSessionHistory",
        }),
      );
      const dialog = await screen.findByRole("dialog");
      expect(dialog).toHaveTextContent(
        enabled
          ? "confirm.unifyCodexHistoryOff.message"
          : "confirm.unifyCodexHistory.message",
      );
      fireEvent.click(
        within(dialog).getByRole("button", { name: "common.cancel" }),
      );
      expect(onChange).not.toHaveBeenCalled();
    },
  );
});

describe("CodexAuthSettings session compression toggle", () => {
  const COMPRESSION = "settings.codexSessionCompression";

  beforeEach(() => {
    forcesMultiAgentV2Mock.mockResolvedValue(false);
    getCompressionMock.mockReset();
    setCompressionMock.mockReset();
    diskUsageMock.mockReset();
  });

  it("reflects config.toml and shows the sessions disk usage", async () => {
    getCompressionMock.mockResolvedValue(true);
    diskUsageMock.mockResolvedValue(3 * 1024 * 1024 * 1024);
    const onChange = await renderWith(false);

    const toggle = screen.getByRole("switch", { name: COMPRESSION });
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(
      await screen.findByText("settings.codexSessionCompressionUsage:3.00 GB"),
    ).toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  it("re-reads config.toml after the saved Codex directory changes", async () => {
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(1024);
    const props = {
      settings: {} as SettingsFormState,
      onChange: vi.fn(),
    };
    let rerender!: (ui: React.ReactElement) => void;
    await act(async () => {
      ({ rerender } = render(
        <CodexAuthSettings {...props} codexConfigDir="/old/codex" />,
      ));
    });
    const toggle = screen.getByRole("switch", { name: COMPRESSION });
    expect(toggle).toHaveAttribute("aria-checked", "false");

    getCompressionMock.mockResolvedValue(true);
    diskUsageMock.mockResolvedValue(2 * 1024 * 1024 * 1024);
    await act(async () => {
      rerender(<CodexAuthSettings {...props} codexConfigDir="/new/codex" />);
    });

    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByText("settings.codexSessionCompressionUsage:2.00 GB"),
    ).toBeInTheDocument();
    expect(getCompressionMock).toHaveBeenCalledTimes(2);
  });

  it("writes the feature through its own command, not the settings form", async () => {
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
    setCompressionMock.mockResolvedValue(true);
    const onChange = await renderWith(false);

    const toggle = screen.getByRole("switch", { name: COMPRESSION });
    await waitFor(() => expect(getCompressionMock).toHaveBeenCalled());
    fireEvent.click(toggle);

    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
    expect(setCompressionMock).toHaveBeenCalledWith(true);
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("Codex managed history migration consent", () => {
  beforeEach(() => {
    hasBackupMock.mockReset().mockResolvedValue(false);
    migrateManagedMock.mockReset();
    historyToastError.mockReset();
    historyToastWarning.mockReset();
    historyToastSuccess.mockReset();
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  async function renderUnified() {
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={{ unifyCodexSessionHistory: true } as SettingsFormState}
          onChange={vi.fn()}
        />,
      );
    });
  }

  it("does nothing on mount or cancellation and migrates only after confirmation", async () => {
    migrateManagedMock.mockResolvedValue({
      migratedJsonlFiles: 1,
      migratedStateRows: 1,
    });
    await renderUnified();
    expect(migrateManagedMock).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    expect(migrateManagedMock).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    await waitFor(() => expect(migrateManagedMock).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("offers manual recovery on conflict without retrying or restarting", async () => {
    migrateManagedMock.mockRejectedValue(new Error("metadata changed"));
    await renderUnified();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastError).toHaveBeenCalledWith(
        "settings.codexHistoryMigrationFailed",
        expect.objectContaining({
          description: "settings.codexHistoryMigrationRecovery",
          duration: Infinity,
        }),
      ),
    );
    expect(migrateManagedMock).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reports safety skips without claiming complete migration", async () => {
    migrateManagedMock.mockResolvedValue({
      migratedJsonlFiles: 1,
      migratedStateRows: 3,
      skippedJsonlFiles: 2,
      skippedStateRows: 4,
    });
    await renderUnified();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastWarning).toHaveBeenCalledWith(
        "settings.codexManagedHistoryPartial:1/3/2/4",
        expect.objectContaining({ closeButton: true }),
      ),
    );
    expect(historyToastSuccess).not.toHaveBeenCalled();
    expect(historyToastError).not.toHaveBeenCalled();
    expect(migrateManagedMock).toHaveBeenCalledTimes(1);
  });

  it("does not offer migration while unified history is off", async () => {
    await renderWith(false);
    expect(
      screen.queryByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    ).not.toBeInTheDocument();
    expect(migrateManagedMock).not.toHaveBeenCalled();
  });

  it("explains an unavailable state DB without reporting success or failure", async () => {
    migrateManagedMock.mockResolvedValue({
      migratedJsonlFiles: 0,
      migratedStateRows: 0,
      skippedJsonlFiles: 0,
      skippedStateRows: 0,
      skippedReason: "state_db_unlockable",
      retryPending: true,
    });
    await renderUnified();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexManagedHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastWarning).toHaveBeenCalledWith(
        "settings.codexHistoryStateUnavailable",
        expect.anything(),
      ),
    );
    expect(historyToastSuccess).not.toHaveBeenCalled();
    expect(historyToastError).not.toHaveBeenCalled();
  });

  it("reports partial restore counts after explicit confirmation", async () => {
    restoreHistoryMock.mockReset().mockResolvedValue({
      restoredJsonlFiles: 1,
      restoredStateRows: 3,
      skippedJsonlFiles: 2,
      skippedStateRows: 4,
    });
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={
            {
              unifyCodexSessionHistory: true,
              unifyCodexMigrateExisting: true,
            } as SettingsFormState
          }
          onChange={vi.fn()}
        />,
      );
    });
    expect(restoreHistoryMock).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.unifyCodexSessionHistory" }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "confirm.unifyCodexHistoryOff.confirm",
      }),
    );
    await waitFor(() =>
      expect(historyToastWarning).toHaveBeenCalledWith(
        "settings.unifyCodexHistoryRestorePartial:1/3/2/4",
        expect.anything(),
      ),
    );
    expect(historyToastSuccess).not.toHaveBeenCalled();
    expect(restoreHistoryMock).toHaveBeenCalledTimes(1);
  });

  it("offers a confirmed restore retry while unified history stays off", async () => {
    hasBackupMock.mockResolvedValue(true);
    restoreHistoryMock
      .mockReset()
      .mockResolvedValueOnce({
        restoredJsonlFiles: 1,
        restoredStateRows: 1,
        skippedJsonlFiles: 1,
        skippedStateRows: 1,
      })
      .mockResolvedValueOnce({
        restoredJsonlFiles: 1,
        restoredStateRows: 1,
        skippedJsonlFiles: 0,
        skippedStateRows: 0,
      });
    const onChange = await renderWith(false);
    const retry = await screen.findByRole("button", {
      name: "settings.unifyCodexHistoryRestoreAction",
    });
    expect(restoreHistoryMock).not.toHaveBeenCalled();
    fireEvent.click(retry);
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    expect(restoreHistoryMock).not.toHaveBeenCalled();
    for (const callCount of [1, 2]) {
      fireEvent.click(retry);
      fireEvent.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "settings.unifyCodexHistoryRestoreAction",
        }),
      );
      await waitFor(() =>
        expect(restoreHistoryMock).toHaveBeenCalledTimes(callCount),
      );
      await waitFor(() => expect(retry).not.toBeDisabled());
    }
    expect(onChange).not.toHaveBeenCalled();
    expect(
      screen.getByRole("switch", { name: "settings.unifyCodexSessionHistory" }),
    ).toHaveAttribute("aria-checked", "false");
  });
});

describe("Codex legacy official history repair consent", () => {
  beforeEach(() => {
    hasBackupMock.mockReset().mockResolvedValue(false);
    repairLegacyOfficialMock.mockReset();
    historyToastError.mockReset();
    historyToastWarning.mockReset();
    historyToastSuccess.mockReset();
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  it("offers repair while unification is off and requires explicit confirmation", async () => {
    repairLegacyOfficialMock.mockResolvedValue({
      migratedJsonlFiles: 1,
      migratedStateRows: 1,
      skippedJsonlFiles: 0,
      skippedStateRows: 0,
      retryPending: false,
    });
    const onChange = await renderWith(false);
    expect(repairLegacyOfficialMock).not.toHaveBeenCalled();
    const open = () =>
      fireEvent.click(
        screen.getByRole("button", {
          name: "settings.codexLegacyOfficialHistoryAction",
        }),
      );
    open();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    expect(repairLegacyOfficialMock).not.toHaveBeenCalled();
    open();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(repairLegacyOfficialMock).toHaveBeenCalledTimes(1),
    );
    await waitFor(() => expect(historyToastSuccess).toHaveBeenCalled());
    expect(onChange).not.toHaveBeenCalled();
  });

  it.each([
    ["legacy_route_in_use", "settings.codexLegacyOfficialHistoryInUse"],
    ["state_db_unlockable", "settings.codexHistoryStateUnavailable"],
  ])("reports %s without claiming success", async (skippedReason, message) => {
    repairLegacyOfficialMock.mockResolvedValue({
      migratedJsonlFiles: 0,
      migratedStateRows: 0,
      skippedJsonlFiles: 0,
      skippedStateRows: 0,
      retryPending: true,
      skippedReason,
    });
    await renderWith(false);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastWarning).toHaveBeenCalledWith(
        message,
        expect.anything(),
      ),
    );
    expect(historyToastSuccess).not.toHaveBeenCalled();
    expect(historyToastError).not.toHaveBeenCalled();
  });

  it("reports partial counts and leaves retries to the user", async () => {
    repairLegacyOfficialMock.mockResolvedValue({
      migratedJsonlFiles: 1,
      migratedStateRows: 2,
      skippedJsonlFiles: 3,
      skippedStateRows: 4,
      retryPending: true,
    });
    await renderWith(false);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastWarning).toHaveBeenCalledWith(
        "settings.codexLegacyOfficialHistoryPartial:1/2/3/4 settings.codexHistoryRetryPending",
        expect.anything(),
      ),
    );
    expect(repairLegacyOfficialMock).toHaveBeenCalledTimes(1);
    expect(historyToastSuccess).not.toHaveBeenCalled();
  });

  it("reports a repair failure without automatic retry or changing settings", async () => {
    repairLegacyOfficialMock.mockRejectedValue(new Error("index busy"));
    const onChange = await renderWith(false);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    await waitFor(() =>
      expect(historyToastError).toHaveBeenCalledWith(
        "settings.codexHistoryMigrationFailed",
        expect.objectContaining({
          description: "settings.codexHistoryMigrationRecovery",
        }),
      ),
    );
    expect(repairLegacyOfficialMock).toHaveBeenCalledTimes(1);
    expect(onChange).not.toHaveBeenCalled();
    expect(historyToastSuccess).not.toHaveBeenCalled();
  });

  it("blocks conflicting history actions while repair is pending", async () => {
    let finish!: (value: unknown) => void;
    repairLegacyOfficialMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={{ unifyCodexSessionHistory: true } as SettingsFormState}
          onChange={vi.fn()}
        />,
      );
    });
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    const dialog = screen.getByRole("dialog");
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    );
    expect(
      within(dialog).getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    expect(
      within(dialog).getByRole("button", {
        name: "settings.codexLegacyOfficialHistoryAction",
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("switch", {
        name: "settings.unifyCodexSessionHistory",
        hidden: true,
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "settings.codexManagedHistoryAction",
        hidden: true,
      }),
    ).toBeDisabled();
    await act(async () =>
      finish({
        migratedJsonlFiles: 0,
        migratedStateRows: 0,
        skippedJsonlFiles: 0,
        skippedStateRows: 0,
        retryPending: false,
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(repairLegacyOfficialMock).toHaveBeenCalledTimes(1);
  });
});
