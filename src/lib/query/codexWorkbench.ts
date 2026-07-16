import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { codexWorkbenchApi } from "@/lib/api/codexWorkbench";
import type { CodexWorkbenchSettings } from "@/types/codexWorkbench";

export const codexWorkbenchKeys = {
  all: ["codexWorkbench"] as const,
  status: () => [...codexWorkbenchKeys.all, "status"] as const,
  settings: () => [...codexWorkbenchKeys.all, "settings"] as const,
};

/** 工作台状态：仅在页面可见时由调用方开启 refetchInterval */
export function useCodexWorkbenchStatusQuery(opts?: {
  enabled?: boolean;
  refetchInterval?: number | false;
}) {
  return useQuery({
    queryKey: codexWorkbenchKeys.status(),
    queryFn: () => codexWorkbenchApi.getStatus(),
    enabled: opts?.enabled ?? true,
    refetchInterval: opts?.refetchInterval,
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
