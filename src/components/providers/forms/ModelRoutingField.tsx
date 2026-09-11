import {
  ArrowDown,
  ArrowRight,
  ArrowUp,
  FlaskConical,
  Plus,
  Shuffle,
  Trash2,
} from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";

export type ModelRouteMatchMode =
  | "exact"
  | "prefix"
  | "suffix"
  | "contains"
  | "regex";

export interface ModelRouteRow {
  id: string;
  enabled: boolean;
  matchMode: ModelRouteMatchMode;
  source: string;
  target: string;
}

const MATCH_MODES: Array<{
  value: ModelRouteMatchMode;
  labelKey: string;
}> = [
  { value: "exact", labelKey: "providerForm.modelRouteModeExact" },
  { value: "prefix", labelKey: "providerForm.modelRouteModePrefix" },
  { value: "suffix", labelKey: "providerForm.modelRouteModeSuffix" },
  { value: "contains", labelKey: "providerForm.modelRouteModeContains" },
  { value: "regex", labelKey: "providerForm.modelRouteModeRegex" },
];

function createRoute(): ModelRouteRow {
  return {
    id: crypto.randomUUID(),
    enabled: true,
    matchMode: "exact",
    source: "",
    target: "",
  };
}

function routeMatches(row: ModelRouteRow, model: string): boolean {
  if (!row.enabled || !row.source.trim()) return false;
  const source = row.source.trim();
  switch (row.matchMode) {
    case "prefix":
      return model.startsWith(source);
    case "suffix":
      return model.endsWith(source);
    case "contains":
      return model.includes(source);
    case "regex":
      try {
        return new RegExp(source).test(model);
      } catch {
        return false;
      }
    default:
      return model === source;
  }
}

