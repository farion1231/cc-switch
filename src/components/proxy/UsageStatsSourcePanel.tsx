import { Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useAppProxyConfig, useUpdateAppProxyConfig } from "@/lib/query/proxy";
import type { UsageStatsSource } from "@/types/proxy";

interface UsageStatsSourcePanelProps {
  appType: string;
}

export function UsageStatsSourcePanel({ appType }: UsageStatsSourcePanelProps) {
  const { t } = useTranslation();
  const { data: config, isLoading } = useAppProxyConfig(appType);
  const updateConfig = useUpdateAppProxyConfig();

  const handleChange = async (value: string) => {
    if (!config) return;
    await updateConfig.mutateAsync({
      ...config,
      usageStatsSource: value as UsageStatsSource,
    });
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center rounded-lg border border-border/50 bg-muted/30 p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-border/50 bg-muted/30 p-4">
      <div className="space-y-1">
        <h4 className="text-sm font-semibold">
          {t("proxy.usageStatsSource.title")}
        </h4>
        <p className="text-xs text-muted-foreground">
          {t("proxy.usageStatsSource.description")}
        </p>
      </div>

      <div className="space-y-2">
        <Label htmlFor={`usage-stats-source-${appType}`}>
          {t("proxy.usageStatsSource.label")}
        </Label>
        <Select
          value={config?.usageStatsSource ?? "proxy"}
          onValueChange={(value) => void handleChange(value)}
          disabled={!config || updateConfig.isPending}
        >
          <SelectTrigger
            id={`usage-stats-source-${appType}`}
            className="w-full sm:w-[220px]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="proxy">
              {t("proxy.usageStatsSource.proxy")}
            </SelectItem>
            <SelectItem value="session">
              {t("proxy.usageStatsSource.session")}
            </SelectItem>
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("proxy.usageStatsSource.hint")}
        </p>
      </div>
    </div>
  );
}
