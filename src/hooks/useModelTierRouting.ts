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

const EMPTY_CONFIG: ModelTierRoutingConfig = {
  enabled: false,
  routes: {},
};

/** 当前仅 Claude 有层级语义；UI 只编辑 claude 的路由表。 */
export const ROUTING_APP = "claude";

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
    (enabled: boolean) => persist({ ...config, enabled }),
    [config, persist],
  );

  return { config, isLoading, setEnabled, persist };
}
