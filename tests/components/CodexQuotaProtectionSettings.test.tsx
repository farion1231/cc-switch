import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { CodexQuotaProtectionSettings } from "@/components/settings/CodexQuotaProtectionSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { toastError } = vi.hoisted(() => ({ toastError: vi.fn() }));
vi.mock("@/lib/toast", () => ({ toast: { error: toastError } }));

const key = (name: string) => `settings.codexQuotaProtection.${name}`;

function setup(
  initial: Partial<SettingsFormState> = {},
  save = vi.fn().mockResolvedValue(true),
) {
  function Form() {
    const [settings, setSettings] = useState(initial as SettingsFormState);
    return (
      <CodexQuotaProtectionSettings
        settings={settings}
        onChange={async (updates) => {
          const saved = await save(updates);
          if (saved !== false)
            setSettings((previous) => ({ ...previous, ...updates }));
          return saved;
        }}
      />
    );
  }
  render(<Form />);
  return save;
}

describe("Codex official quota protection settings", () => {
  it("reports a rejected save and leaves the draft retryable", async () => {
    setup({}, vi.fn().mockRejectedValue(new Error("disk full")));
    fireEvent.change(
      screen.getByRole("spinbutton", { name: key("fiveHour") }),
      {
        target: { value: "91" },
      },
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: key("save") }));
    });
    expect(toastError).toHaveBeenCalledWith("notifications.saveFailedGeneric");
    expect(screen.getByRole("button", { name: key("save") })).toBeEnabled();
  });
  it("defaults to enabled with 95% and 98%, and saves independently edited thresholds together", async () => {
    const save = setup();
    expect(screen.getByRole("switch", { name: key("title") })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    const fiveHour = screen.getByRole("spinbutton", { name: key("fiveHour") });
    const weekly = screen.getByRole("spinbutton", { name: key("weekly") });
    expect(fiveHour).toHaveValue(95);
    expect(weekly).toHaveValue(98);
    expect(screen.getByRole("button", { name: key("save") })).toBeDisabled();
    fireEvent.change(fiveHour, { target: { value: "90" } });
    expect(save).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: key("save") }));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        codexQuotaProtection: {
          enabled: true,
          fiveHourThreshold: 90,
          weeklyThreshold: 98,
        },
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: key("save") })).toBeDisabled(),
    );
  });

  it.each(["", "0", "100", "95.5", "-1"])(
    "rejects invalid threshold %j without persisting it",
    (value) => {
      const save = setup();
      fireEvent.change(
        screen.getByRole("spinbutton", { name: key("weekly") }),
        { target: { value } },
      );
      expect(screen.getByRole("alert")).toHaveTextContent(key("invalid"));
      expect(screen.getByRole("button", { name: key("save") })).toBeDisabled();
      expect(save).not.toHaveBeenCalled();
    },
  );

  it("restores defaults and preserves the ability to disable protection", async () => {
    const save = setup({
      codexQuotaProtection: {
        enabled: true,
        fiveHourThreshold: 85,
        weeklyThreshold: 90,
      },
    });
    fireEvent.click(screen.getByRole("button", { name: key("reset") }));
    await waitFor(() =>
      expect(
        screen.getByRole("spinbutton", { name: key("fiveHour") }),
      ).toHaveValue(95),
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("switch", { name: key("title") }));
    });
    expect(save).toHaveBeenLastCalledWith({
      codexQuotaProtection: {
        enabled: false,
        fiveHourThreshold: 95,
        weeklyThreshold: 98,
      },
    });
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
  });

  it("keeps unsaved edits retryable when saving fails", async () => {
    const save = setup({}, vi.fn().mockResolvedValue(false));
    fireEvent.change(
      screen.getByRole("spinbutton", { name: key("fiveHour") }),
      { target: { value: "89" } },
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: key("save") }));
    });
    expect(save).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: key("save") })).toBeEnabled();
    expect(
      screen.getByRole("spinbutton", { name: key("fiveHour") }),
    ).toHaveValue(89);
  });
});
