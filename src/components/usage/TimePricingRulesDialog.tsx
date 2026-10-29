import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import {
  ArrowDown,
  ArrowUp,
  Loader2,
  Pencil,
  Plus,
  Save,
  Trash2,
} from "lucide-react";
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
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  useModelPricing,
  useModelPricingTimeRules,
  useSaveModelPricingTimeRules,
} from "@/lib/query/usage";
import type { ModelPricingTimeRule } from "@/types/usage";
import { parseFiniteNumber } from "./format";
import { formatPrice } from "@/lib/modelsDevPricing";
import {
  ALL_WEEKDAYS_MASK,
  MAX_UTC_OFFSET_MINUTES,
  MIN_UTC_OFFSET_MINUTES,
  UTC_OFFSET_PRESETS,
  WEEKDAY_MON_FRI_MASK,
  WEEKEND_MASK,
  describeTimeRule,
  draftToTimeRule,
  emptyTimeRuleDraft,
  formatUtcOffset,
  newTimeRuleId,
  timeRuleToDraft,
  validateTimeRuleDraft,
  type TimeRuleDraft,
  type TimeRuleErrorCode,
  type TimeRuleField,
  type TimeRuleFieldError,
} from "@/lib/timePricing";

/** 星期复选框的顺序：bit0 = 周一 … bit6 = 周日 */
const WEEKDAY_LABELS = [
  { bit: 1, key: "monday", name: "Mon" },
  { bit: 2, key: "tuesday", name: "Tue" },
  { bit: 4, key: "wednesday", name: "Wed" },
  { bit: 8, key: "thursday", name: "Thu" },
  { bit: 16, key: "friday", name: "Fri" },
  { bit: 32, key: "saturday", name: "Sat" },
  { bit: 64, key: "sunday", name: "Sun" },
] as const;

const PRICE_FIELDS = [
  {
    id: "inputCostPerMillion",
    labelKey: "inputCost",
    defaultLabel: "Input cost (per 1M tokens, USD)",
  },
  {
    id: "outputCostPerMillion",
    labelKey: "outputCost",
    defaultLabel: "Output cost (per 1M tokens, USD)",
  },
  {
    id: "cacheReadCostPerMillion",
    labelKey: "cacheReadCost",
    defaultLabel: "Cache hit cost (per 1M tokens, USD)",
  },
  {
    id: "cacheCreationCostPerMillion",
    labelKey: "cacheCreationCost",
    defaultLabel: "Cache write cost (per 1M tokens, USD)",
  },
] as const satisfies readonly {
  id: TimeRuleField;
  labelKey: string;
  defaultLabel: string;
}[];
const PRICE_STEP = "0.0001";

/** 校验错误码 → 英文兜底文案；正式文案在 4 个语言包里 */
const ERROR_DEFAULTS: Record<TimeRuleErrorCode, string> = {
  modelIdRequired: "Pick a model ID",
  utcOffsetNotInteger: "UTC offset must be a whole number of minutes",
  utcOffsetOutOfRange: `UTC offset must be between ${MIN_UTC_OFFSET_MINUTES} and ${MAX_UTC_OFFSET_MINUTES}`,
  weekdayMaskEmpty: "Pick at least one day",
  startMinuteOutOfRange: "Start must be HH:MM between 00:00 and 23:59",
  endMinuteOutOfRange: "End must be HH:MM between 00:00 and 24:00",
  excludedDateFormat: "Dates must look like YYYY-MM-DD",
  excludedDateNotReal: "That date does not exist",
  priceRequired: "Fill in all four prices — a blank one is billed as nothing",
  priceInvalid: "Prices must be non-negative decimals",
};

/** `$0.15` / `$0.003`：认不出的值原样带 $ 显示，不猜 */
function formatRulePrice(value: string): string {
  const parsed = parseFiniteNumber(value);
  if (parsed == null) return `$${value}`;
  return `$${formatPrice(parsed)}`;
}

interface TimePricingRulesDialogProps {
  open: boolean;
  onClose: () => void;
}

