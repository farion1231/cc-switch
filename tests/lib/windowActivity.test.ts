import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  isTauri: vi.fn(() => true),
  isFocused: vi.fn(),
  isVisible: vi.fn(),
  isMinimized: vi.fn(),
  onFocusChanged: vi.fn(),
  onResized: vi.fn(),
  listen: vi.fn(),
  off: vi.fn(),
  offResize: vi.fn(),
  offVisibility: vi.fn(),
  listener: undefined as undefined | ((event: { payload: boolean }) => void),
  resizeListener: undefined as undefined | (() => void),
  visibilityListener: undefined as undefined | (() => void),
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
    native.isVisible.mockReset().mockResolvedValue(true);
    native.isMinimized.mockReset().mockResolvedValue(false);
    native.off.mockReset();
    native.offResize.mockReset();
    native.offVisibility.mockReset();
    native.listener = undefined;
    native.resizeListener = undefined;
    native.visibilityListener = undefined;
    native.onFocusChanged.mockReset().mockImplementation(async (listener) => {
      native.listener = listener;
      return native.off;
    });
    native.onResized.mockReset().mockImplementation(async (listener) => {
      native.resizeListener = listener;
      return native.offResize;
    });
    native.listen.mockReset().mockImplementation(async (event, listener) => {
      expect(event).toBe("main-window-visibility-changed");
      native.visibilityListener = listener;
      return native.offVisibility;
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

  it("keeps queries active when a visible native window loses focus", async () => {
    initialize();
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    expect(native.onFocusChanged).toHaveBeenCalledTimes(1);
    expect(focusManager.isFocused()).toBe(true);
    await vi.advanceTimersByTimeAsync(3000);
    expect(document.documentElement.dataset.statusHeartbeat).toBe("true");
    native.listener?.({ payload: false });
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
    expect(document.documentElement.dataset.windowActive).toBe("false");
    expect(vi.getTimerCount()).toBe(0);
    native.listener?.({ payload: true });
    expect(focusManager.isFocused()).toBe(true);
    expect(vi.getTimerCount()).toBe(1);
    // Duplicate DOM/native focus notifications do not restart the timer.
    window.dispatchEvent(new Event("focus"));
    expect(vi.getTimerCount()).toBe(1);
  });

  it("keeps a visible browser window refreshing after blur", () => {
    native.isTauri.mockReturnValue(false);
    initialize();
    window.dispatchEvent(new Event("blur"));
    expect(focusManager.isFocused()).toBe(true);
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
    expect(focusManager.isFocused()).toBe(true);
    expect(document.documentElement.dataset.windowActive).toBe("false");
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
    expect(document.documentElement.dataset.windowActive).toBe("true");
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
    expect(focusManager.isFocused()).toBe(true);
    expect(document.documentElement.dataset.windowActive).toBe("false");
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
    // The native focus transition happens before our listener can observe it.
    nativeFocused = false;
    register();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
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
    await vi.advanceTimersByTimeAsync(0);
    expect(native.off).toHaveBeenCalledTimes(1);
    expect(native.offResize).toHaveBeenCalledTimes(1);
    expect(native.offVisibility).toHaveBeenCalledTimes(1);
    expect(native.isFocused).not.toHaveBeenCalled();
    expect(native.isVisible).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each([
    { visible: false, minimized: false },
    { visible: true, minimized: true },
  ])(
    "pauses for native state %j even if the document stays visible",
    async (state) => {
      native.isFocused.mockResolvedValue(false);
      native.isVisible.mockResolvedValue(state.visible);
      native.isMinimized.mockResolvedValue(state.minimized);
      initialize();
      await vi.advanceTimersByTimeAsync(0);
      expect(focusManager.isFocused()).toBe(false);
      expect(vi.getTimerCount()).toBe(0);

      native.isVisible.mockResolvedValue(true);
      native.isMinimized.mockResolvedValue(false);
      native.resizeListener?.();
      await vi.advanceTimersByTimeAsync(0);
      expect(focusManager.isFocused()).toBe(true);
      expect(document.documentElement.dataset.windowActive).toBe("false");
    },
  );

  it("observes hide/show without a focus or resize event", async () => {
    native.isFocused.mockResolvedValue(false);
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
    native.isVisible.mockResolvedValue(false);
    native.visibilityListener?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(false);
    native.isVisible.mockResolvedValue(true);
    native.visibilityListener?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("ignores a stale visibility response after a newer window transition", async () => {
    let resolve!: (visible: boolean) => void;
    native.isVisible.mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    native.isVisible.mockResolvedValue(false);
    native.visibilityListener?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(false);
    resolve(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(focusManager.isFocused()).toBe(false);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps real Query polling on blur and pauses it only while hidden", async () => {
    const { QueryClient, QueryObserver } = await import(
      "@tanstack/react-query"
    );
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const query = vi.fn().mockResolvedValue(1);
    initialize();
    await vi.advanceTimersByTimeAsync(0);
    client.mount();
    const observer = new QueryObserver(client, {
      queryKey: ["proxyStatus"],
      queryFn: query,
      initialData: 0,
      staleTime: Infinity,
      refetchInterval: 2000,
    });
    const unsubscribe = observer.subscribe(() => {});
    try {
      native.listener?.({ payload: false });
      await vi.advanceTimersByTimeAsync(2000);
      expect(query).toHaveBeenCalledTimes(1);
      native.isVisible.mockResolvedValue(false);
      native.visibilityListener?.();
      await vi.advanceTimersByTimeAsync(6000);
      expect(query).toHaveBeenCalledTimes(1);
      native.isVisible.mockResolvedValue(true);
      native.visibilityListener?.();
      await vi.advanceTimersByTimeAsync(2000);
      expect(query).toHaveBeenCalledTimes(2);
    } finally {
      unsubscribe();
      client.unmount();
      client.clear();
    }
  });
});
