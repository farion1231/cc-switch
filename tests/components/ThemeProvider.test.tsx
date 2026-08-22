import { render, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { ThemeProvider, useTheme } from "@/components/theme-provider";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
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
    window.localStorage.clear();
    document.documentElement.classList.remove("light", "dark");
  });

  afterEach(() => {
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
});
