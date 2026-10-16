import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ToolVersion } from "@/components/apps/useToolManagement";
import type { ToolInstallationReport } from "@/lib/api/settings";

const api = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  listToolInstallations: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ settingsApi: api }));

function version(name: string, found = true): ToolVersion {
  return {
    name,
    version: found ? "1.0.0" : null,
    latest_version: "1.0.0",
    error: found ? null : "not installed or not executable",
    installed_but_broken: false,
    env_type: "windows",
    wsl_distro: null,
    update_blocked_by_node: null,
  };
}

async function mountTools() {
  const { useToolManagement } = await import(
    "@/components/apps/useToolManagement"
  );
  const view = renderHook(useToolManagement);
  await act(async () => {});
  return view;
}

async function advance(ms: number) {
  await act(async () => vi.advanceTimersByTimeAsync(ms));
}

describe("tool detection recovery", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-16T12:00:00Z"));
    api.getToolVersions
      .mockReset()
      .mockImplementation(async (names: string[]) =>
        names.map((name) => version(name, name !== "grok")),
      );
    api.listToolInstallations.mockReset().mockResolvedValue([]);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("retries once, then rechecks only the expired failure when focused", async () => {
    await mountTools();
    await advance(1999);
    expect(api.getToolVersions).toHaveBeenCalledTimes(9);
    await advance(1);
    expect(api.getToolVersions).toHaveBeenCalledTimes(10);
    await advance(29_999);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(api.getToolVersions).toHaveBeenCalledTimes(10);
    await advance(1);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(api.getToolVersions).toHaveBeenCalledTimes(11);
    expect(api.getToolVersions).toHaveBeenLastCalledWith(["grok"], {});
    await advance(60_000);
    expect(api.getToolVersions).toHaveBeenCalledTimes(11);
  });

  it("cleans up its retry and focus listeners when the page unmounts", async () => {
    const view = await mountTools();
    view.unmount();
    await advance(31_000);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
    });
    expect(api.getToolVersions).toHaveBeenCalledTimes(9);
  });

  it("does not overlap an automatic or focus retry with a pending manual probe", async () => {
    const view = await mountTools();
    let resolve!: (tools: ToolVersion[]) => void;
    api.getToolVersions.mockImplementationOnce(
      () => new Promise<ToolVersion[]>((done) => (resolve = done)),
    );
    let manual!: Promise<ToolVersion[]>;
    act(() => {
      manual = view.result.current.retryToolDetection("grok");
    });
    await advance(31_000);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(api.getToolVersions).toHaveBeenCalledTimes(10);
    await act(async () => {
      resolve([version("grok")]);
      await manual;
    });
    expect(view.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
  });

  it("defers the short retry while hidden and resumes it when visible", async () => {
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    await mountTools();
    await advance(5000);
    expect(api.getToolVersions).toHaveBeenCalledTimes(9);
    hidden.mockReturnValue(false);
    await act(async () =>
      document.dispatchEvent(new Event("visibilitychange")),
    );
    await advance(0);
    expect(api.getToolVersions).toHaveBeenCalledTimes(10);
  });

  it("recovers failures cached by the startup background check", async () => {
    const { checkToolUpdatesInBackground } = await import(
      "@/components/apps/useToolManagement"
    );
    await checkToolUpdatesInBackground();
    expect(api.getToolVersions).toHaveBeenCalledTimes(1);
    api.getToolVersions.mockImplementation(async (names: string[]) =>
      names.map((name) => version(name)),
    );
    const view = await mountTools();
    await advance(2000);
    expect(api.getToolVersions).toHaveBeenCalledTimes(2);
    expect(api.getToolVersions).toHaveBeenLastCalledWith(["grok"], {});
    expect(view.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
  });

  it("does not extend healthy caches when a failed tool recovers", async () => {
    await mountTools();
    await advance(2000);
    api.getToolVersions.mockImplementation(async (names: string[]) =>
      names.map((name) => version(name)),
    );
    await advance(31_000);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(api.getToolVersions).toHaveBeenCalledTimes(11);
    await advance(10 * 60_000 - 33_000);
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(api.getToolVersions).toHaveBeenCalledTimes(20);
  });

  it("keeps the recovered installation report when the initial scan arrives late", async () => {
    let finishInitialScan!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishInitialScan = resolve),
        ),
    );
    const view = await mountTools();
    const recovered: ToolInstallationReport = {
      tool: "grok",
      installs: [],
      is_conflict: false,
      needs_confirmation: false,
      command: "grok update",
      anchored: true,
      unmanaged: false,
    };
    api.listToolInstallations.mockResolvedValueOnce([recovered]);
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    expect(view.result.current.installReports?.grok).toEqual(recovered);
    await act(async () =>
      finishInitialScan([{ ...recovered, command: "", anchored: false }]),
    );
    expect(view.result.current.installReports?.grok).toEqual(recovered);
  });

  it("ignores a late failure after a newer probe has recovered the tool", async () => {
    const view = await mountTools();
    let finishOld!: (tools: ToolVersion[]) => void;
    api.getToolVersions.mockImplementationOnce(
      () => new Promise<ToolVersion[]>((resolve) => (finishOld = resolve)),
    );
    let old!: Promise<ToolVersion[]>;
    act(() => {
      old = view.result.current.retryToolDetection("grok");
    });
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await act(async () => view.result.current.retryToolDetection("grok"));
    await act(async () => {
      finishOld([version("grok", false)]);
      await old;
    });
    expect(view.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
    await advance(2000);
    expect(api.getToolVersions).toHaveBeenCalledTimes(11);
  });
});
