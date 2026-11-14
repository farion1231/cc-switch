import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsagePage } from "@/components/usage/UsagePage";
import { UsageDashboard } from "@/components/usage/UsageDashboard";

const autoSaveSettings = vi.hoisted(() => vi.fn());
const settingsState = vi.hoisted(() => ({
  current: null as null | {
    usageDashboardRefreshIntervalMs: number;
    sessionAutoSyncEnabled: boolean;
  },
}));
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({ settings: settingsState.current, autoSaveSettings }),
}));
vi.mock("@/components/usage/UsageDashboard", () => ({
  UsageDashboard: vi.fn(() => null),
}));

describe("UsagePage settings persistence", () => {
  beforeEach(() => {
    autoSaveSettings.mockReset();
    vi.mocked(UsageDashboard).mockClear();
    settingsState.current = {
      usageDashboardRefreshIntervalMs: 30000,
      sessionAutoSyncEnabled: true,
    };
  });

  it("tells the dashboard settings are not ready until they load", () => {
    settingsState.current = null;
    const { rerender } = render(<UsagePage />);
    expect(vi.mocked(UsageDashboard).mock.lastCall![0]).toMatchObject({
      settingsReady: false,
    });
    settingsState.current = {
      usageDashboardRefreshIntervalMs: 5000,
      sessionAutoSyncEnabled: false,
    };
    rerender(<UsagePage />);
    expect(vi.mocked(UsageDashboard).mock.lastCall![0]).toMatchObject({
      settingsReady: true,
      sessionAutoSyncEnabled: false,
    });
  });

  it("waits for saving before reporting success to the dashboard", async () => {
    let finish!: (value: { requiresRestart: boolean }) => void;
    autoSaveSettings.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    render(<UsagePage />);
    const onChange =
      vi.mocked(UsageDashboard).mock.calls[0][0]!.onRefreshIntervalChange!;
    const settled = vi.fn();
    const save = Promise.resolve(onChange(5000)).then(settled);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    expect(autoSaveSettings).toHaveBeenCalledWith({
      usageDashboardRefreshIntervalMs: 5000,
    });
    finish({ requiresRestart: false });
    await save;
    expect(settled).toHaveBeenCalledWith(true);
  });

  it.each(["rejected", "not loaded"])(
    "reports a failed save when settings are %s",
    async (failure) => {
      if (failure === "rejected")
        autoSaveSettings.mockRejectedValue(new Error("save failed"));
      else autoSaveSettings.mockResolvedValue(null);
      render(<UsagePage />);
      const props = vi.mocked(UsageDashboard).mock.calls[0][0]!;
      await expect(props.onRefreshIntervalChange!(5000)).resolves.toBe(false);
      await expect(props.onSessionAutoSyncEnabledChange!(false)).resolves.toBe(
        false,
      );
    },
  );
});
