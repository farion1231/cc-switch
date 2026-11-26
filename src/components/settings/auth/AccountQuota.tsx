import { useId, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  ResetSlot,
  TONE_FILL,
  TONE_TEXT,
  TonedText,
  useNow,
} from "@/components/quota/QuotaLines";
import { useQuotaDisplay } from "@/components/quota/useQuotaDisplay";
import {
  QuotaBreakdownChevron,
  QuotaBreakdownRow,
} from "@/components/quota/QuotaBreakdown";
import {
  barPercent,
  countdownStr,
  formatRelativeTime,
  lineHint,
  toneForLeft,
  type QuotaDisplay,
  type QuotaLine,
} from "@/components/quota/quotaRules";
import {
  quotaFailureReason,
  quotaRows,
} from "@/components/SubscriptionQuotaFooter";
import type { SubscriptionQuota } from "@/types/subscription";
import type { CodebuddyQuota } from "@/lib/api/auth";
import { useCopilotQuota } from "@/lib/query/copilot";
import { copilotQuotaRows } from "@/components/CopilotQuotaFooter";
import {
  useAgyAccountQuota,
  useCodebuddyAccountQuota,
  useClaudeAccountQuota,
  useKimiAccountQuota,
  useXaiOauthQuotaByAccountId,
} from "@/lib/query/subscription";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { WorkBuddyQuota } from "@/lib/api/auth";
import {
  useWorkBuddyAccountQuota,
  creditExpiryLine,
  workbuddyCreditsLine,
  workbuddyExpiryLine,
} from "@/components/providers/workbuddyQuota";

export interface AccountQuotaRow {
  label: string;
  line: QuotaLine;
}

/** 额度列要画的东西：成功按档画条；查询失败写「额度查询失败」+ 原因；查成功但没有额度时写一句灰字；null 表示不显示 */
export type AccountQuotaState =
  | { kind: "rows"; rows: AccountQuotaRow[] }
  | { kind: "failed"; reason: string }
  | { kind: "note"; text: string }
  | { kind: "loading" }
  | null;

interface AccountQuotaColumnProps {
  /** 账号名，拼进刷新按钮的无障碍名字 */
  login: string;
  state: AccountQuotaState;
  queriedAt?: number | null;
  loading: boolean;
  onRefresh: () => void;
  /** 只手动查询、还没查过：写一句「点刷新查询额度」，留着 ↻ */
  idle?: boolean;
}

/**
 * 授权中心账号行右侧的额度（v7 Auth 画板）：额度条（条和数字按设置写剩余或已用，
 * 平时 --chart-1；不到 20% 换 warning，用完换 danger）+ 右边的「x 分钟前 ↻」。
 * 条、数值、倒计时三格宽度由 .auth-quota-col 变量控制（随界面密度联动），倒计时格没有也留着，
 * 条的宽度只随分组卡片宽度变（见 index.css）；档名格按最长的档名放宽（56–112），
 * 短档名不再留出一大块空白、把账号名挤短。
 * 打开页面时查一次，不轮询；↻ 手动重查。
 */
export function AccountQuotaColumn({
  login,
  state,
  queriedAt,
  loading,
  onRefresh,
  idle = false,
}: AccountQuotaColumnProps) {
  const { t } = useTranslation();
  const now = useNow(
    Boolean(queriedAt) ||
      (state?.kind === "rows" && state.rows.some(({ line }) => line.resetsAt)),
  );
  // 只手动查询、还没查过：留一句提示和 ↻，不然这一行什么都没有，看不出能查
  if (!state && idle) {
    state = { kind: "note", text: t("quota.manualIdle") };
  }
  if (!state) return null;
  // 倒计时格始终留着（没有重置时间就空着），别的号的条才不会因此变宽或挪位
  const resetCell = (line: QuotaLine) => (
    <ResetSlot
      countdown={countdownStr(line.resetsAt, now)}
      className="w-[var(--auth-quota-reset)]"
    />
  );

  return (
    <>
      <div
        className={cn(
          "auth-quota-col flex shrink-0 flex-col gap-0.5",
          // 不是额度条时收紧到最小列宽，长原因截断，不把列撑宽
          state.kind !== "rows" && "auth-quota-col-static",
        )}
      >
        {state.kind === "rows" &&
          state.rows.map(({ label, line }) =>
            line.breakdown ? (
              <QuotaBreakdownRow
                key={line.key}
                line={line}
                breakdown={line.breakdown}
                className={ROW_CLASS}
              >
                <QuotaRowCells
                  label={label}
                  line={line}
                  trailing={<QuotaBreakdownChevron />}
                />
                {resetCell(line)}
              </QuotaBreakdownRow>
            ) : (
              <div
                key={line.key}
                title={lineHint(t, line, now)}
                className={ROW_CLASS}
              >
                <QuotaRowCells label={label} line={line} />
                {resetCell(line)}
              </div>
            ),
          )}
        {state.kind === "failed" && (
          <>
            <span className="h-[18px] text-caption font-medium text-danger-text">
              {t("quota.failed", { defaultValue: "额度查询失败" })}
            </span>
            <span
              className="truncate text-caption text-fg-3"
              title={state.reason}
            >
              {state.reason}
            </span>
          </>
        )}
        {state.kind === "note" && (
          // 写在额度行数值那一格：右边让出倒计时格和间距，和别的号的数值对齐
          <span className="me-[calc(var(--auth-quota-reset)+var(--auth-quota-gap))] max-w-full truncate self-end text-caption text-fg-3">
            {state.text}
          </span>
        )}
        {state.kind === "loading" && (
          <span className="text-caption text-fg-3">
            {t("authCenter.quota.loading", { defaultValue: "正在查询额度…" })}
          </span>
        )}
      </div>
      <QuotaRefreshCell
        login={login}
        queriedAt={queriedAt}
        loading={loading}
        onRefresh={onRefresh}
      />
    </>
  );
}

