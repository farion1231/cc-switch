import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";
import { Check, Loader2, Search } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useUpdateModelPricing } from "@/lib/query/usage";
import { isTextEditableTarget } from "@/utils/domUtils";

const MODELS_DEV_API_URL = "https://models.dev/api.json";
// 全量约 5000 条：默认只展示最新发布的一批，搜索时才做全量匹配
const DEFAULT_VISIBLE_ROWS = 50;
const MAX_VISIBLE_ROWS = 200;

interface ModelsDevCost {
  input?: number;
  output?: number;
  cache_read?: number;
  cache_write?: number;
}

interface ModelsDevModel {
  id?: string;
  name?: string;
  release_date?: string;
  cost?: ModelsDevCost;
}

interface ModelsDevProvider {
  id?: string;
  name?: string;
  models?: Record<string, ModelsDevModel>;
}

type ModelsDevResponse = Record<string, ModelsDevProvider>;

interface ModelsDevEntry {
  /** providerId/modelId，同一模型可能出现在多个供应商下 */
  key: string;
  providerId: string;
  providerName: string;
  modelId: string;
  /** 实际入库的 ID，与后端 clean_model_id_for_pricing 的归一化规则一致 */
  normalizedId: string;
  modelName: string;
  /** YYYY-MM-DD 或 YYYY-MM，缺失时为空串 */
  releaseDate: string;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}

/**
 * 与后端 clean_model_id_for_pricing（usage_stats.rs）保持一致：
 * 取最后一个 '/' 之后的段、去掉 ':' 后缀、'@' 换成 '-'、转小写、去掉 [1m] 标记。
 * 成本归因查询用的就是这种归一化形式，原样入库的 ID 永远匹配不上。
 */
export function normalizeModelIdForPricing(modelId: string): string {
  const afterSlash = modelId.slice(modelId.lastIndexOf("/") + 1);
  const beforeColon = afterSlash.split(":")[0] ?? "";
  let normalized = beforeColon.trim().replace(/@/g, "-").toLowerCase();
  if (normalized.endsWith("[1m]")) {
    normalized = normalized.slice(0, -"[1m]".length).trim();
  }
  return normalized;
}

/** 转成后端可解析的非负十进制字符串（不能用 String()，小数可能变成科学计数法） */
export function formatPrice(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0";
  // toFixed 对 >=1e21 会退化成科学计数法；这种量级的"价格"只可能是脏数据，按 0 处理
  if (value >= 1e12) return "0";
  const trimmed = value.toFixed(6).replace(/0+$/, "").replace(/\.$/, "");
  return trimmed || "0";
}

export function flattenModels(data: ModelsDevResponse): ModelsDevEntry[] {
  const entries: ModelsDevEntry[] = [];
  for (const [providerId, provider] of Object.entries(data)) {
    if (!provider || typeof provider !== "object") continue;
    const providerName = provider.name || providerId;
    for (const [modelId, model] of Object.entries(provider.models ?? {})) {
      const cost = model?.cost;
      const input = typeof cost?.input === "number" ? cost.input : null;
      const output = typeof cost?.output === "number" ? cost.output : null;
      if (input === null && output === null) continue;
      const normalizedId = normalizeModelIdForPricing(modelId);
      if (!normalizedId) continue;
      entries.push({
        key: `${providerId}/${modelId}`,
        providerId,
        providerName,
        modelId,
        normalizedId,
        modelName: model?.name || modelId,
        releaseDate:
          typeof model?.release_date === "string" ? model.release_date : "",
        input: input ?? 0,
        output: output ?? 0,
        cacheRead: typeof cost?.cache_read === "number" ? cost.cache_read : 0,
        cacheWrite:
          typeof cost?.cache_write === "number" ? cost.cache_write : 0,
      });
    }
  }
  // 最新发布的排在前面
  entries.sort(
    (a, b) =>
      b.releaseDate.localeCompare(a.releaseDate) ||
      a.modelName.localeCompare(b.modelName),
  );
  return entries;
}

interface ModelsDevPickerDialogProps {
  open: boolean;
  onClose: () => void;
  /** 导入成功后调用（此时定价列表已刷新） */
  onImported: () => void;
}

