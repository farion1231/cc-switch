import { useMemo, useState } from "react";
import {
  AlertTriangle,
  ExternalLink,
  KeyRound,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import type { DshProvider } from "@/lib/api/dsh";
import { dshErrorMessage, isDshConflictError } from "@/lib/api/dsh";
import { useDshActions, useDshSnapshot } from "@/lib/query/dsh";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { DshDefaultModelPicker } from "./DshDefaultModelPicker";
import { DshProviderDialog } from "./DshProviderDialog";

interface DshProvidersPageProps {
  onUnsupportedFeature?: (feature: string) => void;
}

function displayError(error: unknown, fallback: string): string {
  if (isDshConflictError(error))
    return "配置在编辑期间发生变化，请刷新后重试；你的草稿未被覆盖。";
  return dshErrorMessage(error, fallback);
}

/** Live DeepSeek Harness provider manager; it never uses SQLite provider state. */
export function DshProvidersPage({
  onUnsupportedFeature,
}: DshProvidersPageProps) {
  const query = useDshSnapshot();
  const actions = useDshActions();
  const [editing, setEditing] = useState<DshProvider | null | undefined>();
  const [confirmDelete, setConfirmDelete] = useState<DshProvider | null>(null);
  const [deleting, setDeleting] = useState(false);
  const snapshot = query.data;

  const sortedProviders = useMemo(() => {
    if (!snapshot) return [];
    return [...snapshot.providers].sort((left, right) =>
      left.kind === right.kind
        ? left.displayName.localeCompare(right.displayName)
        : left.kind === "native"
          ? -1
          : 1,
    );
  }, [snapshot]);

  const showFailure = (error: unknown, fallback: string) =>
    toast.error(displayError(error, fallback));
  const reload = async () => {
    try {
      await actions.refresh();
      toast.success("DSH 配置已刷新");
    } catch (error) {
      showFailure(error, "读取 DSH 配置失败");
    }
  };
  const saveCredentialIfNeeded = async (key?: {
    ref: string;
    value: string;
  }) => {
    if (key) await actions.setCredential(key);
  };
  const saveNative = async (
    input: Parameters<typeof actions.upsertNative>[0],
    key?: { ref: string; value: string },
  ) => {
    await actions.upsertNative(input);
    await saveCredentialIfNeeded(key);
    await actions.refresh();
    toast.success("DeepSeek Official 已保存");
  };
  const saveCustom = async (
    input: Parameters<typeof actions.createCustom>[0],
    key?: { ref: string; value: string },
  ) => {
    if (editing?.kind === "custom") await actions.updateCustom(input);
    else await actions.createCustom(input);
    await saveCredentialIfNeeded(key);
    await actions.refresh();
    toast.success(editing ? "Provider 已更新" : "Provider 已添加");
  };
  const removeProvider = async () => {
    if (!confirmDelete || !snapshot) return;
    setDeleting(true);
    try {
      if (snapshot.defaultModel?.provider === confirmDelete.route)
        throw new Error(
          "该 Provider 是新 Agent 的默认 Provider，请先选择替代模型。",
        );
      await actions.removeCustom(
        confirmDelete.route,
        confirmDelete.revision ?? snapshot.settingsRevision,
      );
      await actions.refresh();
      setConfirmDelete(null);
      toast.success("Provider 已移除");
    } catch (error) {
      showFailure(error, "Provider 移除失败");
    } finally {
      setDeleting(false);
    }
  };
  const resetNative = async () => {
    if (!snapshot) return;
    try {
      await actions.resetNative(snapshot.settingsRevision);
      await actions.refresh();
      toast.success("已恢复 DeepSeek 默认设置");
    } catch (error) {
      showFailure(error, "恢复默认设置失败");
    }
  };

  if (query.isLoading && !snapshot)
    return (
      <div className="p-6 text-sm text-muted-foreground">
        正在读取 DSH 配置…
      </div>
    );
  if (query.error && !snapshot)
    return (
      <div className="space-y-4 p-6">
        <Alert variant="destructive">
          <AlertTitle>无法读取 DSH 配置</AlertTitle>
          <AlertDescription>
            {displayError(query.error, "请检查 DSH Home、YAML 格式和文件权限")}
          </AlertDescription>
        </Alert>
        <Button type="button" onClick={() => void reload()}>
          <RefreshCw className="h-4 w-4" />
          重试
        </Button>
      </div>
    );
  if (!snapshot) return null;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-y-auto px-6 pb-12 pt-4"
      data-testid="dsh-providers-page"
    >
      <div className="mb-5 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">DeepSeek Harness</h1>
          <p className="mt-1 text-xs text-muted-foreground">
            实时管理 DSH settings.yaml 与 credentials；不会导入 cc-switch
            数据库。
          </p>
          <p className="mt-1 break-all text-xs text-muted-foreground">
            Home: {snapshot.home}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void actions.openHome()}
          >
            <ExternalLink className="h-4 w-4" />
            打开目录
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void reload()}
            disabled={query.isFetching}
          >
            <RefreshCw
              className={query.isFetching ? "h-4 w-4 animate-spin" : "h-4 w-4"}
            />
            刷新
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={() => setEditing(null)}
            disabled={snapshot.readOnly}
          >
            <Plus className="h-4 w-4" />
            添加 Provider
          </Button>
        </div>
      </div>
      {snapshot.readOnly && (
        <Alert className="mb-4">
          <AlertTitle>只读配置</AlertTitle>
          <AlertDescription>
            当前 DSH settings/credentials provider
            不可写；可以查看和刷新，但保存操作已禁用。
          </AlertDescription>
        </Alert>
      )}
      {snapshot.unsupported && snapshot.unsupported.length > 0 && (
        <Alert className="mb-4">
          <AlertTriangle className="h-4 w-4" />
          <AlertTitle>存在未编辑的配置</AlertTitle>
          <AlertDescription>
            页面只管理 native DeepSeek 和 OpenAI/Anthropic compatible
            routes；其他字段已保留，不会被覆盖。
            {onUnsupportedFeature && (
              <Button
                variant="link"
                className="h-auto p-0"
                onClick={() => onUnsupportedFeature("dsh-unsupported")}
              >
                了解范围
              </Button>
            )}
          </AlertDescription>
        </Alert>
      )}
      <div className="space-y-4">
        <DshDefaultModelPicker
          providers={sortedProviders}
          value={snapshot.defaultModel}
          disabled={snapshot.readOnly}
          onSave={async (selection) => {
            await actions.setDefaultModel(selection);
            await actions.refresh();
            toast.success("默认模型已保存");
          }}
        />
        <div className="grid gap-4">
          {sortedProviders.map((provider) => {
            const isDefault =
              snapshot.defaultModel?.provider === provider.route;
            return (
              <Card key={provider.route}>
                <CardHeader className="flex-row items-start justify-between gap-4 space-y-0 pb-3">
                  <div className="min-w-0">
                    <CardTitle className="flex flex-wrap items-center gap-2 text-base">
                      <span className="truncate">{provider.displayName}</span>
                      <Badge
                        variant={
                          provider.kind === "native" ? "default" : "secondary"
                        }
                      >
                        {provider.kind === "native"
                          ? "Native"
                          : (provider.api ?? "Custom")}
                      </Badge>
                      {isDefault && (
                        <Badge variant="outline">默认 Provider</Badge>
                      )}
                    </CardTitle>
                    <p className="mt-1 break-all text-xs text-muted-foreground">
                      {provider.route}
                      {provider.baseURL ? ` · ${provider.baseURL}` : ""}
                    </p>
                  </div>
                  <div className="flex shrink-0 gap-1">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      title="编辑"
                      onClick={() => setEditing(provider)}
                    >
                      <Pencil className="h-4 w-4" />
                    </Button>
                    {provider.kind === "native" ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="恢复默认"
                        onClick={() => void resetNative()}
                        disabled={snapshot.readOnly || !provider.customized}
                      >
                        <RotateCcw className="h-4 w-4" />
                      </Button>
                    ) : (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        title="删除"
                        onClick={() => setConfirmDelete(provider)}
                        disabled={snapshot.readOnly}
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    )}
                  </div>
                </CardHeader>
                <CardContent className="space-y-2 pt-0">
                  <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>
                      {provider.models.length || provider.modelCount || 0}{" "}
                      个模型
                    </span>
                    <span>·</span>
                    <span className="inline-flex items-center gap-1">
                      <KeyRound className="h-3.5 w-3.5" />
                      {provider.credential?.configured
                        ? provider.credential.source === "env"
                          ? "环境 key"
                          : "key 已配置"
                        : "未配置 key"}
                    </span>
                    {provider.credential?.source === "env" &&
                      !provider.credential.writable && <span>（只读）</span>}
                  </div>
                  {isDefault && snapshot.defaultModel && (
                    <p className="text-xs text-muted-foreground">
                      默认模型：{snapshot.defaultModel.model}
                    </p>
                  )}
                </CardContent>
              </Card>
            );
          })}
        </div>
      </div>
      <DshProviderDialog
        open={editing !== undefined}
        provider={editing ?? null}
        protocols={snapshot.protocols}
        readOnly={snapshot.readOnly}
        onClose={() => setEditing(undefined)}
        onSaveNative={saveNative}
        onSaveCustom={saveCustom}
        onDiscover={async (input) =>
          (await actions.discoverModels(input)).models
        }
      />
      {confirmDelete && (
        <div
          className="fixed inset-0 z-[70] flex items-center justify-center bg-black/50 p-4"
          role="dialog"
          aria-modal="true"
          aria-label="确认删除 Provider"
        >
          <div className="w-full max-w-md rounded-lg border bg-background p-5 shadow-lg">
            <h2 className="font-semibold">
              删除 {confirmDelete.displayName}？
            </h2>
            <p className="mt-2 text-sm text-muted-foreground">
              只删除 DSH route，不会自动删除共享 API key。若该 route 是默认
              Provider，必须先选择替代模型。
            </p>
            <div className="mt-5 flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => setConfirmDelete(null)}
              >
                取消
              </Button>
              <Button
                type="button"
                variant="destructive"
                onClick={() => void removeProvider()}
                disabled={deleting}
              >
                {deleting ? "删除中…" : "确认删除"}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default DshProvidersPage;
