import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { AppWindow, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { AppGlyph } from "@/components/shell/AppGlyph";
import {
  quotaFailureReason,
  tierLabel,
  TIER_I18N_KEYS,
} from "@/components/SubscriptionQuotaFooter";
import { planLine, toQuotaTier } from "@/components/UsageFooter";
import { TONE_FILL, TONE_TEXT, useNow } from "@/components/quota/QuotaLines";
import {
  countdownStr,
  formatRelativeTime,
  resetCreditsLine,
  toneForLeft,
  type QuotaTone,
} from "@/components/quota/quotaRules";
import { ProviderIcon } from "@/components/ProviderIcon";
import { useTheme } from "@/components/theme-provider";
import { fmtUsd } from "@/components/usage/format";
import { usageApi } from "@/lib/api/usage";
import type { ManagedAuthProvider } from "@/lib/api/auth";
import {
  trayPanelApi,
  TRAY_PANEL_SHOWN_EVENT,
  TRAY_PANEL_UPDATED_EVENT,
  type TrayPanelAccount,
  type TrayPanelApp,
} from "@/lib/api/trayPanel";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";
import type { UsageResult } from "@/types";

type Range = "day" | "month" | "total";

/** 和主界面 ThemeProvider 的 storageKey 一致 */
const THEME_STORAGE_KEY = "cc-switch-theme";

const RANGES: Range[] = ["day", "month", "total"];

function rangeStart(range: Range): number {
  const now = new Date();
  if (range === "day") {
    return Math.floor(
      new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() /
        1000,
    );
  }
  if (range === "month") {
    return Math.floor(
      new Date(now.getFullYear(), now.getMonth(), 1).getTime() / 1000,
    );
  }
  return 0;
}

// ─── 额度行 ────────────────────────────────────────────────────────────────────

interface PanelRow {
  key: string;
  label: string;
  value: string;
  /** 条的长度（剩余百分比）；余额没有总额时为空，不画条 */
  percent: number | null;
  tone: QuotaTone;
  note?: string;
  /** 悬停说明 */
  title?: string;
  /** 占满一整行（重置次数那一行） */
  wide?: boolean;
}

type PanelQuota =
  | { kind: "rows"; rows: PanelRow[] }
  | { kind: "failed"; reason: string }
  | null;

function tierRow(t: TFunction, tier: QuotaTier, label: string): PanelRow {
  const left = Math.max(0, Math.round(100 - (tier.utilization ?? 0)));
  const countdown = countdownStr(tier.resetsAt);
  return {
    key: tier.name,
    label,
    value: left <= 0 ? t("quota.usedUp") : t("quota.left", { value: left }),
    percent: left,
    tone: toneForLeft(left),
    note: countdown ? t("trayPanel.resetIn", { time: countdown }) : undefined,
  };
}

function subscriptionPanelQuota(
  t: TFunction,
  quota: SubscriptionQuota | null | undefined,
  locale: string,
): PanelQuota {
  if (
    !quota ||
    quota.credentialStatus === "not_found" ||
    quota.credentialStatus === "parse_error"
  ) {
    return null;
  }
  if (!quota.success) {
    return { kind: "failed", reason: quotaFailureReason(t, quota) };
  }
  const known = (quota.tiers || []).filter(
    (tier) => tier.name in TIER_I18N_KEYS,
  );
  // 面板里总的每周额度放在按模型分的每周额度（Fable / Opus）后面
  const rows = [
    ...known.filter((tier) => tier.name !== "seven_day"),
    ...known.filter((tier) => tier.name === "seven_day"),
  ].map((tier) => tierRow(t, tier, tierLabel(t, tier.name)));
  // 和卡片 / 授权中心一样（quotaRows）：一档都没有时重置次数也不单独出来
  if (rows.length === 0) return null;
  const resets = resetCreditsRow(t, quota, locale);
  return { kind: "rows", rows: resets ? [...rows, resets] : rows };
}

/**
 * ChatGPT 订阅存下的限额重置：次数用 resetCreditsLine（和软件里同一套去过期、文案、
 * 快到期加深），下面逐次写剩多久到期，先到期的在前。
 */
function resetCreditsRow(
  t: TFunction,
  quota: SubscriptionQuota,
  locale: string,
): PanelRow | null {
  const line = resetCreditsLine(t, quota.resetCredits, { locale });
  if (!line) return null;
  const now = Date.now();
  const note = (quota.resetCredits?.expiresAt ?? [])
    .filter((at) => !at || !(Date.parse(at) <= now))
    .map((at) => countdownStr(at, now) ?? t("quota.resetCredits.noExpiry"))
    .join(" · ");
  return {
    key: line.key,
    label: t("quota.resetCredits.label"),
    value: line.value ?? line.text,
    percent: null,
    tone: line.tone,
    note,
    title: line.detail,
    wide: true,
  };
}

function scriptPanelQuota(
  t: TFunction,
  usage: UsageResult | null,
  tokenPlan: boolean,
): PanelQuota {
  if (!usage) return null;
  if (!usage.success) {
    return { kind: "failed", reason: usage.error || t("usage.queryFailed") };
  }
  const list = usage.data || [];
  if (tokenPlan) {
    const rows = list.map((data, index) => {
      const tier = toQuotaTier(data);
      return {
        ...tierRow(t, tier, tierLabel(t, tier.name)),
        key: `${index}-${tier.name}`,
      };
    });
    return rows.length > 0 ? { kind: "rows", rows } : null;
  }
  const rows = list.flatMap((data, index): PanelRow[] => {
    const line = planLine(t, data, index);
    if (!line) return [];
    return [
      {
        key: line.key,
        label: data.planName || t("usage.planUsage"),
        value: line.text,
        percent: Number.isFinite(line.left)
          ? Math.max(0, Math.min(100, line.left))
          : null,
        tone: line.tone,
      },
    ];
  });
  return rows.length > 0 ? { kind: "rows", rows } : null;
}

/**
 * 两列排，落单的占满整行：本身要占满的（重置次数）照旧；夹在它们之间的一段额度条是奇数个时，
 * 最后一个占满，不留半行空白。
 */
function fullWidthRows(rows: PanelRow[]): boolean[] {
  const full = rows.map((row) => Boolean(row.wide));
  let runStart = 0;
  rows.forEach((row, index) => {
    if (row.wide) {
      runStart = index + 1;
      return;
    }
    const runEnds = index === rows.length - 1 || rows[index + 1].wide;
    if (runEnds && (index - runStart + 1) % 2 === 1) full[index] = true;
  });
  return full;
}

function QuotaGrid({ quota }: { quota: PanelQuota }) {
  const { t } = useTranslation();
  if (!quota) return null;
  if (quota.kind === "failed") {
    return (
      <p className="mt-2 text-caption">
        <span className="font-medium text-danger-text">
          {t("quota.failed")}
        </span>{" "}
        <span className="text-fg-3">{quota.reason}</span>
      </p>
    );
  }
  const full = fullWidthRows(quota.rows);
  return (
    <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-2.5">
      {quota.rows.map((row, index) => (
        <div
          key={row.key}
          title={row.title}
          className={cn("min-w-0", full[index] && "col-span-2")}
        >
          <div className="flex items-baseline justify-between gap-2 text-caption">
            <span className="truncate text-fg-2">{row.label}</span>
            <span
              className={cn(
                "shrink-0 whitespace-nowrap tabular-nums",
                row.tone === "normal" ? "text-fg-1" : TONE_TEXT[row.tone],
              )}
            >
              {row.value}
            </span>
          </div>
          {row.percent !== null && (
            <div
              role="meter"
              aria-label={`${row.label}: ${row.value}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={Math.round(row.percent)}
              className="relative mt-1 h-1.5 overflow-hidden rounded-full bg-chart-grid"
            >
              <span
                className={cn(
                  "absolute inset-y-0 start-0 rounded-full",
                  TONE_FILL[row.tone],
                )}
                style={{ width: `${row.percent}%` }}
              />
            </div>
          )}
          {row.note && (
            <div className="mt-1 truncate text-badge font-normal text-fg-3 tabular-nums">
              {row.note}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}

function UpdatedAt({ at }: { at: number | null | undefined }) {
  const { t } = useTranslation();
  const now = useNow(Boolean(at));
  if (!at) return null;
  return (
    <span className="text-badge font-normal text-fg-3">
      {t("quota.updatedAt", { time: formatRelativeTime(at, now, t) })}
    </span>
  );
}

// ─── 应用分区 ──────────────────────────────────────────────────────────────────

/** 邮箱只露首尾：j***3@gmail.com */
function maskLogin(login: string): string {
  const at = login.indexOf("@");
  if (at <= 2) return login;
  const name = login.slice(0, at);
  return `${name[0]}***${name[name.length - 1]}${login.slice(at)}`;
}

function SectionHeader({
  icon,
  title,
  right,
}: {
  icon: ReactNode;
  title: string;
  right?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      {icon}
      <span className="truncate text-strong text-fg-1">{title}</span>
      <span className="ms-auto min-w-0 truncate text-caption text-fg-3">
        {right}
      </span>
    </div>
  );
}

function AppSection({ app }: { app: TrayPanelApp }) {
  const { t, i18n } = useTranslation();
  const quota =
    app.usageKind === "script"
      ? scriptPanelQuota(t, app.script, app.tokenPlan)
      : subscriptionPanelQuota(t, app.subscription, i18n.language);
  return (
    <section className="px-4 py-3">
      <SectionHeader
        icon={<AppGlyph app={app.appType} size={16} />}
        title={app.appName}
        right={app.providerName}
      />
      <UpdatedAt at={app.subscription?.queriedAt} />
      {app.usageKind === null ? (
        <p className="mt-1 text-caption text-fg-3">
          {t("trayPanel.noUsageQuery")}
        </p>
      ) : (
        <QuotaGrid quota={quota} />
      )}
    </section>
  );
}

// ─── 授权中心的账号 ────────────────────────────────────────────────────────────

/** 授权中心里登录的账号分组，顺序和授权中心一致 */
const AUTH_GROUPS: {
  provider: ManagedAuthProvider;
  title: string;
  icon: string;
}[] = [
  {
    provider: "github_copilot",
    title: "GitHub Copilot",
    icon: "githubcopilot",
  },
  { provider: "codex_oauth", title: "ChatGPT", icon: "openai" },
  { provider: "xai_oauth", title: "xAI", icon: "xai" },
];

function needsReauth({ account }: TrayPanelAccount): boolean {
  return Boolean(account.reauth_required || account.requires_reauth);
}

function AccountRow({
  entry,
  inUseBy,
}: {
  entry: TrayPanelAccount;
  /** 正在用这个账号的应用名（Codex 绑定的托管账号） */
  inUseBy?: string;
}) {
  const { t, i18n } = useTranslation();
  const { account, quota } = entry;
  return (
    <div className="mt-2.5">
      <div className="flex items-center gap-1.5 text-caption">
        <span className="truncate text-fg-1" title={account.login}>
          {maskLogin(account.login)}
        </span>
        {inUseBy && (
          <span
            className="shrink-0 text-fg-2"
            title={t("trayPanel.inUseBy", { app: inUseBy })}
            aria-label={t("trayPanel.inUseBy", { app: inUseBy })}
          >
            ✓
          </span>
        )}
      </div>
      {needsReauth(entry) ? (
        <p className="mt-1 text-caption font-medium text-danger-text">
          {t("trayPanel.reauthRequired")}
        </p>
      ) : (
        <>
          <UpdatedAt at={quota?.queriedAt} />
          <QuotaGrid quota={subscriptionPanelQuota(t, quota, i18n.language)} />
        </>
      )}
    </div>
  );
}

function AuthGroupSection({
  title,
  icon,
  entries,
  codexAccountId,
}: {
  title: string;
  icon: string;
  entries: TrayPanelAccount[];
  codexAccountId: string | null;
}) {
  const { t } = useTranslation();
  if (entries.length === 0) return null;
  return (
    <section className="px-4 py-3">
      <SectionHeader
        icon={<ProviderIcon icon={icon} name={title} size={16} />}
        title={title}
        right={t("trayPanel.accounts", { count: entries.length })}
      />
      {entries.map((entry) => (
        <AccountRow
          key={entry.account.id}
          entry={entry}
          inUseBy={
            entry.provider === "codex_oauth" &&
            entry.account.id === codexAccountId
              ? "Codex"
              : undefined
          }
        />
      ))}
    </section>
  );
}

// ─── 面板 ──────────────────────────────────────────────────────────────────────

export function TrayPanel() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const rootRef = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState<Range>("day");

  const summary = useQuery({
    queryKey: ["tray-panel", "summary", range],
    queryFn: () =>
      usageApi.getUsageSummary(
        rangeStart(range),
        Math.floor(Date.now() / 1000),
      ),
    refetchInterval: 60_000,
  });
  // 只读后端缓存，不发请求：额度由后台每 5 分钟查一次
  const snapshot = useQuery({
    queryKey: ["tray-panel", "snapshot"],
    queryFn: () => trayPanelApi.getSnapshot(),
  });
  // 手动刷新才立刻查一遍
  const refresh = useMutation({
    mutationFn: () => trayPanelApi.refresh(),
    onSuccess: (data) => {
      queryClient.setQueryData(["tray-panel", "snapshot"], data);
    },
  });

  // 面板只隐藏不销毁，主界面切过深浅色后这里的主题还是建窗口时读的那个：
  // 弹出前、以及别的窗口改了主题时，按主界面存的主题重新套一遍（也同步原生窗口外观，玻璃跟着变）
  const { setTheme } = useTheme();
  useEffect(() => {
    const syncTheme = () => {
      const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
      if (stored === "light" || stored === "dark" || stored === "system") {
        setTheme(stored);
      }
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY) syncTheme();
    };
    window.addEventListener("storage", onStorage);
    const unlisten = listen(TRAY_PANEL_SHOWN_EVENT, syncTheme);
    return () => {
      window.removeEventListener("storage", onStorage);
      void unlisten.then((off) => off());
    };
  }, [setTheme]);

  // 弹出时、后台查完时重读缓存（都是本地读，不发额度请求）
  useEffect(() => {
    const reload = () =>
      void queryClient.invalidateQueries({ queryKey: ["tray-panel"] });
    const shown = listen(TRAY_PANEL_SHOWN_EVENT, reload);
    const updated = listen(TRAY_PANEL_UPDATED_EVENT, reload);
    return () => {
      void shown.then((off) => off());
      void updated.then((off) => off());
    };
  }, [queryClient]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") void trayPanelApi.hide();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 窗口高度跟着内容走
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const report = () => void trayPanelApi.setHeight(el.scrollHeight);
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Codex 绑的是授权中心的账号时，额度在 ChatGPT 账号那组里写（打 ✓），这里不重复
  const apps = snapshot.data?.apps ?? [];
  const accounts = snapshot.data?.accounts ?? [];
  const codexAccountId =
    apps.find(
      (app) => app.appType === "codex" && app.usageKind === "managedCodex",
    )?.accountId ?? null;
  const appSections = apps.filter(
    (app) => !(app.appType === "codex" && app.usageKind === "managedCodex"),
  );
  const totalTokens = summary.data?.realTotalTokens;
  const refreshing = refresh.isPending;

  return (
    <div
      ref={rootRef}
      className="flex max-h-[720px] flex-col overflow-y-auto text-fg-1"
    >
      <header className="flex items-center justify-between px-4 pt-3.5">
        <span className="text-strong font-semibold text-fg-2">CC Switch</span>
        <div
          role="tablist"
          aria-label={t("trayPanel.range")}
          className="flex rounded-control bg-black/[0.06] p-0.5 dark:bg-white/[0.08]"
        >
          {RANGES.map((key) => (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={range === key}
              onClick={() => setRange(key)}
              className={cn(
                "rounded-[5px] px-2.5 py-0.5 text-badge transition-colors",
                range === key
                  ? "bg-white/80 text-fg-1 shadow-sm dark:bg-white/[0.16]"
                  : "text-fg-3 hover:text-fg-1",
              )}
            >
              {t(`trayPanel.ranges.${key}`)}
            </button>
          ))}
        </div>
      </header>

      <section className="px-4 pb-3 pt-3">
        <div className="text-caption text-fg-3">
          {t("trayPanel.totalTokens")}
        </div>
        <div className="mt-0.5 text-[30px] font-semibold leading-9 tracking-tight tabular-nums">
          {totalTokens == null
            ? "--"
            : new Intl.NumberFormat(i18n.language).format(totalTokens)}
        </div>
        <div className="mt-0.5 flex items-center gap-2 text-caption text-fg-2 tabular-nums">
          <span>{fmtUsd(summary.data?.totalCost, 2)}</span>
          {summary.data && (
            <span className="text-fg-3">
              {t("trayPanel.requests", {
                count: summary.data.totalRequests,
              })}
            </span>
          )}
        </div>
      </section>

      <div className="divide-y divide-black/[0.08] border-t border-black/[0.08] dark:divide-white/[0.1] dark:border-white/[0.1]">
        {appSections.map((app) => (
          <AppSection key={app.appType} app={app} />
        ))}
        {AUTH_GROUPS.map(({ provider, title, icon }) => (
          <AuthGroupSection
            key={provider}
            title={title}
            icon={icon}
            entries={accounts.filter((entry) => entry.provider === provider)}
            codexAccountId={codexAccountId}
          />
        ))}
      </div>

      <footer className="flex items-center gap-2 border-t border-black/[0.08] px-3 dark:border-white/[0.1] py-2.5">
        <button
          type="button"
          onClick={() => void trayPanelApi.openMain()}
          className="flex items-center gap-1.5 rounded-control px-2 py-1 text-caption text-fg-2 hover:bg-black/[0.06] dark:hover:bg-white/[0.08] hover:text-fg-1"
        >
          <AppWindow className="h-3.5 w-3.5" strokeWidth={1.5} />
          {t("trayPanel.openMain")}
        </button>
        <span className="ms-auto text-badge font-normal text-fg-3">
          {t("trayPanel.rightClickHint")}
        </span>
        <button
          type="button"
          aria-label={t("trayPanel.refresh")}
          title={t("trayPanel.refresh")}
          disabled={refreshing}
          onClick={() => {
            refresh.mutate();
            void queryClient.invalidateQueries({
              queryKey: ["tray-panel", "summary"],
            });
          }}
          className="rounded-control p-1.5 text-fg-2 hover:bg-black/[0.06] dark:hover:bg-white/[0.08] hover:text-fg-1 disabled:opacity-50"
        >
          <RefreshCw
            className={cn("h-3.5 w-3.5", refreshing && "animate-spin")}
            strokeWidth={1.5}
          />
        </button>
      </footer>
    </div>
  );
}
