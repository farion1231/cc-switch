import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Eye,
  EyeOff,
  Loader2,
  RefreshCw,
  Search,
  TestTube2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  useProxyWatchdogConfig,
  useProxyWatchdogStatus,
  useRefreshProxyWatchdog,
  useScanProxies,
  useSetProxyWatchdogConfig,
  useTestProxy,
  type DetectedProxy,
} from "@/hooks/useGlobalProxy";
import type { ProxyWatchdogMode } from "@/lib/api/globalProxy";

function extractAuth(url: string): {
  baseUrl: string;
  username: string;
  password: string;
} {
  if (!url.trim()) return { baseUrl: "", username: "", password: "" };

  try {
    const parsed = new URL(url);
    const username = decodeURIComponent(parsed.username || "");
    const password = decodeURIComponent(parsed.password || "");
    parsed.username = "";
    parsed.password = "";
    return { baseUrl: parsed.toString(), username, password };
  } catch {
    return { baseUrl: url, username: "", password: "" };
  }
}

function mergeAuth(
  baseUrl: string,
  username: string,
  password: string,
): string {
  if (!baseUrl.trim()) return "";
  if (!username.trim()) return baseUrl;

  try {
    const parsed = new URL(baseUrl);
    parsed.username = username.trim();
    if (password) {
      parsed.password = password;
    }
    return parsed.toString();
  } catch {
    const match = baseUrl.match(/^(\w+:\/\/)(.+)$/);
    if (match) {
      const auth = password
        ? `${encodeURIComponent(username.trim())}:${encodeURIComponent(password)}@`
        : `${encodeURIComponent(username.trim())}@`;
      return `${match[1]}${auth}${match[2]}`;
    }
    return baseUrl;
  }
}

