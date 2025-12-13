import { useCallback, useMemo, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useSettingsQuery, useSaveSettingsMutation } from "@/lib/query";
import type { ConfigDirectorySet, Settings } from "@/types";
import { syncCurrentProvidersLiveSafe } from "@/utils/postChangeSync";
import { settingsApi } from "@/lib/api";

const sanitizeDir = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

const sanitizeId = (value?: string | null): string | undefined => {
  if (!value) return undefined;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : undefined;
};

const clampConfigSetName = (
  value: string | undefined | null,
  fallback: string,
  index: number,
): string => {
  const trimmed = value?.trim();
  const base =
    trimmed && trimmed.length > 0
      ? trimmed
      : index === 0
        ? fallback
        : `${fallback} ${index + 1}`;
  return base.length > 60 ? base.slice(0, 60) : base;
};

const normalizeConfigSetId = (value: string | undefined | null, index: number) => {
  const trimmed = value?.trim();
  if (trimmed && trimmed.length > 0) {
    return trimmed;
  }
  return `configset-${index + 1}`;
};

const reorderSets = (
  sets: ConfigDirectorySet[],
  activeId?: string,
): ConfigDirectorySet[] => {
  if (!activeId) return sets;
  const index = sets.findIndex((set) => set.id === activeId);
  if (index <= 0) return sets;
  const target = sets[index];
  const remaining = [...sets.slice(0, index), ...sets.slice(index + 1)];
  return [target, ...remaining];
};

export interface LaunchConfigSet {
  id: string;
  name: string;
  claudeConfigDir?: string;
  codexConfigDir?: string;
  geminiConfigDir?: string;
  currentProviderClaude?: string;
  currentProviderCodex?: string;
  currentProviderGemini?: string;
}

interface ActivateConfigSetOptions {
  silent?: boolean;
}

interface UseConfigSetsResult {
  configSets: LaunchConfigSet[];
  activeConfigSetId?: string;
  hasMultipleSets: boolean;
  isReady: boolean;
  isActivating: boolean;
  activateConfigSet: (
    setId: string,
    options?: ActivateConfigSetOptions,
  ) => Promise<boolean>;
}

interface BuildResult {
  configSets: LaunchConfigSet[];
  activeConfigSetId?: string;
}

const buildLaunchConfigSets = (
  settings: Settings | undefined,
  defaultName: string,
): BuildResult => {
  if (!settings) return { configSets: [], activeConfigSetId: undefined };

  const rawSets = settings.configDirectorySets ?? [];
  const normalized: ConfigDirectorySet[] = rawSets.map((set, index) => ({
    id: normalizeConfigSetId(set.id, index),
    name: clampConfigSetName(set.name, defaultName, index),
    claudeConfigDir: sanitizeDir(set.claudeConfigDir),
    codexConfigDir: sanitizeDir(set.codexConfigDir),
    geminiConfigDir: sanitizeDir(set.geminiConfigDir),
    currentProviderClaude: sanitizeId(set.currentProviderClaude),
    currentProviderCodex: sanitizeId(set.currentProviderCodex),
    currentProviderGemini: sanitizeId(set.currentProviderGemini),
  }));

  const reordered = reorderSets(normalized, settings.activeConfigDirectorySetId);

  const topLevelDirs = {
    claudeConfigDir: sanitizeDir(settings.claudeConfigDir),
    codexConfigDir: sanitizeDir(settings.codexConfigDir),
    geminiConfigDir: sanitizeDir(settings.geminiConfigDir),
  };
  const topLevelProviders = {
    currentProviderClaude: sanitizeId(settings.currentProviderClaude),
    currentProviderCodex: sanitizeId(settings.currentProviderCodex),
    currentProviderGemini: sanitizeId(settings.currentProviderGemini),
  };

  if (reordered.length === 0) {
    const fallbackSet: LaunchConfigSet = {
      id: normalizeConfigSetId(settings.activeConfigDirectorySetId, 0),
      name: defaultName,
      claudeConfigDir: topLevelDirs.claudeConfigDir,
      codexConfigDir: topLevelDirs.codexConfigDir,
      geminiConfigDir: topLevelDirs.geminiConfigDir,
      currentProviderClaude: topLevelProviders.currentProviderClaude,
      currentProviderCodex: topLevelProviders.currentProviderCodex,
      currentProviderGemini: topLevelProviders.currentProviderGemini,
    };
    return {
      configSets: [fallbackSet],
      activeConfigSetId: fallbackSet.id,
    };
  }

  const [primary, ...rest] = reordered;
  const configSets: LaunchConfigSet[] = [
    {
      id: primary.id,
      name: primary.name,
      claudeConfigDir: topLevelDirs.claudeConfigDir ?? primary.claudeConfigDir,
      codexConfigDir: topLevelDirs.codexConfigDir ?? primary.codexConfigDir,
      geminiConfigDir: topLevelDirs.geminiConfigDir ?? primary.geminiConfigDir,
      currentProviderClaude:
        topLevelProviders.currentProviderClaude ?? primary.currentProviderClaude,
      currentProviderCodex:
        topLevelProviders.currentProviderCodex ?? primary.currentProviderCodex,
      currentProviderGemini:
        topLevelProviders.currentProviderGemini ?? primary.currentProviderGemini,
    },
    ...rest.map((set) => ({
      id: set.id,
      name: set.name,
      claudeConfigDir: set.claudeConfigDir,
      codexConfigDir: set.codexConfigDir,
      geminiConfigDir: set.geminiConfigDir,
      currentProviderClaude: set.currentProviderClaude,
      currentProviderCodex: set.currentProviderCodex,
      currentProviderGemini: set.currentProviderGemini,
    })),
  ];

  const activeConfigSetId =
    configSets.find((set) => set.id === settings.activeConfigDirectorySetId)?.id ??
    configSets[0]?.id;

  return { configSets, activeConfigSetId };
};

