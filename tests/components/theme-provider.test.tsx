import { act, render, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useTheme } from "@/components/theme-provider";

const invokeMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

let prefersDark = false;
const mediaQueryListeners = new Set<(event: MediaQueryListEvent) => void>();

const mediaQueryList = {
  get matches() {
    return prefersDark;
  },
  media: "(prefers-color-scheme: dark)",
  addEventListener: (_event: string, listener: (event: MediaQueryListEvent) => void) => {
    mediaQueryListeners.add(listener);
  },
  removeEventListener: (
    _event: string,
    listener: (event: MediaQueryListEvent) => void,
  ) => {
    mediaQueryListeners.delete(listener);
  },
};

function installMatchMedia(initialDark: boolean) {
  prefersDark = initialDark;
  mediaQueryListeners.clear();
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation(() => mediaQueryList),
  });
}

function emitSystemThemeChange(nextDark: boolean) {
  prefersDark = nextDark;
  act(() => {
    for (const listener of [...mediaQueryListeners]) {
      listener({
        matches: nextDark,
        media: mediaQueryList.media,
      } as MediaQueryListEvent);
    }
  });
}

function ThemeHarness() {
  const { setThemePreset, setCustomThemeColor } = useTheme();

  useEffect(() => {
    setThemePreset("custom");
    setCustomThemeColor("light", "background", "#ffffff");
    // Theme actions are invoked once to seed the custom preset before assertions.
    // The provider recreates these callbacks when theme state changes, so we
    // intentionally avoid depending on them here to prevent a test-only loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return null;
}

describe("ThemeProvider", () => {
  beforeEach(() => {
    installMatchMedia(false);
    window.localStorage.clear();
    invokeMock.mockClear();
    document.documentElement.className = "";
    document.documentElement.dataset.themePreset = "";
    document.documentElement.style.removeProperty("--background");
  });

  afterEach(() => {
    document.documentElement.className = "";
    document.documentElement.dataset.themePreset = "";
    document.documentElement.style.removeProperty("--background");
  });

  it("reapplies custom theme variables when system theme changes", async () => {
    render(
      <ThemeProvider>
        <ThemeHarness />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).toBe(
        "0 0% 100%",
      );
    });
    expect(document.documentElement.classList.contains("light")).toBe(true);

    emitSystemThemeChange(true);

    await waitFor(() => {
      expect(document.documentElement.classList.contains("dark")).toBe(true);
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).not.toBe(
        "0 0% 100%",
      );
    });

    emitSystemThemeChange(false);

    await waitFor(() => {
      expect(document.documentElement.classList.contains("light")).toBe(true);
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).toBe(
        "0 0% 100%",
      );
    });
  });
});
