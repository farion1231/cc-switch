import { useCallback, useEffect, useRef, useState } from "react";
import { Lock, RefreshCw, ShieldCheck } from "lucide-react";
import {
  ApiError,
  consumeFragmentToken,
  mobileApi,
  type AuthStatus,
  type OverviewResponse,
  type ProvidersListResponse,
  type ProxyStatusApiResponse,
  type SafePaginatedLogs,
} from "./api";
import { BottomShortcutBar } from "./components/BottomShortcutBar";
import { CustomizeBarSheet } from "./components/CustomizeBarSheet";
import { MoreSheet } from "./components/MoreSheet";
import { SignInScreen } from "./components/SignInScreen";
import { loadMobileLang, saveMobileLang, tr, type MobileLang } from "./i18n";
import {
  ALL_MOBILE_VIEWS,
  loadShortcuts,
  maxPinnedShortcuts,
  saveShortcuts,
  type MobileViewId,
} from "./shortcuts";
import { LogsView } from "./views/LogsView";
import { ModelsView } from "./views/ModelsView";
import { OverviewView } from "./views/OverviewView";
import {
  PreferencesView,
  type MobileTheme,
  type RefreshIntervalMs,
} from "./views/PreferencesView";
import { ProvidersView } from "./views/ProvidersView";
import { ProxyView } from "./views/ProxyView";

const THEME_KEY = "cc-switch-mobile-theme";
const REFRESH_KEY = "cc-switch-mobile-refresh";
const VIEW_MAP = new Map(ALL_MOBILE_VIEWS.map((v) => [v.id, v]));

