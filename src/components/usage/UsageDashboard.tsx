import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { UsageHero } from "./UsageHero";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import {
  KNOWN_APP_TYPES,
  type AppTypeFilter,
  type UsageRangeSelection,
  type UsageSourceFilter,
} from "@/types/usage";
import { motion } from "framer-motion";
import {
  BarChart3,
  ChevronDown,
  Database,
  HardDrive,
  ListFilter,
  Activity,
  RefreshCw,
  Coins,
  Server,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";
import { usageApi } from "@/lib/api/usage";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { cn } from "@/lib/utils";
import { getLocaleFromLanguage } from "./format";
import { getUsageRangePresetLabel, resolveUsageRange } from "@/lib/usageRange";
import { UsageDateRangePicker } from "./UsageDateRangePicker";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { toast } from "sonner";
import { extractErrorMessage } from "@/utils/errorUtils";

const APP_FILTER_OPTIONS: AppTypeFilter[] = ["all", ...KNOWN_APP_TYPES];
const SOURCE_MULTI_SEPARATOR = "\u001f";
type SourceMode = "all" | "local" | "remote";
type RemoteSourceOption = `remote:${string}`;

const remoteHostFromSource = (source: string) =>
  source.startsWith("remote:") ? source.slice("remote:".length) : "";

const serializeSourceFilter = (
  sourceMode: SourceMode,
  selectedRemoteSources: RemoteSourceOption[],
): UsageSourceFilter => {
  if (sourceMode !== "remote") {
    return sourceMode;
  }

  if (selectedRemoteSources.length === 0) {
    return "remote";
  }

  if (selectedRemoteSources.length === 1) {
    return selectedRemoteSources[0];
  }

  return `multi:${selectedRemoteSources.join(SOURCE_MULTI_SEPARATOR)}`;
};

export function UsageDashboard() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<UsageRangeSelection>({ preset: "today" });
  const [appType, setAppType] = useState<AppTypeFilter>("all");
  const [sourceMode, setSourceMode] = useState<SourceMode>("all");
  const [selectedRemoteSources, setSelectedRemoteSources] = useState<
    RemoteSourceOption[]
  >([]);
  const [deleteRemoteSource, setDeleteRemoteSource] =
    useState<RemoteSourceOption | null>(null);
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(30000);

  const { data: dataSources } = useQuery({
    queryKey: [...usageKeys.all, "data-sources"],
    queryFn: usageApi.getDataSourceBreakdown,
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    refetchIntervalInBackground: false,
  });

  const refreshIntervalOptionsMs = [0, 5000, 10000, 30000, 60000] as const;
  const changeRefreshInterval = () => {
    const currentIndex = refreshIntervalOptionsMs.indexOf(
      refreshIntervalMs as (typeof refreshIntervalOptionsMs)[number],
    );
    const safeIndex = currentIndex >= 0 ? currentIndex : 3;
    const nextIndex = (safeIndex + 1) % refreshIntervalOptionsMs.length;
    const next = refreshIntervalOptionsMs[nextIndex];
    setRefreshIntervalMs(next);
    queryClient.invalidateQueries({ queryKey: usageKeys.all });
  };

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);
  const resolvedRange = useMemo(() => resolveUsageRange(range), [range]);
  const rangeLabel = useMemo(() => {
    if (range.preset !== "custom") {
      return getUsageRangePresetLabel(range.preset, t);
    }

    return `${new Date(resolvedRange.startDate * 1000).toLocaleString(locale)} - ${new Date(
      resolvedRange.endDate * 1000,
    ).toLocaleString(locale)}`;
  }, [locale, range, resolvedRange.endDate, resolvedRange.startDate, t]);

  const remoteSourceOptions = useMemo(() => {
    const remoteSources =
      dataSources
        ?.map((item) => item.dataSource)
        .filter((item): item is RemoteSourceOption =>
          item.startsWith("remote:"),
        ) ?? [];
    return Array.from(new Set(remoteSources));
  }, [dataSources]);
  const source = useMemo(
    () => serializeSourceFilter(sourceMode, selectedRemoteSources),
    [selectedRemoteSources, sourceMode],
  );

  const toggleRemoteSource = (option: RemoteSourceOption) => {
    setSourceMode("remote");
    setSelectedRemoteSources((previous) => {
      const next = previous.includes(option)
        ? previous.filter((item) => item !== option)
        : [...previous, option];
      return Array.from(new Set(next));
    });
  };

  const getRemoteSourceLabel = (option: RemoteSourceOption) =>
    t("usage.sourceFilter.remoteHost", {
      defaultValue: "{{host}}",
      host: remoteHostFromSource(option),
    });

  const deleteRemoteUsageMutation = useMutation({
    mutationFn: (dataSource: RemoteSourceOption) =>
      usageApi.deleteRemoteUsageData(dataSource),
    onSuccess: (result, dataSource) => {
      setSelectedRemoteSources((previous) =>
        previous.filter((item) => item !== dataSource),
      );
      setDeleteRemoteSource(null);
      void queryClient.invalidateQueries({ queryKey: usageKeys.all });
      toast.success(
        t("usage.sourceFilter.deleteSuccess", {
          defaultValue: "已删除远端用量数据",
        }),
        {
          description: t("usage.sourceFilter.deleteSuccessDescription", {
            defaultValue:
              "删除 {{logs}} 条明细、{{rollups}} 条聚合、{{states}} 条同步状态",
            logs: result.deletedRequestLogs,
            rollups: result.deletedRollups,
            states: result.deletedSyncStates,
          }),
        },
      );
    },
    onError: (error) => {
      toast.error(
        t("usage.sourceFilter.deleteFailed", {
          defaultValue: "删除远端用量数据失败",
        }),
        { description: extractErrorMessage(error) },
      );
    },
  });

  const remoteModeLabel = useMemo(() => {
    if (selectedRemoteSources.length === 0) {
      return t("usage.sourceFilter.remote", { defaultValue: "仅远程" });
    }
    if (selectedRemoteSources.length > 1) {
      return t("usage.sourceFilter.remoteHostCount", {
        defaultValue: "{{count}} 台远端",
        count: selectedRemoteSources.length,
      });
    }

    return getRemoteSourceLabel(selectedRemoteSources[0]);
  }, [selectedRemoteSources, t]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="space-y-8 pb-8"
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div className="flex flex-col gap-1">
            <h2 className="text-2xl font-bold">{t("usage.title")}</h2>
            <p className="text-sm text-muted-foreground">
              {t("usage.subtitle")}
            </p>
          </div>
        </div>

        <div className="rounded-xl border border-border/50 bg-card/40 backdrop-blur-sm p-4">
          <div className="flex flex-wrap items-center gap-1.5">
            {APP_FILTER_OPTIONS.map((type) => (
              <button
                key={type}
                type="button"
                onClick={() => setAppType(type)}
                className={cn(
                  "px-4 py-1.5 rounded-lg text-sm font-medium transition-all",
                  appType === type
                    ? "bg-primary/10 text-primary shadow-sm border border-primary/20"
                    : "text-muted-foreground hover:text-primary hover:bg-muted/50 border border-transparent",
                )}
              >
                {t(`usage.appFilter.${type}`)}
              </button>
            ))}

            <div className="mx-2 h-5 w-px bg-border" />

            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => setSourceMode("all")}
                className={cn(
                  "inline-flex h-8 items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
                  sourceMode === "all"
                    ? "border-primary/20 bg-primary/10 text-primary shadow-sm"
                    : "border-transparent text-muted-foreground hover:bg-muted/50 hover:text-primary",
                )}
              >
                <Database className="h-3.5 w-3.5" />
                {t("usage.sourceFilter.all", { defaultValue: "全部" })}
              </button>
              <button
                type="button"
                onClick={() => setSourceMode("local")}
                className={cn(
                  "inline-flex h-8 items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
                  sourceMode === "local"
                    ? "border-primary/20 bg-primary/10 text-primary shadow-sm"
                    : "border-transparent text-muted-foreground hover:bg-muted/50 hover:text-primary",
                )}
              >
                <HardDrive className="h-3.5 w-3.5" />
                {t("usage.sourceFilter.local", { defaultValue: "仅本地" })}
              </button>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <button
                    type="button"
                    onClick={() => setSourceMode("remote")}
                    className={cn(
                      "inline-flex h-8 max-w-[13rem] items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
                      sourceMode === "remote"
                        ? "border-primary/20 bg-primary/10 text-primary shadow-sm"
                        : "border-transparent text-muted-foreground hover:bg-muted/50 hover:text-primary",
                    )}
                  >
                    <Server className="h-3.5 w-3.5 shrink-0" />
                    <span className="truncate">
                      {sourceMode === "remote"
                        ? remoteModeLabel
                        : t("usage.sourceFilter.remote", {
                            defaultValue: "仅远程",
                          })}
                    </span>
                    <ChevronDown className="h-3.5 w-3.5 shrink-0 opacity-70" />
                  </button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-64">
                  <DropdownMenuLabel>
                    {t("usage.sourceFilter.label", {
                      defaultValue: "数据来源",
                    })}
                  </DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    onSelect={(event) => {
                      event.preventDefault();
                      setSourceMode("remote");
                      setSelectedRemoteSources([]);
                    }}
                    className="pl-2"
                  >
                    <Server className="h-3.5 w-3.5" />
                    <span className="truncate">
                      {t("usage.sourceFilter.remoteAggregateTitle", {
                        defaultValue: "仅统计所有远端服务器",
                      })}
                    </span>
                  </DropdownMenuItem>
                  {remoteSourceOptions.length > 0 ? (
                    <>
                      <DropdownMenuSeparator />
                      {remoteSourceOptions.map((option) => (
                        <DropdownMenuCheckboxItem
                          key={option}
                          checked={selectedRemoteSources.includes(option)}
                          onCheckedChange={() => toggleRemoteSource(option)}
                          onSelect={(event) => event.preventDefault()}
                          className="gap-2 pl-8 pr-2"
                        >
                          <Server className="h-3.5 w-3.5" />
                          <span className="min-w-0 flex-1 truncate">
                            {getRemoteSourceLabel(option)}
                          </span>
                          <button
                            type="button"
                            className="ml-2 inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                            title={t("usage.sourceFilter.deleteRemote", {
                              defaultValue: "删除该远端数据",
                            })}
                            aria-label={t("usage.sourceFilter.deleteRemote", {
                              defaultValue: "删除该远端数据",
                            })}
                            disabled={deleteRemoteUsageMutation.isPending}
                            onPointerDown={(event) => event.stopPropagation()}
                            onClick={(event) => {
                              event.preventDefault();
                              event.stopPropagation();
                              setDeleteRemoteSource(option);
                            }}
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </button>
                        </DropdownMenuCheckboxItem>
                      ))}
                    </>
                  ) : (
                    <div className="px-2 py-2 text-xs text-muted-foreground">
                      {t("usage.sourceFilter.remoteEmpty", {
                        defaultValue: "暂无远端数据",
                      })}
                    </div>
                  )}
                </DropdownMenuContent>
              </DropdownMenu>
            </div>

            <div className="ml-auto flex items-center gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-8 px-2 text-xs text-muted-foreground"
                title={t("common.refresh", "刷新")}
                onClick={changeRefreshInterval}
              >
                <RefreshCw className="mr-1 h-3.5 w-3.5" />
                {refreshIntervalMs > 0 ? `${refreshIntervalMs / 1000}s` : "--"}
              </Button>

              <UsageDateRangePicker
                selection={range}
                triggerLabel={rangeLabel}
                onApply={(nextRange) => setRange(nextRange)}
              />
            </div>
          </div>
        </div>
      </div>

      <UsageHero
        range={range}
        appType={appType === "all" ? undefined : appType}
        source={source}
        refreshIntervalMs={refreshIntervalMs}
      />

      <UsageTrendChart
        range={range}
        rangeLabel={rangeLabel}
        appType={appType}
        source={source}
        refreshIntervalMs={refreshIntervalMs}
      />

      <div className="space-y-4">
        <Tabs defaultValue="logs" className="w-full">
          <div className="flex items-center justify-between mb-4">
            <TabsList className="bg-muted/50">
              <TabsTrigger value="logs" className="gap-2">
                <ListFilter className="h-4 w-4" />
                {t("usage.requestLogs")}
              </TabsTrigger>
              <TabsTrigger value="providers" className="gap-2">
                <Activity className="h-4 w-4" />
                {t("usage.providerStats")}
              </TabsTrigger>
              <TabsTrigger value="models" className="gap-2">
                <BarChart3 className="h-4 w-4" />
                {t("usage.modelStats")}
              </TabsTrigger>
            </TabsList>
          </div>

          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.2 }}
          >
            <TabsContent value="logs" className="mt-0">
              <RequestLogTable
                range={range}
                rangeLabel={rangeLabel}
                appType={appType}
                source={source}
                refreshIntervalMs={refreshIntervalMs}
                onRangeChange={setRange}
              />
            </TabsContent>

            <TabsContent value="providers" className="mt-0">
              <ProviderStatsTable
                range={range}
                appType={appType}
                source={source}
                refreshIntervalMs={refreshIntervalMs}
              />
            </TabsContent>

            <TabsContent value="models" className="mt-0">
              <ModelStatsTable
                range={range}
                appType={appType}
                source={source}
                refreshIntervalMs={refreshIntervalMs}
              />
            </TabsContent>
          </motion.div>
        </Tabs>
      </div>

      <Accordion type="multiple" defaultValue={[]} className="w-full space-y-4">
        <AccordionItem
          value="pricing"
          className="rounded-xl glass-card overflow-hidden"
        >
          <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
            <div className="flex items-center gap-3">
              <Coins className="h-5 w-5 text-yellow-500" />
              <div className="text-left">
                <h3 className="text-base font-semibold">
                  {t("settings.advanced.pricing.title")}
                </h3>
                <p className="text-sm text-muted-foreground font-normal">
                  {t("settings.advanced.pricing.description")}
                </p>
              </div>
            </div>
          </AccordionTrigger>
          <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
            <PricingConfigPanel />
          </AccordionContent>
        </AccordionItem>
      </Accordion>

      <ConfirmDialog
        isOpen={Boolean(deleteRemoteSource)}
        title={t("usage.sourceFilter.deleteConfirmTitle", {
          defaultValue: "删除远端用量数据？",
        })}
        message={t("usage.sourceFilter.deleteConfirmMessage", {
          defaultValue:
            "这只会删除本地数据库中 {{host}} 的远端用量记录和同步状态，不会删除远端服务器上的日志文件。之后重新同步会重新导入该服务器的用量。",
          host: deleteRemoteSource
            ? remoteHostFromSource(deleteRemoteSource)
            : "",
        })}
        confirmText={
          deleteRemoteUsageMutation.isPending
            ? t("common.deleting", { defaultValue: "删除中..." })
            : t("common.delete", { defaultValue: "删除" })
        }
        cancelText={t("common.cancel")}
        onConfirm={() => {
          if (deleteRemoteSource && !deleteRemoteUsageMutation.isPending) {
            deleteRemoteUsageMutation.mutate(deleteRemoteSource);
          }
        }}
        onCancel={() => {
          if (!deleteRemoteUsageMutation.isPending) {
            setDeleteRemoteSource(null);
          }
        }}
      />
    </motion.div>
  );
}
