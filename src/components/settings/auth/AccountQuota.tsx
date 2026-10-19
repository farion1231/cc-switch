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
  tierLine,
  toneForLeft,
  type QuotaDisplay,
  type QuotaLine,
} from "@/components/quota/quotaRules";
import {
  quotaFailureReason,
  quotaRows,
} from "@/components/SubscriptionQuotaFooter";
import type { CodebuddyQuota, SubscriptionQuota } from "@/types/subscription";
import { useCopilotQuota } from "@/lib/query/copilot";
import {
  useCodebuddyOauthQuotaByAccountId,
  useXaiOauthQuotaByAccountId,
} from "@/lib/query/subscription";
import { extractErrorMessage } from "@/utils/errorUtils";

export interface AccountQuotaRow {
  label: string;
  line: QuotaLine;
}

/** 额度列要画的东西：成功按档画条；查询失败写「额度没查到」+ 原因；null 表示不显示 */
export type AccountQuotaState =
  | { kind: "rows"; rows: AccountQuotaRow[] }
  | { kind: "failed"; reason: string }
  | { kind: "loading" }
  | null;

interface AccountQuotaColumnProps {
  /** 账号名，拼进刷新按钮的无障碍名字 */
  login: string;
  state: AccountQuotaState;
  queriedAt?: number | null;
  loading: boolean;
  onRefresh: () => void;
}

/**
 * 授权中心账号行右侧的额度（v7 Auth 画板）：212 宽的额度条（条和数字按设置写剩余或已用，
 * 平时 --chart-1；不到 20% 换 warning，用完换 danger）+ 84 宽的「x 分钟前 ↻」。
 * 打开页面时查一次，不轮询；↻ 手动重查。
 */
export function AccountQuotaColumn({
  login,
  state,
  queriedAt,
  loading,
  onRefresh,
}: AccountQuotaColumnProps) {
  const { t } = useTranslation();
  const now = useNow(
    Boolean(queriedAt) ||
      (state?.kind === "rows" && state.rows.some(({ line }) => line.resetsAt)),
  );
  const agoId = useId();
  if (!state) return null;
  // 有一档带重置时间，每行数值后面都留出倒计时那一格（各行对齐），整列加宽
  const showReset =
    state.kind === "rows" &&
    state.rows.some(({ line }) => countdownStr(line.resetsAt, now));
  const resetCell = (line: QuotaLine) =>
    showReset ? (
      <ResetSlot countdown={countdownStr(line.resetsAt, now)} className="" />
    ) : null;

  const ago = queriedAt ? formatRelativeTime(queriedAt, now, t) : "";
  const agoText = loading
    ? t("authCenter.quota.updating", { defaultValue: "更新中…" })
    : ago;

  return (
    <>
      <div
        className={cn(
          "flex shrink-0 flex-col gap-0.5",
          showReset ? "w-[272px]" : "w-[212px]",
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
              {t("quota.failed", { defaultValue: "额度没查到" })}
            </span>
            <span
              className="truncate text-caption text-fg-3"
              title={state.reason}
            >
              {state.reason}
            </span>
          </>
        )}
        {state.kind === "loading" && (
          <span className="text-caption text-fg-3">
            {t("authCenter.quota.loading", { defaultValue: "正在查询额度…" })}
          </span>
        )}
      </div>
      <div className="flex w-[84px] shrink-0 items-center justify-end gap-0.5 whitespace-nowrap text-caption text-fg-3">
        <span id={agoId} className="truncate">
          {agoText}
        </span>
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
            aria-describedby={loading ? agoId : undefined}
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
    </>
  );
}

