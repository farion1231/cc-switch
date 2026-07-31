import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Layers3, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ModelInputWithFetch } from "./shared/ModelInputWithFetch";
import {
  hasClaudeOneMMarker,
  setClaudeOneMMarker,
  stripClaudeOneMMarker,
} from "./hooks/useModelState";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { AggregateRoutes, Provider } from "@/types";
import {
  AGGREGATE_ROUTE_TIERS,
  CODEX_OFFICIAL_MODEL_SUGGESTIONS,
  codexConfiguredModelsOf,
  configuredModelsOf,
  customRoutesToRows,
  getAggregateRouteConnection,
  getAggregateRouteTargets,
  getCodexAggregateRouteConnection,
  rowsToCustomRoutes,
  type AggregateCustomRouteRow,
  type AggregateRouteConnection,
  type AggregateRouteTier,
} from "@/utils/aggregateRoutes";

const EMPTY_PROVIDER = "__none__";

interface AggregateProviderFieldsProps {
  appId: "claude" | "codex";
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  routes: AggregateRoutes;
  onRoutesChange: (routes: AggregateRoutes) => void;
  providers: Provider[];
  currentProviderId?: string;
  // Codex 模式的有序行态（重复 key 只能在列表形态下保留，提交校验依赖它）
  customRows?: AggregateCustomRouteRow[];
  onCustomRowsChange?: (rows: AggregateCustomRouteRow[]) => void;
}

