import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import { CodexPauseOfficialLoginSettings } from "@/components/settings/CodexPauseOfficialLoginSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const { toastSuccess, toastError } = vi.hoisted(() => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: toastSuccess, error: toastError },
}));

const key = (name: string) => `settings.codexPauseOfficialLogin.${name}`;

function setup(
  initial: Partial<SettingsFormState>,
  save: (updates: Partial<SettingsFormState>) => Promise<boolean> = vi
    .fn()
    .mockResolvedValue(true),
) {
  function Form() {
    const [settings, setSettings] = useState(initial as SettingsFormState);
    return (
      <CodexPauseOfficialLoginSettings
        settings={settings}
        onChange={async (updates) => {
          const previous = settings;
          setSettings({ ...settings, ...updates });
          const saved = await save(updates);
          if (saved === false) setSettings(previous);
          return saved;
        }}
      />
    );
  }
  render(<Form />);
  return save;
}

describe("Codex pause official login setting", () => {
  beforeEach(() => {
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it("is off by default and tells the user to restart Codex after pausing", async () => {
    const save = setup({});
    const toggle = screen.getByRole("switch", { name: key("title") });
    expect(toggle).not.toBeChecked();
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(save).toHaveBeenLastCalledWith({
      codexProxyWithoutOfficialLogin: true,
    });
    expect(toggle).toBeChecked();
    expect(toastSuccess).toHaveBeenLastCalledWith(key("pausedToast"));
  });

  it("resumes with its own hint and stays put when saving fails", async () => {
    setup({ codexProxyWithoutOfficialLogin: true });
    const toggle = screen.getByRole("switch", { name: key("title") });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(toggle).not.toBeChecked();
    expect(toastSuccess).toHaveBeenLastCalledWith(key("resumedToast"));
  });

  it("rolls back without a success hint when the backend rejects the change", async () => {
    setup(
      { codexProxyWithoutOfficialLogin: false },
      vi.fn().mockResolvedValue(false),
    );
    const toggle = screen.getByRole("switch", { name: key("title") });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(toggle).not.toBeChecked();
    expect(toastSuccess).not.toHaveBeenCalled();
  });
});
