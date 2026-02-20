import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { pluginsApi } from "@/lib/api";

export const PLUGINS_QUERY_KEY = ["plugins"] as const;

export function usePluginList() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const unlisten = listen("plugins://changed", () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_QUERY_KEY });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [queryClient]);

  return useQuery({
    queryKey: PLUGINS_QUERY_KEY,
    queryFn: () => pluginsApi.list(),
  });
}

export function useSetPluginEnabled() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ pluginId, enabled }: { pluginId: string; enabled: boolean }) =>
      pluginsApi.setEnabled(pluginId, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: PLUGINS_QUERY_KEY });
    },
  });
}