interface QuotaRefreshCellProps {
  /** 账号名，拼进刷新按钮的无障碍名字 */
  login: string;
  queriedAt?: number | null;
  loading: boolean;
  onRefresh: () => void;
}

/**
 * 额度列右边的「x 分钟前 ↻」。各服务的账号行共用，刷新按钮的位置和样式一致；
 * 不是额度条的服务（DeepSeek 账号的余额）也直接用它。
 */
export function QuotaRefreshCell({
  login,
  queriedAt,
  loading,
  onRefresh,
}: QuotaRefreshCellProps) {
  const { t } = useTranslation();
  const now = useNow(Boolean(queriedAt));
  const agoId = useId();
  const ago = queriedAt ? formatRelativeTime(queriedAt, now, t) : "";

  return (
    // 刷新中不换文字（只转圈），格子宽度只随「x 分钟前」变；最小宽度放得下中日文最长的
    // 「45 分钟前」（见 .auth-quota-refresh），时间跳档、刷新前后都不把额度条推得横向抖动
    <div className="auth-quota-refresh flex shrink-0 items-center justify-end gap-0.5 whitespace-nowrap text-caption text-fg-3">
      <span id={agoId}>{ago}</span>
      <HoverTip
        content={t("authCenter.quota.refresh", { defaultValue: "刷新额度" })}
      >
        <button
          type="button"
          aria-label={t("authCenter.quota.refreshAria", {
            defaultValue: "刷新 {{login}} 的额度",
            login,
          })}
          aria-disabled={loading}
          aria-busy={loading}
          aria-describedby={ago ? agoId : undefined}
          onClick={() => {
            if (!loading) onRefresh();
          }}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1 aria-disabled:opacity-45 aria-disabled:hover:bg-transparent"
        >
          <RefreshCw
            className={cn(
              "h-[13px] w-[13px]",
              loading && "motion-safe:animate-spin",
            )}
            strokeWidth={1.5}
          />
        </button>
      </HoverTip>
    </div>
  );
}

const ROW_CLASS =
  "flex h-[18px] items-center gap-[var(--auth-quota-gap)] text-caption";

