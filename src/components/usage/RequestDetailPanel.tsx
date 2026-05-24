import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useRequestDetail } from "@/lib/query/usage";
import { getFreshInputTokens, isUnpricedUsage } from "@/types/usage";
import { Check, Copy } from "lucide-react";
import { useState } from "react";

interface RequestDetailPanelProps {
  requestId: string;
  onClose: () => void;
}

function CopyValueButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard?.writeText(value);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch {
      setCopied(false);
    }
  };

  return (
    <button
      type="button"
      onClick={handleCopy}
      className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded border bg-background text-muted-foreground opacity-0 transition hover:text-foreground group-hover:opacity-100 focus:opacity-100"
      aria-label="Copy value"
    >
      {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
    </button>
  );
}

function DetailValue({
  label,
  value,
  mono = false,
  long = "auto",
}: {
  label: string;
  value: unknown;
  mono?: boolean;
  long?: "auto" | "always" | "never";
}) {
  const text =
    value == null || value === ""
      ? "-"
      : typeof value === "string"
        ? value
        : String(value);
  const isLong =
    long === "always" ||
    (long === "auto" && (text.length > 72 || text.includes("\n")));
  const valueClass = mono ? "font-mono" : "";

  return (
    <div className="min-w-0">
      <dt className="mb-1 text-muted-foreground">{label}</dt>
      <dd className="group min-w-0 text-foreground">
        {isLong ? (
          <details className="rounded-md border bg-muted/30 p-2 open:bg-background">
            <summary className="flex cursor-pointer list-none items-start gap-2 text-xs text-muted-foreground">
              <span
                className={`min-w-0 flex-1 overflow-hidden break-words ${valueClass}`}
                style={{
                  display: "-webkit-box",
                  WebkitLineClamp: 2,
                  WebkitBoxOrient: "vertical",
                }}
              >
                {text}
              </span>
              <CopyValueButton value={text} />
            </summary>
            <pre
              className={`mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-words rounded bg-background p-2 text-xs leading-relaxed ${valueClass}`}
            >
              {text}
            </pre>
          </details>
        ) : (
          <div className="flex min-w-0 items-center gap-2">
            <span className={`min-w-0 truncate ${valueClass}`}>{text}</span>
            {text !== "-" && <CopyValueButton value={text} />}
          </div>
        )}
      </dd>
    </div>
  );
}

