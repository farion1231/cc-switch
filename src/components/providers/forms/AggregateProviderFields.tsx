import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Layers3 } from "lucide-react";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ModelInputWithFetch } from "./shared/ModelInputWithFetch";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { AggregateRoutes, Provider } from "@/types";
import {
  AGGREGATE_ROUTE_TIERS,
  configuredModelsOf,
  getAggregateRouteConnection,
  getAggregateRouteTargets,
  type AggregateRouteTier,
} from "@/utils/aggregateRoutes";

const EMPTY_PROVIDER = "__none__";

interface AggregateProviderFieldsProps {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  routes: AggregateRoutes;
  onRoutesChange: (routes: AggregateRoutes) => void;
  providers: Provider[];
  currentProviderId?: string;
}

export function AggregateProviderFields({
  enabled,
  onEnabledChange,
  routes,
  onRoutesChange,
  providers,
  currentProviderId,
}: AggregateProviderFieldsProps) {
  const { t } = useTranslation();
  const targets = useMemo(
    () => getAggregateRouteTargets(providers, currentProviderId),
    [providers, currentProviderId],
  );
  const [fetchedModels, setFetchedModels] = useState<
    Record<string, FetchedModel[]>
  >({});
  const [loadingProviderId, setLoadingProviderId] = useState<string | null>(
    null,
  );

  const updateRoute = (
    tier: AggregateRouteTier,
    patch: Partial<{ providerId: string; model: string }>,
  ) => {
    const previous = routes[tier] ?? { providerId: "", model: "" };
    onRoutesChange({
      ...routes,
      [tier]: { ...previous, ...patch },
    });
  };

  const fetchModels = async (provider: Provider) => {
    const connection = getAggregateRouteConnection(provider);
    if (!connection.baseUrl || !connection.apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(connection.apiKey),
        hasBaseUrl: Boolean(connection.baseUrl),
      });
      return;
    }
    setLoadingProviderId(provider.id);
    try {
      const models = await fetchModelsForConfig(
        connection.baseUrl,
        connection.apiKey,
        connection.isFullUrl,
        connection.modelsUrl,
        connection.customUserAgent,
      );
      setFetchedModels((previous) => ({
        ...previous,
        [provider.id]: models,
      }));
      if (models.length === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else {
        toast.success(
          t("providerForm.fetchModelsSuccess", { count: models.length }),
        );
      }
    } catch (error) {
      showFetchModelsError(error, t);
    } finally {
      setLoadingProviderId(null);
    }
  };

  return (
    <div className="space-y-4 rounded-lg border border-border-default bg-muted/20 p-4">
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-1">
          <Label className="flex items-center gap-2 text-sm font-medium">
            <Layers3 className="h-4 w-4" />
            {t("providerForm.aggregate.title", {
              defaultValue: "Aggregate provider",
            })}
          </Label>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t("providerForm.aggregate.hint", {
              defaultValue:
                "Route Haiku, Sonnet, Opus, and Fable requests to models from different providers. Proxy takeover is required.",
            })}
          </p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={onEnabledChange}
          aria-label={t("providerForm.aggregate.title", {
            defaultValue: "Aggregate provider",
          })}
        />
      </div>

      {enabled && (
        <div className="space-y-3 border-t border-border-default pt-4">
          {targets.length === 0 && (
            <p className="text-sm text-destructive">
              {t("providerForm.aggregate.noTargets", {
                defaultValue:
                  "Add at least one regular Claude provider before configuring aggregate routes.",
              })}
            </p>
          )}

          <div className="hidden grid-cols-[100px_minmax(0,1fr)_minmax(0,1fr)] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
            <span>
              {t("providerForm.aggregate.tier", { defaultValue: "Tier" })}
            </span>
            <span>
              {t("providerForm.aggregate.targetProvider", {
                defaultValue: "Target provider",
              })}
            </span>
            <span>
              {t("providerForm.aggregate.targetModel", {
                defaultValue: "Target model",
              })}
            </span>
          </div>

          {AGGREGATE_ROUTE_TIERS.map((tier) => {
            const route = routes[tier];
            const target = targets.find(
              (item) => item.id === route?.providerId,
            );
            const configuredModels = target
              ? configuredModelsOf(target).map((id) => ({
                  id,
                  ownedBy: target.name,
                }))
              : [];
            const models = target
              ? [
                  ...configuredModels,
                  ...(fetchedModels[target.id] ?? []),
                ].filter(
                  (model, index, all) =>
                    all.findIndex((candidate) => candidate.id === model.id) ===
                    index,
                )
              : [];

            return (
              <div
                key={tier}
                className="grid grid-cols-1 gap-2 md:grid-cols-[100px_minmax(0,1fr)_minmax(0,1fr)] md:items-center"
              >
                <Label
                  htmlFor={`aggregate-${tier}-model`}
                  className="capitalize"
                >
                  {t(`providerForm.aggregate.tiers.${tier}`, {
                    defaultValue: tier,
                  })}
                </Label>
                <Select
                  value={route?.providerId || EMPTY_PROVIDER}
                  onValueChange={(providerId) =>
                    updateRoute(tier, {
                      providerId:
                        providerId === EMPTY_PROVIDER ? "" : providerId,
                      model: "",
                    })
                  }
                >
                  <SelectTrigger>
                    <SelectValue
                      placeholder={t("providerForm.aggregate.selectProvider", {
                        defaultValue: "Select provider",
                      })}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={EMPTY_PROVIDER}>
                      {t("providerForm.aggregate.notConfigured", {
                        defaultValue: "Not configured",
                      })}
                    </SelectItem>
                    {targets.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <ModelInputWithFetch
                  id={`aggregate-${tier}-model`}
                  value={route?.model ?? ""}
                  onChange={(model) => updateRoute(tier, { model })}
                  placeholder={t("providerForm.aggregate.modelPlaceholder", {
                    defaultValue: "e.g. kimi-k3",
                  })}
                  fetchedModels={models}
                  isLoading={loadingProviderId === target?.id}
                  onFetch={target ? () => void fetchModels(target) : undefined}
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
