import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  BarChart3,
  CheckCircle2,
  Download,
  Loader2,
  RefreshCw,
  Server,
  UploadCloud,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  providersApi,
  type RemoteProviderState,
  type RemoteUsageSyncProgressEvent,
  type SshConnectionTarget,
  type SshHostEntry,
} from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

interface RemoteProviderPageProps {
  appId: AppId;
  providers: Record<string, Provider>;
  currentProviderId: string;
  isLoading?: boolean;
}

const SUPPORTED_REMOTE_APPS: AppId[] = ["claude", "codex", "gemini"];
const SECRET_KEY_PATTERN =
  /(api[_-]?key|token|secret|password|authorization|credential|auth)/i;

const formatHostLabel = (host: SshHostEntry) => {
  const target = host.hostName
    ? `${host.hostName}${host.port ? `:${host.port}` : ""}`
    : "";
  const detail =
    host.user && target ? `${host.user}@${target}` : target || host.user || "";
  return detail ? `${host.alias} (${detail})` : host.alias;
};

const formatTargetLabel = (target: SshConnectionTarget | null) => {
  if (!target) return "";
  if (target.type === "config") return target.alias;

  const userPrefix = target.user ? `${target.user}@` : "";
  const portSuffix = target.port ? `:${target.port}` : "";
  return `${userPrefix}${target.host}${portSuffix}`;
};

const getTargetKey = (target: SshConnectionTarget | null) => {
  if (!target) return "";
  if (target.type === "config") return `config:${target.alias}`;
  return `manual:${target.user ?? ""}@${target.host}:${target.port ?? ""}`;
};

const areTargetsEqual = (
  left: SshConnectionTarget | null,
  right: SshConnectionTarget | null,
) => {
  if (!left || !right || left.type !== right.type) return false;
  if (left.type === "config" && right.type === "config") {
    return left.alias === right.alias;
  }
  if (left.type === "manual" && right.type === "manual") {
    return (
      left.host === right.host &&
      (left.user ?? "") === (right.user ?? "") &&
      (left.port ?? undefined) === (right.port ?? undefined) &&
      (left.password ?? "") === (right.password ?? "")
    );
  }
  return false;
};

const formatBytes = (bytes?: number) => {
  if (!bytes || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const digits = value >= 10 || unitIndex === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
};

const createUsageSyncId = () =>
  `remote-usage-${Date.now()}-${Math.random().toString(36).slice(2)}`;

const maskSecrets = (value: unknown, keyHint = ""): unknown => {
  if (Array.isArray(value)) {
    return value.map((item) => maskSecrets(item, keyHint));
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, item]) => [
        key,
        maskSecrets(item, key),
      ]),
    );
  }

  if (typeof value === "string" && SECRET_KEY_PATTERN.test(keyHint)) {
    return value.trim() ? "********" : value;
  }

  return value;
};

const getProviderSummary = (provider: Provider, appId: AppId) => {
  const config = provider.settingsConfig ?? {};
  if (appId === "claude") {
    return (
      config.env?.ANTHROPIC_BASE_URL ||
      config.env?.ANTHROPIC_MODEL ||
      provider.notes ||
      provider.websiteUrl ||
      ""
    );
  }

  if (appId === "codex") {
    const configText = typeof config.config === "string" ? config.config : "";
    const baseUrl = configText.match(/base_url\s*=\s*"([^"]+)"/)?.[1];
    return baseUrl || config.auth?.OPENAI_API_BASE || provider.notes || "";
  }

  if (appId === "gemini") {
    return (
      config.env?.GOOGLE_GEMINI_BASE_URL ||
      config.env?.GEMINI_MODEL ||
      provider.notes ||
      ""
    );
  }

  return provider.notes || "";
};

