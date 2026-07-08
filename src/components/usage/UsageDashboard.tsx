import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { UsageHero } from "./UsageHero";
import { UsageTrendChart } from "./UsageTrendChart";
import { RequestLogTable } from "./RequestLogTable";
import { ProviderStatsTable } from "./ProviderStatsTable";
import { ModelStatsTable } from "./ModelStatsTable";
import {
  KNOWN_APP_TYPES,
  type AppType,
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
  LayoutGrid,
  Server,
  Trash2,
} from "lucide-react";
import { ProviderIcon } from "@/components/ProviderIcon";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usageKeys, useModelStats, useProviderStats } from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
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

// 0 表示关闭自动刷新（refetchInterval=false）
const REFRESH_INTERVAL_OPTIONS_MS = [0, 5000, 10000, 30000, 60000] as const;

// 与 AppSwitcher 的 appIconName 保持一致（codex 复用 openai 图标）
const APP_FILTER_ICON: Record<AppType, string> = {
  claude: "claude",
  codex: "openai",
  gemini: "gemini",
  opencode: "opencode",
};

// Select 的 "all" 哨兵和用户自定义名称同处一个值域——真有来源/模型叫 "all"
// 就会撞名（重复 value、选中即清空筛选）。动态选项统一加前缀编码隔离值域。
const DYNAMIC_OPTION_PREFIX = "v:";
const encodeOptionValue = (name: string) => `${DYNAMIC_OPTION_PREFIX}${name}`;
const decodeOptionValue = (value: string) =>
  value === "all" ? undefined : value.slice(DYNAMIC_OPTION_PREFIX.length);

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
  const [providerName, setProviderName] = useState<string | undefined>(
    undefined,
  );
  const [model, setModel] = useState<string | undefined>(undefined);
  const [refreshIntervalMs, setRefreshIntervalMs] = useState(30000);

  const source = useMemo(
    () => serializeSourceFilter(sourceMode, selectedRemoteSources),
    [selectedRemoteSources, sourceMode],
  );

  // 切应用/来源时清掉下游筛选，避免留下一个在新范围内查无数据的"幽灵"组合；
  // 切 Provider 同理清掉模型（模型选项随 Provider 级联）。
  const changeAppType = (next: AppTypeFilter) => {
    setAppType(next);
    if (next !== appType) {
      setProviderName(undefined);
      setModel(undefined);
    }
  };
  const changeSourceMode = (next: SourceMode) => {
    setSourceMode(next);
    if (next !== sourceMode) {
      setSelectedRemoteSources([]);
      setProviderName(undefined);
      setModel(undefined);
    }
  };
  const changeProviderName = (next: string | undefined) => {
    setProviderName(next);
    if (next !== providerName) {
      setModel(undefined);
    }
  };

  // 后端写入新日志时 emit `usage-log-recorded`，本 hook 立刻 invalidate 所有
  // usage 查询，实现实时刷新（仅在 Dashboard 挂载时生效，离开页面自动取消监听）
  useUsageEventBridge();

  const { data: dataSources } = useQuery({
    queryKey: [...usageKeys.all, "data-sources"],
    queryFn: usageApi.getDataSourceBreakdown,
    refetchInterval: refreshIntervalMs > 0 ? refreshIntervalMs : false,
    refetchIntervalInBackground: false,
  });

  const changeRefreshInterval = (next: number) => {
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

    const startStr = new Date(resolvedRange.startDate * 1000).toLocaleString(
      locale,
    );

    if (range.liveEndTime) {
      return `${startStr} → ${t("usage.liveEndTimeNow", "现在")}`;
    }

    const endStr = new Date(resolvedRange.endDate * 1000).toLocaleString(
      locale,
    );
    return `${startStr} - ${endStr}`;
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

  const toggleRemoteSource = (option: RemoteSourceOption) => {
    setSourceMode("remote");
    setProviderName(undefined);
    setModel(undefined);
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

  // 顶栏下拉的选项池：Provider 列表只跟应用/时间范围走（不受自身选中值影响），
  // 模型列表随所选 Provider 级联。两者都只列当前范围内真实有数据的条目。
  // refetchInterval 必须跟随面板的刷新设置——未筛选时这两个查询与统计表共享
  // query key，落下的话会以默认 30s 拖着同 key 查询一起轮询，"--" 形同虚设。
  const optionsRefetch = {
    refetchInterval:
      refreshIntervalMs > 0 ? refreshIntervalMs : (false as const),
  };
  const { data: providerOptionsData } = useProviderStats(
    range,
    { appType, source },
    optionsRefetch,
  );
  const { data: modelOptionsData } = useModelStats(
    range,
    { appType, source, providerName },
    optionsRefetch,
  );

  const providerOptions = useMemo(() => {
    const names = new Set<string>();
    for (const stat of providerOptionsData ?? []) {
      names.add(stat.providerName);
    }
    // 数据刷新后选中项可能掉出列表（如改了时间范围）；补回去保证 Select
    // 仍能渲染选中文案，用户看得见才能主动清除。
    if (providerName) names.add(providerName);
    return Array.from(names);
  }, [providerOptionsData, providerName]);

  const modelOptions = useMemo(() => {
    const names = new Set<string>();
    for (const stat of modelOptionsData ?? []) {
      names.add(stat.model);
    }
    if (model) names.add(model);
    return Array.from(names);
  }, [modelOptionsData, model]);

  return (
    <motion.div
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.4 }}
      className="space-y-8 pb-8"
    >
      <div className="flex flex-col lg:flex-row lg:items-end justify-between gap-4 mb-2">
        <div className="flex flex-col gap-1">
          <h2 className="text-2xl font-bold tracking-tight">
            {t("usage.title")}
          </h2>
          <p className="text-sm text-muted-foreground">{t("usage.subtitle")}</p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <div className="flex items-center p-1 bg-muted/30 rounded-lg border border-border/50">
            {APP_FILTER_OPTIONS.map((type) => {
              const label = t(`usage.appFilter.${type}`);
              return (
                <button
                  key={type}
                  type="button"
                  onClick={() => changeAppType(type)}
                  title={label}
                  aria-label={label}
                  className={cn(
                    "flex h-8 items-center justify-center px-2.5 rounded-md transition-all",
                    appType === type
                      ? "bg-background text-primary shadow-sm"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
                  )}
                >
                  {type === "all" ? (
                    <LayoutGrid className="h-4 w-4" />
                  ) : (
                    <ProviderIcon
                      icon={APP_FILTER_ICON[type]}
                      name={label}
                      size={16}
                    />
                  )}
                </button>
              );
            })}
          </div>

          <div className="mx-2 h-5 w-px bg-border" />

          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => changeSourceMode("all")}
              className={cn(
                "inline-flex h-9 items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
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
              onClick={() => changeSourceMode("local")}
              className={cn(
                "inline-flex h-9 items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
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
                    "inline-flex h-9 max-w-[13rem] items-center gap-1.5 rounded-lg border px-3 text-sm font-medium transition-all",
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
                    setProviderName(undefined);
                    setModel(undefined);
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

          <Select
            value={
              providerName != null ? encodeOptionValue(providerName) : "all"
            }
            onValueChange={(v) => changeProviderName(decodeOptionValue(v))}
          >
            <SelectTrigger
              className="h-9 w-[100px] bg-background text-xs focus:border-border-default [&>span]:min-w-0 [&>span]:truncate"
              title={providerName ?? t("usage.filterBySource")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="max-w-[280px]">
              <SelectItem value="all">{t("usage.allSources")}</SelectItem>
              {providerOptions.map((name) => (
                <SelectItem
                  key={name}
                  value={encodeOptionValue(name)}
                  title={name}
                  className="[&>span]:min-w-0 [&>span]:truncate"
                >
                  {name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <Select
            value={model != null ? encodeOptionValue(model) : "all"}
            onValueChange={(v) => setModel(decodeOptionValue(v))}
          >
            <SelectTrigger
              className="h-9 w-[100px] bg-background text-xs focus:border-border-default [&>span]:min-w-0 [&>span]:truncate"
              title={model ?? t("usage.filterByModel")}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="max-w-[280px]">
              <SelectItem value="all">{t("usage.allModels")}</SelectItem>
              {modelOptions.map((name) => (
                <SelectItem
                  key={name}
                  value={encodeOptionValue(name)}
                  title={name}
                  className="[&>span]:min-w-0 [&>span]:truncate"
                >
                  {name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>

          <div className="flex items-center gap-2 ml-auto lg:ml-0">
            <Select
              value={String(refreshIntervalMs)}
              onValueChange={(v) => changeRefreshInterval(Number(v))}
            >
              <SelectTrigger
                className="h-9 w-[100px] bg-background text-xs focus:border-border-default"
                title={t("usage.refreshInterval")}
                aria-label={t("usage.refreshInterval")}
              >
                <span className="flex items-center gap-2">
                  <RefreshCw className="h-3.5 w-3.5 shrink-0" />
                  <SelectValue />
                </span>
              </SelectTrigger>
              <SelectContent>
                {REFRESH_INTERVAL_OPTIONS_MS.map((ms) => (
                  <SelectItem key={ms} value={String(ms)}>
                    {ms > 0 ? `${ms / 1000}s` : t("usage.refreshOff")}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            <UsageDateRangePicker
              selection={range}
              triggerLabel={rangeLabel}
              onApply={(nextRange) => setRange(nextRange)}
            />
          </div>
        </div>
      </div>

      <UsageHero
        range={range}
        appType={appType === "all" ? undefined : appType}
        source={source}
        providerName={providerName}
        model={model}
        refreshIntervalMs={refreshIntervalMs}
      />

      <UsageTrendChart
        range={range}
        rangeLabel={rangeLabel}
        appType={appType}
        source={source}
        providerName={providerName}
        model={model}
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
                providerName={providerName}
                model={model}
                refreshIntervalMs={refreshIntervalMs}
                onRangeChange={setRange}
              />
            </TabsContent>

            <TabsContent value="providers" className="mt-0">
              <ProviderStatsTable
                range={range}
                appType={appType}
                source={source}
                providerName={providerName}
                model={model}
                refreshIntervalMs={refreshIntervalMs}
              />
            </TabsContent>

            <TabsContent value="models" className="mt-0">
              <ModelStatsTable
                range={range}
                appType={appType}
                source={source}
                providerName={providerName}
                model={model}
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
