import { forwardRef, useImperativeHandle, useMemo, useState } from "react";
import {
  Activity,
  ChevronDown,
  CircleAlert,
  Edit,
  Loader2,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  useCursorEndpoints,
  useCursorProviders,
  useCursorRuntimeState,
  useDeleteCursorEndpoint,
  useDeleteCursorProvider,
  useSaveCursorProvider,
  useSaveCursorProviders,
  useTestCursorModel,
  useToggleCursorProvider,
} from "@/lib/query/cursor";
import { groupCursorProvidersByEndpoint } from "@/lib/api/cursor";
import type {
  CursorEndpoint,
  CursorModelTestResult,
  CursorProvider,
  CursorProviderChanges,
} from "@/lib/api/cursor";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { resolveProviderIcon } from "@/utils/providerIcon";
import type { ProviderCatalogHandle } from "@/components/providers/ProviderCatalogHandle";
import { CursorEndpointDialog } from "./CursorEndpointDialog";
import { CursorModelDialog } from "./CursorModelDialog";

const resolveCursorEndpointIcon = (providers: CursorProvider[]) => {
  for (const provider of providers) {
    const icon = resolveProviderIcon(
      "cursor",
      provider.icon,
      provider.iconColor,
    );
    if (icon) return { icon, color: provider.iconColor };
  }

  return { icon: undefined, color: undefined };
};

