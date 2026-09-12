import { useQuery, type QueryClient } from "@tanstack/react-query";
import { ompApi, piApi } from "@/lib/api/pi";

export const piKeys = {
  all: ["pi"] as const,
  currentState: ["pi", "currentState"] as const,
  sessionDiscovery: ["pi", "sessionDiscovery"] as const,
};

export const ompKeys = {
  all: ["omp"] as const,
  currentState: ["omp", "currentState"] as const,
};

export const invalidatePiProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "pi"] }),
  ]);
};

export const invalidateOmpProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ompKeys.currentState }),
    queryClient.invalidateQueries({ queryKey: ["providers", "omp"] }),
  ]);
};

export const invalidateNativeProviderCaches = async (
  queryClient: QueryClient,
  appId: "pi" | "omp",
) => {
  if (appId === "pi") {
    await invalidatePiProviderCaches(queryClient);
  } else {
    await invalidateOmpProviderCaches(queryClient);
  }
};

export const usePiCurrentState = (enabled = true) =>
  useQuery({
    queryKey: piKeys.currentState,
    queryFn: () => piApi.getCurrentState(),
    enabled,
  });

export const useOmpCurrentState = (enabled = true) =>
  useQuery({
    queryKey: ompKeys.currentState,
    queryFn: () => ompApi.getCurrentState(),
    enabled,
  });
export const invalidatePiDirectoryCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: piKeys.all }),
    queryClient.invalidateQueries({ queryKey: ["providers", "pi"] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};