/** 一行额度：档名 + 额度条（或一句说明）+ 剩余 */
function QuotaRowCells({
  label,
  line,
  trailing,
}: {
  label: string;
  line: QuotaLine;
  /** 跟在说明文字后面的小图标（可点开的行用） */
  trailing?: ReactNode;
}) {
  const width = barPercent(line);
  const value = line.value ?? line.text;
  return (
    <>
      {/* 按内容放宽，列内各行一起撑到最长的档名（最宽 112，更长的截断，悬停看全名）；下限保住短档名不被长说明挤成省略号 */}
      <span
        title={label}
        className="min-w-[3.5em] flex-auto truncate whitespace-nowrap text-fg-2"
      >
        {label}
      </span>
      {/* 说明文字至少和条一样宽，舒适模式字大了放不下「12月2日到期」时往左借空间；超过上限后由内层截断 */}
      {line.caption ? (
        <span
          title={line.caption}
          className="flex min-w-[var(--auth-quota-bar)] max-w-[132px] shrink items-center gap-0.5 text-fg-3"
        >
          <span className="min-w-0 truncate">{line.caption}</span>
          {trailing}
        </span>
      ) : (
        <span
          role="meter"
          aria-label={`${label}: ${value}`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(width)}
          className="relative h-1 w-[var(--auth-quota-bar)] shrink-0 overflow-hidden rounded-full bg-chart-grid"
        >
          <span
            className={cn(
              "absolute inset-y-0 start-0 rounded-full",
              TONE_FILL[line.tone],
            )}
            style={{ width: `${width}%` }}
          />
        </span>
      )}
      {/* 旁边已有额度条，正常时数值不上色 */}
      {/* 至少占 --auth-quota-value 宽以对齐常规文案；长译文不截断，直接撑宽该格 */}
      <span className="min-w-[var(--auth-quota-value)] shrink-0 whitespace-nowrap text-end tabular-nums text-fg-1">
        <TonedText
          text={value}
          line={line}
          className={line.tone === "normal" ? "" : TONE_TEXT[line.tone]}
        />
      </span>
    </>
  );
}

/** SubscriptionQuota（ChatGPT / xAI 账号的订阅额度）→ 额度列状态 */
export function subscriptionQuotaState(
  t: TFunction,
  quota: SubscriptionQuota | undefined,
  loading: boolean,
  locale: string,
  display: QuotaDisplay = "left",
): AccountQuotaState {
  if (!quota) return loading ? { kind: "loading" } : null;
  // 没有凭据 / 凭据解析失败：和供应商卡片一样不显示
  if (
    quota.credentialStatus === "not_found" ||
    quota.credentialStatus === "parse_error"
  ) {
    return null;
  }
  if (!quota.success) {
    return {
      kind: "failed",
      reason: quotaFailureReason(t, quota),
    };
  }
  const rows = quotaRows(t, quota, locale, { display });
  return rows.length > 0 ? { kind: "rows", rows } : null;
}

/** GitHub Copilot 账号的额度：高级请求，Free 另有代码补全（copilot_get_usage_for_account） */
export function CopilotAccountQuota({
  accountId,
  login,
}: {
  accountId: string;
  login: string;
}) {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const query = useCopilotQuota(accountId, { enabled: true, autoQuery: false });
  const { data: quota, isFetching: loading } = query;

  let state: AccountQuotaState;
  if (quota?.success) {
    state =
      quota.tiers.length > 0
        ? {
            kind: "rows",
            rows: copilotQuotaRows(t, quota.tiers, {
              display,
              locale: i18n.language,
            }),
          }
        : null;
  } else if (quota) {
    state = {
      kind: "failed",
      reason: quota.error || t("subscription.queryFailed"),
    };
  } else if (query.isError) {
    state = {
      kind: "failed",
      reason: extractErrorMessage(query.error) || t("subscription.queryFailed"),
    };
  } else {
    state = loading ? { kind: "loading" } : null;
  }

  return (
    <AccountQuotaColumn
      login={login}
      state={state}
      idle={query.manualIdle}
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void query.refetch()}
    />
  );
}

/** 按账号查的订阅额度列：xAI、Antigravity、Claude 只是查询 hook 不同 */
function subscriptionAccountQuota(
  useQuota: (
    accountId: string,
    options: { enabled: boolean; autoQuery: boolean },
  ) => {
    data: SubscriptionQuota | undefined;
    isFetching: boolean;
    refetch: () => unknown;
    manualIdle: boolean;
  },
) {
  return function SubscriptionAccountQuota({
    accountId,
    login,
  }: {
    accountId: string;
    login: string;
  }) {
    const { t, i18n } = useTranslation();
    const display = useQuotaDisplay();
    const {
      data: quota,
      isFetching: loading,
      refetch,
      manualIdle,
    } = useQuota(accountId, { enabled: true, autoQuery: false });
    return (
      <AccountQuotaColumn
        login={login}
        state={subscriptionQuotaState(
          t,
          quota,
          loading,
          i18n.language,
          display,
        )}
        queriedAt={quota?.queriedAt ?? null}
        loading={loading}
        onRefresh={() => void refetch()}
        idle={manualIdle}
      />
    );
  };
}

/** xAI 账号的订阅额度（get_xai_oauth_quota(accountId)） */
export const XaiAccountQuota = subscriptionAccountQuota(
  useXaiOauthQuotaByAccountId,
);

