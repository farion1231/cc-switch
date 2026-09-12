import { useCallback } from "react";
import {
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from "@tanstack/react-query";
import {
  dshApi,
  type DshCredentialWrite,
  type DshCustomInput,
  type DshDefaultModel,
  type DshModelDiscoveryInput,
  type DshModelDiscoveryResult,
  type DshNativeInput,
  type DshSnapshot,
} from "@/lib/api/dsh";

/** Stable query keys for the live DSH registry. */
export const dshKeys = {
  all: ["dsh"] as const,
  snapshot: ["dsh", "snapshot"] as const,
};

/** Query options for the DSH snapshot. */
export interface UseDshSnapshotOptions {
  enabled?: boolean;
}

/** Read the redacted live DSH snapshot. */
export function useDshSnapshot(
  options: UseDshSnapshotOptions = {},
): UseQueryResult<DshSnapshot> {
  return useQuery({
    queryKey: dshKeys.snapshot,
    queryFn: dshApi.getSnapshot,
    enabled: options.enabled ?? true,
    // DSH watches its files; a short stale window avoids duplicate reads while
    // the page remains responsive to the explicit refresh action.
    staleTime: 1_000,
  });
}

/**
 * Imperative live actions for the DSH page.
 *
 * Secret-bearing operations intentionally are not React Query mutations: a
 * mutation cache would retain the API-key argument. They run as one-shot
 * calls and only invalidate the redacted snapshot afterwards.
 */
export function useDshActions() {
  const queryClient = useQueryClient();

  const commitSnapshot = useCallback(
    async (
      snapshot: DshSnapshot | undefined,
    ): Promise<DshSnapshot | undefined> => {
      if (snapshot) {
        queryClient.setQueryData(dshKeys.snapshot, snapshot);
      } else {
        void queryClient.invalidateQueries({ queryKey: dshKeys.snapshot });
      }
      return snapshot;
    },
    [queryClient],
  );

  const refresh = useCallback(async (): Promise<DshSnapshot> => {
    const snapshot = await dshApi.refresh();
    queryClient.setQueryData(dshKeys.snapshot, snapshot);
    return snapshot;
  }, [queryClient]);

  const upsertNative = useCallback(
    async (input: DshNativeInput): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.upsertNative(input)),
    [commitSnapshot],
  );

  const resetNative = useCallback(
    async (expectedRevision?: string): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.resetNative(expectedRevision)),
    [commitSnapshot],
  );

  const createCustom = useCallback(
    async (input: DshCustomInput): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.createCustom(input)),
    [commitSnapshot],
  );

  const updateCustom = useCallback(
    async (input: DshCustomInput): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.updateCustom(input)),
    [commitSnapshot],
  );

  const removeCustom = useCallback(
    async (
      route: string,
      expectedRevision?: string,
    ): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.removeCustom(route, expectedRevision)),
    [commitSnapshot],
  );

  const setDefaultModel = useCallback(
    async (
      selection: DshDefaultModel,
      expectedRevision?: string,
    ): Promise<DshSnapshot | undefined> =>
      commitSnapshot(await dshApi.setDefaultModel(selection, expectedRevision)),
    [commitSnapshot],
  );

  const setCredential = useCallback(
    async (input: DshCredentialWrite): Promise<void> => {
      // Do not wrap this in useMutation: its variables would put `value` in
      // React Query's mutation cache. The value is released after this await.
      await dshApi.setCredential(input);
      void queryClient.invalidateQueries({ queryKey: dshKeys.snapshot });
    },
    [queryClient],
  );

  const unsetCredential = useCallback(
    async (ref: string, expectedRevision?: string): Promise<void> => {
      await dshApi.unsetCredential(ref, expectedRevision);
      void queryClient.invalidateQueries({ queryKey: dshKeys.snapshot });
    },
    [queryClient],
  );

  const discoverModels = useCallback(
    async (input: DshModelDiscoveryInput): Promise<DshModelDiscoveryResult> =>
      dshApi.discoverModels(input),
    [],
  );

  const openHome = useCallback(() => dshApi.openHome(), []);

  return {
    refresh,
    upsertNative,
    resetNative,
    createCustom,
    updateCustom,
    removeCustom,
    setDefaultModel,
    setCredential,
    unsetCredential,
    discoverModels,
    openHome,
  };
}