const toConfigDirectorySet = (
  set: LaunchConfigSet,
  index: number,
): ConfigDirectorySet => ({
  id: set.id || normalizeConfigSetId(set.id, index),
  name: set.name,
  claudeConfigDir: sanitizeDir(set.claudeConfigDir),
  codexConfigDir: sanitizeDir(set.codexConfigDir),
  geminiConfigDir: sanitizeDir(set.geminiConfigDir),
  currentProviderClaude: sanitizeId(set.currentProviderClaude),
  currentProviderCodex: sanitizeId(set.currentProviderCodex),
  currentProviderGemini: sanitizeId(set.currentProviderGemini),
});

export function useConfigSets(): UseConfigSetsResult {
  const { data } = useSettingsQuery();
  const saveMutation = useSaveSettingsMutation();
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const defaultName = useMemo(
    () =>
      t("settings.configSetDefaultName", {
        defaultValue: "默认环境",
      }),
    [t],
  );

  const { configSets: builtConfigSets, activeConfigSetId } = useMemo(
    () => buildLaunchConfigSets(data, defaultName),
    [data, defaultName],
  );

  const displayOrderRef = useRef<string[]>([]);

  const configSets = useMemo(() => {
    if (!builtConfigSets.length) {
      displayOrderRef.current = [];
      return builtConfigSets;
    }

    const availableIds = builtConfigSets.map((set) => set.id);
    const preservedOrder = displayOrderRef.current.filter((id) =>
      availableIds.includes(id),
    );
    const newIds = availableIds.filter((id) => !preservedOrder.includes(id));
    const nextOrder = [...preservedOrder, ...newIds];

    displayOrderRef.current = nextOrder;

    const orderMap = new Map(nextOrder.map((id, index) => [id, index]));

    return [...builtConfigSets].sort((a, b) => {
      const orderA = orderMap.get(a.id) ?? Number.MAX_SAFE_INTEGER;
      const orderB = orderMap.get(b.id) ?? Number.MAX_SAFE_INTEGER;
      return orderA - orderB;
    });
  }, [builtConfigSets]);

  const fetchLatestSettings = useCallback(async () => {
    try {
      const latest = await settingsApi.get();
      queryClient.setQueryData(["settings"], latest);
      return latest;
    } catch (error) {
      console.error("[useConfigSets] Failed to fetch latest settings", error);
      return undefined;
    }
  }, [queryClient]);

  const activateConfigSet = useCallback(
    async (setId: string, options?: ActivateConfigSetOptions) => {
      const latestSettings = await fetchLatestSettings();
      const sourceSettings = latestSettings ?? data;

      if (!sourceSettings) {
        toast.error(
          t("settings.configSetActivateFailed", {
            defaultValue: "切换环境失败，请稍后重试",
          }),
        );
        return false;
      }

      const {
        configSets: workingSets,
        activeConfigSetId: workingActiveSetId,
      } = buildLaunchConfigSets(sourceSettings, defaultName);

      const target =
        workingSets.find((set) => set.id === setId) ?? workingSets[0];
      if (!target) return false;

      if (setId === workingActiveSetId) {
        return true;
      }

      const others = workingSets.filter((set) => set.id !== target.id);
      const nextSets: ConfigDirectorySet[] = [
        toConfigDirectorySet(target, 0),
        ...others.map((set, index) => toConfigDirectorySet(set, index + 1)),
      ];

      const payload: Settings = {
        ...sourceSettings,
        claudeConfigDir: target.claudeConfigDir,
        codexConfigDir: target.codexConfigDir,
        geminiConfigDir: target.geminiConfigDir,
        currentProviderClaude: target.currentProviderClaude,
        currentProviderCodex: target.currentProviderCodex,
        currentProviderGemini: target.currentProviderGemini,
        configDirectorySets: nextSets,
        activeConfigDirectorySetId: target.id,
      };

      try {
        await saveMutation.mutateAsync(payload);

        const syncResult = await syncCurrentProvidersLiveSafe();
        if (!syncResult.ok) {
          console.warn(
            "[useConfigSets] Failed to sync providers after environment switch",
            syncResult.error,
          );
        }

        if (!options?.silent) {
          toast.success(
            t("settings.configSetActivated", {
              defaultValue: "已切换到 {{name}} 环境",
              name: target.name,
            }),
          );
        }
        return true;
      } catch (error) {
        console.error("[useConfigSets] Failed to activate config set", error);
        toast.error(
          t("settings.configSetActivateFailed", {
            defaultValue: "切换环境失败，请稍后重试",
          }),
        );
        return false;
      }
    },
    [data, defaultName, fetchLatestSettings, saveMutation, t],
  );

  return {
    configSets,
    activeConfigSetId,
    hasMultipleSets: configSets.length > 1,
    isReady: Boolean(data),
    isActivating: saveMutation.isPending,
    activateConfigSet,
  };
}
