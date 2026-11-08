import { describe, it, expect, vi, beforeEach } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import { showCodexHistorySplitHint } from "@/utils/codexHistorySplitHint";

const warningMock = vi.fn();
const successMock = vi.fn();
const errorMock = vi.fn();
vi.mock("@/lib/toast", () => ({
  toast: {
    warning: (...args: unknown[]) => warningMock(...args),
    success: (...args: unknown[]) => successMock(...args),
    error: (...args: unknown[]) => errorMock(...args),
  },
}));

const getMock = vi.fn();
const saveMock = vi.fn();
const dismissMock = vi.fn();
vi.mock("@/lib/api", () => ({
  settingsApi: {
    get: () => getMock(),
    save: (settings: unknown) => saveMock(settings),
    dismissCodexHistorySplitHint: () => dismissMock(),
  },
}));

const t = ((key: string, options?: Record<string, unknown>) =>
  options && "count" in options
    ? `${key}:${String(options.previous)}:${String(options.count)}`
    : key) as unknown as TFunction;

type ToastOptions = {
  description: string;
  action: { onClick: () => void };
  cancel: { onClick: () => void };
};

function lastToastOptions(): ToastOptions {
  return warningMock.mock.calls.at(-1)![1] as ToastOptions;
}

describe("showCodexHistorySplitHint", () => {
  beforeEach(() => {
    warningMock.mockReset();
    successMock.mockReset();
    errorMock.mockReset();
    getMock.mockReset();
    saveMock.mockReset();
    dismissMock.mockReset();
  });

  it("names the group that got locked and how many sessions it holds", () => {
    showCodexHistorySplitHint(
      { previousBucket: "openai", currentBucket: "custom", lockedThreads: 12 },
      t,
      new QueryClient(),
    );
    expect(warningMock).toHaveBeenCalledWith(
      "notifications.codexHistorySplit.title",
      expect.anything(),
    );
    expect(lastToastOptions().description).toBe(
      "notifications.codexHistorySplit.description:notifications.codexHistorySplit.officialBucket:12",
    );

    showCodexHistorySplitHint(
      { previousBucket: "custom", currentBucket: "openai", lockedThreads: 3 },
      t,
      new QueryClient(),
    );
    expect(lastToastOptions().description).toContain("thirdPartyBucket:3");
  });

  it("turns unified history and migration on from freshly read settings", async () => {
    getMock.mockResolvedValue({ language: "zh", unifyCodexSessionHistory: false });
    saveMock.mockResolvedValue(true);
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");

    showCodexHistorySplitHint(
      { previousBucket: "openai", currentBucket: "custom", lockedThreads: 1 },
      t,
      queryClient,
    );
    lastToastOptions().action.onClick();

    await vi.waitFor(() => expect(successMock).toHaveBeenCalled());
    // 从提示点进来就是为了让旧会话能继续：迁入一并打开
    expect(saveMock).toHaveBeenCalledWith({
      language: "zh",
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: true,
    });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["settings"] });
    expect(dismissMock).not.toHaveBeenCalled();
  });

  it("reports a failed save instead of claiming success", async () => {
    getMock.mockResolvedValue({});
    saveMock.mockRejectedValue(new Error("live rewrite failed"));

    showCodexHistorySplitHint(
      { previousBucket: "openai", currentBucket: "custom", lockedThreads: 1 },
      t,
      new QueryClient(),
    );
    lastToastOptions().action.onClick();

    await vi.waitFor(() => expect(errorMock).toHaveBeenCalled());
    expect(successMock).not.toHaveBeenCalled();
  });

  it("remembers don't-show-again through its own command", () => {
    dismissMock.mockResolvedValue(true);
    showCodexHistorySplitHint(
      { previousBucket: "custom", currentBucket: "openai", lockedThreads: 2 },
      t,
      new QueryClient(),
    );
    lastToastOptions().cancel.onClick();
    expect(dismissMock).toHaveBeenCalledTimes(1);
    expect(saveMock).not.toHaveBeenCalled();
  });
});
