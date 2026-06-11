import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe, Play, Square, Copy, ExternalLink, Shield } from "lucide-react";
import { toast } from "sonner";
import { ToggleRow } from "@/components/ui/toggle-row";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { settingsApi } from "@/lib/api/settings";
import { invokeCommand, isTauriRuntime } from "@/lib/commandClient";
import type { SettingsFormState } from "@/hooks/useSettings";

interface WebUiSettingsProps {
  settings: SettingsFormState;
  onChange: (updates: Partial<SettingsFormState>) => void;
}

interface WebUiStatus {
  running: boolean;
  address: string | null;
  enabled: boolean;
  port: number;
  host: string;
  tokenSet: boolean;
  authRequired: boolean;
}

export function WebUiSettings({ settings, onChange }: WebUiSettingsProps) {
  const { t } = useTranslation();
  const isTauri = isTauriRuntime();
  const [status, setStatus] = useState<WebUiStatus | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [passwordEnabledOverride, setPasswordEnabledOverride] = useState<
    boolean | null
  >(null);

  const fetchStatus = useCallback(async () => {
    try {
      const s = await invokeCommand<WebUiStatus>("get_webui_status");
      setStatus(s);
    } catch (e) {
      console.error("[WebUiSettings] Failed to fetch status", e);
    }
  }, []);

  useEffect(() => {
    fetchStatus();
    const interval = setInterval(fetchStatus, 5000);
    return () => clearInterval(interval);
  }, [fetchStatus]);

  const handleToggleEnabled = useCallback(
    (enabled: boolean) => {
      onChange({ webuiEnabled: enabled });
      if (!isTauri) return;
      // If disabling and server is running, stop it
      if (!enabled && status?.running) {
        invokeCommand("stop_webui_server")
          .then(() => fetchStatus())
          .catch((e) => toast.error(String(e)));
      }
      // If enabling and server is not running, start it
      if (enabled && !status?.running) {
        invokeCommand("start_webui_server")
          .then(() => fetchStatus())
          .catch((e) => toast.error(String(e)));
      }
    },
    [isTauri, onChange, status, fetchStatus],
  );

  const handleStart = useCallback(async () => {
    setIsLoading(true);
    try {
      await invokeCommand("start_webui_server");
      await fetchStatus();
      toast.success(
        t("settings.webui.started", { defaultValue: "WebUI 已启动" }),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setIsLoading(false);
    }
  }, [fetchStatus, t]);

  const handleStop = useCallback(async () => {
    setIsLoading(true);
    try {
      await invokeCommand("stop_webui_server");
      await fetchStatus();
      toast.success(
        t("settings.webui.stopped", { defaultValue: "WebUI 已停止" }),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setIsLoading(false);
    }
  }, [fetchStatus, t]);

  const handleRestart = useCallback(async () => {
    setIsLoading(true);
    try {
      await invokeCommand("restart_webui_server");
      await fetchStatus();
      toast.success(
        t("settings.webui.restarted", { defaultValue: "WebUI 已重启" }),
      );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setIsLoading(false);
    }
  }, [fetchStatus, t]);

  const handleCopyUrl = useCallback(() => {
    if (status?.address) {
      navigator.clipboard.writeText(status.address);
      toast.success(
        t("settings.webui.urlCopied", { defaultValue: "地址已复制" }),
      );
    }
  }, [status, t]);

  const handleOpenInBrowser = useCallback(async () => {
    if (status?.address) {
      try {
        await settingsApi.openExternal(status.address);
      } catch (e) {
        toast.error(String(e));
      }
    }
  }, [status]);

  const accessUrl =
    status?.address ||
    `http://${settings.webuiHost ?? "127.0.0.1"}:${settings.webuiPort ?? 15722}`;
  const isPublic =
    settings.webuiHost !== "127.0.0.1" && settings.webuiHost !== "localhost";
  const hasDraftPassword =
    typeof settings.webuiToken === "string" &&
    settings.webuiToken.trim().length > 0;
  const savedPasswordEnabled = Boolean(
    status?.tokenSet || status?.authRequired,
  );
  const requirePassword =
    passwordEnabledOverride ?? (hasDraftPassword || savedPasswordEnabled);

  const handleRequirePasswordChange = useCallback(
    (checked: boolean) => {
      setPasswordEnabledOverride(checked);
      onChange({
        webuiToken: checked ? (settings.webuiToken ?? "") : undefined,
      });
    },
    [onChange, settings.webuiToken],
  );

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <Globe className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">
          {t("settings.webui.title", { defaultValue: "WebUI 远程管理" })}
        </h3>
        {status?.running ? (
          <Badge
            variant="default"
            className="ml-auto text-xs bg-green-500/20 text-green-600 border-green-500/30"
          >
            {t("settings.webui.running", { defaultValue: "运行中" })}
          </Badge>
        ) : (
          <Badge variant="secondary" className="ml-auto text-xs">
            {t("settings.webui.stopped", { defaultValue: "已停止" })}
          </Badge>
        )}
      </div>

      <p className="text-xs text-muted-foreground">
        {t("settings.webui.description", {
          defaultValue:
            "启用后可通过浏览器远程管理 cc-switch，无需打开桌面应用。",
        })}
      </p>

      <ToggleRow
        icon={<Globe className="h-4 w-4 text-blue-500" />}
        title={t("settings.webui.enable", { defaultValue: "启用 WebUI" })}
        description={t("settings.webui.enableDescription", {
          defaultValue: "随应用启动自动开启 WebUI 服务",
        })}
        checked={settings.webuiEnabled !== false}
        onCheckedChange={handleToggleEnabled}
      />

      {status?.running && (
        <div className="rounded-lg border border-border/50 p-3 space-y-2">
          <Label className="text-xs text-muted-foreground">
            {t("settings.webui.accessUrl", { defaultValue: "访问地址" })}
          </Label>
          <div className="flex items-center gap-2">
            <code className="flex-1 text-sm font-mono bg-muted/50 px-3 py-1.5 rounded">
              {accessUrl}
            </code>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={handleCopyUrl}
              title={t("common.copy", { defaultValue: "复制" })}
            >
              <Copy className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={handleOpenInBrowser}
              title={t("settings.webui.openInBrowser", {
                defaultValue: "在浏览器中打开",
              })}
            >
              <ExternalLink className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      )}

      <div className="space-y-2">
        <Label className="text-xs">
          {t("settings.webui.host", { defaultValue: "监听地址" })}
        </Label>
        <div className="flex gap-2">
          <Button
            variant={!isPublic ? "default" : "outline"}
            size="sm"
            onClick={() => onChange({ webuiHost: "127.0.0.1" })}
          >
            {t("settings.webui.localhost", { defaultValue: "仅本机" })}
          </Button>
          <Button
            variant={isPublic ? "default" : "outline"}
            size="sm"
            onClick={() => onChange({ webuiHost: "0.0.0.0" })}
          >
            {t("settings.webui.lanAndPublic", { defaultValue: "局域网/公网" })}
          </Button>
        </div>
      </div>

      <div className="space-y-2">
        <Label className="text-xs">
          {t("settings.webui.port", { defaultValue: "端口" })}
        </Label>
        <Input
          type="number"
          min={1}
          max={65535}
          value={settings.webuiPort ?? 15722}
          onChange={(e) => {
            const val = e.target.value;
            if (val === "") {
              onChange({ webuiPort: undefined });
              return;
            }
            const port = parseInt(val, 10);
            if (!isNaN(port) && port >= 1 && port <= 65535) {
              onChange({ webuiPort: port });
            }
          }}
          className="w-32"
        />
      </div>

      <ToggleRow
        icon={<Shield className="h-4 w-4 text-amber-500" />}
        title={t("settings.webui.requirePassword", {
          defaultValue: "需要密码",
        })}
        description={t("settings.webui.requirePasswordDesc", {
          defaultValue: "启用后需要密码才能访问 WebUI",
        })}
        checked={requirePassword}
        onCheckedChange={handleRequirePasswordChange}
      />

      {requirePassword && (
        <div className="space-y-2">
          <Label className="text-xs">
            {t("settings.webui.password", { defaultValue: "密码" })}
          </Label>
          <Input
            type="password"
            placeholder={t("settings.webui.passwordPlaceholder", {
              defaultValue: "设置访问密码",
            })}
            value={hasDraftPassword ? (settings.webuiToken ?? "") : ""}
            onChange={(e) => {
              setPasswordEnabledOverride(true);
              onChange({ webuiToken: e.target.value });
            }}
          />
        </div>
      )}

      {isTauri && (
        <div className="flex items-center gap-2 pt-2">
          {!status?.running ? (
            <Button size="sm" onClick={handleStart} disabled={isLoading}>
              <Play className="h-3.5 w-3.5 mr-1.5" />
              {t("settings.webui.start", { defaultValue: "启动" })}
            </Button>
          ) : (
            <>
              <Button
                size="sm"
                variant="destructive"
                onClick={handleStop}
                disabled={isLoading}
              >
                <Square className="h-3.5 w-3.5 mr-1.5" />
                {t("settings.webui.stop", { defaultValue: "停止" })}
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={handleRestart}
                disabled={isLoading}
              >
                {t("settings.webui.restart", { defaultValue: "重启" })}
              </Button>
            </>
          )}
          <p className="text-xs text-muted-foreground ml-auto">
            {t("settings.webui.restartHint", {
              defaultValue: "修改端口或地址后需重启生效",
            })}
          </p>
        </div>
      )}
    </section>
  );
}
