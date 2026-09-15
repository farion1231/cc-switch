import { useQuery, type QueryClient } from "@tanstack/react-query";
import { dshApi } from "@/lib/api/dsh";

export const dshKeys = {
  all: ["dsh"] as const,
  currentState: ["dsh", "currentState"] as const,
};

export const invalidateDshProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: dshKeys.currentState }),
    queryClient.invalidateQueries({
      queryKey: ["providers", "deepseek-harness"],
    }),
  ]);
};

export function useDshCurrentState(enabled = true) {
  return useQuery({
    queryKey: dshKeys.currentState,
    queryFn: () => dshApi.getCurrentState(),
    enabled,
  });
}
