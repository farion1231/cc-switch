import { describe, expect, it, vi } from "vitest";
import {
  runSequentialBulkAction,
  runSequentialBulkActionCollect,
} from "@/lib/utils/sequentialBulkAction";

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

describe("runSequentialBulkActionCollect", () => {
  it("keeps each action's resolved value next to its item", async () => {
    // Some backends report partial success in an Ok body — uninstalling a Skill
    // can drop the managed record yet leave files behind. Dropping the resolved
    // value would hide that from the caller.
    const result = await runSequentialBulkActionCollect(
      ["alpha", "beta"],
      async (item) => ({ id: item, preservedPath: item === "beta" }),
    );

    expect(result.succeeded).toEqual([
      { item: "alpha", result: { id: "alpha", preservedPath: false } },
      { item: "beta", result: { id: "beta", preservedPath: true } },
    ]);
    expect(result.failed).toEqual([]);
  });

  it("still runs in order and records failures separately", async () => {
    const failure = new Error("nope");
    const action = vi.fn(async (item: number) => {
      if (item === 2) throw failure;
      return item * 10;
    });

    const result = await runSequentialBulkActionCollect([1, 2, 3], action);

    expect(action.mock.calls.map(([item]) => item)).toEqual([1, 2, 3]);
    expect(result.succeeded).toEqual([
      { item: 1, result: 10 },
      { item: 3, result: 30 },
    ]);
    expect(result.failed).toEqual([{ item: 2, error: failure }]);
  });
});
