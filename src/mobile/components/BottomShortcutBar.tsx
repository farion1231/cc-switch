import { Ellipsis } from "lucide-react";
import {
  ALL_MOBILE_VIEWS,
  getUnpinnedViews,
  type MobileViewId,
} from "../shortcuts";
import { tr, type MobileLang } from "../i18n";
import { ViewIcon } from "./ViewIcon";

interface BottomShortcutBarProps {
  pinned: readonly MobileViewId[];
  activeView: MobileViewId;
  lang: MobileLang;
  moreOpen: boolean;
  onSelectView: (view: MobileViewId) => void;
  onToggleMore: () => void;
}

const VIEW_MAP = new Map(ALL_MOBILE_VIEWS.map((v) => [v.id, v]));

export function BottomShortcutBar({
  pinned,
  activeView,
  lang,
  moreOpen,
  onSelectView,
  onToggleMore,
}: BottomShortcutBarProps) {
  const unpinned = getUnpinnedViews(pinned);
  const hasOverflow = unpinned.length > 0;
  const activeInOverflow = hasOverflow && unpinned.includes(activeView);

  return (
    <nav
      aria-label="Mobile navigation"
      className="fixed inset-x-0 bottom-0 z-30 border-t border-zinc-200/80 bg-white/85 pb-[env(safe-area-inset-bottom)] backdrop-blur-xl dark:border-zinc-800/90 dark:bg-[#1c1c21]/85"
    >
      <div className="mx-auto flex max-w-lg items-stretch px-1">
        {pinned.map((id) => {
          const meta = VIEW_MAP.get(id);
          if (!meta) return null;
          const isActive = activeView === id && !moreOpen;
          return (
            <button
              key={id}
              type="button"
              onClick={() => onSelectView(id)}
              aria-current={isActive ? "page" : undefined}
              className={`relative flex min-h-[56px] flex-1 flex-col items-center justify-center gap-0.5 px-1 py-1.5 transition ${
                isActive
                  ? "text-[#f97316]"
                  : "text-zinc-500 active:text-zinc-900 dark:text-zinc-400 dark:active:text-zinc-100"
              }`}
            >
              <ViewIcon id={id} className="h-[22px] w-[22px]" />
              <span className="max-w-full truncate text-[11px] font-medium leading-tight">
                {lang === "zh" ? meta.shortZh : meta.shortEn}
              </span>
            </button>
          );
        })}

        {hasOverflow && (
          <button
            type="button"
            onClick={onToggleMore}
            aria-expanded={moreOpen}
            aria-haspopup="dialog"
            className={`relative flex min-h-[56px] flex-1 flex-col items-center justify-center gap-0.5 px-1 py-1.5 transition ${
              moreOpen || activeInOverflow
                ? "text-[#f97316]"
                : "text-zinc-500 active:text-zinc-900 dark:text-zinc-400 dark:active:text-zinc-100"
            }`}
          >
            <div className="relative">
              <Ellipsis className="h-[22px] w-[22px]" />
              {activeInOverflow && !moreOpen && (
                <span className="absolute -right-1 -top-0.5 h-2 w-2 rounded-full bg-[#f97316]" />
              )}
            </div>
            <span className="max-w-full truncate text-[11px] font-medium leading-tight">
              {tr(lang, "more")}
            </span>
          </button>
        )}
      </div>
    </nav>
  );
}