const ROW_CLASS = "flex h-[18px] items-center gap-2 text-caption";

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
      <span className="min-w-[52px] shrink-0 whitespace-nowrap text-fg-2">
        {label}
      </span>
      {line.caption ? (
        <span className="flex min-w-0 flex-1 items-center gap-0.5 text-fg-3">
          <span className="truncate">{line.caption}</span>
          {trailing}
        </span>
      ) : (
        <span
          role="meter"
          aria-label={`${label}: ${value}`}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(width)}
          className="relative h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-chart-grid"
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
      <span className="w-14 shrink-0 whitespace-nowrap text-end tabular-nums text-fg-1">
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

/** GitHub Copilot 账号的高级请求额度（copilot_get_usage_for_account） */
export function CopilotAccountQuota({
  accountId,
  login,
}: {
  accountId: string;
  login: string;
}) {
  const { t } = useTranslation();
  const display = useQuotaDisplay();
  const query = useCopilotQuota(accountId, { enabled: true, autoQuery: false });
  const { data: quota, isFetching: loading } = query;

  let state: AccountQuotaState;
  if (quota?.success) {
    const label = t("subscription.copilotPremium");
    state =
      quota.tiers.length > 0
        ? {
            kind: "rows",
            rows: quota.tiers.map((tier) => ({
              label,
              line: tierLine(t, tier, label, { display }),
            })),
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
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void query.refetch()}
    />
  );
}

/** xAI 账号的订阅额度（get_xai_oauth_quota(accountId)） */
export function XaiAccountQuota({
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
  } = useXaiOauthQuotaByAccountId(accountId, {
    enabled: true,
    autoQuery: false,
  });
  return (
    <AccountQuotaColumn
      login={login}
      state={subscriptionQuotaState(t, quota, loading, i18n.language, display)}
      queriedAt={quota?.queriedAt ?? null}
      loading={loading}
      onRefresh={() => void refetch()}
    />
  );
}

/** 额度数字按界面语言加千位分隔、最多两位小数（「1,000」「665.26」） */
function formatCredits(value: number, locale: string): string {
  if (!Number.isFinite(value)) return "-";
  try {
    return new Intl.NumberFormat(locale, {
      maximumFractionDigits: 2,
    }).format(value);
  } catch {
    return String(value);
  }
}

/**
 * CodeBuddy 的 cycle_end 可能是毫秒时间戳字符串，也可能是 `YYYY-MM-DD HH:mm:ss`
 *（接口原值，没有时区）；统一转成 ISO，交给额度列算倒计时。
 */
function cycleEndIso(value: string | null | undefined): string | null {
  const raw = value?.trim();
  if (!raw) return null;
  if (/^\d+$/.test(raw)) {
    const ms = Number(raw);
    if (!Number.isFinite(ms)) return null;
    const date = new Date(ms);
    return Number.isNaN(date.getTime()) ? null : date.toISOString();
  }
  // 空格换成 T 后按本地时间解析，和接口给的本地时间一致
  const ms = Date.parse(raw.replace(" ", "T"));
  return Number.isFinite(ms) ? new Date(ms).toISOString() : null;
}

/**
 * CodeBuddy 额度 → 额度列状态：一行「剩余 / 总量 credits」。
 * left 用后端算好的 remaining_percent；重置时间取第一个资源包的周期结束。
 */
export function codebuddyQuotaState(
  t: TFunction,
  quota: CodebuddyQuota | undefined,
  loading: boolean,
  locale: string,
): AccountQuotaState {
  if (!quota) return loading ? { kind: "loading" } : null;
  if (!quota.success) {
    return {
      kind: "failed",
      reason:
        quota.error ||
        t("subscription.queryFailed", { defaultValue: "查询失败" }),
    };
  }
  const label = t("subscription.codebuddyCredits", {
    defaultValue: "CodeBuddy 额度",
  });
  const remaining = formatCredits(quota.remaining, locale);
  const total = formatCredits(quota.total, locale);
  const text = t("subscription.codebuddyCreditsText", {
    defaultValue: "剩余 {{remaining}} / {{total}} credits",
    remaining,
    total,
  });
  const left = Math.max(0, Math.min(100, Math.round(quota.remaining_percent)));
  const line: QuotaLine = {
    key: "codebuddy_credits",
    left,
    tone: toneForLeft(left),
    emphasis: remaining,
    text,
    // 额度条旁边只放数字（列很窄），完整的「剩余 / 总量」写进悬停说明
    value: remaining,
    detail: text,
    resetsAt: cycleEndIso(quota.packages[0]?.cycle_end),
  };
  return { kind: "rows", rows: [{ label, line }] };
}

/** CodeBuddy 账号的额度（get_codebuddy_oauth_quota(accountId)） */
export function CodebuddyAccountQuota({
  accountId,
  login,
}: {
  accountId: string;
  login: string;
}) {
  const { t, i18n } = useTranslation();
  const {
    data: quota,
    isFetching: loading,
    isError,
    error,
    refetch,
  } = useCodebuddyOauthQuotaByAccountId(accountId, {
    enabled: true,
    autoQuery: false,
  });

  let state: AccountQuotaState;
  if (quota) {
    state = codebuddyQuotaState(t, quota, loading, i18n.language);
  } else if (isError) {
    state = {
      kind: "failed",
      reason:
        extractErrorMessage(error) ||
        t("subscription.queryFailed", { defaultValue: "查询失败" }),
    };
  } else {
    state = loading ? { kind: "loading" } : null;
  }

  return (
    <AccountQuotaColumn
      login={login}
      state={state}
      queriedAt={quota?.queried_at ?? null}
      loading={loading}
      onRefresh={() => void refetch()}
    />
  );
}
