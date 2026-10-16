import { render, screen, waitFor, within } from "@testing-library/react";
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
  requestTokenReminderUserRole: false,
  requestTodoReminderUserRole: false,
  requestTaskNotificationUserRole: false,
  requestAllSystemUserRole: false,
};

beforeEach(() => {
  vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue({ ...initial });
  vi.mocked(settingsApi.setRectifierConfig).mockResolvedValue(true);
  vi.mocked(settingsApi.getOptimizerConfig).mockResolvedValue({
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  });
});

const conversions = [
  ["requestSteerUserRole", "steerUserRole"],
  ["requestTokenReminderUserRole", "tokenReminderUserRole"],
  ["requestTodoReminderUserRole", "todoReminderUserRole"],
  ["requestTaskNotificationUserRole", "taskNotificationUserRole"],
  ["requestAllSystemUserRole", "allSystemUserRole"],
] as const;

describe("response system conversion group", () => {
  it("contains all five switches, including steer", async () => {
    render(<RectifierConfigPanel />);
    const group = await screen.findByRole("region", {
      name: "settings.advanced.rectifier.responseSystemGroup",
    });
    expect(within(group).getAllByRole("switch")).toHaveLength(5);
    for (const [, name] of conversions) {
      expect(
        within(group).getByRole("switch", {
          name: `settings.advanced.rectifier.${name}`,
        }),
      ).not.toBeChecked();
    }
  });

  it.each(conversions)("persists %s independently", async (key, name) => {
    render(<RectifierConfigPanel />);
    const toggle = await screen.findByRole("switch", {
      name: `settings.advanced.rectifier.${name}`,
    });
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(settingsApi.setRectifierConfig).toHaveBeenLastCalledWith({
        ...initial,
        [key]: true,
      }),
    );
    expect(toggle).toBeChecked();
  });

  it("all conversion overrides the four filters without clearing their saved choices", async () => {
    const saved = {
      ...initial,
      requestSteerUserRole: true,
      requestTodoReminderUserRole: true,
    };
    vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue(saved);
    render(<RectifierConfigPanel />);
    const all = await screen.findByRole("switch", {
      name: "settings.advanced.rectifier.allSystemUserRole",
    });
    await userEvent.click(all);
    await waitFor(() =>
      expect(settingsApi.setRectifierConfig).toHaveBeenLastCalledWith({
        ...saved,
        requestAllSystemUserRole: true,
      }),
    );
    for (const [key, name] of conversions.slice(0, 4)) {
      const toggle = screen.getByRole("switch", {
        name: `settings.advanced.rectifier.${name}`,
      });
      expect(toggle).toBeDisabled();
      expect(toggle.getAttribute("aria-checked")).toBe(String(saved[key]));
    }
    await userEvent.click(all);
    await waitFor(() =>
      expect(settingsApi.setRectifierConfig).toHaveBeenLastCalledWith(saved),
    );
    for (const [, name] of conversions.slice(0, 4)) {
      expect(
        screen.getByRole("switch", {
          name: `settings.advanced.rectifier.${name}`,
        }),
      ).toBeEnabled();
    }
  });

  it("respects the master switch for all five settings", async () => {
    vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue({
      ...initial,
      enabled: false,
    });
    render(<RectifierConfigPanel />);
    const group = await screen.findByRole("region", {
      name: "settings.advanced.rectifier.responseSystemGroup",
    });
    for (const toggle of within(group).getAllByRole("switch"))
      expect(toggle).toBeDisabled();
  });

  it("restores filter availability when saving all conversion fails", async () => {
    vi.mocked(settingsApi.setRectifierConfig).mockRejectedValueOnce(
      new Error("Save failed"),
    );
    render(<RectifierConfigPanel />);
    const all = await screen.findByRole("switch", {
      name: "settings.advanced.rectifier.allSystemUserRole",
    });
    await userEvent.click(all);
    await waitFor(() => expect(all).not.toBeChecked());
    for (const [, name] of conversions.slice(0, 4)) {
      expect(
        screen.getByRole("switch", {
          name: `settings.advanced.rectifier.${name}`,
        }),
      ).toBeEnabled();
    }
  });
});
