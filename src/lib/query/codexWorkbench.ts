import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { codexWorkbenchApi } from "@/lib/api/codexWorkbench";
import type {
  CodexWorkbenchSettings,
  ScriptInstallRequest,
} from "@/types/codexWorkbench";

export const codexWorkbenchKeys = {
  all: ["codexWorkbench"] as const,
  status: () => [...codexWorkbenchKeys.all, "status"] as const,
  settings: () => [...codexWorkbenchKeys.all, "settings"] as const,
  radar: () => [...codexWorkbenchKeys.all, "radar"] as const,
  scripts: () => [...codexWorkbenchKeys.all, "scripts"] as const,
  market: () => [...codexWorkbenchKeys.all, "market"] as const,
  pluginHome: () => [...codexWorkbenchKeys.all, "pluginHome"] as const,
  pluginMarket: () => [...codexWorkbenchKeys.all, "pluginMarket"] as const,
  pluginCaches: () => [...codexWorkbenchKeys.all, "pluginCaches"] as const,
};

export function useCodexWorkbenchStatusQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.status(),
    queryFn: () => codexWorkbenchApi.getStatus(),
    refetchInterval: 3000,
    enabled,
  });
}

export function useCodexWorkbenchSettingsQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.settings(),
    queryFn: () => codexWorkbenchApi.getSettings(),
    enabled,
  });
}

export function useUpdateCodexWorkbenchSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (settings: CodexWorkbenchSettings) =>
      codexWorkbenchApi.updateSettings(settings),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.settings() });
    },
  });
}

export function useLaunchEnhancedCodex() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => codexWorkbenchApi.launchEnhanced(),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.status() });
    },
  });
}

export function useReinjectCodexEnhancements() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => codexWorkbenchApi.reinject(),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.status() });
    },
  });
}

async function afterScriptMutation(qc: ReturnType<typeof useQueryClient>) {
  void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.scripts() });
  try {
    await codexWorkbenchApi.reinjectAfterScriptChange();
    void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.status() });
  } catch {
    // reinject is best-effort when enhanced Codex is not running
  }
}

export function useCodexUserScriptsQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.scripts(),
    queryFn: () => codexWorkbenchApi.listScripts(),
    enabled,
  });
}

export function useCodexScriptMarketQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.market(),
    queryFn: () => codexWorkbenchApi.getMarketCache(),
    // cache only — never auto-refresh market
    enabled,
  });
}

export function useRefreshCodexScriptMarket() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => codexWorkbenchApi.refreshMarket(),
    onSuccess: (data) => {
      qc.setQueryData(codexWorkbenchKeys.market(), data);
    },
  });
}

export function useInstallCodexMarketScript() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (request: ScriptInstallRequest) =>
      codexWorkbenchApi.installMarketScript(request),
    onSuccess: async () => {
      await afterScriptMutation(qc);
    },
  });
}

export function useSetCodexUserScriptEnabled() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ key, enabled }: { key: string; enabled: boolean }) =>
      codexWorkbenchApi.setScriptEnabled(key, enabled),
    onSuccess: async () => {
      await afterScriptMutation(qc);
    },
  });
}

export function useDeleteCodexUserScript() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (key: string) => codexWorkbenchApi.deleteScript(key),
    onSuccess: async () => {
      await afterScriptMutation(qc);
    },
  });
}

export function useImportCodexUserScript() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      sourcePath,
      key,
    }: {
      sourcePath: string;
      key?: string;
    }) => codexWorkbenchApi.importScript(sourcePath, key),
    onSuccess: async () => {
      await afterScriptMutation(qc);
    },
  });
}

export function useGetCodexScriptsDir() {
  return useMutation({
    mutationFn: () => codexWorkbenchApi.getScriptsDir(),
  });
}

export function useCodexEffectiveHome(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.pluginHome(),
    queryFn: () => codexWorkbenchApi.getEffectiveHome(),
    enabled,
  });
}

export function useCodexPluginMarketplaceStatus(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.pluginMarket(),
    queryFn: () => codexWorkbenchApi.getPluginMarketplaceStatus(),
    enabled,
  });
}

export function useCodexPluginCaches(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.pluginCaches(),
    queryFn: () => codexWorkbenchApi.listPluginCaches(),
    enabled,
  });
}

export function useInitializeCodexPluginMarketplace() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => codexWorkbenchApi.initializePluginMarketplace(),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: codexWorkbenchKeys.pluginMarket() });
      await qc.invalidateQueries({ queryKey: codexWorkbenchKeys.pluginCaches() });
      await qc.invalidateQueries({ queryKey: codexWorkbenchKeys.pluginHome() });
    },
  });
}

export function useRefreshCodexPluginCache() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (pluginId: string) =>
      codexWorkbenchApi.refreshPluginCache(pluginId),
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: codexWorkbenchKeys.pluginCaches() });
    },
  });
}

export function useCodexRadarQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.radar(),
    queryFn: () => codexWorkbenchApi.getRadar(false),
    enabled,
    staleTime: 5 * 60 * 1000,
  });
}

export function useRefreshCodexRadar() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => codexWorkbenchApi.getRadar(true),
    onSuccess: (data) => {
      qc.setQueryData(codexWorkbenchKeys.radar(), data);
    },
  });
}

