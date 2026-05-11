import { Fragment, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  ArrowLeft,
  CheckCircle2,
  Clock3,
  Copy,
  Download,
  Eye,
  EyeOff,
  Gauge,
  History as HistoryIcon,
  KeyRound,
  Loader2,
  Radar,
  ShieldAlert,
  ShieldCheck,
  Trash2,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  verifyModelAuthenticity,
  type ModelVerifyProbe,
  type ModelVerifyProbeGroup,
  type ModelVerifyProtocol,
  type ModelVerifyResult,
  type ProbeStatus,
} from "@/lib/api/model-verify";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { ModelInputWithFetch } from "@/components/providers/forms/shared";
import { copyText } from "@/lib/clipboard";
import { cn } from "@/lib/utils";

const HISTORY_STORAGE_KEY = "cc-switch-model-verify-history";
const HISTORY_PAGE_SIZE = 10;

const statusIcon: Record<ProbeStatus, typeof CheckCircle2> = {
  passed: CheckCircle2,
  warning: AlertTriangle,
  failed: XCircle,
};

const statusClass: Record<ProbeStatus, string> = {
  passed: "text-emerald-600 dark:text-emerald-400",
  warning: "text-amber-600 dark:text-amber-400",
  failed: "text-red-600 dark:text-red-400",
};

interface ModelVerifyHistoryItem {
  id: string;
  testedAt: number;
  model: string;
  protocol: ModelVerifyProtocol;
  baseUrl: string;
  remark?: string;
  score: number;
  status: ProbeStatus;
  summary: string;
  evidenceLevel: string;
  metrics: ModelVerifyResult["metrics"];
}

