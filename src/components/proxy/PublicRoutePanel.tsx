import { useEffect, useRef, useState } from "react";
import {
  Globe,
  Loader2,
  Copy,
  AlertTriangle,
  Info,
  Cloud,
  CloudFog,
  RefreshCw,
} from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  usePublicRouteStatus,
  useEnablePublicRoute,
  useDisablePublicRoute,
  useSetPublicRouteTunnelConfig,
  useRegeneratePublicRouteApiKey,
} from "@/lib/query/proxy";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 各应用的路由命名空间（与 src-tauri/src/proxy/server.rs 的路由注册保持一致）。
 * 隧道是共享的，但每个应用接不同的路径前缀。
 */
const APP_ROUTE_PATHS = [
  { app: "Cursor", path: "/cursor/v1" },
  { app: "Claude", path: "/claude" },
  { app: "Codex", path: "/codex/v1" },
  { app: "Gemini", path: "/gemini" },
  { app: "Grok Build", path: "/grokbuild/v1" },
  { app: "Claude Desktop", path: "/claude-desktop" },
] as const;

/**
 * 公网路由面板：经 cloudflared 公网隧道把本地路由暴露给支持自定义
 * Base URL 的应用（如 Cursor）。隧道为共享基础设施；面板内以 Cursor
 * 为例展示接入步骤（模型名映射走供应商 modelCatalog：客户端名 → 上游名）。
 *
 * 注意：Cursor 的模型/Base URL 设置走云端账户同步，无法程序化写入
 * （2026-08-05 实测），所以隧道就绪后自动复制地址并引导用户手动粘贴。
 */
