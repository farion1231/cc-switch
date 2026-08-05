import {
  fetchModelsDevPricing,
  MODELS_DEV_QUERY_KEY,
  MODELS_DEV_STALE_TIME_MS,
  type ModelsDevResponse,
} from "@/lib/modelsDevPricing";
import { queryClient } from "./queryClient";

/**
 * Reuse the same Models.dev cache as the pricing UI without requiring a
 * component-level query subscription. Model forms only need a best-effort
 * snapshot to prefill metadata; failures must never block manual setup.
 */
export function loadModelsDevCatalog(): Promise<ModelsDevResponse> {
  return queryClient.fetchQuery({
    queryKey: MODELS_DEV_QUERY_KEY,
    queryFn: fetchModelsDevPricing,
    staleTime: MODELS_DEV_STALE_TIME_MS,
    retry: 1,
  });
}
