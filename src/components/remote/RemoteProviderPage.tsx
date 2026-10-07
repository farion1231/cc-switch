import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  Loader2,
  Network,
  RefreshCw,
  RotateCcw,
  Server,
  UploadCloud,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  providersApi,
  type RemoteGatewayApplyResult,
  type RemoteGatewayState,
  type RemoteProviderState,
  type SshConnectionTarget,
  type SshHostEntry,
} from "@/lib/api/providers";
import { extractErrorMessage } from "@/utils/errorUtils";
import { proxyKeys } from "@/lib/query/proxy";
import { cn } from "@/lib/utils";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { RemoteGatewayCard } from "./RemoteGatewayCard";
import { REMOTE_GATEWAY_OVERVIEW_KEY } from "./RemoteGatewayIndicator";

interface RemoteProviderPageProps {
  appId: AppId;
  providers: Record<string, Provider>;
  currentProviderId: string;
  isLoading?: boolean;
  /** Selects and connects this host, e.g. when opened from the header. */
  focusTarget?: { target: SshConnectionTarget; nonce: number } | null;
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

/** Subscription logins stay on this machine and can't serve a remote host. */
const isLocalLoginProvider = (provider: Provider) =>
  provider.category === "official";

export function RemoteProviderPage({
  appId,
  providers,
  currentProviderId,
  isLoading = false,
  focusTarget,
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
  const [confirmRestart, setConfirmRestart] = useState(false);
  const [confirmDisableGateway, setConfirmDisableGateway] =
    useState<Provider | null>(null);

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

  const focusNonce = focusTarget?.nonce;
  useEffect(() => {
    const target = focusTarget?.target;
    if (!target) return;
    setConnectionMode(target.type);
    if (target.type === "config") {
      setSelectedHost(target.alias);
    } else {
      setManualHost(target.host);
      setManualUser(target.user ?? "");
      setManualPort(target.port ? String(target.port) : "");
      setManualPassword("");
    }
    setConnectedTarget(target);
    setConnectionVersion((value) => value + 1);
    // Only a new nonce (a new click) should re-focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusNonce]);

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

  const gatewayQueryKey = ["remoteGatewayState", appId, connectedTargetKey];
  const gatewayQuery = useQuery<RemoteGatewayState>({
    queryKey: gatewayQueryKey,
    queryFn: () => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.getRemoteGatewayState(appId, connectedTarget);
    },
    enabled: isSupported && Boolean(connectedTarget),
    refetchInterval: (query) => (query.state.data?.enabled ? 3000 : false),
  });
  const gatewayEnabled = gatewayQuery.data?.enabled ?? false;
  const gatewayProviderId = gatewayQuery.data?.providerId ?? null;
  const isPasswordTarget =
    connectedTarget?.type === "manual" && Boolean(connectedTarget.password);

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
      defaultValue:
        "远端已有配置。切换会替换其中的供应商配置，建议先同步到本地。",
    });

  // isPending only flips after a re-render, so fast repeated clicks (or a
  // confirm button clicked again) would otherwise send duplicate SSH requests.
  const inFlightRef = useRef(new Set<string>());
  const runExclusive = (key: string, run: () => void) => {
    if (inFlightRef.current.has(key)) return;
    inFlightRef.current.add(key);
    run();
  };
  const releaseExclusive = (key: string) => inFlightRef.current.delete(key);

  const restartMutation = useMutation({
    mutationFn: (target: SshConnectionTarget) =>
      providersApi.restartRemoteProcesses(appId, target),
    onSettled: () => releaseExclusive("restart"),
    onSuccess: (result) => {
      setConfirmRestart(false);
      const count = result.stopped.length;
      if (count === 0) {
        toast.info(
          t("remote.restartNone", {
            defaultValue: "远端没有正在运行的 {{app}} 进程",
            app: t(`apps.${appId}`),
          }),
        );
        return;
      }
      toast.success(
        t("remote.restartSuccess", {
          defaultValue: "已结束 {{count}} 个 {{app}} 进程",
          count,
          app: t(`apps.${appId}`),
        }),
        {
          description: [
            result.forceKilled.length > 0
              ? t("remote.restartForced", {
                  defaultValue: "{{count}} 个进程未响应，已强制结束。",
                  count: result.forceKilled.length,
                })
              : "",
            t("remote.restartHint", {
              defaultValue:
                "在 IDE 中重新打开对话或重新加载窗口即可使用新配置。",
            }),
          ]
            .filter(Boolean)
            .join(" "),
        },
      );
    },
    onError: (error: unknown) => {
      setConfirmRestart(false);
      toast.error(
        t("remote.restartFailed", {
          defaultValue: "停止远端进程失败: {{error}}",
          error: extractErrorMessage(error),
        }),
        { duration: 7000 },
      );
    },
  });

  const applyMutation = useMutation({
    mutationFn: ({
      provider,
      forceOverwrite,
      target,
    }: {
      provider: Provider;
      forceOverwrite: boolean;
      target: SshConnectionTarget;
    }) =>
      providersApi.applyToRemote(provider.id, appId, target, forceOverwrite),
    onSettled: () => releaseExclusive("apply"),
    onSuccess: (result, { target }) => {
      if (getTargetKey(target) === connectedTargetKey) {
        queryClient.setQueryData(
          ["remoteProviderState", appId, connectedTargetKey, connectionVersion],
          result.remoteState,
        );
      }
      toast.success(
        t("remote.applySuccess", {
          defaultValue: "远端已切换到选中的供应商",
        }),
        {
          description: `${result.hostAlias} - ${
            result.writtenFiles?.length ?? 0
          } files`,
          duration: 10000,
          action: {
            label: t("remote.restartProcesses", {
              defaultValue: "停止进程",
            }),
            onClick: () => startRestart(target),
          },
        },
      );
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
    if (isRemoteCurrent || !connectedTarget) return;

    if (hasUnmanagedRemoteConfig) {
      setConfirmApplyProvider(provider);
      return;
    }

    startApply(provider, false);
  };

  const startApply = (provider: Provider, forceOverwrite: boolean) => {
    if (!connectedTarget) return;
    const target = connectedTarget;
    runExclusive("apply", () =>
      applyMutation.mutate({ provider, forceOverwrite, target }),
    );
  };

  const startRestart = (target: SshConnectionTarget) =>
    runExclusive("restart", () => restartMutation.mutate(target));

  const startImport = () =>
    runExclusive("import", () => importMutation.mutate());

  const importMutation = useMutation({
    mutationFn: () => {
      if (!connectedTarget) {
        throw new Error("SSH target is not connected");
      }
      return providersApi.importRemote(appId, connectedTarget);
    },
    onSettled: () => releaseExclusive("import"),
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

  const storeGatewayResult = (
    target: SshConnectionTarget,
    result: RemoteGatewayApplyResult,
  ) => {
    const targetKey = getTargetKey(target);
    queryClient.setQueryData(
      ["remoteGatewayState", appId, targetKey],
      result.state,
    );
    void queryClient.invalidateQueries({
      queryKey: REMOTE_GATEWAY_OVERVIEW_KEY,
    });
    if (result.remoteState && targetKey === connectedTargetKey) {
      queryClient.setQueryData(
        ["remoteProviderState", appId, connectedTargetKey, connectionVersion],
        result.remoteState,
      );
    }
  };

  const restartAction = (target: SshConnectionTarget) => ({
    label: t("remote.restartProcesses", { defaultValue: "停止进程" }),
    onClick: () => startRestart(target),
  });

  const invalidateProxyQueries = () => {
    void queryClient.invalidateQueries({ queryKey: proxyKeys.status });
    void queryClient.invalidateQueries({ queryKey: proxyKeys.globalConfig });
  };

  const gatewayError = (error: unknown) =>
    toast.error(
      t("remote.gateway.failed", {
        defaultValue: "本机网关操作失败: {{error}}",
        error: extractErrorMessage(error),
      }),
      { duration: 8000 },
    );

  const enableGatewayMutation = useMutation({
    mutationFn: ({
      target,
      providerId,
      remotePort,
    }: {
      target: SshConnectionTarget;
      providerId: string | null;
      remotePort?: number;
    }) =>
      providersApi.enableRemoteGateway(appId, target, providerId, remotePort),
    onSettled: () => releaseExclusive("gateway"),
    onSuccess: (result, { target }) => {
      storeGatewayResult(target, result);
      invalidateProxyQueries();
      toast.success(
        t("remote.gateway.enabled", {
          defaultValue: "{{host}} 已改为经本机网关访问",
          host: result.state.hostKey,
        }),
        {
          description:
            result.writtenFiles.length > 0
              ? t("remote.gateway.enabledHint", {
                  defaultValue: "重启远端进程后生效，之后切换供应商无需重启。",
                })
              : undefined,
          duration: 10000,
          action:
            result.writtenFiles.length > 0 ? restartAction(target) : undefined,
        },
      );
    },
    onError: gatewayError,
  });

  const gatewayRouteMutation = useMutation({
    mutationFn: ({
      target,
      providerId,
    }: {
      target: SshConnectionTarget;
      providerId: string | null;
    }) => providersApi.setRemoteGatewayProvider(appId, target, providerId),
    onSettled: () => releaseExclusive("gateway"),
    onSuccess: (result, { target }) => {
      storeGatewayResult(target, result);
      toast.success(
        t("remote.gateway.routeSwitched", {
          defaultValue: "已切换，下一次请求生效",
        }),
      );
    },
    onError: gatewayError,
  });

  const disableGatewayMutation = useMutation({
    mutationFn: ({
      target,
      provider,
    }: {
      target: SshConnectionTarget;
      provider: Provider;
    }) => providersApi.disableRemoteGateway(provider.id, appId, target),
    onSettled: () => releaseExclusive("gateway"),
    onSuccess: async (result, { target }) => {
      if (getTargetKey(target) === connectedTargetKey) {
        queryClient.setQueryData(
          ["remoteProviderState", appId, connectedTargetKey, connectionVersion],
          result.remoteState,
        );
      }
      await queryClient.invalidateQueries({
        queryKey: ["remoteGatewayState", appId, getTargetKey(target)],
      });
      void queryClient.invalidateQueries({
        queryKey: REMOTE_GATEWAY_OVERVIEW_KEY,
      });
      toast.success(
        t("remote.gateway.disabled", {
          defaultValue: "已改回直连，远端使用 {{provider}}",
          provider: providers[result.providerId]?.name ?? result.providerId,
        }),
        { duration: 10000, action: restartAction(target) },
      );
    },
    onError: gatewayError,
  });

  const reconnectGatewayMutation = useMutation({
    mutationFn: (target: SshConnectionTarget) =>
      providersApi.reconnectRemoteGateway(appId, target),
    onSettled: () => releaseExclusive("gateway"),
    onSuccess: (state, target) => {
      queryClient.setQueryData(
        ["remoteGatewayState", appId, getTargetKey(target)],
        state,
      );
      void queryClient.invalidateQueries({
        queryKey: REMOTE_GATEWAY_OVERVIEW_KEY,
      });
      invalidateProxyQueries();
    },
    onError: gatewayError,
  });

  const gatewayBusy =
    enableGatewayMutation.isPending ||
    gatewayRouteMutation.isPending ||
    disableGatewayMutation.isPending ||
    reconnectGatewayMutation.isPending;

  const startEnableGateway = (remotePort?: number) => {
    if (!connectedTarget) return;
    const target = connectedTarget;
    // Re-applying keeps the route; first enable pins what the remote runs now.
    const matchedId = remoteQuery.data?.matchedProviderId;
    const providerId = gatewayEnabled
      ? gatewayProviderId
      : matchedId &&
          providers[matchedId] &&
          !isLocalLoginProvider(providers[matchedId])
        ? matchedId
        : null;
    runExclusive("gateway", () =>
      enableGatewayMutation.mutate({ target, providerId, remotePort }),
    );
  };

  const startGatewayRoute = (providerId: string | null) => {
    if (!connectedTarget || providerId === gatewayProviderId) return;
    const target = connectedTarget;
    runExclusive("gateway", () =>
      gatewayRouteMutation.mutate({ target, providerId }),
    );
  };

  const requestDisableGateway = () => {
    const candidates = [gatewayProviderId, currentProviderId]
      .map((id) => (id ? providers[id] : undefined))
      .filter((provider): provider is Provider => Boolean(provider));
    const provider =
      candidates.find((item) => !isLocalLoginProvider(item)) ?? candidates[0];
    if (!provider) {
      toast.error(
        t("remote.gateway.noDirectProvider", {
          defaultValue: "没有可写入远端的供应商，请先在本地添加一个。",
        }),
      );
      return;
    }
    setConfirmDisableGateway(provider);
  };

  const startDisableGateway = (provider: Provider) => {
    if (!connectedTarget) return;
    const target = connectedTarget;
    runExclusive("gateway", () =>
      disableGatewayMutation.mutate({ target, provider }),
    );
  };

  const startReconnectGateway = () => {
    if (!connectedTarget) return;
    const target = connectedTarget;
    runExclusive("gateway", () => reconnectGatewayMutation.mutate(target));
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
    </div>
  );

  const renderGatewayRow = ({
    key,
    routeId,
    title,
    summary,
    icon,
    badges,
    blockedReason,
  }: {
    key: string;
    routeId: string | null;
    title: string;
    summary?: string;
    icon: ReactNode;
    badges?: ReactNode;
    blockedReason?: string;
  }) => {
    const isActive = routeId === gatewayProviderId;
    const isSwitching =
      gatewayRouteMutation.isPending &&
      gatewayRouteMutation.variables?.providerId === routeId;
    return (
      <div
        key={key}
        className={cn(
          "rounded-lg border border-border p-3 transition-colors",
          isActive && "border-sky-500/60 bg-sky-500/10",
        )}
      >
        <div className="flex items-center gap-3">
          <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border bg-muted">
            {icon}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="truncate text-sm font-medium">{title}</h3>
              {badges}
            </div>
            {(blockedReason || summary) && (
              <p className="mt-1 truncate text-xs text-muted-foreground">
                {blockedReason || summary}
              </p>
            )}
          </div>
          <Button
            size="sm"
            variant={isActive ? "secondary" : "default"}
            disabled={isActive || Boolean(blockedReason) || gatewayBusy}
            onClick={() => startGatewayRoute(routeId)}
          >
            {isSwitching ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            {isActive
              ? t("remote.gateway.inUse", { defaultValue: "使用中" })
              : t("remote.gateway.use", { defaultValue: "使用" })}
          </Button>
        </div>
      </div>
    );
  };

  const renderGatewayProviderList = () => {
    const localCurrent = providers[currentProviderId];
    return (
      <>
        {renderGatewayRow({
          key: "__follow_local__",
          routeId: null,
          title: t("remote.gateway.followLocal", {
            defaultValue: "跟随本地",
          }),
          summary: t("remote.gateway.followLocalHint", {
            defaultValue: "使用本机当前供应商（{{name}}），本机切换时一起切换",
            name: localCurrent?.name ?? "-",
          }),
          icon: <RefreshCw className="h-4 w-4 text-muted-foreground" />,
        })}
        {localProviders.map((provider) =>
          renderGatewayRow({
            key: provider.id,
            routeId: provider.id,
            title: provider.name,
            summary: getProviderSummary(provider, appId),
            icon: (
              <ProviderIcon
                icon={provider.icon}
                name={provider.name}
                color={provider.iconColor}
                size={20}
              />
            ),
            badges:
              provider.id === currentProviderId ? (
                <Badge variant="outline" className="rounded-md">
                  {t("remote.localCurrent", { defaultValue: "本地使用中" })}
                </Badge>
              ) : undefined,
            blockedReason: isLocalLoginProvider(provider)
              ? t("remote.gateway.localLoginOnly", {
                  defaultValue: "依赖本机官方登录，不能给远端使用",
                })
              : undefined,
          }),
        )}
      </>
    );
  };

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
    <div className="mx-auto w-full max-w-[1040px] space-y-4">
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
        <RemoteGatewayCard
          appName={t(`apps.${appId}`)}
          hostLabel={connectedHost}
          state={gatewayQuery.data}
          passwordTarget={isPasswordTarget}
          enabling={enableGatewayMutation.isPending}
          disabling={disableGatewayMutation.isPending}
          reconnecting={reconnectGatewayMutation.isPending}
          onEnable={startEnableGateway}
          onDisable={requestDisableGateway}
          onReconnect={startReconnectGateway}
        />
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
              {remoteQuery.data.viaGateway ? (
                <Badge
                  variant="secondary"
                  className="gap-1 bg-sky-100 text-sky-700 dark:bg-sky-900/40 dark:text-sky-300"
                >
                  <Network className="h-3.5 w-3.5" />
                  {t("remote.gateway.viaGateway", {
                    defaultValue: "经本机网关",
                  })}
                </Badge>
              ) : matchedProvider ? (
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
                  onClick={startImport}
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
                    onClick={startImport}
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
                  {gatewayEnabled
                    ? t("remote.gateway.providersHint", {
                        defaultValue:
                          "经本机网关转发，切换即时生效，无需重启远端进程。",
                      })
                    : t("remote.localProvidersHint", {
                        defaultValue: "选择一个本地供应商写入当前 SSH 服务器。",
                      })}
                </p>
              </div>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setConfirmRestart(true)}
                disabled={!connectedTarget || restartMutation.isPending}
                title={t("remote.restartProcessesHint", {
                  defaultValue: "结束远端相关进程，让新配置立即生效",
                })}
              >
                {restartMutation.isPending ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <RotateCcw className="h-4 w-4" />
                )}
                {t("remote.restartProcesses", { defaultValue: "停止进程" })}
              </Button>
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
              ) : gatewayEnabled ? (
                renderGatewayProviderList()
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
                          disabled={
                            isRemoteCurrent ||
                            applyMutation.isPending ||
                            remoteQuery.isFetching
                          }
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
                "继续后会把选中的本地供应商写入远端。供应商相关配置会被替换，MCP、项目信任等远端设置会保留。",
            },
          )}`}
          confirmText={t("remote.confirmOverwrite", {
            defaultValue: "确认切换",
          })}
          cancelText={t("common.cancel")}
          onConfirm={() => {
            setConfirmApplyProvider(null);
            startApply(confirmApplyProvider, true);
          }}
          onCancel={() => setConfirmApplyProvider(null)}
        />
      )}

      {confirmDisableGateway && (
        <ConfirmDialog
          isOpen={Boolean(confirmDisableGateway)}
          title={t("remote.gateway.confirmDisableTitle", {
            defaultValue: "改回直连？",
          })}
          message={t("remote.gateway.confirmDisableMessage", {
            defaultValue:
              "会把「{{provider}}」直接写入 {{host}} 的 {{app}} 配置并断开隧道（其他应用仍在使用时保留隧道）。之后可在下方列表换成其他供应商。",
            provider: confirmDisableGateway.name,
            host: connectedHost,
            app: t(`apps.${appId}`),
          })}
          confirmText={t("remote.gateway.confirmDisable", {
            defaultValue: "改回直连",
          })}
          cancelText={t("common.cancel")}
          onConfirm={() => {
            const provider = confirmDisableGateway;
            setConfirmDisableGateway(null);
            startDisableGateway(provider);
          }}
          onCancel={() => setConfirmDisableGateway(null)}
        />
      )}

      <ConfirmDialog
        isOpen={confirmRestart}
        title={t("remote.confirmRestartTitle", {
          defaultValue: "停止远端 {{app}} 进程？",
          app: t(`apps.${appId}`),
        })}
        message={t("remote.confirmRestartMessage", {
          defaultValue:
            "会结束 {{host}} 上当前用户的所有 {{app}} 进程（包括 VS Code / Cursor 插件启动的进程），正在进行的对话会被中断。IDE 窗口本身不受影响，重新打开对话即可使用新配置。",
          host: connectedHost,
          app: t(`apps.${appId}`),
        })}
        confirmText={t("remote.restartProcesses", {
          defaultValue: "停止进程",
        })}
        cancelText={t("common.cancel")}
        variant="destructive"
        pending={restartMutation.isPending}
        onConfirm={() => {
          if (connectedTarget) startRestart(connectedTarget);
        }}
        onCancel={() => setConfirmRestart(false)}
      />
    </div>
  );
}
