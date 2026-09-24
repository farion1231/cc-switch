import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";

const getToolVersions = vi.hoisted(() => vi.fn().mockResolvedValue([]));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve("3.20.4"),
}));
vi.mock("@/lib/api", () => ({ settingsApi: { getToolVersions } }));
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
    localStorage.clear();
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
