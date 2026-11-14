import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Plus,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { isPlainObject } from "@/lib/requestOverrides";

/**
 * Levels @ai-sdk/openai (Responses path) injects by default. A config-defined
 * `variants` object REPLACES that list instead of appending to it, so the
 * template fills every standard level (gateways often expose extra ones).
 */
const STANDARD_VARIANT_LEVELS = [
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
] as const;

/** Per-level value the issue's structure shows for every standard level. */
const TEMPLATE_INCLUDE = ["reasoning.encrypted_content"];

interface ParsedVariant {
  /** reasoningEffort as edited; "" means not set. */
  effort: string;
  /** reasoningSummary as edited; "" means not set. */
  summary: string;
  /** The include slot keeps the stored value type (array, boolean, ...). */
  include: unknown;
  hasInclude: boolean;
  /** Fields the structured inputs do not cover (disabled, tools, ...). */
  rest: Record<string, unknown>;
}

function parseVariantValue(value: unknown): ParsedVariant {
  const parsed: ParsedVariant = {
    effort: "",
    summary: "",
    include: undefined,
    hasInclude: false,
    rest: {},
  };
  if (!isPlainObject(value)) return parsed;
  for (const [key, val] of Object.entries(value)) {
    if (key === "reasoningEffort") {
      if (typeof val === "string") parsed.effort = val;
      else parsed.rest[key] = val;
    } else if (key === "reasoningSummary") {
      if (typeof val === "string") parsed.summary = val;
      else parsed.rest[key] = val;
    } else if (key === "include") {
      parsed.include = val;
      parsed.hasInclude = true;
    } else {
      parsed.rest[key] = val;
    }
  }
  return parsed;
}

function buildVariantValue(parsed: ParsedVariant): Record<string, unknown> {
  // Rest first, so a structured field the user explicitly types overrides a
  // stray same-named raw field, while a cleared field keeps the raw value.
  const next = { ...parsed.rest };
  if (parsed.effort) next.reasoningEffort = parsed.effort;
  if (parsed.summary) next.reasoningSummary = parsed.summary;
  if (parsed.hasInclude && parsed.include !== undefined) {
    next.include = parsed.include;
  }
  return next;
}

function formatIncludeValue(include: unknown): string {
  if (include === undefined) return "";
  if (Array.isArray(include))
    return include.map((item) => String(item)).join(", ");
  if (typeof include === "boolean" || typeof include === "string") {
    return String(include);
  }
  return stringifyInline(include);
}

/** "" removes the field; "true"/"false" keep the boolean form; a non-empty
 * list becomes a string array (e.g. "reasoning.encrypted_content"). */
function parseIncludeInput(text: string): unknown {
  const trimmed = text.trim();
  if (!trimmed) return undefined;
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  const items = trimmed
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  return items.length > 0 ? items : undefined;
}

function stringifyPretty(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? "";
  } catch {
    return "";
  }
}

function stringifyInline(value: unknown): string {
  try {
    return JSON.stringify(value) ?? "";
  } catch {
    return String(value);
  }
}

/** Local draft for a blur-committed field, resynced when the canonical
 * display form coming from the parent actually changes. */
function useExternalSyncedDraft(display: string) {
  const [draft, setDraft] = useState(display);
  const lastDisplayRef = useRef(display);
  useEffect(() => {
    if (lastDisplayRef.current !== display) {
      lastDisplayRef.current = display;
      setDraft(display);
    }
  }, [display]);
  return [draft, setDraft] as const;
}

function VariantIdInput({
  levelId,
  onRename,
  ariaLabel,
  placeholder,
}: {
  levelId: string;
  onRename: (nextId: string) => boolean;
  ariaLabel: string;
  placeholder: string;
}) {
  const [localValue, setLocalValue] = useState(levelId);

  useEffect(() => {
    setLocalValue(levelId);
  }, [levelId]);

  return (
    <Input
      value={localValue}
      onChange={(event) => setLocalValue(event.target.value)}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        event.currentTarget.blur();
      }}
      onBlur={() => {
        const nextId = localValue.trim();
        if (!nextId || nextId === levelId) {
          setLocalValue(levelId);
          return;
        }
        if (!onRename(nextId)) setLocalValue(levelId);
      }}
      aria-label={ariaLabel}
      placeholder={placeholder}
      className="w-28 shrink-0"
    />
  );
}

