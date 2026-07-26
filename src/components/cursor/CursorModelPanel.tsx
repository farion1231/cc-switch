import { forwardRef, useImperativeHandle, useMemo, useState } from "react";
import {
  Activity,
  CircleAlert,
  Gauge,
  Layers3,
  List,
  Pencil,
  Power,
  RefreshCw,
  ShieldCheck,
  ShieldMinus,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  useCursorProviders,
  useCursorRuntimeState,
  useDeleteCursorProvider,
  useInstallCursorCA,
  useRemoveCursorCA,
  useSaveCursorProvider,
  useTestCursorModel,
  useToggleCursorProvider,
} from "@/lib/query/cursor";
import type {
  CursorModelTestResult,
  CursorProvider,
  CursorRuntimeState,
} from "@/lib/api/cursor";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import {
  formatTokenCount,
  resolveCursorEndpointGroup,
} from "@/lib/cursorModelMetadata";
import { CursorModelDialog } from "./CursorModelDialog";

const PHASE_LABEL: Record<string, string> = {
  stopped: "未运行",
  starting: "启动中",
  running: "运行中",
  restoring: "恢复中",
  testing: "测速中",
  maintenance: "维护中",
  error: "异常",
};

const getEndpointGroup = (provider: CursorProvider) =>
  resolveCursorEndpointGroup(
    provider.settingsConfig.baseURL,
    provider.settingsConfig.providerGroup,
    provider.settingsConfig.type,
  );

type CatalogViewMode = "provider" | "flat";

export interface CursorModelPanelHandle {
  openAddModel: () => void;
}

