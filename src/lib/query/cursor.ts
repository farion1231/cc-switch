import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  cursorApi,
  type CursorProvider,
  type CursorProviderChanges,
} from "@/lib/api/cursor";

export const cursorKeys = {
  all: ["cursor"] as const,
  endpoints: ["cursor", "endpoints"] as const,
  providers: ["cursor", "providers"] as const,
  runtime: ["cursor", "runtime"] as const,
};

export const useCursorEndpoints = () =>
  useQuery({
    queryKey: cursorKeys.endpoints,
    queryFn: cursorApi.getEndpoints,
  });

export const useCursorProviders = () =>
  useQuery({
    queryKey: cursorKeys.providers,
    queryFn: cursorApi.getProviders,
  });

export const useCursorRuntimeState = () =>
  useQuery({
    queryKey: cursorKeys.runtime,
    queryFn: cursorApi.getRuntimeState,
    refetchInterval: (query) => {
      const phase = query.state.data?.phase;
      return phase === "running" ||
        phase === "starting" ||
        phase === "restoring"
        ? 2000
        : false;
    },
  });

const useCursorMutation = <TVariables, TResult>(
  mutationFn: (variables: TVariables) => Promise<TResult>,
) => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: cursorKeys.endpoints }),
        queryClient.invalidateQueries({ queryKey: cursorKeys.providers }),
        queryClient.invalidateQueries({ queryKey: cursorKeys.runtime }),
      ]);
    },
  });
};

export const useSaveCursorProvider = () =>
  useCursorMutation((provider: CursorProvider) =>
    cursorApi.saveProvider(provider),
  );

export const useSaveCursorProviders = () =>
  useCursorMutation((changes: CursorProviderChanges) =>
    cursorApi.saveProviders(changes),
  );

export const useDeleteCursorEndpoint = () =>
  useCursorMutation((id: string) => cursorApi.deleteEndpoint(id));

export const useDeleteCursorProvider = () =>
  useCursorMutation((id: string) => cursorApi.deleteProvider(id));

export const useToggleCursorProvider = () =>
  useCursorMutation(({ id, enabled }: { id: string; enabled: boolean }) =>
    cursorApi.setProviderEnabled(id, enabled),
  );

export const useStartCursorRuntime = () =>
  useCursorMutation(() => cursorApi.startRuntime());

export const useStopCursorRuntime = () =>
  useCursorMutation(() => cursorApi.stopRuntime());

export const useInstallCursorCA = () =>
  useCursorMutation(() => cursorApi.installCA());

export const useRemoveCursorCA = () =>
  useCursorMutation(() => cursorApi.removeCA());

export const useTestCursorModel = () =>
  useMutation({
    mutationFn: (providerId: string) => cursorApi.testModel(providerId),
  });
