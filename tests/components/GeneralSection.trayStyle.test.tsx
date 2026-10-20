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

const platform = vi.hoisted(() => ({ linux: false, mac: false }));
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  isLinux: () => platform.linux,
  isMac: () => platform.mac,
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
    platform.mac = false;
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

describe("GeneralSection menu bar data", () => {
  beforeEach(() => {
    platform.linux = false;
    platform.mac = true;
  });

  it("shows five independent switches with speed enabled by default", () => {
    renderSection();

    expect(
      screen.getByText("settings.general.menuBarData"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("switch", {
        name: "settings.general.menuBarSpeed",
      }),
    ).toBeChecked();
    for (const name of [
      "settings.general.menuBarTotalTokens",
      "settings.general.menuBarRequests",
      "settings.general.menuBarCost",
      "settings.general.menuBarCacheHitRate",
    ]) {
      expect(screen.getByRole("switch", { name })).not.toBeChecked();
    }
    expect(
      screen.getByText("settings.general.menuBarSpeedHelp"),
    ).toBeInTheDocument();
  });

  it("saves one changed item while preserving the other settings", () => {
    const onAutoSave = renderSection({
      menuBarData: {
        speed: true,
        totalTokens: false,
        requests: false,
        cost: false,
        cacheHitRate: false,
      },
    });

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.general.menuBarTotalTokens",
      }),
    );

    expect(onAutoSave).toHaveBeenCalledWith({
      menuBarData: {
        speed: true,
        totalTokens: true,
        requests: false,
        cost: false,
        cacheHitRate: false,
      },
    });
  });

  it("is hidden on non-macOS platforms", () => {
    platform.mac = false;
    renderSection();
    expect(
      screen.queryByText("settings.general.menuBarData"),
    ).not.toBeInTheDocument();
  });
});
