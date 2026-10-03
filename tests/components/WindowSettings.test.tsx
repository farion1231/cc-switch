import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { SettingsFormState } from "@/hooks/useSettings";
import {
  DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
  MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
  MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
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
  const view = render(
    <WindowSettings settings={createSettings(overrides)} onChange={onChange} />,
  );
  return {
    onChange,
    toggle: screen.getByRole("switch", {
      name: "settings.autoLightweightMode",
    }),
    rerenderSettings: (next: Partial<SettingsFormState>) =>
      view.rerender(
        <WindowSettings settings={createSettings(next)} onChange={onChange} />,
      ),
  };
};

const getDurationInput = () =>
  screen.getByRole("spinbutton", {
    name: "settings.autoLightweightDelay",
  });

describe("WindowSettings auto lightweight mode", () => {
  it("hides the duration by default and enables the five minute policy", async () => {
    const { onChange, toggle } = setup();
    expect(toggle).not.toBeChecked();
    expect(
      screen.getByText("auto lightweight description"),
    ).toBeInTheDocument();
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(onChange).toHaveBeenCalledWith({
        autoLightweightEnabled: true,
        autoLightweightAfterMinutes: DEFAULT_AUTO_LIGHTWEIGHT_AFTER_MINUTES,
      }),
    );
  });

  it("shows the duration only while the policy is enabled", () => {
    const { rerenderSettings } = setup({
      autoLightweightEnabled: true,
      autoLightweightAfterMinutes: 12,
    });
    expect(getDurationInput()).toHaveValue(12);

    rerenderSettings({
      autoLightweightEnabled: false,
      autoLightweightAfterMinutes: 12,
    });
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();

    rerenderSettings({
      autoLightweightEnabled: true,
      autoLightweightAfterMinutes: 12,
    });
    expect(screen.getByRole("spinbutton")).toHaveValue(12);
  });

  it("saves a valid custom duration on blur", async () => {
    const { onChange } = setup({ autoLightweightEnabled: true });
    const input = getDurationInput();
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
    const { onChange, toggle } = setup({
      autoLightweightEnabled: true,
      autoLightweightAfterMinutes: 5,
    });
    const input = getDurationInput();
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

  it("accepts zero as the immediate-entry policy", () => {
    const { onChange } = setup({ autoLightweightEnabled: true });
    const input = getDurationInput();
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenCalledWith({
      autoLightweightEnabled: true,
      autoLightweightAfterMinutes: 0,
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it.each(["-1", "1.5", String(MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES + 1)])(
    "rejects invalid duration %s without saving",
    (value) => {
      const { onChange } = setup({ autoLightweightEnabled: true });
      const input = getDurationInput();
      fireEvent.change(input, { target: { value } });
      fireEvent.blur(input);
      expect(screen.getByRole("alert")).toHaveTextContent(
        `enter ${MIN_AUTO_LIGHTWEIGHT_AFTER_MINUTES}-${MAX_AUTO_LIGHTWEIGHT_AFTER_MINUTES}`,
      );
      expect(onChange).not.toHaveBeenCalled();
    },
  );
});