interface VariantRowProps {
  levelId: string;
  value: unknown;
  isFirst: boolean;
  isLast: boolean;
  onRename: (nextId: string) => boolean;
  onValueChange: (nextValue: Record<string, unknown>) => void;
  onMove: (direction: -1 | 1) => void;
  onRemove: () => void;
}

function VariantRow({
  levelId,
  value,
  isFirst,
  isLast,
  onRename,
  onValueChange,
  onMove,
  onRemove,
}: VariantRowProps) {
  const { t } = useTranslation();
  const parsed = parseVariantValue(value);
  // A variant whose value is not an object cannot be projected onto the
  // structured inputs; keep it as-is instead of silently rewriting it.
  const representable = isPlainObject(value);

  const otherFieldsLabel = t("opencode.variantOtherFields", {
    defaultValue: "Other fields (JSON)",
  });

  const updateFromParsed = (patch: Partial<ParsedVariant>) => {
    onValueChange(buildVariantValue({ ...parsed, ...patch }));
  };

  const includeDisplay = formatIncludeValue(parsed.include);
  const [includeDraft, setIncludeDraft] =
    useExternalSyncedDraft(includeDisplay);
  const commitInclude = () => {
    if (includeDraft === includeDisplay) return;
    const nextInclude = parseIncludeInput(includeDraft);
    updateFromParsed({
      include: nextInclude,
      hasInclude: nextInclude !== undefined,
    });
    setIncludeDraft(formatIncludeValue(nextInclude));
  };

  const extrasCount = Object.keys(parsed.rest).length;
  const extrasDisplay = extrasCount > 0 ? stringifyPretty(parsed.rest) : "";
  const [extrasDraft, setExtrasDraft] = useExternalSyncedDraft(extrasDisplay);
  const [extrasOpen, setExtrasOpen] = useState(false);
  const [extrasInvalid, setExtrasInvalid] = useState(false);
  const commitExtras = () => {
    if (extrasDraft === extrasDisplay) {
      setExtrasInvalid(false);
      return;
    }
    if (!extrasDraft.trim()) {
      setExtrasInvalid(false);
      updateFromParsed({ rest: {} });
      setExtrasDraft("");
      return;
    }
    let nextRest: unknown;
    try {
      nextRest = JSON.parse(extrasDraft);
    } catch {
      setExtrasInvalid(true);
      return;
    }
    if (!isPlainObject(nextRest)) {
      setExtrasInvalid(true);
      return;
    }
    setExtrasInvalid(false);
    updateFromParsed({ rest: nextRest });
    setExtrasDraft(stringifyPretty(nextRest));
  };

  return (
    <div className="space-y-1.5 rounded-lg border border-border/60 p-2">
      <div className="flex items-center gap-2">
        <VariantIdInput
          levelId={levelId}
          onRename={onRename}
          ariaLabel={t("opencode.variantLevel", { defaultValue: "Level" })}
          placeholder={t("opencode.variantLevelPlaceholder", {
            defaultValue: "max",
          })}
        />
        <ImeSafeInput
          value={parsed.effort}
          onValueChange={(next) => updateFromParsed({ effort: next })}
          aria-label={t("opencode.variantEffort", {
            defaultValue: "Reasoning effort",
          })}
          placeholder={t("opencode.variantEffortPlaceholder", {
            defaultValue: "low",
          })}
          className="w-24 shrink-0"
        />
        <ImeSafeInput
          value={parsed.summary}
          onValueChange={(next) => updateFromParsed({ summary: next })}
          aria-label={t("opencode.variantSummary", {
            defaultValue: "Reasoning summary",
          })}
          placeholder={t("opencode.variantSummaryPlaceholder", {
            defaultValue: "auto",
          })}
          className="w-24 shrink-0"
        />
        <div className="flex shrink-0 items-center">
          <HoverTip
            content={t("opencode.moveVariantUp", {
              defaultValue: "Move level up",
            })}
          >
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => onMove(-1)}
              disabled={isFirst}
              aria-label={t("opencode.moveVariantUp", {
                defaultValue: "Move level up",
              })}
              className="h-7 w-7"
            >
              <ChevronUp className="h-3.5 w-3.5" />
            </Button>
          </HoverTip>
          <HoverTip
            content={t("opencode.moveVariantDown", {
              defaultValue: "Move level down",
            })}
          >
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => onMove(1)}
              disabled={isLast}
              aria-label={t("opencode.moveVariantDown", {
                defaultValue: "Move level down",
              })}
              className="h-7 w-7"
            >
              <ChevronDown className="h-3.5 w-3.5" />
            </Button>
          </HoverTip>
          <HoverTip content={t("common.delete")}>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={onRemove}
              aria-label={t("common.delete")}
              className="h-7 w-7 text-fg-2 hover:text-destructive"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </Button>
          </HoverTip>
        </div>
      </div>

      {representable ? (
        <>
          <div className="flex items-center gap-2">
            <Input
              value={includeDraft}
              onChange={(event) => setIncludeDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key !== "Enter") return;
                event.preventDefault();
                event.currentTarget.blur();
              }}
              onBlur={commitInclude}
              aria-label={t("opencode.variantInclude", {
                defaultValue: "Include",
              })}
              placeholder={t("opencode.variantIncludePlaceholder", {
                defaultValue: "reasoning.encrypted_content",
              })}
              className="min-w-0 flex-1 font-mono"
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => setExtrasOpen((open) => !open)}
              aria-label={otherFieldsLabel}
              aria-expanded={extrasOpen}
              className="h-7 shrink-0 gap-1 px-2 text-xs text-fg-2"
            >
              <ChevronRight
                className={cn(
                  "h-3 w-3 transition-transform",
                  extrasOpen && "rotate-90",
                )}
              />
              {extrasCount > 0
                ? `${otherFieldsLabel} (${extrasCount})`
                : otherFieldsLabel}
            </Button>
          </div>

          {extrasOpen && (
            <div className="space-y-1">
              <Textarea
                value={extrasDraft}
                onChange={(event) => setExtrasDraft(event.target.value)}
                onBlur={commitExtras}
                aria-label={otherFieldsLabel}
                placeholder={t("opencode.variantOtherFieldsPlaceholder", {
                  defaultValue: '{"disabled": true}',
                })}
                rows={3}
                className="min-h-[60px] font-mono text-xs"
              />
              {extrasInvalid && (
                <p className="text-xs text-destructive">
                  {t("opencode.variantOtherFieldsInvalid", {
                    defaultValue: "Other fields must be a JSON object",
                  })}
                </p>
              )}
            </div>
          )}
        </>
      ) : (
        <p className="truncate font-mono text-xs text-fg-2">
          {stringifyInline(value)}
        </p>
      )}
    </div>
  );
}

