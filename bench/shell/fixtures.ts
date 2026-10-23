// Deliberately exclude IPC/network timing: this benchmark measures the UI shell.
export const getVersion = async () => "benchmark";
export const useUpdate = () => ({ hasUpdate: false });
export const useSidebarStatus = () => ({
  appStatus: () => ({ mode: "direct", alert: false }),
  todayCost: 20,
  todayTokens: 10000,
  todayRequests: 120,
  todayHitRate: 0.8,
  authNeedsAttention: false,
});
