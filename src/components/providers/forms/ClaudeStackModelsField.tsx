import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { FormLabel } from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { useCommittableRef } from "@/hooks/useLatestRef";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type {
  KnownModelMetadata,
  PresetModelSource,
} from "@/lib/modelMetadata";
import type { ClaudeStackModel } from "@/types";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { ModelDropdown } from "./shared";
import {
  hasClaudeOneMMarker,
  stripClaudeOneMMarker,
} from "./hooks/useModelState";

/** 行 id 和窗口输入草稿只用于表单；无效输入保留原文，不能改写已解析的窗口。 */
export type ClaudeStackModelRow = ClaudeStackModel & {
  rowId: string;
  contextWindowInput?: string;
};

let nextRowId = 0;

export function createClaudeStackModelRow(
  seed?: Partial<ClaudeStackModel>,
): ClaudeStackModelRow {
  nextRowId += 1;
  return { model: "", ...seed, rowId: `stack-model-${nextRowId}` };
}

/** 自定义窗口不小于它时按 1M 存：Claude Code 按模型只认 `[1m]` 一种窗口。 */
const ONE_M_WINDOW = 1_000_000;

/** 空白表示继承；只接受正整数或正确分组的千分位，null 表示无效输入。 */
function parseContextWindowInput(input: string): number | undefined | null {
  const text = input.trim();
  if (!text) return undefined;
  if (!/^(?:[0-9]+|[0-9]{1,3}(?:,[0-9]{3})+)$/.test(text)) return null;
  const window = Number(text.replace(/,/g, ""));
  return Number.isSafeInteger(window) && window > 0 ? window : null;
}

export function hasInvalidClaudeStackWindow(row: ClaudeStackModelRow): boolean {
  return (
    row.oneM !== true &&
    !hasClaudeOneMMarker(row.model) &&
    row.contextWindowInput !== undefined &&
    parseContextWindowInput(row.contextWindowInput) === null
  );
}

/** 行里有效的自定义窗口：正整数才算。 */
const customWindowOf = (row: ClaudeStackModel): number | undefined => {
  const window = row.contextWindow;
  return typeof window === "number" && Number.isInteger(window) && window > 0
    ? window
    : undefined;
};

/**
 * 存进 `meta.stackModels` 的样子：去掉空行，模型名里写的 `[1M]` 和不小于 1M 的自定义窗口
 * 换成 `oneM`，同名的合并（有一处是 1M 就按 1M，显示名取第一个填了的，自定义窗口取最小的）。
 */
export function normalizeClaudeStackModels(
  rows: ClaudeStackModel[],
): ClaudeStackModel[] {
  const result: ClaudeStackModel[] = [];
  for (const row of rows) {
    const raw = row.model.trim();
    const model = stripClaudeOneMMarker(raw).trim();
    if (!model) continue;
    const customWindow = customWindowOf(row);
    const oneM =
      row.oneM === true ||
      hasClaudeOneMMarker(raw) ||
      (customWindow !== undefined && customWindow >= ONE_M_WINDOW);
    const contextWindow = oneM ? undefined : customWindow;
    const displayName = row.displayName?.trim() || undefined;
    const existing = result.find((entry) => entry.model === model);
    if (existing) {
      if (oneM) {
        existing.oneM = true;
        delete existing.contextWindow;
      } else if (!existing.oneM && contextWindow !== undefined) {
        existing.contextWindow = Math.min(
          existing.contextWindow ?? contextWindow,
          contextWindow,
        );
      }
      if (!existing.displayName && displayName) {
        existing.displayName = displayName;
      }
      continue;
    }
    result.push({
      model,
      ...(displayName ? { displayName } : {}),
      ...(oneM ? { oneM: true } : {}),
      ...(contextWindow !== undefined ? { contextWindow } : {}),
    });
  }
  return result;
}

/**
 * 按查到的参数补这一行的窗口：已经是 1M 或填了自定义窗口的不动；查到的窗口不小于 1M
 * 时标 1M（保存时也会这样换），否则填进自定义窗口。没有可补的返回 null。
 */
