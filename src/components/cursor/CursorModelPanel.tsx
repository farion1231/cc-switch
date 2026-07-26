import { useMemo, useState } from "react";
import {
  Activity,
  BadgeCheck,
  CircleAlert,
  Gauge,
  KeyRound,
  Pencil,
  Play,
  Plus,
  Power,
  RefreshCw,
  ShieldCheck,
  ShieldMinus,
  Square,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  useCursorProviders,
  useCursorRuntimeState,
  useDeleteCursorProvider,
  useInstallCursorCA,
  useRemoveCursorCA,
  useSaveCursorProvider,
  useStartCursorRuntime,
  useStopCursorRuntime,
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

export function CursorModelPanel() {
  const providersQuery = useCursorProviders();
  const runtimeQuery = useCursorRuntimeState();
  const saveProvider = useSaveCursorProvider();
  const deleteProvider = useDeleteCursorProvider();
  const toggleProvider = useToggleCursorProvider();
  const startRuntime = useStartCursorRuntime();
  const stopRuntime = useStopCursorRuntime();
  const installCA = useInstallCursorCA();
  const removeCA = useRemoveCursorCA();
  const testModel = useTestCursorModel();

  const [editing, setEditing] = useState<CursorProvider | null>(null);
  const [modelDialogOpen, setModelDialogOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<CursorProvider | null>(null);
  const [trustDialogOpen, setTrustDialogOpen] = useState(false);
  const [tests, setTests] = useState<Record<string, CursorModelTestResult>>({});
  const [batchTesting, setBatchTesting] = useState(false);

  const providers = useMemo(
    () =>
      Object.values(providersQuery.data ?? {}).sort((left, right) => {
        const typeOrder = left.settingsConfig.type.localeCompare(
          right.settingsConfig.type,
        );
        return typeOrder || left.name.localeCompare(right.name);
      }),
    [providersQuery.data],
  );
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

  const start = async () => {
    try {
      await startRuntime.mutateAsync(undefined);
      toast.success("Cursor 模型转发已启动");
    } catch (error) {
      reportError("启动 Cursor 模型转发失败", error);
    }
  };

  const handleStart = () => {
    if (!state?.caInstalled) {
      setTrustDialogOpen(true);
      return;
    }
    void start();
  };

  const handleStop = async () => {
    try {
      await stopRuntime.mutateAsync(undefined);
      toast.success("Cursor 已恢复原始配置");
    } catch (error) {
      reportError("停止 Cursor 模型转发失败", error);
    }
  };

  const handleInstallCAAndStart = async () => {
    setTrustDialogOpen(false);
    try {
      await installCA.mutateAsync(undefined);
      await startRuntime.mutateAsync(undefined);
      toast.success("CA 已信任，Cursor 模型转发已启动");
    } catch (error) {
      reportError("启用 Cursor 模型转发失败", error);
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
        toast.error(`${provider.name} 测速失败`, { description: result.error });
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
          onStart={handleStart}
          onStop={() => void handleStop()}
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
            <div className="flex gap-2">
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
              <Button
                size="sm"
                onClick={() => {
                  setEditing(null);
                  setModelDialogOpen(true);
                }}
              >
                <Plus className="mr-2 h-4 w-4" />
                添加模型
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
            ) : (
              <div className="divide-y divide-border-default">
                {providers.map((provider) => (
                  <ModelRow
                    key={provider.id}
                    provider={provider}
                    test={tests[provider.id]}
                    testing={
                      testModel.isPending && testModel.variables === provider.id
                    }
                    disabled={busy}
                    onToggle={async (enabled) => {
                      try {
                        await toggleProvider.mutateAsync({
                          id: provider.id,
                          enabled,
                        });
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
                ))}
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
                  onClick={() => void installCA.mutateAsync(undefined)}
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
      <TrustDialog
        open={trustDialogOpen}
        platform={state?.platform}
        onOpenChange={setTrustDialogOpen}
        onConfirm={() => void handleInstallCAAndStart()}
      />
    </div>
  );
}

function RuntimeCard({
  state,
  enabledCount,
  running,
  busy,
  onStart,
  onStop,
  onRefresh,
}: {
  state?: CursorRuntimeState;
  enabledCount: number;
  running: boolean;
  busy: boolean;
  onStart: () => void;
  onStop: () => void;
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
          <div className="flex flex-wrap gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={onRefresh}
              disabled={busy}
            >
              <RefreshCw className="mr-2 h-4 w-4" />
              刷新
            </Button>
            {running ? (
              <Button
                size="sm"
                variant="destructive"
                onClick={onStop}
                disabled={busy}
              >
                <Square className="mr-2 h-4 w-4" />
                停止并恢复
              </Button>
            ) : (
              <Button
                size="sm"
                onClick={onStart}
                disabled={busy || enabledCount === 0}
              >
                <Play className="mr-2 h-4 w-4" />
                启动
              </Button>
            )}
          </div>
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
          <Badge variant="outline">
            {config.type === "anthropic" ? "Anthropic" : "OpenAI"}
          </Badge>
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

function TrustDialog({
  open,
  platform,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  platform?: string;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  const explanation =
    platform === "windows"
      ? "证书会安装到当前用户的 Windows Root 证书库，不需要管理员权限。"
      : platform === "linux"
        ? "系统会通过 pkexec 请求管理员授权，并安装到当前发行版的系统信任库。"
        : "证书会安装到当前用户的 macOS 登录钥匙串，系统可能要求确认。";
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <KeyRound className="h-5 w-5 text-blue-500" />
            首次启用 Cursor 转发
          </DialogTitle>
          <DialogDescription>
            Cursor 的 HTTPS 流量需要信任本机为本安装生成的独立 CA。
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 px-6 py-5 text-sm">
          <p>{explanation}</p>
          <div className="rounded-lg border border-border-default bg-muted/30 p-3 text-muted-foreground">
            私钥仅保存在本机 CC Switch 数据目录。停止服务会恢复 Cursor 配置，但
            CA 会保留；你可以稍后独立移除。
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={onConfirm}>
            <BadgeCheck className="mr-2 h-4 w-4" />
            信任并启动
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

const platformTrustLabel = (platform?: string) => {
  if (platform === "windows") return "CurrentUser\\Root";
  if (platform === "linux") return "System Trust Store";
  return "Login Keychain";
};
