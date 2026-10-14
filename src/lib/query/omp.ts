import { invoke } from "@tauri-apps/api/core";
import type { QueryClient } from "@tanstack/react-query";
import type { PiCurrentState } from "@/lib/api/pi";

export const ompApi = {
  getCurrentState: () => invoke<PiCurrentState>("get_omp_current_state"),
};
export const invalidateOmpProviderCaches = async (queryClient: QueryClient) => {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["omp", "currentState"] }),
    queryClient.invalidateQueries({ queryKey: ["providers", "omp"] }),
  ]);
};
