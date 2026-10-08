import { describe, expect, it, vi } from "vitest";
import { runSequentialBulkAction } from "@/lib/utils/sequentialBulkAction";

describe("runSequentialBulkAction", () => {
  it("waits for each action before starting the next one", async () => {
    let releaseFirst: (() => void) | undefined;
    const firstPending = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const action = vi.fn(async (item: number) => {
      if (item === 1) await firstPending;
    });

    const resultPromise = runSequentialBulkAction([1, 2, 3], action);
    await Promise.resolve();
    expect(action).toHaveBeenCalledTimes(1);

    releaseFirst?.();
    const result = await resultPromise;
    expect(action.mock.calls.map(([item]) => item)).toEqual([1, 2, 3]);
    expect(result).toEqual({ succeeded: [1, 2, 3], failed: [] });
  });

  it("continues after failures and returns their original items", async () => {
    const failure = new Error("failed");
    const action = vi.fn(async (item: string) => {
      if (item === "beta") throw failure;
    });

    const result = await runSequentialBulkAction(
      ["alpha", "beta", "gamma"],
      action,
    );

    expect(action).toHaveBeenCalledTimes(3);
    expect(result.succeeded).toEqual(["alpha", "gamma"]);
    expect(result.failed).toEqual([{ item: "beta", error: failure }]);
  });
});
