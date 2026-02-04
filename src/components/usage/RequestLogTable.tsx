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

export function RequestLogTable() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();

  // 默认时间范围：过去24小时
  const getDefaultFilters = (): LogFilters => {
    const now = Math.floor(Date.now() / 1000);
    const oneDayAgo = now - 24 * 60 * 60;
    return { startDate: oneDayAgo, endDate: now };
  };

  const [filters, setFilters] = useState<LogFilters>(getDefaultFilters);
  const [tempFilters, setTempFilters] = useState<LogFilters>(getDefaultFilters);
  const [page, setPage] = useState(0);
  const pageSize = 20;

  const { data: result, isLoading } = useRequestLogs(filters, page, pageSize);

  const logs = result?.data ?? [];
  const total = result?.total ?? 0;
  const totalPages = Math.ceil(total / pageSize);

  const handleSearch = () => {
    setFilters(tempFilters);
    setPage(0);
  };

  const handleReset = () => {
    const defaults = getDefaultFilters();
    setTempFilters(defaults);
    setFilters(defaults);
    setPage(0);
  };

  const handleRefresh = () => {
    queryClient.invalidateQueries({
      queryKey: usageKeys.logs(filters, page, pageSize),
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

  const dateLocale =
    i18n.language === "zh"
      ? "zh-CN"
      : i18n.language === "ja"
        ? "ja-JP"
        : "en-US";

  return (
    <div className="space-y-4">
      {/* 筛选栏 */}
      <div className="flex flex-col gap-4 rounded-lg border bg-card/50 p-4 backdrop-blur-sm">
        <div className="flex flex-wrap items-center gap-3">
          <Select
            value={tempFilters.appType || "all"}
            onValueChange={(v) =>
              setTempFilters({
                ...tempFilters,
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
            value={tempFilters.statusCode?.toString() || "all"}
            onValueChange={(v) =>
              setTempFilters({
                ...tempFilters,
                statusCode: v === "all" ? undefined : parseInt(v),
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
                value={tempFilters.providerName || ""}
                onChange={(e) =>
                  setTempFilters({
                    ...tempFilters,
                    providerName: e.target.value || undefined,
                  })
                }
              />
            </div>
            <Input
              placeholder={t("usage.searchModelPlaceholder")}
              className="w-[180px] bg-background"
              value={tempFilters.model || ""}
              onChange={(e) =>
                setTempFilters({
                  ...tempFilters,
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
                tempFilters.startDate
                  ? timestampToLocalDatetime(tempFilters.startDate)
                  : ""
              }
              onChange={(e) => {
                const timestamp = localDatetimeToTimestamp(e.target.value);
                setTempFilters({
                  ...tempFilters,
                  startDate: timestamp,
                });
              }}
            />
            <span>-</span>
            <Input
              type="datetime-local"
              className="h-8 w-[200px] bg-background"
              value={
                tempFilters.endDate
                  ? timestampToLocalDatetime(tempFilters.endDate)
                  : ""
              }
              onChange={(e) => {
                const timestamp = localDatetimeToTimestamp(e.target.value);
                setTempFilters({
                  ...tempFilters,
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
                  <TableHead className="min-w-[280px] whitespace-nowrap">
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
                      colSpan={10}
                      className="text-center text-muted-foreground"
                    >
                      {t("usage.noData")}
                    </TableCell>
                  </TableRow>
                ) : (
                  logs.map((log) => (
                    <TableRow key={log.requestId}>
                      <TableCell>
                        {new Date(log.createdAt * 1000).toLocaleString(
                          dateLocale,
                        )}
                      </TableCell>
                      <TableCell>
                        {log.providerName || t("usage.unknownProvider")}
                      </TableCell>
                      <TableCell
                        className="font-mono text-sm max-w-[280px] truncate"
                        title={log.model}
                      >
                        {log.model}
                      </TableCell>
                      <TableCell className="text-right">
                        {log.inputTokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right">
                        {log.outputTokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right">
                        {log.cacheReadTokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right">
                        {log.cacheCreationTokens.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right">
                        ${parseFloat(log.totalCostUsd).toFixed(6)}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center justify-center gap-1">
                          {(() => {
                            const durationSec =
                              (log.durationMs ?? log.latencyMs) / 1000;
                            const durationColor =
                              durationSec <= 5
                                ? "bg-green-100 text-green-800"
                                : durationSec <= 120
                                  ? "bg-orange-100 text-orange-800"
                                  : "bg-red-200 text-red-900";
                            return (
                              <span
                                className={`inline-flex items-center justify-center rounded-full px-2 py-0.5 text-xs ${durationColor}`}
                              >
                                {Math.round(durationSec)}s
                              </span>
                            );
                          })()}
                          {log.isStreaming &&
                            log.firstTokenMs != null &&
                            (() => {
                              const firstSec = log.firstTokenMs / 1000;
                              const firstColor =
                                firstSec <= 5
                                  ? "bg-green-100 text-green-800"
                                  : firstSec <= 120
                                    ? "bg-orange-100 text-orange-800"
                                    : "bg-red-200 text-red-900";
                              return (
                                <span
                                  className={`inline-flex items-center justify-center rounded-full px-2 py-0.5 text-xs ${firstColor}`}
                                >
                                  {firstSec.toFixed(1)}s
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
