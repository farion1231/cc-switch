import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { GeneralSection } from "@/components/settings/sections/GeneralSection";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));

const LABEL = "settings.general.sessionScanConcurrency";

// jsdom 没有这些 API，Radix Select 打开时要用
beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
  Element.prototype.hasPointerCapture = vi.fn(() => false);
  Element.prototype.releasePointerCapture = vi.fn();
});

const renderSection = (settings: Partial<SettingsFormState> = {}) => {
  const onAutoSave = vi.fn().mockResolvedValue(true);
  render(
    <GeneralSection
      settings={
        {
          language: "zh",
          minimizeToTrayOnClose: true,
          ...settings,
        } as SettingsFormState
      }
      onAutoSave={onAutoSave}
      onOpenApps={vi.fn()}
    />,
  );
  return { onAutoSave };
};

const concurrencySelect = () => screen.getByRole("combobox", { name: LABEL });

describe("GeneralSection session scan concurrency", () => {
  it("shows auto when nothing is configured", () => {
    renderSection();

    expect(concurrencySelect()).toHaveTextContent(`${LABEL}Auto`);
  });

  it("shows the saved value", () => {
    renderSection({ sessionScanConcurrency: 4 });

    expect(concurrencySelect()).toHaveTextContent("4");
  });

  it("shows a value edited by hand that is not one of the options", () => {
    renderSection({ sessionScanConcurrency: 5 });

    expect(concurrencySelect()).toHaveTextContent("5");
  });

  it("saves the picked value immediately", async () => {
    const { onAutoSave } = renderSection();

    await userEvent.click(concurrencySelect());
    await userEvent.click(await screen.findByRole("option", { name: "16" }));

    await waitFor(() =>
      expect(onAutoSave).toHaveBeenCalledWith({ sessionScanConcurrency: 16 }),
    );
  });

  it("saves 0 when switching back to auto", async () => {
    const { onAutoSave } = renderSection({ sessionScanConcurrency: 4 });

    await userEvent.click(concurrencySelect());
    await userEvent.click(
      await screen.findByRole("option", { name: `${LABEL}Auto` }),
    );

    await waitFor(() =>
      expect(onAutoSave).toHaveBeenCalledWith({ sessionScanConcurrency: 0 }),
    );
  });
});
