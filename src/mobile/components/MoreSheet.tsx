import { Check, SlidersHorizontal, X } from "lucide-react";
import {
  ALL_MOBILE_VIEWS,
  getUnpinnedViews,
  type MobileViewId,
} from "../shortcuts";
import { tr, type MobileLang } from "../i18n";
import { ViewIcon } from "./ViewIcon";

interface MoreSheetProps {
  open: boolean;
  pinned: readonly MobileViewId[];
  activeView: MobileViewId;
  lang: MobileLang;
  onSelectView: (view: MobileViewId) => void;
  onOpenCustomize: () => void;
  onClose: () => void;
}

const VIEW_MAP = new Map(ALL_MOBILE_VIEWS.map((v) => [v.id, v]));

export function MoreSheet({
  open,
  pinned,
  activeView,
  lang,
  onSelectView,
  onOpenCustomize,
  onClose,
}: MoreSheetProps) {
  if (!open) return null;
  const unpinned = getUnpinnedViews(pinned);

  return (
    <div
      className="fixed inset-0 z-40 flex items-end justify-center bg-black/45 backdrop-blur-[2px]"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label={tr(lang, "moreSheetTitle")}
    >
      <div
        className="w-full max-w-lg rounded-t-3xl border-t border-zinc-200 bg-white px-4 pb-[calc(1.25rem+env(safe-area-inset-bottom))] pt-3 shadow-2xl dark:border-zinc-800 dark:bg-[#232328]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-zinc-300 dark:bg-zinc-700" />

        <div className="mb-3 flex items-center justify-between px-1">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
            {tr(lang, "moreSheetTitle")}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={tr(lang, "close")}
            className="flex h-9 w-9 items-center justify-center rounded-full text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-1.5">
          {unpinned.map((id) => {
            const meta = VIEW_MAP.get(id);
            if (!meta) return null;
            const isActive = activeView === id;
            return (
              <button
                key={id}
                type="button"
                onClick={() => {
                  onSelectView(id);
                  onClose();
                }}
                className={`flex min-h-[48px] w-full items-center justify-between rounded-xl px-3.5 py-2.5 text-left transition ${
                  isActive
                    ? "bg-orange-500/10 text-[#f97316]"
                    : "bg-zinc-50 text-zinc-800 active:bg-zinc-100 dark:bg-zinc-800/60 dark:text-zinc-100 dark:active:bg-zinc-800"
                }`}
              >
                <span className="flex items-center gap-3 text-sm font-medium">
                  <ViewIcon id={id} className="h-5 w-5" />
                  {lang === "zh" ? meta.labelZh : meta.labelEn}
                </span>
                {isActive && <Check className="h-4 w-4" />}
              </button>
            );
          })}
        </div>

        <div className="mt-4 border-t border-zinc-200/80 pt-3 dark:border-zinc-800">
          <button
            type="button"
            onClick={() => {
              onClose();
              onOpenCustomize();
            }}
            className="flex min-h-[48px] w-full items-center justify-center gap-2 rounded-xl border border-zinc-200 bg-zinc-50 px-4 text-sm font-medium text-zinc-800 transition active:scale-[0.99] dark:border-zinc-700 dark:bg-zinc-900/70 dark:text-zinc-100"
          >
            <SlidersHorizontal className="h-4 w-4 text-[#f97316]" />
            {tr(lang, "customizeBarBtn")}
          </button>
        </div>
      </div>
    </div>
  );
}
