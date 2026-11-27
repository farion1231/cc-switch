import { ChevronRight, LogOut, SlidersHorizontal } from "lucide-react";
import { tr, type MobileLang } from "../i18n";

export type MobileTheme = "system" | "light" | "dark";
export type RefreshIntervalMs = 0 | 5000 | 15000 | 30000;

interface PreferencesViewProps {
  lang: MobileLang;
  theme: MobileTheme;
  refreshMs: RefreshIntervalMs;
  pinnedCount: number;
  maxPinned: number;
  onChangeLang: (lang: MobileLang) => void;
  onChangeTheme: (theme: MobileTheme) => void;
  onChangeRefreshMs: (ms: RefreshIntervalMs) => void;
  onOpenCustomize: () => void;
  onSignOut: () => void;
}

export function PreferencesView({
  lang,
  theme,
  refreshMs,
  pinnedCount,
  maxPinned,
  onChangeLang,
  onChangeTheme,
  onChangeRefreshMs,
  onOpenCustomize,
  onSignOut,
}: PreferencesViewProps) {
  return (
    <div className="space-y-4">
      {/* Shortcut Bar Customization Entry */}
      <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
        <h2 className="mb-2.5 text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">
          {tr(lang, "shortcutBarSection")}
        </h2>
        <button
          type="button"
          onClick={onOpenCustomize}
          className="flex min-h-[52px] w-full items-center justify-between gap-3 rounded-xl bg-zinc-50 px-3.5 py-2.5 text-left transition active:scale-[0.99] dark:bg-zinc-900/70"
        >
          <div className="flex min-w-0 items-center gap-3">
            <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-orange-500/15 text-[#f97316]">
              <SlidersHorizontal className="h-4 w-4" />
            </span>
            <div className="min-w-0">
              <div className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">
                {tr(lang, "customizeBarBtn")}
              </div>
              <div className="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">
                {tr(lang, "customizeBarSub")}
              </div>
            </div>
          </div>
          <span className="flex shrink-0 items-center gap-1 text-xs font-medium tabular-nums text-zinc-400">
            {pinnedCount}/{maxPinned}
            <ChevronRight className="h-4 w-4" />
          </span>
        </button>
      </section>

      {/* Appearance & Language */}
      <section className="space-y-4 rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
        <div>
          <div className="mb-2 text-xs font-semibold text-zinc-600 dark:text-zinc-300">
            {tr(lang, "appearance")}
          </div>
          <div className="grid grid-cols-3 rounded-xl bg-zinc-100 p-1 dark:bg-zinc-900">
            {(
              [
                ["system", tr(lang, "themeSystem")],
                ["light", tr(lang, "themeLight")],
                ["dark", tr(lang, "themeDark")],
              ] as const
            ).map(([val, label]) => (
              <button
                key={val}
                type="button"
                onClick={() => onChangeTheme(val)}
                className={`min-h-[38px] rounded-lg text-xs font-semibold transition ${
                  theme === val
                    ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-800 dark:text-zinc-100"
                    : "text-zinc-500 dark:text-zinc-400"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="mb-2 text-xs font-semibold text-zinc-600 dark:text-zinc-300">
            {tr(lang, "language")}
          </div>
          <div className="grid grid-cols-2 rounded-xl bg-zinc-100 p-1 dark:bg-zinc-900">
            {(
              [
                ["zh", "简体中文"],
                ["en", "English"],
              ] as const
            ).map(([val, label]) => (
              <button
                key={val}
                type="button"
                onClick={() => onChangeLang(val)}
                className={`min-h-[38px] rounded-lg text-xs font-semibold transition ${
                  lang === val
                    ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-800 dark:text-zinc-100"
                    : "text-zinc-500 dark:text-zinc-400"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        <div>
          <div className="mb-2 text-xs font-semibold text-zinc-600 dark:text-zinc-300">
            {tr(lang, "autoRefresh")}
          </div>
          <div className="grid grid-cols-4 rounded-xl bg-zinc-100 p-1 dark:bg-zinc-900">
            {(
              [
                [0, tr(lang, "refreshOff")],
                [5000, "5s"],
                [15000, "15s"],
                [30000, "30s"],
              ] as const
            ).map(([val, label]) => (
              <button
                key={val}
                type="button"
                onClick={() => onChangeRefreshMs(val)}
                className={`min-h-[38px] rounded-lg text-xs font-semibold tabular-nums transition ${
                  refreshMs === val
                    ? "bg-white text-zinc-900 shadow-sm dark:bg-zinc-800 dark:text-zinc-100"
                    : "text-zinc-500 dark:text-zinc-400"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>
      </section>

      {/* Sign out */}
      <section className="rounded-2xl border border-zinc-200/80 bg-white p-4 dark:border-zinc-800 dark:bg-[#232328]">
        <button
          type="button"
          onClick={onSignOut}
          className="flex min-h-[44px] w-full items-center justify-center gap-2 rounded-xl bg-red-500/10 text-sm font-semibold text-red-600 transition active:scale-[0.99] dark:text-red-400"
        >
          <LogOut className="h-4 w-4" />
          {tr(lang, "signOutBtn")}
        </button>
      </section>
    </div>
  );
}
