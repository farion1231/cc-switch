import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { GeneralSection } from "@/components/settings/sections/GeneralSection";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "system", setTheme: vi.fn() }),
}));

vi.mock("@/components/settings/TerminalSettings", () => ({
  TerminalSelect: () => null,
}));

const platform = vi.hoisted(() => ({ linux: false }));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isLinux: () => platform.linux,
}));

const baseSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  language: "zh",
} as SettingsFormState;

function renderSection(settings: Partial<SettingsFormState> = {}) {
  const onAutoSave = vi.fn().mockResolvedValue(true);
  render(
    <GeneralSection
      settings={{ ...baseSettings, ...settings }}
      onAutoSave={onAutoSave}
      onOpenApps={vi.fn()}
    />,
  );
  return onAutoSave;
}

describe("GeneralSection tray style", () => {
  beforeEach(() => {
    platform.linux = false;
  });

  it("defaults to the panel and saves the native menu when Menu is picked", () => {
    const onAutoSave = renderSection();
    const group = screen.getByRole("group", {
      name: "settings.general.trayStyle",
    });
    expect(group).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "settings.general.trayStylePanel" }),
    ).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(
      screen.getByRole("button", { name: "settings.general.trayStyleMenu" }),
    );
    expect(onAutoSave).toHaveBeenCalledWith({ nativeTrayMenu: true });
  });

  it("switches back to the panel", () => {
    const onAutoSave = renderSection({ nativeTrayMenu: true });
    fireEvent.click(
      screen.getByRole("button", { name: "settings.general.trayStylePanel" }),
    );
    expect(onAutoSave).toHaveBeenCalledWith({ nativeTrayMenu: false });
  });

  it("is hidden on Linux, which only has the native menu", () => {
    platform.linux = true;
    renderSection();
    expect(
      screen.queryByRole("group", { name: "settings.general.trayStyle" }),
    ).not.toBeInTheDocument();
  });
});

describe("GeneralSection GTK window buttons", () => {
  beforeEach(() => {
    platform.linux = true;
  });

  it("hides the GTK option until app-level controls are enabled", () => {
    renderSection({ useAppWindowControls: false });
    expect(
      screen.queryByRole("switch", { name: "settings.useGtkWindowControls" }),
    ).not.toBeInTheDocument();
  });

  it("keeps existing GTK styling by default and saves the independent preference", () => {
    const onAutoSave = renderSection({ useAppWindowControls: true });
    const toggle = screen.getByRole("switch", {
      name: "settings.useGtkWindowControls",
    });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(onAutoSave).toHaveBeenCalledWith({ useGtkWindowControls: false });
  });

  it("restores a saved disabled GTK preference", () => {
    const onAutoSave = renderSection({
      useAppWindowControls: true,
      useGtkWindowControls: false,
    });
    const toggle = screen.getByRole("switch", {
      name: "settings.useGtkWindowControls",
    });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(onAutoSave).toHaveBeenCalledWith({ useGtkWindowControls: true });
  });

  it("hides the GTK option on other platforms", () => {
    platform.linux = false;
    renderSection({ useAppWindowControls: true });
    expect(
      screen.queryByRole("switch", { name: "settings.useGtkWindowControls" }),
    ).not.toBeInTheDocument();
  });
});
