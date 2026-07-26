import { forwardRef, useImperativeHandle, useMemo, useState } from "react";
import {
  Activity,
  ChevronDown,
  CircleAlert,
  Gauge,
  Pencil,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  useCursorProviders,
  useCursorRuntimeState,
  useDeleteCursorProvider,
  useSaveCursorProvider,
  useSaveCursorProviders,
  useTestCursorModel,
  useToggleCursorProvider,
} from "@/lib/query/cursor";
import type { CursorModelTestResult, CursorProvider } from "@/lib/api/cursor";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import {
  formatTokenCount,
  resolveCursorEndpointGroup,
} from "@/lib/cursorModelMetadata";
import { CursorEndpointDialog } from "./CursorEndpointDialog";
import { CursorModelDialog } from "./CursorModelDialog";

const getEndpointGroup = (provider: CursorProvider) =>
  resolveCursorEndpointGroup(
    provider.settingsConfig.baseURL,
    provider.settingsConfig.providerGroup,
    provider.settingsConfig.type,
  );

export interface CursorModelPanelHandle {
  openAddModel: () => void;
}

export const CursorModelPanel = forwardRef<CursorModelPanelHandle>(
  function CursorModelPanel(_props, ref) {
    const providersQuery = useCursorProviders();
    const runtimeQuery = useCursorRuntimeState();
    const saveProvider = useSaveCursorProvider();
    const saveProviders = useSaveCursorProviders();
    const deleteProvider = useDeleteCursorProvider();
    const toggleProvider = useToggleCursorProvider();
    const testModel = useTestCursorModel();

    const [editing, setEditing] = useState<CursorProvider | null>(null);
    const [modelDialogOpen, setModelDialogOpen] = useState(false);
    const [endpointDialogOpen, setEndpointDialogOpen] = useState(false);
    const [editingEndpointProviders, setEditingEndpointProviders] = useState<
      CursorProvider[]
    >([]);
    const [deleteTarget, setDeleteTarget] = useState<CursorProvider | null>(
      null,
    );
    const [tests, setTests] = useState<Record<string, CursorModelTestResult>>(
      {},
    );
    const [expandedGroups, setExpandedGroups] = useState<Set<string>>(
      () => new Set(),
    );

    useImperativeHandle(ref, () => ({
      openAddModel: () => {
        setEditingEndpointProviders([]);
        setEndpointDialogOpen(true);
      },
    }));

    const providers = useMemo(
      () =>
        Object.values(providersQuery.data ?? {}).sort((left, right) => {
          const groupOrder = getEndpointGroup(left).label.localeCompare(
            getEndpointGroup(right).label,
          );
          return groupOrder || left.name.localeCompare(right.name);
        }),
      [providersQuery.data],
    );
    const providerGroups = useMemo(() => {
      const groups = new Map<
        string,
        { label: string; providers: CursorProvider[] }
      >();
      for (const provider of providers) {
        const endpoint = getEndpointGroup(provider);
        const current = groups.get(endpoint.key);
        groups.set(endpoint.key, {
          label: current?.label || endpoint.label,
          providers: [...(current?.providers ?? []), provider],
        });
      }
      return Array.from(groups.entries());
    }, [providers]);
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
        toast.success("Cursor 模型已保存");
      } catch (error) {
        reportError("保存 Cursor 模型失败", error);
        throw error;
      }
    };

    const handleSaveEndpoint = async (nextProviders: CursorProvider[]) => {
      try {
        await saveProviders.mutateAsync(nextProviders);
        toast.success(
          `${editingEndpointProviders.length > 0 ? "Endpoint 已更新" : "Endpoint 已添加"} · ${nextProviders.length} 个模型`,
        );
      } catch (error) {
        reportError("保存 Cursor Endpoint 失败", error);
        throw error;
      }
    };

    const runTest = async (provider: CursorProvider) => {
      try {
        const result = await testModel.mutateAsync(provider.id);
        setTests((current) => ({ ...current, [provider.id]: result }));
        if (result.status === "success") {
          toast.success(`${provider.name} 测速完成`);
        } else {
          toast.error(`${provider.name} 测速失败`, {
            description: result.error,
          });
        }
      } catch (error) {
        reportError(`${provider.name} 测速失败`, error);
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
            reportError("更新模型启用状态失败", error);
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

    if (providersQuery.isLoading || runtimeQuery.isLoading) {
      return (
        <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
          <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
          加载 Cursor 运行状态…
        </div>
      );
    }

    return (
      <div className="h-full overflow-y-auto px-6 pb-12">
        <div className="mx-auto max-w-6xl space-y-5">
          {state?.lastError && (
            <Alert variant="destructive">
              <CircleAlert className="h-4 w-4" />
              <AlertTitle>Cursor 运行时异常</AlertTitle>
              <AlertDescription>{state.lastError}</AlertDescription>
            </Alert>
          )}

          <Card className="overflow-hidden border-border-default">
            <CardContent className="p-0">
              {providers.length === 0 ? (
                <div className="flex flex-col items-center justify-center px-6 py-16 text-center">
                  <div className="mb-4 rounded-2xl bg-blue-500/10 p-4 text-blue-500">
                    <Activity className="h-7 w-7" />
                  </div>
                  <h3 className="font-medium">尚未配置 Cursor 模型</h3>
                  <p className="mt-1 max-w-md text-sm text-muted-foreground">
                    添加 OpenAI 或 Anthropic 兼容模型后，即可通过安全的本地
                    sidecar 转发 Cursor 聊天。
                  </p>
                </div>
              ) : (
                <div className="divide-y divide-border-default">
                  {providerGroups.map(([endpointKey, group]) => {
                    const expanded = expandedGroups.has(endpointKey);
                    return (
                      <Collapsible
                        key={endpointKey}
                        open={expanded}
                        onOpenChange={(open) =>
                          setExpandedGroups((current) => {
                            const next = new Set(current);
                            if (open) next.add(endpointKey);
                            else next.delete(endpointKey);
                            return next;
                          })
                        }
                      >
                        <div className="flex items-center px-2">
                          <CollapsibleTrigger asChild>
                            <button
                              type="button"
                              className="flex min-w-0 flex-1 items-center gap-3 px-3 py-4 text-left transition-colors hover:bg-muted/40"
                              aria-label={`${expanded ? "收起" : "展开"} ${group.label} 模型列表`}
                            >
                              <h3 className="min-w-0 flex-1 truncate font-semibold">
                                {group.label}
                              </h3>
                              <Badge variant="secondary" className="shrink-0">
                                {group.providers.length} 个模型
                              </Badge>
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
                            title={`编辑 ${group.label} Endpoint`}
                            aria-label={`编辑 ${group.label} Endpoint`}
                            onClick={() => {
                              setEditingEndpointProviders(group.providers);
                              setEndpointDialogOpen(true);
                            }}
                          >
                            <Pencil className="h-4 w-4" />
                          </Button>
                        </div>
                        <CollapsibleContent>
                          <div className="divide-y divide-border-default border-t border-border-default bg-muted/[0.08]">
                            {group.providers.map(renderModelRow)}
                          </div>
                        </CollapsibleContent>
                      </Collapsible>
                    );
                  })}
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        <CursorEndpointDialog
          open={endpointDialogOpen}
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
          isOpen={Boolean(deleteTarget)}
          title="删除 Cursor 模型"
          message={`确定删除“${deleteTarget?.name ?? ""}”吗？历史使用记录仍会按 Provider ID 和名称快照保留。`}
          onConfirm={() => {
            if (!deleteTarget) return;
            void deleteProvider
              .mutateAsync(deleteTarget.id)
              .then(() => toast.success("Cursor 模型已删除"))
              .catch((error) => reportError("删除 Cursor 模型失败", error))
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
  const config = provider.settingsConfig;
  return (
    <div className="flex flex-wrap items-center gap-4 px-5 py-4">
      <ProviderIcon
        icon={config.type === "anthropic" ? "anthropic" : "openai"}
        name={provider.name}
        size={28}
      />
      <div className="min-w-[220px] flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{provider.name}</span>
          <Badge variant="outline">
            {config.type === "anthropic" ? "Anthropic" : "OpenAI"}
          </Badge>
          {config.contextWindowTokens > 0 && (
            <Badge variant="secondary">
              {formatTokenCount(config.contextWindowTokens)} 上下文
            </Badge>
          )}
          {!config.enabled && <Badge variant="secondary">已停用</Badge>}
        </div>
        <p
          className="mt-1 truncate text-sm text-muted-foreground"
          title={`${config.modelID} · ${config.baseURL}`}
        >
          {config.modelID} · {config.baseURL}
        </p>
        {config.pricingModel && config.pricingModel !== config.modelID && (
          <p className="mt-0.5 text-xs text-muted-foreground">
            计价模型 {config.pricingModel}
          </p>
        )}
      </div>
      {test && (
        <div className="min-w-[130px] text-right text-xs">
          {test.status === "success" ? (
            <>
              <div className="font-medium text-emerald-600 dark:text-emerald-400">
                {test.tokensPerSecond.toFixed(1)} tok/s
              </div>
              <div className="text-muted-foreground">
                首 token {test.firstTextTokenMs} ms
              </div>
            </>
          ) : (
            <div className="max-w-44 truncate text-red-500" title={test.error}>
              {test.error || "测速失败"}
            </div>
          )}
        </div>
      )}
      <div className="flex items-center gap-1">
        <Switch
          checked={config.enabled}
          disabled={disabled}
          onCheckedChange={onToggle}
          aria-label={`启用 ${provider.name}`}
        />
        <Button
          variant="ghost"
          size="icon"
          onClick={onTest}
          disabled={testing || disabled}
          title="测速"
        >
          <Gauge className={cn("h-4 w-4", testing && "animate-pulse")} />
        </Button>
        <Button variant="ghost" size="icon" onClick={onEdit} title="编辑">
          <Pencil className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          onClick={onDelete}
          disabled={disabled}
          title="删除"
          className="hover:text-red-500"
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
