import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useRequestDetail } from "@/lib/query/usage";
import {
  getFreshInputTokens,
  isUnpricedUsage,
  type RequestLog,
} from "@/types/usage";
import { cn } from "@/lib/utils";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  SPEED_ESTIMATE_MIN_OUTPUT_TOKENS,
  SPEED_MIN_OUTPUT_TOKENS,
  fmtInt,
  formatEstimatedTokensPerSecond,
  formatOutputTokensPerSecond,
  getLocaleFromLanguage,
  getResolvedLang,
  parseFiniteNumber,
} from "./format";
import { appDisplayName } from "./RequestLogTable";

interface RequestDetailPanelProps {
  /** 要看的请求；null 时抽屉关闭 */
  requestId: string | null;
  onClose: () => void;
}

interface DetailRow {
  key: string;
  label: string;
  value: ReactNode;
  mono?: boolean;
  strong?: boolean;
  muted?: boolean;
  title?: string;
}

function DetailSection({ title, rows }: { title: string; rows: DetailRow[] }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="m-0 text-strong font-semibold text-fg-1">{title}</h3>
      <dl className="m-0 rounded-panel border border-border">
        {rows.map((row, index) => (
          <div
            key={row.key}
            className={cn(
              "flex min-h-8 items-center gap-3 px-3.5 py-1.5",
              index < rows.length - 1 && "border-b border-border",
            )}
          >
            <dt className="shrink-0 text-caption text-fg-2">{row.label}</dt>
            <dd
              className={cn(
                "m-0 min-w-0 flex-1 truncate text-end tabular-nums",
                row.mono ? "font-mono text-caption" : "text-body",
                row.strong && "font-semibold",
                row.muted ? "text-fg-3" : "text-fg-1",
              )}
              title={
                row.title ??
                (typeof row.value === "string" ? row.value : undefined)
              }
            >
              {row.value}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

const usd = (value: string) => {
  const num = parseFiniteNumber(value);
  return num == null ? "--" : `$${num.toFixed(4)}`;
};

function useDetailSections(request: RequestLog) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const freshInput = getFreshInputTokens(request);
  const isCacheInclusive = request.inputTokens !== freshInput;
  const unpriced = isUnpricedUsage(request);
  const multiplier = parseFiniteNumber(request.costMultiplier);
  const isProxy = !request.dataSource || request.dataSource === "proxy";
  const tps = formatOutputTokensPerSecond(request);
  const estimatedTps = formatEstimatedTokensPerSecond(request);
  const latency = parseFiniteNumber(request.latencyMs);
  const firstToken = parseFiniteNumber(request.firstTokenMs);

  const basic: DetailRow[] = [
    {
      key: "time",
      label: t("usage.time"),
      value: new Date(request.createdAt * 1000).toLocaleString(locale),
    },
    {
      key: "app",
      label: t("usage.app"),
      value: appDisplayName(request.appType),
    },
    {
      key: "provider",
      label: t("usage.provider"),
      value: request.providerName || t("usage.unknownProvider"),
    },
    {
      key: "providerId",
      label: t("usage.detail.providerId"),
      value: request.providerId,
      mono: true,
    },
    { key: "model", label: t("usage.model"), value: request.model, mono: true },
  ];
  if (request.requestModel && request.requestModel !== request.model) {
    basic.push({
      key: "requestModel",
      label: t("usage.requestModel"),
      value: request.requestModel,
      mono: true,
    });
  }
  if (request.pricingModel && request.pricingModel !== request.model) {
    basic.push({
      key: "pricingModel",
      label: t("usage.pricingModel"),
      value: request.pricingModel,
      mono: true,
    });
  }
  basic.push(
    {
      key: "source",
      label: t("usage.detail.source"),
      value: isProxy
        ? t("usage.detail.sourceProxy", { code: request.statusCode })
        : t("usage.detail.sourceSession"),
      title: request.dataSource ?? "proxy",
    },
    {
      key: "requestId",
      label: t("usage.requestId"),
      value: request.requestId,
      mono: true,
    },
  );

  const tokens: DetailRow[] = [
    {
      key: "fresh",
      label: t("usage.freshInput"),
      value: isCacheInclusive
        ? `${fmtInt(freshInput, locale)} (${t("usage.rawInputLabel")}: ${fmtInt(request.inputTokens, locale)})`
        : fmtInt(freshInput, locale),
    },
    {
      key: "output",
      label: t("usage.outputTokens"),
      value: fmtInt(request.outputTokens, locale),
    },
    {
      key: "read",
      label: t("usage.cacheReadTokens"),
      value: fmtInt(request.cacheReadTokens, locale),
    },
    {
      key: "write",
      label: t("usage.pricing.cacheWrite"),
      value: fmtInt(request.cacheCreationTokens, locale),
    },
  ];

  const cost: DetailRow[] = [
    {
      key: "input",
      label: t("usage.freshInput"),
      value: usd(request.inputCostUsd),
    },
    {
      key: "output",
      label: t("usage.outputTokens"),
      value: usd(request.outputCostUsd),
    },
    {
      key: "read",
      label: t("usage.cacheReadTokens"),
      value: usd(request.cacheReadCostUsd),
    },
    {
      key: "write",
      label: t("usage.pricing.cacheWrite"),
      value: usd(request.cacheCreationCostUsd),
    },
  ];
  if (multiplier != null && multiplier !== 1) {
    cost.push({
      key: "multiplier",
      label: t("usage.costMultiplier"),
      value: `×${request.costMultiplier}`,
    });
  }
  cost.push({
    key: "total",
    label:
      multiplier != null && multiplier !== 1
        ? `${t("usage.detail.total")} (${t("usage.withMultiplier")})`
        : t("usage.detail.total"),
    value: unpriced ? t("usage.unpriced") : usd(request.totalCostUsd),
    strong: !unpriced,
    muted: unpriced,
    title: unpriced
      ? undefined
      : `$${parseFiniteNumber(request.totalCostUsd)?.toFixed(6) ?? "--"}`,
  });

  let performance: DetailRow[];
  if (!isProxy) {
    // 会话日志没有首字计时；耗时是导入时按日志时间戳估的（0 = 没估出来）
    if (latency == null || latency <= 0) {
      performance = [
        {
          key: "speed",
          label: t("usage.speed"),
          value: t("usage.detail.noTimingSession"),
          muted: true,
        },
      ];
    } else {
      performance = [
        estimatedTps != null
          ? {
              key: "speed",
              label: t("usage.speed"),
              value: t("usage.speedEstimatedValue", { value: estimatedTps }),
            }
          : {
              key: "speed",
              label: t("usage.speed"),
              value:
                request.outputTokens < SPEED_ESTIMATE_MIN_OUTPUT_TOKENS
                  ? t("usage.detail.speedEstimateTooFew")
                  : "—",
              muted: true,
            },
        {
          key: "duration",
          label: t("usage.detail.durationEstimated"),
          value: `${(latency / 1000).toFixed(1)}s`,
          title: `${fmtInt(latency, locale)} ms`,
        },
      ];
    }
  } else {
    const speedRow: DetailRow =
      tps != null
        ? {
            key: "speed",
            label: t("usage.speed"),
            value: t("usage.speedValue", { value: tps }),
          }
        : firstToken == null
          ? {
              key: "speed",
              label: t("usage.speed"),
              value: t("usage.detail.noFirstToken"),
              muted: true,
            }
          : request.outputTokens < SPEED_MIN_OUTPUT_TOKENS
            ? {
                key: "speed",
                label: t("usage.speed"),
                value: t("usage.detail.speedTooFew"),
                muted: true,
              }
            : {
                key: "speed",
                label: t("usage.speed"),
                value: "—",
                muted: true,
              };
    performance = [speedRow];
    if (firstToken != null) {
      performance.push({
        key: "ttft",
        label: t("usage.detail.firstToken"),
        value: `${(firstToken / 1000).toFixed(1)}s`,
      });
    }
    if (latency != null) {
      performance.push({
        key: "duration",
        label: t("usage.detail.duration"),
        value: `${(latency / 1000).toFixed(1)}s`,
        title: `${fmtInt(latency, locale)} ms`,
      });
    }
  }

  return { basic, tokens, cost, performance };
}

function RequestDetailBody({ request }: { request: RequestLog }) {
  const { t } = useTranslation();
  const sections = useDetailSections(request);
  return (
    <div className="flex flex-col gap-4">
      <DetailSection title={t("usage.basicInfo")} rows={sections.basic} />
      <DetailSection
        title={t("usage.detail.tokensTitle")}
        rows={sections.tokens}
      />
      <DetailSection title={t("usage.detail.costTitle")} rows={sections.cost} />
      <DetailSection
        title={t("usage.detail.performance")}
        rows={sections.performance}
      />
      {request.errorMessage && (
        <section className="flex flex-col gap-2">
          <h3 className="m-0 text-strong font-semibold text-danger-text">
            {t("usage.errorMessage")}
          </h3>
          <p className="m-0 whitespace-pre-wrap break-words rounded-panel bg-danger-soft px-3.5 py-2.5 font-mono text-caption text-fg-1">
            {request.errorMessage}
          </p>
        </section>
      )}
    </div>
  );
}

/** 请求详情抽屉（v7 S6：点表格一行打开）。 */
export function RequestDetailPanel({
  requestId,
  onClose,
}: RequestDetailPanelProps) {
  const { t } = useTranslation();
  const {
    data: request,
    isLoading,
    error,
    refetch,
  } = useRequestDetail(requestId ?? "");

  const subtitle = request
    ? [
        new Date(request.createdAt * 1000).toLocaleString(),
        appDisplayName(request.appType),
        request.providerName || t("usage.unknownProvider"),
      ].join(" · ")
    : undefined;

  return (
    <Sheet
      open={requestId != null}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <SheetContent
        width={420}
        closeLabel={t("common.close")}
        dismissOnOutsideClick
      >
        <SheetHeader className="pb-3">
          <SheetTitle>{t("usage.requestDetail")}</SheetTitle>
          <SheetDescription className={subtitle ? "truncate" : "sr-only"}>
            {subtitle ?? t("usage.requestDetail")}
          </SheetDescription>
        </SheetHeader>
        <SheetBody className="border-t border-border">
          {isLoading ? (
            <div className="h-[320px] animate-pulse rounded-panel bg-subtle" />
          ) : request ? (
            <RequestDetailBody request={request} />
          ) : error ? (
            // 查询失败和「没有这条记录」分开说：前者能重试，也要让人看到原因
            <Notice
              tone="danger"
              title={t("usage.requestLoadFailed")}
              actions={
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={() => refetch()}
                >
                  {t("common.retry")}
                </Button>
              }
            >
              {extractErrorMessage(error) || String(error)}
            </Notice>
          ) : (
            <p className="py-10 text-center text-body text-fg-3">
              {t("usage.requestNotFound")}
            </p>
          )}
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
