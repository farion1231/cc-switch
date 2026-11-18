import type { AppId } from "@/lib/api";
import type { UniversalProviderApps } from "@/types";

export const COMPATIBLE_APP_IDS: readonly AppId[] = [
  "claude",
  "claude-desktop",
  "codex",
  "gemini",
  "grokbuild",
  "opencode",
  "openclaw",
  "hermes",
  "pi",
  "omp",
  "mcode",
  "kimi",
  "dsh",
  "copilot",
  "workbuddy",
];

export const APP_FLAG_KEY: Partial<Record<AppId, keyof UniversalProviderApps>> =
  {
    claude: "claude",
    "claude-desktop": "claudeDesktop",
    codex: "codex",
    gemini: "gemini",
    grokbuild: "grokbuild",
    opencode: "opencode",
    openclaw: "openclaw",
    hermes: "hermes",
    pi: "pi",
    omp: "omp",
    mcode: "mcode",
    kimi: "kimi",
    dsh: "dsh",
    copilot: "copilot",
    workbuddy: "workbuddy",
  };

/** 构造仅包含可见兼容应用的开关对象；未在应用页面开启显示的应用一律置为 `false`（不配置）。 */
export function buildVisibleUniversalApps(
  visibleIds: readonly AppId[],
  seed?: Partial<UniversalProviderApps>,
): UniversalProviderApps {
  const visibleSet = new Set<AppId>(visibleIds);
  const result: UniversalProviderApps = {
    claude: false,
    claudeDesktop: false,
    codex: false,
    gemini: false,
    grokbuild: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    pi: false,
    omp: false,
    mcode: false,
    kimi: false,
    dsh: false,
    copilot: false,
    workbuddy: false,
  };

  for (const appId of COMPATIBLE_APP_IDS) {
    const flag = APP_FLAG_KEY[appId];
    if (!flag) continue;
    if (!visibleSet.has(appId)) {
      result[flag] = false;
      continue;
    }
    result[flag] = seed?.[flag] ?? true;
  }
  return result;
}

export function countVisibleEnabledUniversalApps(
  apps: UniversalProviderApps,
  visibleSet: ReadonlySet<string>,
): number {
  let count = 0;
  for (const appId of COMPATIBLE_APP_IDS) {
    if (!visibleSet.has(appId)) continue;
    const flag = APP_FLAG_KEY[appId];
    if (flag && apps[flag]) {
      count += 1;
    }
  }
  return count;
}