/** Kimi 官方账号的剩余额度（kimi_oauth_quota(accountId)，不用先切号） */
export function KimiAccountQuota({
  accountId,
  login,
}: {
  accountId: string;
  login: string;
}) {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const {
    data: quota,
    isFetching: loading,
    refetch,
    manualIdle,
  } = useKimiAccountQuota(accountId, { enabled: true, autoQuery: false });
  const state = subscriptionQuotaState(
    t,
    quota,
    loading,
    i18n.language,
    display,
  );
  return (
    <AccountQuotaColumn
      login={login}
      // 没订阅 Kimi Code 的号，额度接口返回空对象：查成功但一档都没有
      state={
        !state && quota?.success
          ? { kind: "note", text: t("kimi.oauth.noSubscription") }
          : state
      }
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void refetch()}
      idle={manualIdle}
    />
  );
}

/** Antigravity 账号的剩余额度（agy_account_quota(accountId)，不用先切号） */
export const AgyAccountQuota = subscriptionAccountQuota(useAgyAccountQuota);

/** 积分数：一万以上用紧凑写法（「12.3万」「12.3K」），放得下数值那一格 */
function formatCredits(value: number, locale: string): string {
  try {
    return new Intl.NumberFormat(
      locale,
      value >= 10_000
        ? { notation: "compact", maximumFractionDigits: 1 }
        : { maximumFractionDigits: value < 100 ? 2 : 0 },
    ).format(value);
  } catch {
    return String(Math.round(value));
  }
}

/** CodebuddyQuota → 额度列状态：一行「套餐名  额度条  剩余积分」，悬停写全总量 / 剩余 / 已用 */
/** CodeBuddy 额度查询的语言：后端按它给套餐名，只分中文 / 英文 */
export function codebuddyQuotaLang(language: string | undefined): "zh" | "en" {
  return language?.toLowerCase().startsWith("zh") ? "zh" : "en";
}

/**
 * `planInLabel`：额度行标签用套餐名（供应商卡片）。授权中心的账号行第二行已显示套餐，
 * 这里传 false，标签只写「积分 / 企业版」。
 */
export function codebuddyQuotaState(
  t: TFunction,
  quota: CodebuddyQuota | undefined,
  loading: boolean,
  locale: string,
  display: QuotaDisplay = "left",
  { planInLabel = true }: { planInLabel?: boolean } = {},
): AccountQuotaState {
  if (!quota) return loading ? { kind: "loading" } : null;
  if (
    quota.credentialStatus === "not_found" ||
    quota.credentialStatus === "parse_error"
  ) {
    return null;
  }
  if (!quota.success) {
    const reason =
      quota.credentialStatus === "expired"
        ? t("quota.reason.loginExpired")
        : quota.credentialStatus === "refresh_pending"
          ? t("authCenter.codebuddy.quota.refreshPending", {
              defaultValue: "令牌待 CodeBuddy 续期，切到这个号用一次后再查",
            })
          : quota.error || t("subscription.queryFailed");
    return { kind: "failed", reason };
  }

  const label =
    (planInLabel ? quota.planName : null) ||
    (quota.enterprise
      ? t("authCenter.codebuddy.quota.enterprise", { defaultValue: "企业版" })
      : t("authCenter.codebuddy.quota.credits", { defaultValue: "积分" }));
  const remaining = formatCredits(quota.remaining, locale);
  const used = formatCredits(quota.used, locale);
  const total = formatCredits(quota.total, locale);

  if (quota.unlimited) {
    const caption = t("authCenter.codebuddy.quota.unlimited", {
      defaultValue: "不限量",
    });
    return {
      kind: "rows",
      rows: [
        {
          label,
          line: {
            key: "codebuddy_credits",
            left: Infinity,
            tone: "normal",
            text: caption,
            caption,
            value: used,
            detail: t("authCenter.codebuddy.quota.usedDetail", {
              defaultValue: "本周期已用 {{used}} 积分",
              used,
            }),
          },
        },
      ],
    };
  }

  const leftPct =
    quota.total > 0
      ? Math.max(0, Math.min(100, (quota.remaining / quota.total) * 100))
      : 0;
  const showUsed = display === "used";
  const value =
    quota.remaining <= 0 ? t("quota.usedUp") : showUsed ? used : remaining;
  // 和 WorkBuddy 一样：最早到期的积分另起一行（同一套计费后端）
  const expiry = creditExpiryLine(
    t,
    quota.expiries,
    locale,
    "codebuddy_expiry",
  );
  return {
    kind: "rows",
    rows: [
      {
        label,
        line: {
          key: "codebuddy_credits",
          left: leftPct,
          bar: showUsed ? 100 - leftPct : leftPct,
          tone: toneForLeft(quota.remaining <= 0 ? 0 : leftPct),
          text: value,
          value,
          detail: t("authCenter.codebuddy.quota.detail", {
            defaultValue:
              "积分剩余 {{remaining}} / 共 {{total}}，已用 {{used}}",
            remaining,
            total,
            used,
          }),
        },
      },
      ...(expiry ? [{ label: t("workbuddy.quota.expiry"), line: expiry }] : []),
    ],
  };
}

