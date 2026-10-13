import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { deviceUsageApi, type DeviceUsageReport } from "@/lib/api/deviceUsage";

export const deviceUsageKeys = {
  all: ["device-usage"] as const,
  report: (scope: string) => ["device-usage", scope] as const,
};
export function useDeviceUsage(scope: string, refreshIntervalMs: number) {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: deviceUsageKeys.report(scope),
    queryFn: async (): Promise<DeviceUsageReport> =>
      scope === "local"
        ? {
            ...(await deviceUsageApi.status()),
            rows: [],
            totals: [],
            conflictingEvents: 0,
          }
        : deviceUsageApi.report(scope),
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
  });
  const refresh = () =>
    client.invalidateQueries({ queryKey: deviceUsageKeys.all });
  const configure = useMutation({
    mutationFn: ({ enabled, name }: { enabled: boolean; name: string }) =>
      deviceUsageApi.configure(enabled, name),
    onSuccess: refresh,
  });
  const sync = useMutation({
    mutationFn: deviceUsageApi.sync,
    onSuccess: refresh,
    onError: refresh,
  });
  return { query, configure, sync };
}
