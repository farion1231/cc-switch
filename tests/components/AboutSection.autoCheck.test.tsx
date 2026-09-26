import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";

const getToolVersions = vi.hoisted(() => vi.fn().mockResolvedValue([]));
const probeToolInstallations = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve("3.20.4"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: { getToolVersions, probeToolInstallations },
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: false,
    updateInfo: null,
    checkUpdate: vi.fn(),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("AboutSection tool version checking", () => {
  beforeEach(() => {
    getToolVersions.mockClear();
    probeToolInstallations.mockReset();
    localStorage.clear();
  });

  it("shows manual conflicts without checking versions or offering installation", async () => {
    localStorage.setItem("ccswitch:about:autoCheckToolVersions", "false");
    probeToolInstallations.mockResolvedValue([
      {
        tool: "claude",
        is_conflict: true,
        installs: [
          { path: "/fixture/a/claude", version: "1.0.0", runnable: true },
          { path: "/fixture/b/claude", version: "2.0.0", runnable: true },
        ].map((install) => ({
          ...install,
          error: null,
          source: "npm",
          is_path_default: false,
        })),
      },
    ]);
    render(<AboutSection isPortable={false} />);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.toolDiagnose" }),
    );

    expect(
      await screen.findByText("settings.toolConflictTitle"),
    ).toBeInTheDocument();
    expect(screen.getByText("/fixture/a/claude")).toBeInTheDocument();
    expect(screen.getByText("/fixture/b/claude")).toBeInTheDocument();
    expect(screen.getAllByText("common.unknown")).toHaveLength(2);
    expect(screen.queryByText("common.notInstalled")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "settings.toolInstall" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("settings.toolReady")).not.toBeInTheDocument();
    expect(getToolVersions).not.toHaveBeenCalled();

    probeToolInstallations.mockResolvedValue([]);
    fireEvent.click(
      screen.getByRole("button", { name: "settings.toolDiagnose" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("settings.toolConflictTitle"),
      ).not.toBeInTheDocument(),
    );
    expect(screen.queryByText("/fixture/a/claude")).not.toBeInTheDocument();
    expect(getToolVersions).not.toHaveBeenCalled();
  });

  it("skips automatic requests when disabled but keeps manual refresh available", async () => {
    localStorage.setItem("ccswitch:about:autoCheckToolVersions", "false");
    const { unmount } = render(<AboutSection isPortable={false} />);

    expect(
      screen.getByRole("switch", { name: "settings.autoCheckToolVersions" }),
    ).not.toBeChecked();
    expect(
      screen.getByText("settings.toolVersionsNotChecked"),
    ).toBeInTheDocument();
    expect(getToolVersions).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "common.refresh" }));
    await waitFor(() => expect(getToolVersions).toHaveBeenCalledTimes(8));
    unmount();

    render(<AboutSection isPortable={false} />);
    expect(getToolVersions).toHaveBeenCalledTimes(8);
  });
});
