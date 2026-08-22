import { render, waitFor, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import { invoke } from "@tauri-apps/api/core";
import * as platform from "@/lib/platform";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@/lib/platform", () => ({
  isLinux: vi.fn(() => true),
}));

function ThemeConsumer() {
  const { theme, setTheme } = useTheme();
  return (
    <div>
      <span data-testid="theme-value">{theme}</span>
      <button onClick={() => setTheme("light")}>Set Light</button>
      <button onClick={() => setTheme("dark")}>Set Dark</button>
      <button onClick={() => setTheme("system")}>Set System</button>
    </div>
  );
}

describe("ThemeProvider", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(platform.isLinux).mockReturnValue(true);
    window.localStorage.clear();
    document.documentElement.classList.remove("light", "dark");
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("syncs system dark theme using get_system_theme command", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_system_theme") return "dark";
      return undefined;
    });

    render(
      <ThemeProvider defaultTheme="system" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.classList.contains("dark")).toBe(true);
      expect(document.documentElement.classList.contains("light")).toBe(false);
    });

    expect(invoke).toHaveBeenCalledWith("get_system_theme");
  });

  it("syncs system light theme using get_system_theme command", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_system_theme") return "light";
      return undefined;
    });

    render(
      <ThemeProvider defaultTheme="system" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.classList.contains("light")).toBe(true);
      expect(document.documentElement.classList.contains("dark")).toBe(false);
    });
  });

  it("applies fixed theme when set to light or dark", async () => {
    render(
      <ThemeProvider defaultTheme="dark" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    await waitFor(() => {
      expect(document.documentElement.classList.contains("dark")).toBe(true);
    });

    expect(invoke).toHaveBeenCalledWith("set_window_theme", { theme: "dark" });
  });

  it("polls periodically every 2s on Linux", async () => {
    vi.useFakeTimers();
    vi.mocked(platform.isLinux).mockReturnValue(true);
    let themeValue = "dark";
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_system_theme") return themeValue;
      return undefined;
    });

    render(
      <ThemeProvider defaultTheme="system" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    const getSystemThemeCalls = () =>
      vi
        .mocked(invoke)
        .mock.calls.filter(([cmd]) => cmd === "get_system_theme");

    // Initial mount sync
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(getSystemThemeCalls()).toHaveLength(1);
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    // Theme changes on system
    themeValue = "light";
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(getSystemThemeCalls()).toHaveLength(2);
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("does not poll periodically on non-Linux platforms", async () => {
    vi.useFakeTimers();
    vi.mocked(platform.isLinux).mockReturnValue(false);
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_system_theme") return "dark";
      return undefined;
    });

    render(
      <ThemeProvider defaultTheme="system" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    const getSystemThemeCalls = () =>
      vi
        .mocked(invoke)
        .mock.calls.filter(([cmd]) => cmd === "get_system_theme");

    // Initial sync
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(getSystemThemeCalls()).toHaveLength(1);

    // Advance timers - on non-Linux, setInterval was not scheduled
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(getSystemThemeCalls()).toHaveLength(1);
  });

  it("prevents overlapping concurrent get_system_theme calls via in-flight guard", async () => {
    vi.useFakeTimers();
    vi.mocked(platform.isLinux).mockReturnValue(true);

    let resolveFirstCall: (val: string) => void;
    const firstCallPromise = new Promise<string>((resolve) => {
      resolveFirstCall = resolve;
    });

    let callCount = 0;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_system_theme") {
        callCount++;
        if (callCount === 1) {
          return firstCallPromise;
        }
        return "light";
      }
      return undefined;
    });

    render(
      <ThemeProvider defaultTheme="system" storageKey="test-theme">
        <ThemeConsumer />
      </ThemeProvider>,
    );

    // First call started but hasn't resolved
    expect(callCount).toBe(1);

    // Advance 2s while first call is still in flight
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    // in-flight guard should have skipped the tick
    expect(callCount).toBe(1);

    // Resolve first call
    await act(async () => {
      resolveFirstCall!("dark");
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    // Now that in-flight is cleared, next 2s tick should proceed
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(callCount).toBe(2);
    expect(document.documentElement.classList.contains("light")).toBe(true);
  });
});
