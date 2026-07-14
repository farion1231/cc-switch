import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import type { OpenCodeProviderConfig } from "@/types";
import { OPENCODE_PRESET_MODEL_VARIANTS } from "@/config/opencodeProviderPresets";
import { parseOpencodeConfigStrict } from "../helpers/opencodeFormUtils";

interface UseOmoModelSourceParams {
  isOmoCategory: boolean;
  providerId?: string;
}

interface OmoModelBuild {
  options: Array<{ value: string; label: string }>;
  variantsMap: Record<string, string[]>;
  presetMetaMap: Record<
    string,
    {
      options?: Record<string, unknown>;
      limit?: { context?: number; output?: number };
    }
  >;
  parseFailedProviders: string[];
  usedFallbackSource: boolean;
}

export interface OmoModelSourceResult {
  omoModelOptions: Array<{ value: string; label: string }>;
  omoModelVariantsMap: Record<string, string[]>;
  omoPresetMetaMap: Record<
    string,
    {
      options?: Record<string, unknown>;
      limit?: { context?: number; output?: number };
    }
  >;
  existingOpencodeKeys: string[];
}

export function useOmoModelSource({
  isOmoCategory,
  providerId,
}: UseOmoModelSourceParams): OmoModelSourceResult {
  const { t } = useTranslation();

  const { data: opencodeProvidersData } = useProvidersQuery("opencode");
  const existingOpencodeKeys = useMemo(() => {
    if (!opencodeProvidersData?.providers) return [];
    return Object.keys(opencodeProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [opencodeProvidersData?.providers, providerId]);

  const [enabledOpencodeProviderIds, setEnabledOpencodeProviderIds] = useState<
    string[] | null
  >(null);
  const [omoLiveIdsLoadFailed, setOmoLiveIdsLoadFailed] = useState(false);
  const lastOmoModelSourceWarningRef = useRef<string>("");

  useEffect(() => {
    let active = true;
    if (!isOmoCategory) {
      setEnabledOpencodeProviderIds(null);
      setOmoLiveIdsLoadFailed(false);
      return () => {
        active = false;
      };
    }

    setEnabledOpencodeProviderIds(null);
    setOmoLiveIdsLoadFailed(false);

    (async () => {
      try {
        const ids = await providersApi.getOpenCodeLiveProviderIds();
        if (active) {
          setEnabledOpencodeProviderIds(ids);
        }
      } catch (error) {
        console.warn(
          "[OMO_MODEL_SOURCE_LIVE_IDS_FAILED] failed to load live provider ids",
          error,
        );
        if (active) {
          setOmoLiveIdsLoadFailed(true);
          setEnabledOpencodeProviderIds(null);
        }
      }
    })();

    return () => {
      active = false;
    };
  }, [isOmoCategory]);

  const omoModelBuild = useMemo<OmoModelBuild>(() => {
    const empty: OmoModelBuild = {
      options: [],
      variantsMap: {},
      presetMetaMap: {},
      parseFailedProviders: [],
      usedFallbackSource: false,
    };
    if (!isOmoCategory) {
      return empty;
    }

    const allProviders = opencodeProvidersData?.providers;
    if (!allProviders) {
      return empty;
    }

    const shouldFilterByLive = !omoLiveIdsLoadFailed;
    if (shouldFilterByLive && enabledOpencodeProviderIds === null) {
      return empty;
    }
    const liveSet =
      shouldFilterByLive && enabledOpencodeProviderIds
        ? new Set(enabledOpencodeProviderIds)
        : null;

    const dedupedOptions = new Map<string, string>();
    const variantsMap: Record<string, string[]> = {};
    const presetMetaMap: Record<
      string,
      {
        options?: Record<string, unknown>;
        limit?: { context?: number; output?: number };
      }
    > = {};
    const parseFailedProviders: string[] = [];

    for (const [providerKey, provider] of Object.entries(allProviders)) {
      if (provider.category === "omo" || provider.category === "omo-slim") {
        continue;
      }
      if (liveSet && !liveSet.has(providerKey)) {
        continue;
      }

      let parsedConfig: OpenCodeProviderConfig;
      try {
        parsedConfig = parseOpencodeConfigStrict(provider.settingsConfig);
      } catch (error) {
        parseFailedProviders.push(providerKey);
        console.warn(
          "[OMO_MODEL_SOURCE_PARSE_FAILED] failed to parse provider settings",
          {
            providerKey,
            error,
          },
        );
        continue;
      }
      for (const [modelId, model] of Object.entries(
        parsedConfig.models || {},
      )) {
        const modelName =
          typeof model.name === "string" && model.name.trim()
            ? model.name
            : modelId;
        const providerDisplayName =
          typeof provider.name === "string" && provider.name.trim()
            ? provider.name
            : providerKey;
        const value = `${providerKey}/${modelId}`;
        const label = `${providerDisplayName} / ${modelName} (${modelId})`;
        if (!dedupedOptions.has(value)) {
          dedupedOptions.set(value, label);
        }

        const rawVariants = model.variants;
        if (
          rawVariants &&
          typeof rawVariants === "object" &&
          !Array.isArray(rawVariants)
        ) {
          const variantKeys = Object.keys(rawVariants).filter(Boolean);
          if (variantKeys.length > 0) {
            variantsMap[value] = variantKeys;
          }
        }
      }

      // Preset fallback: for models without config-defined variants,
      // check if the npm package has preset variant definitions.
      // Also collect preset metadata (options, limit) for enrichment.
      const presetModels = OPENCODE_PRESET_MODEL_VARIANTS[parsedConfig.npm];
      if (presetModels) {
        for (const modelId of Object.keys(parsedConfig.models || {})) {
          const fullKey = `${providerKey}/${modelId}`;
          const preset = presetModels.find((p) => p.id === modelId);
          if (!preset) continue;

          // Variant fallback
          if (!variantsMap[fullKey] && preset.variants) {
            const presetKeys = Object.keys(preset.variants).filter(Boolean);
            if (presetKeys.length > 0) {
              variantsMap[fullKey] = presetKeys;
            }
          }

          // Collect preset metadata for model enrichment
          const meta: (typeof presetMetaMap)[string] = {};
          if (preset.options) meta.options = preset.options;
          if (preset.contextLimit || preset.outputLimit) {
            meta.limit = {};
            if (preset.contextLimit) meta.limit.context = preset.contextLimit;
            if (preset.outputLimit) meta.limit.output = preset.outputLimit;
          }
          if (Object.keys(meta).length > 0) {
            presetMetaMap[fullKey] = meta;
          }
        }
      }
    }

    return {
      options: Array.from(dedupedOptions.entries())
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label, "zh-CN")),
      variantsMap,
      presetMetaMap,
      parseFailedProviders,
      usedFallbackSource: omoLiveIdsLoadFailed,
    };
  }, [
    isOmoCategory,
    opencodeProvidersData?.providers,
    enabledOpencodeProviderIds,
    omoLiveIdsLoadFailed,
  ]);

  // Warning toast for parse failures / fallback
  useEffect(() => {
    if (!isOmoCategory) return;
    const failed = omoModelBuild.parseFailedProviders;
    const fallback = omoModelBuild.usedFallbackSource;
    if (failed.length === 0 && !fallback) return;

    const signature = `${fallback ? "fallback:" : ""}${failed
      .slice()
      .sort()
      .join(",")}`;
    if (lastOmoModelSourceWarningRef.current === signature) return;
    lastOmoModelSourceWarningRef.current = signature;

    if (failed.length > 0) {
      toast.warning(
        t("omo.modelSourcePartialWarning", {
          count: failed.length,
          defaultValue:
            "Some provider model configs are invalid and were skipped.",
        }),
      );
    }
    if (fallback) {
      toast.warning(
        t("omo.modelSourceFallbackWarning", {
          defaultValue:
            "Failed to load live provider state. Falling back to configured providers.",
        }),
      );
    }
  }, [
    isOmoCategory,
    omoModelBuild.parseFailedProviders,
    omoModelBuild.usedFallbackSource,
    t,
  ]);

  return {
    omoModelOptions: omoModelBuild.options,
    omoModelVariantsMap: omoModelBuild.variantsMap,
    omoPresetMetaMap: omoModelBuild.presetMetaMap,
    existingOpencodeKeys,
  };
}
