import { useState, useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { DeepLinkImportRequest, deeplinkApi } from "@/lib/api/deeplink";
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

  useEffect(() => {
    // Listen for deep link import events
    const unlistenImport = listen<DeepLinkImportRequest>(
      "deeplink-import",
      async (event) => {
        console.log("Deep link import event received:", event.payload);

        // If config is present, merge it to get the complete configuration
        if (event.payload.config || event.payload.configUrl) {
          try {
            const mergedRequest = await deeplinkApi.mergeDeeplinkConfig(
              event.payload,
            );
            console.log("Config merged successfully:", mergedRequest);
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
      await deeplinkApi.importFromDeeplink(request);

      // Invalidate provider queries to refresh the list
      await queryClient.invalidateQueries({
        queryKey: ["providers", request.app],
      });

      toast.success(t("deeplink.importSuccess"), {
        description: t("deeplink.importSuccessDescription", {
          name: request.name,
        }),
      });

      setIsOpen(false);
    } catch (error) {
      console.error("Failed to import provider from deep link:", error);
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

  // Parse config file content for display
  interface ParsedConfig {
    type: "claude" | "codex" | "gemini";
    env?: Record<string, string>;
    auth?: Record<string, string>;
    tomlConfig?: string;
    raw: Record<string, unknown>;
  }

  // Helper to decode base64 with UTF-8 support
  const b64ToUtf8 = (str: string): string => {
    try {
      const binString = atob(str);
      const bytes = Uint8Array.from(binString, (m) => m.codePointAt(0) || 0);
      return new TextDecoder().decode(bytes);
    } catch (e) {
      console.error("Failed to decode base64:", e);
      return atob(str);
    }
  };

  const parsedConfig = useMemo((): ParsedConfig | null => {
    if (!request?.config) return null;
    try {
      const decoded = b64ToUtf8(request.config);
      const parsed = JSON.parse(decoded) as Record<string, unknown>;

      if (request.app === "claude") {
        // Claude 格式: { env: { ANTHROPIC_AUTH_TOKEN: ..., ... } }
        return {
          type: "claude",
          env: (parsed.env as Record<string, string>) || {},
          raw: parsed,
        };
      } else if (request.app === "codex") {
        // Codex 格式: { auth: { OPENAI_API_KEY: ... }, config: "TOML string" }
        return {
          type: "codex",
          auth: (parsed.auth as Record<string, string>) || {},
          tomlConfig: (parsed.config as string) || "",
          raw: parsed,
        };
      } else if (request.app === "gemini") {
        // Gemini 格式: 扁平结构 { GEMINI_API_KEY: ..., GEMINI_BASE_URL: ... }
        return {
          type: "gemini",
          env: parsed as Record<string, string>,
          raw: parsed,
        };
      }
      return null;
    } catch (e) {
      console.error("Failed to parse config:", e);
      return null;
    }
  }, [request?.config, request?.app]);

  // Helper to mask sensitive values
  const maskValue = (key: string, value: string): string => {
    const sensitiveKeys = ["TOKEN", "KEY", "SECRET", "PASSWORD"];
    const isSensitive = sensitiveKeys.some((k) =>
      key.toUpperCase().includes(k),
    );
    if (isSensitive && value.length > 8) {
      return `${value.substring(0, 8)}${"*".repeat(12)}`;
    }
    return value;
  };

  return (
    <Dialog open={isOpen && !!request} onOpenChange={setIsOpen}>
      <DialogContent className="sm:max-w-[500px]" zIndex="top">
        {request && (
          <>
            {/* 标题显式左对齐，避免默认居中样式影响 */}
            <DialogHeader className="text-left sm:text-left">
              <DialogTitle>{t("deeplink.confirmImport")}</DialogTitle>
              <DialogDescription>
                {t("deeplink.confirmImportDescription")}
              </DialogDescription>
            </DialogHeader>

            {/* 主体内容整体右移，略大于标题内边距，让内容看起来不贴边 */}
            <div className="space-y-4 px-8 py-4 max-h-[60vh] overflow-y-auto [scrollbar-width:thin] [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar]:block [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-gray-200 dark:[&::-webkit-scrollbar-thumb]:bg-gray-700">
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
              <div className="grid grid-cols-3 items-center gap-4">
                <div className="font-medium text-sm text-muted-foreground">
                  {t("deeplink.endpoint")}
                </div>
                <div className="col-span-2 text-sm break-all">
                  {request.endpoint}
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
                      {parsedConfig.type === "claude" && parsedConfig.env && (
                        <div className="space-y-1.5">
                          {Object.entries(parsedConfig.env).map(
                            ([key, value]) => (
                              <div
                                key={key}
                                className="grid grid-cols-2 gap-2 text-xs"
                              >
                                <span className="font-mono text-muted-foreground truncate">
                                  {key}
                                </span>
                                <span className="font-mono truncate">
                                  {maskValue(key, String(value))}
                                </span>
                              </div>
                            ),
                          )}
                        </div>
                      )}

                      {/* Codex config */}
                      {parsedConfig.type === "codex" && (
                        <div className="space-y-2">
                          {parsedConfig.auth &&
                            Object.keys(parsedConfig.auth).length > 0 && (
                              <div className="space-y-1.5">
                                <div className="text-xs text-muted-foreground">
                                  Auth:
                                </div>
                                {Object.entries(parsedConfig.auth).map(
                                  ([key, value]) => (
                                    <div
                                      key={key}
                                      className="grid grid-cols-2 gap-2 text-xs pl-2"
                                    >
                                      <span className="font-mono text-muted-foreground truncate">
                                        {key}
                                      </span>
                                      <span className="font-mono truncate">
                                        {maskValue(key, String(value))}
                                      </span>
                                    </div>
                                  ),
                                )}
                              </div>
                            )}
                          {parsedConfig.tomlConfig && (
                            <div className="space-y-1">
                              <div className="text-xs text-muted-foreground">
                                TOML Config:
                              </div>
                              <pre className="text-xs font-mono bg-background p-2 rounded overflow-x-auto max-h-24 whitespace-pre-wrap">
                                {parsedConfig.tomlConfig.substring(0, 300)}
                                {parsedConfig.tomlConfig.length > 300 && "..."}
                              </pre>
                            </div>
                          )}
                        </div>
                      )}

                      {/* Gemini config */}
                      {parsedConfig.type === "gemini" && parsedConfig.env && (
                        <div className="space-y-1.5">
                          {Object.entries(parsedConfig.env).map(
                            ([key, value]) => (
                              <div
                                key={key}
                                className="grid grid-cols-2 gap-2 text-xs"
                              >
                                <span className="font-mono text-muted-foreground truncate">
                                  {key}
                                </span>
                                <span className="font-mono truncate">
                                  {maskValue(key, String(value))}
                                </span>
                              </div>
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

              {/* Warning */}
              <div className="rounded-lg bg-yellow-50 dark:bg-yellow-900/20 p-3 text-sm text-yellow-800 dark:text-yellow-200">
                {t("deeplink.warning")}
              </div>
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
