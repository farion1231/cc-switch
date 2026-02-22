import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useRequestLogs, usageKeys } from "@/lib/query/usage";
import { useQueryClient } from "@tanstack/react-query";
import type { LogFilters } from "@/types/usage";
import { ChevronLeft, ChevronRight, RefreshCw, Search, X } from "lucide-react";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";

interface RequestLogTableProps {
  refreshIntervalMs: number;
}

const ONE_DAY_SECONDS = 24 * 60 * 60;
const MAX_FIXED_RANGE_SECONDS = 30 * ONE_DAY_SECONDS;

type TimeMode = "rolling" | "fixed";

export function RequestLogTable({ refreshIntervalMs }: RequestLogTableProps) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();

  const getRollingRange = () => {
    const now = Math.floor(Date.now() / 1000);
    const oneDayAgo = now - ONE_DAY_SECONDS;
    return { startDate: oneDayAgo, endDate: now };
  };

  const [appliedTimeMode, setAppliedTimeMode] = useState<TimeMode>("rolling");
  const [draftTimeMode, setDraftTimeMode] = useState<TimeMode>("rolling");

  const [appliedFilters, setAppliedFilters] = useState<LogFilters>({});
  const [draftFilters, setDraftFilters] = useState<LogFilters>({});
  const [page, setPage] = useState(0);
  const pageSize = 20;
  const [validationError, setValidationError] = useState<string | null>(null);

  const { data: result, isLoading } = useRequestLogs({
    filters: appliedFilters,
    timeMode: appliedTimeMode,
    rollingWindowSeconds: ONE_DAY_SECONDS,
    page,
    pageSize,
    options: {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  });

  const logs = result?.data ?? [];
  const total = result?.total ?? 0;
  const totalPages = Math.ceil(total / pageSize);

  const handleSearch = () => {
    setValidationError(null);

    if (draftTimeMode === "fixed") {
      const start = draftFilters.startDate;
      const end = draftFilters.endDate;

      if (typeof start !== "number" || typeof end !== "number") {
        setValidationError(
          t("usage.invalidTimeRange", "请选择完整的开始/结束时间"),
        );
        return;
      }

      if (start > end) {
        setValidationError(
          t("usage.invalidTimeRangeOrder", "开始时间不能晚于结束时间"),
        );
        return;
      }

      if (end - start > MAX_FIXED_RANGE_SECONDS) {
        setValidationError(
          t("usage.timeRangeTooLarge", "时间范围过大，请缩小范围"),
        );
        return;
      }
    }

    setAppliedTimeMode(draftTimeMode);
    setAppliedFilters((prev) => {
      const next = { ...prev, ...draftFilters };
      if (draftTimeMode === "rolling") {
        delete next.startDate;
        delete next.endDate;
      }
      return next;
    });
    setPage(0);
  };

  const handleReset = () => {
    setValidationError(null);
    setAppliedTimeMode("rolling");
    setDraftTimeMode("rolling");
    setDraftFilters({});
    setAppliedFilters({});
    setPage(0);
  };

  const handleRefresh = () => {
    const key = {
      timeMode: appliedTimeMode,
      rollingWindowSeconds:
        appliedTimeMode === "rolling" ? ONE_DAY_SECONDS : undefined,
      appType: appliedFilters.appType,
      providerName: appliedFilters.providerName,
      model: appliedFilters.model,
      statusCode: appliedFilters.statusCode,
      startDate:
        appliedTimeMode === "fixed" ? appliedFilters.startDate : undefined,
      endDate: appliedTimeMode === "fixed" ? appliedFilters.endDate : undefined,
    };

    queryClient.invalidateQueries({
      queryKey: usageKeys.logs(key, page, pageSize),
    });
  };

  // 将 Unix 时间戳转换为本地时间的 datetime-local 格式
  const timestampToLocalDatetime = (timestamp: number): string => {
    const date = new Date(timestamp * 1000);
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    const hours = String(date.getHours()).padStart(2, "0");
    const minutes = String(date.getMinutes()).padStart(2, "0");
    return `${year}-${month}-${day}T${hours}:${minutes}`;
  };

  // 将 datetime-local 格式转换为 Unix 时间戳
  const localDatetimeToTimestamp = (datetime: string): number | undefined => {
    if (!datetime) return undefined;
    // 验证格式是否完整 (YYYY-MM-DDTHH:mm)
    if (datetime.length < 16) return undefined;
    const timestamp = new Date(datetime).getTime();
    // 验证是否为有效日期
    if (isNaN(timestamp)) return undefined;
    return Math.floor(timestamp / 1000);
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  const rollingRangeForDisplay =
    draftTimeMode === "rolling" ? getRollingRange() : null;

  return (
    <div className="space-y-4">
      {/* 筛选栏 */}
      <div className="flex flex-col gap-4 rounded-lg border bg-card/50 p-4 backdrop-blur-sm">
        <div className="flex flex-wrap items-center gap-3">
          <Select
            value={draftFilters.appType || "all"}
            onValueChange={(v) =>
              setDraftFilters({
                ...draftFilters,
                appType: v === "all" ? undefined : v,
              })
            }
          >
            <SelectTrigger className="w-[130px] bg-background">
              <SelectValue placeholder={t("usage.appType")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("usage.allApps")}</SelectItem>
              <SelectItem value="claude">Claude</SelectItem>
              <SelectItem value="codex">Codex</SelectItem>
              <SelectItem value="gemini">Gemini</SelectItem>
            </SelectContent>
          </Select>

          <Select
            value={draftFilters.statusCode?.toString() || "all"}
            onValueChange={(v) =>
              setDraftFilters({
                ...draftFilters,
                statusCode:
                  v === "all"
                    ? undefined
                    : Number.isFinite(Number.parseInt(v, 10))
                      ? Number.parseInt(v, 10)
                      : undefined,
              })
            }
          >
            <SelectTrigger className="w-[130px] bg-background">
              <SelectValue placeholder={t("usage.statusCode")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("common.all")}</SelectItem>
              <SelectItem value="200">200 OK</SelectItem>
              <SelectItem value="400">400 Bad Request</SelectItem>
              <SelectItem value="401">401 Unauthorized</SelectItem>
              <SelectItem value="429">429 Rate Limit</SelectItem>
              <SelectItem value="500">500 Server Error</SelectItem>
            </SelectContent>
          </Select>

          <div className="flex items-center gap-2 flex-1 min-w-[300px]">
            <div className="relative flex-1">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder={t("usage.searchProviderPlaceholder")}
                className="pl-9 bg-background"
                value={draftFilters.providerName || ""}
                onChange={(e) =>
                  setDraftFilters({
                    ...draftFilters,
                    providerName: e.target.value || undefined,
                  })
                }
              />
            </div>
            <Input
              placeholder={t("usage.searchModelPlaceholder")}
              className="w-[180px] bg-background"
              value={draftFilters.model || ""}
              onChange={(e) =>
                setDraftFilters({
                  ...draftFilters,
                  model: e.target.value || undefined,
                })
              }
            />
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <span className="whitespace-nowrap">{t("usage.timeRange")}:</span>
            <Input
              type="datetime-local"
              className="h-8 w-[200px] bg-background"
              value={
                (rollingRangeForDisplay?.startDate ?? draftFilters.startDate)
                  ? timestampToLocalDatetime(
                      (rollingRangeForDisplay?.startDate ??
                        draftFilters.startDate) as number,
                    )
                  : ""
              }
              onChange={(e) => {
                const timestamp = localDatetimeToTimestamp(e.target.value);
                setDraftTimeMode("fixed");
                setDraftFilters({
                  ...draftFilters,
                  startDate: timestamp,
                });
              }}
            />
            <span>-</span>
            <Input
              type="datetime-local"
              className="h-8 w-[200px] bg-background"
              value={
                (rollingRangeForDisplay?.endDate ?? draftFilters.endDate)
                  ? timestampToLocalDatetime(
                      (rollingRangeForDisplay?.endDate ??
                        draftFilters.endDate) as number,
                    )
                  : ""
              }
              onChange={(e) => {
                const timestamp = localDatetimeToTimestamp(e.target.value);
                setDraftTimeMode("fixed");
                setDraftFilters({
                  ...draftFilters,
                  endDate: timestamp,
                });
              }}
            />
          </div>

          <div className="flex items-center gap-2 ml-auto">
            <Button
              size="sm"
              variant="default"
              onClick={handleSearch}
              className="h-8"
            >
              <Search className="mr-2 h-3.5 w-3.5" />
              {t("common.search")}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={handleReset}
              className="h-8"
            >
              <X className="mr-2 h-3.5 w-3.5" />
              {t("common.reset")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={handleRefresh}
              className="h-8 px-2"
            >
              <RefreshCw className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {validationError && (
          <div className="text-sm text-red-600">{validationError}</div>
        )}
      </div>

      {isLoading ? (
        <div className="h-[400px] animate-pulse rounded bg-gray-100" />
      ) : (
        <>
          <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="whitespace-nowrap">
                    {t("usage.time")}
                  </TableHead>
                  <TableHead className="whitespace-nowrap">
                    {t("usage.provider")}
                  </TableHead>
                  <TableHead className="min-w-[200px] whitespace-nowrap">
                    {t("usage.billingModel")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap">
                    {t("usage.inputTokens")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap">
                    {t("usage.outputTokens")}
                  </TableHead>
                  <TableHead className="text-right min-w-[90px] whitespace-nowrap">
                    {t("usage.cacheReadTokens")}
                  </TableHead>
                  <TableHead className="text-right min-w-[90px] whitespace-nowrap">
                    {t("usage.cacheCreationTokens")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap">
                    {t("usage.multiplier")}
                  </TableHead>
                  <TableHead className="text-right whitespace-nowrap">
                    {t("usage.totalCost")}
                  </TableHead>
                  <TableHead className="text-center min-w-[140px] whitespace-nowrap">
                    {t("usage.timingInfo")}
                  </TableHead>
                  <TableHead className="whitespace-nowrap">
                    {t("usage.status")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {logs.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={11}
                      className="text-center text-muted-foreground"
                    >
                      {t("usage.noData")}
                    </TableCell>
                  </TableRow>
                ) : (
                  logs.map((log) => (
                    <TableRow key={log.requestId}>
                      <TableCell>
                        {new Date(log.createdAt * 1000).toLocaleString(locale)}
                      </TableCell>
                      <TableCell>
                        {log.providerName || t("usage.unknownProvider")}
                      </TableCell>
                      <TableCell className="font-mono text-xs max-w-[200px]">
                        <div
                          className="truncate"
                          title={
                            log.requestModel && log.requestModel !== log.model
                              ? `${t("usage.requestModel")}: ${log.requestModel}\n${t("usage.responseModel")}: ${log.model}`
                              : log.model
                          }
                        >
                          {log.model}
                        </div>
                        {log.requestModel && log.requestModel !== log.model && (
                          <div
                            className="truncate text-muted-foreground text-[10px]"
                            title={log.requestModel}
                          >
                            ← {log.requestModel}
                          </div>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        {fmtInt(log.inputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right">
                        {fmtInt(log.outputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right">
                        {fmtInt(log.cacheReadTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right">
                        {fmtInt(log.cacheCreationTokens, locale)}
                      </TableCell>
                      <TableCell className="text-right font-mono text-xs">
                        {(parseFiniteNumber(log.costMultiplier) ?? 1) !== 1 ? (
                          <span className="text-orange-600">
                            ×{log.costMultiplier}
                          </span>
                        ) : (
                          <span className="text-muted-foreground">×1</span>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        {fmtUsd(log.totalCostUsd, 6)}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center justify-center gap-1">
                          {(() => {
                            const durationMs =
                              typeof log.durationMs === "number"
                                ? log.durationMs
                                : log.latencyMs;
                            const durationSec = durationMs / 1000;
                            const durationColor = Number.isFinite(durationSec)
                              ? durationSec <= 5
                                ? "bg-green-100 text-green-800"
                                : durationSec <= 120
                                  ? "bg-orange-100 text-orange-800"
                                  : "bg-red-200 text-red-900"
                              : "bg-gray-100 text-gray-700";
                            return (
                              <span
                                className={`inline-flex items-center justify-center rounded-full px-2 py-0.5 text-xs ${durationColor}`}
                              >
                                {Number.isFinite(durationSec)
                                  ? `${Math.round(durationSec)}s`
                                  : "--"}
                              </span>
                            );
                          })()}
                          {log.isStreaming &&
                            log.firstTokenMs != null &&
                            (() => {
                              const firstSec = log.firstTokenMs / 1000;
                              const firstColor = Number.isFinite(firstSec)
                                ? firstSec <= 5
                                  ? "bg-green-100 text-green-800"
                                  : firstSec <= 120
                                    ? "bg-orange-100 text-orange-800"
                                    : "bg-red-200 text-red-900"
                                : "bg-gray-100 text-gray-700";
                              return (
                                <span
                                  className={`inline-flex items-center justify-center rounded-full px-2 py-0.5 text-xs ${firstColor}`}
                                >
                                  {Number.isFinite(firstSec)
                                    ? `${firstSec.toFixed(1)}s`
                                    : "--"}
                                </span>
                              );
                            })()}
                          <span
                            className={`inline-flex items-center justify-center rounded-full px-2 py-0.5 text-xs ${
                              log.isStreaming
                                ? "bg-blue-100 text-blue-800"
                                : "bg-purple-100 text-purple-800"
                            }`}
                          >
                            {log.isStreaming
                              ? t("usage.stream")
                              : t("usage.nonStream")}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell>
                        <span
                          className={`inline-flex rounded-full px-2 py-1 text-xs ${
                            log.statusCode >= 200 && log.statusCode < 300
                              ? "bg-green-100 text-green-800"
                              : "bg-red-100 text-red-800"
                          }`}
                        >
                          {log.statusCode}
                        </span>
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          {/* 分页控件 */}
          {total > 0 && (
            <div className="flex items-center justify-between px-2">
              <span className="text-sm text-muted-foreground">
                {t("usage.totalRecords", { total })}
              </span>
              <div className="flex items-center gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setPage(Math.max(0, page - 1))}
                  disabled={page === 0}
                >
                  <ChevronLeft className="h-4 w-4" />
                </Button>
                {/* 页码按钮 */}
                {(() => {
                  const pages: (number | string)[] = [];
                  if (totalPages <= 7) {
                    for (let i = 0; i < totalPages; i++) pages.push(i);
                  } else {
                    pages.push(0);
                    if (page > 2) pages.push("...");
                    for (
                      let i = Math.max(1, page - 1);
                      i <= Math.min(totalPages - 2, page + 1);
                      i++
                    ) {
                      pages.push(i);
                    }
                    if (page < totalPages - 3) pages.push("...");
                    pages.push(totalPages - 1);
                  }
                  return pages.map((p, idx) =>
                    typeof p === "string" ? (
                      <span
                        key={`ellipsis-${idx}`}
                        className="px-2 text-muted-foreground"
                      >
                        ...
                      </span>
                    ) : (
                      <Button
                        key={p}
                        variant={p === page ? "default" : "outline"}
                        size="sm"
                        className="h-8 w-8 p-0"
                        onClick={() => setPage(p)}
                      >
                        {p + 1}
                      </Button>
                    ),
                  );
                })()}
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setPage(page + 1)}
                  disabled={page >= totalPages - 1}
                >
                  <ChevronRight className="h-4 w-4" />
                </Button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