interface OpenCodeVariantsEditorProps {
  /** The model's `variants` value (V1 object form: level id -> settings). */
  variants: unknown;
  /** Receives the whole replacement map; an empty map means "remove it". */
  onVariantsChange: (variants: Record<string, unknown>) => void;
}

/**
 * Structured editor for a model's reasoning levels (`variants`), replacing
 * the raw-JSON route through "Model Properties". Renders for models without
 * `variants` too, so imported providers can discover it (see issue #7802).
 */
export function OpenCodeVariantsEditor({
  variants,
  onVariantsChange,
}: OpenCodeVariantsEditorProps) {
  const { t } = useTranslation();
  const title = t("opencode.variants", {
    defaultValue: "Reasoning Levels (variants)",
  });
  const hint = t("opencode.variantsHint", {
    defaultValue:
      "Per-model reasoning levels passed to the AI SDK. Declaring levels here replaces the package's built-in level list, so declare every level the model should expose. Levels are saved in the order shown.",
  });

  // Non-object shapes (e.g. a pasted native V2 array) are not represented by
  // this editor; leave them to raw JSON editing in Model Properties.
  if (variants !== undefined && !isPlainObject(variants)) {
    return (
      <div className="space-y-2">
        <div className="flex items-center gap-1">
          <span className="text-xs font-medium text-fg-2">{title}</span>
          <HelpTip title={title}>{hint}</HelpTip>
        </div>
        <p className="py-1 text-xs text-fg-2">
          {t("opencode.variantsRawFallback", {
            defaultValue:
              "variants is not in object form; edit it as raw JSON in Model Properties below.",
          })}
        </p>
      </div>
    );
  }

  const entries: Array<[string, unknown]> = Object.entries(variants ?? {});

  const replaceEntries = (next: Array<[string, unknown]>) => {
    const map: Record<string, unknown> = {};
    for (const [key, value] of next) map[key] = value;
    onVariantsChange(map);
  };

  const setLevelValue = (levelId: string, nextValue: unknown) => {
    replaceEntries(
      entries.map(([key, value]): [string, unknown] =>
        key === levelId ? [levelId, nextValue] : [key, value],
      ),
    );
  };

  const renameLevel = (levelId: string, nextId: string): boolean => {
    const trimmed = nextId.trim();
    if (!trimmed || trimmed === levelId) return false;
    if (entries.some(([key]) => key === trimmed)) return false;
    replaceEntries(
      entries.map(([key, value]): [string, unknown] =>
        key === levelId ? [trimmed, value] : [key, value],
      ),
    );
    return true;
  };

  const removeLevel = (levelId: string) => {
    replaceEntries(entries.filter(([key]) => key !== levelId));
  };

  const moveLevel = (index: number, direction: -1 | 1) => {
    const target = index + direction;
    if (target < 0 || target >= entries.length) return;
    const next = [...entries];
    [next[index], next[target]] = [next[target], next[index]];
    replaceEntries(next);
  };

  const addLevel = () => {
    let suffix = Date.now();
    while (entries.some(([key]) => key === `variant-${suffix}`)) suffix += 1;
    replaceEntries([...entries, [`variant-${suffix}`, {}]]);
  };

  const missingStandardLevels = STANDARD_VARIANT_LEVELS.filter(
    (level) => !entries.some(([key]) => key === level),
  );

  const fillStandardLevels = () => {
    replaceEntries([
      ...entries,
      ...missingStandardLevels.map((level): [string, unknown] => [
        level,
        {
          reasoningEffort: level,
          reasoningSummary: "auto",
          include: [...TEMPLATE_INCLUDE],
        },
      ]),
    ]);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-1 items-center gap-1">
          <span className="text-xs font-medium text-fg-2">{title}</span>
          <HelpTip title={title}>{hint}</HelpTip>
        </div>
        <div className="flex shrink-0 gap-1">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={fillStandardLevels}
            disabled={missingStandardLevels.length === 0}
            className="h-6 gap-1 px-2 text-xs"
          >
            {t("opencode.fillVariantTemplate", {
              defaultValue: "Fill 7 standard levels",
            })}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={addLevel}
            className="h-6 gap-1 px-2"
          >
            <Plus className="h-3 w-3" />
            {t("opencode.addVariant", { defaultValue: "Add level" })}
          </Button>
        </div>
      </div>

      {entries.length === 0 ? (
        <p className="py-1 text-xs text-fg-2">
          {t("opencode.noVariants", {
            defaultValue: "No reasoning levels configured",
          })}
        </p>
      ) : (
        <div className="space-y-2">
          {entries.map(([levelId, value], index) => (
            <VariantRow
              key={levelId}
              levelId={levelId}
              value={value}
              isFirst={index === 0}
              isLast={index === entries.length - 1}
              onRename={(nextId) => renameLevel(levelId, nextId)}
              onValueChange={(nextValue) => setLevelValue(levelId, nextValue)}
              onMove={(direction) => moveLevel(index, direction)}
              onRemove={() => removeLevel(levelId)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
