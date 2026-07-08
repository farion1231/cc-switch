import { renderHook, act, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { useAppShellState } from "@/app/useAppShellState";
import type { VisibleApps } from "@/types";

const visibleApps: VisibleApps = {
  claude: true,
  "claude-desktop": true,
  codex: true,
  gemini: true,
  opencode: true,
  openclaw: true,
  hermes: true,
};

describe("useAppShellState", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("falls back from OpenClaw-only views when switching to another app", async () => {
    window.localStorage.setItem("cc-switch-last-app", "openclaw");
    window.localStorage.setItem("cc-switch-last-view", "openclawEnv");

    const { result } = renderHook(() => useAppShellState(visibleApps));

    expect(result.current.activeApp).toBe("openclaw");
    expect(result.current.currentView).toBe("openclawEnv");

    act(() => {
      result.current.setActiveApp("codex");
    });

    await waitFor(() => {
      expect(result.current.activeApp).toBe("codex");
      expect(result.current.currentView).toBe("providers");
    });
  });

  it("falls back from Hermes-only views when switching away from Hermes", async () => {
    window.localStorage.setItem("cc-switch-last-app", "hermes");
    window.localStorage.setItem("cc-switch-last-view", "hermesMemory");

    const { result } = renderHook(() => useAppShellState(visibleApps));

    expect(result.current.activeApp).toBe("hermes");
    expect(result.current.currentView).toBe("hermesMemory");

    act(() => {
      result.current.setActiveApp("claude");
    });

    await waitFor(() => {
      expect(result.current.activeApp).toBe("claude");
      expect(result.current.currentView).toBe("providers");
    });
  });

  it("falls back from skills views when switching to OpenClaw", async () => {
    window.localStorage.setItem("cc-switch-last-app", "claude");
    window.localStorage.setItem("cc-switch-last-view", "skillsDiscovery");

    const { result } = renderHook(() => useAppShellState(visibleApps));

    expect(result.current.activeApp).toBe("claude");
    expect(result.current.currentView).toBe("skillsDiscovery");

    act(() => {
      result.current.setActiveApp("openclaw");
    });

    await waitFor(() => {
      expect(result.current.activeApp).toBe("openclaw");
      expect(result.current.currentView).toBe("providers");
    });
  });
});
