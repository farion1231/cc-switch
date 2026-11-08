import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const setZoomMock = vi.hoisted(() => vi.fn(() => Promise.resolve()));

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ setZoom: setZoomMock }),
}));

// 模块加载时就读 localStorage，每个用例重新加载一次
async function load() {
  vi.resetModules();
  return import("@/lib/interfacePrefs");
}

describe("interfacePrefs", () => {
  beforeEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset.density;
    setZoomMock.mockClear();
  });

  afterEach(() => {
    localStorage.clear();
    delete document.documentElement.dataset.density;
  });

  it("defaults to compact density and 100% zoom without touching the webview", async () => {
    const prefs = await load();
    prefs.initializeInterfacePrefs();

    expect(document.documentElement.dataset.density).toBe("compact");
    expect(setZoomMock).not.toHaveBeenCalled();
  });

  it("applies the stored density and zoom on startup", async () => {
    localStorage.setItem("cc-switch-density", "comfortable");
    localStorage.setItem("cc-switch-zoom", "1.25");
    const prefs = await load();
    prefs.initializeInterfacePrefs();

    expect(document.documentElement.dataset.density).toBe("comfortable");
    expect(setZoomMock).toHaveBeenCalledWith(1.25);
  });

  it("ignores stored values that are not a known density or zoom level", async () => {
    localStorage.setItem("cc-switch-density", "spacious");
    localStorage.setItem("cc-switch-zoom", "3");
    const prefs = await load();
    prefs.initializeInterfacePrefs();

    expect(document.documentElement.dataset.density).toBe("compact");
    expect(setZoomMock).not.toHaveBeenCalled();
  });

  it("persists a density change and applies it to <html>", async () => {
    const prefs = await load();
    prefs.setDensity("comfortable");

    expect(document.documentElement.dataset.density).toBe("comfortable");
    expect(localStorage.getItem("cc-switch-density")).toBe("comfortable");
  });

  it("steps zoom one level at a time and stops at both ends", async () => {
    const prefs = await load();

    prefs.stepZoom(1);
    prefs.stepZoom(1);
    prefs.stepZoom(1);
    expect(setZoomMock).toHaveBeenLastCalledWith(1.25);
    expect(localStorage.getItem("cc-switch-zoom")).toBe("1.25");

    setZoomMock.mockClear();
    prefs.stepZoom(1);
    expect(setZoomMock).not.toHaveBeenCalled();

    prefs.setZoom(1);
    prefs.stepZoom(-1);
    expect(setZoomMock).toHaveBeenLastCalledWith(0.9);
    setZoomMock.mockClear();
    prefs.stepZoom(-1);
    expect(setZoomMock).not.toHaveBeenCalled();
  });
});
