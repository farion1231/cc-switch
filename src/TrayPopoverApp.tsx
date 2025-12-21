import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ButtonHTMLAttributes,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "framer-motion";
import {
  ArrowRight,
  Bot,
  CheckCircle2,
  Cpu,
  Loader2,
  Power,
  RefreshCw,
  Sparkles,
  X,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { exit } from "@tauri-apps/plugin-process";

import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import { useProvidersQuery } from "@/lib/query";
import { providersApi, type AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

type ProvidersQueryResult = ReturnType<typeof useProvidersQuery>;

const APP_ORDER: AppId[] = ["claude", "codex", "gemini"];

type TrayAppTheme = {
  Icon: LucideIcon;
  gradient: string;
  highlight: string;
  ring: string;
};

const APP_THEMES: Record<AppId, TrayAppTheme> = {
  claude: {
    Icon: Sparkles,
    gradient: "from-orange-500/30 via-rose-500/20 to-pink-500/20",
    highlight: "text-orange-200",
    ring: "focus-visible:ring-orange-200/60",
  },
  codex: {
    Icon: Cpu,
    gradient: "from-sky-500/30 via-blue-500/20 to-indigo-500/20",
    highlight: "text-blue-200",
    ring: "focus-visible:ring-blue-200/60",
  },
  gemini: {
    Icon: Bot,
    gradient: "from-emerald-500/30 via-teal-500/20 to-cyan-500/20",
    highlight: "text-emerald-200",
    ring: "focus-visible:ring-emerald-200/60",
  },
};

const formatHost = (url?: string) => {
  if (!url) return "";
  try {
    const host = new URL(url);
    return host.hostname.replace(/^www\./, "");
  } catch {
    return url.replace(/^https?:\/\//, "");
  }
};

interface ProviderCardProps {
  provider: Provider;
  appId: AppId;
  theme: TrayAppTheme;
  isActive: boolean;
  onSwitch: (appId: AppId, provider: Provider) => Promise<void>;
  isSwitching: boolean;
  t: (key: string, params?: Record<string, any>) => string;
}

const ProviderCard = ({
  provider,
  appId,
  theme,
  isActive,
  onSwitch,
  isSwitching,
  t,
}: ProviderCardProps) => {
  const metaHost = formatHost(provider.websiteUrl);
  const handleClick = useCallback(async () => {
    if (isActive || isSwitching) return;
    await onSwitch(appId, provider);
  }, [appId, isActive, isSwitching, onSwitch, provider]);

  return (
    <motion.button
      layout
      type="button"
      disabled={isActive || isSwitching}
      onClick={handleClick}
      data-tauri-no-drag
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      className={cn(
        "group relative flex w-full items-center gap-3 rounded-2xl border border-white/10",
        "bg-white/5/80 px-3 py-2.5 text-left text-white transition-all",
        "hover:border-white/30 hover:bg-white/10 focus-visible:outline-none focus-visible:ring-2",
        theme.ring,
        isActive &&
          "border-white/30 bg-white/15 shadow-[0_15px_45px_rgba(255,255,255,0.12)]",
        (isActive || isSwitching) && "cursor-default"
      )}
    >
      <div className="flex items-center justify-center h-11 w-11 rounded-2xl bg-white/10">
        <ProviderIcon
          icon={provider.icon}
          name={provider.name}
          size={26}
          className="text-white"
        />
      </div>
      <div className="flex flex-col flex-1 overflow-hidden">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold text-white">
            {provider.name}
          </span>
          <span
            className={cn(
              "rounded-full bg-white/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
              theme.highlight
            )}
          >
            {t(`tray.categories.${provider.category ?? "custom"}`, {
              defaultValue: provider.category ?? "Custom",
            })}
          </span>
        </div>
        {provider.notes ? (
          <p className="text-xs truncate text-white/70 opacity-90">
            {provider.notes}
          </p>
        ) : (
          metaHost && (
            <p className="text-xs truncate text-white/60">{metaHost}</p>
          )
        )}
      </div>
      <div className="flex flex-col items-end text-[11px] font-semibold text-white/70">
        {isActive ? (
          <span className="inline-flex items-center gap-1 rounded-full bg-emerald-500/20 px-2 py-0.5 text-emerald-100">
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t("tray.card.current", { defaultValue: "当前" })}
          </span>
        ) : (
          <span className="inline-flex items-center gap-1 rounded-full bg-white/10 px-2 py-0.5">
            {isSwitching ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                {t("tray.card.switching", { defaultValue: "切换中" })}
              </>
            ) : (
              <>
                <ArrowRight className="h-3.5 w-3.5" />
                {t("tray.card.switch", { defaultValue: "切换" })}
              </>
            )}
          </span>
        )}
      </div>
    </motion.button>
  );
};

interface ActionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: ReactNode;
  label: string;
}

