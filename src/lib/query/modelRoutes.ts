import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { modelRoutesApi, type ModelClass, type ModelRoutes } from "@/lib/api";
import type { AppId } from "@/lib/api";

/**
 * Get the model-class -> provider routes configured for an app.
 */
export function useModelRoutes(appType: AppId) {
  return useQuery<ModelRoutes>({
    queryKey: ["modelRoutes", appType],
    queryFn: () => modelRoutesApi.getModelRoutes(appType),
    enabled: !!appType,
  });
}

/**
 * Set (or clear) the provider route for a single model class.
 */
export function useSetModelRoute() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      modelClass,
      providerId,
    }: {
      appType: AppId;
      modelClass: ModelClass;
      providerId: string | null;
    }) => modelRoutesApi.setModelRoute(appType, modelClass, providerId),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["modelRoutes", variables.appType],
      });
    },
  });
}
