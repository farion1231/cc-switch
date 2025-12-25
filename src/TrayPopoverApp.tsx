import { useCallback, useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "framer-motion";
import { Activity, Clock, DollarSign } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { emitTo } from "@tauri-apps/api/event";
import { exit } from "@tauri-apps/plugin-process";

import { TrayFooter } from "@/components/tray/TrayFooter";
import { TrayHeader } from "@/components/tray/TrayHeader";
import { ProviderList } from "@/components/tray/ProviderList";
import { TabSwitcher } from "@/components/tray/TabSwitcher";
import {
  APP_ORDER,
  APP_TO_TAB,
  TAB_ITEMS,
  TAB_TO_APP,
  type TabKey,
} from "@/components/tray/constants";
import { TrendsView, type UsageStat } from "@/components/tray/TrendsView";
import { formatHost, formatPercentage } from "@/components/tray/utils";
import { useProvidersQuery, useUsageQuery } from "@/lib/query";
import { useUsageTrends } from "@/lib/query/usage";
import { providersApi, type AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

type ProvidersQueryResult = ReturnType<typeof useProvidersQuery>;

const TrayPopoverApp = () => {
  const { t, i18n } = useTranslation();
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

  const [activeApp, setActiveApp] = useState<AppId>("claude");
  const [viewMode, setViewMode] = useState<"main" | "trends">("main");
  const [switchingKey, setSwitchingKey] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  const activeTab = APP_TO_TAB[activeApp];
  const tabItems = useMemo(
    () =>
      TAB_ITEMS.map((tab) => ({
        key: tab,
        label: t(`apps.${TAB_TO_APP[tab]}`, { defaultValue: tab }),
      })),
    [t]
  );
  const activeQuery = queries[activeApp];
  const providers = useMemo(
    () => Object.values(activeQuery.data?.providers ?? {}),
    [activeQuery.data?.providers]
  );
  const currentId = activeQuery.data?.currentProviderId ?? "";
  const currentProvider = currentId
    ? activeQuery.data?.providers?.[currentId]
    : undefined;

  const usageQuery = useUsageQuery(currentId, activeApp, {
    enabled: Boolean(currentId),
  });
  const usagePlans = usageQuery.data?.success
    ? (usageQuery.data?.data ?? [])
    : [];
  const { data: trendData } = useUsageTrends(30);

  const historyPoints = useMemo(() => {
    if (!trendData || trendData.length === 0) return [];
    return trendData.slice(-30).map((stat) => ({
      date: new Date(stat.date),
      cost: parseFloat(stat.totalCost ?? "0"),
    }));
  }, [trendData]);

  const usageProgress = useMemo<UsageStat[]>(() => {
    return usagePlans.slice(0, 3).map((plan) => {
      const percent = formatPercentage(plan.used, plan.total) ?? 0;
      const unit = plan.unit ? ` ${plan.unit}` : "";
      let subLeft: string | undefined;
      if (plan.used !== undefined && plan.total !== undefined) {
        subLeft = `${plan.used.toLocaleString()}${unit} / ${plan.total.toLocaleString()}${unit}`;
      } else if (plan.used !== undefined) {
        subLeft = `${plan.used.toLocaleString()}${unit}`;
      } else if (plan.remaining !== undefined) {
        subLeft = t("tray.usage.remaining", {
          defaultValue: "剩余 {{value}}",
          value: `${plan.remaining.toLocaleString()}${unit}`,
        });
      }
      return {
        label:
          plan.planName || t("tray.usage.unnamed", { defaultValue: "Plan" }),
        percent,
        hintRight: plan.extra,
        subLeft,
      };
    });
  }, [t, usagePlans]);

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
            await queryClient.invalidateQueries({
              queryKey: ["providers", appId],
            });
          }
        });
      } catch (error) {
        console.error(
          "[TrayPopover] Failed to subscribe provider switch event",
          error
        );
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
        await emitTo("main", "tray:navigate", { view: "providers" });
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

  const openSettingsWindow = useCallback(async () => {
    try {
      const main = await WebviewWindow.getByLabel("main");
      if (main) {
        await main.unminimize();
        await main.show();
        await main.setFocus();
        await emitTo("main", "tray:navigate", { view: "settings" });
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
      if (currentId) {
        await queryClient.invalidateQueries({
          queryKey: ["usage", currentId, activeApp],
        });
      }
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
  }, [activeApp, currentId, queryClient, t]);

  const handleQuit = useCallback(async () => {
    await exit(0);
  }, []);

  const handleToggleViewMode = useCallback(() => {
    setViewMode((mode) => (mode === "main" ? "trends" : "main"));
  }, []);

  const handleShowMainView = useCallback(() => {
    setViewMode("main");
  }, []);

  const handleOpenSearch = useCallback(() => {
    setIsSearchOpen(true);
  }, []);

  const handleCloseSearch = useCallback(() => {
    setIsSearchOpen(false);
    setSearchQuery("");
  }, []);

  useEffect(() => {
    if (viewMode === "trends" && isSearchOpen) {
      handleCloseSearch();
    }
  }, [handleCloseSearch, isSearchOpen, viewMode]);

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

  const orderedProviders = useMemo(() => {
    if (!currentId) return providers;
    return [...providers].sort((a, b) => {
      const aActive = a.id === currentId;
      const bActive = b.id === currentId;
      if (aActive === bActive) {
        return a.name.localeCompare(b.name, i18n.language);
      }
      return aActive ? -1 : 1;
    });
  }, [providers, currentId, i18n.language]);

  const normalizedQuery = useMemo(
    () => searchQuery.trim().toLowerCase(),
    [searchQuery]
  );
  const filteredProviders = useMemo(() => {
    if (!normalizedQuery) return orderedProviders;
    return orderedProviders.filter((provider) => {
      const host = formatHost(provider.websiteUrl);
      const haystack = [
        provider.name,
        provider.category,
        provider.notes,
        host,
      ]
        .filter(Boolean)
        .join(" ")
        .toLowerCase();
      return haystack.includes(normalizedQuery);
    });
  }, [normalizedQuery, orderedProviders]);

  const historyTotal = historyPoints.reduce(
    (sum, point) => sum + point.cost,
    0
  );
  const historyStartLabel = historyPoints.length
    ? historyPoints[0].date.toLocaleDateString(i18n.language, {
        month: "short",
        day: "numeric",
      })
    : "";
  const historyMidLabel = historyPoints.length
    ? historyPoints[
        Math.floor(historyPoints.length / 2)
      ].date.toLocaleDateString(i18n.language, {
        month: "short",
        day: "numeric",
      })
    : "";
  const historyEndLabel = historyPoints.length
    ? historyPoints[historyPoints.length - 1].date.toLocaleDateString(
        i18n.language,
        { month: "short", day: "numeric" }
      )
    : "";

  const totalUsageValue = usagePlans.reduce((sum, plan) => {
    return sum + (plan.used ?? 0);
  }, 0);
  const summaryCards = [
    {
      key: "cost",
      label: t("tray.summary.cost30", { defaultValue: "30天成本" }),
      value:
        historyTotal > 0
          ? `$${historyTotal.toFixed(2)}`
          : t("tray.summary.unknown", { defaultValue: "--" }),
      bg: "bg-blue-50 border-blue-200",
      text: "text-blue-900",
      accent: "text-blue-600",
      Icon: DollarSign,
    },
    {
      key: "usage",
      label: t("tray.summary.totalUsage", { defaultValue: "总用量" }),
      value:
        totalUsageValue > 0
          ? totalUsageValue.toLocaleString()
          : t("tray.summary.unknown", { defaultValue: "--" }),
      bg: "bg-purple-50 border-purple-200",
      text: "text-purple-900",
      accent: "text-purple-600",
      Icon: Activity,
    },
    {
      key: "latency",
      label: t("tray.summary.avgLatency", { defaultValue: "平均延迟" }),
      value: t("tray.summary.unavailable", { defaultValue: "暂无数据" }),
      bg: "bg-green-50 border-green-200",
      text: "text-green-900",
      accent: "text-green-600",
      Icon: Clock,
    },
  ];

  const historyBars = historyPoints.map((point) => point.cost);
  const historyLabels = {
    start: historyStartLabel,
    mid: historyMidLabel,
    end: historyEndLabel,
  };
  const handleTabChange = useCallback((tab: TabKey) => {
    setActiveApp(TAB_TO_APP[tab]);
    setViewMode("main");
  }, []);

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      className="select-none relative w-full h-full bg-white/90 backdrop-blur-sm border border-slate-200 shadow-[0_8px_24px_rgba(0,0,0,0.08)] "
    >
      <div className="relative flex flex-col h-full">
        <TrayHeader
          currentProvider={currentProvider}
          activeApp={activeApp}
          activeTab={activeTab}
          viewMode={viewMode}
          onToggleView={handleToggleViewMode}
        />

        <AnimatePresence mode="wait" initial={false}>
          {viewMode === "main" ? (
            <motion.div
              key="main"
              initial={{ opacity: 0, x: -20 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: -20 }}
              transition={{ duration: 0.25 }}
              className="flex flex-col h-full overflow-hidden"
            >
              <TabSwitcher
                tabs={tabItems}
                activeTab={activeTab}
                onSelect={handleTabChange}
              />
              <div className="flex-1 h-56 px-4 py-3 overflow-y-auto">
                <ProviderList
                  providers={filteredProviders}
                  currentId={currentId}
                  isLoading={activeQuery.isLoading}
                  isError={Boolean(activeQuery.isError)}
                  switchingKey={switchingKey}
                  activeApp={activeApp}
                  onSwitch={(provider) => handleSwitch(activeApp, provider)}
                  emptyMessage={
                    normalizedQuery
                      ? t("tray.searchEmpty", {
                          defaultValue: "No matching providers",
                        })
                      : undefined
                  }
                />
              </div>
            </motion.div>
          ) : (
            <motion.div
              key="trends"
              initial={{ opacity: 0, x: 20 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 20 }}
              transition={{ duration: 0.25 }}
              className="h-full px-4 py-3 overflow-y-auto"
            >
              <TrendsView
                summaryCards={summaryCards}
                historyBars={historyBars}
                historyLabels={historyLabels}
                costBreakdown={usageProgress}
                usageDetails={usageProgress}
              />
            </motion.div>
          )}
        </AnimatePresence>

      <TrayFooter
        viewMode={viewMode}
        isRefreshing={isRefreshing}
        onOpenMain={openMainWindow}
        onOpenSettings={openSettingsWindow}
        onRefresh={handleRefresh}
        onQuit={handleQuit}
        onShowMainView={handleShowMainView}
        isSearchOpen={isSearchOpen}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        onOpenSearch={handleOpenSearch}
        onCloseSearch={handleCloseSearch}
      />
      </div>
    </motion.div>
  );
};

export default TrayPopoverApp;