export function RemoteProviderPage({
  appId,
  providers,
  currentProviderId,
  isLoading = false,
}: RemoteProviderPageProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const isSupported = SUPPORTED_REMOTE_APPS.includes(appId);
  const [connectionMode, setConnectionMode] = useState<"config" | "manual">(
    "config",
  );
  const [selectedHost, setSelectedHost] = useState("");
  const [manualHost, setManualHost] = useState("");
  const [manualUser, setManualUser] = useState("");
  const [manualPort, setManualPort] = useState("22");
  const [manualPassword, setManualPassword] = useState("");
  const [connectedTarget, setConnectedTarget] =
    useState<SshConnectionTarget | null>(null);
  const [connectionVersion, setConnectionVersion] = useState(0);
  const [confirmApplyProvider, setConfirmApplyProvider] =
    useState<Provider | null>(null);
  const activeUsageSyncIdRef = useRef<string | null>(null);
  const [usageSyncProgress, setUsageSyncProgress] =
    useState<RemoteUsageSyncProgressEvent | null>(null);

  const hostsQuery = useQuery({
    queryKey: ["sshConfigHosts"],
    queryFn: () => providersApi.getSshHosts(),
    enabled: isSupported,
  });

  useEffect(() => {
    if (!selectedHost && hostsQuery.data?.length) {
      setSelectedHost(hostsQuery.data[0].alias);
    }
  }, [hostsQuery.data, selectedHost]);

  useEffect(() => {
    setConnectedTarget(null);
    setConnectionVersion((value) => value + 1);
  }, [appId]);

  const manualPortNumber = useMemo(() => {
    const trimmed = manualPort.trim();
    if (!trimmed) return undefined;
    const value = Number(trimmed);
    return Number.isInteger(value) && value > 0 && value <= 65535
      ? value
      : null;
  }, [manualPort]);

  const selectedTarget = useMemo<SshConnectionTarget | null>(() => {
    if (connectionMode === "config") {
      return selectedHost ? { type: "config", alias: selectedHost } : null;
    }

    const host = manualHost.trim();
    if (!host || manualPortNumber === null) return null;
    return {
      type: "manual",
      host,
      user: manualUser.trim() || undefined,
      port: manualPortNumber,
      password: manualPassword || undefined,
    };
  }, [
    connectionMode,
    manualHost,
    manualPassword,
    manualPortNumber,
    manualUser,
    selectedHost,
  ]);

  const selectedTargetKey = useMemo(
    () => getTargetKey(selectedTarget),
    [selectedTarget],
  );
  const connectedTargetKey = useMemo(
    () => getTargetKey(connectedTarget),
    [connectedTarget],
  );
  const connectedHost = useMemo(
    () => formatTargetLabel(connectedTarget),
    [connectedTarget],
  );

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void providersApi
      .onRemoteUsageSyncProgress((event) => {
        if (event.syncId === activeUsageSyncIdRef.current) {
          setUsageSyncProgress((current) => {
            if (event.stage !== "failed" || current?.syncId !== event.syncId) {
              return event;
            }

            return {
              ...current,
              ...event,
              currentStep: current.currentStep,
              totalSteps: current.totalSteps,
              fileCount: event.fileCount ?? current.fileCount,
              payloadBytes: event.payloadBytes ?? current.payloadBytes,
              filesScanned: event.filesScanned ?? current.filesScanned,
              imported: event.imported ?? current.imported,
              skipped: event.skipped ?? current.skipped,
              unchangedFiles: event.unchangedFiles ?? current.unchangedFiles,
            };
          });
        }
      })
      .then((nextUnlisten) => {
        if (disposed) {
          nextUnlisten();
          return;
        }
        unlisten = nextUnlisten;
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const remoteQuery = useQuery<RemoteProviderState>({
    queryKey: [
      "remoteProviderState",
      appId,
      connectedTargetKey,
      connectionVersion,
    ],
    queryFn: () => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.inspectRemote(appId, connectedTarget);
    },
    enabled: isSupported && Boolean(connectedTarget),
  });

  const localProviders = useMemo(() => Object.values(providers), [providers]);

  const matchedProvider = useMemo(() => {
    const matchedId = remoteQuery.data?.matchedProviderId;
    return matchedId ? providers[matchedId] : undefined;
  }, [providers, remoteQuery.data?.matchedProviderId]);

  const selectedHostInfo = useMemo(
    () => hostsQuery.data?.find((host) => host.alias === selectedHost),
    [hostsQuery.data, selectedHost],
  );

  const previewText = useMemo(() => {
    const config = remoteQuery.data?.provider?.settingsConfig;
    if (!config) return "";
    return JSON.stringify(maskSecrets(config), null, 2);
  }, [remoteQuery.data?.provider?.settingsConfig]);
  const remoteWarnings = remoteQuery.data?.warnings ?? [];
  const remoteFiles = remoteQuery.data?.files ?? [];
  const hasExistingRemoteConfig =
    remoteQuery.data?.hasExistingConfig ??
    remoteFiles.some((file) => file.exists);
  const hasUnmanagedRemoteConfig =
    remoteQuery.data?.hasUnmanagedConfig ??
    (hasExistingRemoteConfig && !remoteQuery.data?.matchedProviderId);
  const overwriteWarning =
    remoteQuery.data?.overwriteWarning ??
    t("remote.overwriteWarning", {
      defaultValue: "远端已有配置。切换会覆盖这些文件，建议先同步到本地。",
    });
  const usageSyncStage = usageSyncProgress?.stage ?? "starting";
  const usageSyncStageLabel = usageSyncProgress
    ? t(`remote.usageSyncProgress.stages.${usageSyncStage}`, {
        defaultValue: usageSyncStage,
        count: usageSyncProgress.fileCount ?? 0,
        imported: usageSyncProgress.imported ?? 0,
        skipped: usageSyncProgress.skipped ?? 0,
        unchangedFiles: usageSyncProgress.unchangedFiles ?? 0,
        filesScanned: usageSyncProgress.filesScanned ?? 0,
      })
    : "";

  const applyMutation = useMutation({
    mutationFn: ({
      provider,
      forceOverwrite,
    }: {
      provider: Provider;
      forceOverwrite: boolean;
    }) => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.applyToRemote(
        provider.id,
        appId,
        connectedTarget,
        forceOverwrite,
      );
    },
    onSuccess: async (result) => {
      setConfirmApplyProvider(null);
      toast.success(
        t("remote.applySuccess", {
          defaultValue: "远端已切换到选中的供应商",
        }),
        {
          description: `${result.hostAlias} - ${
            result.writtenFiles?.length ?? 0
          } files`,
        },
      );
      await queryClient.invalidateQueries({
        queryKey: ["remoteProviderState", appId, connectedTargetKey],
      });
    },
    onError: (error: unknown) => {
      toast.error(
        t("remote.applyFailed", {
          defaultValue: "远端切换失败: {{error}}",
          error: extractErrorMessage(error),
        }),
        { duration: 7000 },
      );
    },
  });

  const requestApplyProvider = (
    provider: Provider,
    isRemoteCurrent: boolean,
  ) => {
    if (isRemoteCurrent || applyMutation.isPending) return;

    if (hasUnmanagedRemoteConfig) {
      setConfirmApplyProvider(provider);
      return;
    }

    applyMutation.mutate({ provider, forceOverwrite: false });
  };

  const importMutation = useMutation({
    mutationFn: () => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.importRemote(appId, connectedTarget);
    },
    onSuccess: async (result) => {
      toast.success(
        t("remote.importSuccess", {
          defaultValue: "已同步远端配置到本地",
        }),
        { description: result.provider.name },
      );
      await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
      await queryClient.invalidateQueries({
        queryKey: ["remoteProviderState", appId, connectedTargetKey],
      });
    },
    onError: (error: unknown) => {
      toast.error(
        t("remote.importFailed", {
          defaultValue: "同步远端配置失败: {{error}}",
          error: extractErrorMessage(error),
        }),
        { duration: 7000 },
      );
    },
  });

  const syncUsageMutation = useMutation({
    mutationFn: ({ syncId }: { syncId: string }) => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.syncRemoteUsage(appId, connectedTarget, syncId);
    },
    onSuccess: async (result, variables) => {
      setUsageSyncProgress((current) =>
        current?.syncId === variables.syncId
          ? {
              ...current,
              stage: "completed",
              currentStep: current.totalSteps || 5,
              filesScanned: result.filesScanned,
              imported: result.imported,
              skipped: result.skipped,
              unchangedFiles: result.unchangedFiles,
            }
          : current,
      );
      toast.success(
        t("remote.usageSyncSuccess", {
          defaultValue: "已同步远端用量",
        }),
        {
          description: t("remote.usageSyncDescription", {
            defaultValue:
              "导入 {{imported}} 条，跳过 {{skipped}} 条，未变化文件 {{unchangedFiles}} 个",
            imported: result.imported,
            skipped: result.skipped,
            unchangedFiles: result.unchangedFiles,
          }),
        },
      );
      if (result.errors?.length) {
        toast.warning(
          t("remote.usageSyncPartial", {
            defaultValue: "部分远端日志解析失败",
          }),
          {
            description: result.errors.slice(0, 2).join("\n"),
            duration: 8000,
          },
        );
      }
      await queryClient.invalidateQueries({ queryKey: ["usage"] });
    },
    onError: (error: unknown, variables) => {
      const errorMessage = extractErrorMessage(error);
      setUsageSyncProgress((current) => {
        const fallback: RemoteUsageSyncProgressEvent = {
          syncId: variables.syncId,
          hostAlias: connectedHost,
          app: appId,
          stage: "failed",
          currentStep: current?.currentStep ?? 0,
          totalSteps: current?.totalSteps ?? 5,
          fileCount: current?.fileCount,
          payloadBytes: current?.payloadBytes,
          filesScanned: current?.filesScanned,
          imported: current?.imported,
          skipped: current?.skipped,
          unchangedFiles: current?.unchangedFiles,
          error: errorMessage,
        };
        return current?.syncId === variables.syncId ? fallback : current;
      });
      toast.error(
        t("remote.usageSyncFailed", {
          defaultValue: "同步远端用量失败: {{error}}",
          error: errorMessage,
        }),
        { duration: 7000 },
      );
    },
  });

  const usageSyncCanClose =
    Boolean(usageSyncProgress) &&
    (!syncUsageMutation.isPending || usageSyncStage === "failed");

  const startRemoteUsageSync = () => {
    if (!connectedTarget || syncUsageMutation.isPending) return;

    const syncId = createUsageSyncId();
    activeUsageSyncIdRef.current = syncId;
    setUsageSyncProgress({
      syncId,
      hostAlias: formatTargetLabel(connectedTarget),
      app: appId,
      stage: "starting",
      currentStep: 0,
      totalSteps: 5,
    });
    syncUsageMutation.mutate({ syncId });
  };

  const isSelectedHostConnected =
    Boolean(selectedTarget) &&
    areTargetsEqual(selectedTarget, connectedTarget) &&
    remoteQuery.isSuccess;
  const isInitialConnectingSelectedHost =
    Boolean(selectedTargetKey) &&
    connectedTargetKey === selectedTargetKey &&
    remoteQuery.isFetching &&
    !remoteQuery.data;

  const renderConnectionActions = (className?: string) => (
    <div className={cn("flex items-center gap-2", className)}>
      <Button
        onClick={() => {
          if (!selectedTarget) return;
          setConnectedTarget(selectedTarget);
          setConnectionVersion((value) => value + 1);
        }}
        disabled={
          !selectedTarget ||
          (connectionMode === "config" && hostsQuery.isLoading) ||
          isInitialConnectingSelectedHost ||
          isSelectedHostConnected
        }
        className={cn(
          isSelectedHostConnected &&
            "border-emerald-500/30 bg-emerald-600 text-white hover:bg-emerald-600 disabled:opacity-100 dark:bg-emerald-600 dark:text-white",
        )}
      >
        {isInitialConnectingSelectedHost ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : isSelectedHostConnected ? (
          <CheckCircle2 className="h-4 w-4" />
        ) : (
          <Server className="h-4 w-4" />
        )}
        {isInitialConnectingSelectedHost
          ? t("remote.connecting", { defaultValue: "连接中" })
          : isSelectedHostConnected
            ? t("remote.connected", { defaultValue: "已连接" })
            : t("remote.connect", { defaultValue: "连接" })}
      </Button>
      <Button
        variant="outline"
        size="icon"
        onClick={() => remoteQuery.refetch()}
        disabled={!connectedTarget || remoteQuery.isFetching}
        title={t("common.refresh")}
      >
        <RefreshCw
          className={cn("h-4 w-4", remoteQuery.isFetching && "animate-spin")}
        />
      </Button>
      <Button
        variant="outline"
        onClick={startRemoteUsageSync}
        disabled={!connectedTarget || syncUsageMutation.isPending}
      >
        {syncUsageMutation.isPending ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <BarChart3 className="h-4 w-4" />
        )}
        {t("remote.syncUsage", {
          defaultValue: "同步用量",
        })}
      </Button>
    </div>
  );

  if (!isSupported) {
    return (
      <div className="px-6 pt-4">
        <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
          {t("remote.unsupported", {
            defaultValue: "远端配置暂时只支持 Claude、Codex 和 Gemini。",
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-10 space-y-4">
      <section className="rounded-lg border border-border bg-card p-4">
        <Tabs
          value={connectionMode}
          onValueChange={(value) => {
            setConnectionMode(value as "config" | "manual");
            setConnectedTarget(null);
          }}
        >
          <TabsList className="mx-auto grid w-full max-w-[44rem] grid-cols-2 sm:w-[36rem]">
            <TabsTrigger value="config">
              {t("remote.configHostTab", { defaultValue: "SSH Host" })}
            </TabsTrigger>
            <TabsTrigger value="manual">
              {t("remote.manualHostTab", {
                defaultValue: "用户名/IP",
              })}
            </TabsTrigger>
          </TabsList>

          <TabsContent value="config" className="mt-3 space-y-2">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div className="space-y-2">
                <Label>
                  {t("remote.hostLabel", {
                    defaultValue: "SSH Host",
                  })}
                </Label>
                {hostsQuery.isLoading ? (
                  <div className="h-10 rounded-md border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
                    {t("remote.hostsLoading", {
                      defaultValue: "正在读取 ~/.ssh/config...",
                    })}
                  </div>
                ) : hostsQuery.data && hostsQuery.data.length > 0 ? (
                  <Select
                    value={selectedHost}
                    onValueChange={(value) => {
                      setSelectedHost(value);
                      setConnectedTarget(null);
                    }}
                  >
                    <SelectTrigger>
                      <SelectValue
                        placeholder={t("remote.hostPlaceholder", {
                          defaultValue: "选择 SSH Host",
                        })}
                      />
                    </SelectTrigger>
                    <SelectContent>
                      {hostsQuery.data.map((host) => (
                        <SelectItem key={host.alias} value={host.alias}>
                          {formatHostLabel(host)}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <div className="h-10 rounded-md border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
                    {t("remote.hostsEmpty", {
                      defaultValue: "没有在 ~/.ssh/config 中找到可用 Host。",
                    })}
                  </div>
                )}
              </div>
              {renderConnectionActions("sm:justify-end")}
            </div>
          </TabsContent>

          <TabsContent value="manual" className="mt-3 space-y-2">
            <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
              <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,0.75fr)_5.5rem_minmax(0,0.85fr)]">
                <div className="space-y-2">
                  <Label htmlFor="remote-manual-host">
                    {t("remote.manualHost", {
                      defaultValue: "IP / 域名",
                    })}
                  </Label>
                  <Input
                    id="remote-manual-host"
                    value={manualHost}
                    onChange={(event) => {
                      setManualHost(event.target.value);
                      setConnectedTarget(null);
                    }}
                    placeholder={t("remote.manualHostPlaceholder", {
                      defaultValue: "192.168.1.10",
                    })}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="remote-manual-user">
                    {t("remote.manualUser", {
                      defaultValue: "用户名",
                    })}
                  </Label>
                  <Input
                    id="remote-manual-user"
                    value={manualUser}
                    onChange={(event) => {
                      setManualUser(event.target.value);
                      setConnectedTarget(null);
                    }}
                    placeholder={t("remote.manualUserPlaceholder", {
                      defaultValue: "root",
                    })}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="remote-manual-port">
                    {t("remote.manualPort", {
                      defaultValue: "端口",
                    })}
                  </Label>
                  <Input
                    id="remote-manual-port"
                    type="text"
                    inputMode="numeric"
                    pattern="[0-9]*"
                    value={manualPort}
                    onChange={(event) => {
                      setManualPort(event.target.value);
                      setConnectedTarget(null);
                    }}
                    placeholder="22"
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="remote-manual-password">
                    {t("remote.manualPassword", {
                      defaultValue: "密码",
                    })}
                  </Label>
                  <Input
                    id="remote-manual-password"
                    type="password"
                    value={manualPassword}
                    onChange={(event) => {
                      setManualPassword(event.target.value);
                      setConnectedTarget(null);
                    }}
                    autoComplete="off"
                    placeholder={t("remote.manualPasswordPlaceholder", {
                      defaultValue: "可选",
                    })}
                  />
                </div>
              </div>
              {renderConnectionActions("sm:justify-end")}
            </div>
            {manualPortNumber === null && (
              <p className="text-xs text-destructive">
                {t("remote.manualPortInvalid", {
                  defaultValue: "端口必须在 1-65535 之间。",
                })}
              </p>
            )}
            <p className="text-xs text-muted-foreground">
              {t("remote.manualPasswordHint", {
                defaultValue:
                  "密码仅用于本次 SSH 连接，不会保存到本地配置或数据库。",
              })}
            </p>
          </TabsContent>
        </Tabs>

        {connectionMode === "config" && selectedHostInfo?.source && (
          <p className="mt-2 text-xs text-muted-foreground">
            {selectedHostInfo.source}
          </p>
        )}
      </section>

      {remoteQuery.isError && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
          {extractErrorMessage(remoteQuery.error)}
        </div>
      )}

      {!connectedHost && (
        <div className="rounded-lg border border-dashed border-border px-4 py-12 text-center text-sm text-muted-foreground">
          {t("remote.connectHint", {
            defaultValue:
              "选择一台 SSH 服务器并连接后，会显示远端当前配置和可切换的本地供应商。",
          })}
        </div>
      )}

      {connectedHost && remoteQuery.isLoading && (
        <div className="rounded-lg border border-dashed border-border px-4 py-12 text-center text-sm text-muted-foreground">
          <Loader2 className="mx-auto mb-3 h-5 w-5 animate-spin" />
          {t("remote.inspecting", {
            defaultValue: "正在读取远端配置...",
          })}
        </div>
      )}

      {remoteQuery.data && (
        <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(22rem,0.9fr)]">
          <section className="rounded-lg border border-border bg-card p-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold">
                  {t("remote.currentConfig", {
                    defaultValue: "远端当前配置",
                  })}
                </h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  {connectedHost} / {t(`apps.${appId}`)}
                </p>
              </div>
              {matchedProvider ? (
                <Badge
                  variant="secondary"
                  className="gap-1 bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300"
                >
                  <CheckCircle2 className="h-3.5 w-3.5" />
                  {matchedProvider.name}
                </Badge>
              ) : remoteQuery.data.provider ? (
                <Button
                  size="sm"
                  onClick={() => importMutation.mutate()}
                  disabled={importMutation.isPending}
                >
                  {importMutation.isPending ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Download className="h-4 w-4" />
                  )}
                  {t("remote.downloadLocal", {
                    defaultValue: "同步到本地",
                  })}
                </Button>
              ) : null}
            </div>

            {hasUnmanagedRemoteConfig && (
              <div className="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-900 dark:text-amber-200">
                <div className="flex items-center gap-2 font-medium">
                  <AlertTriangle className="h-4 w-4" />
                  {t("remote.overwriteRiskTitle", {
                    defaultValue: "远端已有未同步配置",
                  })}
                </div>
                <p className="mt-1 text-xs">{overwriteWarning}</p>
                {remoteQuery.data.provider && !matchedProvider && (
                  <Button
                    className="mt-3"
                    size="sm"
                    variant="outline"
                    onClick={() => importMutation.mutate()}
                    disabled={importMutation.isPending}
                  >
                    {importMutation.isPending ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Download className="h-4 w-4" />
                    )}
                    {t("remote.syncToLocalFirst", {
                      defaultValue: "先同步到本地",
                    })}
                  </Button>
                )}
              </div>
            )}

            {remoteWarnings.length > 0 && (
              <div className="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-amber-900 dark:text-amber-200">
                <div className="flex items-center gap-2 font-medium">
                  <AlertTriangle className="h-4 w-4" />
                  {t("remote.warningTitle", {
                    defaultValue: "远端配置不完整",
                  })}
                </div>
                <div className="mt-1 space-y-1 text-xs">
                  {remoteWarnings.map((warning) => (
                    <p key={warning}>{warning}</p>
                  ))}
                </div>
              </div>
            )}

            <div className="mt-4 grid gap-2 sm:grid-cols-2">
              {remoteFiles.map((file) => (
                <div
                  key={file.path}
                  className="rounded-md border border-border px-3 py-2 text-xs"
                >
                  <div className="truncate font-mono" title={file.path}>
                    {file.path}
                  </div>
                  <div
                    className={cn(
                      "mt-1",
                      file.exists
                        ? "text-emerald-600"
                        : "text-muted-foreground",
                    )}
                  >
                    {file.exists
                      ? t("remote.fileExists", {
                          defaultValue: "{{bytes}} bytes",
                          bytes: file.bytes,
                        })
                      : t("remote.fileMissing", {
                          defaultValue: "未找到",
                        })}
                  </div>
                </div>
              ))}
            </div>

            {previewText ? (
              <pre className="mt-4 max-h-[26rem] overflow-auto rounded-lg bg-muted p-3 text-xs leading-relaxed">
                {previewText}
              </pre>
            ) : (
              <div className="mt-4 rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
                {t("remote.noConfig", {
                  defaultValue: "这台服务器上还没有可识别的当前配置。",
                })}
              </div>
            )}
          </section>

          <section className="rounded-lg border border-border bg-card p-4">
            <div className="flex items-center justify-between gap-3">
              <div>
                <h2 className="text-base font-semibold">
                  {t("remote.localProviders", {
                    defaultValue: "切换远端供应商",
                  })}
                </h2>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t("remote.localProvidersHint", {
                    defaultValue: "选择一个本地供应商写入当前 SSH 服务器。",
                  })}
                </p>
              </div>
            </div>

            <div className="mt-4 space-y-2">
              {isLoading ? (
                <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
                  {t("common.loading")}
                </div>
              ) : localProviders.length === 0 ? (
                <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-sm text-muted-foreground">
                  {t("provider.noProviders")}
                </div>
              ) : (
                localProviders.map((provider) => {
                  const isRemoteCurrent =
                    remoteQuery.data?.matchedProviderId === provider.id;
                  const isApplying =
                    applyMutation.variables?.provider.id === provider.id;
                  const summary = getProviderSummary(provider, appId);
                  const willOverwriteUnmanaged =
                    hasUnmanagedRemoteConfig && !isRemoteCurrent;

                  return (
                    <div
                      key={provider.id}
                      className={cn(
                        "rounded-lg border border-border p-3 transition-colors",
                        isRemoteCurrent &&
                          "border-emerald-500/60 bg-emerald-500/10",
                      )}
                    >
                      <div className="flex items-center gap-3">
                        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted">
                          <ProviderIcon
                            icon={provider.icon}
                            name={provider.name}
                            color={provider.iconColor}
                            size={20}
                          />
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            <h3 className="truncate text-sm font-medium">
                              {provider.name}
                            </h3>
                            {provider.id === currentProviderId && (
                              <Badge variant="outline" className="rounded-md">
                                {t("remote.localCurrent", {
                                  defaultValue: "本地使用中",
                                })}
                              </Badge>
                            )}
                            {isRemoteCurrent && (
                              <Badge
                                variant="secondary"
                                className="rounded-md bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300"
                              >
                                {t("remote.remoteCurrent", {
                                  defaultValue: "远端当前",
                                })}
                              </Badge>
                            )}
                          </div>
                          {summary && (
                            <p className="mt-1 truncate text-xs text-muted-foreground">
                              {summary}
                            </p>
                          )}
                        </div>
                        <Button
                          size="sm"
                          variant={isRemoteCurrent ? "secondary" : "default"}
                          disabled={isRemoteCurrent || applyMutation.isPending}
                          onClick={() =>
                            requestApplyProvider(provider, isRemoteCurrent)
                          }
                        >
                          {isApplying && applyMutation.isPending ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : willOverwriteUnmanaged ? (
                            <AlertTriangle className="h-4 w-4" />
                          ) : (
                            <UploadCloud className="h-4 w-4" />
                          )}
                          {isRemoteCurrent
                            ? t("remote.applied", { defaultValue: "已应用" })
                            : t("remote.apply", { defaultValue: "切换" })}
                        </Button>
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </section>
        </div>
      )}

      {usageSyncProgress && (
        <Dialog
          modal={false}
          open={Boolean(usageSyncProgress)}
          onOpenChange={(open) => {
            if (!open && usageSyncCanClose) {
              activeUsageSyncIdRef.current = null;
              setUsageSyncProgress(null);
            }
          }}
        >
          <DialogContent
            className="max-w-md"
            overlayClassName="pointer-events-none"
          >
            <DialogHeader>
              <DialogTitle>
                {t("remote.usageSyncProgress.title", {
                  defaultValue: "同步远端用量",
                })}
              </DialogTitle>
              <DialogDescription>
                {usageSyncProgress.hostAlias} /{" "}
                {t(`apps.${usageSyncProgress.app}`)}
              </DialogDescription>
            </DialogHeader>

            <div className="space-y-4 px-6 py-5">
              <div className="flex items-start gap-3">
                <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-border bg-muted">
                  {usageSyncStage === "failed" ? (
                    <AlertTriangle className="h-4 w-4 text-destructive" />
                  ) : usageSyncStage === "completed" ? (
                    <CheckCircle2 className="h-4 w-4 text-emerald-600" />
                  ) : (
                    <Loader2 className="h-4 w-4 animate-spin text-primary" />
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="text-sm font-medium">
                    {usageSyncStageLabel}
                  </div>
                  <div className="mt-1 text-xs text-muted-foreground">
                    {t("remote.usageSyncProgress.step", {
                      defaultValue: "阶段 {{current}} / {{total}}",
                      current: usageSyncProgress.currentStep,
                      total: usageSyncProgress.totalSteps,
                    })}
                  </div>
                </div>
              </div>

              <div
                className="grid gap-1"
                style={{
                  gridTemplateColumns: `repeat(${Math.max(
                    usageSyncProgress.totalSteps,
                    1,
                  )}, minmax(0, 1fr))`,
                }}
                aria-hidden="true"
              >
                {Array.from({
                  length: Math.max(usageSyncProgress.totalSteps, 1),
                }).map((_, index) => {
                  const isReached =
                    usageSyncStage === "completed" ||
                    index < usageSyncProgress.currentStep;
                  return (
                    <div
                      key={index}
                      className={cn(
                        "h-2 rounded-full bg-muted transition-colors",
                        isReached &&
                          (usageSyncStage === "failed"
                            ? "bg-destructive"
                            : "bg-primary"),
                      )}
                    />
                  );
                })}
              </div>

              <div className="grid gap-2 text-xs text-muted-foreground sm:grid-cols-2">
                {usageSyncProgress.fileCount !== undefined && (
                  <div className="rounded-md border border-border px-3 py-2">
                    <div className="font-medium text-foreground">
                      {usageSyncProgress.fileCount}
                    </div>
                    <div>
                      {t("remote.usageSyncProgress.files", {
                        defaultValue: "日志文件",
                      })}
                    </div>
                  </div>
                )}
                {usageSyncProgress.payloadBytes !== undefined && (
                  <div className="rounded-md border border-border px-3 py-2">
                    <div className="font-medium text-foreground">
                      {formatBytes(usageSyncProgress.payloadBytes)}
                    </div>
                    <div>
                      {t("remote.usageSyncProgress.payload", {
                        defaultValue: "远端摘要",
                      })}
                    </div>
                  </div>
                )}
                {usageSyncProgress.imported !== undefined && (
                  <div className="rounded-md border border-border px-3 py-2">
                    <div className="font-medium text-foreground">
                      {usageSyncProgress.imported}
                    </div>
                    <div>
                      {t("remote.usageSyncProgress.imported", {
                        defaultValue: "导入记录",
                      })}
                    </div>
                  </div>
                )}
                {usageSyncProgress.skipped !== undefined && (
                  <div className="rounded-md border border-border px-3 py-2">
                    <div className="font-medium text-foreground">
                      {usageSyncProgress.skipped}
                    </div>
                    <div>
                      {t("remote.usageSyncProgress.skipped", {
                        defaultValue: "跳过记录",
                      })}
                    </div>
                  </div>
                )}
                {usageSyncProgress.unchangedFiles !== undefined && (
                  <div className="rounded-md border border-border px-3 py-2">
                    <div className="font-medium text-foreground">
                      {usageSyncProgress.unchangedFiles}
                    </div>
                    <div>
                      {t("remote.usageSyncProgress.unchangedFiles", {
                        defaultValue: "未变化文件",
                      })}
                    </div>
                  </div>
                )}
              </div>

              {usageSyncProgress.error && (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                  {usageSyncProgress.error}
                </div>
              )}
            </div>

            {usageSyncCanClose && (
              <DialogFooter>
                <Button
                  onClick={() => {
                    activeUsageSyncIdRef.current = null;
                    setUsageSyncProgress(null);
                  }}
                >
                  {t("remote.usageSyncProgress.close", {
                    defaultValue: "关闭",
                  })}
                </Button>
              </DialogFooter>
            )}
          </DialogContent>
        </Dialog>
      )}

      {confirmApplyProvider && (
        <ConfirmDialog
          isOpen={Boolean(confirmApplyProvider)}
          title={t("remote.confirmOverwriteTitle", {
            defaultValue: "确认覆盖远端配置？",
          })}
          message={`${overwriteWarning}\n\n${t(
            "remote.confirmOverwriteMessage",
            {
              defaultValue:
                "继续后会把选中的本地供应商写入远端。当前远端文件会先自动备份，但远端正在使用的配置会被替换。",
            },
          )}`}
          confirmText={t("remote.confirmOverwrite", {
            defaultValue: "确认切换",
          })}
          cancelText={t("common.cancel")}
          onConfirm={() =>
            applyMutation.mutate({
              provider: confirmApplyProvider,
              forceOverwrite: true,
            })
          }
          onCancel={() => setConfirmApplyProvider(null)}
        />
      )}
    </div>
  );
}