export function fillClaudeStackModelWindow(
  row: ClaudeStackModelRow,
  metadata: Pick<KnownModelMetadata, "contextWindow">,
): ClaudeStackModelRow | null {
  const window = metadata.contextWindow;
  if (typeof window !== "number" || !Number.isInteger(window) || window <= 0) {
    return null;
  }
  if (row.oneM === true || hasClaudeOneMMarker(row.model)) return null;
  // 用户已经编辑过窗口（包括无效草稿或主动清空）：晚到的补全不能覆盖其选择。
  if (row.contextWindowInput !== undefined) return null;
  if (customWindowOf(row) !== undefined) return null;
  return window >= ONE_M_WINDOW
    ? { ...row, oneM: true, contextWindow: undefined }
    : { ...row, contextWindow: window };
}

/** Claude 预设不带各模型的窗口，只靠 models.dev（和选择弹窗里显示的一致）。 */
const NO_PRESET_SOURCES: readonly PresetModelSource[] = [];
const noPresetSources = () => NO_PRESET_SOURCES;

/** 模型映射里的字段：`ANTHROPIC_MODEL` 和各档，按后端 `mode::stack::mapped_models` 的顺序。 */
const MAPPED_MODEL_FIELDS: Array<[string, string | null]> = [
  ["ANTHROPIC_MODEL", null],
  ["ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"],
  ["ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"],
  ["ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME"],
  ["ANTHROPIC_DEFAULT_FABLE_MODEL", "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME"],
];

/**
 * 没配列表时后端发布的模型（和 `mode::stack::mapped_models` 一致）：模型映射里的主模型和
 * 各档，按去掉 1M 标记后的名字去重，有一处带标记就按 1M，显示名取对应档位的。
 */
export function claudeStackModelsFromEnv(
  env: Record<string, unknown> | undefined,
): ClaudeStackModel[] {
  const rows: ClaudeStackModel[] = [];
  for (const [modelKey, nameKey] of MAPPED_MODEL_FIELDS) {
    const raw = env?.[modelKey];
    if (typeof raw !== "string") continue;
    const nameRaw = nameKey ? env?.[nameKey] : undefined;
    rows.push({
      model: raw,
      displayName: typeof nameRaw === "string" ? nameRaw : undefined,
    });
  }
  return normalizeClaudeStackModels(rows);
}

interface ClaudeStackModelsFieldProps {
  rows: ClaudeStackModelRow[];
  onRowsChange: (rows: ClaudeStackModelRow[]) => void;
  /** 从上游取到的模型：批量勾选和每行的下拉都用它。 */
  fetchedModels: FetchedModel[];
  onFetchModels: () => void;
  isFetchingModels: boolean;
  /** 供应商地址：选择弹窗按它查各模型的窗口等参数。 */
  baseUrl?: string;
}

/**
 * Stack 模式下这家发布到 Claude Code 模型选择器里的模型。第一个是这家的默认模型：这家被
 * 设为默认时，Claude Code 启动、后台任务和子代理别名都用它。
 */
