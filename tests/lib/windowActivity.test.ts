import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  isFocused: vi.fn(),
  onFocusChanged: vi.fn(),
  off: vi.fn(),
  listener: undefined as undefined | ((event: { payload: boolean }) => void),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: native.isTauri }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => native }));

describe("window activity", () => {
  let focusManager: (typeof import("@tanstack/react-query"))["focusManager"];
  let initialize: (typeof import("@/lib/windowActivity"))["initializeWindowActivity"];

  beforeEach(async () => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    native.isTauri.mockReturnValue(true);
    native.isFocused.mockReset().mockResolvedValue(true);
    native.off.mockReset();
    native.onFocusChanged.mockReset().mockImplementation(async (listener) => {
      native.listener = listener;
      return native.off;
    });
    ({ focusManager } = await import("@tanstack/react-query"));
    ({ initializeWindowActivity: initialize } = await import(
      "@/lib/windowActivity"
    ));
  });

  afterEach(() => {
    focusManager.setEventListener(() => () => {});
    focusManager.setFocused(undefined);
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("uses native focus to pause foreground queries and heartbeat in a visible WebView", async () => {
    initialize();
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    expect(native.onFocusChanged).toHaveBeenCalledTimes(1);
    expect(focusManager.isFocused()).toBe(true);
    await vi.advanceTimersByTimeAsync(3000);
    expect(document.documentElement.dataset.statusHeartbeat).toBe("true");
    native.listener?.({ payload: false });
    expect(focusManager.isFocused()).toBe(false);
    expect(document.documentElement.dataset.windowActive).toBe("false");
    expect(vi.getTimerCount()).toBe(0);
    native.listener?.({ payload: true });
    expect(focusManager.isFocused()).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
    // Duplicate DOM/native focus notifications do not restart the timer.
    window.dispatchEvent(new Event("focus"));
    expect(vi.getTimerCount()).toBe(1);
  });

  it("also respects document visibility and browser focus", () => {
    native.isTauri.mockReturnValue(false);
    initialize();
    window.dispatchEvent(new Event("blur"));
    expect(focusManager.isFocused()).toBe(false);
    window.dispatchEvent(new Event("focus"));
    expect(focusManager.isFocused()).toBe(true);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    expect(focusManager.isFocused()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("reads native focus when the window was focused before listener registration", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    initialize();
    expect(focusManager.isFocused()).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
  });

  it("does not overwrite a newer focus event with a delayed initial snapshot", async () => {
    let resolve!: (focused: boolean) => void;
    native.isFocused.mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    native.listener?.({ payload: false });
    resolve(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("captures focus changes while native listener registration is pending", async () => {
    let register!: () => void;
    let nativeFocused = true;
    native.isFocused.mockImplementation(async () => nativeFocused);
    native.onFocusChanged.mockImplementationOnce(
      (listener) =>
        new Promise((resolve) => {
          register = () => {
            native.listener = listener;
            resolve(native.off);
          };
        }),
    );
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    // No DOM notification is guaranteed when a desktop WebView is hidden.
    // The native transition happens before our listener can observe it.
    nativeFocused = false;
    register();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(false);
    expect(document.documentElement.dataset.windowActive).toBe("false");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cleans up a native subscription that resolves after teardown", async () => {
    let resolve!: (off: () => void) => void;
    native.onFocusChanged.mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    initialize();
    focusManager.setEventListener(() => () => {});
    resolve(native.off);
    await Promise.resolve();
    expect(native.off).toHaveBeenCalledTimes(1);
    expect(native.isFocused).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });
});
