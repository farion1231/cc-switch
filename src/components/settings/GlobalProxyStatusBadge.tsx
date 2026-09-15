import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { useUpstreamProxyStatus } from "@/hooks/useGlobalProxy";
import type { UpstreamProxyStatus } from "@/lib/api/globalProxy";

type BadgeVariant = "default" | "secondary" | "destructive" | "outline";

/** 折叠态也能一眼看出出站请求在走什么:自定义代理 / 系统代理 / 代理不可达 / 直连 */
export function resolveGlobalProxyBadge(status: UpstreamProxyStatus): {
  variant: BadgeVariant;
  labelKey: string;
} {
  if (status.mode === "explicit") {
    return {
      variant: "default",
      labelKey: "settings.advanced.globalProxy.badgeExplicit",
    };
  }
  if (status.mode === "system" && status.systemProxyUrl) {
    return status.systemProxyReachable === false
      ? {
          variant: "destructive",
          labelKey: "settings.advanced.globalProxy.badgeUnreachable",
        }
      : {
          variant: "secondary",
          labelKey: "settings.advanced.globalProxy.badgeSystem",
        };
  }
  return {
    variant: "outline",
    labelKey: "settings.advanced.globalProxy.badgeDirect",
  };
}

export function GlobalProxyStatusBadge() {
  const { t } = useTranslation();
  const { data: status } = useUpstreamProxyStatus();
  if (!status) return null;

  const { variant, labelKey } = resolveGlobalProxyBadge(status);
  return (
    <Badge variant={variant} className="h-6 ml-auto mr-2">
      {t(labelKey)}
    </Badge>
  );
}
