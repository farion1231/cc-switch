import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
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
  toneForLeft,
  type QuotaTone,
} from "@/components/quota/quotaRules";
import { fmtUsd } from "@/components/usage/format";
import { usageApi } from "@/lib/api/usage";
import { authListAccounts, type ManagedAuthAccount } from "@/lib/api/auth";
import {
  trayPanelApi,
  TRAY_PANEL_SHOWN_EVENT,
  type TrayPanelApp,
} from "@/lib/api/trayPanel";
import { useCodexOauthQuotaByAccountId } from "@/lib/query/subscription";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";
import type { UsageResult } from "@/types";

type Range = "day" | "month" | "total";
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
  const rows = (quota.tiers || [])
    .filter((tier) => tier.name in TIER_I18N_KEYS)
    .map((tier) => tierRow(t, tier, tierLabel(t, tier.name)));
  return rows.length > 0 ? { kind: "rows", rows } : null;
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
  const single = quota.rows.length === 1;
  return (
    <div
      className={cn(
        "mt-2 grid gap-x-4 gap-y-2.5",
        single ? "grid-cols-1" : "grid-cols-2",
      )}
    >
      {quota.rows.map((row) => (
        <div key={row.key} className="min-w-0">
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
            <div className="mt-1 text-badge font-normal text-fg-3">
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
  app,
  title,
  right,
}: {
  app: TrayPanelApp;
  title?: string;
  right?: ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <AppGlyph app={app.appType} size={16} />
      <span className="truncate text-strong text-fg-1">
        {title ?? app.appName}
      </span>
      <span className="ms-auto min-w-0 truncate text-caption text-fg-3">
        {right}
      </span>
    </div>
  );
}

function AppSection({ app }: { app: TrayPanelApp }) {
  const { t } = useTranslation();
  const quota =
    app.usageKind === "script"
      ? scriptPanelQuota(t, app.script, app.tokenPlan)
      : subscriptionPanelQuota(t, app.subscription);
  return (
    <section className="px-4 py-3">
      <SectionHeader app={app} right={app.providerName} />
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

function CodexAccount({
  account,
  current,
}: {
  account: ManagedAuthAccount;
  current: boolean;
}) {
  const { t } = useTranslation();
  const { data } = useCodexOauthQuotaByAccountId(account.id, {
    enabled: true,
    autoQuery: false,
  });
  return (
    <div className="mt-2.5">
      <div className="flex items-center gap-1.5 text-caption">
        <span className="truncate text-fg-1" title={account.login}>
          {maskLogin(account.login)}
        </span>
        {current && (
          <span className="text-fg-2" aria-label={t("trayPanel.inUse")}>
            ✓
          </span>
        )}
      </div>
      <UpdatedAt at={data?.queriedAt} />
      <QuotaGrid quota={subscriptionPanelQuota(t, data)} />
    </div>
  );
}

/** Codex 绑了 cc-switch 自管账号时，把所有账号的额度都列出来 */
function CodexAccountsSection({
  app,
  accounts,
}: {
  app: TrayPanelApp;
  accounts: ManagedAuthAccount[];
}) {
  const { t } = useTranslation();
  return (
    <section className="px-4 py-3">
      <SectionHeader
        app={app}
        right={t("trayPanel.accounts", { count: accounts.length })}
      />
      {accounts.map((account) => (
        <CodexAccount
          key={account.id}
          account={account}
          current={account.id === app.accountId}
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
  const apps = useQuery({
    queryKey: ["tray-panel", "apps"],
    queryFn: () => trayPanelApi.refreshApps(),
  });
  const codexAccounts = useQuery({
    queryKey: ["tray-panel", "codex-accounts"],
    queryFn: () => authListAccounts("codex_oauth"),
  });

  // 每次弹出都重查：面板只隐藏不销毁
  useEffect(() => {
    const unlisten = listen(TRAY_PANEL_SHOWN_EVENT, () => {
      void queryClient.invalidateQueries({ queryKey: ["tray-panel"] });
      // 各账号额度有自己的 5 分钟缓存，超过一分钟的才重查
      void queryClient.invalidateQueries({
        queryKey: ["codex_oauth", "quota"],
        predicate: (query) => Date.now() - query.state.dataUpdatedAt > 60_000,
      });
    });
    return () => {
      void unlisten.then((off) => off());
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

  const accounts = codexAccounts.data ?? [];
  const totalTokens = summary.data?.realTotalTokens;
  const refreshing = apps.isFetching || summary.isFetching;

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
        {(apps.data ?? []).map((app) =>
          app.appType === "codex" &&
          app.usageKind === "managedCodex" &&
          accounts.length > 0 ? (
            <CodexAccountsSection
              key={app.appType}
              app={app}
              accounts={accounts}
            />
          ) : (
            <AppSection key={app.appType} app={app} />
          ),
        )}
        {apps.data?.length === 0 && (
          <p className="px-4 py-3 text-caption text-fg-3">
            {t("trayPanel.empty")}
          </p>
        )}
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
            void queryClient.invalidateQueries({ queryKey: ["tray-panel"] });
            void queryClient.invalidateQueries({
              queryKey: ["codex_oauth", "quota"],
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
