import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { RefreshCw, Activity } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { toast } from "sonner";

interface OrchestrationStatus {
  enabled: boolean;
}

export function OrchestrationPanel() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<OrchestrationStatus | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      setLoading(true);
      const result = await invoke<OrchestrationStatus>("orchestration_status");
      setStatus(result);
    } catch (err) {
      console.error("[OrchestrationPanel] Failed to fetch status:", err);
      toast.error(
        t("orchestration.reloadFailed", {
          defaultValue: "Failed to fetch status",
          error: String(err),
        }),
      );
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    let active = true;
    invoke<OrchestrationStatus>("orchestration_status")
      .then((result) => {
        if (active) setStatus(result);
      })
      .catch((err) => {
        if (active) {
          console.error("[OrchestrationPanel] Failed to fetch status:", err);
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, []);

  const handleToggle = async () => {
    if (!status) return;
    try {
      setLoading(true);
      const newEnabled = !status.enabled;
      const result = await invoke<boolean>("orchestration_toggle", {
        enable: newEnabled,
      });
      setStatus({ enabled: result });
    } catch (err) {
      console.error("[OrchestrationPanel] Toggle failed:", err);
      toast.error(
        t("orchestration.reloadFailed", {
          defaultValue: "Toggle failed",
          error: String(err),
        }),
      );
    } finally {
      setLoading(false);
    }
  };

  const handleReload = async () => {
    try {
      setLoading(true);
      await invoke<void>("orchestration_reload");
      await fetchStatus();
      toast.success(
        t("orchestration.reloadSuccess", {
          defaultValue: "Config reloaded",
        }),
      );
    } catch (err) {
      console.error("[OrchestrationPanel] Reload failed:", err);
      toast.error(
        t("orchestration.reloadFailed", {
          defaultValue: "Reload failed",
          error: String(err),
        }),
      );
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card className="border-border/50">
      <CardHeader className="flex flex-row items-center justify-between pb-2">
        <div className="flex items-center gap-2">
          <Activity className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-sm font-medium">
            {t("orchestration.title", { defaultValue: "Model Orchestration" })}
          </CardTitle>
        </div>
        <div className="flex items-center gap-2">
          <Badge
            variant={status?.enabled ? "default" : "secondary"}
            className={cn(
              "text-xs",
              status?.enabled && "bg-emerald-500/15 text-emerald-600",
            )}
          >
            {status?.enabled
              ? t("orchestration.status.active", { defaultValue: "Active" })
              : t("orchestration.status.inactive", {
                  defaultValue: "Inactive",
                })}
          </Badge>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={handleReload}
            disabled={loading}
            aria-label={t("orchestration.reload", {
              defaultValue: "Reload Config",
            })}
          >
            <RefreshCw
              className={cn("h-3.5 w-3.5", loading && "animate-spin")}
            />
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex items-center justify-between">
          <span className="text-sm text-muted-foreground">
            {t("orchestration.enable", {
              defaultValue: "Enable multi-model orchestration",
            })}
          </span>
          <Switch
            checked={status?.enabled ?? false}
            onCheckedChange={handleToggle}
            disabled={loading}
          />
        </div>
      </CardContent>
    </Card>
  );
}
