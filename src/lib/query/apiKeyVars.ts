import { useContext } from "react";
import {
  QueryClient,
  QueryClientContext,
  useQuery,
} from "@tanstack/react-query";
import { API_KEY_VARS_QUERY_KEY, apiKeyVarsApi } from "@/lib/api/apiKeyVars";

/** 没有 QueryClientProvider 时（独立渲染的表单组件）用的占位客户端，查询保持禁用 */
const detachedClient = new QueryClient();

/** API Key 变量列表（只含掩码值） */
export function useApiKeyVars(enabled = true) {
  const client = useContext(QueryClientContext);
  return useQuery(
    {
      queryKey: API_KEY_VARS_QUERY_KEY,
      queryFn: () => apiKeyVarsApi.list(),
      enabled: enabled && client !== undefined,
      staleTime: 10_000,
    },
    client ?? detachedClient,
  );
}
