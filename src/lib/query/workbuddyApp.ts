import { useQuery, type QueryClient } from "@tanstack/react-query";
import { workbuddyAppApi } from "@/lib/api/workbuddyApp";

export const workbuddyAppKeys = {
  status: ["workbuddy-app", "status"] as const,
};

/** 本机检测：只起几个本地子进程，切回窗口时顺手刷新「运行中」。 */
export function useWorkBuddyAppStatus() {
  return useQuery({
    queryKey: workbuddyAppKeys.status,
    queryFn: () => workbuddyAppApi.getStatus(),
    staleTime: 15_000,
  });
}

export const refreshWorkBuddyApp = (queryClient: QueryClient) =>
  queryClient.invalidateQueries({ queryKey: workbuddyAppKeys.status });
