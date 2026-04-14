import { act, render, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import {
  DEFAULT_CUSTOM_THEME,
  deriveDarkPalette,
  getHueFromSliderPosition,
  hexToHslString,
} from "@/lib/theme/customTheme";

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

function ExplicitDarkThemeHarness() {
  const { setTheme, setThemePreset, setCustomThemeColor } = useTheme();

  useEffect(() => {
    setThemePreset("custom");
    setTheme("dark");
    setCustomThemeColor("light", "background", "#ffffff");
    setCustomThemeColor("dark", "background", "#000000");
    // Seed the explicit palettes once for deterministic assertions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return null;
}

function SemanticThemeHarness() {
  const { setThemePreset, setCustomThemeColor } = useTheme();

  useEffect(() => {
    setThemePreset("custom");
    setCustomThemeColor("light", "success", "#22c55e");
    setCustomThemeColor("light", "info", "#0ea5e9");
    setCustomThemeColor("light", "warning", "#f59e0b");
    setCustomThemeColor("light", "error", "#ef4444");
    // Seed semantic colors once for stable CSS variable assertions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return null;
}

function LightUpdatePreservesDarkOverrideHarness() {
  const { setCustomThemeColor } = useTheme();

  useEffect(() => {
    setCustomThemeColor("light", "primary", "#f97316");
    // Trigger a base palette edit to verify dark overrides survive recomputation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return null;
}

function LightResetPreservesDarkOverrideHarness() {
  const { resetCustomTheme } = useTheme();

  useEffect(() => {
    resetCustomTheme("light");
    // Reset the base palette and ensure explicit dark overrides are still kept.
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

  it("clamps the hue slider below 360 degrees at the right edge", () => {
    expect(getHueFromSliderPosition(100, 100)).toBeLessThan(360);
    expect(getHueFromSliderPosition(100, 100)).toBeCloseTo(359.999, 3);
    expect(getHueFromSliderPosition(0, 100)).toBe(0);
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

  it("uses the explicit dark custom palette instead of deriving from the light palette", async () => {
    render(
      <ThemeProvider>
        <ExplicitDarkThemeHarness />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.classList.contains("dark")).toBe(true);
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).toBe(
        "0 0% 0%",
      );
    });
  });

  it("applies semantic custom colors to the root CSS variables", async () => {
    render(
      <ThemeProvider>
        <SemanticThemeHarness />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--success")).toBe(
        "142 71% 45%",
      );
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        "199 89% 48%",
      );
      expect(document.documentElement.style.getPropertyValue("--warning")).toBe(
        "38 92% 50%",
      );
      expect(document.documentElement.style.getPropertyValue("--error")).toBe(
        "0 84% 60%",
      );
    });
  });

  it("restores explicit dark custom colors from storage without re-deriving them", async () => {
    window.localStorage.setItem("cc-switch-theme", "dark");
    window.localStorage.setItem("cc-switch-theme-preset", "custom");
    window.localStorage.setItem(
      "cc-switch-theme-custom",
      JSON.stringify({
        light: {
          background: "#ffffff",
          info: "#2563eb",
        },
        dark: {
          background: "#000000",
          info: "#22d3ee",
        },
      }),
    );

    render(
      <ThemeProvider>
        <div>theme</div>
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.classList.contains("dark")).toBe(true);
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).toBe(
        "0 0% 0%",
      );
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        "188 86% 53%",
      );
    });
  });

  it("derives dark colors from a stored light-only payload", async () => {
    const storedLight = {
      ...DEFAULT_CUSTOM_THEME.light,
      info: "#22d3ee",
    };
    const derivedDark = deriveDarkPalette(storedLight);

    window.localStorage.setItem("cc-switch-theme", "dark");
    window.localStorage.setItem("cc-switch-theme-preset", "custom");
    window.localStorage.setItem(
      "cc-switch-theme-custom",
      JSON.stringify({
        light: {
          info: "#22d3ee",
        },
      }),
    );

    render(
      <ThemeProvider>
        <div>theme</div>
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        hexToHslString(derivedDark.info),
      );
    });
  });

  it("only applies dark overrides that are explicitly stored", async () => {
    const storedLight = {
      ...DEFAULT_CUSTOM_THEME.light,
      primary: "#f97316",
      info: "#2563eb",
    };
    const derivedDark = deriveDarkPalette(storedLight);

    window.localStorage.setItem("cc-switch-theme", "dark");
    window.localStorage.setItem("cc-switch-theme-preset", "custom");
    window.localStorage.setItem(
      "cc-switch-theme-custom",
      JSON.stringify({
        light: {
          primary: "#f97316",
          info: "#2563eb",
        },
        dark: {
          info: "#22d3ee",
        },
      }),
    );

    render(
      <ThemeProvider>
        <div>theme</div>
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
        hexToHslString(derivedDark.primary),
      );
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        "188 86% 53%",
      );
    });
  });

  it("keeps explicit dark overrides when the light palette is edited", async () => {
    window.localStorage.setItem("cc-switch-theme", "dark");
    window.localStorage.setItem("cc-switch-theme-preset", "custom");
    window.localStorage.setItem(
      "cc-switch-theme-custom",
      JSON.stringify({
        light: {
          background: "#ffffff",
          info: "#2563eb",
        },
        dark: {
          background: "#000000",
          info: "#22d3ee",
        },
      }),
    );

    render(
      <ThemeProvider>
        <LightUpdatePreservesDarkOverrideHarness />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--primary")).not.toBe(
        "206 95% 58%",
      );
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        "188 86% 53%",
      );
    });
  });

  it("keeps explicit dark overrides when the light palette is reset", async () => {
    window.localStorage.setItem("cc-switch-theme", "dark");
    window.localStorage.setItem("cc-switch-theme-preset", "custom");
    window.localStorage.setItem(
      "cc-switch-theme-custom",
      JSON.stringify({
        light: {
          background: "#ffffff",
          info: "#2563eb",
        },
        dark: {
          background: "#000000",
          info: "#22d3ee",
        },
      }),
    );

    render(
      <ThemeProvider>
        <LightResetPreservesDarkOverrideHarness />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.dataset.themePreset).toBe("custom");
    });
    await waitFor(() => {
      expect(document.documentElement.style.getPropertyValue("--background")).toBe(
        "0 0% 0%",
      );
      expect(document.documentElement.style.getPropertyValue("--info")).toBe(
        "188 86% 53%",
      );
    });
  });
});
