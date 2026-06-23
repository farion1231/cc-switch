/**
 * 模型层级路由配置管理 Hook。
 *
 * `enabled` 同时是首页「Provider 路由 / 模型层级路由」的模式开关：
 * - enabled=false（默认）：provider 模式，走现有单 provider 选择。
 * - enabled=true：模型层级模式，按 Claude 层级（opus/sonnet/haiku/fable）路由到不同 provider。
 *
 * 由 App 顶层持有，把 `config` 与 `updateRoute` 传给首页的 `ModelTierRoutingEditor`。
 */
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { settingsApi, type ModelTierRoutingConfig } from "@/lib/api/settings";
import type { AppId } from "@/lib/api/types";

const EMPTY_CONFIG: ModelTierRoutingConfig = {
  enabled: false,
  enabledApps: {},
  routes: {},
};

export type ModelTierRoutingApp = Extract<AppId, "claude" | "claude-desktop">;
export const MODEL_TIER_ROUTING_APPS: ModelTierRoutingApp[] = [
  "claude",
  "claude-desktop",
];

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

export function isModelTierRoutingEnabledForApp(
  config: ModelTierRoutingConfig | null | undefined,
  appId: AppId,
): boolean {
  if (!config?.enabled || !supportsModelTierRoutingApp(appId)) return false;
  return materializeEnabledApps(config)[appId];
}

export function useModelTierRouting() {
  const [config, setConfig] = useState<ModelTierRoutingConfig>(EMPTY_CONFIG);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    settingsApi
      .getModelTierRoutingConfig()
      .then((c) => {
        if (!cancelled) setConfig(c ?? EMPTY_CONFIG);
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
    setConfig(next);
    try {
      await settingsApi.setModelTierRoutingConfig(next);
    } catch (e) {
      console.error("Failed to save model tier routing:", e);
      toast.error(String(e));
      // 回滚到服务端真值
      try {
        const server = await settingsApi.getModelTierRoutingConfig();
        setConfig(server ?? EMPTY_CONFIG);
      } catch {
        /* ignore */
      }
    }
  }, []);

  const setEnabled = useCallback(
    (appId: ModelTierRoutingApp, enabled: boolean) => {
      const enabledApps = {
        ...materializeEnabledApps(config),
        [appId]: enabled,
      };
      const anyEnabled = MODEL_TIER_ROUTING_APPS.some(
        (app) => enabledApps[app],
      );
      return persist({ ...config, enabled: anyEnabled, enabledApps });
    },
    [config, persist],
  );

  return { config, isLoading, setEnabled, persist };
}
