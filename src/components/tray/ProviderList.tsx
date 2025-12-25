import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Check, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ProviderIcon } from "@/components/ProviderIcon";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { Provider } from "@/types";

import { formatHost } from "./utils";

type ProviderListProps = {
  providers: Provider[];
  currentId?: string;
  isLoading: boolean;
  isError: boolean;
  switchingKey: string | null;
  activeApp: AppId;
  onSwitch: (provider: Provider) => void;
  emptyMessage?: string;
};

export const ProviderList = ({
  providers,
  currentId,
  isLoading,
  isError,
  switchingKey,
  activeApp,
  onSwitch,
  emptyMessage,
}: ProviderListProps) => {
  const { t } = useTranslation();
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const isSwitchingAny = Boolean(switchingKey);
  const emptyLabel =
    emptyMessage ??
    t("tray.empty", {
      defaultValue: "No providers yet. Use the main window to add one.",
    });

  return (
    <div className="space-y-2">
      <AnimatePresence mode="popLayout">
        {isLoading ? (
          <div className="flex items-center gap-2 px-3 py-2 text-sm bg-white border rounded-xl border-slate-200/70 text-slate-600">
            <Loader2 className="w-4 h-4 animate-spin" />
            {t("tray.loading", { defaultValue: "载入供应商..." })}
          </div>
        ) : isError ? (
          <div className="px-3 py-2 text-xs text-red-700 border border-red-200 rounded-xl bg-red-50">
            {t("tray.error.loadFailed", {
              defaultValue: "加载失败，请刷新",
            })}
          </div>
        ) : providers.length === 0 ? (
          <div className="px-3 py-3 text-xs bg-white border border-dashed rounded-xl border-slate-200 text-slate-500">
            {emptyLabel}
          </div>
        ) : (
          providers.map((provider, index) => {
            const isCurrent = provider.id === currentId;
            const isSwitchingProvider =
              switchingKey === `${activeApp}:${provider.id}`;
            const host = formatHost(provider.websiteUrl);
            return (
              <motion.div
                key={provider.id}
                initial={{ opacity: 0, y: 20 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, scale: 0.95 }}
                transition={{ delay: index * 0.03 }}
                onHoverStart={() => setHoveredId(provider.id)}
                onHoverEnd={() => setHoveredId(null)}
                className={cn("relative group", isCurrent && "order-first")}
              >
                <button
                  onClick={() => onSwitch(provider)}
                  disabled={isCurrent || isSwitchingProvider}
                  className={cn(
                    "w-full relative flex items-center gap-3 p-3 rounded-xl transition-all duration-200 text-left",
                    isCurrent
                      ? "bg-blue-50/80 backdrop-blur-sm border-2 border-blue-200/80 shadow-md cursor-default"
                      : "bg-white/70 backdrop-blur-sm border border-slate-200/60 hover:bg-white hover:border-blue-200 hover:shadow-lg cursor-pointer active:scale-[0.98]",
                    isSwitchingAny && !isSwitchingProvider ? "opacity-60" : ""
                  )}
                  data-tauri-no-drag
                >
                  {hoveredId === provider.id &&
                    !isCurrent &&
                    !isSwitchingProvider && (
                      <motion.div
                        layoutId="hoverGlow"
                        className="absolute inset-0 bg-gradient-to-r from-blue-50/60 to-indigo-50/60 rounded-xl -z-10"
                        transition={{
                          type: "spring",
                          bounce: 0.2,
                          duration: 0.6,
                        }}
                      />
                    )}
                  <div
                    className={cn(
                      "w-11 h-11 flex items-center justify-center rounded-xl flex-shrink-0",
                      isCurrent
                        ? "bg-white shadow-sm"
                        : "bg-slate-50 group-hover:bg-white group-hover:shadow-sm"
                    )}
                  >
                    <ProviderIcon
                      icon={provider.icon}
                      name={provider.name}
                      size={24}
                      className="text-slate-700"
                    />
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 mb-0.5">
                      <span className="text-[13px] leading-[18px] font-medium text-slate-900 truncate">
                        {provider.name}
                      </span>
                      {isCurrent && (
                        <div className="flex items-center gap-1 px-1.5 h-5 bg-blue-600 text-white rounded-md flex-shrink-0">
                          <Check className="w-3 h-3" />
                          <span className="text-[11px] leading-4 font-medium">
                            {t("tray.card.current", { defaultValue: "当前" })}
                          </span>
                        </div>
                      )}
                    </div>
                    <div className="flex items-center gap-2 text-[11px] leading-4 text-slate-500">
                      <span>
                        {provider.category
                          ? t(`tray.categories.${provider.category}`, {
                              defaultValue: provider.category,
                            })
                          : t("tray.providers", { defaultValue: "供应商" })}
                      </span>
                      {(provider.notes || host) && (
                        <>
                          <span className="text-slate-300">·</span>
                          <span className="truncate text-slate-400">
                            {provider.notes || host}
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                  <div className="flex items-center justify-end flex-shrink-0">
                    {!isCurrent && (
                      <motion.div
                        initial={{ opacity: 0, scale: 0.8 }}
                        animate={{
                          opacity:
                            hoveredId === provider.id && !isSwitchingProvider
                              ? 1
                              : 0,
                          scale:
                            hoveredId === provider.id && !isSwitchingProvider
                              ? 1
                              : 0.8,
                          x:
                            hoveredId === provider.id && !isSwitchingProvider
                              ? 0
                              : -4,
                        }}
                        className="px-3 h-7 flex items-center justify-center rounded-xl bg-blue-600 text-white text-[12px] leading-4 font-medium shadow-lg shadow-blue-600/20"
                      >
                        {isSwitchingProvider
                          ? t("tray.card.switching", {
                              defaultValue: "切换中",
                            })
                          : t("tray.card.switch", { defaultValue: "切换" })}
                      </motion.div>
                    )}
                  </div>
                </button>
              </motion.div>
            );
          })
        )}
      </AnimatePresence>
    </div>
  );
};