export function ClaudeStackModelsField({
  rows,
  onRowsChange,
  fetchedModels,
  onFetchModels,
  isFetchingModels,
  baseUrl = "",
}: ClaudeStackModelsFieldProps) {
  const { t } = useTranslation();

  // 补窗口可能在 models.dev 晚到时才发生，要读到当时最新的行，所以经 ref 同步提交。
  const [rowsRef, commitRows] = useCommittableRef(rows, onRowsChange);
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: noPresetSources,
    prefetch: fetchedModels.length > 0,
  });

  // 按模型名补这一行的上下文窗口（只补没设的）。
  const fillRowWindow = useCallback(
    (rowId: string, modelId: string, silent?: boolean) =>
      fillModelMetadata(
        modelId,
        (metadata) => {
          const current = rowsRef.current;
          const row = current.find((entry) => entry.rowId === rowId);
          if (!row || stripClaudeOneMMarker(row.model).trim() !== modelId) {
            return false;
          }
          const filled = fillClaudeStackModelWindow(row, metadata);
          if (!filled) return false;
          commitRows(
            current.map((entry) => (entry.rowId === rowId ? filled : entry)),
          );
          return true;
        },
        { silent },
      ),
    [commitRows, fillModelMetadata, rowsRef],
  );

  // 「获取模型列表」拿到新列表时，已在列表里、窗口没填的行也补上。按模型 ID
  // 比较：Copilot 等的列表每次渲染都是新数组。
  const fetchedKey = fetchedModels.map((model) => model.id).join("\n");
  const lastFetchedKey = useRef(fetchedKey);
  useEffect(() => {
    if (fetchedKey === lastFetchedKey.current) return;
    lastFetchedKey.current = fetchedKey;
    if (!fetchedKey) return;
    for (const row of rowsRef.current) {
      const modelId = stripClaudeOneMMarker(row.model).trim();
      if (modelId) fillRowWindow(row.rowId, modelId);
    }
  }, [fetchedKey, fillRowWindow, rowsRef]);

  const updateRow = (rowId: string, patch: Partial<ClaudeStackModelRow>) =>
    commitRows(
      rowsRef.current.map((row) =>
        row.rowId === rowId ? { ...row, ...patch } : row,
      ),
    );

  const selectFetchedModel = (rowId: string, modelId: string) => {
    updateRow(rowId, { model: modelId });
    fillRowWindow(rowId, modelId);
  };

  const applyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const removed = new Set(remove);
    const kept = rowsRef.current.filter(
      (row) => !removed.has(stripClaudeOneMMarker(row.model).trim()),
    );
    const configured = new Set(
      kept.map((row) => stripClaudeOneMMarker(row.model).trim()),
    );
    const additions = add
      .filter((id) => !configured.has(id))
      .map((id) => createClaudeStackModelRow({ model: id }));
    commitRows([...kept, ...additions]);
    // 弹窗里已经显示过这些窗口，补完不再提示。
    for (const row of additions) fillRowWindow(row.rowId, row.model, true);
  };

  return (
    <div className="space-y-3">
      <div className="space-y-1">
        <div className="flex items-center justify-between gap-3">
          <div className="flex items-center gap-0.5">
            <FormLabel>
              {t("providerForm.stackModelsLabel", { defaultValue: "模型列表" })}
            </FormLabel>
            <HelpTip
              title={t("providerForm.stackModelsLabel", {
                defaultValue: "模型列表",
              })}
            >
              {t("providerForm.stackModelDefaultHelp", {
                defaultValue:
                  "这些模型会出现在 Claude Code 的 /model 里，选中后请求直达这家。第一行是默认模型，也会写成这张卡的 ANTHROPIC_MODEL。",
              })}
            </HelpTip>
          </div>
          <div className="flex gap-2">
            <FetchedModelPicker
              models={fetchedModels}
              configuredModelIds={rows.map((row) =>
                stripClaudeOneMMarker(row.model).trim(),
              )}
              onApply={applyFetchedModels}
              onFetch={onFetchModels}
              isFetching={isFetchingModels}
              metadataSource={{ baseUrl }}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                onRowsChange([...rows, createClaudeStackModelRow()])
              }
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("providerForm.addStackModel", { defaultValue: "手动添加" })}
            </Button>
          </div>
        </div>
      </div>

      {rows.length === 0 ? (
        <p className="text-xs leading-relaxed text-fg-2">
          {t("providerForm.stackModelsEmpty", {
            defaultValue:
              "未配置模型：把这家加入聚合后，模型选择器里不会多出它的模型",
          })}
        </p>
      ) : (
        <div className="space-y-2">
          <div className="hidden grid-cols-[1fr_minmax(0,1fr)_200px_36px] gap-2 px-1 text-xs font-medium text-fg-2 md:grid">
            <span>
              {t("providerForm.modelDisplayNameLabel", {
                defaultValue: "显示名称",
              })}
            </span>
            <span>
              {t("providerForm.requestModelLabel", {
                defaultValue: "实际请求模型",
              })}
            </span>
            <span className="flex items-center gap-0.5">
              {t("providerForm.modelContextLabel", {
                defaultValue: "上下文窗口",
              })}
              <HelpTip
                title={t("providerForm.modelContextLabel", {
                  defaultValue: "上下文窗口",
                })}
              >
                {t("providerForm.modelContextHelp", {
                  defaultValue:
                    "Claude Code 可给单个模型标 1M，其余模型共用一个 CLAUDE_CODE_MAX_CONTEXT_TOKENS。聚合时它取所有非 1M 模型里最大的窗口；窗口更小的模型由 CC Switch 代理按请求补齐用量，让 Claude Code 在这个模型自己的窗口处自动压缩，各会话、子代理和 /model 切换互不影响（Claude Code 显示的上下文用量会偏大）。自定义填实际 token 数（例如 262144）；留空时用这家的 CLAUDE_CODE_MAX_CONTEXT_TOKENS，没有是 200K；填不小于 1000000 时按 1M 保存。获取模型列表时会按 models.dev 补上没填的窗口。",
                })}
              </HelpTip>
            </span>
            <span />
          </div>
          {rows.map((row) => {
            const modelBase = stripClaudeOneMMarker(row.model).trim();
            const isOneM = row.oneM === true || hasClaudeOneMMarker(row.model);
            const invalidWindow = hasInvalidClaudeStackWindow(row);
            const windowErrorId = `${row.rowId}-window-error`;
            return (
              <div
                key={row.rowId}
                className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_minmax(0,1fr)_200px_36px]"
              >
                <ImeSafeInput
                  value={row.displayName ?? ""}
                  onValueChange={(value) =>
                    updateRow(row.rowId, { displayName: value })
                  }
                  placeholder={
                    modelBase ||
                    t("providerForm.modelDisplayNamePlaceholder", {
                      defaultValue: "例如 DeepSeek V4 Pro",
                    })
                  }
                  aria-label={t("providerForm.modelDisplayNameLabel", {
                    defaultValue: "显示名称",
                  })}
                  autoComplete="off"
                />
                <div className="flex gap-1">
                  <Input
                    value={row.model}
                    onChange={(event) =>
                      updateRow(row.rowId, { model: event.target.value })
                    }
                    placeholder={t("providerForm.stackModelPlaceholder", {
                      defaultValue: "例如 deepseek-v4-pro",
                    })}
                    aria-label={t("providerForm.requestModelLabel", {
                      defaultValue: "实际请求模型",
                    })}
                    autoComplete="off"
                    className="flex-1"
                  />
                  {fetchedModels.length > 0 && (
                    <ModelDropdown
                      models={fetchedModels}
                      onSelect={(id) => selectFetchedModel(row.rowId, id)}
                    />
                  )}
                </div>
                <div className="space-y-1">
                  <div className="flex gap-1">
                    <Select
                      value={isOneM ? "1m" : "custom"}
                      onValueChange={(value) =>
                        updateRow(row.rowId, {
                          model: stripClaudeOneMMarker(row.model),
                          oneM: value === "1m",
                          contextWindow: undefined,
                          contextWindowInput: undefined,
                        })
                      }
                    >
                      <SelectTrigger
                        className="w-[84px] shrink-0"
                        aria-label={t("providerForm.modelContextLabel", {
                          defaultValue: "上下文窗口",
                        })}
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="1m">
                          {t("providerForm.modelOneMLabel", {
                            defaultValue: "1M",
                          })}
                        </SelectItem>
                        <SelectItem value="custom">
                          {t("providerForm.modelContextCustom", {
                            defaultValue: "自定义",
                          })}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                    {!isOneM && (
                      <Input
                        value={
                          row.contextWindowInput ??
                          row.contextWindow?.toString() ??
                          ""
                        }
                        onChange={(event) => {
                          const input = event.target.value;
                          const window = parseContextWindowInput(input);
                          updateRow(row.rowId, {
                            contextWindowInput: input,
                            ...(window !== null
                              ? { contextWindow: window }
                              : {}),
                          });
                        }}
                        aria-invalid={invalidWindow}
                        aria-describedby={
                          invalidWindow ? windowErrorId : undefined
                        }
                        inputMode="numeric"
                        placeholder={t(
                          "providerForm.modelContextWindowPlaceholder",
                          { defaultValue: "默认" },
                        )}
                        aria-label={t("providerForm.modelContextWindowLabel", {
                          defaultValue: "窗口 token 数",
                        })}
                        autoComplete="off"
                        className="min-w-0 flex-1"
                      />
                    )}
                  </div>
                  {invalidWindow && (
                    <p
                      id={windowErrorId}
                      role="alert"
                      className="text-xs text-danger"
                    >
                      {t("providerForm.modelContextWindowInvalid", {
                        defaultValue:
                          "请输入正整数 token 数（如 262144 或 262,144），不要使用单位、小数或科学计数法；留空则继承默认值。",
                      })}
                    </p>
                  )}
                </div>
                <HoverTip content={t("common.delete")}>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() =>
                      onRowsChange(
                        rows.filter((other) => other.rowId !== row.rowId),
                      )
                    }
                    aria-label={t("common.delete")}
                    className="h-9 w-9 text-fg-2 hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </HoverTip>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
