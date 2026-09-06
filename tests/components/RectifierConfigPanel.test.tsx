import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { settingsApi, type RectifierConfig } from "@/lib/api/settings";

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getRectifierConfig: vi.fn(),
    setRectifierConfig: vi.fn(),
    getOptimizerConfig: vi.fn(),
  },
}));

const initial: RectifierConfig = {
  enabled: true,
  requestThinkingSignature: true,
  requestThinkingBudget: true,
  requestMediaFallback: true,
  requestMediaHeuristic: true,
  requestSteerUserRole: false,
};
const label = "settings.advanced.rectifier.steerUserRole";

beforeEach(() => {
  vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue({ ...initial });
  vi.mocked(settingsApi.setRectifierConfig).mockResolvedValue(true);
  vi.mocked(settingsApi.getOptimizerConfig).mockResolvedValue({
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  });
});

describe("steer rectifier setting", () => {
  it("persists the opt-in without changing other rectifier settings", async () => {
    render(<RectifierConfigPanel />);
    const toggle = await screen.findByRole("switch", { name: label });
    expect(toggle).not.toBeChecked();
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(settingsApi.setRectifierConfig).toHaveBeenCalledWith({
        ...initial,
        requestSteerUserRole: true,
      }),
    );
    expect(toggle).toBeChecked();
  });

  it("disables the toggle when the rectifier master switch is off", async () => {
    vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue({
      ...initial,
      enabled: false,
    });
    render(<RectifierConfigPanel />);
    expect(await screen.findByRole("switch", { name: label })).toBeDisabled();
  });
});
