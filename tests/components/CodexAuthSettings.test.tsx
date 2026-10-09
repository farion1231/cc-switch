import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const forcesMultiAgentV2Mock = vi.fn();

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: () => forcesMultiAgentV2Mock(),
    hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(false),
    restoreCodexUnifiedHistory: vi.fn(),
  },
}));

const LABEL = "settings.codexStackClassicSubagents";
const FORCED_HINT = "settings.codexStackClassicSubagentsForcedV2";

function renderWith(classic: boolean, onChange = vi.fn()) {
  render(
    <CodexAuthSettings
      settings={{ codexStackClassicSubagents: classic } as SettingsFormState}
      onChange={onChange}
    />,
  );
  return onChange;
}

describe("CodexAuthSettings classic sub-agent toggle", () => {
  beforeEach(() => {
    forcesMultiAgentV2Mock.mockReset();
  });

  it("is off by default and saves the new value when toggled", () => {
    forcesMultiAgentV2Mock.mockResolvedValue(false);
    const onChange = renderWith(false);

    const toggle = screen.getByRole("switch", { name: LABEL });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    fireEvent.click(toggle);

    expect(onChange).toHaveBeenCalledWith({ codexStackClassicSubagents: true });
    // 关着时不去读 config.toml
    expect(forcesMultiAgentV2Mock).not.toHaveBeenCalled();
  });

  it("warns when config.toml forces multi_agent_v2 while the toggle is on", async () => {
    forcesMultiAgentV2Mock.mockResolvedValue(true);
    renderWith(true);

    expect(await screen.findByText(FORCED_HINT)).toBeInTheDocument();
  });

  it("shows no warning when config.toml leaves multi_agent_v2 alone", async () => {
    forcesMultiAgentV2Mock.mockResolvedValue(false);
    renderWith(true);

    await waitFor(() => expect(forcesMultiAgentV2Mock).toHaveBeenCalled());
    expect(screen.queryByText(FORCED_HINT)).not.toBeInTheDocument();
  });
});
