import { useState, useMemo, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useAvailableFilters } from "@/lib/query/usage";
import { useDeleteRequestLogsByDate, useCountRequestLogsByDate } from "@/lib/query/usage";
import { usageApi } from "@/lib/api/usage";
import type { LogFilters } from "@/types/usage";
import { X, ChevronDown, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog";
import { LogDetail } from "./LogDetail";
import { fmtUsd } from "../usage/format";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { toast } from "sonner";

interface LogListProps {
  startDate?: number;
  endDate?: number;
  providerFilter?: string;
  modelFilter?: string;
  refreshIntervalMs?: number;
  manualRefreshTrigger?: number; // External manual refresh trigger
}

interface ProviderInfo {
  id: string;
  name: string;
}

// Get initial filter values from URL or defaults
const getInitialFilterFromUrl = () => {
  const params = new URLSearchParams(window.location.search);
  return {
    provider: params.get("provider") || "",
    model: params.get("model") || "",
    protocol: params.get("protocol") || "",
    status: (params.get("status") as "all" | "success" | "error") || "all",
    page: parseInt(params.get("page") || "0", 10),
    pageSize: parseInt(params.get("pageSize") || "10", 10),
  };
};

export function LogList({ startDate, endDate, providerFilter, modelFilter, refreshIntervalMs, manualRefreshTrigger }: LogListProps) {
  const { t } = useTranslation();
  const refreshIntervalRef = useRef<number | undefined>(undefined);
  const initialFilters = useMemo(() => getInitialFilterFromUrl(), []);
  const [page, setPage] = useState(initialFilters.page);
  const [pageSize, setPageSize] = useState(initialFilters.pageSize);
  const [selectedRequestId, setSelectedRequestId] = useState<string | null>(null);
  const [selectedProvider, setSelectedProvider] = useState(initialFilters.provider);
  const [selectedModel, setSelectedModel] = useState(initialFilters.model);
  const [selectedProtocol, setSelectedProtocol] = useState(initialFilters.protocol);
  const [selectedStatusFilter, setSelectedStatusFilter] = useState<"all" | "success" | "error">(initialFilters.status);

  // Delete dialog state
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [deleteDays, setDeleteDays] = useState<number | null>(null); // null means delete all
  const [deleteCount, setDeleteCount] = useState<number>(0);

  // Protocol options list - only include provider types
  const PROTOCOL_OPTIONS = [
    { value: "claude", label: "Claude" },
    { value: "codex", label: "Codex" },
    { value: "gemini", label: "Gemini" },
  ];

  // When time range or status filter changes, reset page number
  useEffect(() => {
    setPage(0);
  }, [startDate, endDate, selectedStatusFilter, selectedProtocol, pageSize]);

  // Update URL when filters change
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    if (selectedProvider) {
      params.set("provider", selectedProvider);
    } else {
      params.delete("provider");
    }
    if (selectedModel) {
      params.set("model", selectedModel);
    } else {
      params.delete("model");
    }
    if (selectedProtocol) {
      params.set("protocol", selectedProtocol);
    } else {
      params.delete("protocol");
    }
    if (selectedStatusFilter !== "all") {
      params.set("status", selectedStatusFilter);
    } else {
      params.delete("status");
    }
    if (page > 0) {
      params.set("page", page.toString());
    } else {
      params.delete("page");
    }
    if (pageSize !== 10) {
      params.set("pageSize", pageSize.toString());
    } else {
      params.delete("pageSize");
    }
    const newUrl = `${window.location.pathname}?${params.toString()}`;
    window.history.replaceState({}, "", newUrl);
  }, [selectedProvider, selectedModel, selectedProtocol, selectedStatusFilter, page, pageSize]);

  // Use parent filter if provided, otherwise use local selection
  const effectiveProviderFilter = providerFilter || selectedProvider;
  const effectiveModelFilter = modelFilter || selectedModel;

  // Refresh counter - increment on each refresh trigger
  const [refreshCounter, setRefreshCounter] = useState(0);

  // Local state to store log data
  const [logsData, setLogsData] = useState<{ data: any[]; total: number } | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // Listen for external manual refresh trigger
  useEffect(() => {
    if (manualRefreshTrigger !== undefined && manualRefreshTrigger > 0) {
      setRefreshCounter(c => c + 1);
    }
  }, [manualRefreshTrigger]);

  // Calculate rolling time range
  const getRollingRange = (windowSeconds: number) => {
    const endDate = Math.floor(Date.now() / 1000);
    const startDate = endDate - windowSeconds;
    return { startDate, endDate };
  };

  // Directly call API to fetch data
  useEffect(() => {
    let cancelled = false;

    async function fetchData() {
      setIsLoading(true);
      try {
        const filters: LogFilters = {
          providerId: effectiveProviderFilter || undefined,
          model: effectiveModelFilter || undefined,
          statusFilter: selectedStatusFilter === "all" ? undefined : selectedStatusFilter,
          providerType: selectedProtocol || undefined,
          startDate: startDate,
          endDate: endDate,
        };

        // Determine date range to use based on time mode
        const timeMode = startDate !== undefined && endDate !== undefined ? "fixed" : "rolling";
        const effectiveFilters = timeMode === "rolling"
          ? { ...filters, ...getRollingRange(24 * 60 * 60) } // 24-hour rolling window
          : filters;

        const result = await usageApi.getRequestLogs(effectiveFilters, page, pageSize);

        if (!cancelled) {
          setLogsData(result);
        }
      } catch (error) {
        console.error("[LogList] Error fetching data:", error);
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    fetchData();

    return () => {
      cancelled = true;
    };
  }, [refreshCounter, page, pageSize, effectiveProviderFilter, effectiveModelFilter, selectedStatusFilter, selectedProtocol, startDate, endDate]);

  // Auto refresh: use timer to increment refreshCounter, force new query creation
  useEffect(() => {
    // If interval hasn't changed, do nothing
    if (refreshIntervalMs === refreshIntervalRef.current) {
      return;
    }
    refreshIntervalRef.current = refreshIntervalMs;

    // Clear previous timer
    if (refreshIntervalRef.current !== undefined && refreshIntervalRef.current > 0) {
      const timer = setInterval(() => {
        setRefreshCounter(c => c + 1);
      }, refreshIntervalRef.current);
      return () => {
        clearInterval(timer);
      };
    }
  }, [refreshIntervalMs]);

  // Get all available Provider and Model filter options for current time range
  const { data: availableFilters, isLoading: isFiltersLoading } = useAvailableFilters(startDate, endDate);

  // Delete mutations
  const deleteLogsMutation = useDeleteRequestLogsByDate();
  const countLogsMutation = useCountRequestLogsByDate();

  // Helper function to prepare delete (count logs first, then open confirm dialog)
  const prepareDelete = (days: number | null) => {
    const endDate = days === null
      ? Math.floor(Date.now() / 1000)
      : Math.floor(Date.now() / 1000) - days * 24 * 60 * 60;
    const startDate = 0;
    setDeleteDays(days);
    countLogsMutation.mutate(
      { startDate, endDate },
      {
        onSuccess: (count) => {
          setDeleteCount(count);
          setDeleteDialogOpen(true);
        },
      }
    );
  };

  // Get Provider and Model lists from available filter options
  const allProviders = useMemo((): ProviderInfo[] => {
    if (!availableFilters?.providers) return [];
    return availableFilters.providers.sort((a, b) => a.name.localeCompare(b.name));
  }, [availableFilters]);

  const allModels = useMemo(() => {
    if (!availableFilters?.models) return [];
    return availableFilters.models.sort();
  }, [availableFilters]);

  const totalPages = logsData ? Math.ceil(logsData.total / pageSize) : 0;

  // Show loading state
  if (isLoading || isFiltersLoading) {
    return <div className="h-[400px] animate-pulse rounded bg-gray-100" />;
  }

  return (
    <div className="flex flex-col h-full">
      {/* Filter dropdowns - fixed */}
      <div className="flex flex-wrap items-center gap-2 shrink-0">
        {/* Delete button with dropdown */}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <Button variant="destructive" size="sm" className="h-8">
              <Trash2 className="h-4 w-4 mr-1" />
              {t("logs.delete")}
              <ChevronDown className="ml-1 h-3 w-3" />
            </Button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="min-w-[150px] bg-background rounded-md border shadow-md p-1">
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => prepareDelete(1)}
              >
                {t("logs.delete1Day")}
              </DropdownMenu.Item>
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => prepareDelete(7)}
              >
                {t("logs.delete7Days")}
              </DropdownMenu.Item>
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => prepareDelete(30)}
              >
                {t("logs.delete30Days")}
              </DropdownMenu.Item>
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => prepareDelete(90)}
              >
                {t("logs.delete90Days")}
              </DropdownMenu.Item>
              <DropdownMenu.Separator className="h-px bg-border my-1" />
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent text-destructive"
                onClick={() => prepareDelete(null)}
              >
                {t("logs.deleteAll")}
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        {/* Filter group - using TabsList style */}
        <Tabs className="flex">
          <TabsList className="h-10 p-1 bg-card/60 border border-border/50 backdrop-blur-sm flex items-center">
            {/* Provider filter */}
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <Button variant="ghost" size="sm" className="h-8 px-4 text-xs data-[state=active]:bg-primary/10 data-[state=active]:text-primary">
                  {effectiveProviderFilter
                    ? allProviders.find(p => p.id === effectiveProviderFilter)?.name || effectiveProviderFilter
                    : t("logs.allProviders")}
                  <ChevronDown className="ml-1 h-3 w-3" />
                </Button>
              </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="min-w-[180px] bg-background rounded-md border shadow-md p-1">
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => setSelectedProvider("")}
              >
                {t("logs.allProviders")}
              </DropdownMenu.Item>
              {allProviders.map((provider) => (
                <DropdownMenu.Item
                  key={provider.id}
                  className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                  onClick={() => setSelectedProvider(provider.id)}
                >
                  {provider.name}
                </DropdownMenu.Item>
              ))}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <Button variant="ghost" size="sm" className="h-8 px-4 text-xs">
              {effectiveModelFilter || t("logs.allModels")}
              <ChevronDown className="ml-1 h-3 w-3" />
            </Button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="min-w-[200px] max-h-[300px] overflow-auto bg-background rounded-md border shadow-md p-1">
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => setSelectedModel("")}
              >
                {t("logs.allModels")}
              </DropdownMenu.Item>
              {allModels.map((model) => (
                <DropdownMenu.Item
                  key={model}
                  className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent font-mono"
                  onClick={() => setSelectedModel(model)}
                >
                  {model}
                </DropdownMenu.Item>
              ))}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>

        {/* Protocol filter */}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <Button variant="ghost" size="sm" className="h-8 px-4 text-xs">
              {selectedProtocol
                ? PROTOCOL_OPTIONS.find(p => p.value === selectedProtocol)?.label || selectedProtocol
                : t("logs.allProtocols")}
              <ChevronDown className="ml-1 h-3 w-3" />
            </Button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="min-w-[120px] bg-background rounded-md border shadow-md p-1">
              <DropdownMenu.Item
                className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                onClick={() => setSelectedProtocol("")}
              >
                {t("logs.allProtocols")}
              </DropdownMenu.Item>
              {PROTOCOL_OPTIONS.map((protocol) => (
                <DropdownMenu.Item
                  key={protocol.value}
                  className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                  onClick={() => setSelectedProtocol(protocol.value)}
                >
                  {protocol.label}
                </DropdownMenu.Item>
              ))}
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
          </TabsList>
        </Tabs>

        {/* Status filter - using Tabs style */}
        <div className="flex items-center h-10 ml-auto">
          <Tabs value={selectedStatusFilter} onValueChange={(v) => setSelectedStatusFilter(v as "all" | "success" | "error")}>
            <TabsList className="h-10 p-1 bg-card/60 border border-border/50 backdrop-blur-sm">
              <TabsTrigger value="all" className="text-xs px-4 h-8 data-[state=active]:bg-primary/10 data-[state=active]:text-primary">{t("logs.all")}</TabsTrigger>
              <TabsTrigger value="success" className="text-xs px-4 h-8 data-[state=active]:bg-primary/10 data-[state=active]:text-primary">{t("logs.success")}</TabsTrigger>
              <TabsTrigger value="error" className="text-xs px-4 h-8 data-[state=active]:bg-primary/10 data-[state=active]:text-primary">{t("logs.error")}</TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
      </div>

      <div className="rounded-lg border border-border/50 bg-card/40 backdrop-blur-sm overflow-hidden flex flex-col min-h-0 flex-1 mt-2">
        {/* Single table, header is sticky fixed */}
        <div className="overflow-auto h-full">
          <table className="w-full caption-bottom text-sm">
            <thead className="sticky top-0 z-10 bg-background border-b-4 border-border shadow-inner">
              <tr>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.time")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.provider")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.model")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.tokensIO")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.ttft")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.tps")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.cost")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.latency")}</th>
                <th className="h-12 px-4 text-center align-middle font-medium text-muted-foreground">{t("logs.status")}</th>
              </tr>
            </thead>
            <tbody className="[&_tr:last-child]:border-0">
              {logsData?.data.length === 0 ? (
                <tr>
                  <td colSpan={9} className="p-3 align-middle text-center text-muted-foreground">
                    {t("logs.noData")}
                  </td>
                </tr>
              ) : (
                logsData?.data.map((log) => (
                  <tr
                    key={log.requestId}
                    className="border-b border-border-default transition-colors hover:bg-muted/50 cursor-pointer"
                    onClick={() => setSelectedRequestId(log.requestId)}
                  >
                    <td className="p-3 align-middle text-center text-sm">
                      {new Date(log.createdAt * 1000).toLocaleString()}
                    </td>
                    <td className="p-3 align-middle text-center text-sm">{log.providerName || log.providerId}</td>
                    <td className="p-3 align-middle text-center font-mono text-sm">{log.model}</td>
                    <td className="p-3 align-middle text-center text-sm">
                      {log.inputTokens.toLocaleString()} / {log.outputTokens.toLocaleString()}
                    </td>
                    <td className="p-3 align-middle text-center text-sm">
                      {log.isStreaming && log.firstTokenMs ? `${log.firstTokenMs}ms` : '-'}
                    </td>
                    <td className="p-3 align-middle text-center text-sm">
                      {log.isStreaming && log.firstTokenMs
                        ? (() => {
                            const outputTimeMs = log.latencyMs - log.firstTokenMs;
                            return outputTimeMs > 0 && log.outputTokens > 0
                              ? `${(log.outputTokens * 1000 / outputTimeMs).toFixed(2)}`
                              : '-';
                          })()
                        : log.latencyMs && log.outputTokens > 0
                          ? `${(log.outputTokens * 1000 / log.latencyMs).toFixed(2)}`
                          : '-'}
                    </td>
                    <td className="p-3 align-middle text-center text-sm">
                      {fmtUsd(log.totalCostUsd, 4)}
                    </td>
                    <td className="p-3 align-middle text-center text-sm">
                      {log.latencyMs}ms
                    </td>
                    <td className="p-3 align-middle text-center">
                      <span
                        className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${
                          log.statusCode >= 200 && log.statusCode < 300
                            ? "bg-green-100 text-green-800"
                            : log.statusCode >= 400
                              ? "bg-red-100 text-red-800"
                              : "bg-yellow-100 text-yellow-800"
                        }`}
                      >
                        {log.statusCode}
                      </span>
                    </td>
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      </div>

      {/* Pagination - fixed at bottom */}
      {logsData && (
        <div className="flex items-center justify-between shrink-0 mt-2 mb-2">
          <div className="flex items-center gap-3">
            <div className="text-sm text-muted-foreground">
              {logsData.total > 0 ? (
                t("logs.pageInfo", {
                  page: page + 1,
                  total: totalPages,
                  count: logsData.total,
                })
              ) : (
                t("logs.totalCount", {
                  count: logsData.total,
                })
              )}
            </div>
            {/* Per page size filter */}
            <DropdownMenu.Root>
              <DropdownMenu.Trigger asChild>
                <Button variant="outline" size="sm" className="h-8 min-w-[80px] justify-between">
                  {t("logs.perPage", { size: pageSize })}
                  <ChevronDown className="ml-1 h-3 w-3 opacity-50" />
                </Button>
              </DropdownMenu.Trigger>
              <DropdownMenu.Portal>
                <DropdownMenu.Content className="min-w-[80px] bg-background rounded-md border shadow-md p-1">
                  {[10, 20, 50, 100, 200, 500].map((size) => (
                    <DropdownMenu.Item
                      key={size}
                      className="cursor-pointer px-2 py-1.5 text-sm rounded hover:bg-accent"
                      onClick={() => setPageSize(size)}
                    >
                      {size} {t("logs.perPage", { size: "" }).replace("{{size}}", "")}
                    </DropdownMenu.Item>
                  ))}
                </DropdownMenu.Content>
              </DropdownMenu.Portal>
            </DropdownMenu.Root>
          </div>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              disabled={page === 0}
            >
              {t("logs.prev")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              disabled={page >= totalPages - 1}
            >
              {t("logs.next")}
            </Button>
          </div>
        </div>
      )}

      {/* Detail dialog */}
      <Dialog open={selectedRequestId !== null} onOpenChange={(open) => !open && setSelectedRequestId(null)}>
        <DialogContent
          className="max-w-3xl max-h-[85vh] overflow-hidden flex flex-col p-0 z-[100]"
          overlayClassName="cursor-pointer z-[99]"
          closeOnOverlayClick={true}
        >
          <div className="flex items-center justify-between px-4 py-3 border-b shrink-0">
            <DialogTitle className="text-lg font-semibold">{t("logs.requestDetail")}</DialogTitle>
            <Button
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0 hover:bg-muted"
              onClick={() => setSelectedRequestId(null)}
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
          <div className="flex-1 overflow-y-auto px-4 pb-4">
            {selectedRequestId && <LogDetail key={selectedRequestId} requestId={selectedRequestId} />}
          </div>
        </DialogContent>
      </Dialog>

      {/* Delete confirm dialog */}
      <ConfirmDialog
        isOpen={deleteDialogOpen}
        title={t("logs.deleteConfirmTitle")}
        message={t("logs.deleteConfirmMessage", { count: deleteCount, days: deleteDays === null ? t("logs.all") : t("logs.daysAgo", { days: deleteDays }) })}
        confirmText={t("logs.delete")}
        cancelText={t("common.cancel")}
        onConfirm={() => {
          const endDate = deleteDays === null
            ? Math.floor(Date.now() / 1000)
            : Math.floor(Date.now() / 1000) - deleteDays * 24 * 60 * 60;
          const startDate = 0;
          deleteLogsMutation.mutate(
            { startDate, endDate },
            {
              onSuccess: (deletedCount) => {
                setDeleteDialogOpen(false);
                // Trigger refresh with a small delay to ensure state is properly updated
                setTimeout(() => {
                  setRefreshCounter(c => c + 1);
                }, 0);
                toast.success(t("logs.deleteSuccess"));
              },
              onError: (error) => {
                setDeleteDialogOpen(false);
                toast.error(t("logs.deleteError") + ": " + String(error));
              },
            }
          );
        }}
        onCancel={() => {
          setDeleteDialogOpen(false);
          setDeleteDays(null);
        }}
      />
    </div>
  );
}
