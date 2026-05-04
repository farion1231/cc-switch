import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Input } from "@/components/ui/input";
import { Wifi, Globe, Server, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";

interface RemoteSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

export function RemoteSettings({ settings, onChange }: RemoteSettingsProps) {
  const { t } = useTranslation();
  const [toggling, setToggling] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [serverRunning, setServerRunning] = useState<boolean | null>(null);
  const [tailscaleIp, setTailscaleIp] = useState<string | null>(null);
  const checkingRef = useRef(false);

  const checkServerStatus = useCallback(
    async (port?: number) => {
      if (checkingRef.current) return false;
      checkingRef.current = true;
      const targetPort = port ?? settings.remotePort ?? 4000;
      try {
        const res = await fetch(`http://127.0.0.1:${targetPort}/api/health`);
        setServerRunning(res.ok);
        return res.ok;
      } catch {
        setServerRunning(false);
        return false;
      } finally {
        checkingRef.current = false;
      }
    },
    [settings.remotePort],
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

  useEffect(() => {
    if (settings.remoteTailscaleEnabled && settings.remoteEnabled) {
      invoke<string | null>("get_tailscale_ip")
        .then((ip) => {
          setTailscaleIp(ip);
        })
        .catch(() => {
          setTailscaleIp(null);
        });
    } else {
      setTailscaleIp(null);
    }
  }, [settings.remoteTailscaleEnabled, settings.remoteEnabled]);

  const handleEnabledToggle = async (value: boolean) => {
    setToggling(true);

    try {
      if (value) {
        const startPort = settings.remotePort || 4000;
        await invoke("start_remote_server", {
          port: startPort,
          tailscaleEnabled: settings.remoteTailscaleEnabled,
        });
        // 等待服务器真正启动
        await new Promise((resolve) => setTimeout(resolve, 1000));
        const isRunning = await checkServerStatus(startPort);
        if (!isRunning) {
          throw new Error("Server failed to start");
        }
        setServerRunning(true);
      } else {
        await invoke("stop_remote_server");
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
    port: number,
    tailscaleEnabled: boolean,
    updateSettings: () => void,
    delayMs: number = 1000,
  ) => {
    setRestarting(true);
    try {
      await invoke("restart_remote_server", { port, tailscaleEnabled });
      await new Promise((resolve) => setTimeout(resolve, delayMs));
      const isRunning = await checkServerStatus(port);
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

  const handleTailscaleToggle = async (value: boolean) => {
    if (value) {
      try {
        const available = await invoke<boolean>("check_tailscale_available");
        if (!available) {
          toast.error(t("settings.remoteTailscaleNotAvailable"));
          return;
        }
      } catch {
        toast.error(t("settings.remoteTailscaleNotAvailable"));
        return;
      }
    }

    if (serverRunning) {
      await restartServer(settings.remotePort || 4000, value, () =>
        onChange({ remoteTailscaleEnabled: value }),
      );
    } else {
      onChange({ remoteTailscaleEnabled: value });
    }
  };

  const port = settings.remotePort || 4000;

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

        <ToggleRow
          icon={<Globe className="h-4 w-4 text-blue-500" />}
          title={t("settings.remoteTailscaleEnabled")}
          description={t("settings.remoteTailscaleEnabledDescription")}
          checked={!!settings.remoteTailscaleEnabled}
          onCheckedChange={handleTailscaleToggle}
          disabled={!settings.remoteEnabled || restarting || toggling}
        />

        {tailscaleIp && settings.remoteTailscaleEnabled && (
          <div className="ml-1 text-xs text-muted-foreground">
            {t("settings.remoteTailscaleAddress")}{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 font-mono">
              http://{tailscaleIp}:{port}
            </code>
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
                    !!settings.remoteTailscaleEnabled,
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
