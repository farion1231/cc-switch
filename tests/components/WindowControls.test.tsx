import React from "react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, act, cleanup } from "@testing-library/react";

type ResizeCb = () => void;

const mockState = {
  isMaximizedDelayMs: 0,
  onResizedDelayMs: 0,
  shouldFailIsMaximized: false,
  activeListeners: new Set<ResizeCb>(),
  reset() {
    this.isMaximizedDelayMs = 0;
    this.onResizedDelayMs = 0;
    this.shouldFailIsMaximized = false;
    this.activeListeners.clear();
  },
  triggerResize() {
    for (const cb of [...this.activeListeners]) {
      cb();
    }
  },
};

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => {
      if (mockState.isMaximizedDelayMs > 0) {
        await new Promise((r) => setTimeout(r, mockState.isMaximizedDelayMs));
      }
      if (mockState.shouldFailIsMaximized) {
        throw new Error("simulated isMaximized error");
      }
      return false;
    },
    onResized: async (cb: ResizeCb) => {
      if (mockState.onResizedDelayMs > 0) {
        await new Promise((r) => setTimeout(r, mockState.onResizedDelayMs));
      }
      mockState.activeListeners.add(cb);
      return () => {
        mockState.activeListeners.delete(cb);
      };
    },
    minimize: async () => {},
    toggleMaximize: async () => {},
    close: async () => {},
  }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (k: string) => k }),
}));

import { WindowControls } from "@/components/shell/WindowControls";

describe("WindowControls", () => {
  beforeEach(() => {
    mockState.reset();
  });

  afterEach(() => {
    cleanup();
  });

  it("cleans up onResized listener when unmounted before registration resolves (StrictMode)", async () => {
    mockState.isMaximizedDelayMs = 2;
    mockState.onResizedDelayMs = 4;

    const { unmount } = render(
      <React.StrictMode>
        <WindowControls />
      </React.StrictMode>,
    );
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(mockState.activeListeners.size).toBe(1);

    unmount();
    await act(async () => {
      await new Promise((r) => setTimeout(r, 20));
    });
    expect(mockState.activeListeners.size).toBe(0);
  });

  it("handles isMaximized errors during resize without unhandled rejections", async () => {
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { unmount } = render(<WindowControls />);
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });
    expect(mockState.activeListeners.size).toBe(1);

    mockState.shouldFailIsMaximized = true;
    mockState.triggerResize();
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });

    expect(errSpy).toHaveBeenCalled();
    unmount();
    errSpy.mockRestore();
  });
});