function loadTheme(): MobileTheme {
  try {
    const v = window.localStorage.getItem(THEME_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    // ignore
  }
  return "system";
}

function loadRefreshMs(): RefreshIntervalMs {
  try {
    const n = Number(window.localStorage.getItem(REFRESH_KEY));
    if (n === 0 || n === 5000 || n === 15000 || n === 30000) return n;
  } catch {
    // ignore
  }
  return 15000;
}

export function MobileApp() {
  const [lang, setLang] = useState<MobileLang>(loadMobileLang);
  const [theme, setTheme] = useState<MobileTheme>(loadTheme);
  const [refreshMs, setRefreshMs] = useState<RefreshIntervalMs>(loadRefreshMs);
  const [pinned, setPinned] = useState<MobileViewId[]>(() => loadShortcuts());
  const [activeView, setActiveView] = useState<MobileViewId>(() => {
    const initial = loadShortcuts();
    return initial[0] ?? "overview";
  });
  const [moreOpen, setMoreOpen] = useState(false);
  const [customizeOpen, setCustomizeOpen] = useState(false);

  // Auth state
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [authLoading, setAuthLoading] = useState(true);
  const [authError, setAuthError] = useState<string | null>(null);
  const [signingIn, setSigningIn] = useState(false);

  // View data
  const [days, setDays] = useState<1 | 7 | 30>(1);
  const [overview, setOverview] = useState<OverviewResponse | null>(null);
  const [providers, setProviders] = useState<ProvidersListResponse | null>(
    null,
  );
  const [logs, setLogs] = useState<SafePaginatedLogs | null>(null);
  const [logsPage, setLogsPage] = useState(1);
  const [logsStatusFilter, setLogsStatusFilter] = useState<
    "all" | "ok" | "err"
  >("all");
  const [proxy, setProxy] = useState<ProxyStatusApiResponse | null>(null);

  const [loading, setLoading] = useState(false);
  const [viewError, setViewError] = useState<string | null>(null);
  const [switchingKey, setSwitchingKey] = useState<string | null>(null);
  const [proxyMutating, setProxyMutating] = useState(false);
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const toastTimerRef = useRef<number | null>(null);
  const langRef = useRef<MobileLang>(lang);
  langRef.current = lang;

  const showToast = useCallback((msg: string) => {
    setToastMsg(msg);
    if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setToastMsg(null), 2600);
  }, []);

  // Apply light/dark theme to <html>
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && mq.matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    mq.addEventListener("change", apply);
    try {
      window.localStorage.setItem(THEME_KEY, theme);
    } catch {
      // ignore
    }
    return () => mq.removeEventListener("change", apply);
  }, [theme]);

  // Bootstrap auth once on mount: exchange fragment token `#t=` if present, else check session cookie.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const fragToken = consumeFragmentToken();
        if (fragToken) {
          const res = await mobileApi.createSession(fragToken);
          if (!cancelled) {
            setAuth(res);
            setAuthError(null);
          }
          return;
        }
        const st = await mobileApi.getAuthStatus();
        if (!cancelled) {
          setAuth(st);
          setAuthError(null);
        }
      } catch (err) {
        if (!cancelled) {
          setAuth({ authenticated: false, allowControl: false, version: "" });
          if (err instanceof ApiError && err.status === 401) {
            setAuthError(tr(langRef.current, "invalidToken"));
          } else {
            setAuthError(tr(langRef.current, "networkError"));
          }
        }
      } finally {
        if (!cancelled) setAuthLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleApiError = useCallback(
    (err: unknown) => {
      if (err instanceof ApiError && err.status === 401) {
        setAuth({ authenticated: false, allowControl: false, version: "" });
        setAuthError(tr(lang, "invalidToken"));
        return;
      }
      setViewError(
        err instanceof Error ? err.message : tr(lang, "networkError"),
      );
    },
    [lang],
  );

  const syncAllowControl = useCallback((allowControl: boolean) => {
    setAuth((prev) =>
      prev && prev.allowControl !== allowControl
        ? { ...prev, allowControl }
        : prev,
    );
  }, []);

  const loadActiveView = useCallback(async () => {
    if (!auth?.authenticated) return;
    setLoading(true);
    setViewError(null);
    try {
      if (activeView === "overview" || activeView === "models") {
        const res = await mobileApi.getOverview(days);
        setOverview(res);
        syncAllowControl(res.allowControl);
      } else if (activeView === "providers") {
        const res = await mobileApi.getProviders();
        setProviders(res);
        syncAllowControl(res.allowControl);
      } else if (activeView === "logs") {
        const res = await mobileApi.getLogs({
          page: logsPage,
          pageSize: 20,
          statusClass:
            logsStatusFilter === "all" ? undefined : logsStatusFilter,
        });
        setLogs(res);
      } else if (activeView === "proxy") {
        const res = await mobileApi.getProxyStatus();
        setProxy(res);
        syncAllowControl(res.allowControl);
      }
    } catch (err) {
      handleApiError(err);
    } finally {
      setLoading(false);
    }
  }, [
    auth?.authenticated,
    activeView,
    days,
    logsPage,
    logsStatusFilter,
    handleApiError,
    syncAllowControl,
  ]);

  useEffect(() => {
    void loadActiveView();
  }, [loadActiveView]);

  // Auto-refresh polling paused whenever tab is hidden.
  useEffect(() => {
    if (
      !auth?.authenticated ||
      refreshMs === 0 ||
      activeView === "preferences"
    ) {
      return;
    }
    const id = window.setInterval(() => {
      if (!document.hidden) {
        void loadActiveView();
      }
    }, refreshMs);
    return () => window.clearInterval(id);
  }, [auth?.authenticated, refreshMs, activeView, loadActiveView]);

  const handleSignIn = async (token: string) => {
    setSigningIn(true);
    setAuthError(null);
    try {
      const res = await mobileApi.createSession(token);
      setAuth(res);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setAuthError(tr(lang, "invalidToken"));
      } else {
        setAuthError(tr(lang, "networkError"));
      }
    } finally {
      setSigningIn(false);
    }
  };

  const handleSignOut = async () => {
    try {
      await mobileApi.logout();
    } catch {
      // ignore
    }
    setAuth({ authenticated: false, allowControl: false, version: "" });
    setOverview(null);
    setProviders(null);
    setLogs(null);
    setProxy(null);
  };

  const handleSwitchProvider = async (app: string, providerId: string) => {
    const key = `${app}:${providerId}`;
    setSwitchingKey(key);
    setViewError(null);
    try {
      await mobileApi.switchProvider(app, providerId);
      showToast(tr(lang, "switchedToast"));
      const next = await mobileApi.getProviders();
      setProviders(next);
    } catch (err) {
      handleApiError(err);
    } finally {
      setSwitchingKey(null);
    }
  };

  const handleProxyToggle = async (action: "start" | "stop") => {
    setProxyMutating(true);
    setViewError(null);
    try {
      if (action === "start") {
        await mobileApi.startProxy();
      } else {
        await mobileApi.stopProxy();
      }
      const next = await mobileApi.getProxyStatus();
      setProxy(next);
    } catch (err) {
      handleApiError(err);
    } finally {
      setProxyMutating(false);
    }
  };

  const handleUpdatePinned = (next: MobileViewId[]) => {
    const saved = saveShortcuts(next);
    setPinned(saved);
  };

  const handleChangeLang = (next: MobileLang) => {
    setLang(next);
    saveMobileLang(next);
  };

  const handleChangeRefreshMs = (ms: RefreshIntervalMs) => {
    setRefreshMs(ms);
    try {
      window.localStorage.setItem(REFRESH_KEY, String(ms));
    } catch {
      // ignore
    }
  };

  if (authLoading) {
    return (
      <div className="flex min-h-[100dvh] items-center justify-center bg-[#f5f5f7] text-sm text-zinc-500 dark:bg-[#18181b] dark:text-zinc-400">
        {tr(lang, "loading")}
      </div>
    );
  }

  if (!auth?.authenticated) {
    return (
      <SignInScreen
        lang={lang}
        error={authError}
        submitting={signingIn}
        onSubmit={(token) => void handleSignIn(token)}
      />
    );
  }

  const currentMeta = VIEW_MAP.get(activeView);
  const pageTitle = currentMeta
    ? lang === "zh"
      ? currentMeta.labelZh
      : currentMeta.labelEn
    : "CC Switch";

  return (
    <div className="min-h-[100dvh] bg-[#f5f5f7] pb-[calc(76px+env(safe-area-inset-bottom))] text-zinc-900 dark:bg-[#18181b] dark:text-zinc-100">
      {/* Top Header */}
      <header className="sticky top-0 z-20 border-b border-zinc-200/80 bg-[#f5f5f7]/85 pt-[env(safe-area-inset-top)] backdrop-blur-xl dark:border-zinc-800/90 dark:bg-[#18181b]/85">
        <div className="mx-auto flex h-14 max-w-lg items-center justify-between px-4">
          <div className="flex min-w-0 items-center gap-2.5">
            <img
              src="/icons/icon-256.png"
              alt="CC Switch"
              className="h-7 w-7 rounded-lg"
            />
            <h1 className="truncate text-base font-semibold tracking-tight">
              {pageTitle}
            </h1>
          </div>

          <div className="flex items-center gap-2">
            <span
              className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium ${
                auth.allowControl
                  ? "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400"
                  : "bg-zinc-200/80 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400"
              }`}
            >
              {auth.allowControl ? (
                <ShieldCheck className="h-3 w-3" />
              ) : (
                <Lock className="h-3 w-3" />
              )}
              {auth.allowControl
                ? tr(lang, "controlEnabledBadge")
                : tr(lang, "readOnlyBadge")}
            </span>

            {activeView !== "preferences" && (
              <button
                type="button"
                onClick={() => void loadActiveView()}
                disabled={loading}
                aria-label={tr(lang, "refresh")}
                className="flex h-9 w-9 items-center justify-center rounded-full bg-white text-zinc-600 ring-1 ring-zinc-200 transition active:scale-95 dark:bg-[#232328] dark:text-zinc-300 dark:ring-zinc-800"
              >
                <RefreshCw
                  className={`h-4 w-4 ${loading ? "animate-spin" : ""}`}
                />
              </button>
            )}
          </div>
        </div>
      </header>

      {/* Toast Notification */}
      {toastMsg && (
        <div className="pointer-events-none fixed inset-x-0 top-[calc(4rem+env(safe-area-inset-top))] z-40 flex justify-center px-4">
          <div className="rounded-full bg-zinc-900 px-4 py-2 text-xs font-semibold text-white shadow-lg dark:bg-zinc-100 dark:text-zinc-900">
            {toastMsg}
          </div>
        </div>
      )}

      {/* Main Content */}
      <main className="mx-auto max-w-lg px-4 py-4">
        {activeView === "overview" && (
          <OverviewView
            lang={lang}
            days={days}
            onChangeDays={setDays}
            data={overview}
            loading={loading}
            error={viewError}
            onRetry={() => void loadActiveView()}
          />
        )}
        {activeView === "providers" && (
          <ProvidersView
            lang={lang}
            data={providers}
            loading={loading}
            error={viewError}
            switchingKey={switchingKey}
            onRetry={() => void loadActiveView()}
            onSwitch={(app, id) => void handleSwitchProvider(app, id)}
          />
        )}
        {activeView === "logs" && (
          <LogsView
            lang={lang}
            data={logs}
            page={logsPage}
            statusFilter={logsStatusFilter}
            loading={loading}
            error={viewError}
            onChangePage={setLogsPage}
            onChangeStatusFilter={(next) => {
              setLogsStatusFilter(next);
              setLogsPage(1);
            }}
            onRetry={() => void loadActiveView()}
          />
        )}
        {activeView === "proxy" && (
          <ProxyView
            lang={lang}
            data={proxy}
            loading={loading}
            mutating={proxyMutating}
            error={viewError}
            onRetry={() => void loadActiveView()}
            onStart={() => void handleProxyToggle("start")}
            onStop={() => void handleProxyToggle("stop")}
          />
        )}
        {activeView === "models" && (
          <ModelsView lang={lang} data={overview} loading={loading} />
        )}
        {activeView === "preferences" && (
          <PreferencesView
            lang={lang}
            theme={theme}
            refreshMs={refreshMs}
            pinnedCount={pinned.length}
            maxPinned={maxPinnedShortcuts()}
            onChangeLang={handleChangeLang}
            onChangeTheme={setTheme}
            onChangeRefreshMs={handleChangeRefreshMs}
            onOpenCustomize={() => setCustomizeOpen(true)}
            onSignOut={() => void handleSignOut()}
          />
        )}
      </main>

      {/* Customizable Bottom Shortcut Bar */}
      <BottomShortcutBar
        pinned={pinned}
        activeView={activeView}
        lang={lang}
        moreOpen={moreOpen}
        onSelectView={(view) => {
          setActiveView(view);
          setMoreOpen(false);
        }}
        onToggleMore={() => setMoreOpen((v) => !v)}
      />

      {/* Overflow "More" Bottom Sheet */}
      <MoreSheet
        open={moreOpen}
        pinned={pinned}
        activeView={activeView}
        lang={lang}
        onSelectView={(view) => setActiveView(view)}
        onOpenCustomize={() => setCustomizeOpen(true)}
        onClose={() => setMoreOpen(false)}
      />

      {/* Shortcut Bar Customizer Sheet */}
      <CustomizeBarSheet
        open={customizeOpen}
        pinned={pinned}
        lang={lang}
        onChange={handleUpdatePinned}
        onClose={() => setCustomizeOpen(false)}
      />
    </div>
  );
}