export function RequestDetailPanel({
  requestId,
  onClose,
}: RequestDetailPanelProps) {
  const { t, i18n } = useTranslation();
  const { data: request, isLoading } = useRequestDetail(requestId);
  const dateLocale =
    i18n.language === "zh"
      ? "zh-CN"
      : i18n.language === "ja"
        ? "ja-JP"
        : "en-US";

  if (isLoading) {
    return (
      <Dialog open onOpenChange={onClose}>
        <DialogContent className="max-w-2xl">
          <div className="h-[400px] animate-pulse rounded bg-gray-100" />
        </DialogContent>
      </Dialog>
    );
  }

  if (!request) {
    return (
      <Dialog open onOpenChange={onClose}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t("usage.requestDetail", "请求详情")}</DialogTitle>
          </DialogHeader>
          <div className="text-center text-muted-foreground">
            {t("usage.requestNotFound", "请求未找到")}
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  const freshInput = getFreshInputTokens(request);
  const isCacheInclusive = request.inputTokens !== freshInput;
  const unpriced = isUnpricedUsage(request);

  return (
    <Dialog open onOpenChange={onClose}>
      <DialogContent className="flex max-h-[86vh] max-w-[min(96vw,980px)] flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t("usage.requestDetail", "请求详情")}</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 space-y-4 overflow-y-auto pr-1">
          {/* 基本信息 */}
          <div className="rounded-lg border p-4">
            <h3 className="mb-3 font-semibold">
              {t("usage.basicInfo", "基本信息")}
            </h3>
            <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2 xl:grid-cols-3">
              <DetailValue
                label={t("usage.requestId", "请求ID")}
                value={request.requestId}
                mono
              />
              <DetailValue
                label={t("usage.time", "时间")}
                value={new Date(request.createdAt * 1000).toLocaleString(
                  dateLocale,
                )}
              />
              <DetailValue
                label={t("usage.provider", "供应商")}
                value={`${request.providerName || t("usage.unknownProvider", "未知")} (${request.providerId})`}
              />
              <DetailValue
                label={t("usage.appType", "应用类型")}
                value={request.appType}
              />
              <DetailValue
                label={t("usage.model", "模型")}
                value={
                  request.requestModel && request.requestModel !== request.model
                    ? `${request.requestModel} → ${request.model}`
                    : request.model
                }
                mono
              />
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.status", "状态")}
                </dt>
                <dd>
                  <span
                    className={`inline-flex rounded-full px-2 py-1 text-xs ${
                      request.statusCode >= 200 && request.statusCode < 300
                        ? "bg-green-100 text-green-800"
                        : "bg-red-100 text-red-800"
                    }`}
                  >
                    {request.statusCode}
                  </span>
                </dd>
              </div>
            </dl>
          </div>

          {/* Token 使用量 */}
          <div className="rounded-lg border p-4">
            <h3 className="mb-3 font-semibold">
              {t("usage.tokenUsage", "Token 使用量")}
            </h3>
            <dl className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.inputTokens", "输入 Tokens")}
                </dt>
                <dd className="font-mono">
                  {freshInput.toLocaleString()}
                  {isCacheInclusive && (
                    <span className="ml-2 text-xs text-muted-foreground/70 font-normal">
                      ({t("usage.rawInputLabel", "原始")}:{" "}
                      {request.inputTokens.toLocaleString()})
                    </span>
                  )}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.outputTokens", "输出 Tokens")}
                </dt>
                <dd className="font-mono">
                  {request.outputTokens.toLocaleString()}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.cacheReadTokens", "缓存读取")}
                </dt>
                <dd className="font-mono">
                  {request.cacheReadTokens.toLocaleString()}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.cacheCreationTokens", "缓存写入")}
                </dt>
                <dd className="font-mono">
                  {request.cacheCreationTokens.toLocaleString()}
                </dd>
              </div>
              <div className="col-span-2">
                <dt className="text-muted-foreground">
                  {t("usage.totalTokens", "总计")}
                </dt>
                <dd className="text-lg font-semibold">
                  {(freshInput + request.outputTokens).toLocaleString()}
                </dd>
              </div>
            </dl>
          </div>

          {/* 成本明细 */}
          <div className="rounded-lg border p-4">
            <h3 className="mb-3 font-semibold">
              {t("usage.costBreakdown", "成本明细")}
            </h3>
            <dl className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.inputCost", "输入成本")}
                  <span className="ml-1 text-xs">
                    ({t("usage.baseCost", "基础")})
                  </span>
                </dt>
                <dd className="font-mono">
                  ${parseFloat(request.inputCostUsd).toFixed(6)}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.outputCost", "输出成本")}
                  <span className="ml-1 text-xs">
                    ({t("usage.baseCost", "基础")})
                  </span>
                </dt>
                <dd className="font-mono">
                  ${parseFloat(request.outputCostUsd).toFixed(6)}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.cacheReadCost", "缓存读取成本")}
                  <span className="ml-1 text-xs">
                    ({t("usage.baseCost", "基础")})
                  </span>
                </dt>
                <dd className="font-mono">
                  ${parseFloat(request.cacheReadCostUsd).toFixed(6)}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.cacheCreationCost", "缓存写入成本")}
                  <span className="ml-1 text-xs">
                    ({t("usage.baseCost", "基础")})
                  </span>
                </dt>
                <dd className="font-mono">
                  ${parseFloat(request.cacheCreationCostUsd).toFixed(6)}
                </dd>
              </div>
              {/* 显示成本倍率（如果不等于1） */}
              {request.costMultiplier &&
                parseFloat(request.costMultiplier) !== 1 && (
                  <div className="col-span-2 border-t pt-3">
                    <dt className="text-muted-foreground">
                      {t("usage.costMultiplier", "成本倍率")}
                    </dt>
                    <dd className="font-mono">×{request.costMultiplier}</dd>
                  </div>
                )}
              <div
                className={`col-span-2 ${request.costMultiplier && parseFloat(request.costMultiplier) !== 1 ? "" : "border-t"} pt-3`}
              >
                <dt className="text-muted-foreground">
                  {t("usage.totalCost", "总成本")}
                  {request.costMultiplier &&
                    parseFloat(request.costMultiplier) !== 1 && (
                      <span className="ml-1 text-xs">
                        ({t("usage.withMultiplier", "含倍率")})
                      </span>
                    )}
                </dt>
                <dd
                  className={`text-lg font-semibold ${
                    unpriced ? "text-muted-foreground" : "text-primary"
                  }`}
                >
                  {unpriced
                    ? t("usage.unpriced", "未定价")
                    : `$${parseFloat(request.totalCostUsd).toFixed(6)}`}
                </dd>
              </div>
            </dl>
          </div>

          {/* 性能信息 */}
          <div className="rounded-lg border p-4">
            <h3 className="mb-3 font-semibold">
              {t("usage.performance", "性能信息")}
            </h3>
            <dl className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <dt className="text-muted-foreground">
                  {t("usage.latency", "延迟")}
                </dt>
                <dd className="font-mono">{request.latencyMs}ms</dd>
              </div>
            </dl>
          </div>

          {/* 错误信息 */}
          {request.errorMessage && (
            <div className="rounded-lg border border-red-200 bg-red-50 p-4">
              <h3 className="mb-2 font-semibold text-red-800">
                {t("usage.errorMessage", "错误信息")}
              </h3>
              <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-words rounded bg-white/70 p-3 text-sm text-red-700">
                {request.errorMessage}
              </pre>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
