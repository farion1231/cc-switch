import { queryOptions } from "@tanstack/react-query";
import { authApi } from "@/lib/api";
import type { ManagedAuthProvider, ManagedAuthStatus } from "@/lib/api/auth";

export const managedAuthStatusQueryKey = (authProvider: ManagedAuthProvider) =>
  ["managed-auth-status", authProvider] as const;

export function managedAuthStatusQuery(authProvider: ManagedAuthProvider) {
  return queryOptions<ManagedAuthStatus>({
    queryKey: managedAuthStatusQueryKey(authProvider),
    queryFn: () => authApi.authGetStatus(authProvider),
    staleTime: 30_000,
  });
}

// The proxy can expire an account or change the default without an Auth Center mutation.
export const xaiAuthStatusQuery = {
  ...managedAuthStatusQuery("xai_oauth"),
  refetchInterval: 15_000,
};
