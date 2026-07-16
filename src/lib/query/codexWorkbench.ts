import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { codexWorkbenchApi } from "@/lib/api/codexWorkbench";
import type { CodexWorkbenchSettings } from "@/types/codexWorkbench";

export const codexWorkbenchKeys = {
  all: ["codexWorkbench"] as const,
  status: () => [...codexWorkbenchKeys.all, "status"] as const,
  settings: () => [...codexWorkbenchKeys.all, "settings"] as const,
};

export function useCodexWorkbenchStatusQuery(enabled = true) {
  return useQuery({
    queryKey: codexWorkbenchKeys.status(),
    queryFn: () => codexWorkbenchApi.getStatus(),
    enabled,
    refetchInterval: 3000,
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
      void qc.invalidateQueries({ queryKey: codexWorkbenchKeys.status() });
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
