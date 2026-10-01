import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { vpsApi, type VpsServer } from "@/lib/api/vps";

export const vpsKeys = { servers: ["vps", "servers"] as const };

export function useVpsServers() {
  return useQuery({
    queryKey: vpsKeys.servers,
    queryFn: vpsApi.getServers,
    retry: false,
  });
}

function useVpsMutation<T>(mutationFn: (value: T) => Promise<VpsServer[]>) {
  const queryClient = useQueryClient();
  return useMutation({
    gcTime: 0,
    mutationFn: (value: T) => mutationFn(value),
    onError: async () => {
      // Host data may already be committed when deployment fails. Keep writes
      // pending until reconciliation finishes; a failed query blocks stale actions.
      await queryClient.invalidateQueries({ queryKey: vpsKeys.servers });
    },
    onSuccess: async (servers) => {
      queryClient.setQueryData(vpsKeys.servers, servers);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skills", "installed"] }),
        queryClient.invalidateQueries({ queryKey: ["skills", "unmanaged"] }),
      ]);
    },
  });
}

export function useSaveVpsServer() {
  return useVpsMutation(
    ({ server, password }: { server: VpsServer; password?: string }) =>
      vpsApi.saveServer(server, password),
  );
}

export function useDeleteVpsServer() {
  return useVpsMutation(vpsApi.deleteServer);
}
