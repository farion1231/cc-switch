import { describe, expect, it } from "vitest";
import {
  runningWslDistros,
  isTargetRunning,
  visibleTargetKeys,
  wslCandidates,
  isTargetProviderVisible,
  type AppTargetsConfig,
  type AppTargetsOverview,
  type AppTargetsView,
} from "@/lib/api/appTargets";
import type { Provider } from "@/types";

const config: AppTargetsConfig = {
  primary: "windows",
  windows: { enabled: true },
  wsl: [
    { distro: "Ubuntu", enabled: true, route: true },
    {
      distro: "Debian",
      enabled: true,
      dir: "\\\\wsl.localhost\\Debian\\home\\me\\.claude",
    },
  ],
};

function overview(
  running: string[],
): Pick<AppTargetsOverview, "inventory" | "installs"> {
  return {
    inventory: {
      hostWindows: true,
      wslAvailable: true,
      distros: ["ubuntu", "Debian", "Alpine"].map((distro) => ({
        distro,
        available: true,
        running: running.includes(distro),
        cached: !running.includes(distro),
      })),
    },
    installs: Object.fromEntries(
      ["ubuntu", "Debian", "Alpine"].map((distro) => [
        distro,
        {
          claude: {
            status: "installed",
            version: "2.1.0",
            detectedAt: 1,
            cached: true,
          },
        },
      ]),
    ),
  };
}

describe("WSL visibility", () => {
  it("hides saved targets and cached installations when every distro is stopped", () => {
    const data = overview([]);
    expect(runningWslDistros(data.inventory)).toEqual([]);
    expect(visibleTargetKeys(config, data.inventory)).toEqual(["windows"]);
    expect(wslCandidates(config, data, "claude")).toEqual([]);
  });

  it("shows only running distros, matching saved target names without case sensitivity", () => {
    const data = overview(["ubuntu", "Alpine"]);
    expect(visibleTargetKeys(config, data.inventory)).toEqual([
      "windows",
      "wsl:Ubuntu",
    ]);
    expect(wslCandidates(config, data, "claude")).toEqual(["Ubuntu", "Alpine"]);
  });

  it("keeps saved choices across shutdown and startup without treating hidden targets as removed", () => {
    const saved = structuredClone(config);
    expect(visibleTargetKeys(config, overview([]).inventory)).toEqual([
      "windows",
    ]);
    expect(
      visibleTargetKeys(config, overview(["ubuntu", "Debian"]).inventory),
    ).toEqual(["windows", "wsl:Ubuntu", "wsl:Debian"]);
    expect(config).toEqual(saved);
  });

  it("does not show cached WSL targets while runtime detection is unavailable", () => {
    const data = overview(["ubuntu"]);
    data.inventory.wslAvailable = false;
    expect(visibleTargetKeys(config, data.inventory)).toEqual(["windows"]);
    expect(wslCandidates(config, data, "claude")).toEqual([]);
  });

  it.each(["macOS", "Linux"])(
    "preserves saved targets and primary-directory visibility on %s",
    () => {
      const data = overview([]);
      data.inventory = { hostWindows: false, wslAvailable: false, distros: [] };
      const saved: AppTargetsConfig = {
        ...config,
        primary: "wsl:Ubuntu",
        windows: { enabled: false },
      };
      expect(isTargetRunning(saved.primary, data.inventory)).toBe(true);
      expect(visibleTargetKeys(saved, data.inventory)).toEqual([
        "wsl:Ubuntu",
        "wsl:Debian",
      ]);
      expect(wslCandidates(saved, data, "claude")).toEqual([
        "Ubuntu",
        "Debian",
      ]);
    },
  );

  it("does not apply a Windows-only filter before the host platform is known", () => {
    expect(visibleTargetKeys(config)).toEqual([
      "windows",
      "wsl:Ubuntu",
      "wsl:Debian",
    ]);
    expect(wslCandidates(config, undefined, "claude")).toEqual([
      "Ubuntu",
      "Debian",
    ]);
  });
});

describe("WSL imported provider visibility", () => {
  const provider: Provider = {
    id: "imported",
    name: "Renamed provider",
    settingsConfig: {},
    meta: { importedFromTarget: "wsl:Ubuntu" },
  };
  const view: AppTargetsView = {
    app: "codex",
    source: "saved",
    config,
    resolvedDirs: {},
    records: {
      "wsl:Ubuntu": { providerId: provider.id, status: "ok", at: 1 },
    },
  };
  const sharedIds = new Set<string>();

  it("hides renamed imports after shutdown and restores them after startup without changing saved data", () => {
    const saved = structuredClone({ provider, view });
    expect(
      isTargetProviderVisible(
        provider,
        view,
        overview([]).inventory,
        sharedIds,
      ),
    ).toBe(false);
    expect(
      isTargetProviderVisible(
        provider,
        view,
        overview(["ubuntu"]).inventory,
        sharedIds,
      ),
    ).toBe(true);
    expect({ provider, view }).toEqual(saved);
  });

  it("recognizes old automatically named imports, including imports no longer selected in WSL", () => {
    const legacy = {
      ...provider,
      meta: undefined,
      name: "WSL · Ubuntu · api.example.com",
    };
    expect(
      isTargetProviderVisible(
        legacy,
        { ...view, records: {} },
        overview([]).inventory,
        sharedIds,
      ),
    ).toBe(false);
  });

  it("keeps ordinary global providers even when a stopped WSL environment selected them", () => {
    expect(
      isTargetProviderVisible(
        { ...provider, meta: undefined },
        view,
        overview([]).inventory,
        sharedIds,
      ),
    ).toBe(true);
  });

  it("keeps an import used by Windows, a route, a failover queue or a stack", () => {
    expect(
      isTargetProviderVisible(
        provider,
        view,
        overview([]).inventory,
        new Set([provider.id]),
      ),
    ).toBe(true);
  });

  it("keeps an import used by another running environment, matching target names case insensitively", () => {
    expect(
      isTargetProviderVisible(
        provider,
        {
          ...view,
          records: {
            ...view.records,
            "wsl:debian": { providerId: provider.id, status: "ok", at: 1 },
          },
        },
        overview(["Debian"]).inventory,
        sharedIds,
      ),
    ).toBe(true);
  });

  it("uses explicit Windows provenance before interpreting a legacy-looking name", () => {
    expect(
      isTargetProviderVisible(
        {
          ...provider,
          name: "WSL · Ubuntu · api.example.com",
          meta: { importedFromTarget: "windows" },
        },
        view,
        overview([]).inventory,
        sharedIds,
      ),
    ).toBe(true);
  });

  it.each(["macOS", "Linux"])("preserves imported cards on %s", () => {
    expect(
      isTargetProviderVisible(
        provider,
        view,
        { hostWindows: false, wslAvailable: false, distros: [] },
        sharedIds,
      ),
    ).toBe(true);
  });
});