/** CodeBuddy 账号的套餐与积分（codebuddy_account_quota(accountId)，不用先切号） */
export function CodebuddyAccountQuota({
  accountId,
  login,
}: {
  accountId: string;
  login: string;
}) {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const lang = codebuddyQuotaLang(i18n.language);
  const {
    data: quota,
    isFetching: loading,
    isError,
    error,
    refresh,
    manualIdle,
  } = useCodebuddyAccountQuota(accountId, lang);
  const state: AccountQuotaState =
    !quota && isError
      ? {
          kind: "failed",
          reason: extractErrorMessage(error) || t("subscription.queryFailed"),
        }
      : codebuddyQuotaState(t, quota, loading, i18n.language, display, {
          planInLabel: false,
        });
  return (
    <AccountQuotaColumn
      login={login}
      state={state}
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void refresh()}
      idle={manualIdle}
    />
  );
}

/** Claude 官方账号的订阅额度（get_claude_account_quota(accountId)，不用先切号） */
export const ClaudeAccountQuota = subscriptionAccountQuota(
  useClaudeAccountQuota,
);

/**
 * WorkBuddyQuota → 额度列状态：一行「套餐名  额度条  剩余积分」，悬停写全剩余 / 总量 / 已用；
 * 显示的是上次结果时悬停说明原因。还没有结果时：当前号且 WorkBuddy 没运行给一句提示，
 * 查询失败写「额度没查到」，其余不显示。
 */
export function workbuddyQuotaState(
  t: TFunction,
  quota: WorkBuddyQuota | undefined,
  loading: boolean,
  locale: string,
  display: QuotaDisplay = "left",
  planInLabel = true,
): AccountQuotaState {
  if (!quota) return loading ? { kind: "loading" } : null;
  const credits = workbuddyCreditsLine(t, quota, locale, display);
  if (credits) {
    const expiry = workbuddyExpiryLine(t, quota, locale);
    return {
      kind: "rows",
      rows: [
        {
          label:
            (planInLabel ? credits.planName : undefined) ??
            t("workbuddy.quota.credits"),
          line: credits.line,
        },
        ...(expiry
          ? [{ label: t("workbuddy.quota.expiry"), line: expiry }]
          : []),
      ],
    };
  }
  if (quota.unavailable === "not_running" && quota.current) {
    const caption = t("workbuddy.quota.openToQuery");
    return {
      kind: "rows",
      rows: [
        {
          label: t("workbuddy.quota.credits"),
          line: {
            key: "workbuddy_not_running",
            left: Infinity,
            tone: "muted",
            text: caption,
            caption,
            value: "",
            detail: t("workbuddy.quota.onlyWhileRunning"),
          },
        },
      ],
    };
  }
  if (quota.unavailable === "failed") {
    return {
      kind: "failed",
      reason: quota.error || t("workbuddy.quota.unavailable"),
    };
  }
  return null;
}

/** WorkBuddy 账号的套餐与积分（workbuddy_account_quota(accountId)）：只有当前号能经运行中的 WorkBuddy 刷新 */
export function WorkBuddyAccountQuota({
  accountId,
  login,
  planInLabel = true,
}: {
  accountId: string;
  login: string;
  /** 额度行标签用套餐名；授权中心把套餐放在账号第二行，这里就只写「积分」 */
  planInLabel?: boolean;
}) {
  const { t, i18n } = useTranslation();
  const display = useQuotaDisplay();
  const {
    data: quota,
    isFetching: loading,
    isError,
    error,
    refresh,
    manualIdle,
  } = useWorkBuddyAccountQuota(accountId);
  const state: AccountQuotaState =
    !quota && isError
      ? {
          kind: "failed",
          reason:
            extractErrorMessage(error) || t("workbuddy.quota.unavailable"),
        }
      : workbuddyQuotaState(
          t,
          quota,
          loading,
          i18n.language,
          display,
          planInLabel,
        );
  return (
    <AccountQuotaColumn
      login={login}
      state={state}
      queriedAt={quota?.snapshot?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void refresh()}
      idle={manualIdle}
    />
  );
}