export function AggregateProviderFields({
  appId,
  enabled,
  onEnabledChange,
  routes,
  onRoutesChange,
  providers,
  currentProviderId,
  customRows,
  onCustomRowsChange,
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

  // Codex：行列表 -> custom Record 同步写回 routes；行态本身由父组件持有
  const rows = customRows ?? customRoutesToRows(routes.custom);
  const updateRows = (next: AggregateCustomRouteRow[]) => {
    onCustomRowsChange?.(next);
    onRoutesChange({ ...routes, custom: rowsToCustomRoutes(next) });
  };
  const patchRow = (index: number, patch: Partial<AggregateCustomRouteRow>) => {
    updateRows(
      rows.map((row, rowIndex) =>
        rowIndex === index ? { ...row, ...patch } : row,
      ),
    );
  };

  const fetchModels = async (
    provider: Provider,
    connection: AggregateRouteConnection,
  ) => {
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
            {appId === "codex"
              ? t("providerForm.aggregate.hintCodex", {
                  defaultValue:
                    "Route requests to different providers by exact request model name. Proxy takeover is required.",
                })
              : t("providerForm.aggregate.hint", {
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

      {enabled && appId === "claude" && (
        <div className="space-y-3 border-t border-border-default pt-4">
          {targets.length === 0 && (
            <p className="text-sm text-destructive">
              {t("providerForm.aggregate.noTargets", {
                defaultValue:
                  "Add at least one regular Claude provider before configuring aggregate routes.",
              })}
            </p>
          )}

          <div className="hidden grid-cols-[100px_minmax(0,1fr)_minmax(0,1fr)_104px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
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
            <span>
              {t("providerForm.modelOneMHeader", {
                defaultValue: "Declare 1M",
              })}
            </span>
          </div>

          {AGGREGATE_ROUTE_TIERS.map((tier) => {
            const route = routes[tier];
            const routeModel = route?.model ?? "";
            const routeModelBase = stripClaudeOneMMarker(routeModel);
            const routeUsesOneM = hasClaudeOneMMarker(routeModel);
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
                className="grid grid-cols-1 gap-2 md:grid-cols-[100px_minmax(0,1fr)_minmax(0,1fr)_104px] md:items-center"
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
                  value={routeModelBase}
                  onChange={(model) =>
                    updateRoute(tier, {
                      model: setClaudeOneMMarker(model, routeUsesOneM),
                    })
                  }
                  placeholder={t("providerForm.aggregate.modelPlaceholder", {
                    defaultValue: "e.g. kimi-k3",
                  })}
                  fetchedModels={models}
                  isLoading={loadingProviderId === target?.id}
                  onFetch={
                    target
                      ? () =>
                          void fetchModels(
                            target,
                            getAggregateRouteConnection(target),
                          )
                      : undefined
                  }
                />
                <label className="flex h-9 items-center gap-2 text-sm text-muted-foreground">
                  <Checkbox
                    aria-label={t("providerForm.modelOneMLabel", {
                      defaultValue: "1M",
                    })}
                    checked={routeUsesOneM}
                    onCheckedChange={(checked) => {
                      const base = routeModelBase.trim();
                      if (!base) return;
                      updateRoute(tier, {
                        model: setClaudeOneMMarker(base, checked === true),
                      });
                    }}
                  />
                  {t("providerForm.modelOneMLabel", {
                    defaultValue: "1M",
                  })}
                </label>
              </div>
            );
          })}
        </div>
      )}

      {enabled && appId === "codex" && (
        <div className="space-y-3 border-t border-border-default pt-4">
          {targets.length === 0 && (
            <p className="text-sm text-destructive">
              {t("providerForm.aggregate.noTargets", {
                defaultValue:
                  "Add at least one regular provider before configuring aggregate routes.",
              })}
            </p>
          )}

          <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
            <span>
              {t("providerForm.aggregate.requestModel", {
                defaultValue: "Request model",
              })}
            </span>
            <span>
              {t("providerForm.aggregate.targetProvider", {
                defaultValue: "Target provider",
              })}
            </span>
            <span>
              {t("providerForm.aggregate.targetModel", {
                defaultValue: "Upstream model",
              })}
            </span>
            <span />
          </div>

          {rows.map((row, index) => {
            const target = targets.find((item) => item.id === row.providerId);
            const configuredModels = target
              ? codexConfiguredModelsOf(target).map((id) => ({
                  id,
                  ownedBy: target.name,
                }))
              : [];
            const models = target
              ? [
                  ...configuredModels,
                  ...(fetchedModels[target.id] ?? []),
                ].filter(
                  (model, modelIndex, all) =>
                    all.findIndex((candidate) => candidate.id === model.id) ===
                    modelIndex,
                )
              : [];

            return (
              <div
                key={index}
                className="grid grid-cols-1 gap-2 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_36px] md:items-center"
              >
                <ModelInputWithFetch
                  id={`aggregate-custom-${index}-key`}
                  value={row.key}
                  onChange={(key) => patchRow(index, { key })}
                  placeholder={t(
                    "providerForm.aggregate.requestModelPlaceholder",
                    { defaultValue: "e.g. gpt-5.5" },
                  )}
                  fetchedModels={CODEX_OFFICIAL_MODEL_SUGGESTIONS}
                  isLoading={false}
                />
                <Select
                  value={row.providerId || EMPTY_PROVIDER}
                  onValueChange={(providerId) =>
                    patchRow(index, {
                      providerId:
                        providerId === EMPTY_PROVIDER ? "" : providerId,
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
                  id={`aggregate-custom-${index}-model`}
                  value={row.model}
                  onChange={(model) => patchRow(index, { model })}
                  placeholder={t(
                    "providerForm.aggregate.upstreamModelPlaceholder",
                    { defaultValue: "e.g. kimi-k2" },
                  )}
                  fetchedModels={models}
                  isLoading={loadingProviderId === target?.id}
                  onFetch={
                    target
                      ? () =>
                          void fetchModels(
                            target,
                            getCodexAggregateRouteConnection(target),
                          )
                      : undefined
                  }
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={t("common.delete", { defaultValue: "Delete" })}
                  onClick={() =>
                    updateRows(rows.filter((_, rowIndex) => rowIndex !== index))
                  }
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            );
          })}

          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              updateRows([...rows, { key: "", providerId: "", model: "" }])
            }
          >
            <Plus className="mr-1 h-4 w-4" />
            {t("providerForm.aggregate.addRoute", {
              defaultValue: "Add route",
            })}
          </Button>
        </div>
      )}
    </div>
  );
}
