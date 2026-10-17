import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Input } from "@/components/ui/input";
import {
  Wifi,
  Globe,
  Server,
  Loader2,
  Laptop,
  ShieldCheck,
} from "lucide-react";
import { toast } from "sonner";
import {
  remoteApi,
  type BindScope,
  type NetworkAddress,
} from "@/lib/api/remote";

interface RemoteSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

/**
 * Allowed `BindScope` values, in the backend's narrow-to-wide order.
 *
 * The order is load-bearing twice: the UI uses it to decide "is this a
 * widening?" (which needs the confirmation panel + native dialog) and the
 * backend uses the same order to decide whether a request can apply
 * immediately. Keep it in sync with the Rust enum.
 */
const BIND_SCOPE_ORDER: readonly BindScope[] = [
  "loopback",
  "tailscale",
  "allInterfaces",
] as const;

function bindScopeRank(scope: BindScope): number {
  return BIND_SCOPE_ORDER.indexOf(scope);
}

/**
 * `indexOf(unknown) === -1` 会把任何一次点击都判成「放宽」。契约外的值一律
 * 当作最紧的一档处理，而不是让状态变成一个谁也不认识的字符串。
 */
function isBindScope(value: unknown): value is BindScope {
  return BIND_SCOPE_ORDER.includes(value as BindScope);
}

