import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.size ? `${key}:${options.size}` : key,
  }),
}));

const forcesMultiAgentV2Mock = vi.fn();
const getCompressionMock = vi.fn();
const setCompressionMock = vi.fn();
const diskUsageMock = vi.fn();
const hasUnifyBackupMock = vi.fn().mockResolvedValue(false);

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: () => forcesMultiAgentV2Mock(),
    hasCodexUnifyHistoryBackup: () => hasUnifyBackupMock(),
    restoreCodexUnifiedHistory: vi.fn(),
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

describe("CodexAuthSettings unified session history toggle", () => {
  const UNIFY = "settings.unifyCodexSessionHistory";

  beforeEach(() => {
    hasUnifyBackupMock.mockResolvedValue(false);
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  async function renderUnify(
    enabled: boolean,
    migrateExisting?: boolean,
    paused = false,
  ) {
    const onChange = vi.fn().mockResolvedValue(true);
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={
            {
              unifyCodexSessionHistory: enabled,
              unifyCodexMigrateExisting: migrateExisting,
              codexProxyWithoutOfficialLogin: paused,
            } as SettingsFormState
          }
          onChange={onChange}
        />,
      );
    });
    return onChange;
  }

  // Identity preference alone must not implicitly rewrite existing history.
  it("enables identity preference without implicitly migrating existing sessions", async () => {
    const onChange = await renderUnify(false);

    fireEvent.click(screen.getByRole("switch", { name: UNIFY }));
    const checkbox = await screen.findByRole("checkbox");
    expect(
      screen.getByText("confirm.unifyCodexHistory.migrateExisting"),
    ).toBeInTheDocument();
    expect(checkbox).not.toBeChecked();

    fireEvent.click(
      screen.getByRole("button", { name: "confirm.unifyCodexHistory.confirm" }),
    );
    expect(onChange).toHaveBeenCalledWith({
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: false,
    });
  });

  it("migrates only after a separate explicit opt-in", async () => {
    const onChange = await renderUnify(false);

    fireEvent.click(screen.getByRole("switch", { name: UNIFY }));
    fireEvent.click(await screen.findByRole("checkbox"));
    fireEvent.click(
      screen.getByRole("button", { name: "confirm.unifyCodexHistory.confirm" }),
    );
    expect(onChange).toHaveBeenCalledWith({
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: true,
    });
  });

  it("blocks enabling unified identity while login pause is active", async () => {
    await renderUnify(false, false, true);
    expect(screen.getByRole("switch", { name: UNIFY })).toBeDisabled();
    expect(
      screen.getByText("settings.codexPauseOfficialLogin.unifiedPriority"),
    ).toBeInTheDocument();
  });

  it("allows resolving an older conflicting preference by disabling unified identity", async () => {
    await renderUnify(true, false, true);
    const toggle = screen.getByRole("switch", { name: UNIFY });
    expect(toggle).toBeEnabled();
    fireEvent.click(toggle);
    expect(
      await screen.findByText("confirm.unifyCodexHistoryOff.message"),
    ).toBeInTheDocument();
  });

  it("offers restore only when a legacy v1 backup exists", async () => {
    hasUnifyBackupMock.mockResolvedValue(true);
    await renderUnify(true, true);

    fireEvent.click(screen.getByRole("switch", { name: UNIFY }));
    expect(
      await screen.findByText("confirm.unifyCodexHistoryOff.restoreBackup"),
    ).toBeInTheDocument();
  });

  it("does not offer a reverse migration for a v2-only opt-in", async () => {
    await renderUnify(true, true);
    fireEvent.click(screen.getByRole("switch", { name: UNIFY }));
    expect(
      await screen.findByText("confirm.unifyCodexHistoryOff.message"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("confirm.unifyCodexHistoryOff.restoreBackup"),
    ).not.toBeInTheDocument();
  });

  it("has nothing to restore when migration was never opted into", async () => {
    await renderUnify(true, false);

    fireEvent.click(screen.getByRole("switch", { name: UNIFY }));
    expect(
      await screen.findByText("confirm.unifyCodexHistoryOff.message"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("confirm.unifyCodexHistoryOff.restoreBackup"),
    ).not.toBeInTheDocument();
  });
});