export function ModelsDevPickerDialog({
  open,
  onClose,
  onImported,
}: ModelsDevPickerDialogProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();

  const [search, setSearch] = useState("");
  const [providerFilter, setProviderFilter] = useState("all");
  const [selected, setSelected] = useState<ModelsDevEntry | null>(null);

  // 每次打开时重置选择与过滤条件
  useEffect(() => {
    if (open) {
      setSearch("");
      setProviderFilter("all");
      setSelected(null);
    }
  }, [open]);

  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ["models-dev-pricing"],
    queryFn: async (): Promise<ModelsDevResponse> => {
      const res = await fetch(MODELS_DEV_API_URL);
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      return res.json();
    },
    enabled: open,
    staleTime: 60 * 60 * 1000,
    retry: 1,
  });

  const entries = useMemo(() => (data ? flattenModels(data) : []), [data]);

  const providers = useMemo(() => {
    const map = new Map<string, string>();
    for (const entry of entries) {
      if (!map.has(entry.providerId)) {
        map.set(entry.providerId, entry.providerName);
      }
    }
    return Array.from(map, ([id, name]) => ({ id, name })).sort((a, b) =>
      a.name.localeCompare(b.name),
    );
  }, [entries]);

  const isFiltering = search.trim() !== "" || providerFilter !== "all";

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    return entries.filter(
      (entry) =>
        (providerFilter === "all" || entry.providerId === providerFilter) &&
        (!query ||
          entry.modelId.toLowerCase().includes(query) ||
          entry.normalizedId.includes(query) ||
          entry.modelName.toLowerCase().includes(query) ||
          entry.providerName.toLowerCase().includes(query)),
    );
  }, [entries, search, providerFilter]);

  // 默认只展示最新发布的一批，搜索/筛选时展示全量匹配（设上限防卡顿）
  const visible = useMemo(
    () =>
      filtered.slice(0, isFiltering ? MAX_VISIBLE_ROWS : DEFAULT_VISIBLE_ROWS),
    [filtered, isFiltering],
  );

  // 单选：点击未选中的行替换选择，点击已选中的行取消选择。
  // 限制单选是为了避免批量导入时每条都触发一次全量零成本回填扫描（见 update_model_pricing）。
  const toggleEntry = (entry: ModelsDevEntry) => {
    setSelected((prev) => (prev?.key === entry.key ? null : entry));
  };

  const handleImport = async () => {
    if (!selected) return;

    try {
      await updatePricing.mutateAsync({
        modelId: selected.normalizedId,
        displayName: selected.modelName,
        inputCost: formatPrice(selected.input),
        outputCost: formatPrice(selected.output),
        cacheReadCost: formatPrice(selected.cacheRead),
        cacheCreationCost: formatPrice(selected.cacheWrite),
      });

      toast.success(
        t("usage.modelsDevImported", {
          name: selected.modelName,
          defaultValue: "已导入 {{name}} 的定价",
        }),
        { closeButton: true },
      );
      onImported();
    } catch (error) {
      toast.error(String(error));
    }
  };

  const priceColumns = (entry: ModelsDevEntry) =>
    [
      { label: t("usage.inputCost", "输入成本"), value: entry.input },
      { label: t("usage.outputCost", "输出成本"), value: entry.output },
      { label: t("usage.cacheReadCost", "缓存命中"), value: entry.cacheRead },
      {
        label: t("usage.cacheWriteCost", "缓存创建"),
        value: entry.cacheWrite,
      },
    ] as const;

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !updatePricing.isPending) {
          onClose();
        }
      }}
    >
      <DialogContent
        zIndex="top"
        className="max-w-3xl h-[80vh]"
        onEscapeKeyDown={(e) => {
          // 在搜索框里按 ESC 不应关闭弹窗丢掉已选模型（与 FullScreenPanel 的约定一致）
          if (isTextEditableTarget(e.target)) {
            e.preventDefault();
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>
            {t("usage.modelsDevPickerTitle", "从 models.dev 导入定价")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "usage.modelsDevPickerDesc",
              "选择要导入的模型（价格单位：USD / 百万 tokens），每次导入一个",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-1 min-h-0 flex-col gap-3 px-6 py-4">
          {isLoading ? (
            <div className="flex flex-1 items-center justify-center">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          ) : error ? (
            <Alert variant="destructive">
              <AlertDescription className="flex items-center justify-between gap-3">
                <span>
                  {t("usage.modelsDevLoadError", "加载 models.dev 数据失败")}:{" "}
                  {error instanceof Error ? error.message : String(error)}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => refetch()}
                  className="shrink-0"
                >
                  {t("usage.modelsDevRetry", "重试")}
                </Button>
              </AlertDescription>
            </Alert>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <Select
                  value={providerFilter}
                  onValueChange={setProviderFilter}
                >
                  <SelectTrigger className="w-44 shrink-0">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent className="z-[120] max-h-[min(24rem,var(--radix-select-content-available-height))]">
                    <SelectItem value="all">
                      {t("usage.modelsDevAllProviders", "全部供应商")}
                    </SelectItem>
                    {providers.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <div className="relative flex-1">
                  <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                  <Input
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    placeholder={t(
                      "usage.modelsDevSearchPlaceholder",
                      "搜索模型或供应商（全量搜索）...",
                    )}
                    className="pl-8"
                  />
                </div>
              </div>

              <div className="flex-1 min-h-0 overflow-y-auto rounded-md border border-border/50">
                {filtered.length === 0 ? (
                  <div className="flex h-full items-center justify-center py-8 text-sm text-muted-foreground">
                    {t("usage.modelsDevNoResults", "没有匹配的模型")}
                  </div>
                ) : (
                  <div className="divide-y divide-border/30">
                    {visible.map((entry) => (
                      <div
                        key={entry.key}
                        role="button"
                        aria-pressed={selected?.key === entry.key}
                        onClick={() => toggleEntry(entry)}
                        className={`flex cursor-pointer items-center gap-3 px-3 py-2 ${
                          selected?.key === entry.key
                            ? "bg-accent/50"
                            : "hover:bg-muted/40"
                        }`}
                      >
                        <Check
                          className={`h-4 w-4 shrink-0 text-primary ${
                            selected?.key === entry.key
                              ? "visible"
                              : "invisible"
                          }`}
                        />
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <span className="truncate text-sm font-medium">
                              {entry.modelName}
                            </span>
                            <span className="shrink-0 text-xs text-muted-foreground">
                              {entry.providerName}
                            </span>
                            {entry.releaseDate && (
                              <span className="shrink-0 text-[10px] text-muted-foreground/70">
                                {entry.releaseDate}
                              </span>
                            )}
                          </div>
                          <div
                            className="truncate font-mono text-xs text-muted-foreground"
                            title={entry.modelId}
                          >
                            {entry.normalizedId}
                          </div>
                        </div>
                        <div className="flex shrink-0 gap-3 text-right">
                          {priceColumns(entry).map((column) => (
                            <div key={column.label} className="w-16">
                              <div className="text-[10px] text-muted-foreground">
                                {column.label}
                              </div>
                              <div className="font-mono text-xs">
                                ${formatPrice(column.value)}
                              </div>
                            </div>
                          ))}
                        </div>
                      </div>
                    ))}
                    {filtered.length > visible.length && (
                      <div className="px-3 py-2 text-center text-xs text-muted-foreground">
                        {isFiltering
                          ? t("usage.modelsDevTruncated", {
                              shown: visible.length,
                              total: filtered.length,
                              defaultValue:
                                "仅显示前 {{shown}} 条，共 {{total}} 条结果，请缩小搜索范围",
                            })
                          : t("usage.modelsDevDefaultHint", {
                              shown: visible.length,
                              total: filtered.length,
                              defaultValue:
                                "默认展示最新发布的 {{shown}} 个模型（共 {{total}} 个），输入关键字可全量搜索",
                            })}
                      </div>
                    )}
                  </div>
                )}
              </div>
            </>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={onClose}
            disabled={updatePricing.isPending}
          >
            {t("common.cancel", "取消")}
          </Button>
          <Button
            onClick={handleImport}
            disabled={!selected || updatePricing.isPending}
          >
            {updatePricing.isPending ? (
              <>
                <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                {t("usage.modelsDevImporting", "导入中...")}
              </>
            ) : (
              t("usage.modelsDevImportButton", "导入")
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
