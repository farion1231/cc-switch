import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppVisibilitySettings } from "@/components/settings/AppVisibilitySettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const createSettings = (): SettingsFormState =>
  ({
    language: "zh",
    visibleApps: {
      claude: true,
      "claude-desktop": true,
      codex: true,
      gemini: true,
      grokbuild: true,
      opencode: true,
      openclaw: true,
      hermes: true,
      pi: true,
      cursor: true,
    },
  }) as SettingsFormState;

describe("AppVisibilitySettings", () => {
  it("renders Cursor with its brand icon and toggles visibility", () => {
    const onChange = vi.fn();

    render(
      <AppVisibilitySettings settings={createSettings()} onChange={onChange} />,
    );

    const cursorButton = screen.getByRole("button", { name: "apps.cursor" });
    const cursorIcon = cursorButton.querySelector('img[alt="Cursor"]');

    expect(cursorIcon).toHaveAttribute(
      "src",
      expect.stringContaining("cursor"),
    );

    fireEvent.click(cursorButton);

    expect(onChange).toHaveBeenCalledWith({
      visibleApps: expect.objectContaining({ cursor: false }),
    });
  });
});
