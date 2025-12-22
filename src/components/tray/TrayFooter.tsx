import { useEffect, useRef } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { ArrowLeft, Home, LogOut, RefreshCw, Search, Settings, X } from "lucide-react";
import { useTranslation } from "react-i18next";

type TrayFooterProps = {
  viewMode: "main" | "trends";
  isRefreshing: boolean;
  onOpenMain: () => void;
  onOpenSettings: () => void;
  onRefresh: () => void;
  onQuit: () => void;
  onShowMainView: () => void;
  isSearchOpen: boolean;
  searchQuery: string;
  onSearchChange: (value: string) => void;
  onToggleSearch: () => void;
};

export const TrayFooter = ({
  viewMode,
  isRefreshing,
  onOpenMain,
  onOpenSettings,
  onRefresh,
  onQuit,
  onShowMainView,
  isSearchOpen,
  searchQuery,
  onSearchChange,
  onToggleSearch,
}: TrayFooterProps) => {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (isSearchOpen) {
      inputRef.current?.focus();
    }
  }, [isSearchOpen]);

  const showSearch = viewMode === "main";

  return (
    <div className="sticky bottom-0 z-20 flex-shrink-0 px-4 py-3 border-t border-slate-200/60 bg-white/95 backdrop-blur-xl">
      <div className="flex items-center gap-2">
        <AnimatePresence>
          {viewMode === "trends" && (
            <motion.button
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              onClick={onShowMainView}
              className="flex items-center gap-1.5 px-3 h-9 rounded-full bg-slate-100 hover:bg-slate-200 transition-all active:scale-95 flex-shrink-0"
              data-tauri-no-drag
            >
              <ArrowLeft className="w-4 h-4 text-slate-700" />
              <span className="text-[13px] leading-4 font-medium text-slate-700">
                {t("common.back", { defaultValue: "Back" })}
              </span>
            </motion.button>
          )}
        </AnimatePresence>

        {showSearch && isSearchOpen ? (
          <div
            className="flex-1 min-w-0 flex items-center gap-2 h-9 px-3 rounded-full bg-white border border-slate-200 shadow-sm"
            data-tauri-no-drag
          >
            <Search className="w-4 h-4 text-slate-500" />
            <input
              ref={inputRef}
              value={searchQuery}
              onChange={(event) => onSearchChange(event.target.value)}
              placeholder={t("common.search", { defaultValue: "Search" })}
              className="flex-1 min-w-0 bg-transparent text-[13px] leading-4 text-slate-800 placeholder:text-slate-400 focus:outline-none"
              aria-label={t("common.search", { defaultValue: "Search" })}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  onToggleSearch();
                }
              }}
            />
          </div>
        ) : (
          <button
            onClick={onOpenMain}
            className="flex-1 min-w-0 flex items-center justify-center gap-2 h-9 rounded-full bg-blue-600 hover:bg-blue-700 transition-all active:scale-95 shadow-sm"
            data-tauri-no-drag
            title={t("tray.actions.openMain", {
              defaultValue: "Open main window",
            })}
          >
            <Home className="w-4 h-4 text-white" />
            <span className="text-[13px] leading-4 font-semibold text-white truncate">
              {t("tray.actions.openMain", {
                defaultValue: "Open main window",
              })}
            </span>
          </button>
        )}

        {showSearch && (
          <button
            onClick={onToggleSearch}
            className="flex items-center justify-center w-9 h-9 rounded-full bg-slate-100 hover:bg-slate-200 transition-all active:scale-95 flex-shrink-0"
            data-tauri-no-drag
            title={t("common.search", { defaultValue: "Search" })}
          >
            {isSearchOpen ? (
              <X className="w-[18px] h-[18px] text-slate-700" />
            ) : (
              <Search className="w-[18px] h-[18px] text-slate-700" />
            )}
          </button>
        )}

        <button
          onClick={onRefresh}
          disabled={isRefreshing}
          className="flex items-center justify-center w-9 h-9 rounded-full bg-slate-100 hover:bg-slate-200 transition-all active:scale-95 disabled:opacity-60 flex-shrink-0"
          data-tauri-no-drag
          title={t("tray.actions.refresh", { defaultValue: "Refresh" })}
        >
          <RefreshCw className="w-[18px] h-[18px] text-slate-700" />
        </button>

        <button
          onClick={onOpenSettings}
          className="flex items-center justify-center w-9 h-9 rounded-full bg-slate-100 hover:bg-slate-200 transition-all active:scale-95 flex-shrink-0"
          data-tauri-no-drag
          title={t("common.settings", { defaultValue: "Settings" })}
        >
          <Settings className="w-[18px] h-[18px] text-slate-700" />
        </button>

        <button
          onClick={onQuit}
          className="flex items-center justify-center w-9 h-9 rounded-full bg-red-100 hover:bg-red-200 transition-all active:scale-95 flex-shrink-0"
          data-tauri-no-drag
          title={t("tray.actions.quit", { defaultValue: "Quit" })}
        >
          <LogOut className="w-[18px] h-[18px] text-red-600" />
        </button>
      </div>
    </div>
  );
};
