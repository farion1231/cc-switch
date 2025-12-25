import { TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ProviderIcon } from "@/components/ProviderIcon";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { Provider } from "@/types";

import type { TabKey } from "./constants";
import { formatHost } from "./utils";

type TrayHeaderProps = {
  currentProvider?: Provider;
  activeApp: AppId;
  activeTab: TabKey;
  viewMode: "main" | "trends";
  onToggleView: () => void;
  className?: string;
};

export const TrayHeader = ({
  currentProvider,
  activeApp,
  activeTab,
  viewMode,
  onToggleView,
  className,
}: TrayHeaderProps) => {
  const { t } = useTranslation();
  const heroDescription =
    currentProvider?.notes ||
    (currentProvider?.websiteUrl ? formatHost(currentProvider.websiteUrl) : "");
  const heroStatusLabel = currentProvider
    ? t("tray.hero.running", { defaultValue: "运行中" })
    : t("tray.hero.idle", { defaultValue: "尚未选择" });

  return (
    <div
      className={cn(
        "p-4 overflow-hidden border-b border-slate-200/60 bg-white/95 backdrop-blur-sm",
        className
      )}
      data-tauri-drag-region
    >
      <div className="absolute inset-0 bg-gradient-to-br from-slate-50 via-blue-50/30 to-indigo-50/40" />
      <div
        className="absolute inset-0 opacity-[0.03]"
        style={{
          backgroundImage:
            "url(\"data:image/svg+xml,%3Csvg width='60' height='60' xmlns='http://www.w3.org/2000/svg'%3E%3Cpath d='M0 0h60v60H0z' fill='none'/%3E%3Cpath d='M0 0L60 60M60 0L0 60' stroke='%23000' stroke-width='0.5'/%3E%3C/svg%3E\")",
          backgroundSize: "30px 30px",
        }}
      />
      <div className="relative z-10 flex items-start justify-between gap-3">
        <div
          className="flex items-start flex-1 min-w-0 gap-3"
          data-tauri-no-drag
        >
          <div className="relative flex-shrink-0">
            <div className="absolute inset-0 rounded-full bg-blue-400/20 blur-xl" />
            <div className="relative flex items-center justify-center border shadow-md w-14 h-14 bg-white/80 backdrop-blur-md rounded-2xl border-slate-200/60">
              {currentProvider ? (
                <ProviderIcon
                  icon={currentProvider.icon}
                  name={currentProvider.name}
                  size={28}
                  className="text-slate-700"
                />
              ) : (
                <span className="text-2xl text-slate-500">?</span>
              )}
            </div>
          </div>
          <div className="flex-1 min-w-0 py-2">
            <div className="flex items-center gap-2 mb-1">
              <h2 className="text-[15px] leading-5 font-semibold text-slate-900 truncate">
                {currentProvider
                  ? currentProvider.name
                  : t("tray.noCurrent", { defaultValue: "尚未选择" })}
              </h2>
              <div className="flex items-center flex-shrink-0 h-5 gap-1 px-2 text-green-700 rounded-full bg-green-100/80 backdrop-blur-sm">
                <div className="w-1.5 h-1.5 bg-green-500 rounded-full animate-pulse" />
                <span className="text-[11px] leading-4 font-medium">
                  {heroStatusLabel}
                </span>
              </div>
            </div>
            <div className="flex items-center gap-2 mb-1.5 text-[12px] text-slate-600">
              <span className="px-2 h-5 flex items-center bg-white/60 backdrop-blur-sm text-slate-700 rounded-lg text-[11px] leading-4 border border-slate-200/60">
                {t(`apps.${activeApp}`, { defaultValue: activeTab })}
              </span>
              {currentProvider?.category && (
                <span className="text-nowrap px-2 h-5 flex items-center bg-slate-100 text-slate-700 rounded-lg text-[11px] leading-4 border border-slate-200/60">
                  {t(`tray.categories.${currentProvider.category}`, {
                    defaultValue: currentProvider.category,
                  })}
                </span>
              )}
              {currentProvider?.websiteUrl && (
                <span className="text-[12px] leading-4 text-slate-500 truncate">
                  {formatHost(currentProvider.websiteUrl)}
                </span>
              )}
            </div>
            {heroDescription && (
              <p className="text-[12px] leading-4 text-slate-600 truncate">
                {heroDescription}
              </p>
            )}
          </div>
        </div>
        <div className="flex flex-col items-center gap-2" data-tauri-no-drag>
          <button
            onClick={onToggleView}
            className={cn(
              "flex items-center justify-center w-10 h-10 transition-all border rounded-xl border-slate-200/60",
              viewMode === "main"
                ? "bg-white/60 backdrop-blur-sm hover:bg-white hover:border-blue-200"
                : "bg-white/80 hover:bg-white"
            )}
          >
            <TrendingUp className="w-5 h-5 text-slate-600" />
          </button>
        </div>
      </div>
    </div>
  );
};
