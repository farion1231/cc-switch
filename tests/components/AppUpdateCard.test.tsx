import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppUpdateCard } from "@/components/settings/AppUpdateCard";
import { AboutSection } from "@/components/settings/AboutSection";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import summary from "@/whats-new/4.0.14.json";
import announcement from "../../docs/release-notes/v4.0.14-zh.md?raw";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "zh" } }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("4.0.13"),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: {
    openExternal: vi.fn().mockResolvedValue(undefined),
  },
}));
vi.mock("@/lib/toast", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("@/components/WhatsNewDialog", () => ({ WhatsNewDialog: () => null }));

const notes =
  "## Release announcement\n\n- First feature\n- Second feature\n- Third feature\n- Fourth feature\n\n[Details](https://example.com/news)";
const state = {
  hasUpdate: true,
  updateInfo: {
    currentVersion: "4.0.13",
    availableVersion: "4.1.0",
    notes,
    pubDate: "2026-10-16T00:00:00Z",
  },
  isChecking: false,
  error: null as string | null,
  checkUpdate: vi.fn().mockResolvedValue(true),
};
vi.mock("@/contexts/UpdateContext", () => ({ useUpdate: () => state }));

describe("about page update card", () => {
  beforeEach(() => {
    state.updateInfo.notes = notes;
    state.updateInfo.availableVersion = "4.1.0";
    state.error = null;
    vi.mocked(settingsApi.openExternal)
      .mockReset()
      .mockResolvedValue(undefined);
  });

  it("shows the full announcement and opens the version's assets only after a click", async () => {
    render(<AppUpdateCard isPortable={false} />);
    expect(
      screen.getByRole("heading", { name: "Release announcement" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Fourth feature")).toBeInTheDocument();
    expect(settingsApi.openExternal).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.downloadNewVersion" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v4.1.0#assets",
      ),
    );
  });

  it("checks for updates without opening a download or starting installation", async () => {
    render(<AboutSection isPortable={false} />);
    fireEvent.click(
      screen.getByRole("button", { name: "settings.checkForUpdates" }),
    );
    await waitFor(() => expect(state.checkUpdate).toHaveBeenCalledOnce());
    expect(settingsApi.openExternal).not.toHaveBeenCalled();
  });

  it("shows manual instructions for portable installations", async () => {
    render(<AppUpdateCard isPortable />);
    expect(
      screen.getByText("settings.manualPortableUpdateHint"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.viewReleaseNotes" }),
    );
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v4.1.0",
      ),
    );
  });

  it("offers the full release page when no announcement was provided", () => {
    state.updateInfo.notes = "";
    render(<AppUpdateCard isPortable={false} />);
    expect(screen.getByText("settings.noReleaseNotes")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "settings.viewReleaseNotes" }),
    ).toBeInTheDocument();
  });

  it("renders the complete localized announcement from the published manifest format", () => {
    state.updateInfo.availableVersion = summary.version;
    state.updateInfo.notes = JSON.stringify({
      ...summary,
      releaseNotes: { zh: announcement },
    });
    render(<AppUpdateCard isPortable={false} />);
    expect(
      screen.getByRole("heading", { name: "CC Switch v4.0.14" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("region", { name: "settings.releaseNotes" }).textContent,
    ).not.toContain('"releaseNotes"');
  });

  it("treats legacy placeholder notes as a missing announcement", () => {
    state.updateInfo.notes = "Release v4.1.0";
    render(<AppUpdateCard isPortable={false} />);
    expect(screen.getByText("settings.noReleaseNotes")).toBeInTheDocument();
    expect(screen.queryByText("Release v4.1.0")).not.toBeInTheDocument();
  });

  it("reports a failed browser launch and allows retrying", async () => {
    vi.mocked(settingsApi.openExternal).mockRejectedValueOnce(
      new Error("browser unavailable"),
    );
    render(<AppUpdateCard isPortable={false} />);
    const button = screen.getByRole("button", {
      name: "settings.downloadNewVersion",
    });
    fireEvent.click(button);
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "settings.openUpdateDownloadFailed",
        expect.objectContaining({ description: "browser unavailable" }),
      ),
    );
    expect(button).toBeEnabled();
    fireEvent.click(button);
    await waitFor(() =>
      expect(settingsApi.openExternal).toHaveBeenCalledTimes(2),
    );
  });
});
