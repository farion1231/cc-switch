import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, Loader2, TestTube2 } from "lucide-react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { useGlobalProxyUrl, useTestProxy } from "@/hooks/useGlobalProxy";
import { isValidProxyUrl, joinProxyUrl, splitProxyUrl } from "@/lib/proxyUrl";

interface ProviderProxyFieldProps {
  /** 输入框的 id（用于 label htmlFor）；各表单需传入各自唯一值。 */
  id: string;
  /** 完整代理地址，账号密码嵌在 URL 里（与全局代理的存法一致） */
  value: string;
  onChange: (value: string) => void;
  /**
   * 应用自己直连供应商、不经 CC Switch 转发（OpenCode、Hermes 等）：代理只作用于
   * CC Switch 替这家供应商发的请求（测速、获取模型），说明文案随之改变。
   */
  directOnly?: boolean;
}

type ProxyParts = ReturnType<typeof splitProxyUrl>;

/**
 * 供应商级出站代理字段（各应用的供应商表单共用）：代理地址 + 可选的用户名、密码。
 *
 * 只在未启用全局代理时生效：全局代理已配置时三个输入框置灰禁用并跟随显示全局代理，
 * 供应商已存的值保留不改动，并提示改由全局代理接管。校验口径与后端 `http_client::validate_proxy` 一致。
 */
export function ProviderProxyField({
  id,
  value,
  onChange,
  directOnly = false,
}: ProviderProxyFieldProps) {
  const { t } = useTranslation();
  const { data: globalProxyUrl } = useGlobalProxyUrl();
  const testMutation = useTestProxy();
  const [showPassword, setShowPassword] = useState(false);

  // 拆开的三段放本地：只填了密码、还没填用户名时合并结果里没有密码，不能再从 value 反推。
  const [parts, setParts] = useState<ProxyParts>(() => splitProxyUrl(value));
  const lastEmittedRef = useRef(value);
  useEffect(() => {
    // 只在外部改了值（表单重置、切换供应商）时重新拆分
    if (value !== lastEmittedRef.current) {
      lastEmittedRef.current = value;
      setParts(splitProxyUrl(value));
    }
  }, [value]);

  const updateParts = (patch: Partial<ProxyParts>) => {
    const next = { ...parts, ...patch };
    setParts(next);
    const joined = joinProxyUrl(next.baseUrl, next.username, next.password);
    lastEmittedRef.current = joined;
    onChange(joined);
  };

  const globalEnabled = !!globalProxyUrl?.trim();
  // 全局代理生效时跟随显示全局代理；供应商自己的值原样保留不改。
  const shown = globalEnabled ? splitProxyUrl(globalProxyUrl!.trim()) : parts;
  const trimmed = value.trim();
  const valid = !trimmed || isValidProxyUrl(trimmed);
  const label = t("providerForm.proxyUrl", { defaultValue: "代理地址" });
  const disabledClass = globalEnabled && "bg-subtle text-fg-3";

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-0.5">
        <FormLabel htmlFor={id} className={cn(globalEnabled && "text-fg-3")}>
          {label}
        </FormLabel>
        <HelpTip title={label}>
          {directOnly
            ? t("providerForm.proxyUrlHelpDirect", {
                defaultValue:
                  "CC Switch 替这家供应商发的请求（测速、获取模型）走这个代理；应用本身直连供应商，不受影响。留空则跟随系统代理。启用全局代理时以全局代理为准。",
              })
            : t("providerForm.proxyUrlHelp", {
                defaultValue:
                  "这家供应商的请求（转发、测速、获取模型）走这个代理。留空则跟随系统代理。启用全局代理时以全局代理为准。",
              })}
        </HelpTip>
      </div>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="text"
          value={shown.baseUrl}
          onChange={(e) => updateParts({ baseUrl: e.target.value })}
          placeholder="http://127.0.0.1:7890 / socks5://127.0.0.1:1080"
          autoComplete="off"
          disabled={globalEnabled}
          aria-invalid={!valid && !globalEnabled}
          className={cn("flex-1 font-mono text-sm", disabledClass)}
        />
        <HoverTip content={t("settings.globalProxy.test")}>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="shrink-0"
            disabled={
              globalEnabled || !trimmed || !valid || testMutation.isPending
            }
            onClick={() => testMutation.mutate(trimmed)}
            aria-label={t("settings.globalProxy.test")}
          >
            {testMutation.isPending ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <TestTube2 className="h-4 w-4" />
            )}
          </Button>
        </HoverTip>
      </div>
      <div className="flex items-center gap-2">
        <Input
          type="text"
          value={shown.username}
          onChange={(e) => updateParts({ username: e.target.value })}
          placeholder={t("settings.globalProxy.username", {
            defaultValue: "用户名（可选）",
          })}
          aria-label={t("settings.globalProxy.username", {
            defaultValue: "用户名（可选）",
          })}
          autoComplete="off"
          disabled={globalEnabled}
          className={cn("flex-1 font-mono text-sm", disabledClass)}
        />
        <div className="relative flex-1">
          <Input
            type={showPassword && !globalEnabled ? "text" : "password"}
            value={shown.password}
            onChange={(e) => updateParts({ password: e.target.value })}
            placeholder={t("settings.globalProxy.password", {
              defaultValue: "密码（可选）",
            })}
            aria-label={t("settings.globalProxy.password", {
              defaultValue: "密码（可选）",
            })}
            autoComplete="new-password"
            disabled={globalEnabled}
            className={cn("font-mono text-sm pr-10", disabledClass)}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
            onClick={() => setShowPassword((v) => !v)}
            disabled={globalEnabled}
            tabIndex={-1}
          >
            {showPassword && !globalEnabled ? (
              <EyeOff className="h-4 w-4 text-fg-2" />
            ) : (
              <Eye className="h-4 w-4 text-fg-2" />
            )}
          </Button>
        </div>
      </div>
      {globalEnabled ? (
        <p className="text-xs text-fg-3">
          {t("providerForm.proxyUrlGlobalEnabled", {
            defaultValue:
              "已启用全局代理，此项不生效。如需为该供应商单独设置代理，请先在设置中清除全局代理。",
          })}
        </p>
      ) : (
        !valid && (
          <p className="text-xs text-destructive">
            {t("providerForm.proxyUrlInvalid", {
              defaultValue:
                "代理地址格式无效，支持 http、https、socks5、socks5h。",
            })}
          </p>
        )
      )}
    </div>
  );
}
