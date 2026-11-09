import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { providersApi } from "@/lib/api/providers";

/**
 * Claude Desktop 配置文件的检查结果（后端 `get_claude_desktop_status`）：平台不支持、profile 里
 * 有 Desktop 会拒收的模型名、路由那家缺映射、地址和应写的不一致。没问题时为空数组。
 */
export function useDesktopConfigMessages(enabled: boolean): string[] {
  const { t } = useTranslation();
  const { data: status } = useQuery({
    queryKey: ["claudeDesktopStatus"],
    queryFn: () => providersApi.getClaudeDesktopStatus(),
    refetchInterval: 5000,
    enabled,
  });

  return useMemo(() => {
    if (!enabled || !status) return [];
    if (!status.supported) {
      return [t("claudeDesktop.statusUnsupported")];
    }
    const list: string[] = [];
    if (status.staleRawModels)
      list.push(t("claudeDesktop.statusStaleRawModels"));
    if (status.missingRouteMappings) {
      list.push(t("claudeDesktop.statusMissingRouteMappings"));
    }
    if (status.mode === "proxy" && !status.gatewayTokenConfigured) {
      list.push(t("claudeDesktop.statusGatewayTokenMissing"));
    }
    const expected = status.expectedBaseUrl?.replace(/\/+$/, "");
    const actual = status.actualBaseUrl?.replace(/\/+$/, "");
    if (expected && actual && expected !== actual) {
      list.push(t("claudeDesktop.statusBaseUrlMismatch", { expected, actual }));
    }
    return list;
  }, [enabled, status, t]);
}