const ActionButton = ({
  icon,
  label,
  className,
  ...props
}: ActionButtonProps) => (
  <button
    type="button"
    data-tauri-no-drag
    {...props}
    className={cn(
      "flex items-center gap-1.5 rounded-full border border-white/10 px-3 py-1.5",
      "text-xs font-semibold text-white/80 transition hover:border-white/30 hover:bg-white/10",
      "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white/40",
      props.disabled && "cursor-not-allowed opacity-60",
      className
    )}
  >
    {icon}
    <span>{label}</span>
  </button>
);

const TrayPopoverApp = () => {
  const { t } = useTranslation();
  const trayWindow = useMemo(() => getCurrentWindow(), []);
  const queryClient = useQueryClient();
  const claudeQuery = useProvidersQuery("claude");
  const codexQuery = useProvidersQuery("codex");
  const geminiQuery = useProvidersQuery("gemini");
  const queries: Record<AppId, ProvidersQueryResult> = {
    claude: claudeQuery,
    codex: codexQuery,
    gemini: geminiQuery,
  };

  const [switchingKey, setSwitchingKey] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);

  useEffect(() => {
    document.body.classList.add("tray-window");
    const keyHandler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        void trayWindow.hide();
      }
    };
    window.addEventListener("keydown", keyHandler);
    return () => {
      document.body.classList.remove("tray-window");
      window.removeEventListener("keydown", keyHandler);
    };
  }, [trayWindow]);

  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    const setup = async () => {
      try {
        unsubscribe = await providersApi.onSwitched(async (event) => {
          const appId = event.appType;
          if (APP_ORDER.includes(appId)) {
            await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
          }
        });
      } catch (error) {
        console.error("[TrayPopover] Failed to subscribe provider switch event", error);
      }
    };
    void setup();
    return () => {
      unsubscribe?.();
    };
  }, [queryClient]);

  const handleClose = useCallback(async () => {
    await trayWindow.hide();
  }, [trayWindow]);

  const openMainWindow = useCallback(async () => {
    try {
      const main = await WebviewWindow.getByLabel("main");
      if (main) {
        await main.unminimize();
        await main.show();
        await main.setFocus();
      }
      await handleClose();
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("tray.error.generic", { defaultValue: "发生未知错误" });
      toast.error(
        t("tray.error.openMain", { defaultValue: "打开主界面失败" }),
        {
          description: detail,
        }
      );
    }
  }, [handleClose, t]);

  const handleRefresh = useCallback(async () => {
    setIsRefreshing(true);
    try {
      await Promise.all(
        APP_ORDER.map((appId) =>
          queryClient.invalidateQueries({ queryKey: ["providers", appId] })
        )
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("tray.error.generic", { defaultValue: "发生未知错误" });
      toast.error(
        t("tray.error.refreshFailed", {
          defaultValue: "刷新失败",
        }),
        { description: detail }
      );
    } finally {
      setIsRefreshing(false);
    }
  }, [queryClient, t]);

  const handleQuit = useCallback(async () => {
    await exit(0);
  }, []);

  const handleSwitch = useCallback(
    async (appId: AppId, provider: Provider) => {
      const key = `${appId}:${provider.id}`;
      setSwitchingKey(key);
      try {
        await providersApi.switch(provider.id, appId);
        await providersApi.updateTrayMenu();
        await queryClient.invalidateQueries({ queryKey: ["providers", appId] });
        await trayWindow.hide();
      } catch (error) {
        const detail =
          extractErrorMessage(error) ||
          t("tray.error.generic", { defaultValue: "发生未知错误" });
        toast.error(t("tray.switchFailedTitle", { defaultValue: "切换失败" }), {
          description: t("tray.switchFailed", {
            defaultValue: "切换供应商失败：{{error}}",
            error: detail,
          }),
        });
      } finally {
        setSwitchingKey((prev) => (prev === key ? null : prev));
      }
    },
    [queryClient, t, trayWindow]
  );

  const headerSubtitle = useMemo(
    () => t("tray.subtitle", { defaultValue: "一键切换到任意供应商" }),
    [t]
  );

  return (
    <div
      className={cn(
        "tray-popover flex h-[480px] w-[360px] flex-col rounded-md",
        "border border-white/10 bg-gradient-to-br from-slate-950/95 via-slate-900/90 to-slate-950/95",
        "p-4 text-white shadow-[0_20px_70px_rgba(0,0,0,0.55)] backdrop-blur-2xl"
      )}
    >
      <div
        className="p-3 border rounded-2xl border-white/10 bg-white/5"
        data-tauri-drag-region
      >
        <div className="flex items-start justify-between gap-3">
          <div>
            <p className="text-sm font-semibold tracking-wide">
              {t("tray.title", { defaultValue: "快速切换" })}
            </p>
            <p className="text-xs text-white/70">{headerSubtitle}</p>
          </div>
          <button
            type="button"
            data-tauri-no-drag
            onClick={handleClose}
            className="p-1 transition border rounded-full border-white/10 bg-white/5 text-white/70 hover:bg-white/15"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
        <div className="flex flex-wrap gap-2 mt-3" data-tauri-no-drag>
          <ActionButton
            icon={<Sparkles className="h-3.5 w-3.5" />}
            label={t("tray.actions.openMain", { defaultValue: "打开主界面" })}
            onClick={openMainWindow}
          />
          <ActionButton
            icon={
              isRefreshing ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="h-3.5 w-3.5" />
              )
            }
            label={t("tray.actions.refresh", { defaultValue: "刷新" })}
            onClick={handleRefresh}
            disabled={isRefreshing}
          />
          <ActionButton
            icon={<Power className="h-3.5 w-3.5 text-red-200" />}
            label={t("tray.actions.quit", { defaultValue: "退出" })}
            onClick={handleQuit}
            className="hover:bg-red-500/20"
          />
        </div>
      </div>

      <div className="flex-1 pr-1 mt-3 space-y-3 overflow-y-auto">
        {APP_ORDER.map((appId) => {
          const theme = APP_THEMES[appId];
          const query = queries[appId];
          const providers = Object.values(query.data?.providers ?? {});
          const currentId = query.data?.currentProviderId ?? "";
          const currentProvider =
            currentId && query.data?.providers
              ? query.data.providers[currentId]
              : undefined;

          return (
            <motion.section
              key={appId}
              layout
              className={cn(
                "rounded-2xl border border-white/10 p-3",
                "bg-gradient-to-br",
                theme.gradient
              )}
            >
              <div className="flex items-start justify-between gap-2">
                <div className="flex items-center gap-2">
                  <div className="p-2 rounded-2xl bg-black/30">
                    <theme.Icon className="w-4 h-4 text-white" />
                  </div>
                  <div>
                    <p className="text-sm font-semibold tracking-wide">
                      {t(`apps.${appId}`, {
                        defaultValue: appId,
                      })}
                    </p>
                    <p className="text-xs text-white/70">
                      {currentProvider
                        ? t("tray.currentProvider", {
                            defaultValue: "当前：{{name}}",
                            name: currentProvider.name,
                          })
                        : t("tray.noCurrent", { defaultValue: "尚未选择" })}
                    </p>
                  </div>
                </div>
                <span className="rounded-full bg-white/10 px-2 py-0.5 text-[10px] font-medium text-white/70">
                  {providers.length}{" "}
                  {t("tray.providers", { defaultValue: "个供应商" })}
                </span>
              </div>

              <div className="mt-3 space-y-2">
                {query.isLoading ? (
                  <div className="flex items-center gap-2 px-3 py-2 text-sm border rounded-2xl border-white/5 bg-black/10 text-white/70">
                    <Loader2 className="w-4 h-4 animate-spin" />
                    {t("tray.loading", { defaultValue: "载入供应商..." })}
                  </div>
                ) : query.isError ? (
                  <div className="px-3 py-2 text-sm text-red-100 border rounded-2xl border-red-500/30 bg-red-500/10">
                    {t("tray.error.loadFailed", {
                      defaultValue: "加载失败，请刷新",
                    })}
                  </div>
                ) : providers.length === 0 ? (
                  <div className="px-3 py-3 text-xs border border-dashed rounded-2xl border-white/20 bg-white/5 text-white/70">
                    {t("tray.empty", {
                      defaultValue: "暂未添加供应商，点击主界面添加",
                    })}
                  </div>
                ) : (
                  <AnimatePresence mode="popLayout">
                    {providers.map((provider) => (
                      <ProviderCard
                        key={provider.id}
                        appId={appId}
                        provider={provider}
                        theme={theme}
                        isActive={provider.id === currentId}
                        onSwitch={handleSwitch}
                        isSwitching={switchingKey === `${appId}:${provider.id}`}
                        t={t}
                      />
                    ))}
                  </AnimatePresence>
                )}
              </div>
            </motion.section>
          );
        })}
      </div>
    </div>
  );
};

export default TrayPopoverApp;