export function TimePricingRulesDialog({
  open,
  onClose,
}: TimePricingRulesDialogProps) {
  const { t } = useTranslation();
  const rulesQuery = useModelPricingTimeRules();
  const pricingQuery = useModelPricing();
  const saveMutation = useSaveModelPricingTimeRules();

  const serverRules = useMemo(() => rulesQuery.data ?? [], [rulesQuery.data]);

  // 本地副本：null = 还没动过，直接用服务端顺序（那个顺序就是优先级）。
  // 保存成功后用后端返回的权威列表覆盖，之后不再被后台重取冲掉。
  const [rules, setRules] = useState<ModelPricingTimeRule[] | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<TimeRuleDraft>(emptyTimeRuleDraft);
  const [errors, setErrors] = useState<TimeRuleFieldError[]>([]);
  const [deleteTarget, setDeleteTarget] = useState<ModelPricingTimeRule | null>(
    null,
  );

  const visibleRules = rules ?? serverRules;
  const isSaving = saveMutation.isPending;
  const editingRule = editingId
    ? (visibleRules.find((rule) => rule.id === editingId) ?? null)
    : null;
  const isEditing = editingId !== null;

  useEffect(() => {
    if (open) {
      setRules(null);
      setEditingId(null);
      setErrors([]);
      setDeleteTarget(null);
    }
  }, [open]);

  const knownModelIds = useMemo(() => {
    const ids = (pricingQuery.data ?? []).map((model) => model.modelId);
    return Array.from(new Set(ids)).sort((a, b) => a.localeCompare(b));
  }, [pricingQuery.data]);

  /** 同一 modelId 的相邻规则合成一组，靠组标题分隔；只有一个组时不出标题 */
  const groups = useMemo(() => {
    const result: { modelId: string; indexes: number[] }[] = [];
    visibleRules.forEach((rule, index) => {
      const last = result[result.length - 1];
      if (last && last.modelId === rule.modelId) last.indexes.push(index);
      else result.push({ modelId: rule.modelId, indexes: [index] });
    });
    return result;
  }, [visibleRules]);

  const startEditing = (rule: ModelPricingTimeRule | null) => {
    setDraft(rule ? timeRuleToDraft(rule) : emptyTimeRuleDraft());
    setEditingId(rule ? rule.id : newTimeRuleId());
    setErrors([]);
  };

  const cancelEditing = () => {
    setEditingId(null);
    setErrors([]);
  };

  // 换序立刻存：优先级是用户盯着的关键信息，不能等关掉弹窗才丢。
  const move = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= visibleRules.length || isSaving) return;
    const next = [...visibleRules];
    [next[index], next[target]] = [next[target], next[index]];
    void saveAll(next);
  };

  /** 整份发一次；成功返回 true。失败时留在原处，用户改完还能再试。 */
  const saveAll = async (next: ModelPricingTimeRule[]) => {
    try {
      // 后端按数组顺序重排并补 id，返回的才是权威列表
      const canonical = await saveMutation.mutateAsync(next);
      setRules(canonical ?? next);
      toast.success(
        t("usage.pricing.timePricing.saved", {
          defaultValue: "Time-based pricing saved",
        }),
        { closeButton: true },
      );
      return true;
    } catch (error) {
      toast.error(
        t("usage.pricing.timePricing.saveFailed", {
          defaultValue: "Failed to save time-based pricing: {{error}}",
          error: String(error),
        }),
      );
      return false;
    }
  };

  const commitRule = async () => {
    const found = validateTimeRuleDraft(draft);
    setErrors(found);
    if (found.length > 0) return;

    const rule = draftToTimeRule(draft, editingId ?? newTimeRuleId());
    if (!rule) return;

    const exists = visibleRules.some((item) => item.id === rule.id);
    const next = exists
      ? visibleRules.map((item) => (item.id === rule.id ? rule : item))
      : // 新规则排在最后（优先级最低）：后补的兜底全天价也不会盖住前面的高峰段
        [...visibleRules, rule];

    if (await saveAll(next)) setEditingId(null);
  };

  const removeRule = async () => {
    if (!deleteTarget) return;
    const next = visibleRules.filter((rule) => rule.id !== deleteTarget.id);
    setDeleteTarget(null);
    await saveAll(next);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !isSaving) onClose();
      }}
    >
      <DialogContent zIndex="top" className="max-w-3xl gap-0 p-0">
        <DialogHeader>
          <DialogTitle>
            {t("usage.pricing.timePricing.title", {
              defaultValue: "Time-based pricing",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("usage.pricing.timePricing.description", {
              defaultValue:
                "Charge a different unit price for the same model inside a weekday + time-of-day window, e.g. an off-peak discount.",
            })}
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto px-6 py-4">
          {!isEditing && (
            <>
              <Notice
                tone="neutral"
                title={t("usage.pricing.timePricing.fallbackTitle", {
                  defaultValue: "When no rule matches",
                })}
              >
                {t("usage.pricing.timePricing.fallback", {
                  defaultValue:
                    "The model's ordinary single-tier price is used, unchanged. A rule only overrides the price inside its own window.",
                })}
              </Notice>
              <p className="m-0 text-caption text-fg-2">
                {t("usage.pricing.timePricing.precedence", {
                  defaultValue:
                    "Rules are checked top to bottom and the first match wins, so the order below is the priority. New rules land at the bottom — move them up when a broader rule above would swallow them.",
                })}
              </p>
            </>
          )}

          {rulesQuery.isLoading ? (
            <div className="flex flex-1 items-center justify-center py-10">
              <Loader2 className="h-6 w-6 animate-spin text-fg-3" />
            </div>
          ) : rulesQuery.error ? (
            <Notice
              tone="danger"
              title={t("usage.pricing.timePricing.loadError", {
                defaultValue: "Failed to load time-based pricing rules",
              })}
              actions={
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={() => void rulesQuery.refetch()}
                >
                  {t("common.retry", { defaultValue: "Retry" })}
                </Button>
              }
            />
          ) : isEditing ? (
            <RuleEditor
              draft={draft}
              errors={errors}
              position={
                editingRule
                  ? visibleRules.indexOf(editingRule) + 1
                  : visibleRules.length + 1
              }
              total={visibleRules.length + (editingRule ? 0 : 1)}
              knownModelIds={knownModelIds}
              onChange={setDraft}
              onSubmit={() => void commitRule()}
            />
          ) : (
            <div className="flex flex-col gap-4">
              {visibleRules.length === 0 ? (
                <div className="py-10 text-center text-body text-fg-3">
                  {t("usage.pricing.timePricing.empty", {
                    defaultValue:
                      "No time-based rules yet. Every model keeps its single-tier price until you add one.",
                  })}
                </div>
              ) : (
                groups.map((group) => (
                  <div key={group.indexes[0]} className="flex flex-col gap-2">
                    {groups.length > 1 && (
                      <div className="font-mono text-caption text-fg-2">
                        {group.modelId}
                      </div>
                    )}
                    {group.indexes.map((index) => {
                      const rule = visibleRules[index];
                      return (
                        <div
                          key={rule.id}
                          className="flex items-start gap-3 rounded-panel border border-border px-3 py-2.5"
                        >
                          <span className="mt-0.5 w-4 shrink-0 text-center text-caption tabular-nums text-fg-3">
                            {index + 1}
                          </span>
                          <div className="min-w-0 flex-1">
                            <div className="text-body text-fg-1">
                              {describeTimeRule(rule)}
                            </div>
                            <div className="mt-0.5 text-caption text-fg-2">
                              {t("usage.pricing.timePricing.priceLine", {
                                input: formatRulePrice(
                                  rule.inputCostPerMillion,
                                ),
                                output: formatRulePrice(
                                  rule.outputCostPerMillion,
                                ),
                                cacheRead: formatRulePrice(
                                  rule.cacheReadCostPerMillion,
                                ),
                                cacheWrite: formatRulePrice(
                                  rule.cacheCreationCostPerMillion,
                                ),
                                defaultValue:
                                  "in {{input}} · out {{output}} · cache hit {{cacheRead}} · cache write {{cacheWrite}}",
                              })}
                            </div>
                          </div>
                          <div className="flex shrink-0 items-center gap-0.5">
                            <Button
                              type="button"
                              variant="quiet"
                              size="icon-compact"
                              aria-label={t(
                                "usage.pricing.timePricing.moveUp",
                                {
                                  defaultValue: "Move up",
                                },
                              )}
                              disabled={index === 0 || isSaving}
                              onClick={() => move(index, -1)}
                            >
                              <ArrowUp
                                className="h-3.5 w-3.5"
                                strokeWidth={1.75}
                              />
                            </Button>
                            <Button
                              type="button"
                              variant="quiet"
                              size="icon-compact"
                              aria-label={t(
                                "usage.pricing.timePricing.moveDown",
                                {
                                  defaultValue: "Move down",
                                },
                              )}
                              disabled={
                                index === visibleRules.length - 1 || isSaving
                              }
                              onClick={() => move(index, 1)}
                            >
                              <ArrowDown
                                className="h-3.5 w-3.5"
                                strokeWidth={1.75}
                              />
                            </Button>
                            <Button
                              type="button"
                              variant="quiet"
                              size="icon-compact"
                              aria-label={t(
                                "usage.pricing.timePricing.editAria",
                                {
                                  index: index + 1,
                                  defaultValue: "Edit rule {{index}}",
                                },
                              )}
                              disabled={isSaving}
                              onClick={() => startEditing(rule)}
                            >
                              <Pencil
                                className="h-3.5 w-3.5"
                                strokeWidth={1.75}
                              />
                            </Button>
                            <Button
                              type="button"
                              variant="quiet"
                              size="icon-compact"
                              aria-label={t(
                                "usage.pricing.timePricing.deleteAria",
                                {
                                  index: index + 1,
                                  defaultValue: "Delete rule {{index}}",
                                },
                              )}
                              className="hover:text-danger-text"
                              disabled={isSaving}
                              onClick={() => setDeleteTarget(rule)}
                            >
                              <Trash2
                                className="h-3.5 w-3.5"
                                strokeWidth={1.75}
                              />
                            </Button>
                          </div>
                        </div>
                      );
                    })}
                  </div>
                ))
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          {isEditing ? (
            <>
              <Button
                type="button"
                variant="neutral"
                size="regular"
                onClick={cancelEditing}
                disabled={isSaving}
              >
                {t("common.cancel", { defaultValue: "Cancel" })}
              </Button>
              <Button
                type="button"
                variant="solid"
                size="regular"
                disabled={isSaving}
                onClick={() => void commitRule()}
              >
                {isSaving ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Save className="h-4 w-4" />
                )}
                {isSaving
                  ? t("common.saving", { defaultValue: "Saving…" })
                  : t("usage.pricing.timePricing.saveRule", {
                      defaultValue: "Save rule",
                    })}
              </Button>
            </>
          ) : (
            <>
              <Button
                type="button"
                variant="neutral"
                size="regular"
                onClick={onClose}
                disabled={isSaving}
              >
                {t("common.close", { defaultValue: "Close" })}
              </Button>
              <Button
                type="button"
                variant="solid"
                size="regular"
                onClick={() => startEditing(null)}
                disabled={isSaving}
              >
                <Plus className="h-4 w-4" />
                {t("usage.pricing.timePricing.add", {
                  defaultValue: "Add rule",
                })}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>

      <ConfirmDialog
        isOpen={!!deleteTarget}
        zIndex="top"
        title={t("usage.pricing.timePricing.deleteConfirmTitle", {
          defaultValue: "Delete this rule?",
        })}
        message={t("usage.pricing.timePricing.deleteConfirmDesc", {
          defaultValue:
            "Outside the remaining rules the model falls back to its ordinary price.",
        })}
        confirmText={
          isSaving
            ? t("common.deleting", { defaultValue: "Deleting…" })
            : t("common.delete", { defaultValue: "Delete" })
        }
        variant="destructive"
        pending={isSaving}
        onConfirm={() => void removeRule()}
        onCancel={() => setDeleteTarget(null)}
      />
    </Dialog>
  );
}

interface RuleEditorProps {
  draft: TimeRuleDraft;
  errors: TimeRuleFieldError[];
  position: number;
  total: number;
  knownModelIds: string[];
  onChange: (draft: TimeRuleDraft) => void;
  onSubmit: () => void;
}

function RuleEditor({
  draft,
  errors,
  position,
  total,
  knownModelIds,
  onChange,
  onSubmit,
}: RuleEditorProps) {
  const { t } = useTranslation();

  const errorFor = (field: TimeRuleField) =>
    errors.find((error) => error.field === field)?.code;

  /** 四个单价共用一句文案，取第一个出错的来显示 */
  const firstPriceError = PRICE_FIELDS.find(
    (price) => errorFor(price.id) !== undefined,
  );

  const patch = (next: Partial<TimeRuleDraft>) => {
    onChange({ ...draft, ...next });
  };

  const toggleWeekday = (bit: number, checked: boolean) => {
    patch({
      weekdayMask: checked ? draft.weekdayMask | bit : draft.weekdayMask & ~bit,
    });
  };

  const parsedOffset = Number.parseInt(draft.utcOffsetText, 10);
  const offsetText = Number.isInteger(parsedOffset)
    ? formatUtcOffset(parsedOffset)
    : "--";

  const previewRule = draftToTimeRule(draft, "preview");

  return (
    <form
      id="time-rule-form"
      className="flex flex-col gap-5"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      <p className="m-0 text-caption text-fg-3">
        {t("usage.pricing.timePricing.position", {
          position,
          total,
          defaultValue: "Position {{position}} of {{total}} — first match wins",
        })}
      </p>

      <Field
        id="time-rule-model"
        label={t("usage.pricing.timePricing.modelId", {
          defaultValue: "Model ID",
        })}
        hint={t("usage.pricing.timePricing.modelIdHint", {
          defaultValue:
            "Pick one of your configured models, or type an ID — a typo silently never matches.",
        })}
        helpTitle={t("usage.pricing.timePricing.modelIdHelpTitle", {
          defaultValue: "Which model this rule applies to",
        })}
        errorCode={errorFor("modelId")}
      >
        <Input
          id="time-rule-model"
          list="time-rule-model-options"
          value={draft.modelId}
          onChange={(event) => patch({ modelId: event.target.value })}
          placeholder={t("usage.pricing.timePricing.modelIdPlaceholder", {
            defaultValue: "e.g. deepseek-chat",
          })}
          aria-invalid={errorFor("modelId") ? true : undefined}
          required
        />
        <datalist id="time-rule-model-options">
          {knownModelIds.map((modelId) => (
            <option key={modelId} value={modelId} />
          ))}
        </datalist>
      </Field>

      <Field
        id="time-rule-label"
        label={t("usage.pricing.timePricing.label", {
          defaultValue: "Label (optional)",
        })}
      >
        <Input
          id="time-rule-label"
          value={draft.label}
          onChange={(event) => patch({ label: event.target.value })}
          placeholder={t("usage.pricing.timePricing.labelPlaceholder", {
            defaultValue: "e.g. Peak hours, Off-peak",
          })}
        />
      </Field>

      <Field
        id="time-rule-offset"
        label={t("usage.pricing.timePricing.utcOffset", {
          defaultValue: "UTC offset (minutes)",
        })}
        hint={t("usage.pricing.timePricing.utcOffsetHint", {
          defaultValue:
            "A fixed offset with no daylight saving. If your zone observes DST you have to adjust it twice a year.",
        })}
        helpTitle={t("usage.pricing.timePricing.utcOffsetHelpTitle", {
          defaultValue: "Why a fixed offset",
        })}
        errorCode={errorFor("utcOffsetMinutes")}
        aside={
          <span className="font-mono text-body text-fg-1">{offsetText}</span>
        }
      >
        <Input
          id="time-rule-offset"
          type="number"
          step={1}
          min={MIN_UTC_OFFSET_MINUTES}
          max={MAX_UTC_OFFSET_MINUTES}
          value={draft.utcOffsetText}
          onChange={(event) => patch({ utcOffsetText: event.target.value })}
          aria-invalid={errorFor("utcOffsetMinutes") ? true : undefined}
          required
        />
        <div className="flex flex-wrap items-center gap-1.5 pt-1">
          <span className="text-caption text-fg-3">
            {t("usage.pricing.timePricing.presets", {
              defaultValue: "Presets",
            })}
          </span>
          {UTC_OFFSET_PRESETS.map((preset) => (
            <Button
              key={preset.offset}
              type="button"
              variant="neutral"
              size="compact"
              onClick={() => patch({ utcOffsetText: String(preset.offset) })}
            >
              {preset.observesDst
                ? t("usage.pricing.timePricing.presetWinter", {
                    zone: preset.zone,
                    offset: formatUtcOffset(preset.offset),
                    defaultValue: "{{zone}} ({{offset}}, standard time)",
                  })
                : `${formatUtcOffset(preset.offset)} · ${preset.zone}`}
            </Button>
          ))}
        </div>
      </Field>

      <Field
        id="time-rule-weekdays"
        label={t("usage.pricing.timePricing.weekdays", {
          defaultValue: "Days",
        })}
        errorCode={errorFor("weekdayMask")}
      >
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          {WEEKDAY_LABELS.map((weekday) => (
            <label
              key={weekday.bit}
              className="flex items-center gap-1.5 text-body text-fg-1"
            >
              <Checkbox
                checked={!!(draft.weekdayMask & weekday.bit)}
                onCheckedChange={(checked) =>
                  toggleWeekday(weekday.bit, checked)
                }
              />
              {t(`usage.pricing.timePricing.weekday.${weekday.key}`, {
                defaultValue: weekday.name,
              })}
            </label>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-1.5 pt-1">
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => patch({ weekdayMask: ALL_WEEKDAYS_MASK })}
          >
            {t("usage.pricing.timePricing.everyDay", {
              defaultValue: "Every day",
            })}
          </Button>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => patch({ weekdayMask: WEEKDAY_MON_FRI_MASK })}
          >
            {t("usage.pricing.timePricing.weekdaysShortcut", {
              defaultValue: "Weekdays",
            })}
          </Button>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => patch({ weekdayMask: WEEKEND_MASK })}
          >
            {t("usage.pricing.timePricing.weekendShortcut", {
              defaultValue: "Weekend",
            })}
          </Button>
        </div>
      </Field>

      <Field
        id="time-rule-start"
        label={t("usage.pricing.timePricing.window", {
          defaultValue: "Time window (local time)",
        })}
        hint={t("usage.pricing.timePricing.windowHint", {
          defaultValue:
            "Half-open [start, end). Start equal to end means all 24 hours. Start after end wraps past midnight, e.g. 22:00 → 08:00.",
        })}
        helpTitle={t("usage.pricing.timePricing.windowHelpTitle", {
          defaultValue: "How the time window works",
        })}
        errorCode={errorFor("startMinute") ?? errorFor("endMinute")}
      >
        <div className="flex items-center gap-2">
          <Input
            id="time-rule-start"
            value={draft.startText}
            onChange={(event) => patch({ startText: event.target.value })}
            placeholder="HH:MM"
            inputMode="numeric"
            aria-label={t("usage.pricing.timePricing.start", {
              defaultValue: "Start",
            })}
            aria-invalid={errorFor("startMinute") ? true : undefined}
            className="w-28 font-mono"
            required
          />
          <span className="text-fg-3">→</span>
          <Input
            id="time-rule-end"
            value={draft.endText}
            onChange={(event) => patch({ endText: event.target.value })}
            placeholder="HH:MM"
            inputMode="numeric"
            aria-label={t("usage.pricing.timePricing.end", {
              defaultValue: "End",
            })}
            aria-invalid={errorFor("endMinute") ? true : undefined}
            className="w-28 font-mono"
            required
          />
        </div>
      </Field>

      <Field
        id="time-rule-excluded"
        label={t("usage.pricing.timePricing.excludedDates", {
          defaultValue: "Excluded dates",
        })}
        hint={t("usage.pricing.timePricing.excludedDatesHint", {
          defaultValue:
            "Local dates the rule never applies on — this is how you say “except public holidays”.",
        })}
        helpTitle={t("usage.pricing.timePricing.excludedDatesHelpTitle", {
          defaultValue: "What excluded dates are for",
        })}
        errorCode={errorFor("excludedDates")}
      >
        <Input
          id="time-rule-excluded"
          value={draft.excludedDatesText}
          onChange={(event) => patch({ excludedDatesText: event.target.value })}
          placeholder={t("usage.pricing.timePricing.excludedDatesPlaceholder", {
            defaultValue: "2026-10-01, 2026-10-02",
          })}
          className="font-mono"
          aria-invalid={errorFor("excludedDates") ? true : undefined}
        />
      </Field>

      <div className="grid gap-4 sm:grid-cols-2">
        {PRICE_FIELDS.map((price) => (
          <Field
            key={price.id}
            id={`time-rule-${price.id}`}
            label={t(`usage.pricing.timePricing.${price.labelKey}`, {
              defaultValue: price.defaultLabel,
            })}
            hint={
              price.id === "cacheCreationCostPerMillion"
                ? t("usage.pricing.timePricing.cacheCreationHint", {
                    defaultValue:
                      "Rarely published on its own. Many providers charge a cache write at the same rate as a cache miss, so copy the input price unless your bill says otherwise. Never leave it at 0 unless a cache write really is free — 0 bills every cache write as nothing.",
                  })
                : undefined
            }
            helpTitle={
              price.id === "cacheCreationCostPerMillion"
                ? t("usage.pricing.timePricing.cacheCreationHelpTitle", {
                    defaultValue: "What to put in the cache write price",
                  })
                : undefined
            }
            // 四个单价共用同一句错误文案，只挂在第一个出错的上，别重复四遍
            errorCode={
              price.id === firstPriceError?.id ? errorFor(price.id) : undefined
            }
          >
            <Input
              id={`time-rule-${price.id}`}
              type="number"
              step={PRICE_STEP}
              min="0"
              value={draft[price.id]}
              onChange={(event) =>
                patch({
                  [price.id]: event.target.value,
                } as Partial<TimeRuleDraft>)
              }
              aria-invalid={errorFor(price.id) ? true : undefined}
              required
            />
          </Field>
        ))}
      </div>

      <div className="rounded-panel border border-border bg-subtle px-3 py-2.5">
        <div className="text-caption text-fg-2">
          {t("usage.pricing.timePricing.previewTitle", {
            defaultValue: "Reads as",
          })}
        </div>
        <div className="text-body text-fg-1">
          {previewRule ? (
            describeTimeRule(previewRule)
          ) : (
            <span className="text-fg-3">
              {t("usage.pricing.timePricing.previewEmpty", {
                defaultValue:
                  "Fill in the offset and the time window to see how the rule reads.",
              })}
            </span>
          )}
        </div>
      </div>
    </form>
  );
}

interface FieldProps {
  id: string;
  label: string;
  hint?: string;
  /** 「?」卡片的标题，同时是那颗按钮的无障碍名字；不能和 label 同名，否则两个控件撞名 */
  helpTitle?: string;
  errorCode?: TimeRuleErrorCode;
  aside?: React.ReactNode;
  children: React.ReactNode;
}

function Field({
  id,
  label,
  hint,
  helpTitle,
  errorCode,
  aside,
  children,
}: FieldProps) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-1">
        <Label htmlFor={id}>{label}</Label>
        {hint && (
          <HelpTip title={helpTitle ?? label}>
            <span className="whitespace-pre-line">{hint}</span>
          </HelpTip>
        )}
        {aside && <div className="ms-auto">{aside}</div>}
      </div>
      {children}
      {errorCode && (
        <div className="text-caption text-danger-text">
          {t(`usage.pricing.timePricing.error.${errorCode}`, {
            defaultValue: ERROR_DEFAULTS[errorCode],
          })}
        </div>
      )}
    </div>
  );
}
