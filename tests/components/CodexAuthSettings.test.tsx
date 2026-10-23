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

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: () => forcesMultiAgentV2Mock(),
    hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(false),
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

describe("CodexAuthSettings unified history desktop compatibility", () => {
  beforeEach(() => {
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  it("shows the desktop voice warning for an already-enabled setting", async () => {
    await act(async () => {
      render(
        <CodexAuthSettings
          settings={{ unifyCodexSessionHistory: true } as SettingsFormState}
          onChange={vi.fn()}
        />,
      );
    });
    expect(screen.getByRole("note")).toHaveTextContent(
      "settings.unifyCodexDesktopWarning",
    );
  });

  it("warns before enabling without changing settings on cancel", async () => {
    const onChange = await renderWith(false);
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.unifyCodexSessionHistory" }),
    );
    expect(screen.getByRole("dialog")).toHaveTextContent(
      "settings.unifyCodexDesktopWarning",
    );
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("still allows CLI users to enable without migrating existing sessions", async () => {
    const onChange = await renderWith(false);
    fireEvent.click(
      screen.getByRole("switch", { name: "settings.unifyCodexSessionHistory" }),
    );
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    fireEvent.click(
      screen.getByRole("button", { name: "confirm.unifyCodexHistory.confirm" }),
    );
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: false,
    });
  });

  it("does not change settings when displaying a warning for existing users", async () => {
    const onChange = vi.fn();
    const props = { onChange };
    let rerender!: (ui: React.ReactElement) => void;
    await act(async () => {
      ({ rerender } = render(
        <CodexAuthSettings
          {...props}
          settings={{ unifyCodexSessionHistory: true } as SettingsFormState}
        />,
      ));
    });
    expect(screen.getByRole("note")).toBeInTheDocument();
    await act(async () => {
      rerender(
        <CodexAuthSettings
          {...props}
          settings={{ unifyCodexSessionHistory: false } as SettingsFormState}
        />,
      );
    });
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });
});
