import { useTranslation } from "react-i18next";
import { useMemo } from "react";
import { useProvidersQuery } from "@/lib/query/queries";
import type { OpenClawProviderConfig } from "@/types";
import { getLocaleFromLanguage } from "@/lib/locale";

export interface ModelOption {
  value: string; // "providerId/modelId"
  label: string; // "Provider Name / Model Name"
}

export function useOpenClawModelOptions(): {
  options: ModelOption[];
  isLoading: boolean;
} {
  const { i18n } = useTranslation();
  const { data: providersData, isLoading } = useProvidersQuery("openclaw");

  const options = useMemo<ModelOption[]>(() => {
    const allProviders = providersData?.providers;
    if (!allProviders) return [];

    const dedupedOptions = new Map<string, string>();

    for (const [providerKey, provider] of Object.entries(allProviders)) {
      let config: OpenClawProviderConfig;
      try {
        config =
          typeof provider.settingsConfig === "string"
            ? (JSON.parse(provider.settingsConfig) as OpenClawProviderConfig)
            : (provider.settingsConfig as OpenClawProviderConfig);
      } catch {
        continue;
      }

      const models = config.models;
      if (!Array.isArray(models)) continue;

      const providerDisplayName =
        typeof provider.name === "string" && provider.name.trim()
          ? provider.name
          : providerKey;

      for (const model of models) {
        if (!model.id) continue;
        const value = `${providerKey}/${model.id}`;
        const modelDisplayName =
          typeof model.name === "string" && model.name.trim()
            ? model.name
            : model.id;
        const label = `${providerDisplayName} / ${modelDisplayName}`;

        if (!dedupedOptions.has(value)) {
          dedupedOptions.set(value, label);
        }
      }
    }

    return Array.from(dedupedOptions.entries())
      .map(([value, label]) => ({ value, label }))
      .sort((a, b) =>
        a.label.localeCompare(
          b.label,
          getLocaleFromLanguage(i18n.resolvedLanguage || i18n.language || "en"),
        ),
      );
  }, [providersData?.providers, i18n.language]);

  return { options, isLoading };
}
