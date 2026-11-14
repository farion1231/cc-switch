import { describe, expect, it } from "vitest";
import {
  runningWslDistros,
  visibleTargetKeys,
  wslCandidates,
  type AppTargetsConfig,
  type AppTargetsOverview,
} from "@/lib/api/appTargets";

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
    expect(wslCandidates(config, undefined, "claude")).toEqual([]);
  });
});
