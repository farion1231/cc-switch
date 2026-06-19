/**
 * Per-model provider routing manager (Claude Code).
 *
 * Lets the user map each Claude model family (Opus / Sonnet / Haiku) to a
 * specific provider. When the local proxy receives a request, it inspects the
 * requested model and forwards it to the mapped provider. Model families
 * without a mapping fall back to the app's normal current/failover provider.
 */

import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Info, Loader2 } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AppId, ModelClass } from "@/lib/api";
import { useProvidersQuery } from "@/lib/query/queries";
import { useModelRoutes, useSetModelRoute } from "@/lib/query/modelRoutes";
import { extractErrorMessage } from "@/utils/errorUtils";

// Sentinel value for the "use default provider" option (Radix Select
// disallows empty-string values).
const DEFAULT_VALUE = "__default__";

const MODEL_CLASSES: ModelClass[] = ["opus", "sonnet", "haiku"];

interface ModelRoutingManagerProps {
  appType: AppId;
  disabled?: boolean;
}

export function ModelRoutingManager({
  appType,
  disabled = false,
}: ModelRoutingManagerProps) {
  const { t } = useTranslation();

  const { data: providersData, isLoading: isProvidersLoading } =
    useProvidersQuery(appType);
  const { data: routes, isLoading: isRoutesLoading } = useModelRoutes(appType);
  const setModelRoute = useSetModelRoute();

  const providers = providersData ? Object.values(providersData.providers) : [];
  const hasProviders = providers.length > 0;

  const handleChange = async (modelClass: ModelClass, value: string) => {
    const providerId = value === DEFAULT_VALUE ? null : value;
    try {
      await setModelRoute.mutateAsync({ appType, modelClass, providerId });
      toast.success(
        t("proxy.modelRouting.saved", {
          defaultValue: "Model routing updated",
        }),
        { closeButton: true },
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("common.unknown", { defaultValue: "Unknown error" });
      toast.error(
        t("proxy.modelRouting.saveFailed", {
          detail,
          defaultValue: `Failed to update model routing: ${detail}`,
        }),
      );
    }
  };

  if (isProvidersLoading || isRoutesLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <Alert className="border-blue-500/40 bg-blue-500/10">
        <Info className="h-4 w-4" />
        <AlertDescription className="text-sm">
          {t("proxy.modelRouting.info", {
            defaultValue:
              "Send each Claude model family to a specific provider. The local proxy routes requests by the requested model. Families left on \u201cDefault\u201d use the current/failover provider.",
          })}
        </AlertDescription>
      </Alert>

      {!hasProviders ? (
        <div className="rounded-lg border border-dashed border-muted-foreground/40 p-8 text-center">
          <p className="text-sm text-muted-foreground">
            {t("proxy.modelRouting.noProviders", {
              defaultValue:
                "No providers configured. Add providers on the home screen first.",
            })}
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {MODEL_CLASSES.map((modelClass) => {
            const selected = routes?.[modelClass] ?? DEFAULT_VALUE;
            // If a mapped provider was deleted, fall back to default in the UI.
            const selectedExists =
              selected === DEFAULT_VALUE ||
              providers.some((p) => p.id === selected);
            const value = selectedExists ? selected : DEFAULT_VALUE;

            return (
              <div
                key={modelClass}
                className="flex items-center gap-3 rounded-lg border bg-card p-3"
              >
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">
                    {t(`proxy.modelRouting.class.${modelClass}.label`, {
                      defaultValue:
                        modelClass.charAt(0).toUpperCase() +
                        modelClass.slice(1),
                    })}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t(`proxy.modelRouting.class.${modelClass}.hint`, {
                      defaultValue: "",
                    })}
                  </p>
                </div>
                <Select
                  value={value}
                  onValueChange={(v) => handleChange(modelClass, v)}
                  disabled={disabled || setModelRoute.isPending}
                >
                  <SelectTrigger className="w-[220px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DEFAULT_VALUE}>
                      {t("proxy.modelRouting.useDefault", {
                        defaultValue: "Default (current / failover)",
                      })}
                    </SelectItem>
                    {providers.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
