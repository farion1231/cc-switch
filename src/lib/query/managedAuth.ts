import { queryOptions, useQuery } from "@tanstack/react-query";
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

interface UseManagedAuthStatusOptions {
  enabled?: boolean;
  refetchInterval?: number | false;
}

/**
 * 读取授权中心某类托管账号的状态，并对禁用态做显式门控。
 *
 * TanStack Query 的 `enabled: false` 只会阻止发起请求，observer 仍会返回同 key 的已有缓存；
 * 禁用时屏蔽 `data` / `isSuccess` / `isError`，防止其他应用或弹窗写入的缓存泄漏到无关卡片。
 */
export function useManagedAuthStatus(
  authProvider: ManagedAuthProvider,
  options?: UseManagedAuthStatusOptions,
) {
  const enabled = options?.enabled ?? true;
  const query = useQuery({
    ...managedAuthStatusQuery(authProvider),
    enabled,
    ...(options?.refetchInterval !== undefined
      ? { refetchInterval: options.refetchInterval }
      : {}),
  });

  return {
    ...query,
    data: enabled ? query.data : undefined,
    isSuccess: enabled ? query.isSuccess : false,
    isError: enabled ? query.isError : false,
  };
}
