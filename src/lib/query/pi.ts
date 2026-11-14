import { useQuery, type QueryClient } from "@tanstack/react-query";
import { piApi, type PiFamilyAppId } from "@/lib/api/pi";

export const piKeys = {
  all: ["pi"] as const,
  currentState: ["pi", "currentState"] as const,
  sessionDiscovery: ["pi", "sessionDiscovery"] as const,
};

export const invalidatePiProviderCaches = async (
  queryClient: QueryClient,
  app: PiFamilyAppId = "pi",
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: [app, "currentState"] }),
    queryClient.invalidateQueries({ queryKey: ["providers", app] }),
  ]);
};

export const invalidatePiDirectoryCaches = async (
  queryClient: QueryClient,
  app: PiFamilyAppId = "pi",
) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: [app] }),
    queryClient.invalidateQueries({ queryKey: ["providers", app] }),
    queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
    queryClient.invalidateQueries({ queryKey: ["sessions"] }),
  ]);
};

export function usePiCurrentState(enabled = true, app: PiFamilyAppId = "pi") {
  return useQuery({
    queryKey: [app, "currentState"],
    queryFn: () => piApi.getCurrentState(app),
    enabled,
  });
}