export const CursorModelPanel = forwardRef<ProviderCatalogHandle>(
  function CursorModelPanel(_props, ref) {
    const { t } = useTranslation();
    const endpointsQuery = useCursorEndpoints();
    const providersQuery = useCursorProviders();
    const runtimeQuery = useCursorRuntimeState();
    const saveProvider = useSaveCursorProvider();
    const saveProviders = useSaveCursorProviders();
    const deleteEndpoint = useDeleteCursorEndpoint();
    const deleteProvider = useDeleteCursorProvider();
    const toggleProvider = useToggleCursorProvider();
    const testModel = useTestCursorModel();

    const [editing, setEditing] = useState<CursorProvider | null>(null);
    const [modelDialogOpen, setModelDialogOpen] = useState(false);
    const [endpointDialogOpen, setEndpointDialogOpen] = useState(false);
    const [editingEndpoint, setEditingEndpoint] =
      useState<CursorEndpoint | null>(null);
    const [editingEndpointProviders, setEditingEndpointProviders] = useState<
      CursorProvider[]
    >([]);
    const [deleteTarget, setDeleteTarget] = useState<CursorProvider | null>(
      null,
    );
    const [deleteEndpointTarget, setDeleteEndpointTarget] =
      useState<CursorEndpoint | null>(null);
    const [tests, setTests] = useState<Record<string, CursorModelTestResult>>(
      {},
    );
    const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(
      () => new Set(),
    );

    useImperativeHandle(ref, () => ({
      openCreate: () => {
        setEditingEndpoint(null);
        setEditingEndpointProviders([]);
        setEndpointDialogOpen(true);
      },
    }));

    const providers = useMemo(
      () =>
        Object.values(providersQuery.data ?? {}).sort((left, right) =>
          left.name.localeCompare(right.name),
        ),
      [providersQuery.data],
    );
    const endpointGroups = useMemo(
      () =>
        groupCursorProvidersByEndpoint(endpointsQuery.data ?? [], providers),
      [endpointsQuery.data, providers],
    );
    const state = runtimeQuery.data;
    const busy = ["starting", "restoring", "testing", "maintenance"].includes(
      state?.phase ?? "",
    );

    const reportError = (title: string, error: unknown) => {
      toast.error(title, {
        description: extractErrorMessage(error) || undefined,
      });
    };

    const handleSave = async (provider: CursorProvider) => {
      try {
        await saveProvider.mutateAsync(provider);
        toast.success(t("cursor.panel.toast.modelSaved"));
      } catch (error) {
        reportError(t("cursor.panel.error.modelSaveFailed"), error);
        throw error;
      }
    };

    const handleSaveEndpoint = async (changes: CursorProviderChanges) => {
      try {
        await saveProviders.mutateAsync(changes);
        toast.success(
          t(
            editingEndpoint
              ? "cursor.panel.toast.endpointUpdated"
              : "cursor.panel.toast.endpointAdded",
            { count: changes.upserts.length },
          ),
        );
      } catch (error) {
        reportError(t("cursor.panel.error.endpointSaveFailed"), error);
        throw error;
      }
    };

    const runTest = async (provider: CursorProvider) => {
      try {
        const result = await testModel.mutateAsync(provider.id);
        setTests((current) => ({ ...current, [provider.id]: result }));
        if (result.status === "success") {
          toast.success(
            t("cursor.panel.toast.testSucceeded", { name: provider.name }),
          );
        } else {
          toast.error(
            t("cursor.panel.toast.testFailed", { name: provider.name }),
            { description: result.error },
          );
        }
      } catch (error) {
        reportError(
          t("cursor.panel.toast.testFailed", { name: provider.name }),
          error,
        );
      }
    };

    const renderModelRow = (provider: CursorProvider) => (
      <ModelRow
        key={provider.id}
        provider={provider}
        test={tests[provider.id]}
        testing={testModel.isPending && testModel.variables === provider.id}
        disabled={busy}
        onToggle={async (enabled) => {
          try {
            await toggleProvider.mutateAsync({ id: provider.id, enabled });
          } catch (error) {
            reportError(t("cursor.panel.error.toggleFailed"), error);
          }
        }}
        onTest={() => void runTest(provider)}
        onEdit={() => {
          setEditing(provider);
          setModelDialogOpen(true);
        }}
        onDelete={() => setDeleteTarget(provider)}
      />
    );

    if (
      endpointsQuery.isLoading ||
      providersQuery.isLoading ||
      runtimeQuery.isLoading
    ) {
      return (
        <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
          <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
          {t("cursor.panel.loadingRuntime")}
        </div>
      );
    }

    return (
      <div className="h-full overflow-y-auto px-6 pb-12">
        <div className="mx-auto max-w-6xl space-y-5">
          {state?.lastError && (
            <Alert variant="destructive">
              <CircleAlert className="h-4 w-4" />
              <AlertTitle>{t("cursor.panel.runtimeError")}</AlertTitle>
              <AlertDescription>{state.lastError}</AlertDescription>
            </Alert>
          )}

          {endpointGroups.length === 0 ? (
            <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border p-10 text-center">
              <div className="mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-muted">
                <Activity className="h-7 w-7 text-muted-foreground" />
              </div>
              <h3 className="text-lg font-semibold">
                {t("cursor.panel.empty.title")}
              </h3>
              <p className="mt-2 max-w-lg text-sm text-muted-foreground">
                {t("cursor.panel.empty.description")}
              </p>
            </div>
          ) : (
            <div className="space-y-3">
              {endpointGroups.map(
                ({ endpoint, providers: endpointProviders }) => {
                  const expanded = !collapsedGroups.has(endpoint.id);
                  const enabledCount = endpointProviders.filter(
                    ({ settingsConfig }) => settingsConfig.enabled,
                  ).length;
                  const endpointIcon =
                    resolveCursorEndpointIcon(endpointProviders);

                  return (
                    <Collapsible
                      key={endpoint.id}
                      open={expanded}
                      onOpenChange={(open) =>
                        setCollapsedGroups((current) => {
                          const next = new Set(current);
                          if (open) next.delete(endpoint.id);
                          else next.add(endpoint.id);
                          return next;
                        })
                      }
                      className="group/endpoint relative overflow-hidden rounded-xl border border-border bg-card text-card-foreground transition-all duration-300 hover:border-border-active hover:shadow-sm focus-within:border-border-active focus-within:shadow-sm"
                    >
                      <div className="relative flex items-center gap-3 p-4">
                        <CollapsibleTrigger asChild>
                          <button
                            type="button"
                            className="flex min-w-0 flex-1 items-center gap-3 rounded-lg text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                            aria-label={t(
                              "cursor.panel.endpoint.toggleAriaLabel",
                              {
                                action: expanded
                                  ? t("cursor.panel.endpoint.collapse")
                                  : t("cursor.panel.endpoint.expand"),
                                name: endpoint.name,
                              },
                            )}
                          >
                            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-border bg-muted transition-transform duration-300 group-hover/endpoint:scale-105">
                              <ProviderIcon
                                icon={endpointIcon.icon}
                                color={endpointIcon.color}
                                name={endpoint.name}
                                size={20}
                              />
                            </div>
                            <div className="min-w-0 flex-1 space-y-1">
                              <div className="flex flex-wrap items-center gap-2">
                                <h3 className="truncate text-base font-semibold leading-none">
                                  {endpoint.name}
                                </h3>
                                <Badge
                                  variant="secondary"
                                  className="rounded-md px-1.5 py-0.5 text-[10px]"
                                >
                                  {t("cursor.panel.endpoint.enabledCount", {
                                    enabled: enabledCount,
                                    total: endpointProviders.length,
                                  })}
                                </Badge>
                              </div>
                              <p
                                className="truncate text-sm text-blue-500 dark:text-blue-400"
                                title={endpoint.baseURL}
                              >
                                {endpoint.baseURL}
                              </p>
                            </div>
                            <ChevronDown
                              className={cn(
                                "h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                                expanded && "rotate-180",
                              )}
                            />
                          </button>
                        </CollapsibleTrigger>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 shrink-0 p-1 opacity-0 pointer-events-none transition-opacity duration-200 group-hover/endpoint:opacity-100 group-hover/endpoint:pointer-events-auto group-focus-within/endpoint:opacity-100 group-focus-within/endpoint:pointer-events-auto"
                          title={t("cursor.panel.endpoint.editAriaLabel", {
                            name: endpoint.name,
                          })}
                          aria-label={t("cursor.panel.endpoint.editAriaLabel", {
                            name: endpoint.name,
                          })}
                          onClick={() => {
                            setEditingEndpoint(endpoint);
                            setEditingEndpointProviders(endpointProviders);
                            setEndpointDialogOpen(true);
                          }}
                        >
                          <Edit className="h-4 w-4" />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 shrink-0 p-1 opacity-0 pointer-events-none transition-opacity duration-200 group-hover/endpoint:opacity-100 group-hover/endpoint:pointer-events-auto group-focus-within/endpoint:opacity-100 group-focus-within/endpoint:pointer-events-auto"
                          title={t("cursor.panel.endpoint.deleteAriaLabel", {
                            name: endpoint.name,
                          })}
                          aria-label={t(
                            "cursor.panel.endpoint.deleteAriaLabel",
                            {
                              name: endpoint.name,
                            },
                          )}
                          disabled={busy}
                          onClick={() => setDeleteEndpointTarget(endpoint)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                      <CollapsibleContent>
                        <div className="space-y-2 border-t border-border-default bg-muted/[0.08] p-3">
                          {endpointProviders.length === 0 ? (
                            <div className="rounded-lg border border-dashed border-border px-5 py-6 text-center text-sm text-muted-foreground">
                              {t("cursor.panel.endpoint.noModels")}
                            </div>
                          ) : (
                            endpointProviders.map(renderModelRow)
                          )}
                        </div>
                      </CollapsibleContent>
                    </Collapsible>
                  );
                },
              )}
            </div>
          )}
        </div>

        <CursorEndpointDialog
          open={endpointDialogOpen}
          endpoint={editingEndpoint}
          providers={editingEndpointProviders}
          onOpenChange={setEndpointDialogOpen}
          onSave={handleSaveEndpoint}
        />
        <CursorModelDialog
          open={modelDialogOpen}
          provider={editing}
          onOpenChange={setModelDialogOpen}
          onSave={handleSave}
        />
        <ConfirmDialog
          isOpen={Boolean(deleteEndpointTarget)}
          title={t("cursor.panel.endpointDelete.title")}
          message={t("cursor.panel.endpointDelete.message", {
            name: deleteEndpointTarget?.name ?? "",
          })}
          onConfirm={() => {
            if (!deleteEndpointTarget) return;
            void deleteEndpoint
              .mutateAsync(deleteEndpointTarget.id)
              .then(() =>
                toast.success(t("cursor.panel.toast.endpointDeleted")),
              )
              .catch((error) =>
                reportError(
                  t("cursor.panel.error.endpointDeleteFailed"),
                  error,
                ),
              )
              .finally(() => setDeleteEndpointTarget(null));
          }}
          onCancel={() => setDeleteEndpointTarget(null)}
        />
        <ConfirmDialog
          isOpen={Boolean(deleteTarget)}
          title={t("cursor.panel.delete.title")}
          message={t("cursor.panel.delete.message", {
            name: deleteTarget?.name ?? "",
          })}
          onConfirm={() => {
            if (!deleteTarget) return;
            void deleteProvider
              .mutateAsync(deleteTarget.id)
              .then(() => toast.success(t("cursor.panel.toast.modelDeleted")))
              .catch((error) =>
                reportError(t("cursor.panel.error.modelDeleteFailed"), error),
              )
              .finally(() => setDeleteTarget(null));
          }}
          onCancel={() => setDeleteTarget(null)}
        />
      </div>
    );
  },
);