export function GlobalProxySettings() {
  const { t } = useTranslation();
  const { data: watchdogConfig, isLoading } = useProxyWatchdogConfig();
  const { data: watchdogStatus } = useProxyWatchdogStatus();
  const setMutation = useSetProxyWatchdogConfig();
  const refreshMutation = useRefreshProxyWatchdog();
  const testMutation = useTestProxy();
  const scanMutation = useScanProxies();

  const [mode, setMode] = useState<ProxyWatchdogMode>("manualOn");
  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [detected, setDetected] = useState<DetectedProxy[]>([]);

  const fullUrl = useMemo(
    () => mergeAuth(url, username, password),
    [url, username, password],
  );

  useEffect(() => {
    if (watchdogConfig !== undefined) {
      const {
        baseUrl,
        username: u,
        password: p,
      } = extractAuth(watchdogConfig.proxyUrl || "");
      setMode(watchdogConfig.mode);
      setUrl(baseUrl);
      setUsername(u);
      setPassword(p);
      setDirty(false);
    }
  }, [watchdogConfig]);

  const handleSave = async () => {
    await setMutation.mutateAsync({ mode, proxyUrl: fullUrl });
    setDirty(false);
  };

  const handleTest = async () => {
    if (fullUrl) {
      await testMutation.mutateAsync(fullUrl);
    }
  };

  const handleScan = async () => {
    const result = await scanMutation.mutateAsync();
    setDetected(result);
  };

  const handleSelect = (proxyUrl: string) => {
    const { baseUrl, username: u, password: p } = extractAuth(proxyUrl);
    setUrl(baseUrl);
    setUsername(u);
    setPassword(p);
    setDirty(true);
    setDetected([]);
  };

  const handleClear = () => {
    setUrl("");
    setUsername("");
    setPassword("");
    setDirty(true);
  };

  const handleModeChange = (nextMode: ProxyWatchdogMode) => {
    setMode(nextMode);
    setDirty(true);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (
      e.key === "Enter" &&
      dirty &&
      !setMutation.isPending &&
      (mode === "manualOff" || fullUrl)
    ) {
      handleSave();
    }
  };

  if (isLoading && watchdogConfig === undefined) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const modeOptions: Array<{ value: ProxyWatchdogMode; label: string }> = [
    {
      value: "auto",
      label: t("settings.globalProxy.watchdogAuto", {
        defaultValue: "自动",
      }),
    },
    {
      value: "manualOn",
      label: t("settings.globalProxy.watchdogManualOn", {
        defaultValue: "手动开启",
      }),
    },
    {
      value: "manualOff",
      label: t("settings.globalProxy.watchdogManualOff", {
        defaultValue: "手动关闭",
      }),
    },
  ];

  const probeText =
    watchdogStatus?.lastProbeSuccess === null ||
    watchdogStatus?.lastProbeSuccess === undefined
      ? t("settings.globalProxy.notChecked", { defaultValue: "未检测" })
      : watchdogStatus.lastProbeSuccess
        ? t("settings.globalProxy.available", { defaultValue: "可用" })
        : t("settings.globalProxy.unavailable", { defaultValue: "不可用" });

  const effectiveText =
    watchdogStatus?.effectiveProxyUrl ||
    t("settings.globalProxy.direct", { defaultValue: "直连" });
  const routeText = watchdogStatus?.isProxying
    ? t("settings.globalProxy.proxying", { defaultValue: "代理中" })
    : t("settings.globalProxy.directing", { defaultValue: "直连中" });
  const canRefreshWatchdog = mode === "auto";

  return (
    <div className="space-y-3">
      <p className="text-sm text-muted-foreground">
        {t("settings.globalProxy.hint")}
      </p>

      <div className="flex flex-wrap items-center gap-2">
        {modeOptions.map((option) => (
          <Button
            key={option.value}
            type="button"
            size="sm"
            variant={mode === option.value ? "default" : "outline"}
            onClick={() => handleModeChange(option.value)}
          >
            {option.label}
          </Button>
        ))}
        <Button
          type="button"
          variant="outline"
          size="icon"
          disabled={!canRefreshWatchdog || refreshMutation.isPending}
          onClick={() => refreshMutation.mutate()}
          title={t("settings.globalProxy.watchdogRefresh", {
            defaultValue: canRefreshWatchdog ? "立即检测" : "仅自动模式可检测",
          })}
        >
          {refreshMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
        </Button>
      </div>

      {watchdogStatus && (
        <p className="text-xs text-muted-foreground">
          {t("settings.globalProxy.watchdogStatus", {
            defaultValue:
              "当前状态：{{route}}；当前生效：{{effective}}；最近检测：{{probe}}",
            route: routeText,
            effective: effectiveText,
            probe: probeText,
          })}
        </p>
      )}

      <div className="flex gap-2">
        <Input
          placeholder="http://127.0.0.1:7890 / socks5://127.0.0.1:1080"
          value={url}
          onChange={(e) => {
            setUrl(e.target.value);
            setDirty(true);
          }}
          onKeyDown={handleKeyDown}
          className="font-mono text-sm flex-1"
        />
        <Button
          variant="outline"
          size="icon"
          disabled={scanMutation.isPending}
          onClick={handleScan}
          title={t("settings.globalProxy.scan")}
        >
          {scanMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Search className="h-4 w-4" />
          )}
        </Button>
        <Button
          variant="outline"
          size="icon"
          disabled={!fullUrl || testMutation.isPending}
          onClick={handleTest}
          title={t("settings.globalProxy.test")}
        >
          {testMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <TestTube2 className="h-4 w-4" />
          )}
        </Button>
        <Button
          variant="outline"
          size="icon"
          disabled={!url && !username && !password}
          onClick={handleClear}
          title={t("settings.globalProxy.clear")}
        >
          <X className="h-4 w-4" />
        </Button>
        <Button
          onClick={handleSave}
          disabled={
            !dirty ||
            setMutation.isPending ||
            (mode !== "manualOff" && !fullUrl)
          }
          size="sm"
        >
          {setMutation.isPending && (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          )}
          {t("common.save")}
        </Button>
      </div>

      <div className="flex gap-2">
        <Input
          placeholder={t("settings.globalProxy.username")}
          value={username}
          onChange={(e) => {
            setUsername(e.target.value);
            setDirty(true);
          }}
          onKeyDown={handleKeyDown}
          className="font-mono text-sm flex-1"
        />
        <div className="relative flex-1">
          <Input
            type={showPassword ? "text" : "password"}
            placeholder={t("settings.globalProxy.password")}
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
              setDirty(true);
            }}
            onKeyDown={handleKeyDown}
            className="font-mono text-sm pr-10"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
            onClick={() => setShowPassword(!showPassword)}
            tabIndex={-1}
          >
            {showPassword ? (
              <EyeOff className="h-4 w-4 text-muted-foreground" />
            ) : (
              <Eye className="h-4 w-4 text-muted-foreground" />
            )}
          </Button>
        </div>
      </div>

      {detected.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {detected.map((p) => (
            <Button
              key={p.url}
              variant="secondary"
              size="sm"
              onClick={() => handleSelect(p.url)}
              className="font-mono text-xs"
            >
              {p.url}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
}
