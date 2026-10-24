import { type QueryClient, useQuery } from "@tanstack/react-query";
import { ompApi } from "@/lib/api/omp";

export const ompKeys = {
  all: ["omp"] as const,
  currentState: ["omp", "currentState"] as const,
};

export const invalidateOmpProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ompKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "omp"] }),
  ]);
};

export const invalidateOmpDirectoryCaches = async (
  queryClient: QueryClient,
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ompKeys.all }),
    queryClient.invalidateQueries({ queryKey: ["providers", "omp"] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};

export function useOmpCurrentState(enabled = true) {
  return useQuery({
    queryKey: ompKeys.currentState,
    queryFn: () => ompApi.getCurrentState(),
    enabled,
  });
}