export function ModelRoutingField({
  rows,
  onChange,
}: {
  rows: ModelRouteRow[];
  onChange: (rows: ModelRouteRow[]) => void;
}) {
  const { t } = useTranslation();
  const [testModel, setTestModel] = useState("");
  const [testRequested, setTestRequested] = useState(false);
  const testResult = useMemo(() => {
    const model = testModel.trim();
    if (!testRequested || !model) return null;

    const matchIndex = rows.findIndex((candidate) =>
      routeMatches(candidate, model),
    );
    if (matchIndex === -1) {
      return {
        kind: "miss" as const,
        status: t("providerForm.modelRouteTestNoMatchStatus"),
        message: t("providerForm.modelRouteTestNoMatch", { model }),
      };
    }

    const row = rows[matchIndex];
    const modeLabelKey =
      MATCH_MODES.find((mode) => mode.value === row.matchMode)?.labelKey ??
      "providerForm.modelRouteModeExact";
    const status = t("providerForm.modelRouteTestMatchedStatus", {
      index: matchIndex + 1,
    });
    return {
      kind: "hit" as const,
      status,
      summary: t("providerForm.modelRouteTestMatched", {
        status,
        mode: t(modeLabelKey),
        source: row.source.trim(),
      }),
      target: row.target.trim() || t("providerForm.modelRouteTestEmptyTarget"),
    };
  }, [rows, t, testModel, testRequested]);

  const updateRows = (next: ModelRouteRow[]) => {
    setTestRequested(false);
    onChange(next);
  };

  const updateRow = (index: number, update: Partial<ModelRouteRow>) => {
    const next = [...rows];
    next[index] = { ...next[index], ...update };
    updateRows(next);
  };

  const moveRow = (index: number, offset: -1 | 1) => {
    const targetIndex = index + offset;
    if (targetIndex < 0 || targetIndex >= rows.length) return;
    const next = [...rows];
    [next[index], next[targetIndex]] = [next[targetIndex], next[index]];
    updateRows(next);
  };

  return (
    <section className="space-y-3 border-t border-border-default pt-4">
      <div className="flex items-start gap-2">
        <Shuffle className="mt-0.5 h-4 w-4 shrink-0 text-blue-500" />
        <div>
          <h3 className="text-sm font-semibold text-foreground">
            {t("providerForm.providerModelRouting", {
              defaultValue: "模型路由",
            })}
          </h3>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {t("providerForm.providerModelRoutingHint", {
              defaultValue: "按规则将客户端请求路由到目标上游模型",
            })}
          </p>
        </div>
      </div>

      <div className="hidden grid-cols-[40px_92px_minmax(150px,1fr)_20px_minmax(150px,1fr)_76px] items-center gap-2 px-1 text-[11px] text-muted-foreground md:grid">
        <span>
          {t("providerForm.modelRouteEnabled", { defaultValue: "启用" })}
        </span>
        <span>
          {t("providerForm.modelRouteMode", { defaultValue: "匹配方式" })}
        </span>
        <span>
          {t("providerForm.modelRouteSource", { defaultValue: "源模型" })}
        </span>
        <span />
        <span>
          {t("providerForm.modelRouteTarget", { defaultValue: "目标模型" })}
        </span>
        <span className="text-right">
          {t("providerForm.modelRouteActions", { defaultValue: "操作" })}
        </span>
      </div>

      <div className="space-y-2">
        {rows.map((row, index) => (
          <div
            key={row.id}
            className="grid grid-cols-1 items-center gap-2 rounded-lg border border-border-default bg-background p-2 md:grid-cols-[40px_92px_minmax(150px,1fr)_20px_minmax(150px,1fr)_76px] md:p-1.5"
          >
            <Switch
              checked={row.enabled}
              onCheckedChange={(enabled) => updateRow(index, { enabled })}
              aria-label={t("providerForm.modelRouteEnabled", {
                defaultValue: "启用",
              })}
              className="ml-1 scale-75 origin-left md:justify-self-start"
            />
            <Select
              value={row.matchMode}
              onValueChange={(matchMode: ModelRouteMatchMode) =>
                updateRow(index, { matchMode })
              }
            >
              <SelectTrigger className="h-8 px-2 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {MATCH_MODES.map((mode) => (
                  <SelectItem key={mode.value} value={mode.value}>
                    {t(mode.labelKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Input
              value={row.source}
              onChange={(event) =>
                updateRow(index, { source: event.target.value })
              }
              placeholder={t("providerForm.modelRouteSourcePlaceholder", {
                defaultValue: "例: gpt-5.6-luna",
              })}
              className="h-8 text-xs"
              aria-label={t("providerForm.modelRouteSource", {
                defaultValue: "源模型",
              })}
            />
            <ArrowRight className="hidden h-4 w-4 text-muted-foreground md:block" />
            <Input
              value={row.target}
              onChange={(event) =>
                updateRow(index, { target: event.target.value })
              }
              placeholder={t("providerForm.modelRouteTargetPlaceholder", {
                defaultValue: "例: codex-auto-review",
              })}
              className="h-8 text-xs"
              aria-label={t("providerForm.modelRouteTarget", {
                defaultValue: "目标模型",
              })}
            />
            <div className="flex justify-end gap-0.5 md:justify-end">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                disabled={index === 0}
                onClick={() => moveRow(index, -1)}
                title={t("providerForm.modelRouteMoveUp")}
              >
                <ArrowUp className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                disabled={index === rows.length - 1}
                onClick={() => moveRow(index, 1)}
                title={t("providerForm.modelRouteMoveDown")}
              >
                <ArrowDown className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-red-500 hover:text-red-600"
                onClick={() =>
                  updateRows(rows.filter((_, rowIndex) => rowIndex !== index))
                }
                title={t("providerForm.modelRouteDelete")}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-8 gap-1.5"
          onClick={() => updateRows([...rows, createRoute()])}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("providerForm.addModelRoute", { defaultValue: "添加路由" })}
        </Button>
        <div className="flex min-w-[280px] flex-1 items-center gap-2 rounded-lg border border-border-default p-1">
          <FlaskConical className="ml-1 h-4 w-4 shrink-0 text-muted-foreground" />
          <Input
            value={testModel}
            onChange={(event) => {
              setTestModel(event.target.value);
              setTestRequested(false);
            }}
            placeholder={t("providerForm.modelRouteTestPlaceholder", {
              defaultValue: "输入模型进行路由测试",
            })}
            className="min-w-0 flex-1 h-7 border-0 px-1 text-xs shadow-none focus-visible:ring-0"
          />
          {testResult?.kind === "hit" && (
            <span className="ml-auto flex min-w-0 max-w-[360px] shrink items-center gap-1 text-xs">
              <span className="min-w-0 truncate text-emerald-600 dark:text-emerald-400">
                {testResult.summary}
              </span>
              <ArrowRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <span className="min-w-0 truncate text-foreground">
                {testResult.target}
              </span>
            </span>
          )}
          {testResult?.kind === "miss" && (
            <span className="ml-auto min-w-0 max-w-[360px] shrink truncate text-xs text-emerald-600 dark:text-emerald-400">
              {testResult.message}
            </span>
          )}
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="h-7 shrink-0 px-2 text-xs"
            onClick={() => setTestRequested(true)}
          >
            {t("providerForm.modelRouteTest", { defaultValue: "测试" })}
          </Button>
        </div>
      </div>
    </section>
  );
}
