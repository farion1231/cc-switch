import { useState, useEffect, useMemo, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  DeepLinkImportRequest,
  PendingDeepLink,
  deeplinkApi,
} from "@/lib/api/deeplink";
import { providersApi } from "@/lib/api";
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
import { toast } from "@/lib/toast";
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
import { Switch } from "@/components/ui/switch";
import { getAppLabel, isAdditiveAppId } from "@/config/appConfig";
import { normalizeIconDataUri } from "@/utils/customIcon";
import {
  CODING_PLAN_PROVIDERS,
  codingPlanEnabledByDefault,
  detectCodingPlanProvider,
} from "@/config/codingPlanProviders";

const API_FORMAT_LABELS: Record<
  NonNullable<DeepLinkImportRequest["apiFormat"]>,
  string
> = {
  anthropic: "Anthropic Messages",
  openai_chat: "OpenAI Chat Completions",
  openai_responses: "OpenAI Responses",
  gemini_native: "Gemini",
};

/**
 * 下载链接里的 `iconUrl`，按上传图标的流程重新编码成 96×96 PNG。
 * 任何一步失败都返回 undefined，确认框退回链接里的内置图标或首字母，不提示用户。
 */
async function loadLinkIcon(url: string): Promise<string | undefined> {
  try {
    const dataUri = await deeplinkApi.fetchDeeplinkIcon(url);
    return dataUri ? await normalizeIconDataUri(dataUri) : undefined;
  } catch (error) {
    console.warn("Failed to load the deep link icon", error);
    return undefined;
  }
}

/**
 * 「导入后立即生效」开关的初始值：链接里的 `enabled` 只是建议，导入时以开关为准。
 *
 * 累加式应用（OpenCode、OpenClaw、Hermes、Pi）的开关决定写不写进应用配置，旧链接没写时
 * 一直是写进去的，初始值保持打开；切换式应用和提示词默认关闭。
 */
function initialActivation(request: DeepLinkImportRequest): boolean {
  // Antigravity 的卡没有「启用」，只导入（后端同样忽略 enabled）
  if (request.resource === "provider" && request.app === "agy") return false;
  if (
    request.resource === "provider" &&
    request.app &&
    isAdditiveAppId(request.app)
  ) {
    return request.enabled ?? true;
  }
  return request.enabled === true;
}