function ModelRow({
  provider,
  test,
  testing,
  disabled,
  onToggle,
  onTest,
  onEdit,
  onDelete,
}: {
  provider: CursorProvider;
  test?: CursorModelTestResult;
  testing: boolean;
  disabled: boolean;
  onToggle: (enabled: boolean) => void;
  onTest: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const config = provider.settingsConfig;
  return (
    <div
      className={cn(
        "group/model relative flex flex-wrap items-center gap-4 overflow-hidden rounded-xl border border-border bg-card p-4 text-card-foreground transition-all duration-300",
        "hover:border-border-active hover:shadow-sm focus-within:border-border-active focus-within:shadow-sm",
        !config.enabled && "opacity-70",
      )}
    >
      <div className="min-w-[220px] flex-1 space-y-1">
        <div className="flex min-h-7 flex-wrap items-center gap-2">
          <h4 className="text-base font-semibold leading-none">
            {provider.name}
          </h4>
          {!config.enabled && (
            <Badge
              variant="secondary"
              className="rounded-md px-1.5 py-0.5 text-[10px]"
            >
              {t("cursor.panel.model.disabled")}
            </Badge>
          )}
        </div>
        <p
          className="truncate text-sm text-muted-foreground"
          title={config.modelID}
        >
          {config.modelID}
        </p>
        {config.pricingModel && config.pricingModel !== config.modelID && (
          <p className="text-xs text-muted-foreground">
            {t("cursor.panel.model.pricingModel", {
              model: config.pricingModel,
            })}
          </p>
        )}
      </div>
      {test && (
        <div className="min-w-[130px] text-right text-xs">
          {test.status === "success" ? (
            <>
              <div className="font-medium text-emerald-600 dark:text-emerald-400">
                {t("cursor.panel.model.tokensPerSecond", {
                  value: test.tokensPerSecond.toFixed(1),
                })}
              </div>
              <div className="text-muted-foreground">
                {t("cursor.panel.model.firstToken", {
                  milliseconds: test.firstTextTokenMs,
                })}
              </div>
            </>
          ) : (
            <div className="max-w-44 truncate text-red-500" title={test.error}>
              {test.error || t("cursor.panel.model.testFailed")}
            </div>
          )}
        </div>
      )}
      <div className="ml-auto flex shrink-0 items-center gap-2">
        <Switch
          checked={config.enabled}
          disabled={disabled}
          onCheckedChange={onToggle}
          aria-label={t("cursor.panel.model.enableAriaLabel", {
            name: provider.name,
          })}
        />
        <div className="flex items-center gap-1 opacity-0 pointer-events-none transition-opacity duration-200 group-hover/model:opacity-100 group-hover/model:pointer-events-auto group-focus-within/model:opacity-100 group-focus-within/model:pointer-events-auto">
          <Button
            variant="ghost"
            size="icon"
            onClick={onEdit}
            title={t("common.edit")}
            aria-label={t("cursor.panel.model.editAriaLabel", {
              name: provider.name,
            })}
            className="h-8 w-8 p-1"
          >
            <Edit className="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={onTest}
            disabled={testing || disabled}
            title={t("cursor.panel.model.testAction")}
            aria-label={t("cursor.panel.model.testAriaLabel", {
              name: provider.name,
            })}
            className="h-8 w-8 p-1"
          >
            {testing ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Activity className="h-4 w-4" />
            )}
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={onDelete}
            disabled={disabled}
            title={t("common.delete")}
            aria-label={t("cursor.panel.model.deleteAriaLabel", {
              name: provider.name,
            })}
            className="h-8 w-8 p-1 hover:text-red-500 dark:hover:text-red-400"
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  );
}
