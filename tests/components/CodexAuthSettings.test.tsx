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
const FORCED_HINT = "settings.codexStackClassicSubagentsForcedV2";

/** 挂载后把读取 config.toml / 会话占用的异步更新一并冲刷掉 */
async function renderWith(classic: boolean, onChange = vi.fn()) {
  await act(async () => {
    render(
      <CodexAuthSettings
        settings={{ codexStackClassicSubagents: classic } as SettingsFormState}
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

  it("is off by default and saves the new value when toggled", async () => {
    forcesMultiAgentV2Mock.mockResolvedValue(false);
    const onChange = await renderWith(false);

    const toggle = screen.getByRole("switch", { name: LABEL });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);

    expect(onChange).toHaveBeenCalledWith({ codexStackClassicSubagents: true });
    // 关着时不去读 config.toml
    expect(forcesMultiAgentV2Mock).not.toHaveBeenCalled();
  });

  it("warns when config.toml forces multi_agent_v2 while the toggle is on", async () => {
    forcesMultiAgentV2Mock.mockResolvedValue(true);
    await renderWith(true);

    expect(await screen.findByText(FORCED_HINT)).toBeInTheDocument();
  });

  it("shows no warning when config.toml leaves multi_agent_v2 alone", async () => {
    forcesMultiAgentV2Mock.mockResolvedValue(false);
    await renderWith(true);

    await waitFor(() => expect(forcesMultiAgentV2Mock).toHaveBeenCalled());
    expect(screen.queryByText(FORCED_HINT)).not.toBeInTheDocument();
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
