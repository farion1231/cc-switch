import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Zap, AlertTriangle, Server, Unplug } from "lucide-react";
import { proxyApi, type UrlPreview } from "@/lib/api/proxy";

type AppType = "claude" | "codex" | "gemini";

interface EndpointFieldProps {
  id: string;
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  hint?: string;
  showManageButton?: boolean;
  onManageClick?: () => void;
  manageButtonLabel?: string;
  // 应用类型和 API 格式
  appType?: AppType;
  apiFormat?: string;
  // 是否显示请求地址预览
  showUrlPreview?: boolean;
}

export function EndpointField({
  id,
  label,
  value,
  onChange,
  placeholder,
  hint,
  showManageButton = true,
  onManageClick,
  manageButtonLabel,
  appType,
  apiFormat,
  showUrlPreview = true,
}: EndpointFieldProps) {
  const { t } = useTranslation();
  const [urlPreview, setUrlPreview] = useState<UrlPreview | null>(null);
  const lastRequestIdRef = useRef(0);

  const defaultManageLabel = t("providerForm.manageAndTest", {
    defaultValue: "管理和测速",
  });

  // 调用后端 API 获取 URL 预览
  useEffect(() => {
    if (!value || !appType || !showUrlPreview) {
      setUrlPreview(null);
      return;
    }

    // 防抖：延迟 300ms 后请求
    const timer = setTimeout(async () => {
      const requestId = ++lastRequestIdRef.current;
      try {
        const preview = await proxyApi.buildUrlPreview(
          appType,
          value,
          apiFormat,
        );
        if (requestId !== lastRequestIdRef.current) return;
        setUrlPreview(preview);
      } catch (error) {
        console.error("Failed to build URL preview:", error);
        if (requestId !== lastRequestIdRef.current) return;
        setUrlPreview(null);
      }
    }, 300);

    return () => clearTimeout(timer);
  }, [value, appType, apiFormat, showUrlPreview]);

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <FormLabel htmlFor={id}>{label}</FormLabel>
        {showManageButton && onManageClick && (
          <button
            type="button"
            onClick={onManageClick}
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
          >
            <Zap className="h-3.5 w-3.5" />
            {manageButtonLabel || defaultManageLabel}
          </button>
        )}
      </div>
      <Input
        id={id}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        autoComplete="off"
      />

      {/* 请求地址预览 */}
      {showUrlPreview && urlPreview && (
        <div className="p-2 bg-muted/50 border border-border rounded-md space-y-2">
          {/* CLI 直连请求地址 */}
          <div>
            <p className="text-xs text-muted-foreground mb-0.5 flex items-center gap-1">
              <Unplug className="h-3 w-3" />
              {t("providerForm.directRequestUrl", {
                defaultValue: "CLI 直连请求地址：",
              })}
            </p>
            <p className="text-xs font-mono text-foreground break-all pl-4">
              {urlPreview.direct_url}
            </p>
            <p className="text-xs text-muted-foreground/70 mt-0.5 pl-4">
              {t("providerForm.directRequestUrlDesc", {
                defaultValue: "CLI 硬拼接默认后缀后的实际请求地址",
              })}
            </p>
          </div>

          {/* CCS 代理请求地址 */}
          <div className="pt-1.5 border-t border-border/50">
            <p className="text-xs text-muted-foreground mb-0.5 flex items-center gap-1">
              <Server className="h-3 w-3" />
              {t("providerForm.proxyRequestUrl", {
                defaultValue: "CCS 代理请求地址：",
              })}
            </p>
            <p className="text-xs font-mono text-foreground break-all pl-4">
              {urlPreview.proxy_url}
            </p>
            <p className="text-xs text-muted-foreground/70 mt-0.5 pl-4">
              {t("providerForm.proxyRequestUrlDesc", {
                defaultValue: "CCS 智能拼接后转发到上游的地址",
              })}
            </p>
          </div>
        </div>
      )}

      {/* 全链接警告 */}
      {urlPreview?.is_full_url && (
        <div className="flex items-start gap-2 p-2 bg-orange-50 dark:bg-orange-950/30 border border-orange-200 dark:border-orange-800 rounded-md">
          <AlertTriangle className="h-4 w-4 text-orange-500 mt-0.5 flex-shrink-0" />
          <div className="flex-1">
            <p className="text-xs text-orange-600 dark:text-orange-400 font-medium">
              {t("providerForm.fullUrlWarningTitle", {
                defaultValue: "检测到完整 API 路径",
              })}
            </p>
            <p className="text-xs text-orange-600/80 dark:text-orange-400/80 mt-0.5">
              {t("providerForm.fullUrlWarning", {
                defaultValue:
                  "填写了包含 API 路径的完整地址，此配置仅在代理模式下生效。直连模式下请只填写基础地址。",
              })}
            </p>
          </div>
        </div>
      )}

      {hint ? (
        <div className="p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg">
          <p className="text-xs text-amber-600 dark:text-amber-400">{hint}</p>
        </div>
      ) : null}
    </div>
  );
}
