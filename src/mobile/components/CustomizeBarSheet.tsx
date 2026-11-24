import { useEffect, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Ellipsis,
  GripVertical,
  Pin,
  PinOff,
  RotateCcw,
  X,
} from "lucide-react";
import {
  ALL_MOBILE_VIEWS,
  MIN_PINNED_SHORTCUTS,
  getUnpinnedViews,
  maxPinnedShortcuts,
  moveShortcut,
  reorderShortcut,
  resetShortcuts,
  toggleShortcutPin,
  type MobileViewId,
} from "../shortcuts";
import { tr, type MobileLang } from "../i18n";
import { ViewIcon } from "./ViewIcon";

interface CustomizeBarSheetProps {
  open: boolean;
  pinned: readonly MobileViewId[];
  lang: MobileLang;
  onChange: (nextPinned: MobileViewId[]) => void;
  onClose: () => void;
}

const VIEW_MAP = new Map(ALL_MOBILE_VIEWS.map((v) => [v.id, v]));

export function CustomizeBarSheet({
  open,
  pinned,
  lang,
  onChange,
  onClose,
}: CustomizeBarSheetProps) {
  const [dragIndex, setDragIndex] = useState<number | null>(null);

  useEffect(() => {
    if (!open) setDragIndex(null);
  }, [open]);

  if (!open) return null;

  const maxPinned = maxPinnedShortcuts();
  const unpinned = getUnpinnedViews(pinned);
  const canPinMore = pinned.length < maxPinned;
  const canUnpin = pinned.length > MIN_PINNED_SHORTCUTS;

  return (
    <div
      className="fixed inset-0 z-50 flex items-end justify-center bg-black/50 backdrop-blur-[2px]"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label={tr(lang, "customizeTitle")}
    >
      <div
        className="flex max-h-[88dvh] w-full max-w-lg flex-col rounded-t-3xl border-t border-zinc-200 bg-white shadow-2xl dark:border-zinc-800 dark:bg-[#232328]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-4 pt-3">
          <div className="mx-auto mb-3 h-1 w-10 rounded-full bg-zinc-300 dark:bg-zinc-700" />
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-base font-semibold text-zinc-900 dark:text-zinc-100">
                {tr(lang, "customizeTitle")}
              </h2>
              <p className="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">
                {tr(lang, "slotsHint")}
              </p>
            </div>
            <button
              type="button"
              onClick={onClose}
              aria-label={tr(lang, "close")}
              className="flex h-9 w-9 items-center justify-center rounded-full text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Live Preview of the Bottom Bar */}
        <div className="mt-3 px-4">
          <div className="mb-1.5 flex items-center justify-between text-[11px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
            <span>{tr(lang, "previewLabel")}</span>
            <span className="tabular-nums">
              {pinned.length} / {maxPinned}
            </span>
          </div>
          <div
            data-testid="shortcut-bar-preview"
            className="flex items-stretch rounded-2xl border border-zinc-200 bg-zinc-50/90 px-1 py-1 dark:border-zinc-700/80 dark:bg-zinc-900/90"
          >
            {pinned.map((id, idx) => {
              const meta = VIEW_MAP.get(id);
              if (!meta) return null;
              return (
                <div
                  key={id}
                  className={`flex flex-1 flex-col items-center justify-center gap-0.5 py-1.5 ${
                    idx === 0
                      ? "text-[#f97316]"
                      : "text-zinc-500 dark:text-zinc-400"
                  }`}
                >
                  <ViewIcon id={id} className="h-4 w-4" />
                  <span className="max-w-full truncate text-[10px] font-medium">
                    {lang === "zh" ? meta.shortZh : meta.shortEn}
                  </span>
                </div>
              );
            })}
            {unpinned.length > 0 && (
              <div className="flex flex-1 flex-col items-center justify-center gap-0.5 py-1.5 text-zinc-400 dark:text-zinc-500">
                <Ellipsis className="h-4 w-4" />
                <span className="max-w-full truncate text-[10px] font-medium">
                  {tr(lang, "more")}
                </span>
              </div>
            )}
          </div>
        </div>

        {/* Scrollable Pinned & Available Lists */}
        <div className="mt-4 flex-1 overflow-y-auto px-4 pb-3">
          <div className="mb-2 text-xs font-semibold text-zinc-600 dark:text-zinc-300">
            {tr(lang, "pinnedSection")}
          </div>
          <div className="space-y-1.5">
            {pinned.map((id, idx) => {
              const meta = VIEW_MAP.get(id);
              if (!meta) return null;
              return (
                <div
                  key={id}
                  draggable
                  onDragStart={() => setDragIndex(idx)}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => {
                    if (dragIndex !== null && dragIndex !== idx) {
                      onChange(reorderShortcut(pinned, dragIndex, idx));
                    }
                    setDragIndex(null);
                  }}
                  onDragEnd={() => setDragIndex(null)}
                  className={`flex min-h-[52px] items-center justify-between gap-2 rounded-xl border px-2.5 py-1.5 transition ${
                    dragIndex === idx
                      ? "border-orange-500/60 bg-orange-500/5"
                      : "border-zinc-200/80 bg-zinc-50/70 dark:border-zinc-800 dark:bg-zinc-900/60"
                  }`}
                >
                  <div className="flex min-w-0 items-center gap-2.5">
                    <GripVertical
                      className="h-4 w-4 shrink-0 cursor-grab text-zinc-400"
                      aria-hidden="true"
                    />
                    <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-orange-500/10 text-[#f97316]">
                      <ViewIcon id={id} className="h-4 w-4" />
                    </span>
                    <span className="truncate text-sm font-medium text-zinc-900 dark:text-zinc-100">
                      {lang === "zh" ? meta.labelZh : meta.labelEn}
                    </span>
                  </div>

                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      disabled={idx === 0}
                      onClick={() => onChange(moveShortcut(pinned, id, -1))}
                      aria-label={tr(lang, "moveLeft")}
                      className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-600 hover:bg-zinc-200/70 disabled:opacity-30 dark:text-zinc-300 dark:hover:bg-zinc-800"
                    >
                      <ArrowUp className="h-4 w-4" />
                    </button>
                    <button
                      type="button"
                      disabled={idx === pinned.length - 1}
                      onClick={() => onChange(moveShortcut(pinned, id, 1))}
                      aria-label={tr(lang, "moveRight")}
                      className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-600 hover:bg-zinc-200/70 disabled:opacity-30 dark:text-zinc-300 dark:hover:bg-zinc-800"
                    >
                      <ArrowDown className="h-4 w-4" />
                    </button>
                    <button
                      type="button"
                      disabled={!canUnpin}
                      onClick={() => onChange(toggleShortcutPin(pinned, id))}
                      aria-label={tr(lang, "unpinAction")}
                      className="flex h-9 w-9 items-center justify-center rounded-lg text-zinc-500 hover:bg-red-500/10 hover:text-red-500 disabled:opacity-30"
                    >
                      <PinOff className="h-4 w-4" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          {unpinned.length > 0 && (
            <>
              <div className="mb-2 mt-4 text-xs font-semibold text-zinc-600 dark:text-zinc-300">
                {tr(lang, "availableSection")}
              </div>
              <div className="space-y-1.5">
                {unpinned.map((id) => {
                  const meta = VIEW_MAP.get(id);
                  if (!meta) return null;
                  return (
                    <div
                      key={id}
                      className="flex min-h-[52px] items-center justify-between gap-2 rounded-xl border border-dashed border-zinc-200 px-3 py-1.5 dark:border-zinc-800"
                    >
                      <div className="flex min-w-0 items-center gap-2.5">
                        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-zinc-100 text-zinc-500 dark:bg-zinc-800 dark:text-zinc-400">
                          <ViewIcon id={id} className="h-4 w-4" />
                        </span>
                        <span className="truncate text-sm text-zinc-700 dark:text-zinc-300">
                          {lang === "zh" ? meta.labelZh : meta.labelEn}
                        </span>
                      </div>
                      <button
                        type="button"
                        disabled={!canPinMore}
                        onClick={() => onChange(toggleShortcutPin(pinned, id))}
                        className="flex h-9 items-center gap-1 rounded-lg bg-orange-500/10 px-3 text-xs font-semibold text-[#f97316] transition active:scale-95 disabled:opacity-40"
                      >
                        <Pin className="h-3.5 w-3.5" />
                        {tr(lang, "pinAction")}
                      </button>
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </div>

        {/* Footer Actions */}
        <div className="flex items-center justify-between gap-2 border-t border-zinc-200/80 px-4 pb-[calc(1rem+env(safe-area-inset-bottom))] pt-3 dark:border-zinc-800">
          <button
            type="button"
            onClick={() => onChange(resetShortcuts())}
            className="flex h-11 items-center gap-1.5 rounded-xl border border-zinc-200 px-3.5 text-xs font-medium text-zinc-700 transition active:scale-95 dark:border-zinc-700 dark:text-zinc-300"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            {tr(lang, "resetDefault")}
          </button>
          <button
            type="button"
            onClick={onClose}
            className="flex h-11 flex-1 items-center justify-center rounded-xl bg-[#f97316] px-4 text-sm font-semibold text-white transition active:scale-[0.98]"
          >
            {tr(lang, "doneBtn")}
          </button>
        </div>
      </div>
    </div>
  );
}
