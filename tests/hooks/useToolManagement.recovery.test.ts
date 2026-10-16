import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ToolVersion } from "@/components/apps/useToolManagement";
import type { ToolInstallationReport } from "@/lib/api/settings";

const api = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  listToolInstallations: vi.fn(),
  probeToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
}));
vi.mock("@/lib/api", () => ({ settingsApi: api }));
vi.mock("@/lib/toast", () => ({
  toast: { info: vi.fn(), warning: vi.fn(), success: vi.fn(), error: vi.fn() },
}));

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

function installationReport(
  path = "E:/tools/grok.exe",
): ToolInstallationReport {
  return {
    tool: "grok",
    installs: [
      {
        path,
        version: "1.0.0",
        runnable: true,
        error: null,
        source: "system",
        is_path_default: true,
      },
    ],
    is_conflict: false,
    needs_confirmation: false,
    command: "grok update",
    anchored: true,
    unmanaged: false,
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
    api.probeToolInstallations.mockReset().mockResolvedValue([]);
    api.runToolLifecycleAction.mockReset().mockResolvedValue(undefined);
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
  it("retains an initial known installation when the automatic retry also fails", async () => {
    let finishScan!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishScan = resolve),
        ),
    );
    const view = await mountTools();
    await advance(2000);
    const report: ToolInstallationReport = {
      tool: "grok",
      installs: [
        {
          path: "E:/tools/grok.exe",
          version: null,
          runnable: false,
          error: null,
          source: "system",
          is_path_default: false,
        },
      ],
      is_conflict: false,
      needs_confirmation: false,
      command: "",
      anchored: false,
      unmanaged: false,
    };
    await act(async () => finishScan([report]));
    expect(view.result.current.installReports?.grok).toEqual(report);
  });

  it("rechecks an expired failed tool while a different focus probe is pending", async () => {
    api.getToolVersions.mockImplementation(async (names: string[]) =>
      names.map((name) => version(name, name !== "grok" && name !== "pi")),
    );
    const view = await mountTools();
    await advance(2000);
    let finishPi!: (tools: ToolVersion[]) => void;
    api.getToolVersions.mockImplementation((names: string[]) =>
      names[0] === "pi"
        ? new Promise<ToolVersion[]>((resolve) => (finishPi = resolve))
        : Promise.resolve(names.map((name) => version(name, name !== "grok"))),
    );
    await advance(30_000);
    await act(async () => window.dispatchEvent(new Event("focus")));
    const grokCalls = () =>
      api.getToolVersions.mock.calls.filter(([names]) =>
        (names as string[]).includes("grok"),
      ).length;
    const beforeRefocus = grokCalls();
    await advance(31_000);
    await act(async () => window.dispatchEvent(new Event("focus")));
    const afterRefocus = grokCalls();
    expect(view.result.current.isLoadingTools).toBe(true);
    await act(async () => finishPi([version("pi")]));
    expect(afterRefocus).toBe(beforeRefocus + 1);
    expect(view.result.current.isLoadingTools).toBe(false);
  });

  it("allows a recovered tool to settle while another automatic retry is slow", async () => {
    api.getToolVersions.mockImplementation(async (names: string[]) =>
      names.map((name) => version(name, name !== "gemini" && name !== "grok")),
    );
    const view = await mountTools();
    let releaseGemini!: () => void;
    const gemini = new Promise<void>((resolve) => (releaseGemini = resolve));
    // Faithful to get_tool_versions: await each requested tool, return one array.
    api.getToolVersions.mockImplementation(async (names: string[]) => {
      const result: ToolVersion[] = [];
      for (const name of names) {
        if (name === "gemini") await gemini;
        result.push(version(name));
      }
      return result;
    });
    await advance(2000);
    const grokWhileGeminiPending =
      view.result.current.toolVersionByName.get("grok")?.version;
    await act(async () => releaseGemini());
    expect(grokWhileGeminiPending).toBe("1.0.0");
  });

  it("does not replace a newer diagnostic report with an older recovery scan", async () => {
    const view = await mountTools();
    let finishRecovery!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishRecovery = resolve),
        ),
    );
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    const old = installationReport();
    const fresh: ToolInstallationReport = {
      ...old,
      installs: [
        ...old.installs,
        {
          ...old.installs[0],
          path: "E:/npm/grok.cmd",
          version: "2.0.0",
          is_path_default: false,
        },
      ],
      is_conflict: true,
      needs_confirmation: true,
    };
    api.probeToolInstallations.mockResolvedValueOnce([fresh]);
    await act(async () => view.result.current.handleDiagnoseAll());
    expect(view.result.current.installReports?.grok).toEqual(fresh);
    await act(async () => finishRecovery([old]));
    expect(view.result.current.installReports?.grok).toEqual(fresh);
  });

  it("schedules recovery after a pending manual probe finishes on an unmounted page", async () => {
    const first = await mountTools();
    let finishManual!: (tools: ToolVersion[]) => void;
    api.getToolVersions.mockImplementationOnce(
      () => new Promise<ToolVersion[]>((resolve) => (finishManual = resolve)),
    );
    let manual!: Promise<ToolVersion[]>;
    act(() => {
      manual = first.result.current.retryToolDetection("grok");
    });
    first.unmount();
    const second = await mountTools();
    expect(api.getToolVersions).toHaveBeenCalledTimes(10);
    await act(async () => {
      finishManual([version("grok", false)]);
      await manual;
    });
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    expect(second.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
  });

  it("allows one new automatic retry after success ends a previous failure episode", async () => {
    const view = await mountTools();
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    expect(view.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
    api.getToolVersions.mockResolvedValueOnce([version("grok", false)]);
    await act(async () => view.result.current.retryToolDetection("grok"));
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    expect(view.result.current.toolVersionByName.get("grok")?.version).toBe(
      "1.0.0",
    );
    expect(api.getToolVersions).toHaveBeenCalledTimes(12);
  });

  it("keeps the newer recovery report when the older initial scan finishes first", async () => {
    let finishInitial!: (reports: ToolInstallationReport[]) => void;
    let finishRecovery!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishInitial = resolve),
        ),
    );
    const view = await mountTools();
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishRecovery = resolve),
        ),
    );
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    const fresh = installationReport();
    await act(async () =>
      finishInitial([{ ...fresh, installs: [], anchored: false }]),
    );
    await act(async () => finishRecovery([fresh]));
    expect(view.result.current.installReports?.grok).toEqual(fresh);
  });

  it("retains a useful initial report when a newer diagnostic fails", async () => {
    let finishInitial!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishInitial = resolve),
        ),
    );
    const view = await mountTools();
    api.probeToolInstallations.mockRejectedValueOnce(new Error("scan failed"));
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    await act(async () => view.result.current.handleDiagnoseAll());
    const report = installationReport();
    await act(async () => finishInitial([report]));
    expect(view.result.current.installReports?.grok).toEqual(report);
    expect(log).toHaveBeenCalled();
  });

  it("uses the latest report for conflict details when an older diagnostic finishes last", async () => {
    const view = await mountTools();
    let finishDiagnostic!: (reports: ToolInstallationReport[]) => void;
    api.probeToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishDiagnostic = resolve),
        ),
    );
    let diagnostic!: Promise<void>;
    act(() => {
      diagnostic = view.result.current.handleDiagnoseAll();
    });
    const fresh = {
      ...installationReport("E:/new/grok.exe"),
      is_conflict: true,
      needs_confirmation: true,
    };
    fresh.installs.push({
      ...fresh.installs[0],
      path: "E:/npm/grok.cmd",
      version: "2.0.0",
      is_path_default: false,
    });
    api.listToolInstallations.mockResolvedValueOnce([fresh]);
    api.getToolVersions.mockResolvedValueOnce([version("grok")]);
    await advance(2000);
    await act(async () => {
      finishDiagnostic([installationReport()]);
      await diagnostic;
    });
    expect(view.result.current.installReports?.grok).toEqual(fresh);
    expect(view.result.current.toolDiagnostics.grok).toEqual(fresh.installs);
  });

  it("does not resurrect an uninstalled path from an earlier scan", async () => {
    api.getToolVersions.mockImplementation(async (names: string[]) =>
      names.map((name) => version(name)),
    );
    let finishInitial!: (reports: ToolInstallationReport[]) => void;
    api.listToolInstallations.mockImplementationOnce(
      () =>
        new Promise<ToolInstallationReport[]>(
          (resolve) => (finishInitial = resolve),
        ),
    );
    const view = await mountTools();
    let finishVersion!: (tools: ToolVersion[]) => void;
    api.getToolVersions.mockImplementationOnce(
      () => new Promise<ToolVersion[]>((resolve) => (finishVersion = resolve)),
    );
    let uninstall!: Promise<void>;
    await act(async () => {
      uninstall = view.result.current.handleRunToolAction(
        ["grok"],
        "uninstall",
      );
    });
    await act(async () => finishInitial([installationReport()]));
    expect(view.result.current.installReports?.grok).toBeUndefined();
    await act(async () => {
      finishVersion([version("grok", false)]);
      await uninstall;
    });
    expect(view.result.current.installReports?.grok).toBeUndefined();
    expect(view.result.current.busyTools.has("grok")).toBe(false);
  });
});
