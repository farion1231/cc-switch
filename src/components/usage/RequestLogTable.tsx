import { useEffect, useState } from "react";
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
import { useRequestLogs } from "@/lib/query/usage";
import type { LogFilters, UsageRangeSelection } from "@/types/usage";
import { ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import {
  fmtInt,
  fmtUsd,
  getLocaleFromLanguage,
  parseFiniteNumber,
} from "./format";

interface RequestLogTableProps {
  range: UsageRangeSelection;
  rangeLabel: string;
  appType?: string;
  refreshIntervalMs: number;
  onRangeChange?: (range: UsageRangeSelection) => void;
}

export function RequestLogTable({
  range,
  rangeLabel,
  appType: dashboardAppType,
  refreshIntervalMs,
  onRangeChange,
}: RequestLogTableProps) {
  const { t, i18n } = useTranslation();

  const [appliedFilters, setAppliedFilters] = useState<LogFilters>({});
  const [draftFilters, setDraftFilters] = useState<LogFilters>({});
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("");
  const pageSize = 20;

  const dashboardAppTypeActive = dashboardAppType && dashboardAppType !== "all";
  const effectiveFilters: LogFilters = dashboardAppTypeActive
    ? { ...appliedFilters, appType: dashboardAppType }
    : appliedFilters;

  const { data: result, isLoading } = useRequestLogs({
    filters: effectiveFilters,
    range,
    page,
    pageSize,
    options: {
      refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    },
  });

  const logs = result?.data ?? [];
  const total = result?.total ?? 0;
  const totalPages = Math.ceil(total / pageSize);

  useEffect(() => {
    setPage(0);
  }, [
    dashboardAppType,
    range.customEndDate,
    range.customStartDate,
    range.preset,
  ]);

  const handleSearch = () => {
    setAppliedFilters(draftFilters);
    setPage(0);
  };

  const handleReset = () => {
    setDraftFilters({});
    setAppliedFilters({});
    setPage(0);
  };

  const applySelectFilter = <K extends keyof LogFilters>(
    key: K,
    value: LogFilters[K],
  ) => {
    setDraftFilters((prev) => ({
      ...prev,
      [key]: value,
    }));
    setAppliedFilters((prev) => ({
      ...prev,
      [key]: value,
    }));
    setPage(0);
  };

  const handleGoToPage = () => {
    const trimmed = pageInput.trim();
    if (!/^\d+$/.test(trimmed)) return;
    const parsed = Number(trimmed);
    if (parsed < 1 || parsed > totalPages) return;
    setPage(parsed - 1);
    setPageInput("");
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  return (
    <div className="space-y-4">
      <div className="rounded-lg border bg-card/50 p-2 backdrop-blur-sm">
        <div className="flex flex-wrap items-center gap-1.5">
          {/* App type */}
          <Select
            value={
              dashboardAppTypeActive
                ? dashboardAppType
                : draftFilters.appType || "all"
            }
            onValueChange={(v) =>
              applySelectFilter("appType", v === "all" ? undefined : v)
            }
            disabled={!!dashboardAppTypeActive}
          >
            <SelectTrigger className="h-8 w-[110px] bg-background text-xs">
              <SelectValue placeholder={t("usage.appType")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("usage.allApps")}</SelectItem>
              <SelectItem value="claude">Claude</SelectItem>
              <SelectItem value="codex">Codex</SelectItem>
              <SelectItem value="gemini">Gemini</SelectItem>
              <SelectItem value="hermes">Hermes</SelectItem>
            </SelectContent>
          </Select>

          {/* Status code */}
          <Select
            value={draftFilters.statusCode?.toString() || "all"}
            onValueChange={(v) =>
              applySelectFilter(
                "statusCode",
                v === "all"
                  ? undefined
                  : Number.isFinite(Number.parseInt(v, 10))
                    ? Number.parseInt(v, 10)
                    : undefined,
              )
            }
          >
            <SelectTrigger className="h-8 w-[100px] bg-background text-xs">
              <SelectValue placeholder={t("usage.statusCode")} />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("common.all")}</SelectItem>
              <SelectItem value="200">200 OK</SelectItem>
              <SelectItem value="400">400</SelectItem>
              <SelectItem value="401">401</SelectItem>
              <SelectItem value="429">429</SelectItem>
              <SelectItem value="500">500</SelectItem>
            </SelectContent>
          </Select>

          {/* Provider search */}
          <div className="relative min-w-[140px] flex-1">
            <Search className="absolute left-2 top-2 h-3.5 w-3.5 text-muted-foreground" />
            <Input
              placeholder={t("usage.searchProviderPlaceholder")}
              className="h-8 bg-background pl-7 text-xs"
              value={draftFilters.providerName || ""}
              onChange={(e) =>
                setDraftFilters({
                  ...draftFilters,
                  providerName: e.target.value || undefined,
                })
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") handleSearch();
              }}
            />
          </div>

          {/* Model search */}
          <div className="relative min-w-[120px] flex-1">
            <Input
              placeholder={t("usage.searchModelPlaceholder")}
              className="h-8 bg-background text-xs"
              value={draftFilters.model || ""}
              onChange={(e) =>
                setDraftFilters({
                  ...draftFilters,
                  model: e.target.value || undefined,
                })
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") handleSearch();
              }}
            />
          </div>

          {onRangeChange && (
            <UsageDateRangePicker
              selection={range}
              triggerLabel={rangeLabel}
              onApply={onRangeChange}
            />
          )}

          {/* Search & Reset (icon-only) */}
          <Button
            size="icon"
            variant="default"
            onClick={handleSearch}
            className="h-8 w-8"
            title={t("common.search")}
          >
            <Search className="h-3.5 w-3.5" />
          </Button>
          <Button
            size="icon"
            variant="outline"
            onClick={handleReset}
            className="h-8 w-8"
            title={t("common.reset")}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
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
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.time")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.provider")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.billingModel")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.inputTokens")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.outputTokens")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.totalCost")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.timingInfo")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.status")}
                  </TableHead>
                  <TableHead className="text-center whitespace-nowrap">
                    {t("usage.source", { defaultValue: "Source" })}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {logs.length === 0 ? (
                  <TableRow>
                    <TableCell
                      colSpan={9}
                      className="text-center text-muted-foreground"
                    >
                      {t("usage.noData")}
                    </TableCell>
                  </TableRow>
                ) : (
                  logs.map((log) => (
                    <TableRow key={log.requestId}>
                      <TableCell className="text-center whitespace-nowrap text-xs px-1.5">
                        {new Date(log.createdAt * 1000).toLocaleString(locale, {
                          month: "2-digit",
                          day: "2-digit",
                          hour: "2-digit",
                          minute: "2-digit",
                        })}
                      </TableCell>
                      <TableCell className="text-center">
                        {log.providerName || t("usage.unknownProvider")}
                      </TableCell>
                      <TableCell className="text-center font-mono text-xs max-w-[200px]">
                        <div
                          className="truncate"
                          title={
                            log.requestModel && log.requestModel !== log.model
                              ? `${log.requestModel} → ${log.model}`
                              : log.model
                          }
                        >
                          {log.requestModel &&
                          log.requestModel !== log.model ? (
                            <span>
                              {log.requestModel}
                              <span className="text-muted-foreground">
                                {" → "}
                                {log.model}
                              </span>
                            </span>
                          ) : (
                            log.model
                          )}
                        </div>
                      </TableCell>
                      <TableCell className="text-center px-1.5">
                        <div className="tabular-nums">
                          {fmtInt(log.inputTokens, locale)}
                        </div>
                        {(log.cacheReadTokens > 0 ||
                          log.cacheCreationTokens > 0) && (
                          <div className="text-[10px] text-muted-foreground whitespace-nowrap">
                            {[
                              log.cacheReadTokens > 0 &&
                                `R${fmtInt(log.cacheReadTokens, locale)}`,
                              log.cacheCreationTokens > 0 &&
                                `W${fmtInt(log.cacheCreationTokens, locale)}`,
                            ]
                              .filter(Boolean)
                              .join("·")}
                          </div>
                        )}
                      </TableCell>
                      <TableCell className="text-center">
                        {fmtInt(log.outputTokens, locale)}
                      </TableCell>
                      <TableCell className="text-center px-1.5">
                        <div className="font-medium tabular-nums">
                          {fmtUsd(log.totalCostUsd, 4)}
                        </div>
                        {parseFiniteNumber(log.costMultiplier) != null &&
                          parseFiniteNumber(log.costMultiplier) !== 1 && (
                            <div className="text-[11px] text-muted-foreground">
                              ×
                              {parseFiniteNumber(log.costMultiplier)?.toFixed(
                                2,
                              )}
                            </div>
                          )}
                      </TableCell>
                      <TableCell className="text-center whitespace-nowrap text-xs tabular-nums">
                        {(log.latencyMs / 1000).toFixed(1)}s
                        {log.firstTokenMs != null && (
                          <span className="text-muted-foreground">
                            /{(log.firstTokenMs / 1000).toFixed(1)}s
                          </span>
                        )}
                      </TableCell>
                      <TableCell className="text-center">
                        <span
                          className={
                            log.statusCode >= 200 && log.statusCode < 300
                              ? "text-green-600"
                              : "text-red-600"
                          }
                        >
                          {log.statusCode}
                        </span>
                      </TableCell>
                      <TableCell className="text-center text-xs text-muted-foreground">
                        {log.dataSource || "proxy"}
                      </TableCell>
                    </TableRow>
                  ))
                )}
              </TableBody>
            </Table>
          </div>

          <div className="flex items-center justify-between text-sm text-muted-foreground">
            <span>{t("usage.totalRecords", { total })}</span>
            <div className="flex items-center gap-1">
              <Button
                size="sm"
                variant="outline"
                disabled={page === 0}
                onClick={() => setPage((p) => Math.max(0, p - 1))}
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              {(() => {
                const pages: (number | string)[] = [];
                if (totalPages <= 9) {
                  for (let i = 0; i < totalPages; i++) pages.push(i);
                } else {
                  const pageSet = new Set<number>();
                  for (let i = 0; i < 3; i++) pageSet.add(i);
                  for (let i = totalPages - 3; i < totalPages; i++)
                    pageSet.add(i);
                  for (
                    let i = Math.max(0, page - 1);
                    i <= Math.min(totalPages - 1, page + 1);
                    i++
                  )
                    pageSet.add(i);
                  const sorted = Array.from(pageSet).sort((a, b) => a - b);
                  for (let i = 0; i < sorted.length; i++) {
                    if (i > 0 && sorted[i] - sorted[i - 1] > 1) {
                      pages.push(`ellipsis-${i}`);
                    }
                    pages.push(sorted[i]);
                  }
                }
                return pages.map((p) =>
                  typeof p === "string" ? (
                    <span key={p} className="px-2 text-muted-foreground">
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
                size="sm"
                variant="outline"
                disabled={page >= totalPages - 1}
                onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
              <div className="flex items-center gap-1 ml-2">
                <Input
                  type="text"
                  value={pageInput}
                  onChange={(e) => setPageInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleGoToPage();
                  }}
                  placeholder={t("usage.pageInputPlaceholder")}
                  className="h-8 w-16 text-center text-xs"
                />
                <Button variant="outline" size="sm" onClick={handleGoToPage}>
                  {t("usage.goToPage")}
                </Button>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}
