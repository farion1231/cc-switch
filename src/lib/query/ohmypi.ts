import { type QueryClient, useQuery } from "@tanstack/react-query";
import { ohmypiApi } from "@/lib/api/ohmypi";

export const ohmypiKeys = {
  all: ["ohmypi"] as const,
  currentState: ["ohmypi", "currentState"] as const,
};

export const invalidateOhMyPiProviderCaches = async (
  queryClient: QueryClient,
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ohmypiKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "ohmypi"] }),
  ]);
};

export const invalidateOhMyPiDirectoryCaches = async (
  queryClient: QueryClient,
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ohmypiKeys.all }),
    queryClient.invalidateQueries({ queryKey: ["providers", "ohmypi"] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
  ]);
};

export function useOhMyPiCurrentState(enabled = true) {
  return useQuery({
    queryKey: ohmypiKeys.currentState,
    queryFn: () => ohmypiApi.getCurrentState(),
    enabled,
  });
}
