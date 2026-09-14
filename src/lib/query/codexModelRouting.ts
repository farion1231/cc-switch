import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { codexModelRoutingApi } from "@/lib/api/codexModelRouting";
import { proxyKeys } from "./proxy";

export const codexModelRoutingKey = ["codexModelRouting"] as const;

export function useCodexModelRouting() {
  return useQuery({
    queryKey: codexModelRoutingKey,
    queryFn: codexModelRoutingApi.get,
  });
}

export function useSaveCodexModelRouting() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: codexModelRoutingApi.save,
    onSuccess: (result) => {
      client.setQueryData(codexModelRoutingKey, result.config);
      client.invalidateQueries({ queryKey: proxyKeys.status });
    },
  });
}

export function useSetCodexModelRoutingEnabled() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: codexModelRoutingApi.setEnabled,
    onSettled: () => {
      client.invalidateQueries({ queryKey: codexModelRoutingKey });
      client.invalidateQueries({ queryKey: proxyKeys.status });
      client.invalidateQueries({ queryKey: proxyKeys.takeoverStatus });
      client.invalidateQueries({ queryKey: proxyKeys.appConfig("codex") });
      client.invalidateQueries({ queryKey: ["providers", "codex"] });
    },
  });
}