export function RemoteSettings({ settings, onChange }: RemoteSettingsProps) {
  const { t } = useTranslation();
  const [toggling, setToggling] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [serverRunning, setServerRunning] = useState<boolean | null>(null);
  const [tailscaleIp, setTailscaleIp] = useState<string | null>(null);
  const [networkAddresses, setNetworkAddresses] = useState<NetworkAddress[]>(
    [],
  );
  const [bindScope, setBindScope] = useState<BindScope>("loopback");
  const [bindScopeBusy, setBindScopeBusy] = useState(false);
  const [pendingScope, setPendingScope] = useState<BindScope | null>(null);
  const checkingRef = useRef(false);

  const port = settings.remotePort || 4000;

  const checkServerStatus = useCallback(
    async (targetPort?: number) => {
      if (checkingRef.current) return false;
      checkingRef.current = true;
      const probePort = targetPort ?? port;
      try {
        const res = await fetch(`http://127.0.0.1:${probePort}/api/health`);
        setServerRunning(res.ok);
        return res.ok;
      } catch {
        setServerRunning(false);
        return false;
      } finally {
        checkingRef.current = false;
      }
    },
    [port],
  );

  useEffect(() => {
    const doCheck = () => checkServerStatus();
    doCheck();
    let interval: ReturnType<typeof setInterval>;
    const startInterval = () => {
      interval = setInterval(doCheck, 5000);
    };
    const handleVisibilityChange = () => {
      if (document.hidden) {
        clearInterval(interval);
      } else {
        doCheck();
        startInterval();
      }
    };
    startInterval();
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [checkServerStatus]);

  // 同步 server 实际状态与 toggle 持久化状态
  // 当检测到服务器未运行但设置显示已启用时，自动关闭
  useEffect(() => {
    if (
      serverRunning === false &&
      settings.remoteEnabled &&
      !toggling &&
      !restarting
    ) {
      onChange({ remoteEnabled: false });
    }
  }, [serverRunning, settings.remoteEnabled, toggling, restarting, onChange]);

  // 绑定范围的权威来源是后端的 exposure.json（经 `get_remote_config` 读出），
  // 不是 settings.json 里的旧 `remoteTailscaleEnabled` 旗标 —— 后者只保留做兼容，
  // 不再参与绑定决策。
  useEffect(() => {
    let alive = true;
    remoteApi
      .getRemoteConfig()
      .then((config) => {
        // 契约外的值退回最紧的一档：`undefined` 会让三个单选框全不选中。
        if (alive) {
          setBindScope(
            isBindScope(config.bindScope) ? config.bindScope : "loopback",
          );
        }
      })
      .catch(() => {
        // 服务未起 / 命令失败时保持默认（最紧的一档），不猜。
      });
    return () => {
      alive = false;
    };
  }, [settings.remoteEnabled, settings.remotePort]);

  useEffect(() => {
    if (!settings.remoteEnabled) {
      setTailscaleIp(null);
      return;
    }
    remoteApi
      .getTailscaleIp()
      .then((ip) => setTailscaleIp(ip))
      .catch(() => setTailscaleIp(null));
  }, [settings.remoteEnabled, bindScope]);

  // 「全部网卡」要回答的是「我到底会在哪些地址上暴露」，所以直接列本机所有
  // 非回环 IPv4（含 Tailscale 那条）。只在选到这一档、或正准备放宽到这一档时
  // 才去扫网卡，不增加其它档位的开销。
  useEffect(() => {
    if (bindScope !== "allInterfaces" && pendingScope !== "allInterfaces") {
      return;
    }
    let alive = true;
    remoteApi
      .listNetworkAddresses()
      .then((list) => {
        if (alive) setNetworkAddresses(list);
      })
      .catch(() => {
        // 取不到就只显示 0.0.0.0 那一行，不编造地址。
      });
    return () => {
      alive = false;
    };
  }, [bindScope, pendingScope]);

  const handleEnabledToggle = async (value: boolean) => {
    setToggling(true);

    try {
      if (value) {
        const startPort = port;
        await remoteApi.startRemoteServer(startPort, bindScope !== "loopback");
        // 等待服务器真正启动
        await new Promise((resolve) => setTimeout(resolve, 1000));
        const isRunning = await checkServerStatus(startPort);
        if (!isRunning) {
          throw new Error("Server failed to start");
        }
        setServerRunning(true);
      } else {
        await remoteApi.stopRemoteServer();
        setServerRunning(false);
      }
      // 只有 RPC 成功后才持久化
      onChange({ remoteEnabled: value });
    } catch (e) {
      toast.error(`${t("settings.remoteRestartFailed")} ${e}`);
    } finally {
      setToggling(false);
    }
  };

  const restartServer = async (
    nextPort: number,
    updateSettings: () => void,
    delayMs: number = 1000,
  ) => {
    setRestarting(true);
    try {
      await remoteApi.restartRemoteServer(nextPort, bindScope !== "loopback");
      await new Promise((resolve) => setTimeout(resolve, delayMs));
      const isRunning = await checkServerStatus(nextPort);
      if (!isRunning) {
        throw new Error("Server failed to restart");
      }
      updateSettings();
    } catch (e) {
      toast.error(`${t("settings.remoteRestartFailed")} ${e}`);
    } finally {
      setRestarting(false);
    }
  };

  /**
   * 申请一次档位变更。
   *
   * 变宽：后端会弹**原生**对话框；用户点确认之前配置不变、listener 不重绑。
   * 变窄：后端立即落盘并重绑，不弹窗。
   */
  const applyBindScope = async (target: BindScope) => {
    setBindScopeBusy(true);
    try {
      const result = await remoteApi.requestBindScopeChange(target);
      setBindScope(result.bindScope);
      setPendingScope(null);
      if (result.applied) {
        onChange({ remoteTailscaleEnabled: result.bindScope !== "loopback" });
        toast.success(t("remote.settings.bindScope.applied"));
      } else {
        // 原生对话框被取消（或后端因故没有落实）：保持原档位，明确告知。
        toast.error(t("remote.settings.bindScope.notApplied"));
      }
    } catch (e) {
      toast.error(`${t("remote.settings.bindScope.failed")} ${e}`);
    } finally {
      setBindScopeBusy(false);
    }
  };

  const handleBindScopeSelect = async (target: BindScope) => {
    if (!settings.remoteEnabled || bindScopeBusy || restarting || toggling) {
      return;
    }
    if (target === bindScope) return;

    if (bindScopeRank(target) > bindScopeRank(bindScope)) {
      // 变宽：先确认 Tailscale 真的可用，再展示确认面板。面板之后还有 Rust 侧
      // 的原生对话框 —— 第二段的决定权不在渲染层。
      if (target === "tailscale" || target === "allInterfaces") {
        try {
          const available = await remoteApi.checkTailscaleAvailable();
          if (!available) {
            // AllInterfaces 不依赖 Tailscale（只是多列一条地址），不拦。
            if (target === "tailscale") {
              toast.error(t("remote.settings.bindScope.tailscaleUnavailable"));
              return;
            }
          }
        } catch {
          if (target === "tailscale") {
            toast.error(t("remote.settings.bindScope.tailscaleUnavailable"));
            return;
          }
        }
      }
      setPendingScope(target);
      return;
    }

    // 变窄：立即生效。
    await applyBindScope(target);
  };

  const newReachableAddresses = (() => {
    if (!pendingScope) return [] as string[];
    const addresses = new Set<string>();
    const targetRank = bindScopeRank(pendingScope);
    const currentRank = bindScopeRank(bindScope);
    if (
      targetRank >= bindScopeRank("tailscale") &&
      currentRank < bindScopeRank("tailscale") &&
      tailscaleIp
    ) {
      addresses.add(`http://${tailscaleIp}:${port}`);
    }
    if (
      targetRank >= bindScopeRank("allInterfaces") &&
      currentRank < bindScopeRank("allInterfaces")
    ) {
      if (networkAddresses.length === 0) {
        // 还没拿到网卡列表（或本机没有非回环地址）时，至少给出绑定本身。
        addresses.add(
          t("remote.settings.bindScope.allInterfacesAddress", { port }),
        );
      }
      for (const { address } of networkAddresses) {
        addresses.add(`http://${address}:${port}`);
      }
    }
    return [...addresses];
  })();

  const scopeDisabled =
    !settings.remoteEnabled || bindScopeBusy || restarting || toggling;

  const scopeIcon = (scope: BindScope) => {
    if (scope === "loopback")
      return <Laptop className="h-4 w-4 text-muted-foreground" />;
    if (scope === "tailscale")
      return <Globe className="h-4 w-4 text-blue-500" />;
    return <ShieldCheck className="h-4 w-4 text-amber-500" />;
  };

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Wifi className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.remoteManagement")}
        </h3>
      </div>

      <div className="space-y-3">
        <div className="relative">
          <ToggleRow
            icon={
              toggling ? (
                <Loader2 className="h-4 w-4 text-green-500 animate-spin" />
              ) : (
                <Server className="h-4 w-4 text-green-500" />
              )
            }
            title={t("settings.remoteEnabled")}
            description={t("settings.remoteEnabledDescription")}
            checked={!!settings.remoteEnabled}
            onCheckedChange={handleEnabledToggle}
            disabled={toggling || restarting}
          />
        </div>

        <div className="space-y-2">
          <div className="text-sm font-medium">
            {t("remote.settings.bindScope.label")}
          </div>
          <div className="text-xs text-muted-foreground">
            {t("remote.settings.bindScope.description")}
          </div>
          <div
            role="radiogroup"
            aria-label={t("remote.settings.bindScope.label")}
            className="space-y-1"
          >
            {BIND_SCOPE_ORDER.map((scope) => (
              <label
                key={scope}
                className="flex items-start gap-2 rounded-md border border-border/50 px-3 py-2 cursor-pointer"
              >
                <input
                  type="radio"
                  name="remote-bind-scope"
                  value={scope}
                  checked={bindScope === scope}
                  disabled={scopeDisabled}
                  onChange={() => handleBindScopeSelect(scope)}
                  className="mt-1"
                />
                <span className="flex flex-col">
                  <span className="flex items-center gap-1.5 text-sm">
                    {scopeIcon(scope)}
                    {t(`remote.settings.bindScope.${scope}`)}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {t(`remote.settings.bindScope.${scope}Description`)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </div>

        {pendingScope && (
          <div
            role="dialog"
            aria-label={t("remote.settings.bindScope.widenTitle")}
            className="space-y-2 rounded-md border border-amber-500/40 bg-amber-500/5 p-3"
          >
            <div className="text-sm font-medium">
              {t("remote.settings.bindScope.widenTitle")}
            </div>
            <div className="text-sm">
              {t("remote.settings.bindScope.widenTransition", {
                current: t(`remote.settings.bindScope.${bindScope}`),
                target: t(`remote.settings.bindScope.${pendingScope}`),
              })}
            </div>
            {newReachableAddresses.length > 0 && (
              <div className="text-xs">
                <div className="text-muted-foreground">
                  {t("remote.settings.bindScope.newAddresses")}
                </div>
                <ul className="list-disc pl-5 font-mono">
                  {newReachableAddresses.map((address) => (
                    <li key={address}>{address}</li>
                  ))}
                </ul>
              </div>
            )}
            <div className="text-xs text-muted-foreground">
              {t("remote.settings.bindScope.credentialNote")}
            </div>
            <div className="flex items-center gap-2 pt-1">
              <button
                type="button"
                className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground disabled:opacity-50"
                disabled={bindScopeBusy}
                onClick={() => applyBindScope(pendingScope)}
              >
                {t("remote.settings.bindScope.confirm")}
              </button>
              <button
                type="button"
                className="rounded-md border border-border px-3 py-1.5 text-sm disabled:opacity-50"
                disabled={bindScopeBusy}
                onClick={() => setPendingScope(null)}
              >
                {t("remote.settings.bindScope.cancel")}
              </button>
            </div>
          </div>
        )}

        {bindScope === "tailscale" && tailscaleIp && (
          <div className="ml-1 text-xs text-muted-foreground">
            {t("remote.settings.bindScope.tailscale")}{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 font-mono">
              http://{tailscaleIp}:{port}
            </code>
          </div>
        )}

        {bindScope === "allInterfaces" && (
          <div className="ml-1 space-y-1 text-xs text-muted-foreground">
            <div>
              {t("remote.settings.bindScope.allInterfacesAddress", { port })}
            </div>
            {networkAddresses.map(({ interface: name, address }) => (
              <div key={address} className="flex items-center gap-2">
                <code className="rounded bg-muted px-1.5 py-0.5 font-mono">
                  http://{address}:{port}
                </code>
                {name && <span>{name}</span>}
                {address === tailscaleIp && <span>Tailscale</span>}
              </div>
            ))}
          </div>
        )}

        <div className="flex items-center justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium">
              {t("settings.remotePort")}
            </div>
            <div className="text-xs text-muted-foreground">
              {t("settings.remotePortDescription")}
            </div>
          </div>
          <Input
            type="number"
            min={1024}
            max={65535}
            value={port}
            disabled={restarting || toggling}
            onChange={async (e) => {
              const val = parseInt(e.target.value, 10);
              if (!isNaN(val) && val >= 1024 && val <= 65535) {
                if (serverRunning) {
                  await restartServer(
                    val,
                    () => onChange({ remotePort: val }),
                    1500,
                  );
                  return;
                }
                onChange({ remotePort: val });
              }
            }}
            className="w-24 text-center font-mono text-sm"
          />
        </div>

        <div className="flex items-center gap-2">
          <span
            className={`inline-block h-2 w-2 rounded-full ${
              restarting
                ? "bg-yellow-500 animate-pulse"
                : serverRunning
                  ? "bg-green-500"
                  : "bg-muted-foreground/40"
            }`}
          />
          <span className="text-sm text-muted-foreground">
            {restarting
              ? t("settings.remoteRestarting")
              : serverRunning
                ? t("settings.remoteStatusRunning")
                : t("settings.remoteStatusStopped")}
          </span>
        </div>
      </div>
    </section>
  );
}
