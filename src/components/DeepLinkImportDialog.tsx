import { useState, useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { DeepLinkImportRequest, deeplinkApi } from "@/lib/api/deeplink";
import { parseDeepLinkConfigPreview } from "@/utils/deepLinkConfigPreview";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { PromptConfirmation } from "./deeplink/PromptConfirmation";
import { McpConfirmation } from "./deeplink/McpConfirmation";
import { SkillConfirmation } from "./deeplink/SkillConfirmation";
import { ProviderIcon } from "./ProviderIcon";
import {
  classifyEndpoint,
  classifyEnvKey,
  decodeDeeplinkPayload,
  maskValue,
  riskI18nKey,
} from "@/utils/deeplinkRisk";
import { decodeBase64Utf8 } from "@/lib/utils/base64";

interface DeeplinkError {
  url: string;
  error: string;
}

export function DeepLinkImportDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [request, setRequest] = useState<DeepLinkImportRequest | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [isOpen, setIsOpen] = useState(false);

  // 容错判断：MCP 导入结果可能缺少 type 字段
  const isMcpImportResult = (
    value: unknown,
  ): value is {
    importedCount: number;
    importedIds: string[];
    failed: Array<{ id: string; error: string }>;
    type?: "mcp";
  } => {
    if (!value || typeof value !== "object") return false;
    const v = value as Record<string, unknown>;
    return (
      typeof v.importedCount === "number" &&
      Array.isArray(v.importedIds) &&
      Array.isArray(v.failed)
    );
  };

  useEffect(() => {
    // Listen for deep link import events
    const unlistenImport = listen<DeepLinkImportRequest>(
      "deeplink-import",
      async (event) => {
        // If config is present, merge it to get the complete configuration
        if (event.payload.config || event.payload.configUrl) {
          try {
            const mergedRequest = await deeplinkApi.mergeDeeplinkConfig(
              event.payload,
            );
            setRequest(mergedRequest);
          } catch (error) {
            console.error("Failed to merge config:", error);
            toast.error(t("deeplink.configMergeError"), {
              description:
                error instanceof Error ? error.message : String(error),
            });
            // Fall back to original request
            setRequest(event.payload);
          }
        } else {
          setRequest(event.payload);
        }

        setIsOpen(true);
      },
    );

    // Listen for deep link error events
    const unlistenError = listen<DeeplinkError>("deeplink-error", (event) => {
      console.error("Deep link error:", event.payload);
      toast.error(t("deeplink.parseError"), {
        description: event.payload.error,
      });
    });

    return () => {
      unlistenImport.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, [t]);

  const handleImport = async () => {
    if (!request) return;

    setIsImporting(true);

    try {
      const result = await deeplinkApi.importFromDeeplink(request);
      const refreshMcp = async (summary: {
        importedCount: number;
        importedIds: string[];
        failed: Array<{ id: string; error: string }>;
      }) => {
        // 强制刷新 MCP 相关缓存，确保管理页重新从数据库加载
        await queryClient.invalidateQueries({
          queryKey: ["mcp", "all"],
          refetchType: "all",
        });
        await queryClient.refetchQueries({
          queryKey: ["mcp", "all"],
          type: "all",
        });

        if (summary.failed.length > 0) {
          toast.warning(t("deeplink.mcpPartialSuccess"), {
            description: t("deeplink.mcpPartialSuccessDescription", {
              success: summary.importedCount,
              failed: summary.failed.length,
            }),
          });
        } else {
          toast.success(t("deeplink.mcpImportSuccess"), {
            description: t("deeplink.mcpImportSuccessDescription", {
              count: summary.importedCount,
            }),
            closeButton: true,
          });
        }
      };

      // Handle different result types
      if ("type" in result) {
        if (result.type === "provider") {
          await queryClient.invalidateQueries({
            queryKey: ["providers", request.app],
          });
          toast.success(t("deeplink.importSuccess"), {
            description: t("deeplink.importSuccessDescription", {
              name: request.name,
            }),
            closeButton: true,
          });
        } else if (result.type === "prompt") {
          // Prompts don't use React Query, trigger a custom event for refresh
          window.dispatchEvent(
            new CustomEvent("prompt-imported", {
              detail: { app: request.app },
            }),
          );
          toast.success(t("deeplink.promptImportSuccess"), {
            description: t("deeplink.promptImportSuccessDescription", {
              name: request.name,
            }),
            closeButton: true,
          });
        } else if (result.type === "mcp") {
          await refreshMcp(result);
        } else if (result.type === "skill") {
          // Refresh Skills with aggressive strategy
          queryClient.invalidateQueries({
            queryKey: ["skills"],
            refetchType: "all",
          });
          await queryClient.refetchQueries({
            queryKey: ["skills"],
            type: "all",
          });
          toast.success(t("deeplink.skillImportSuccess"), {
            description: t("deeplink.skillImportSuccessDescription", {
              repo: request.repo,
            }),
            closeButton: true,
          });
        }
      } else if (isMcpImportResult(result)) {
        // 兜底处理：旧版本后端可能未返回 type 字段
        await refreshMcp(result);
      } else {
        // Legacy return type (string ID) - assume provider
        await queryClient.invalidateQueries({
          queryKey: ["providers", request.app],
        });
        toast.success(t("deeplink.importSuccess"), {
          description: t("deeplink.importSuccessDescription", {
            name: request.name,
          }),
          closeButton: true,
        });
      }

      // Close dialog after all refreshes complete
      setIsOpen(false);
    } catch (error) {
      console.error("Failed to import from deep link:", error);
      toast.error(t("deeplink.importError"), {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setIsImporting(false);
    }
  };

  const handleCancel = () => {
    setIsOpen(false);
  };

  // Mask API key for display (show first 4 chars + ***)
  const maskedApiKey =
    request?.apiKey && request.apiKey.length > 4
      ? `${request.apiKey.substring(0, 4)}${"*".repeat(20)}`
      : "****";

  // Check if config file is present
  const hasConfigFile = !!(request?.config || request?.configUrl);
  const configSource = request?.config
    ? "base64"
    : request?.configUrl
      ? "url"
      : null;

  const parsedConfig = useMemo(
    () => (request ? parseDeepLinkConfigPreview(request) : null),
    [request],
  );

  /**
   * env 行：值经 `maskValue` 脱敏，键命中加载器控制变量时标记。
   *
   * `break-all` 而非 `truncate`——被截断的值等于没展示。
   */
  const EnvRow = ({ envKey, value }: { envKey: string; value: string }) => {
    const risk = classifyEnvKey(envKey);
    return (
      <div className="grid grid-cols-2 gap-2 text-xs">
        <span
          className={`font-mono break-all ${
            risk
              ? "text-yellow-700 dark:text-yellow-500 font-semibold"
              : "text-muted-foreground"
          }`}
        >
          {risk && <span aria-hidden="true">⚠ </span>}
          {envKey}
        </span>
        <span className="font-mono break-all">{maskValue(envKey, value)}</span>
      </div>
    );
  };

  const getTitle = () => {
    if (!request) return t("deeplink.confirmImport");
    switch (request.resource) {
      case "prompt":
        return t("deeplink.importPrompt");
      case "mcp":
        return t("deeplink.importMcp");
      case "skill":
        return t("deeplink.importSkill");
      default:
        return t("deeplink.confirmImport");
    }
  };

  const getDescription = () => {
    if (!request) return t("deeplink.confirmImportDescription");
    switch (request.resource) {
      case "prompt":
        return t("deeplink.importPromptDescription");
      case "mcp":
        return t("deeplink.importMcpDescription");
      case "skill":
        return t("deeplink.importSkillDescription");
      default:
        return t("deeplink.confirmImportDescription");
    }
  };

  return (
    <Dialog open={isOpen && !!request} onOpenChange={setIsOpen}>
      <DialogContent className="sm:max-w-[500px]" zIndex="top">
        {request && (
          <>
            {/* 标题显式左对齐，避免默认居中样式影响 */}
            <DialogHeader className="text-left sm:text-left">
              <DialogTitle>{getTitle()}</DialogTitle>
              <DialogDescription>{getDescription()}</DialogDescription>
            </DialogHeader>

            {/* 主体内容整体右移，略大于标题内边距，让内容看起来不贴边 */}
            <div className="space-y-4 px-8 py-4 max-h-[60vh] overflow-y-auto [scrollbar-width:thin] [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar]:block [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-gray-200 dark:[&::-webkit-scrollbar-thumb]:bg-gray-700">
              {request.resource === "prompt" && (
                <PromptConfirmation request={request} />
              )}
              {request.resource === "mcp" && (
                <McpConfirmation request={request} />
              )}
              {request.resource === "skill" && (
                <SkillConfirmation request={request} />
              )}

              {/* Legacy Provider View */}
              {(request.resource === "provider" || !request.resource) && (
                <>
                  {/* Provider Icon - enlarge and center near the top */}
                  {request.icon && (
                    <div className="flex justify-center pt-2 pb-1">
                      <ProviderIcon
                        icon={request.icon}
                        name={request.name || request.icon}
                        size={80}
                        className="drop-shadow-sm"
                      />
                    </div>
                  )}

                  {/* App Type */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.app")}
                    </div>
                    <div className="col-span-2 text-sm font-medium capitalize">
                      {request.app}
                    </div>
                  </div>

                  {/* Provider Name */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.providerName")}
                    </div>
                    <div className="col-span-2 text-sm font-medium">
                      {request.name}
                    </div>
                  </div>

                  {/* Homepage */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.homepage")}
                    </div>
                    <div className="col-span-2 text-sm break-all text-blue-600 dark:text-blue-400">
                      {request.homepage}
                    </div>
                  </div>

                  {/* API Endpoint */}
                  <div className="grid grid-cols-3 items-start gap-4">
                    <div className="font-medium text-sm text-muted-foreground pt-0.5">
                      {t("deeplink.endpoint")}
                    </div>
                    <div className="col-span-2 text-sm break-all space-y-1">
                      {request.endpoint?.split(",").map((ep, idx) => {
                        const endpointRisk = classifyEndpoint(ep.trim());
                        return (
                          <div
                            key={idx}
                            className={
                              endpointRisk
                                ? "text-yellow-700 dark:text-yellow-500 font-semibold"
                                : idx === 0
                                  ? "font-medium"
                                  : "text-muted-foreground"
                            }
                          >
                            {idx === 0 ? "🔹 " : "└ "}
                            {endpointRisk && (
                              <span aria-hidden="true">⚠ </span>
                            )}
                            {ep.trim()}
                            {idx === 0 && request.endpoint?.includes(",") && (
                              <span className="text-xs text-muted-foreground ml-2">
                                ({t("deeplink.primaryEndpoint")})
                              </span>
                            )}
                            {endpointRisk && (
                              <div className="text-xs font-normal mt-0.5">
                                {t(riskI18nKey(endpointRisk))}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>

                  {/* API Key (masked) */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-muted-foreground">
                      {t("deeplink.apiKey")}
                    </div>
                    <div className="col-span-2 text-sm font-mono text-muted-foreground">
                      {maskedApiKey}
                    </div>
                  </div>

                  {/* Model Fields - 根据应用类型显示不同的模型字段 */}
                  {request.app === "claude" ? (
                    <>
                      {/* Claude 四种模型字段 */}
                      {request.haikuModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.haikuModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.haikuModel}
                          </div>
                        </div>
                      )}
                      {request.sonnetModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.sonnetModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.sonnetModel}
                          </div>
                        </div>
                      )}
                      {request.opusModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.opusModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.opusModel}
                          </div>
                        </div>
                      )}
                      {request.model && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.multiModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.model}
                          </div>
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      {/* Codex 和 Gemini 使用通用 model 字段 */}
                      {request.model && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.model")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.model}
                          </div>
                        </div>
                      )}
                    </>
                  )}

                  {/* Notes (if present) */}
                  {request.notes && (
                    <div className="grid grid-cols-3 items-start gap-4">
                      <div className="font-medium text-sm text-muted-foreground">
                        {t("deeplink.notes")}
                      </div>
                      <div className="col-span-2 text-sm text-muted-foreground">
                        {request.notes}
                      </div>
                    </div>
                  )}

                  {/* Config File Details (v3.8+) */}
                  {hasConfigFile && (
                    <div className="space-y-3 pt-2 border-t border-border-default">
                      <div className="grid grid-cols-3 items-center gap-4">
                        <div className="font-medium text-sm text-muted-foreground">
                          {t("deeplink.configSource")}
                        </div>
                        <div className="col-span-2 text-sm">
                          <span className="inline-flex items-center px-2 py-0.5 rounded-md bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs font-medium">
                            {configSource === "base64"
                              ? t("deeplink.configEmbedded")
                              : t("deeplink.configRemote")}
                          </span>
                          {request.configFormat && (
                            <span className="ml-2 text-xs text-muted-foreground uppercase">
                              {request.configFormat}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* Parsed Config Details */}
                      {parsedConfig && (
                        <div className="rounded-lg bg-muted/50 p-3 space-y-2">
                          <div className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                            {t("deeplink.configDetails")}
                          </div>

                          {/* Claude config */}
                          {parsedConfig.type === "claude" &&
                            parsedConfig.env && (
                              <div className="space-y-1.5">
                                {Object.entries(parsedConfig.env).map(
                                  ([key, value]) => (
                                    <EnvRow
                                      key={key}
                                      envKey={key}
                                      value={String(value)}
                                    />
                                  ),
                                )}
                              </div>
                            )}

                          {/* Codex config */}
                          {(parsedConfig.type === "codex" ||
                            parsedConfig.type === "grokbuild") && (
                            <div className="space-y-2">
                              {parsedConfig.type === "codex" &&
                                parsedConfig.auth &&
                                Object.keys(parsedConfig.auth).length > 0 && (
                                  <div className="space-y-1.5">
                                    <div className="text-xs text-muted-foreground">
                                      Auth:
                                    </div>
                                    <div className="pl-2 space-y-1.5">
                                      {Object.entries(parsedConfig.auth).map(
                                        ([key, value]) => (
                                          <EnvRow
                                            key={key}
                                            envKey={key}
                                            value={String(value)}
                                          />
                                        ),
                                      )}
                                    </div>
                                  </div>
                                )}
                              {parsedConfig.tomlConfig && (
                                <div className="space-y-1">
                                  <div className="text-xs text-muted-foreground">
                                    TOML Config:
                                  </div>
                                  <pre className="text-xs font-mono bg-background p-2 rounded overflow-auto max-h-24 whitespace-pre-wrap break-all">
                                    {parsedConfig.tomlConfig}
                                  </pre>
                                </div>
                              )}
                            </div>
                          )}

                          {/* Gemini config */}
                          {parsedConfig.type === "gemini" &&
                            parsedConfig.env && (
                              <div className="space-y-1.5">
                                {Object.entries(parsedConfig.env).map(
                                  ([key, value]) => (
                                    <EnvRow
                                      key={key}
                                      envKey={key}
                                      value={String(value)}
                                    />
                                  ),
                                )}
                              </div>
                            )}
                        </div>
                      )}

                      {/* Config URL (if remote) */}
                      {request.configUrl && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.configUrl")}
                          </div>
                          <div className="col-span-2 text-sm font-mono text-muted-foreground break-all">
                            {request.configUrl}
                          </div>
                        </div>
                      )}
                    </div>
                  )}

                  {/*
                    Usage Script Configuration (v3.9+)
                    区块门槛必须与后端 `build_provider_meta` 的持久化条件对齐：
                    任一 usage 字段存在即落库。若只按 usageScript 开门，一条仅携带
                    usageAccessToken/usageUserId 等字段的链接会在对话框毫无展示的
                    情况下把凭据写进供应商配置——展示是用户同意机制的承重部分。
                  */}
                  {(request.usageScript ||
                    request.usageEnabled !== undefined ||
                    request.usageApiKey ||
                    request.usageBaseUrl ||
                    request.usageAccessToken ||
                    request.usageUserId ||
                    request.usageAutoInterval !== undefined) && (
                    <div className="space-y-3 pt-2 border-t border-border-default">
                      {(request.usageScript ||
                        request.usageEnabled !== undefined) && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.usageScript", {
                              defaultValue: "用量查询",
                            })}
                          </div>
                          <div className="col-span-2 text-sm">
                            {/*
                            判据是 `=== true`，与后端 `usage_enabled.unwrap_or(false)`
                            严格对齐。此前用的 `!== false` 会把"链接没说"渲染成绿色的
                            「已启用」——徽章必须显示实际会发生的事，不能比后端更乐观。
                          */}
                            <span
                              className={`inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium ${
                                request.usageEnabled === true
                                  ? "bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300"
                                  : "bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400"
                              }`}
                            >
                              {request.usageEnabled === true
                                ? t("deeplink.usageScriptEnabled", {
                                    defaultValue: "已启用",
                                  })
                                : t("deeplink.usageScriptDisabled", {
                                    defaultValue: "未启用",
                                  })}
                            </span>
                          </div>
                        </div>
                      )}

                      {request.usageScript && (
                        <>
                          {/*
                        脚本正文必须完整展示。这段是会执行的 JavaScript，而 payload
                        常常整条藏在中间——`whitespace-pre-wrap break-all` + 可滚动容器，
                        不用 truncate，任何字符都不得被 CSS 藏起来。
                      */}
                          <div className="space-y-1">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageScriptCode")}
                            </div>
                            <pre className="max-h-48 overflow-auto rounded border border-border-default bg-muted/40 p-2 text-xs font-mono whitespace-pre-wrap break-all">
                              {decodeDeeplinkPayload(
                                request.usageScript,
                                decodeBase64Utf8,
                              )}
                            </pre>
                          </div>

                          {/*
                        无条件显示，不看 `usageEnabled`：代码无论启用与否都会被写入供应商
                        配置，用户之后在应用内一键即可开启。挂条件等于让攻击者省略参数就能
                        关掉这条警告。
                      */}
                          <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-start gap-2">
                            <span aria-hidden="true">⚠️</span>
                            <span>{t("deeplink.usageScriptWarning")}</span>
                          </div>
                        </>
                      )}

                      {/* Usage API Key (if different from provider) */}
                      {request.usageApiKey &&
                        request.usageApiKey !== request.apiKey && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageApiKey", {
                                defaultValue: "用量 API Key",
                              })}
                            </div>
                            <div className="col-span-2 text-sm font-mono text-muted-foreground">
                              {request.usageApiKey.length > 4
                                ? `${request.usageApiKey.substring(0, 4)}${"*".repeat(12)}`
                                : "****"}
                            </div>
                          </div>
                        )}

                      {/* Usage Base URL (if different from provider) */}
                      {request.usageBaseUrl &&
                        request.usageBaseUrl !== request.endpoint && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageBaseUrl", {
                                defaultValue: "用量查询地址",
                              })}
                            </div>
                            <div className="col-span-2 text-sm break-all">
                              {request.usageBaseUrl}
                            </div>
                          </div>
                        )}

                      {/* Usage Access Token (if present) */}
                      {request.usageAccessToken && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.usageAccessToken", {
                              defaultValue: "用量访问令牌",
                            })}
                          </div>
                          <div className="col-span-2 text-sm font-mono text-muted-foreground">
                            {request.usageAccessToken.length > 4
                              ? `${request.usageAccessToken.substring(0, 4)}${"*".repeat(12)}`
                              : "****"}
                          </div>
                        </div>
                      )}

                      {/* Usage User ID (if present) */}
                      {request.usageUserId && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-muted-foreground">
                            {t("deeplink.usageUserId", {
                              defaultValue: "用量用户 ID",
                            })}
                          </div>
                          <div className="col-span-2 text-sm font-mono break-all">
                            {request.usageUserId}
                          </div>
                        </div>
                      )}

                      {/* Auto Query Interval */}
                      {request.usageAutoInterval &&
                        request.usageAutoInterval > 0 && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-muted-foreground">
                              {t("deeplink.usageAutoInterval", {
                                defaultValue: "自动查询",
                              })}
                            </div>
                            <div className="col-span-2 text-sm">
                              {t("deeplink.usageAutoIntervalValue", {
                                defaultValue: "每 {{minutes}} 分钟",
                                minutes: request.usageAutoInterval,
                              })}
                            </div>
                          </div>
                        )}
                    </div>
                  )}

                  {/* Warning */}
                  <div className="rounded-lg bg-yellow-50 dark:bg-yellow-900/20 p-3 text-sm text-yellow-800 dark:text-yellow-200">
                    {t("deeplink.warning")}
                  </div>
                </>
              )}
            </div>

            <DialogFooter>
              <Button
                variant="outline"
                onClick={handleCancel}
                disabled={isImporting}
              >
                {t("common.cancel")}
              </Button>
              <Button onClick={handleImport} disabled={isImporting}>
                {isImporting ? t("deeplink.importing") : t("deeplink.import")}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
