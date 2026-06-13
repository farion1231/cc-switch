import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { sessionsApi } from "@/lib/api/sessions";
import { usageApi } from "@/lib/api/usage";
import type { SetCodexSessionProvidersRequest } from "@/types";

export const codexSessionKeys = {
  provider: (providerId: string) => ["codex-sessions", providerId] as const,
};

export function useProviderCodexSessions(providerId?: string) {
  return useQuery({
    queryKey: providerId
      ? codexSessionKeys.provider(providerId)
      : (["codex-sessions", "none"] as const),
    queryFn: () => sessionsApi.listProviderCodexSessions(providerId!),
    enabled: Boolean(providerId),
  });
}

export function useCodexSessionUsageSummaries() {
  return useQuery({
    queryKey: ["codex-session-usage-summaries"],
    queryFn: () => usageApi.getCodexSessionUsageSummaries(),
  });
}

export function useSetCodexSessionProviderLinks(providerId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: SetCodexSessionProvidersRequest) =>
      sessionsApi.setCodexSessionProviderLinks(request),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: codexSessionKeys.provider(providerId),
      });
    },
  });
}
