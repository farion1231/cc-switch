/**
 * 模型层级路由配置管理 Hook。
 *
 * `enabled` 同时是首页「Provider 路由 / 模型层级路由」的模式开关：
 * - enabled=false（默认）：provider 模式，走现有单 provider 选择。
 * - enabled=true：模型层级模式，按 Claude 层级（opus/sonnet/haiku/fable）路由到不同 provider。
 *
 * 由 App 顶层持有，把 `config` 与 `updateRoute` 传给首页的 `ModelTierRoutingEditor`。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
  settingsApi,
  type ModelTierRoutingConfig,
  type ModelTierRoutingProfile,
} from "@/lib/api/settings";
import type { AppId } from "@/lib/api/types";

export type ModelTierRoutingApp = Extract<AppId, "claude" | "claude-desktop">;
export const MODEL_TIER_ROUTING_APPS: ModelTierRoutingApp[] = [
  "claude",
  "claude-desktop",
];
export const DEFAULT_MODEL_TIER_PROFILE_ID = "default";

const EMPTY_CONFIG: ModelTierRoutingConfig = {
  enabled: false,
  enabledApps: {},
  routes: {},
  profiles: [
    {
      id: DEFAULT_MODEL_TIER_PROFILE_ID,
      name: "Default",
      routes: {},
    },
  ],
  activeProfileByApp: {
    claude: DEFAULT_MODEL_TIER_PROFILE_ID,
    "claude-desktop": DEFAULT_MODEL_TIER_PROFILE_ID,
  },
};

export function supportsModelTierRoutingApp(
  appId: AppId,
): appId is ModelTierRoutingApp {
  return MODEL_TIER_ROUTING_APPS.includes(appId as ModelTierRoutingApp);
}

function materializeEnabledApps(
  config: ModelTierRoutingConfig,
): Record<ModelTierRoutingApp, boolean> {
  const legacyClaudeEnabled =
    config.enabled &&
    !Object.prototype.hasOwnProperty.call(config.enabledApps ?? {}, "claude");
  return {
    claude: config.enabledApps?.claude ?? legacyClaudeEnabled,
    "claude-desktop": config.enabledApps?.["claude-desktop"] ?? false,
  };
}

function uniqueProfileId(
  profiles: ModelTierRoutingProfile[],
  preferred: string,
): string {
  const used = new Set(profiles.map((profile) => profile.id));
  if (preferred && !used.has(preferred)) return preferred;
  const base = preferred || "profile";
  let index = 2;
  while (used.has(`${base}-${index}`)) index += 1;
  return `${base}-${index}`;
}

export function normalizeModelTierRoutingConfig(
  config: ModelTierRoutingConfig | null | undefined,
): ModelTierRoutingConfig {
  const legacyRoutes = config?.routes ?? {};
  const rawProfiles =
    config?.profiles && config.profiles.length > 0
      ? config.profiles
      : [
          {
            id: DEFAULT_MODEL_TIER_PROFILE_ID,
            name: "Default",
            routes: legacyRoutes,
          },
        ];

  const profiles: ModelTierRoutingProfile[] = [];
  for (const [idx, profile] of rawProfiles.entries()) {
    const trimmedId = profile.id?.trim() || `profile-${idx + 1}`;
    const id = uniqueProfileId(profiles, trimmedId);
    profiles.push({
      id,
      name: profile.name ?? "",
      routes: profile.routes ?? {},
    });
  }

  const firstProfileId = profiles[0]?.id ?? DEFAULT_MODEL_TIER_PROFILE_ID;
  const activeProfileByApp = { ...(config?.activeProfileByApp ?? {}) };
  for (const app of MODEL_TIER_ROUTING_APPS) {
    if (!profiles.some((profile) => profile.id === activeProfileByApp[app])) {
      activeProfileByApp[app] = firstProfileId;
    }
  }

  return {
    enabled: config?.enabled ?? false,
    enabledApps: config?.enabledApps ?? {},
    routes: {},
    profiles,
    activeProfileByApp,
  };
}

export function getModelTierRoutingProfiles(
  config: ModelTierRoutingConfig,
): ModelTierRoutingProfile[] {
  return normalizeModelTierRoutingConfig(config).profiles ?? [];
}

export function getActiveModelTierRoutingProfile(
  config: ModelTierRoutingConfig,
  appId: ModelTierRoutingApp,
): ModelTierRoutingProfile {
  const normalized = normalizeModelTierRoutingConfig(config);
  const profiles = normalized.profiles ?? [];
  const activeId = normalized.activeProfileByApp?.[appId];
  return (
    profiles.find((profile) => profile.id === activeId) ??
    profiles[0] ??
    EMPTY_CONFIG.profiles![0]
  );
}

export function isModelTierRoutingEnabledForApp(
  config: ModelTierRoutingConfig | null | undefined,
  appId: AppId,
): boolean {
  if (!config?.enabled || !supportsModelTierRoutingApp(appId)) return false;
  return materializeEnabledApps(normalizeModelTierRoutingConfig(config))[appId];
}

export function useModelTierRouting() {
  const [config, setConfig] = useState<ModelTierRoutingConfig>(EMPTY_CONFIG);
  const [isLoading, setIsLoading] = useState(true);
  const latestConfigRef = useRef<ModelTierRoutingConfig>(EMPTY_CONFIG);
  const saveQueueRef = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    let cancelled = false;
    settingsApi
      .getModelTierRoutingConfig()
      .then((c) => {
        if (!cancelled) {
          const normalized = normalizeModelTierRoutingConfig(c ?? EMPTY_CONFIG);
          latestConfigRef.current = normalized;
          setConfig(normalized);
        }
      })
      .catch((e) => console.error("Failed to load model tier routing:", e))
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = useCallback(async (next: ModelTierRoutingConfig) => {
    const normalized = normalizeModelTierRoutingConfig(next);
    latestConfigRef.current = normalized;
    setConfig(normalized);

    saveQueueRef.current = saveQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        await settingsApi.setModelTierRoutingConfig(latestConfigRef.current);
      })
      .catch(async (e) => {
        console.error("Failed to save model tier routing:", e);
        toast.error(String(e));
        // 回滚到服务端真值
        try {
          const server = await settingsApi.getModelTierRoutingConfig();
          const normalizedServer = normalizeModelTierRoutingConfig(server);
          latestConfigRef.current = normalizedServer;
          setConfig(normalizedServer);
        } catch {
          /* ignore */
        }
      });
    await saveQueueRef.current;
  }, []);

  const setEnabled = useCallback(
    (appId: ModelTierRoutingApp, enabled: boolean) => {
      const base = normalizeModelTierRoutingConfig(config);
      const enabledApps = {
        ...materializeEnabledApps(base),
        [appId]: enabled,
      };
      const anyEnabled = MODEL_TIER_ROUTING_APPS.some(
        (app) => enabledApps[app],
      );
      return persist({ ...base, enabled: anyEnabled, enabledApps });
    },
    [config, persist],
  );

  return { config, isLoading, setEnabled, persist };
}