export function DeepLinkImportDialog() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [request, setRequest] = useState<DeepLinkImportRequest | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [isOpen, setIsOpen] = useState(false);
  const [activateAfterImport, setActivateAfterImport] = useState(false);

  // 链接带 iconUrl 时，确认框一打开就下载图标并直接预览，不再单独询问；
  // 导入时等这次下载结束（最多几秒），拿到的图标随请求交给后端，后端保存时再完整校验
  const [linkIcon, setLinkIcon] = useState<string>();
  const linkIconTaskRef = useRef<Promise<string | undefined>>();
  useEffect(() => {
    setLinkIcon(undefined);
    const url = request?.resource === "provider" ? request.iconUrl : undefined;
    if (!url) {
      linkIconTaskRef.current = undefined;
      return;
    }
    let stale = false;
    linkIconTaskRef.current = loadLinkIcon(url).then((icon) => {
      if (!stale) setLinkIcon(icon);
      return icon;
    });
    return () => {
      stale = true;
    };
  }, [request]);

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

  // effect 只随挂载/卸载运行，不能依赖 t：启动时校正语言会换掉 t，effect 一重建，
  // 正在处理的链接就被丢下。翻译函数从 ref 读最新值。
  const tRef = useRef(t);
  useEffect(() => {
    tRef.current = t;
  }, [t]);
  // 已显示过的链接编号：同一条链接的重复通知只弹一次框
  const shownIdRef = useRef<number | null>(null);

  useEffect(() => {
    let disposed = false;
    // 串行处理：同一时间只有一次读取在进行，期间来的通知只记一笔，处理完再读一次。
    // 并发读取的话，迟到的旧结果会盖掉已经显示并清除的新链接，新链接就再也取不回来了
    let running = false;
    let rerun = false;

    // 返回是否真的显示了；被卸载打断时返回 false，链接留在后端等下次读取
    const showPending = async (pending: PendingDeepLink): Promise<boolean> => {
      if (pending.kind === "error") {
        if (disposed) return false;
        console.error("Deep link error:", pending);
        toast.error(tRef.current("deeplink.parseError"), {
          description: pending.error,
        });
        return true;
      }

      const incoming = pending.request;
      let next = incoming;
      // If config is present, merge it to get the complete configuration
      if (incoming.config) {
        try {
          next = await deeplinkApi.mergeDeeplinkConfig(incoming);
        } catch (error) {
          console.error("Failed to merge config:", error);
          toast.error(tRef.current("deeplink.configMergeError"), {
            description: error instanceof Error ? error.message : String(error),
          });
        }
      }
      if (disposed) return false;
      // 与请求一起初始化，避免弹窗先显示旧开关状态，再由 effect 更新。
      setActivateAfterImport(initialActivation(next));
      setRequest(next);
      setIsOpen(true);
      return true;
    };

    // 读取不清除，显示之后才按编号确认清除：读到后、显示前被打断，链接也不会丢
    const processOnce = async () => {
      try {
        const entry = await deeplinkApi.peekPendingDeeplink();
        if (!entry || disposed) return;
        if (entry.id === shownIdRef.current) {
          // 显示过但上次确认没送达
          await deeplinkApi.ackPendingDeeplink(entry.id);
          return;
        }
        if (!(await showPending(entry.link))) return;
        shownIdRef.current = entry.id;
        // 处理期间若来了新链接，后端编号已变，这次确认不会清掉它，下一轮读取会显示它
        await deeplinkApi.ackPendingDeeplink(entry.id);
      } catch (error) {
        console.error("Failed to process pending deep link:", error);
      }
    };

    const processPending = async () => {
      if (disposed) return;
      if (running) {
        rerun = true;
        return;
      }
      running = true;
      try {
        do {
          rerun = false;
          await processOnce();
        } while (rerun && !disposed);
      } finally {
        running = false;
      }
    };

    const unlisten = listen("deeplink-pending", () => {
      void processPending();
    });
    // 冷启动或窗口重建时，链接早于本组件挂载到达，挂载后主动读一次
    void unlisten.then(() => processPending());

    return () => {
      disposed = true;
      unlisten.then((fn) => fn());
    };
  }, []);

  // 托盘列出供应商和当前项，导入（以及打开开关时的切换）后要重建
  const refreshTrayMenu = async () => {
    try {
      await providersApi.updateTrayMenu();
    } catch (trayError) {
      console.error(
        "Failed to update tray menu after deep link import",
        trayError,
      );
    }
  };

  const handleImport = async () => {
    if (!request) return;

    setIsImporting(true);

    try {
      // 供应商和提示词的「立即生效」由用户在确认框里决定，覆盖链接里的值
      let payload: DeepLinkImportRequest =
        request.resource === "provider" || request.resource === "prompt"
          ? { ...request, enabled: activateAfterImport }
          : request;
      if (request.resource === "provider" && request.iconUrl) {
        const icon = await linkIconTaskRef.current;
        payload = {
          ...payload,
          icon: icon ?? request.icon,
          iconUrl: undefined,
        };
      }
      const result = await deeplinkApi.importFromDeeplink(payload);
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
          if (request.app === "opencode") {
            await queryClient.invalidateQueries({
              queryKey: ["opencodeLiveProviderIds"],
            });
            await queryClient.invalidateQueries({
              queryKey: ["opencode", "runtime-models"],
            });
          }
          await refreshTrayMenu();
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
        await refreshTrayMenu();
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

  // 主地址是套餐地址且链接没带脚本时，按套餐额度查询（与后端 `link_coding_plan_provider`
  // 一致：所有应用都认，只有 ZenMux 限 Claude）
  const detectedCodingPlan = detectCodingPlanProvider(
    request?.endpoint?.split(",")[0]?.trim(),
  );
  const codingPlan =
    !request?.usageScript &&
    detectedCodingPlan &&
    (detectedCodingPlan !== "zenmux" || request?.app === "claude")
      ? CODING_PLAN_PROVIDERS.find((cp) => cp.id === detectedCodingPlan)
      : undefined;

  // 导入后用量查询是否启用：链接写了 usageEnabled 就照办；没写时带脚本或是套餐地址即启用
  // （火山要另填 AccessKey，套餐地址也不启用；与后端一致）
  const usageWillBeEnabled =
    request?.usageEnabled ??
    (Boolean(request?.usageScript) ||
      (codingPlan ? codingPlanEnabledByDefault(codingPlan.id) : false));

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
            risk ? "text-warning-text font-semibold" : "text-fg-2"
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
            <div className="space-y-4 px-8 py-4 max-h-[60vh] overflow-y-auto [scrollbar-width:thin] [&::-webkit-scrollbar]:w-1.5 [&::-webkit-scrollbar]:block [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-subtle">
              {request.resource === "prompt" && (
                <PromptConfirmation
                  request={request}
                  enableAfterImport={activateAfterImport}
                  onEnableAfterImportChange={setActivateAfterImport}
                />
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
                  {(request.icon || request.iconUrl) && (
                    <div className="flex justify-center pt-2 pb-1">
                      {/* 下载中或下载失败时显示内置图标或首字母 */}
                      <ProviderIcon
                        icon={linkIcon ?? request.icon}
                        name={request.name || request.icon || ""}
                        size={80}
                        className="drop-shadow-sm"
                      />
                    </div>
                  )}

                  {/* App Type */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-fg-2">
                      {t("deeplink.app")}
                    </div>
                    <div className="col-span-2 text-sm font-medium capitalize">
                      {request.app}
                    </div>
                  </div>

                  {/* Provider Name */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-fg-2">
                      {t("deeplink.providerName")}
                    </div>
                    <div className="col-span-2 text-sm font-medium">
                      {request.name}
                    </div>
                  </div>

                  {/* Homepage */}
                  <div className="grid grid-cols-3 items-center gap-4">
                    <div className="font-medium text-sm text-fg-2">
                      {t("deeplink.homepage")}
                    </div>
                    <div className="col-span-2 text-sm break-all text-fg-1">
                      {request.homepage}
                    </div>
                  </div>

                  {/* API Endpoint */}
                  <div className="grid grid-cols-3 items-start gap-4">
                    <div className="font-medium text-sm text-fg-2 pt-0.5">
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
                                ? "text-warning-text font-semibold"
                                : idx === 0
                                  ? "font-medium"
                                  : "text-fg-2"
                            }
                          >
                            {idx === 0 ? "🔹 " : "└ "}
                            {endpointRisk && (
                              <span aria-hidden="true">⚠ </span>
                            )}
                            {ep.trim()}
                            {idx === 0 && request.endpoint?.includes(",") && (
                              <span className="text-xs text-fg-2 ml-2">
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
                    <div className="font-medium text-sm text-fg-2">
                      {t("deeplink.apiKey")}
                    </div>
                    <div className="col-span-2 text-sm font-mono text-fg-2">
                      {maskedApiKey}
                    </div>
                  </div>

                  {/* Model Fields - 根据应用类型显示不同的模型字段 */}
                  {request.app === "claude" ? (
                    <>
                      {/* Claude 四种模型字段 */}
                      {request.haikuModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.haikuModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.haikuModel}
                          </div>
                        </div>
                      )}
                      {request.sonnetModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.sonnetModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.sonnetModel}
                          </div>
                        </div>
                      )}
                      {request.opusModel && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.opusModel")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.opusModel}
                          </div>
                        </div>
                      )}
                      {request.model && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
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
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.model")}
                          </div>
                          <div className="col-span-2 text-sm font-mono">
                            {request.model}
                          </div>
                        </div>
                      )}
                    </>
                  )}

                  {request.apiFormat && (
                    <div className="grid grid-cols-3 items-center gap-4">
                      <div className="font-medium text-sm text-fg-2">
                        {t("deeplink.apiFormat")}
                      </div>
                      <div className="col-span-2 text-sm">
                        {API_FORMAT_LABELS[request.apiFormat] ??
                          request.apiFormat}
                      </div>
                    </div>
                  )}

                  {/* Notes (if present) */}
                  {request.notes && (
                    <div className="grid grid-cols-3 items-start gap-4">
                      <div className="font-medium text-sm text-fg-2">
                        {t("deeplink.notes")}
                      </div>
                      <div className="col-span-2 text-sm text-fg-2">
                        {request.notes}
                      </div>
                    </div>
                  )}

                  {/* Config File Details (v3.8+) */}
                  {hasConfigFile && (
                    <div className="space-y-3 pt-2 border-t border-border">
                      <div className="grid grid-cols-3 items-center gap-4">
                        <div className="font-medium text-sm text-fg-2">
                          {t("deeplink.configSource")}
                        </div>
                        <div className="col-span-2 text-sm">
                          <span className="inline-flex items-center px-2 py-0.5 rounded-md bg-subtle text-fg-1 text-xs font-medium">
                            {configSource === "base64"
                              ? t("deeplink.configEmbedded")
                              : t("deeplink.configRemote")}
                          </span>
                          {request.configFormat && (
                            <span className="ml-2 text-xs text-fg-2 uppercase">
                              {request.configFormat}
                            </span>
                          )}
                        </div>
                      </div>

                      {/* Parsed Config Details */}
                      {parsedConfig && (
                        <div className="rounded-lg bg-subtle p-3 space-y-2">
                          <div className="text-xs font-medium text-fg-2 uppercase tracking-wide">
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
                                    <div className="text-xs text-fg-2">
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
                                  <div className="text-xs text-fg-2">
                                    TOML Config:
                                  </div>
                                  <pre className="text-xs font-mono bg-surface p-2 rounded overflow-auto max-h-24 whitespace-pre-wrap break-all">
                                    {parsedConfig.tomlConfig}
                                  </pre>
                                  {/* 后端按白名单收（deeplink/codex_link.rs、grok_link.rs），原文不会整份导入 */}
                                  <p className="text-xs text-fg-2">
                                    {parsedConfig.type === "codex"
                                      ? t("deeplink.codexConfigScopeHint")
                                      : t("deeplink.grokConfigScopeHint")}
                                  </p>
                                </div>
                              )}
                            </div>
                          )}

                          {/* Gemini config */}
                          {(parsedConfig.type === "gemini" ||
                            parsedConfig.type === "agy") &&
                            parsedConfig.env && (
                              <div className="space-y-1.5">
                                {parsedConfig.model && (
                                  <EnvRow
                                    envKey="model"
                                    value={parsedConfig.model}
                                  />
                                )}
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
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.configUrl")}
                          </div>
                          <div className="col-span-2 text-sm font-mono text-fg-2 break-all">
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
                    codingPlan ||
                    request.usageEnabled !== undefined ||
                    request.usageApiKey ||
                    request.usageBaseUrl ||
                    request.usageAccessToken ||
                    request.usageUserId ||
                    request.usageAutoInterval !== undefined) && (
                    <div className="space-y-3 pt-2 border-t border-border">
                      {(request.usageScript ||
                        codingPlan ||
                        request.usageEnabled !== undefined) && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.usageScript", {
                              defaultValue: "用量查询",
                            })}
                          </div>
                          <div className="col-span-2 text-sm">
                            {/*
                            与后端 `build_provider_meta` 严格对齐：链接写了就照办，
                            没写时带脚本或是套餐地址即启用。徽章必须显示实际会发生
                            的事，不能比后端更乐观，也不能更保守。
                          */}
                            <span
                              className={`inline-flex items-center px-2 py-0.5 rounded-md text-xs font-medium ${
                                usageWillBeEnabled
                                  ? "bg-success-soft text-success-text"
                                  : "bg-subtle text-fg-2"
                              }`}
                            >
                              {usageWillBeEnabled
                                ? t("deeplink.usageScriptEnabled", {
                                    defaultValue: "已启用",
                                  })
                                : t("deeplink.usageScriptDisabled", {
                                    defaultValue: "未启用",
                                  })}
                            </span>
                            {codingPlan && (
                              <span className="ml-2 text-xs text-fg-2">
                                {t("deeplink.usageCodingPlan", {
                                  name: codingPlan.label,
                                  defaultValue: "套餐额度：{{name}}",
                                })}
                              </span>
                            )}
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
                            <div className="font-medium text-sm text-fg-2">
                              {t("deeplink.usageScriptCode")}
                            </div>
                            <pre className="max-h-48 overflow-auto rounded border border-border bg-subtle p-2 text-xs font-mono whitespace-pre-wrap break-all">
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
                          <div className="text-warning-text text-sm flex items-start gap-2">
                            <span aria-hidden="true">⚠️</span>
                            <span>{t("deeplink.usageScriptWarning")}</span>
                          </div>
                        </>
                      )}

                      {/* Usage API Key (if different from provider) */}
                      {request.usageApiKey &&
                        request.usageApiKey !== request.apiKey && (
                          <div className="grid grid-cols-3 items-center gap-4">
                            <div className="font-medium text-sm text-fg-2">
                              {t("deeplink.usageApiKey", {
                                defaultValue: "用量 API Key",
                              })}
                            </div>
                            <div className="col-span-2 text-sm font-mono text-fg-2">
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
                            <div className="font-medium text-sm text-fg-2">
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
                          <div className="font-medium text-sm text-fg-2">
                            {t("deeplink.usageAccessToken", {
                              defaultValue: "用量访问令牌",
                            })}
                          </div>
                          <div className="col-span-2 text-sm font-mono text-fg-2">
                            {request.usageAccessToken.length > 4
                              ? `${request.usageAccessToken.substring(0, 4)}${"*".repeat(12)}`
                              : "****"}
                          </div>
                        </div>
                      )}

                      {/* Usage User ID (if present) */}
                      {request.usageUserId && (
                        <div className="grid grid-cols-3 items-center gap-4">
                          <div className="font-medium text-sm text-fg-2">
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
                            <div className="font-medium text-sm text-fg-2">
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

                  {/* Antigravity 的卡没有「启用」，只导入，不给这个开关 */}
                  {request.app !== "agy" && (
                    <>
                      {/*
                        导入后是否立即生效由用户决定，链接里的 enabled 只是初始值。切换式应用打开后
                        会立即切到这家，客户端流量随之改走链接里的地址，所以打开时要给出提示。
                      */}
                      <div className="flex items-center justify-between gap-4 pt-2 border-t border-border">
                        <label
                          htmlFor="deeplink-activate-after-import"
                          className="text-sm font-medium"
                        >
                          {request.app && isAdditiveAppId(request.app)
                            ? request.app === "pi"
                              ? t("deeplink.enableInPiAfterImport")
                              : t("deeplink.addToAppAfterImport", {
                                  app: getAppLabel(request.app),
                                })
                            : t("deeplink.switchAfterImport")}
                        </label>
                        <Switch
                          id="deeplink-activate-after-import"
                          checked={activateAfterImport}
                          onCheckedChange={setActivateAfterImport}
                        />
                      </div>
                      {activateAfterImport &&
                        !(request.app && isAdditiveAppId(request.app)) && (
                          <div className="text-warning-text text-sm flex items-start gap-2">
                            <span aria-hidden="true">⚠️</span>
                            <span>{t("deeplink.providerEnabledWarning")}</span>
                          </div>
                        )}
                    </>
                  )}

                  {/* Warning */}
                  <div className="rounded-lg bg-warning-soft p-3 text-sm text-warning-text">
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
