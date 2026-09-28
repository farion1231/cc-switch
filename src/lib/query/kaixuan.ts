import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { kaixuanApi, InstallBundleRequest } from "@/lib/api/kaixuan";
import { toast } from "sonner";

// ============================================================================
// kaixuan bundle query/mutation hooks
// ============================================================================

export const kaixuanKeys = {
  bundles: ["kaixuan", "bundles"] as const,
  gatewayEndpoints: ["kaixuan", "gatewayEndpoints"] as const,
  gatewayHealth: ["kaixuan", "gatewayHealth"] as const,
};

/**
 * 列出已注册的 bundle（一次性，SSOT in Rust）。
 */
export function useKaixuanBundles(enabled = true) {
  return useQuery({
    queryKey: kaixuanKeys.bundles,
    queryFn: () => kaixuanApi.listBundles(),
    enabled,
    staleTime: 5 * 60 * 1000, // 5 分钟；bundle 注册表基本不变
    retry: false,
  });
}

/**
 * 安装 bundle：一键装好两供应商 + 故障转移队列 + auto_failover + 切到 P1。
 *
 * 成功后 invalidate providers/failover/proxy 相关 query，让 UI 反映新供应商。
 */
export function useInstallKaixuanBundle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (request: InstallBundleRequest) => kaixuanApi.installBundle(request),
    onSuccess: (result, vars) => {
      toast.success(
        `kaixuan bundle 装好：P1=${result.primaryProviderId}，缺失 env=${result.missingEnvVars.length}`,
      );
      // 刷新供应商列表 + 故障转移队列 + proxy 状态
      qc.invalidateQueries({ queryKey: ["providers", vars.appType] });
      qc.invalidateQueries({ queryKey: ["failoverQueue", vars.appType] });
      qc.invalidateQueries({ queryKey: ["autoFailover", vars.appType] });
      qc.invalidateQueries({ queryKey: ["proxy", "status"] });
      // 刷 gateway health（install 后健康状态可能变了）
      qc.invalidateQueries({ queryKey: kaixuanKeys.gatewayHealth });
    },
    onError: (err: Error) => {
      toast.error(`kaixuan 安装失败：${err.message}`);
    },
  });
}

// ============================================================================
// gateway health hooks
// ============================================================================

/**
 * 列出已知 gateway endpoint（一次性，SSOT in Rust）。
 */
export function useGatewayEndpoints(enabled = true) {
  return useQuery({
    queryKey: kaixuanKeys.gatewayEndpoints,
    queryFn: () => kaixuanApi.listGatewayEndpoints(),
    enabled,
    staleTime: 10 * 60 * 1000,
    retry: false,
  });
}

/**
 * 探测所有 gateway 的健康状态。每 15 秒轮询一次；窗口聚焦/可见时才轮询。
 */
export function useGatewayHealth(enabled = true) {
  return useQuery({
    queryKey: kaixuanKeys.gatewayHealth,
    queryFn: () => kaixuanApi.probeAllGateways(),
    enabled,
    refetchInterval: 15_000,
    refetchOnWindowFocus: true,
    staleTime: 10_000,
    retry: 1,
  });
}

/**
 * 一键启动本机 8782 网关。成功后刷新 health。
 */
export function useStartLocalGateway() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => kaixuanApi.startLocalGateway(),
    onSuccess: (result) => {
      if (result.success) {
        toast.success(`8782 网关已起（${result.durationMs}ms）`);
      } else {
        toast.error(`8782 启动失败：${result.stderr || result.command}`);
      }
      qc.invalidateQueries({ queryKey: kaixuanKeys.gatewayHealth });
    },
    onError: (err: Error) => {
      toast.error(`启动命令异常：${err.message}`);
    },
  });
}