export function ModelVerifyPage() {
  const { t } = useTranslation();
  const [protocol, setProtocol] = useState<ModelVerifyProtocol>("openAiChat");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [remark, setRemark] = useState("");
  const [timeoutSecs, setTimeoutSecs] = useState("45");
  const [isRunning, setIsRunning] = useState(false);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [isApiKeyVisible, setIsApiKeyVisible] = useState(false);
  const [result, setResult] = useState<ModelVerifyResult | null>(null);
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [collapsedJsonProbeIds, setCollapsedJsonProbeIds] = useState<
    Set<string>
  >(new Set());
  const [history, setHistory] = useState<ModelVerifyHistoryItem[]>([]);
  const [pageMode, setPageMode] = useState<"detect" | "history">("detect");
  const [historyPage, setHistoryPage] = useState(1);
  const [expandedHistoryId, setExpandedHistoryId] = useState<string | null>(
    null,
  );

  useEffect(() => {
    try {
      const stored = window.localStorage.getItem(HISTORY_STORAGE_KEY);
      if (stored) {
        setHistory(JSON.parse(stored));
      }
    } catch (error) {
      console.warn("[ModelVerify] Failed to load history", error);
    }
  }, []);

  const canSubmit = useMemo(
    () => baseUrl.trim() && apiKey.trim() && model.trim() && !isRunning,
    [apiKey, baseUrl, isRunning, model],
  );

  const shouldShowResults = isRunning || result != null;

  async function handleVerify() {
    if (!baseUrl.trim() || !apiKey.trim() || !model.trim()) {
      toast.error(t("modelVerify.validation.required"));
      return;
    }

    const parsedTimeout = Number.parseInt(timeoutSecs, 10);
    try {
      setIsRunning(true);
      const data = await verifyModelAuthenticity({
        protocol,
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
        model: model.trim(),
        apiVersion:
          protocol === "anthropicMessages"
            ? "2023-06-01"
            : undefined,
        timeoutSecs: Number.isFinite(parsedTimeout) ? parsedTimeout : 45,
      });
      setResult(data);
      persistHistory(data);
    } catch (error) {
      toast.error(t("modelVerify.runFailed"), {
        description: String(error),
        closeButton: true,
      });
    } finally {
      setIsRunning(false);
    }
  }

  async function handleCopyApiKey() {
    if (!apiKey.trim()) {
      toast.info(t("modelVerify.history.noApiKey"));
      return;
    }
    try {
      await copyText(apiKey);
      toast.success(t("modelVerify.history.copied"));
    } catch (error) {
      toast.error(String(error));
    }
  }

  async function handleFetchModels() {
    if (!baseUrl.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey.trim(),
        hasBaseUrl: !!baseUrl.trim(),
      });
      return;
    }

    try {
      setIsFetchingModels(true);
      const models = await fetchModelsForConfig(baseUrl.trim(), apiKey.trim());
      setFetchedModels(models);
      if (models.length === 0) {
        toast.info(t("providerForm.fetchModelsEmpty"));
      } else {
        toast.success(t("providerForm.fetchModelsSuccess", { count: models.length }));
      }
    } catch (error) {
      console.warn("[ModelVerify] Failed to fetch models", error);
      showFetchModelsError(error, t);
    } finally {
      setIsFetchingModels(false);
    }
  }

  function persistHistory(data: ModelVerifyResult) {
    const item: ModelVerifyHistoryItem = {
      id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
      testedAt: data.testedAt,
      model: data.modelRequested,
      protocol: data.protocol,
      baseUrl: baseUrl.trim(),
      remark: remark.trim() || undefined,
      score: data.overallConfidence,
      status: getHistoryStatus(data),
      summary: data.summary,
      evidenceLevel: data.evidenceLevel,
      metrics: data.metrics,
    };
    setHistory((prev) => {
      const next = [item, ...prev];
      window.localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(next));
      return next;
    });
    setHistoryPage(1);
  }

  function handleClearHistory() {
    setHistory([]);
    setExpandedHistoryId(null);
    setHistoryPage(1);
    window.localStorage.removeItem(HISTORY_STORAGE_KEY);
    toast.success(t("modelVerify.history.cleared"));
  }

  if (pageMode === "history") {
    return (
      <HistoryPage
        history={history}
        page={historyPage}
        expandedId={expandedHistoryId}
        onPageChange={setHistoryPage}
        onBack={() => setPageMode("detect")}
        onClear={handleClearHistory}
        onToggleExpanded={(id) =>
          setExpandedHistoryId((current) => (current === id ? null : id))
        }
        statusLabel={(value) => t(`modelVerify.status.${value}`)}
        t={t}
      />
    );
  }

  return (
    <div className="h-full overflow-y-auto px-6 pb-12 pt-4">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
        <Alert>
          <KeyRound className="h-4 w-4" />
          <AlertDescription>{t("modelVerify.secretNotice")}</AlertDescription>
        </Alert>

        <div className="flex justify-end">
          <Button
            type="button"
            variant="outline"
            onClick={() => setPageMode("history")}
          >
            <HistoryIcon className="mr-2 h-4 w-4" />
            {t("modelVerify.history.open", { count: history.length })}
          </Button>
        </div>

        <section className="grid gap-4 rounded-lg border bg-card p-4 shadow-sm md:grid-cols-2">
          <div className="space-y-2">
            <Label htmlFor="verifyProtocol">{t("modelVerify.protocol")}</Label>
            <Select
              value={protocol}
              onValueChange={(value) =>
                setProtocol(value as ModelVerifyProtocol)
              }
            >
              <SelectTrigger id="verifyProtocol">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="openAiChat">
                  {t("modelVerify.protocols.openAiChat")}
                </SelectItem>
                <SelectItem value="anthropicMessages">
                  {t("modelVerify.protocols.anthropicMessages")}
                </SelectItem>
                <SelectItem value="geminiGenerateContent">
                  {t("modelVerify.protocols.geminiGenerateContent")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label htmlFor="verifyModel">{t("modelVerify.model")}</Label>
            <ModelInputWithFetch
              id="verifyModel"
              value={model}
              onChange={setModel}
              placeholder="gpt-4.1"
              fetchedModels={fetchedModels}
              isLoading={isFetchingModels}
            />
          </div>

          <div className="space-y-2 md:col-span-2">
            <Label htmlFor="verifyBaseUrl">{t("modelVerify.baseUrl")}</Label>
            <Input
              id="verifyBaseUrl"
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              placeholder="https://api.example.com/v1"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="verifyApiKey">{t("modelVerify.apiKey")}</Label>
            <Input
              id="verifyApiKey"
              type={isApiKeyVisible ? "text" : "password"}
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder="sk-..."
            />
            <div className="flex flex-wrap gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setIsApiKeyVisible((value) => !value)}
              >
                {isApiKeyVisible ? (
                  <EyeOff className="mr-2 h-4 w-4" />
                ) : (
                  <Eye className="mr-2 h-4 w-4" />
                )}
                {isApiKeyVisible
                  ? t("modelVerify.hideApiKey")
                  : t("modelVerify.showApiKey")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleCopyApiKey}
              >
                <Copy className="mr-2 h-4 w-4" />
                {t("modelVerify.copyApiKey")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleFetchModels}
                disabled={isFetchingModels}
              >
                {isFetchingModels ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Download className="mr-2 h-4 w-4" />
                )}
                {t("modelVerify.fetchModels")}
              </Button>
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-2">
              <Label htmlFor="verifyRemark">{t("modelVerify.remark")}</Label>
              <Input
                id="verifyRemark"
                value={remark}
                onChange={(event) => setRemark(event.target.value)}
                placeholder={t("common.optional")}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="verifyTimeout">{t("modelVerify.timeout")}</Label>
              <Input
                id="verifyTimeout"
                type="number"
                min={5}
                max={180}
                value={timeoutSecs}
                onChange={(event) => setTimeoutSecs(event.target.value)}
              />
            </div>
          </div>

          <div className="flex justify-end md:col-span-2">
            <Button onClick={handleVerify} disabled={!canSubmit}>
              {isRunning ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Radar className="mr-2 h-4 w-4" />
              )}
              {t("modelVerify.run")}
            </Button>
          </div>
        </section>

        {shouldShowResults && (
          <section className="space-y-4">
            <div className="grid gap-3 md:grid-cols-4">
              <MetricCard
                icon={result?.success ? ShieldCheck : ShieldAlert}
                label={t("modelVerify.confidence")}
                value={result ? `${result.overallConfidence}%` : "-"}
                loading={isRunning}
              />
              <MetricCard
                icon={ShieldAlert}
                label={t("modelVerify.dilutionRisk")}
                value={result ? `${result.dilutionRisk}%` : "-"}
                loading={isRunning}
              />
              <MetricCard
                icon={Clock3}
                label={t("modelVerify.totalLatency")}
                value={formatSeconds(result?.metrics.latencySeconds)}
                loading={isRunning}
              />
              <MetricCard
                icon={Gauge}
                label={t("modelVerify.tokensPerSecond")}
                value={formatNumber(result?.metrics.tokensPerSecond)}
                loading={isRunning}
              />
            </div>

            <div className="grid gap-3 md:grid-cols-4">
              <MetricCard
                icon={Radar}
                label={t("modelVerify.evidence")}
                value={
                  result
                    ? t(`modelVerify.evidenceLevel.${result.evidenceLevel}`)
                    : "-"
                }
                loading={isRunning}
              />
              <MetricCard
                icon={Radar}
                label={t("modelVerify.inputTokens")}
                value={formatInteger(result?.metrics.inputTokens)}
                loading={isRunning}
              />
              <MetricCard
                icon={Radar}
                label={t("modelVerify.outputTokens")}
                value={formatInteger(result?.metrics.outputTokens)}
                loading={isRunning}
              />
              <MetricCard
                icon={Radar}
                label={t("modelVerify.cachedInputTokens")}
                value={formatInteger(result?.metrics.cachedInputTokens)}
                loading={isRunning}
              />
            </div>

            {result && !isRunning && (
              <Alert
                variant={result.success ? "default" : "destructive"}
                className="bg-background"
              >
                <AlertDescription>{result.summary}</AlertDescription>
              </Alert>
            )}

            <div className="space-y-4">
              {(isRunning ? pendingProbeGroups(t) : result?.probeGroups ?? []).map(
                (group) => (
                  <div
                    key={group.id}
                    className="rounded-lg border bg-card p-4 shadow-sm"
                  >
                    <div className="mb-3 flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <h3 className="font-medium">{group.label}</h3>
                        <p className="text-sm text-muted-foreground">
                          {t(`modelVerify.groupDescriptions.${group.id}`, {
                            defaultValue: "",
                          })}
                        </p>
                      </div>
                      <Badge variant="outline">
                        {isRunning
                          ? t("modelVerify.waiting")
                          : `${group.score}/${group.maxScore}`}
                      </Badge>
                    </div>
                    <div className="space-y-3">
                      {group.probes.map((probe) => {
                        const Icon = isRunning
                          ? Loader2
                          : statusIcon[probe.status];
                        return (
                          <div
                            key={probe.id}
                            className="rounded-md bg-muted/40 p-3"
                          >
                            <div className="flex flex-wrap items-center justify-between gap-3">
                              <div className="flex items-center gap-2">
                                <Icon
                                  className={cn(
                                    "h-4 w-4",
                                    isRunning
                                      ? "animate-spin text-muted-foreground"
                                      : statusClass[probe.status],
                                  )}
                                />
                                <span className="font-medium">
                                  {probe.label}
                                </span>
                                <Badge variant="outline">
                                  {isRunning
                                    ? t("modelVerify.waiting")
                                    : t(`modelVerify.status.${probe.status}`)}
                                </Badge>
                              </div>
                              <span className="text-sm text-muted-foreground">
                                {probe.latencyMs != null
                                  ? `${probe.latencyMs}ms`
                                  : "-"}
                              </span>
                            </div>
                            <p className="mt-2 text-sm text-muted-foreground">
                              {probe.message}
                            </p>
                            {probe.excerpt && !isRunning && (
                              <ProbeExcerpt
                                probe={probe}
                                collapsedJsonProbeIds={collapsedJsonProbeIds}
                                onToggleJson={(id) =>
                                  setCollapsedJsonProbeIds((current) => {
                                    const next = new Set(current);
                                    if (next.has(id)) {
                                      next.delete(id);
                                    } else {
                                      next.add(id);
                                    }
                                    return next;
                                  })
                                }
                                t={t}
                              />
                            )}
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ),
              )}
            </div>

            {result && !isRunning && result.diagnostics.length > 0 && (
              <div className="rounded-lg border bg-card p-4 shadow-sm">
                <div className="mb-3">
                  <h3 className="font-medium">
                    {t("modelVerify.diagnostics.title")}
                  </h3>
                  <p className="text-sm text-muted-foreground">
                    {t("modelVerify.diagnostics.description")}
                  </p>
                </div>
                <div className="space-y-3">
                  {result.diagnostics.map((probe) => {
                    const Icon = statusIcon[probe.status];
                    return (
                      <div key={probe.id} className="rounded-md bg-muted/40 p-3">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div className="flex items-center gap-2">
                            <Icon
                              className={cn(
                                "h-4 w-4",
                                statusClass[probe.status],
                              )}
                            />
                            <span className="font-medium">{probe.label}</span>
                            <Badge variant="outline">
                              {t(`modelVerify.status.${probe.status}`)}
                            </Badge>
                          </div>
                          <span className="text-sm text-muted-foreground">
                            {probe.latencyMs != null
                              ? `${probe.latencyMs}ms`
                              : "-"}
                          </span>
                        </div>
                        <p className="mt-2 text-sm text-muted-foreground">
                          {probe.message}
                        </p>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </section>
        )}

      </div>
    </div>
  );
}

function MetricCard({
  icon: Icon,
  label,
  value,
  loading = false,
}: {
  icon: typeof ShieldCheck;
  label: string;
  value: string;
  loading?: boolean;
}) {
  return (
    <div className="rounded-lg border bg-card p-4 shadow-sm">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        {loading ? (
          <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
        ) : (
          <Icon className="h-4 w-4" />
        )}
        {label}
      </div>
      <div className="mt-2 text-2xl font-semibold">
        {loading ? "等待中" : value}
      </div>
    </div>
  );
}

function ProbeExcerpt({
  probe,
  collapsedJsonProbeIds,
  onToggleJson,
  t,
}: {
  probe: ModelVerifyProbe;
  collapsedJsonProbeIds: Set<string>;
  onToggleJson: (id: string) => void;
  t: (key: string, options?: any) => string;
}) {
  const parsed = parseJsonExcerpt(probe.excerpt);
  if (!parsed.ok) {
    return (
      <pre className="mt-3 max-h-32 overflow-auto rounded-md bg-background p-3 text-xs text-muted-foreground">
        {probe.excerpt}
      </pre>
    );
  }

  const isCollapsed = collapsedJsonProbeIds.has(probe.id);
  return (
    <div className="mt-3 rounded-md bg-background">
      <div className="flex justify-end border-b px-3 py-2">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2"
          onClick={() => onToggleJson(probe.id)}
        >
          {isCollapsed
            ? t("modelVerify.json.expand")
            : t("modelVerify.json.collapse")}
        </Button>
      </div>
      <pre className="max-h-64 overflow-auto p-3 text-xs text-muted-foreground">
        {isCollapsed
          ? JSON.stringify(parsed.value)
          : JSON.stringify(parsed.value, null, 2)}
      </pre>
    </div>
  );
}

function HistoryPage({
  history,
  page,
  expandedId,
  onPageChange,
  onBack,
  onClear,
  onToggleExpanded,
  statusLabel,
  t,
}: {
  history: ModelVerifyHistoryItem[];
  page: number;
  expandedId: string | null;
  onPageChange: (page: number) => void;
  onBack: () => void;
  onClear: () => void;
  onToggleExpanded: (id: string) => void;
  statusLabel: (value: ProbeStatus) => string;
  t: (key: string, options?: any) => string;
}) {
  const totalPages = Math.max(1, Math.ceil(history.length / HISTORY_PAGE_SIZE));
  const currentPage = Math.min(page, totalPages);
  const startIndex = (currentPage - 1) * HISTORY_PAGE_SIZE;
  const pageItems = history.slice(startIndex, startIndex + HISTORY_PAGE_SIZE);

  return (
    <div className="h-full overflow-y-auto px-6 pb-12 pt-4">
      <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <Button type="button" variant="ghost" size="icon" onClick={onBack}>
              <ArrowLeft className="h-4 w-4" />
            </Button>
            <div>
              <h2 className="text-lg font-semibold">
                {t("modelVerify.history.title")}
              </h2>
              <p className="text-sm text-muted-foreground">
                {t("modelVerify.history.count", { count: history.length })}
              </p>
            </div>
          </div>
          <Button
            type="button"
            variant="outline"
            onClick={onClear}
            disabled={history.length === 0}
          >
            <Trash2 className="mr-2 h-4 w-4" />
            {t("modelVerify.history.clear")}
          </Button>
        </div>

        <section className="rounded-lg border bg-card p-4 shadow-sm">
          {history.length === 0 ? (
            <div className="rounded-md bg-muted/40 p-4 text-sm text-muted-foreground">
              {t("modelVerify.history.empty")}
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("modelVerify.history.time")}</TableHead>
                  <TableHead>{t("modelVerify.history.model")}</TableHead>
                  <TableHead>{t("modelVerify.history.baseUrl")}</TableHead>
                  <TableHead>{t("modelVerify.history.remark")}</TableHead>
                  <TableHead>{t("modelVerify.history.score")}</TableHead>
                  <TableHead>{t("modelVerify.history.status")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {pageItems.map((item) => (
                  <Fragment key={item.id}>
                    <TableRow
                      className="cursor-pointer"
                      onClick={() => onToggleExpanded(item.id)}
                    >
                      <TableCell>{formatDateTime(item.testedAt)}</TableCell>
                      <TableCell>{item.model}</TableCell>
                      <TableCell className="max-w-64 truncate">
                        {item.baseUrl}
                      </TableCell>
                      <TableCell className="max-w-48 truncate">
                        {item.remark || "-"}
                      </TableCell>
                      <TableCell>{item.score}%</TableCell>
                      <TableCell>{statusLabel(item.status)}</TableCell>
                    </TableRow>
                    {expandedId === item.id && (
                      <TableRow key={`${item.id}-detail`}>
                        <TableCell colSpan={6} className="bg-muted/30">
                          <div className="space-y-2 text-sm">
                            {item.remark && (
                              <div>
                                <span className="text-muted-foreground">
                                  {t("modelVerify.history.remark")}:
                                </span>{" "}
                                {item.remark}
                              </div>
                            )}
                            <div>{item.summary}</div>
                            <div className="flex flex-wrap gap-3 text-muted-foreground">
                              <span>
                                {t("modelVerify.totalLatency")}:{" "}
                                {formatSeconds(item.metrics.latencySeconds)}
                              </span>
                              <span>
                                {t("modelVerify.tokensPerSecond")}:{" "}
                                {formatNumber(item.metrics.tokensPerSecond)}
                              </span>
                              <span>
                                {t("modelVerify.inputTokens")}:{" "}
                                {formatInteger(item.metrics.inputTokens)}
                              </span>
                              <span>
                                {t("modelVerify.outputTokens")}:{" "}
                                {formatInteger(item.metrics.outputTokens)}
                              </span>
                              <span>
                                {t("modelVerify.cachedInputTokens")}:{" "}
                                {formatInteger(item.metrics.cachedInputTokens)}
                              </span>
                            </div>
                          </div>
                        </TableCell>
                      </TableRow>
                    )}
                  </Fragment>
                ))}
              </TableBody>
            </Table>
          )}

          {history.length > 0 && (
            <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t pt-4">
              <div className="text-sm text-muted-foreground">
                {t("modelVerify.history.page", {
                  current: currentPage,
                  total: totalPages,
                })}
              </div>
              <div className="flex gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={currentPage <= 1}
                  onClick={() => onPageChange(currentPage - 1)}
                >
                  {t("modelVerify.history.prev")}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={currentPage >= totalPages}
                  onClick={() => onPageChange(currentPage + 1)}
                >
                  {t("modelVerify.history.next")}
                </Button>
              </div>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function formatSeconds(value?: number) {
  return value == null ? "-" : `${value.toFixed(2)}s`;
}

function formatNumber(value?: number) {
  return value == null ? "-" : value.toFixed(1);
}

function formatInteger(value?: number) {
  return value == null ? "-" : String(value);
}

function formatDateTime(value: number) {
  return new Date(value * 1000).toLocaleString();
}

function parseJsonExcerpt(value?: string):
  | { ok: true; value: unknown }
  | { ok: false } {
  if (!value?.trim()) return { ok: false };
  try {
    return { ok: true, value: JSON.parse(value) };
  } catch {
    return { ok: false };
  }
}

function getHistoryStatus(result: ModelVerifyResult): ProbeStatus {
  if (!result.success || result.overallConfidence < 60) return "failed";
  if (result.overallConfidence < 85) return "warning";
  return "passed";
}

function pendingProbeGroups(
  t: (key: string, options?: any) => string,
): ModelVerifyProbeGroup[] {
  return [
    ["knowledgeQa", "知识问答校验"],
    ["modelFeatures", "型号特征校验"],
    ["protocolConsistency", "协议一致性"],
    ["responseStructure", "响应结构"],
  ].map(([id, label]) => ({
    id,
    label,
    score: 0,
    maxScore: 25,
    probes: [
      {
        id: `${id}.pending`,
        label,
        group: id,
        weight: 25,
        status: "warning" as ProbeStatus,
        message: t("modelVerify.waiting"),
      },
    ],
  }));
}
