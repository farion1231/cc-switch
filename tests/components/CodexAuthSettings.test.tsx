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
    hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(false),
    restoreCodexUnifiedHistory: vi.fn(),
    migrateManagedCodexHistory: () => migrateManagedMock(),
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
});
