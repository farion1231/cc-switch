import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom";
import { WindowSettings } from "@/components/settings/WindowSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@/lib/platform", () => ({
  isLinux: () => false,
}));

const baseSettings: SettingsFormState = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  useAppWindowControls: false,
  enableClaudePluginIntegration: false,
  skipClaudeOnboarding: false,
  launchOnStartup: false,
  silentStartup: false,
  autoLightweightIdleMinutes: 0,
  preserveCodexOfficialAuthOnSwitch: false,
  unifyCodexSessionHistory: false,
  language: "en",
};

describe("WindowSettings", () => {
  it("uses the single auto lightweight minutes setting for enable and disable", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <WindowSettings settings={baseSettings} onChange={onChange} />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.autoLightweight",
      }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      autoLightweightIdleMinutes: 5,
    });

    rerender(
      <WindowSettings
        settings={{ ...baseSettings, autoLightweightIdleMinutes: 10 }}
        onChange={onChange}
      />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.autoLightweight",
      }),
    );
    expect(onChange).toHaveBeenLastCalledWith({
      autoLightweightIdleMinutes: 0,
    });
  });

  it("saves edited auto lightweight minutes on blur", () => {
    const onChange = vi.fn();
    render(
      <WindowSettings
        settings={{ ...baseSettings, autoLightweightIdleMinutes: 10 }}
        onChange={onChange}
      />,
    );

    const input = screen.getByLabelText("settings.autoLightweightMinutes");
    fireEvent.change(input, { target: { value: "15" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenLastCalledWith({
      autoLightweightIdleMinutes: 15,
    });
  });

  it("disables via the minutes field when 0 is entered", () => {
    const onChange = vi.fn();
    render(
      <WindowSettings
        settings={{ ...baseSettings, autoLightweightIdleMinutes: 10 }}
        onChange={onChange}
      />,
    );

    const input = screen.getByLabelText("settings.autoLightweightMinutes");
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenLastCalledWith({
      autoLightweightIdleMinutes: 0,
    });
  });

  it("keeps the current minutes when the field is cleared", () => {
    const onChange = vi.fn();
    render(
      <WindowSettings
        settings={{ ...baseSettings, autoLightweightIdleMinutes: 10 }}
        onChange={onChange}
      />,
    );

    const input = screen.getByLabelText("settings.autoLightweightMinutes");
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);

    expect(onChange).not.toHaveBeenCalled();
    expect(input).toHaveValue(10);
  });
});
