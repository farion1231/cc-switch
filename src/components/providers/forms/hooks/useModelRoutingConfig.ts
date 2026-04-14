import { useState, useEffect } from "react";
import type {
  ModelRoutingConfig,
  ModelRoute,
  RouteFallback,
} from "@/types";

export function useModelRoutingConfig(
  initialConfig?: ModelRoutingConfig | null,
) {
  const [config, setConfig] = useState<ModelRoutingConfig>(() => {
    if (initialConfig) {
      return initialConfig;
    }
    return {
      enabled: false,
      routes: [],
      fallback: undefined,
    };
  });

  useEffect(() => {
    if (initialConfig) {
      setConfig(initialConfig);
    }
  }, [initialConfig]);

  const handleConfigChange = (newConfig: ModelRoutingConfig) => {
    setConfig(newConfig);
  };

  const handleAddRoute = () => {
    const newRoute: ModelRoute = {
      sourceModel: "",
      target: {
        baseUrl: "",
        apiFormat: "anthropic",
        modelName: "",
      },
    };
    setConfig((prev) => ({
      ...prev,
      routes: [...prev.routes, newRoute],
    }));
  };

  const handleUpdateRoute = (index: number, route: ModelRoute) => {
    setConfig((prev) => {
      const newRoutes = [...prev.routes];
      newRoutes[index] = route;
      return { ...prev, routes: newRoutes };
    });
  };

  const handleDeleteRoute = (index: number) => {
    setConfig((prev) => ({
      ...prev,
      routes: prev.routes.filter((_, i) => i !== index),
    }));
  };

  const handleDuplicateRoute = (index: number) => {
    setConfig((prev) => {
      const routeToDuplicate = prev.routes[index];
      const newRoutes = [...prev.routes];
      newRoutes.splice(index + 1, 0, { ...routeToDuplicate });
      return { ...prev, routes: newRoutes };
    });
  };

  const handleFallbackChange = (apiFormat: string) => {
    setConfig((prev) => ({
      ...prev,
      fallback: { apiFormat: apiFormat as RouteFallback["apiFormat"] },
    }));
  };

  const handleEnabledChange = (enabled: boolean) => {
    setConfig((prev) => ({ ...prev, enabled }));
  };

  const resetConfig = () => {
    setConfig({
      enabled: false,
      routes: [],
      fallback: undefined,
    });
  };

  return {
    config,
    handleConfigChange,
    handleAddRoute,
    handleUpdateRoute,
    handleDeleteRoute,
    handleDuplicateRoute,
    handleFallbackChange,
    handleEnabledChange,
    resetConfig,
  };
}
