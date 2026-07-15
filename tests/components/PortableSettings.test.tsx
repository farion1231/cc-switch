import { render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DirectorySettings } from "@/components/settings/DirectorySettings";
import { WindowSettings } from "@/components/settings/WindowSettings";

const resolvedDirs = {
  appConfig: "X:/CC-Switch/data/app",
  claude: "C:/Users/test/.claude",
  codex: "C:/Users/test/.codex",
  gemini: "C:/Users/test/.gemini",
  opencode: "C:/Users/test/.config/opencode",
  openclaw: "C:/Users/test/.openclaw",
  hermes: "C:/Users/test/.hermes",
};

const directoryProps = {
  appConfigDir: resolvedDirs.appConfig,
  resolvedDirs,
  onAppConfigChange: vi.fn(),
  onBrowseAppConfig: vi.fn(async () => undefined),
  onResetAppConfig: vi.fn(async () => undefined),
  onDirectoryChange: vi.fn(),
  onBrowseDirectory: vi.fn(async () => undefined),
  onResetDirectory: vi.fn(async () => undefined),
};

describe("portable settings", () => {
  it("locks only the CC Switch data directory in portable mode", () => {
    render(<DirectorySettings {...directoryProps} isPortable />);

    const appInput = screen.getByPlaceholderText(
      "settings.browsePlaceholderApp",
    );
    const appControls = appInput.parentElement;

    expect(appInput).toBeDisabled();
    expect(appControls).not.toBeNull();
    within(appControls!)
      .getAllByRole("button")
      .forEach((button) => {
        expect(button).toBeDisabled();
      });
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderClaude"),
    ).toBeEnabled();
    expect(
      screen.getByPlaceholderText("settings.browsePlaceholderHermes"),
    ).toBeEnabled();
  });

  it("hides auto-start controls in portable mode", () => {
    render(
      <WindowSettings
        isPortable
        settings={{
          showInTray: true,
          language: "zh",
          launchOnStartup: true,
          silentStartup: true,
          minimizeToTrayOnClose: true,
        }}
        onChange={vi.fn()}
      />,
    );

    expect(
      screen.queryByText("settings.launchOnStartup"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("settings.silentStartup"),
    ).not.toBeInTheDocument();
    expect(screen.getByText("settings.minimizeToTray")).toBeInTheDocument();
  });
});