export function PublicRoutePanel() {
  const { t } = useTranslation();

  // 复制到剪贴板。webview 可能拒绝剪贴板权限（非安全上下文/手势失效），
  // 失败时提示手动复制，不再误报"已复制"（#review Minor）。
  const copyText = (text: string, message: string) => {
    navigator.clipboard
      .writeText(text)
      .then(() => toast.success(message, { closeButton: true }))
      .catch(() =>
        toast.error(
          t("common.copyFailed", { defaultValue: "复制失败，请手动复制" }),
          { closeButton: true },
        ),
      );
  };
  const { data: status } = usePublicRouteStatus();
  const enableMutation = useEnablePublicRoute();
  const disableMutation = useDisablePublicRoute();
  const setConfigMutation = useSetPublicRouteTunnelConfig();
  const regenerateMutation = useRegeneratePublicRouteApiKey();

  const [selectedMode, setSelectedMode] = useState<"quick" | "named">("quick");
  const [tunnelName, setTunnelName] = useState("");
  const [hostname, setHostname] = useState("");
  const lastCopiedUrl = useRef<string | null>(null);

  // 回填已保存的配置
  useEffect(() => {
    if (status?.tunnelMode === "named") setSelectedMode("named");
  }, [status?.tunnelMode]);
  useEffect(() => {
    if (status?.namedTunnel) setTunnelName(status.namedTunnel);
  }, [status?.namedTunnel]);
  useEffect(() => {
    if (status?.namedHostname) setHostname(status.namedHostname);
  }, [status?.namedHostname]);

  const enabled = status?.enabled ?? false;
  const publicUrl = status?.publicUrl ?? null;
  const tunnelError = status?.tunnelError ?? null;
  const isPending = enableMutation.isPending || disableMutation.isPending;

  // 隧道就绪：自动复制 Base URL 并引导粘贴（Cursor 设置无法程序化写入）
  useEffect(() => {
    if (!enabled || !publicUrl) return;
    if (lastCopiedUrl.current === publicUrl) return;
    lastCopiedUrl.current = publicUrl;
    copyText(
      `${publicUrl}/cursor/v1`,
      t("proxy.publicRoute.baseUrlCopied", {
        defaultValue:
          "Base URL 已复制，请到 Cursor → Settings → Models → Override OpenAI Base URL 粘贴",
      }),
    );
  }, [enabled, publicUrl, t]);

  const handleToggle = async (checked: boolean) => {
    try {
      if (checked) {
        // 总是先持久化当前选中模式（quick/named），否则后端读到的仍是旧的持久化配置，
        // 切回快速隧道后会实际跑命名隧道、UI 与真实状态不符（#7 review HIGH）。
        const saved = await persistTunnelConfig();
        if (!saved) return;
        await enableMutation.mutateAsync();
        toast.success(
          t("proxy.publicRoute.enabled", {
            defaultValue: "公网路由已启用，正在建立隧道…",
          }),
          { closeButton: true },
        );
      } else {
        await disableMutation.mutateAsync();
        toast.success(
          t("proxy.publicRoute.disabled", {
            defaultValue: "公网路由已关闭",
          }),
          { closeButton: true },
        );
      }
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("proxy.publicRoute.toggleFailed", {
            defaultValue: "切换公网路由失败",
          }),
      );
    }
  };

  // 静默持久化当前隧道模式配置；失败返回 false 并弹错误 toast。
  const persistTunnelConfig = async (): Promise<boolean> => {
    try {
      await setConfigMutation.mutateAsync({
        mode: selectedMode,
        namedTunnel: selectedMode === "named" ? tunnelName.trim() : null,
        namedHostname: selectedMode === "named" ? hostname.trim() : null,
      });
      return true;
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("proxy.publicRoute.configSaveFailed", {
            defaultValue: "保存隧道配置失败",
          }),
      );
      return false;
    }
  };

  const saveTunnelConfig = async (): Promise<boolean> => {
    const ok = await persistTunnelConfig();
    if (ok) {
      toast.success(
        t("proxy.publicRoute.configSaved", { defaultValue: "隧道配置已保存" }),
        { closeButton: true },
      );
    }
    return ok;
  };

  // 重新生成 ccsk 隧道密钥：Cursor 的 key 无法自动写入，重新生成后复制新密钥并引导重新粘贴。
  // 剪贴板写入放在 mutate 之后会丢失用户手势上下文而被平台拒绝——视为"未复制"降级为手动复制提示，
  // 不再把剪贴板错误当重新生成失败抛出。
  const handleRegenerateKey = async () => {
    try {
      const newStatus = await regenerateMutation.mutateAsync();
      const newKey = newStatus?.apiKey ?? "";
      if (newKey) {
        navigator.clipboard
          .writeText(newKey)
          .then(() =>
            toast.success(
              t("proxy.publicRoute.keyRegenerated", {
                defaultValue:
                  "新密钥已生成并复制，请到 Cursor → Settings → Models → OpenAI API Key 重新粘贴",
              }),
              { closeButton: true },
            ),
          )
          .catch(() =>
            toast.success(
              t("proxy.publicRoute.keyRegeneratedNoCopy", {
                defaultValue:
                  "新密钥已生成，请点击上方复制按钮手动复制到 Cursor",
              }),
              { closeButton: true },
            ),
          );
      }
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("proxy.publicRoute.keyRegenerateFailed", {
            defaultValue: "重新生成密钥失败",
          }),
      );
    }
  };

  const statusText = enabled
    ? publicUrl
      ? t("proxy.publicRoute.statusRunning", { defaultValue: "隧道已建立" })
      : t("proxy.publicRoute.statusStarting", { defaultValue: "隧道建立中…" })
    : t("proxy.publicRoute.statusOff", {
        defaultValue: "把本地路由经公网隧道暴露给外部应用",
      });

  return (
    <div className="rounded-xl border border-border bg-card/50 p-4 space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-background ring-1 ring-border">
            <Globe className="h-4 w-4 text-sky-500" />
          </div>
          <div className="space-y-1">
            <p className="text-sm font-medium leading-none">
              {t("proxy.publicRoute.title", { defaultValue: "公网路由" })}
            </p>
            <p className="text-xs text-muted-foreground">{statusText}</p>
          </div>
        </div>
        {isPending ? (
          <div
            data-testid="public-route-toggle-pending"
            className="flex items-center gap-2 text-xs text-muted-foreground"
          >
            <Loader2 className="h-4 w-4 animate-spin text-sky-500" />
            {t("proxy.publicRoute.switching", { defaultValue: "切换中…" })}
          </div>
        ) : (
          <Switch checked={enabled} onCheckedChange={handleToggle} />
        )}
      </div>

      {/* 未启用：隧道模式 + （命名隧道）配置引导 */}
      {!enabled && (
        <div className="space-y-4 pt-1 border-t border-border">
          {/* 隧道模式 */}
          <div className="space-y-2 pt-2">
            <p className="text-xs text-muted-foreground">
              {t("proxy.publicRoute.tunnelMode", { defaultValue: "隧道模式" })}
            </p>
            <div className="grid gap-2 sm:grid-cols-2">
              <button
                type="button"
                onClick={() => setSelectedMode("quick")}
                className={`flex items-start gap-2 rounded-lg border p-3 text-left transition-colors ${
                  selectedMode === "quick"
                    ? "border-sky-500/60 bg-sky-500/10"
                    : "border-border bg-background/60 hover:bg-muted/50"
                }`}
              >
                <CloudFog className="h-4 w-4 mt-0.5 text-sky-500 flex-shrink-0" />
                <span>
                  <span className="block text-sm font-medium">
                    {t("proxy.publicRoute.modeQuick", {
                      defaultValue: "快速隧道",
                    })}
                  </span>
                  <span className="block text-xs text-muted-foreground">
                    {t("proxy.publicRoute.modeQuickHint", {
                      defaultValue: "免账号，地址每次重建都会变",
                    })}
                  </span>
                </span>
              </button>
              <button
                type="button"
                onClick={() => setSelectedMode("named")}
                className={`flex items-start gap-2 rounded-lg border p-3 text-left transition-colors ${
                  selectedMode === "named"
                    ? "border-sky-500/60 bg-sky-500/10"
                    : "border-border bg-background/60 hover:bg-muted/50"
                }`}
              >
                <Cloud className="h-4 w-4 mt-0.5 text-emerald-500 flex-shrink-0" />
                <span>
                  <span className="block text-sm font-medium">
                    {t("proxy.publicRoute.modeNamed", {
                      defaultValue: "固定域名",
                    })}
                  </span>
                  <span className="block text-xs text-muted-foreground">
                    {t("proxy.publicRoute.modeNamedHint", {
                      defaultValue: "需 Cloudflare 账户，地址不变",
                    })}
                  </span>
                </span>
              </button>
            </div>
          </div>

          {/* 命名隧道配置 */}
          {selectedMode === "named" && (
            <div className="space-y-3 rounded-lg border border-border bg-background/60 p-3">
              <div className="grid gap-3 sm:grid-cols-2">
                <div className="space-y-1.5">
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.publicRoute.namedTunnelName", {
                      defaultValue: "隧道名",
                    })}
                  </p>
                  <Input
                    value={tunnelName}
                    onChange={(e) => setTunnelName(e.target.value)}
                    placeholder="my-tunnel"
                  />
                </div>
                <div className="space-y-1.5">
                  <p className="text-xs text-muted-foreground">
                    {t("proxy.publicRoute.namedHostname", {
                      defaultValue: "对外域名",
                    })}
                  </p>
                  <Input
                    value={hostname}
                    onChange={(e) => setHostname(e.target.value)}
                    placeholder="cc.example.com"
                  />
                </div>
              </div>
              <div className="space-y-1.5">
                <p className="text-xs font-medium">
                  {t("proxy.publicRoute.namedSetupTitle", {
                    defaultValue: "一次性配置（在终端执行）",
                  })}
                </p>
                {[
                  "cloudflared login",
                  `cloudflared tunnel create ${tunnelName.trim() || "<隧道名>"}`,
                  `cloudflared tunnel route dns ${tunnelName.trim() || "<隧道名>"} ${hostname.trim() || "<域名>"}`,
                ].map((cmd) => (
                  <div key={cmd} className="flex items-center gap-2">
                    <code className="flex-1 text-xs bg-muted px-2 py-1 rounded break-all">
                      {cmd}
                    </code>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-6 px-2"
                      onClick={() =>
                        copyText(
                          cmd,
                          t("common.copied", { defaultValue: "已复制" }),
                        )
                      }
                    >
                      <Copy className="h-3 w-3" />
                    </Button>
                  </div>
                ))}
              </div>
              <Button
                size="sm"
                onClick={() => void saveTunnelConfig()}
                disabled={setConfigMutation.isPending}
              >
                {setConfigMutation.isPending && (
                  <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
                )}
                {t("proxy.publicRoute.saveTunnelConfig", {
                  defaultValue: "保存隧道配置",
                })}
              </Button>
            </div>
          )}
        </div>
      )}

      {/* 已启用：隧道状态 + 接入示例（以 Cursor 为例） */}
      {enabled && (
        <div className="space-y-3 pt-1 border-t border-border">
          {tunnelError && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <AlertTriangle className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
              <span>{tunnelError}</span>
            </div>
          )}

          {!publicUrl && !tunnelError && (
            <div className="flex items-center gap-2 text-xs text-muted-foreground pt-2">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("proxy.publicRoute.waitingTunnel", {
                defaultValue: "正在建立公网隧道（cloudflared），通常需要几秒…",
              })}
            </div>
          )}

          {publicUrl && (
            <>
              <div className="space-y-2 pt-2">
                <p className="text-xs text-muted-foreground">
                  {t("proxy.publicRoute.publicUrl", {
                    defaultValue: "公网地址",
                  })}
                </p>
                <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                  <code className="flex-1 text-sm bg-background px-3 py-2 rounded border border-border/60 break-all">
                    {publicUrl}
                  </code>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() =>
                      copyText(
                        publicUrl,
                        t("proxy.panel.addressCopied", {
                          defaultValue: "地址已复制",
                        }),
                      )
                    }
                  >
                    <Copy className="mr-1 h-3.5 w-3.5" />
                    {t("common.copy")}
                  </Button>
                </div>
              </div>

              {/* 全部应用路由路径（隧道共享，路径按应用区分） */}
              <div className="rounded-md border border-border bg-background/60 p-3 space-y-2">
                <div className="flex items-center gap-2">
                  <Info className="h-3.5 w-3.5 text-muted-foreground" />
                  <p className="text-xs font-medium">
                    {t("proxy.publicRoute.routePathsTitle", {
                      defaultValue: "各应用路由路径（按需复制）",
                    })}
                  </p>
                </div>
                <p className="text-xs text-muted-foreground">
                  {t("proxy.publicRoute.routePathsGateHint", {
                    defaultValue:
                      "仅已开启「本地路由」接管的应用可经公网路由访问；未开启的应用请求会被拒绝",
                  })}
                </p>
                <div className="space-y-1.5">
                  {APP_ROUTE_PATHS.map(({ app, path }) => {
                    const full = `${publicUrl}${path}`;
                    return (
                      <div key={app} className="flex items-center gap-2">
                        <span className="w-24 flex-shrink-0 text-xs text-muted-foreground">
                          {app}
                        </span>
                        <code className="flex-1 text-xs bg-muted px-2 py-1 rounded break-all">
                          {full}
                        </code>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-2"
                          onClick={() =>
                            copyText(
                              full,
                              t("proxy.panel.addressCopied", {
                                defaultValue: "地址已复制",
                              }),
                            )
                          }
                        >
                          <Copy className="h-3 w-3" />
                        </Button>
                      </div>
                    );
                  })}
                </div>
              </div>

              {/* Cursor 接入示例 */}
              <div className="rounded-md border border-border bg-background/60 p-3 space-y-2">
                <div className="flex items-center gap-2">
                  <Info className="h-3.5 w-3.5 text-muted-foreground" />
                  <p className="text-xs font-medium">
                    {t("proxy.publicRoute.setupTitle", {
                      defaultValue: "接入示例：在 Cursor 中完成以下配置",
                    })}
                  </p>
                </div>
                <ol className="list-decimal pl-5 space-y-2 text-xs text-muted-foreground">
                  <li>
                    {t("proxy.publicRoute.stepBaseUrl", {
                      defaultValue:
                        "Settings → Models → Override OpenAI Base URL 填：",
                    })}
                    <div className="mt-1 flex items-center gap-2">
                      <code className="text-xs bg-muted px-2 py-1 rounded break-all">
                        {publicUrl}/cursor/v1
                      </code>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-6 px-2"
                        onClick={() =>
                          copyText(
                            `${publicUrl}/cursor/v1`,
                            t("proxy.panel.addressCopied", {
                              defaultValue: "地址已复制",
                            }),
                          )
                        }
                      >
                        <Copy className="h-3 w-3" />
                      </Button>
                    </div>
                  </li>
                  <li>
                    {t("proxy.publicRoute.stepApiKey", {
                      defaultValue:
                        "OpenAI API Key 填 CC Switch 自动生成的隧道密钥（必填，公网隧道鉴权用）：",
                    })}
                    {status?.apiKey ? (
                      <span className="mt-1 flex items-center gap-2 flex-wrap">
                        <code className="text-xs bg-muted px-2 py-1 rounded break-all">
                          {status.apiKey}
                        </code>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-6 px-2"
                          onClick={() =>
                            copyText(
                              status.apiKey!,
                              t("proxy.publicRoute.keyCopied", {
                                defaultValue: "密钥已复制",
                              }),
                            )
                          }
                        >
                          <Copy className="h-3 w-3" />
                        </Button>
                        <Button
                          size="sm"
                          variant="outline"
                          className="h-6 px-2"
                          disabled={regenerateMutation.isPending}
                          onClick={() => void handleRegenerateKey()}
                        >
                          {regenerateMutation.isPending ? (
                            <Loader2 className="h-3 w-3 animate-spin" />
                          ) : (
                            <RefreshCw className="h-3 w-3" />
                          )}
                          <span className="ml-1">
                            {t("proxy.publicRoute.regenerateKey", {
                              defaultValue: "重新生成",
                            })}
                          </span>
                        </Button>
                      </span>
                    ) : (
                      <span className="text-xs text-muted-foreground">…</span>
                    )}
                  </li>
                  <li>
                    {t("proxy.publicRoute.stepModel", {
                      defaultValue:
                        "添加模型并选中使用：模型名填供应商 modelCatalog 里配置的客户端名（Cursor Model Name），代理会自动映射为上游模型名",
                    })}
                  </li>
                </ol>
              </div>

              <p className="text-xs text-muted-foreground">
                {status?.tunnelMode === "named"
                  ? t("proxy.publicRoute.namedStableHint", {
                      defaultValue:
                        "固定域名不会变化，Cursor 里粘贴一次即可长期使用。",
                    })
                  : t("proxy.publicRoute.ephemeralHint", {
                      defaultValue:
                        "快速隧道地址每次重建都会变；新地址已自动复制到剪贴板，需在 Cursor 中重新粘贴。需要固定地址可在关闭后改用「固定域名」模式。",
                    })}
              </p>
            </>
          )}
        </div>
      )}
    </div>
  );
}
