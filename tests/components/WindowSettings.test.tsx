import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { SettingsFormState } from "@/hooks/useSettings";
import {
  DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
  MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
} from "@/components/settings/AutoLightweightSettings";
import { WindowSettings } from "@/components/settings/WindowSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (
      key: string,
      options?: { minutes?: number; min?: number; max?: number },
    ) => {
      if (key === "settings.autoLightweightModeDescription") {
        return "auto lightweight description";
      }
      if (key === "settings.autoLightweightMinutesRange") {
        return `enter ${options?.min}-${options?.max}`;
      }
      return key;
    },
  }),
}));

const createSettings = (
  overrides: Partial<SettingsFormState> = {},
): SettingsFormState =>
  ({
    showInTray: true,
    minimizeToTrayOnClose: true,
    autoLightweightEnabled: false,
    autoLightweightAfterMinutes: DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
    language: "zh",
    ...overrides,
  }) as SettingsFormState;

const setup = (overrides: Partial<SettingsFormState> = {}) => {
  const onChange = vi.fn();
  render(
    <WindowSettings settings={createSettings(overrides)} onChange={onChange} />,
  );
  return {
    onChange,
    input: screen.getByRole("spinbutton", {
      name: "settings.autoLightweightDelay",
    }),
    toggle: screen.getByRole("switch", {
      name: "settings.autoLightweightMode",
    }),
  };
};

describe("WindowSettings auto lightweight mode", () => {
  it("is disabled by default and enables the one minute policy", async () => {
    const { input, onChange, toggle } = setup();
    expect(toggle).not.toBeChecked();
    expect(
      screen.getByText("auto lightweight description"),
    ).toBeInTheDocument();
    expect(input).toBeDisabled();
    expect(input).toHaveValue(DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES);

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith({
        autoLightweightEnabled: true,
        autoLightweightAfterMinutes: DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
      }),
    );
  });

  it("saves a valid custom duration on blur", async () => {
    const { input, onChange } = setup({ autoLightweightEnabled: true });
    fireEvent.change(input, { target: { value: "15" } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith({
        autoLightweightEnabled: true,
        autoLightweightAfterMinutes: 15,
      }),
    );
  });

  it("combines a dirty duration with a toggle click into one save", async () => {
    const user = userEvent.setup();
    const { input, onChange, toggle } = setup({
      autoLightweightEnabled: true,
      autoLightweightAfterMinutes: 5,
    });
    await user.clear(input);
    await user.type(input, "8");
    await user.click(toggle);

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledTimes(1);
      expect(onChange).toHaveBeenCalledWith({
        autoLightweightEnabled: false,
        autoLightweightAfterMinutes: 8,
      });
    });
  });

  it("rejects non-integer and out-of-range durations without saving", () => {
    const { input, onChange } = setup({ autoLightweightEnabled: true });
    fireEvent.change(input, { target: { value: "1.5" } });
    fireEvent.blur(input);
    expect(screen.getByRole("alert")).toHaveTextContent(
      `enter 1-${MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES}`,
    );
    expect(onChange).not.toHaveBeenCalled();
  });
});
