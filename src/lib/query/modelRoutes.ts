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
 * Set (or clear) the route for a single model class.
 */
export function useSetModelRoute() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      appType,
      modelClass,
      providerId,
      targetModel,
    }: {
      appType: AppId;
      modelClass: ModelClass;
      providerId: string | null;
      targetModel: string | null;
    }) =>
      modelRoutesApi.setModelRoute(
        appType,
        modelClass,
        providerId,
        targetModel,
      ),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({
        queryKey: ["modelRoutes", variables.appType],
      });
    },
  });
}
