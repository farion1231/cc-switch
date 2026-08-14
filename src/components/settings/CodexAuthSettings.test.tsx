// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SettingsFormState } from "@/hooks/useSettings";
import { CodexAuthSettings } from "./CodexAuthSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

afterEach(cleanup);

const settings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  language: "zh",
  enableCodexLiveVoice: true,
  codexLiveVoiceRoute: "official_then_current",
} satisfies SettingsFormState;

describe("CodexAuthSettings", () => {
  it("shows the Live voice toggle and persists changes through onChange", () => {
    const onChange = vi.fn();
    render(<CodexAuthSettings settings={settings} onChange={onChange} />);

    const toggle = screen.getByRole("switch", {
      name: "settings.enableCodexLiveVoice",
    });
    expect(toggle.getAttribute("aria-checked")).toBe("true");

    fireEvent.click(toggle);

    expect(onChange).toHaveBeenCalledWith({ enableCodexLiveVoice: false });
  });

  it("shows the selected Live billing route only while enabled", () => {
    render(<CodexAuthSettings settings={settings} onChange={vi.fn()} />);

    expect(
      screen.getByRole("combobox", {
        name: "settings.codexLiveVoiceRoute",
      }),
    ).toBeTruthy();

    cleanup();
    render(
      <CodexAuthSettings
        settings={{ ...settings, enableCodexLiveVoice: false }}
        onChange={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole("combobox", {
        name: "settings.codexLiveVoiceRoute",
      }),
    ).toBeNull();
  });
});
