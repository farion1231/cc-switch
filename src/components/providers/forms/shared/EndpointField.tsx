import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Zap, AlertTriangle, Server, Unplug } from "lucide-react";
import type { ClaudeApiFormat } from "@/types";

// 检测 URL 是否以 API 路径结尾的模式（只检测结尾，不包含 /v1 因为它是正常的 base URL 后缀）
const API_PATH_SUFFIX_PATTERNS = [
  // Claude 完整路径
  "/v1/messages",
  "/messages",
  // OpenAI Chat Completions 完整路径
  "/v1/chat/completions",
  "/chat/completions",
  // Codex Responses 完整路径
  "/v1/responses",
  "/responses",
  // Gemini 完整路径
  "/v1beta/models",
];

// 从 URL 中提取路径部分（移除查询参数和尾部斜杠）
function extractUrlPath(url: string): string {
  try {
    // 移除查询参数
    const pathPart = url.split("?")[0];
    // 移除尾部斜杠并转为小写
    return pathPart.replace(/\/+$/, "").toLowerCase();
  } catch {
    return url.toLowerCase();
  }
}

// 构建 URL 并去重 /v1/v1
function buildUrl(base: string, suffix: string): string {
  const trimmedBase = base.trim().replace(/\/+$/, "");
  let url = `${trimmedBase}${suffix}`;
  // 去重 /v1/v1 模式
  while (url.includes("/v1/v1")) {
    url = url.replace("/v1/v1", "/v1");
  }
  return url;
}

type AppType = "claude" | "codex" | "gemini";
type CodexApiFormat = "responses" | "chat";

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
  // 新增：应用类型和 API 格式
  appType?: AppType;
  apiFormat?: ClaudeApiFormat | CodexApiFormat;
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

  const defaultManageLabel = t("providerForm.manageAndTest", {
    defaultValue: "管理和测速",
  });

  // 根据 appType 和 apiFormat 计算直连和代理后缀
  const suffixes = useMemo(() => {
    if (!appType) return null;

    if (appType === "claude") {
      // Claude: 直连固定 /v1/messages，代理根据 apiFormat 决定
      return {
        direct: "/v1/messages",
        proxy:
          apiFormat === "openai_chat" ? "/v1/chat/completions" : "/v1/messages",
      };
    }

    if (appType === "codex") {
      // Codex: 直连固定 /responses（base URL 已含 /v1），代理根据 apiFormat 决定
      return {
        direct: "/responses",
        proxy: apiFormat === "chat" ? "/chat/completions" : "/responses",
      };
    }

    if (appType === "gemini") {
      // Gemini: 两种模式相同
      return {
        direct: "/v1beta/models",
        proxy: "/v1beta/models",
      };
    }

    return null;
  }, [appType, apiFormat]);

  // 检测 URL 是否已以 API 路径结尾（全链接）
  const urlEndsWithApiPath = useMemo(() => {
    if (!value) return false;
    const urlPath = extractUrlPath(value);
    return API_PATH_SUFFIX_PATTERNS.some((pattern) =>
      urlPath.endsWith(pattern.toLowerCase()),
    );
  }, [value]);

  // 构建直连模式请求 URL
  const directUrlPreview = useMemo(() => {
    if (!value || !suffixes) return null;
    if (urlEndsWithApiPath) return value;
    return buildUrl(value, suffixes.direct);
  }, [value, suffixes, urlEndsWithApiPath]);

  // 构建代理模式请求 URL
  const proxyUrlPreview = useMemo(() => {
    if (!value || !suffixes) return null;
    if (urlEndsWithApiPath) return value;
    return buildUrl(value, suffixes.proxy);
  }, [value, suffixes, urlEndsWithApiPath]);

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
      {showUrlPreview && directUrlPreview && (
        <div className="p-2 bg-muted/50 border border-border rounded-md space-y-2">
          {/* 直连模式请求地址 */}
          <div>
            <p className="text-xs text-muted-foreground mb-0.5 flex items-center gap-1">
              <Unplug className="h-3 w-3" />
              {t("providerForm.directRequestUrl", {
                defaultValue: "直连请求地址：",
              })}
            </p>
            <p className="text-xs font-mono text-foreground break-all pl-4">
              {directUrlPreview}
            </p>
            <p className="text-xs text-muted-foreground/70 mt-0.5 pl-4">
              {t("providerForm.directRequestUrlDesc", {
                defaultValue: "不开启代理时，客户端直接请求此地址",
              })}
            </p>
          </div>

          {/* 代理模式请求地址 */}
          {proxyUrlPreview && (
            <div className="pt-1.5 border-t border-border/50">
              <p className="text-xs text-muted-foreground mb-0.5 flex items-center gap-1">
                <Server className="h-3 w-3" />
                {t("providerForm.proxyRequestUrl", {
                  defaultValue: "代理请求地址：",
                })}
              </p>
              <p className="text-xs font-mono text-foreground break-all pl-4">
                {proxyUrlPreview}
              </p>
              <p className="text-xs text-muted-foreground/70 mt-0.5 pl-4">
                {t("providerForm.proxyRequestUrlDesc", {
                  defaultValue:
                    "开启代理后，代理服务会将请求转发到此地址（支持格式转换）",
                })}
              </p>
            </div>
          )}
        </div>
      )}

      {/* 全链接警告 */}
      {urlEndsWithApiPath && (
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
