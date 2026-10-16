import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { UpdateProvider, useUpdate } from "@/contexts/UpdateContext";
import { checkForUpdate } from "@/lib/updater";

vi.mock("@/lib/updater", () => ({ checkForUpdate: vi.fn() }));

const available = {
  status: "available" as const,
  info: { currentVersion: "4.0.13", availableVersion: "4.1.0", notes: "News" },
};
const interval = 6 * 60 * 60 * 1000;

describe("UpdateProvider", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(checkForUpdate).mockReset().mockResolvedValue(available);
  });
  afterEach(() => vi.useRealTimers());

  it("automatically checks on startup and periodically", async () => {
    const { result } = renderHook(useUpdate, { wrapper: UpdateProvider });
    expect(checkForUpdate).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1000));
    expect(result.current.hasUpdate).toBe(true);
    expect(checkForUpdate).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(interval - 1000));
    expect(checkForUpdate).toHaveBeenCalledTimes(2);
  });

  it("shares an in-flight check and preserves known updates after a network failure", async () => {
    const { result } = renderHook(useUpdate, { wrapper: UpdateProvider });
    let resolveCheck!: (value: typeof available) => void;
    vi.mocked(checkForUpdate).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveCheck = resolve;
      }),
    );
    let first!: Promise<boolean>;
    act(() => {
      first = result.current.checkUpdate();
      expect(result.current.checkUpdate()).toBe(first);
    });
    await act(async () => {
      resolveCheck(available);
      await first;
    });
    vi.mocked(checkForUpdate).mockRejectedValueOnce(new Error("offline"));
    await act(async () => {
      await expect(result.current.checkUpdate()).rejects.toThrow("offline");
    });
    expect(result.current.hasUpdate).toBe(true);
    expect(result.current.updateInfo).toEqual(available.info);
    expect(result.current.error).toBe("offline");
  });

  it("clears the indicators when the server reports no update", async () => {
    const { result } = renderHook(useUpdate, { wrapper: UpdateProvider });
    await act(async () => {
      await result.current.checkUpdate();
    });
    expect(result.current.hasUpdate).toBe(true);
    vi.mocked(checkForUpdate).mockResolvedValueOnce({ status: "up-to-date" });
    await act(async () => {
      await result.current.checkUpdate();
    });
    expect(result.current.hasUpdate).toBe(false);
    expect(result.current.updateInfo).toBeNull();
    expect(result.current.isChecking).toBe(false);
  });

  it("rechecks after returning from a long background session", async () => {
    renderHook(useUpdate, { wrapper: UpdateProvider });
    await act(() => vi.advanceTimersByTimeAsync(1000));
    vi.setSystemTime(Date.now() + interval);
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(checkForUpdate).toHaveBeenCalledTimes(2);
  });

  it("stops scheduled checks after unmounting", async () => {
    const { unmount } = renderHook(useUpdate, { wrapper: UpdateProvider });
    unmount();
    await act(() => vi.advanceTimersByTimeAsync(interval));
    expect(checkForUpdate).not.toHaveBeenCalled();
  });
});
