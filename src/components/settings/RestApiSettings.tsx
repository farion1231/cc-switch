/**
 * REST 控制面设置组件（镜像 GlobalProxySettings 范式）。
 *
 * 总开关 / host / port / token（密码框 + 眼睛切换 + 复制 + 重生成）。
 * 仅 `rest_api` feature on 时面板可见（由父级 useRestAvailable 控制）。
 */

import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Loader2, Eye, EyeOff, Copy, RefreshCw, Check } from "lucide-react";
import {
  useRestConfig,
  useSetRestConfig,
  useRegenerateToken,
} from "@/hooks/useRest";
import type { RestConfig } from "@/lib/api/rest";

export function RestApiSettings() {
  const { t } = useTranslation();
  const { data: saved, isLoading } = useRestConfig();
  const setMutation = useSetRestConfig();
  const regenMutation = useRegenerateToken();

  const [cfg, setCfg] = useState<RestConfig | null>(null);
  const [dirty, setDirty] = useState(false);
  const [showToken, setShowToken] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (saved) {
      setCfg(saved);
      setDirty(false);
    }
  }, [saved]);

  if (isLoading && !cfg) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (!cfg) return null;

  const update = (patch: Partial<RestConfig>) => {
    setCfg({ ...cfg, ...patch });
    setDirty(true);
  };

  const handleSave = async () => {
    await setMutation.mutateAsync(cfg);
    setDirty(false);
  };

  const handleRegenerate = async () => {
    await regenMutation.mutateAsync();
  };

  const handleCopy = async () => {
    if (!cfg.token) return;
    await navigator.clipboard.writeText(cfg.token);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="space-y-3">
      <p className="text-sm text-muted-foreground">
        {t("settings.restApi.hint")}
      </p>

      {/* 总开关 */}
      <div className="flex items-center justify-between">
        <span className="text-sm">{t("settings.restApi.enable")}</span>
        <Switch
          checked={cfg.enabled}
          onCheckedChange={(v) => update({ enabled: v })}
        />
      </div>

      {/* host + port */}
      <div className="flex gap-2">
        <Input
          placeholder="127.0.0.1"
          value={cfg.listenAddress}
          onChange={(e) => update({ listenAddress: e.target.value })}
          className="font-mono text-sm flex-1"
        />
        <Input
          type="number"
          placeholder="8787"
          value={cfg.listenPort}
          onChange={(e) => update({ listenPort: Number(e.target.value) || 0 })}
          className="font-mono text-sm w-28"
        />
      </div>

      {/* token */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Input
            type={showToken ? "text" : "password"}
            placeholder={t("settings.restApi.tokenPlaceholder")}
            value={cfg.token}
            onChange={(e) => update({ token: e.target.value })}
            className="font-mono text-sm pr-10"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
            onClick={() => setShowToken(!showToken)}
            tabIndex={-1}
          >
            {showToken ? (
              <EyeOff className="h-4 w-4 text-muted-foreground" />
            ) : (
              <Eye className="h-4 w-4 text-muted-foreground" />
            )}
          </Button>
        </div>
        <Button
          variant="outline"
          size="icon"
          onClick={handleCopy}
          disabled={!cfg.token}
          title={t("settings.restApi.copy")}
        >
          {copied ? (
            <Check className="h-4 w-4" />
          ) : (
            <Copy className="h-4 w-4" />
          )}
        </Button>
        <Button
          variant="outline"
          size="icon"
          onClick={handleRegenerate}
          disabled={regenMutation.isPending}
          title={t("settings.restApi.regenerate")}
        >
          {regenMutation.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="h-4 w-4" />
          )}
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
    </div>
  );
}
