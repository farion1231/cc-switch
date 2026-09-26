/**
 * 全局出站代理设置组件
 *
 * 提供配置全局代理的输入界面，支持用户名密码认证。
 */

import { useState, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ToggleRow } from "@/components/ui/toggle-row";
import {
  AlertTriangle,
  Loader2,
  Network,
  RefreshCw,
  TestTube2,
  Search,
  Eye,
  EyeOff,
  X,
} from "lucide-react";
import {
  useGlobalProxyUrl,
  useSetGlobalProxyUrl,
  useTestProxy,
  useScanProxies,
  useFollowSystemProxy,
  useSetFollowSystemProxy,
  useUpstreamProxyStatus,
  type DetectedProxy,
} from "@/hooks/useGlobalProxy";
import type { UpstreamProxyStatus } from "@/lib/api/globalProxy";

/** 从完整 URL 提取认证信息 */
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
    // 移除认证信息，获取基础 URL
    parsed.username = "";
    parsed.password = "";
    return { baseUrl: parsed.toString(), username, password };
  } catch {
    return { baseUrl: url, username: "", password: "" };
  }
}

/** 将认证信息合并到 URL */
function mergeAuth(
  baseUrl: string,
  username: string,
  password: string,
): string {
  if (!baseUrl.trim()) return "";
  if (!username.trim()) return baseUrl;

  try {
    const parsed = new URL(baseUrl);
    // URL 对象的 username/password setter 会自动进行 percent-encoding
    // 不要使用 encodeURIComponent，否则会导致双重编码
    parsed.username = username.trim();
    if (password) {
      parsed.password = password;
    }
    return parsed.toString();
  } catch {
    // URL 解析失败，尝试手动插入（此时需要手动编码）
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

interface EffectiveProxyStatusProps {
  status: UpstreamProxyStatus | undefined;
  onRefresh: () => void;
  refreshing: boolean;
  onReapply: () => void;
  reapplying: boolean;
}

/** 当前实际生效的出站代理：模式、来源、端口可达性与「已变化」提示 */
function EffectiveProxyStatus({
  status,
  onRefresh,
  refreshing,
  onReapply,
  reapplying,
}: EffectiveProxyStatusProps) {
  const { t } = useTranslation();
  if (!status) return null;

  const line = (() => {
    switch (status.mode) {
      case "explicit":
        return t("settings.globalProxy.status.explicit", {
          url: extractAuth(status.proxyUrl ?? "").baseUrl,
        });
      case "direct":
        return t("settings.globalProxy.status.direct");
      default:
        return status.systemProxyUrl
          ? t("settings.globalProxy.status.system", {
              url: status.systemProxyUrl,
            })
          : t("settings.globalProxy.status.systemNone");
    }
  })();
  const showSystemDetails = status.mode === "system" && !!status.systemProxyUrl;
  // 变化比较覆盖凭据与 http 槽，展示只取 https 槽的去凭据地址，所以「已变化」时展示地址可能没变
  const changedLine = !status.currentSystemProxyUrl
    ? t("settings.globalProxy.status.changedToNone")
    : status.currentSystemProxyUrl === status.systemProxyUrl
      ? t("settings.globalProxy.status.changedSameUrl")
      : t("settings.globalProxy.status.changed", {
          url: status.currentSystemProxyUrl,
        });

  return (
    <div className="space-y-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2 text-sm">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-muted-foreground">
          {t("settings.globalProxy.status.label")}
        </span>
        <span className="font-mono">{line}</span>
        {showSystemDetails && status.systemProxyReachable !== null && (
          <Badge
            variant={status.systemProxyReachable ? "secondary" : "destructive"}
            className="h-5"
          >
            {status.systemProxyReachable
              ? t("settings.globalProxy.status.reachable")
              : t("settings.globalProxy.status.unreachable")}
          </Badge>
        )}
        {/* 代理软件在应用打开期间退出或换端口时，状态不会自己变；只有跟随系统代理时重新探测才有意义 */}
        {status.mode === "system" && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="ml-auto h-6 w-6 p-0"
            onClick={onRefresh}
            disabled={refreshing}
            title={t("common.refresh")}
            aria-label={t("common.refresh")}
          >
            <RefreshCw
              className={`h-3.5 w-3.5 ${refreshing ? "animate-spin" : ""}`}
            />
          </Button>
        )}
      </div>
      {showSystemDetails && status.systemProxySource && (
        <p className="text-xs text-muted-foreground">
          {status.systemProxySource === "env"
            ? t("settings.globalProxy.status.sourceEnv")
            : t("settings.globalProxy.status.sourceSystem")}
        </p>
      )}
      {/* 本地路由对 Anthropic 格式供应商的转发走 hyper 直连路径，只认显式代理，不读系统代理；状态行说的是共享 reqwest 客户端 */}
      {status.mode === "system" && (
        <p className="text-xs text-muted-foreground">
          {t("settings.globalProxy.status.systemScope")}
        </p>
      )}
      {/* 后端只在跟随系统代理时才可能置位，此处无需再判 mode */}
      {status.systemProxyChanged && (
        <div className="flex flex-wrap items-center gap-2 text-xs text-yellow-600 dark:text-yellow-400">
          <AlertTriangle className="h-3.5 w-3.5" />
          <span>{changedLine}</span>
          <Button
            variant="outline"
            size="sm"
            className="h-6 px-2 text-xs"
            onClick={onReapply}
            disabled={reapplying}
          >
            {reapplying ? (
              <Loader2 className="mr-1 h-3 w-3 animate-spin" />
            ) : (
              <RefreshCw className="mr-1 h-3 w-3" />
            )}
            {t("settings.globalProxy.status.reapply")}
          </Button>
        </div>
      )}
    </div>
  );
}

export function GlobalProxySettings() {
  const { t } = useTranslation();
  const { data: savedUrl, isLoading } = useGlobalProxyUrl();
  const setMutation = useSetGlobalProxyUrl();
  const testMutation = useTestProxy();
  const scanMutation = useScanProxies();
  const { data: followSystemProxy } = useFollowSystemProxy();
  const followMutation = useSetFollowSystemProxy();
  const {
    data: proxyStatus,
    refetch: refetchProxyStatus,
    isFetching: proxyStatusFetching,
  } = useUpstreamProxyStatus();
  // 以已保存的地址为准：填了地址时开关不参与，不能看输入框里未保存的内容
  const hasSavedProxy = Boolean(savedUrl);
  const followChecked = followSystemProxy ?? true;

  const [url, setUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [detected, setDetected] = useState<DetectedProxy[]>([]);

  // 计算完整 URL（含认证信息）
  const fullUrl = useMemo(
    () => mergeAuth(url, username, password),
    [url, username, password],
  );

  // 同步远程配置
  useEffect(() => {
    if (savedUrl !== undefined) {
      const { baseUrl, username: u, password: p } = extractAuth(savedUrl || "");
      setUrl(baseUrl);
      setUsername(u);
      setPassword(p);
      setDirty(false);
    }
  }, [savedUrl]);

  const handleSave = async () => {
    await setMutation.mutateAsync(fullUrl);
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

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && dirty && !setMutation.isPending) {
      handleSave();
    }
  };

  // 只在首次加载且无数据时显示加载状态
  if (isLoading && savedUrl === undefined) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {/* 重新应用：用当前开关值再走一次 set，借它的重建路径把最新系统代理烘焙进客户端 */}
      <EffectiveProxyStatus
        status={proxyStatus}
        onRefresh={() => {
          void refetchProxyStatus();
        }}
        refreshing={proxyStatusFetching}
        onReapply={() => followMutation.mutate(followChecked)}
        reapplying={followMutation.isPending}
      />

      <ToggleRow
        icon={<Network className="h-4 w-4 text-cyan-500" />}
        title={t("settings.globalProxy.followSystemProxy")}
        description={
          hasSavedProxy
            ? t("settings.globalProxy.followSystemProxyExplicitHint")
            : t("settings.globalProxy.followSystemProxyDescription")
        }
        checked={followChecked}
        onCheckedChange={(checked) => followMutation.mutate(checked)}
        // 开关值未返回（加载中或失败）前禁用：此时的 true 只是占位，不是用户存的值
        disabled={
          hasSavedProxy ||
          followMutation.isPending ||
          followSystemProxy === undefined
        }
      />

      {/* 描述 */}
      <p className="text-sm text-muted-foreground">
        {t("settings.globalProxy.hint")}
      </p>

      {/* 代理地址输入框和按钮 */}
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
          disabled={!dirty || setMutation.isPending}
          size="sm"
        >
          {setMutation.isPending && (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          )}
          {t("common.save")}
        </Button>
      </div>

      {/* 认证信息：用户名 + 密码（可选） */}
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

      {/* 扫描结果 */}
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
