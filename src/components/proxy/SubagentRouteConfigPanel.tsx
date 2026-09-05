import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2, AlertTriangle } from "lucide-react";
import { toast } from "sonner";
import { useAppProxyConfig, useUpdateAppProxyConfig } from "@/lib/query/proxy";
import { useProvidersQuery } from "@/lib/query/queries";

export interface SubagentRouteConfigPanelProps {
  appType: string;
  disabled?: boolean;
}

export function SubagentRouteConfigPanel({
  appType,
  disabled = false,
}: SubagentRouteConfigPanelProps) {
  const { t } = useTranslation();
  const { data: config, isLoading } = useAppProxyConfig(appType);
  const updateConfig = useUpdateAppProxyConfig();
  const { data: providersData } = useProvidersQuery(
    appType as Parameters<typeof useProvidersQuery>[0],
  );

  const [enabled, setEnabled] = useState(false);
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");

  useEffect(() => {
    if (config) {
      setEnabled(!!config.subagentRoute);
      setProviderId(config.subagentRoute?.providerId ?? "");
      setModel(config.subagentRoute?.model ?? "");
    }
  }, [config]);

  // 目标供应商候选：排除当前供应商，按名称排序
  const providers = useMemo(() => {
    const all = Object.values(providersData?.providers ?? {});
    return all
      .filter((p) => p.id !== providersData?.currentProviderId)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [providersData]);

  const targetMissing =
    enabled && providerId !== "" && !providers.some((p) => p.id === providerId);

  const handleSave = async () => {
    if (!config) return;
    const trimmed = model.trim();
    try {
      await updateConfig.mutateAsync({
        ...config,
        subagentRoute:
          enabled && providerId
            ? { providerId, model: trimmed ? trimmed : null }
            : null,
      });
      toast.success(t("proxy.settings.toast.saved"), { closeButton: true });
    } catch (e) {
      toast.error(t("proxy.settings.toast.saveFailed", { error: String(e) }));
    }
  };

  if (isLoading || !config) return null;

  return (
    <div
      className={
        disabled ? "space-y-3 opacity-60 pointer-events-none" : "space-y-3"
      }
    >
      <div className="flex items-center justify-between">
        <Label htmlFor="subagent-route-enabled">
          {t("proxy.subagentRoute.title")}
        </Label>
        <Switch
          id="subagent-route-enabled"
          checked={enabled}
          onCheckedChange={setEnabled}
          disabled={disabled}
        />
      </div>

      {enabled && !disabled && (
        <p className="text-xs text-muted-foreground">
          {t("proxy.subagentRoute.description")}
        </p>
      )}

      {enabled && (
        <>
          <div className="space-y-1">
            <Label>{t("proxy.subagentRoute.targetProvider")}</Label>
            <Select
              value={providerId}
              onValueChange={setProviderId}
              disabled={disabled}
            >
              <SelectTrigger data-testid="subagent-route-provider-trigger">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {providers.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <Label htmlFor="subagent-route-model">
              {t("proxy.subagentRoute.model")}
            </Label>
            <Input
              id="subagent-route-model"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              disabled={disabled}
              data-testid="subagent-route-model-input"
              placeholder={t("proxy.subagentRoute.modelPlaceholder")}
            />
          </div>

          {targetMissing && (
            <Alert>
              <AlertTriangle className="h-4 w-4" />
              <AlertDescription>
                {t("proxy.subagentRoute.targetMissingWarning")}
              </AlertDescription>
            </Alert>
          )}
          {!targetMissing && enabled && providerId && !model.trim() && (
            <Alert>
              <AlertTriangle className="h-4 w-4" />
              <AlertDescription>
                {t("proxy.subagentRoute.modelRequiredHint")}
              </AlertDescription>
            </Alert>
          )}
        </>
      )}

      <Button
        onClick={handleSave}
        disabled={disabled || updateConfig.isPending}
      >
        {updateConfig.isPending ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Save className="h-4 w-4" />
        )}
        {t("common.save", "保存")}
      </Button>
    </div>
  );
}
