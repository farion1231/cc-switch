import { useState, useCallback, useMemo } from "react";
import type { OpenClawModel, OpenClawProviderConfig } from "@/types";
import type { AppId } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import {
  OPENCLAW_DEFAULT_CONFIG,
  PI_DEFAULT_CONFIG,
} from "../helpers/opencodeFormUtils";

interface UseOpenclawFormStateParams {
  initialData?: {
    settingsConfig?: Record<string, unknown>;
  };
  appId: AppId;
  providerId?: string;
  onSettingsConfigChange: (config: string) => void;
  getSettingsConfig: () => string;
}

export const OPENCLAW_DEFAULT_USER_AGENT =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:148.0) Gecko/20100101 Firefox/148.0";

export interface OpenclawFormState {
  openclawProviderKey: string;
  setOpenclawProviderKey: (key: string) => void;
  openclawBaseUrl: string;
  openclawApiKey: string;
  openclawApi: string;
  openclawModels: OpenClawModel[];
  openclawUserAgent: boolean;
  existingOpenclawKeys: string[];
  handleOpenclawBaseUrlChange: (baseUrl: string) => void;
  handleOpenclawApiKeyChange: (apiKey: string) => void;
  handleOpenclawApiChange: (api: string) => void;
  handleOpenclawModelsChange: (models: OpenClawModel[]) => void;
  handleOpenclawUserAgentChange: (enabled: boolean) => void;
  resetOpenclawState: (config?: OpenClawProviderConfig) => void;
}

function parseOpenclawField<T>(
  initialData: UseOpenclawFormStateParams["initialData"],
  appId: AppId,
  field: string,
  fallback: T,
): T {
  try {
    const config = JSON.parse(
      initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig)
        : appId === "pi"
          ? PI_DEFAULT_CONFIG
          : OPENCLAW_DEFAULT_CONFIG,
    );
    return (config[field] as T) || fallback;
  } catch {
    return fallback;
  }
}

export function useOpenclawFormState({
  initialData,
  appId,
  providerId,
  onSettingsConfigChange,
  getSettingsConfig,
}: UseOpenclawFormStateParams): OpenclawFormState {
  // Query existing providers for duplicate key checking
  const { data: openclawProvidersData } = useProvidersQuery(
    appId === "pi" ? "pi" : "openclaw",
  );
  const existingOpenclawKeys = useMemo(() => {
    if (!openclawProvidersData?.providers) return [];
    return Object.keys(openclawProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [openclawProvidersData?.providers, providerId]);

  const [openclawProviderKey, setOpenclawProviderKey] = useState<string>(() => {
    if (appId !== "openclaw" && appId !== "pi") return "";
    return providerId || "";
  });

  const [openclawBaseUrl, setOpenclawBaseUrl] = useState<string>(() => {
    if (appId !== "openclaw" && appId !== "pi") return "";
    return parseOpenclawField(initialData, appId, "baseUrl", "");
  });

  const [openclawApiKey, setOpenclawApiKey] = useState<string>(() => {
    if (appId !== "openclaw" && appId !== "pi") return "";
    return parseOpenclawField(initialData, appId, "apiKey", "");
  });

  const [openclawApi, setOpenclawApi] = useState<string>(() => {
    if (appId !== "openclaw" && appId !== "pi") return "openai-completions";
    return parseOpenclawField(
      initialData,
      appId,
      "api",
      appId === "pi" ? "anthropic-messages" : "openai-completions",
    );
  });

  const [openclawModels, setOpenclawModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "openclaw" && appId !== "pi") return [];
    return parseOpenclawField<OpenClawModel[]>(initialData, appId, "models", []);
  });

  const [openclawUserAgent, setOpenclawUserAgent] = useState<boolean>(() => {
    if (appId !== "openclaw" && appId !== "pi") return true;
    const headers = parseOpenclawField<Record<string, string>>(
      initialData,
      appId,
      "headers",
      {},
    );
    return "User-Agent" in headers;
  });

  const updateOpenclawConfig = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          getSettingsConfig() ||
            (appId === "pi" ? PI_DEFAULT_CONFIG : OPENCLAW_DEFAULT_CONFIG),
        );
        updater(config);
        onSettingsConfigChange(JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [getSettingsConfig, onSettingsConfigChange],
  );

  const handleOpenclawBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpenclawBaseUrl(baseUrl);
      updateOpenclawConfig((config) => {
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpenclawApiKey(apiKey);
      updateOpenclawConfig((config) => {
        config.apiKey = apiKey;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawApiChange = useCallback(
    (api: string) => {
      setOpenclawApi(api);
      updateOpenclawConfig((config) => {
        config.api = api;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawModelsChange = useCallback(
    (models: OpenClawModel[]) => {
      setOpenclawModels(models);
      updateOpenclawConfig((config) => {
        config.models = models;
      });
    },
    [updateOpenclawConfig],
  );

  const handleOpenclawUserAgentChange = useCallback(
    (enabled: boolean) => {
      setOpenclawUserAgent(enabled);
      updateOpenclawConfig((config) => {
        if (enabled) {
          config.headers = { "User-Agent": OPENCLAW_DEFAULT_USER_AGENT };
        } else {
          delete config.headers;
        }
      });
    },
    [updateOpenclawConfig],
  );

  const resetOpenclawState = useCallback((config?: OpenClawProviderConfig) => {
    setOpenclawProviderKey("");
    setOpenclawBaseUrl(config?.baseUrl || "");
    setOpenclawApiKey(config?.apiKey || "");
    setOpenclawApi(
      config?.api || (appId === "pi" ? "anthropic-messages" : "openai-completions"),
    );
    setOpenclawModels(config?.models || []);
    const ua = config?.headers ? "User-Agent" in config.headers : false;
    setOpenclawUserAgent(ua);
  }, [appId]);

  return {
    openclawProviderKey,
    setOpenclawProviderKey,
    openclawBaseUrl,
    openclawApiKey,
    openclawApi,
    openclawModels,
    openclawUserAgent,
    existingOpenclawKeys,
    handleOpenclawBaseUrlChange,
    handleOpenclawApiKeyChange,
    handleOpenclawApiChange,
    handleOpenclawModelsChange,
    handleOpenclawUserAgentChange,
    resetOpenclawState,
  };
}