export const CursorModelPanel = forwardRef<CursorModelPanelHandle>(
  function CursorModelPanel(_props, ref) {
    const providersQuery = useCursorProviders();
    const runtimeQuery = useCursorRuntimeState();
    const saveProvider = useSaveCursorProvider();
    const deleteProvider = useDeleteCursorProvider();
    const toggleProvider = useToggleCursorProvider();
    const installCA = useInstallCursorCA();
    const removeCA = useRemoveCursorCA();
    const testModel = useTestCursorModel();

    const [editing, setEditing] = useState<CursorProvider | null>(null);
    const [modelDialogOpen, setModelDialogOpen] = useState(false);
    const [deleteTarget, setDeleteTarget] = useState<CursorProvider | null>(
      null,
    );
    const [tests, setTests] = useState<Record<string, CursorModelTestResult>>(
      {},
    );
    const [batchTesting, setBatchTesting] = useState(false);
    const [catalogViewMode, setCatalogViewMode] =
      useState<CatalogViewMode>("provider");

    useImperativeHandle(ref, () => ({
      openAddModel: () => {
        setEditing(null);
        setModelDialogOpen(true);
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
        { label: string; baseUrl: string; providers: CursorProvider[] }
      >();
      for (const provider of providers) {
        const endpoint = getEndpointGroup(provider);
        const current = groups.get(endpoint.key);
        groups.set(endpoint.key, {
          label: current?.label || endpoint.label,
          baseUrl: current?.baseUrl || endpoint.baseUrl,
          providers: [...(current?.providers ?? []), provider],
        });
      }
      return Array.from(groups.entries());
    }, [providers]);
    const enabledProviders = providers.filter(
      (provider) => provider.settingsConfig.enabled,
    );
    const state = runtimeQuery.data;
    const busy = ["starting", "restoring", "testing", "maintenance"].includes(
      state?.phase ?? "",
    );
    const running = state?.phase === "running";

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

    const handleRemoveCA = async () => {
      try {
        await removeCA.mutateAsync(undefined);
        toast.success("CC Switch Cursor CA 已移除");
      } catch (error) {
        reportError("移除 Cursor CA 失败", error);
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

    const runAllTests = async () => {
      setBatchTesting(true);
      try {
        for (const provider of enabledProviders) {
          await runTest(provider);
        }
      } finally {
        setBatchTesting(false);
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
          <RuntimeCard
            state={state}
            enabledCount={enabledProviders.length}
            running={running}
            busy={busy}
            onRefresh={() => void runtimeQuery.refetch()}
          />

          {state?.lastError && (
            <Alert variant="destructive">
              <CircleAlert className="h-4 w-4" />
              <AlertTitle>Cursor 运行时异常</AlertTitle>
              <AlertDescription>{state.lastError}</AlertDescription>
            </Alert>
          )}

          <Card className="overflow-hidden border-border-default">
            <CardHeader className="flex-row items-center justify-between space-y-0 border-b border-border-default bg-muted/20 py-4">
              <div>
                <CardTitle className="text-base">Cursor 模型目录</CardTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  所有启用模型会同时投影到 Cursor；实际模型在 Cursor
                  聊天框中选择。
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Select
                  value={catalogViewMode}
                  onValueChange={(value) =>
                    setCatalogViewMode(value as CatalogViewMode)
                  }
                >
                  <SelectTrigger
                    className="w-[168px]"
                    aria-label="模型目录视图"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="provider">按提供商分类</SelectItem>
                    <SelectItem value="flat">平铺模型</SelectItem>
                  </SelectContent>
                </Select>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={enabledProviders.length === 0 || batchTesting}
                  onClick={() => void runAllTests()}
                >
                  <Gauge
                    className={cn(
                      "mr-2 h-4 w-4",
                      batchTesting && "animate-pulse",
                    )}
                  />
                  全部测速
                </Button>
              </div>
            </CardHeader>
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
              ) : catalogViewMode === "provider" ? (
                <div className="divide-y divide-border-default">
                  {providerGroups.map(([endpointKey, group]) => (
                    <section key={endpointKey}>
                      <div className="flex items-center justify-between border-b border-border-default bg-muted/20 px-5 py-2.5">
                        <div className="min-w-0">
                          <div className="flex items-center gap-2">
                            <Layers3 className="h-4 w-4 shrink-0 text-muted-foreground" />
                            <h3 className="truncate text-sm font-medium">
                              {group.label}
                            </h3>
                          </div>
                          <p
                            className="mt-0.5 truncate pl-6 font-mono text-xs text-muted-foreground"
                            title={group.baseUrl}
                          >
                            {group.baseUrl}
                          </p>
                        </div>
                        <Badge variant="secondary" className="ml-3 shrink-0">
                          {group.providers.length} 个模型
                        </Badge>
                      </div>
                      <div className="divide-y divide-border-default">
                        {group.providers.map(renderModelRow)}
                      </div>
                    </section>
                  ))}
                </div>
              ) : (
                <div className="divide-y divide-border-default">
                  <div className="flex items-center gap-2 bg-muted/20 px-5 py-2.5 text-sm text-muted-foreground">
                    <List className="h-4 w-4" />
                    平铺模型
                  </div>
                  {providers.map(renderModelRow)}
                </div>
              )}
            </CardContent>
          </Card>

          <Card className="border-border-default">
            <CardHeader className="py-4">
              <CardTitle className="flex items-center gap-2 text-base">
                <ShieldCheck className="h-4 w-4 text-emerald-500" />
                CA 信任
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-wrap items-center justify-between gap-4 pt-0">
              <div className="min-w-0">
                <div className="flex items-center gap-2 text-sm font-medium">
                  {state?.caInstalled ? "已信任" : "未信任"}
                  <Badge variant={state?.caInstalled ? "secondary" : "outline"}>
                    {platformTrustLabel(state?.platform)}
                  </Badge>
                </div>
                {state?.caFingerprint && (
                  <p
                    className="mt-1 max-w-2xl truncate font-mono text-xs text-muted-foreground"
                    title={state.caFingerprint}
                  >
                    SHA-256 {state.caFingerprint}
                  </p>
                )}
              </div>
              <div className="flex gap-2">
                {!state?.caInstalled && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() =>
                      void installCA
                        .mutateAsync(undefined)
                        .then(() => toast.success("Cursor CA 已安装"))
                        .catch((error) =>
                          reportError("安装 Cursor CA 失败", error),
                        )
                    }
                    disabled={busy}
                  >
                    <ShieldCheck className="mr-2 h-4 w-4" />
                    安装 CA
                  </Button>
                )}
                {state?.caInstalled && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleRemoveCA()}
                    disabled={running || busy}
                  >
                    <ShieldMinus className="mr-2 h-4 w-4" />
                    移除 CA
                  </Button>
                )}
              </div>
            </CardContent>
          </Card>
        </div>

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

function RuntimeCard({
  state,
  enabledCount,
  running,
  busy,
  onRefresh,
}: {
  state?: CursorRuntimeState;
  enabledCount: number;
  running: boolean;
  busy: boolean;
  onRefresh: () => void;
}) {
  const layers = [
    ["Sidecar", state?.sidecarRunning],
    ["Backend", state?.backendRunning],
    ["Proxy", state?.proxyRunning],
    ["Cursor 接管", state?.cursorSettingsApplied],
  ] as const;
  return (
    <Card className="overflow-hidden border-border-default bg-gradient-to-br from-blue-500/[0.07] via-background to-violet-500/[0.06]">
      <CardContent className="p-5">
        <div className="flex flex-wrap items-start justify-between gap-5">
          <div>
            <div className="flex items-center gap-3">
              <div
                className={cn(
                  "rounded-xl p-2.5",
                  running
                    ? "bg-emerald-500/15 text-emerald-500"
                    : "bg-muted text-muted-foreground",
                )}
              >
                <Power className="h-5 w-5" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h2 className="text-lg font-semibold">Cursor 模型转发</h2>
                  <Badge
                    variant={
                      running
                        ? "default"
                        : state?.phase === "error"
                          ? "destructive"
                          : "secondary"
                    }
                  >
                    {PHASE_LABEL[state?.phase ?? "stopped"] ?? state?.phase}
                  </Badge>
                </div>
                <p className="mt-1 text-sm text-muted-foreground">
                  已启用 {enabledCount} 个模型
                </p>
              </div>
            </div>
            <div className="mt-5 flex flex-wrap gap-2">
              {layers.map(([label, active]) => (
                <span
                  key={label}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs",
                    active
                      ? "border-emerald-500/25 bg-emerald-500/10 text-emerald-600 dark:text-emerald-400"
                      : "border-border-default bg-background/60 text-muted-foreground",
                  )}
                >
                  <span
                    className={cn(
                      "h-1.5 w-1.5 rounded-full",
                      active ? "bg-emerald-500" : "bg-muted-foreground/40",
                    )}
                  />
                  {label}
                </span>
              ))}
            </div>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={onRefresh}
            disabled={busy}
          >
            <RefreshCw className="mr-2 h-4 w-4" />
            刷新
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

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
          <Badge variant="outline">{getEndpointGroup(provider).label}</Badge>
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

const platformTrustLabel = (platform?: string) => {
  if (platform === "windows") return "CurrentUser\\Root";
  if (platform === "linux") return "System Trust Store";
  return "Login Keychain";
};
