import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Wand2,
  Settings,
  FolderInput,
  Loader2,
  HelpCircle,
  Check,
  ChevronsUpDown,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import { useReadOmoLocalFile, useReadOmoSlimLocalFile } from "@/lib/query/omo";
import {
  OMO_BUILTIN_AGENTS,
  OMO_BUILTIN_CATEGORIES,
  OMO_SLIM_BUILTIN_AGENTS,
  type OmoAgentDef,
  type OmoCategoryDef,
} from "@/types/omo";

const ADVANCED_PLACEHOLDER = `{
  "temperature": 0.5,
  "top_p": 0.9,
  "budgetTokens": 20000,
  "prompt_append": "",
  "permission": { "edit": "allow", "bash": "ask" }
}`;

interface OmoFormFieldsProps {
  modelOptions: Array<{ value: string; label: string }>;
  modelVariantsMap?: Record<string, string[]>;
  presetMetaMap?: Record<
    string,
    {
      options?: Record<string, unknown>;
      limit?: { context?: number; output?: number };
    }
  >;
  agents: Record<string, Record<string, unknown>>;
  onAgentsChange: (agents: Record<string, Record<string, unknown>>) => void;
  categories?: Record<string, Record<string, unknown>>;
  onCategoriesChange?: (
    categories: Record<string, Record<string, unknown>>,
  ) => void;
  otherFieldsStr: string;
  onOtherFieldsStrChange: (value: string) => void;
  isSlim?: boolean;
}

export type CustomModelItem = {
  key: string;
  model: string;
  sourceKey?: string;
};
type BuiltinModelDef = Pick<
  OmoAgentDef | OmoCategoryDef,
  "key" | "display" | "descKey" | "recommended" | "tooltipKey"
>;
type ModelOption = { value: string; label: string };

function DeferredKeyInput({
  value,
  onCommit,
  placeholder,
  className,
}: {
  value: string;
  onCommit: (value: string) => void;
  placeholder?: string;
  className?: string;
}) {
  const [draft, setDraft] = useState(value);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  return (
    <Input
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        if (draft !== value) {
          onCommit(draft);
        }
      }}
      placeholder={placeholder}
      className={className}
    />
  );
}

const BUILTIN_AGENT_KEYS = new Set(OMO_BUILTIN_AGENTS.map((a) => a.key));
const BUILTIN_AGENT_KEYS_SLIM = new Set(
  OMO_SLIM_BUILTIN_AGENTS.map((a) => a.key),
);
const BUILTIN_CATEGORY_KEYS = new Set(OMO_BUILTIN_CATEGORIES.map((c) => c.key));
const EMPTY_VARIANT_VALUE = "__cc_switch_omo_variant_empty__";

function ModelCombobox({
  value,
  options,
  recommended,
  onChange,
}: {
  value: string;
  options: ModelOption[];
  recommended?: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const selectedLabel = options.find((o) => o.value === value)?.label;

  const selectModelText = t("omo.selectModel", {
    defaultValue: "Select configured model",
  });
  const placeholderText = recommended
    ? `${selectModelText} (${t("omo.recommendedHint", { model: recommended, defaultValue: "Recommended: {{model}}" })})`
    : selectModelText;

  return (
    <Popover modal open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-expanded={open}
          className="flex flex-1 h-8 items-center justify-between whitespace-nowrap rounded-md border border-border-default bg-background px-3 py-1 text-sm shadow-sm ring-offset-background focus:outline-none focus-visible:outline-none focus:border-border-default focus-visible:border-border-default focus:ring-0 focus-visible:ring-0 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <span className={cn("truncate", !value && "text-muted-foreground")}>
            {selectedLabel || placeholderText}
          </span>
          <span className="flex items-center shrink-0 ml-1 gap-0.5">
            {value && (
              <X
                className="h-3.5 w-3.5 opacity-50 hover:opacity-100 cursor-pointer"
                onClick={(e) => {
                  e.stopPropagation();
                  onChange("");
                }}
              />
            )}
            <ChevronsUpDown className="h-3.5 w-3.5 opacity-50" />
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        sideOffset={6}
        avoidCollisions={true}
        collisionPadding={8}
        className="z-[1000] w-[var(--radix-popover-trigger-width)] p-0 border-border-default"
      >
        <Command>
          <CommandInput
            placeholder={t("omo.searchModel", {
              defaultValue: "Search model...",
            })}
          />
          <CommandList>
            <CommandEmpty>
              {t("omo.noEnabledModels", {
                defaultValue: "No configured models",
              })}
            </CommandEmpty>
            <CommandGroup>
              {options.map((option) => (
                <CommandItem
                  key={option.value}
                  value={option.value}
                  keywords={[option.label]}
                  onSelect={() => {
                    onChange(option.value);
                    setOpen(false);
                  }}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4",
                      value === option.value ? "opacity-100" : "opacity-0",
                    )}
                  />
                  {option.label}
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function getAdvancedStr(config: Record<string, unknown> | undefined): string {
  if (!config) return "";
  const adv: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(config)) {
    if (k !== "model" && k !== "variant") adv[k] = v;
  }
  return Object.keys(adv).length > 0 ? JSON.stringify(adv, null, 2) : "";
}

function collectCustomModels(
  store: Record<string, Record<string, unknown>>,
  builtinKeys: Set<string>,
): CustomModelItem[] {
  const customs: CustomModelItem[] = [];
  for (const [k, v] of Object.entries(store)) {
    if (!builtinKeys.has(k) && typeof v === "object" && v !== null) {
      customs.push({
        key: k,
        model: ((v as Record<string, unknown>).model as string) || "",
        sourceKey: k,
      });
    }
  }
  return customs;
}

export function mergeCustomModelsIntoStore(
  store: Record<string, Record<string, unknown>>,
  builtinKeys: Set<string>,
  customs: CustomModelItem[],
  modelVariantsMap: Record<string, string[]>,
): Record<string, Record<string, unknown>> {
  const updated: Record<string, Record<string, unknown>> = {};

  for (const [key, value] of Object.entries(store)) {
    if (builtinKeys.has(key)) {
      updated[key] = { ...value };
    }
  }

  for (const custom of customs) {
    const targetKey = custom.key.trim();
    if (!targetKey) continue;

    const sourceKey = (custom.sourceKey || targetKey).trim();
    const sourceEntry = store[sourceKey] ?? store[targetKey];
    const nextEntry = {
      ...(updated[targetKey] || {}),
      ...(sourceEntry || {}),
    };

    if (custom.model.trim()) {
      nextEntry.model = custom.model;
      const currentVariant =
        typeof nextEntry.variant === "string" ? nextEntry.variant : "";
      if (currentVariant) {
        const validVariants = modelVariantsMap[custom.model] || [];
        if (!validVariants.includes(currentVariant)) {
          delete nextEntry.variant;
        }
      }
      updated[targetKey] = nextEntry;
      continue;
    }

    delete nextEntry.model;
    delete nextEntry.variant;
    if (Object.keys(nextEntry).length > 0) {
      updated[targetKey] = nextEntry;
    } else {
      delete updated[targetKey];
    }
  }
  return updated;
}

export function OmoFormFields({
  modelOptions,
  modelVariantsMap = {},
  presetMetaMap: _presetMetaMap = {},
  agents,
  onAgentsChange,
  categories = {},
  onCategoriesChange,
  otherFieldsStr,
  onOtherFieldsStrChange,
  isSlim = false,
}: OmoFormFieldsProps) {
  const { t } = useTranslation();

  const builtinAgentDefs = isSlim
    ? OMO_SLIM_BUILTIN_AGENTS
    : OMO_BUILTIN_AGENTS;
  const builtinAgentKeys = isSlim
    ? BUILTIN_AGENT_KEYS_SLIM
    : BUILTIN_AGENT_KEYS;

  const [mainAgentsOpen, setMainAgentsOpen] = useState(true);
  const [subAgentsOpen, setSubAgentsOpen] = useState(true);
  const [categoriesOpen, setCategoriesOpen] = useState(true);
  const [otherFieldsOpen, setOtherFieldsOpen] = useState(false);

  const [expandedAgents, setExpandedAgents] = useState<Record<string, boolean>>(
    {},
  );
  const [expandedCategories, setExpandedCategories] = useState<
    Record<string, boolean>
  >({});
  const [agentAdvancedDrafts, setAgentAdvancedDrafts] = useState<
    Record<string, string>
  >({});
  const [categoryAdvancedDrafts, setCategoryAdvancedDrafts] = useState<
    Record<string, string>
  >({});

  const [customAgents, setCustomAgents] = useState<CustomModelItem[]>(() =>
    collectCustomModels(agents, builtinAgentKeys),
  );

  const [customCategories, setCustomCategories] = useState<CustomModelItem[]>(
    () => collectCustomModels(categories, BUILTIN_CATEGORY_KEYS),
  );

  useEffect(() => {
    setCustomAgents(collectCustomModels(agents, builtinAgentKeys));
  }, [agents]);

  useEffect(() => {
    setCustomCategories(collectCustomModels(categories, BUILTIN_CATEGORY_KEYS));
  }, [categories]);

  const syncCustomAgents = useCallback(
    (customs: CustomModelItem[]) => {
      onAgentsChange(
        mergeCustomModelsIntoStore(
          agents,
          builtinAgentKeys,
          customs,
          modelVariantsMap,
        ),
      );
    },
    [agents, onAgentsChange, modelVariantsMap, builtinAgentKeys],
  );

  const syncCustomCategories = useCallback(
    (customs: CustomModelItem[]) => {
      if (!onCategoriesChange) return;
      onCategoriesChange(
        mergeCustomModelsIntoStore(
          categories,
          BUILTIN_CATEGORY_KEYS,
          customs,
          modelVariantsMap,
        ),
      );
    },
    [categories, onCategoriesChange, modelVariantsMap],
  );

  const buildEffectiveModelOptions = useCallback(
    (currentModel: string): ModelOption[] => {
      if (!currentModel) return modelOptions;
      if (modelOptions.some((item) => item.value === currentModel)) {
        return modelOptions;
      }
      return [
        {
          value: currentModel,
          label: t("omo.currentValueNotEnabled", {
            value: currentModel,
            defaultValue: "{{value}} (current value, not enabled)",
          }),
        },
        ...modelOptions,
      ];
    },
    [modelOptions, t],
  );

  const resolveRecommendedModel = useCallback(
    (recommended?: string): string | undefined => {
      if (!recommended || modelOptions.length === 0) return undefined;

      const exact = modelOptions.find((item) => item.value === recommended);
      if (exact) return exact.value;

      const bySuffix = modelOptions.find((item) =>
        item.value.endsWith(`/${recommended}`),
      );
      return bySuffix?.value;
    },
    [modelOptions],
  );

  const renderModelSelect = (
    currentModel: string,
    onChange: (value: string) => void,
    recommended?: string,
  ) => {
    const options = buildEffectiveModelOptions(currentModel);
    return (
      <ModelCombobox
        value={currentModel}
        options={options}
        recommended={recommended}
        onChange={onChange}
      />
    );
  };

  const buildEffectiveVariantOptions = useCallback(
    (currentModel: string, currentVariant: string): string[] => {
      const variantKeys = modelVariantsMap[currentModel] || [];
      if (!currentVariant || variantKeys.includes(currentVariant)) {
        return variantKeys;
      }
      return [currentVariant, ...variantKeys];
    },
    [modelVariantsMap],
  );

  const renderVariantSelect = (
    currentModel: string,
    currentVariant: string,
    onChange: (value: string) => void,
  ) => {
    const hasModel = Boolean(currentModel);
    const modelVariantKeys = hasModel
      ? modelVariantsMap[currentModel] || []
      : [];
    const hasVariants = modelVariantKeys.length > 0;
    const shouldShow = hasModel && (hasVariants || Boolean(currentVariant));

    if (!shouldShow) {
      return null;
    }

    const variantOptions = buildEffectiveVariantOptions(
      currentModel,
      currentVariant,
    );
    const firstIsUnavailable =
      Boolean(currentVariant) &&
      !(modelVariantsMap[currentModel] || []).includes(currentVariant);

    return (
      <Select
        value={currentVariant || EMPTY_VARIANT_VALUE}
        onValueChange={(value) =>
          onChange(value === EMPTY_VARIANT_VALUE ? "" : value)
        }
      >
        <SelectTrigger className="w-28 h-8 text-xs shrink-0">
          <SelectValue
            placeholder={t("omo.variantPlaceholder", {
              defaultValue: "variant",
            })}
          />
        </SelectTrigger>
        <SelectContent className="max-h-72">
          <SelectItem value={EMPTY_VARIANT_VALUE}>
            {t("omo.defaultWrapped", { defaultValue: "(Default)" })}
          </SelectItem>
          {variantOptions.map((variant, index) => (
            <SelectItem key={`${variant}-${index}`} value={variant}>
              {firstIsUnavailable && index === 0
                ? t("omo.currentValueUnavailable", {
                    value: variant,
                    defaultValue: "{{value}} (current value, unavailable)",
                  })
                : variant}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  };

  const handleModelChange = (
    key: string,
    model: string,
    store: Record<string, Record<string, unknown>>,
    setter: (v: Record<string, Record<string, unknown>>) => void,
  ) => {
    if (model.trim()) {
      const nextEntry: Record<string, unknown> = {
        ...(store[key] || {}),
        model,
      };
      const currentVariant =
        typeof nextEntry.variant === "string" ? nextEntry.variant : "";
      if (currentVariant) {
        const validVariants = modelVariantsMap[model] || [];
        if (!validVariants.includes(currentVariant)) {
          delete nextEntry.variant;
        }
      }
      setter({ ...store, [key]: nextEntry });
    } else {
      const existing = store[key];
      if (existing) {
        const adv = { ...existing };
        delete adv.model;
        delete adv.variant;
        if (Object.keys(adv).length > 0) {
          setter({ ...store, [key]: adv });
        } else {
          const next = { ...store };
          delete next[key];
          setter(next);
        }
      }
    }
  };

  const handleVariantChange = (
    key: string,
    variant: string,
    store: Record<string, Record<string, unknown>>,
    setter: (v: Record<string, Record<string, unknown>>) => void,
  ) => {
    const existing = store[key];
    if (variant.trim()) {
      setter({ ...store, [key]: { ...existing, variant } });
      return;
    }

    if (!existing) return;
    const nextEntry = { ...existing };
    delete nextEntry.variant;
    if (Object.keys(nextEntry).length > 0) {
      setter({ ...store, [key]: nextEntry });
      return;
    }

    const next = { ...store };
    delete next[key];
    setter(next);
  };

  const handleAdvancedChange = (
    key: string,
    rawJson: string,
    store: Record<string, Record<string, unknown>>,
    setter: (v: Record<string, Record<string, unknown>>) => void,
  ): boolean => {
    const currentModel = (store[key]?.model as string) || "";
    const currentVariant = (store[key]?.variant as string) || "";
    if (!rawJson.trim()) {
      if (currentModel || currentVariant) {
        setter({
          ...store,
          [key]: {
            ...(currentModel ? { model: currentModel } : {}),
            ...(currentVariant ? { variant: currentVariant } : {}),
          },
        });
      } else {
        const next = { ...store };
        delete next[key];
        setter(next);
      }
      return true;
    }
    try {
      const parsed = JSON.parse(rawJson);
      if (
        typeof parsed === "object" &&
        parsed !== null &&
        !Array.isArray(parsed)
      ) {
        const parsedAdvanced = { ...(parsed as Record<string, unknown>) };
        delete parsedAdvanced.model;
        delete parsedAdvanced.variant;
        setter({
          ...store,
          [key]: {
            ...(currentModel ? { model: currentModel } : {}),
            ...(currentVariant ? { variant: currentVariant } : {}),
            ...parsedAdvanced,
          },
        });
        return true;
      }
      return false;
    } catch {
      return false;
    }
  };

  type AdvancedScope = "agent" | "category";

  const setAdvancedDraft = (
    scope: AdvancedScope,
    key: string,
    value: string,
  ) => {
    if (scope === "agent") {
      setAgentAdvancedDrafts((prev) => ({ ...prev, [key]: value }));
      return;
    }
    setCategoryAdvancedDrafts((prev) => ({ ...prev, [key]: value }));
  };

  const removeAdvancedDraft = (scope: AdvancedScope, key: string) => {
    if (scope === "agent") {
      setAgentAdvancedDrafts((prev) => {
        const copied = { ...prev };
        delete copied[key];
        return copied;
      });
      return;
    }
    setCategoryAdvancedDrafts((prev) => {
      const copied = { ...prev };
      delete copied[key];
      return copied;
    });
  };

  const toggleAdvancedEditor = (
    scope: AdvancedScope,
    key: string,
    advStr: string,
    isExpanded: boolean,
  ) => {
    const willOpen = !isExpanded;
    if (scope === "agent") {
      setExpandedAgents((prev) => ({ ...prev, [key]: willOpen }));
      if (willOpen && agentAdvancedDrafts[key] === undefined) {
        setAdvancedDraft(scope, key, advStr);
      }
      return;
    }
    setExpandedCategories((prev) => ({ ...prev, [key]: willOpen }));
    if (willOpen && categoryAdvancedDrafts[key] === undefined) {
      setAdvancedDraft(scope, key, advStr);
    }
  };

  const renderAdvancedEditor = ({
    scope,
    draftKey,
    configKey,
    draftValue,
    store,
    setter,
    showHint,
  }: {
    scope: AdvancedScope;
    draftKey: string;
    configKey: string;
    draftValue: string;
    store: Record<string, Record<string, unknown>>;
    setter: (value: Record<string, Record<string, unknown>>) => void;
    showHint?: boolean;
  }) => (
    <div className="pb-2 pl-2 pr-2">
      <Textarea
        value={draftValue}
        onChange={(e) => setAdvancedDraft(scope, draftKey, e.target.value)}
        onBlur={(e) => {
          if (!handleAdvancedChange(configKey, e.target.value, store, setter)) {
            toast.error(
              t("omo.advancedJsonInvalid", {
                defaultValue: "Advanced JSON is invalid",
              }),
            );
          }
        }}
        placeholder={ADVANCED_PLACEHOLDER}
        className="font-mono text-xs min-h-[130px] py-3"
      />
      {showHint && (
        <p className="text-[10px] text-muted-foreground mt-1">
          {t("omo.advancedJsonHint", {
            defaultValue:
              "temperature, top_p, budgetTokens, prompt_append, permission, etc. Leave empty for defaults",
          })}
        </p>
      )}
    </div>
  );

  const handleFillAllRecommended = () => {
    if (modelOptions.length === 0) {
      toast.warning(
        t("omo.noEnabledModelsWarning", {
          defaultValue:
            "No configured models available. Configure OpenCode models first.",
        }),
      );
      return;
    }

    const updatedAgents = { ...agents };
    for (const agentDef of builtinAgentDefs) {
      const recommendedValue = resolveRecommendedModel(agentDef.recommended);
      if (recommendedValue && !updatedAgents[agentDef.key]?.model) {
        updatedAgents[agentDef.key] = {
          ...updatedAgents[agentDef.key],
          model: recommendedValue,
        };
      }
    }
    onAgentsChange(updatedAgents);

    if (!isSlim && onCategoriesChange) {
      const updatedCategories = { ...categories };
      for (const catDef of OMO_BUILTIN_CATEGORIES) {
        const recommendedValue = resolveRecommendedModel(catDef.recommended);
        if (recommendedValue && !updatedCategories[catDef.key]?.model) {
          updatedCategories[catDef.key] = {
            ...updatedCategories[catDef.key],
            model: recommendedValue,
          };
        }
      }
      onCategoriesChange(updatedCategories);
    }
  };

  const configuredAgentCount = Object.keys(agents).length;
  const configuredCategoryCount = isSlim ? 0 : Object.keys(categories).length;
  const mainAgents = builtinAgentDefs.filter((a) => a.group === "main");
  const subAgents = builtinAgentDefs.filter((a) => a.group === "sub");

  const readLocalFile = useReadOmoLocalFile();
  const readSlimLocalFile = useReadOmoSlimLocalFile();
  const [localFilePath, setLocalFilePath] = useState<string | null>(null);

  const handleImportFromLocal = useCallback(async () => {
    try {
      const data = isSlim
        ? await readSlimLocalFile.mutateAsync()
        : await readLocalFile.mutateAsync();
      const importedAgents =
        (data.agents as Record<string, Record<string, unknown>> | undefined) ||
        {};
      const importedCategories =
        (data.categories as
          | Record<string, Record<string, unknown>>
          | undefined) || {};

      onAgentsChange(importedAgents);
      if (!isSlim && onCategoriesChange) {
        onCategoriesChange(importedCategories);
      }
      onOtherFieldsStrChange(
        data.otherFields ? JSON.stringify(data.otherFields, null, 2) : "",
      );
      setAgentAdvancedDrafts({});
      setCategoryAdvancedDrafts({});
      setCustomAgents(collectCustomModels(importedAgents, builtinAgentKeys));
      if (!isSlim) {
        setCustomCategories(
          collectCustomModels(importedCategories, BUILTIN_CATEGORY_KEYS),
        );
      }
      setLocalFilePath(data.filePath);
      toast.success(
        t("omo.importLocalReplaceSuccess", {
          defaultValue:
            "Imported local file and replaced Agents/Categories/Other Fields",
        }),
      );
    } catch (err) {
      toast.error(
        t("omo.importLocalFailed", {
          error: String(err),
          defaultValue: "Failed to read local file: {{error}}",
        }),
      );
    }
  }, [
    readLocalFile,
    onAgentsChange,
    onCategoriesChange,
    onOtherFieldsStrChange,
    t,
  ]);

  const renderBuiltinModelRow = (
    scope: AdvancedScope,
    def: BuiltinModelDef,
  ) => {
    const isAgent = scope === "agent";
    const store = isAgent ? agents : categories;
    const setter = isAgent ? onAgentsChange : onCategoriesChange!;
    const drafts = isAgent ? agentAdvancedDrafts : categoryAdvancedDrafts;
    const expanded = isAgent ? expandedAgents : expandedCategories;

    const key = def.key;
    const currentModel = (store[key]?.model as string) || "";
    const currentVariant = (store[key]?.variant as string) || "";
    const advStr = getAdvancedStr(store[key]);
    const draftValue = drafts[key] ?? advStr;
    const isExpanded = expanded[key] ?? false;

    return (
      <div key={key} className="border-b border-border/30 last:border-b-0">
        <div className="flex items-center gap-2 py-1.5">
          <div className="w-32 shrink-0">
            <div className="flex items-center gap-1 text-sm font-medium">
              {def.display}
              <span className="relative inline-flex group/tip">
                <HelpCircle className="h-3.5 w-3.5 text-muted-foreground/60 hover:text-muted-foreground cursor-help shrink-0" />
                <span className="invisible opacity-0 group-hover/tip:visible group-hover/tip:opacity-100 transition-opacity duration-150 absolute left-0 top-full mt-1 z-50 w-[260px] rounded-md bg-popover text-popover-foreground border border-border shadow-md px-3 py-2 text-xs leading-relaxed font-normal pointer-events-none">
                  {t(def.tooltipKey)}
                </span>
              </span>
            </div>
            <div className="text-xs text-muted-foreground truncate">
              {t(def.descKey)}
            </div>
          </div>
          {renderModelSelect(
            currentModel,
            (value) => handleModelChange(key, value, store, setter),
            def.recommended,
          )}
          {renderVariantSelect(currentModel, currentVariant, (value) =>
            handleVariantChange(key, value, store, setter),
          )}
          <Button
            type="button"
            variant={isExpanded ? "secondary" : "ghost"}
            size="icon"
            className={cn("h-7 w-7 shrink-0", advStr && "text-primary")}
            onClick={() => toggleAdvancedEditor(scope, key, advStr, isExpanded)}
            title={t("omo.advancedLabel", { defaultValue: "Advanced" })}
          >
            <Settings className="h-3.5 w-3.5" />
          </Button>
        </div>
        {isExpanded &&
          renderAdvancedEditor({
            scope,
            draftKey: key,
            configKey: key,
            draftValue,
            store,
            setter,
            showHint: true,
          })}
      </div>
    );
  };

  const renderAgentRow = (agentDef: OmoAgentDef) =>
    renderBuiltinModelRow("agent", agentDef);

  const renderCategoryRow = (catDef: OmoCategoryDef) =>
    renderBuiltinModelRow("category", catDef);

  const renderCustomModelRow = (
    scope: AdvancedScope,
    item: CustomModelItem,
    index: number,
  ) => {
    const isAgent = scope === "agent";
    const store = isAgent ? agents : categories;
    const setter = isAgent ? onAgentsChange : onCategoriesChange!;
    const drafts = isAgent ? agentAdvancedDrafts : categoryAdvancedDrafts;
    const expanded = isAgent ? expandedAgents : expandedCategories;
    const customs = isAgent ? customAgents : customCategories;
    const setCustoms = isAgent ? setCustomAgents : setCustomCategories;
    const syncCustoms = isAgent ? syncCustomAgents : syncCustomCategories;

    const rowPrefix = isAgent ? "custom-agent" : "custom-cat";
    const emptyKeyPrefix = isAgent ? "__custom_agent_" : "__custom_cat_";
    const keyPlaceholder = isAgent
      ? t("omo.agentKeyPlaceholder", { defaultValue: "agent key" })
      : t("omo.categoryKeyPlaceholder", { defaultValue: "category key" });

    const key = item.key || `${emptyKeyPrefix}${index}`;
    const currentVariant =
      item.key && typeof store[item.key]?.variant === "string"
        ? (store[item.key]?.variant as string) || ""
        : "";
    const advStr = item.key ? getAdvancedStr(store[item.key]) : "";
    const draftValue = drafts[key] ?? advStr;
    const isExpanded = expanded[key] ?? false;

    const updateCustom = (patch: Partial<CustomModelItem>) => {
      const next = [...customs];
      next[index] = { ...next[index], ...patch };
      setCustoms(next);
      syncCustoms(next);
    };

    return (
      <div
        key={`${rowPrefix}-${index}`}
        className="border-b border-border/30 last:border-b-0"
      >
        <div className="flex items-center gap-2 py-1.5">
          <DeferredKeyInput
            value={item.key}
            onCommit={(value) => updateCustom({ key: value })}
            placeholder={keyPlaceholder}
            className="w-32 shrink-0 h-8 text-sm text-primary"
          />
          {renderModelSelect(item.model, (value) =>
            updateCustom({ model: value }),
          )}
          {renderVariantSelect(item.model, currentVariant, (value) => {
            if (!item.key) return;
            handleVariantChange(item.key, value, store, setter);
          })}
          <Button
            type="button"
            variant={isExpanded ? "secondary" : "ghost"}
            size="icon"
            className={cn("h-7 w-7 shrink-0", advStr && "text-primary")}
            onClick={() => toggleAdvancedEditor(scope, key, advStr, isExpanded)}
            title={t("omo.advancedLabel", { defaultValue: "Advanced" })}
          >
            <Settings className="h-3.5 w-3.5" />
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-7 w-7 shrink-0 text-destructive"
            onClick={() => {
              const next = customs.filter((_, idx) => idx !== index);
              setCustoms(next);
              syncCustoms(next);
              removeAdvancedDraft(scope, key);
            }}
          >
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
        {isExpanded &&
          item.key &&
          renderAdvancedEditor({
            scope,
            draftKey: key,
            configKey: item.key,
            draftValue,
            store,
            setter,
          })}
      </div>
    );
  };

  const SectionHeader = ({
    title,
    isOpen,
    onToggle,
    badge,
    action,
  }: {
    title: string;
    isOpen: boolean;
    onToggle: () => void;
    badge?: React.ReactNode | string;
    action?: React.ReactNode;
  }) => (
    <button
      type="button"
      className="flex items-center justify-between w-full py-2 px-3 text-left"
      onClick={onToggle}
    >
      <div className="flex items-center gap-2">
        {isOpen ? (
          <ChevronDown className="h-4 w-4 text-muted-foreground" />
        ) : (
          <ChevronRight className="h-4 w-4 text-muted-foreground" />
        )}
        <Label className="text-sm font-semibold cursor-pointer">{title}</Label>
        {typeof badge === "string" ? (
          <Badge variant="outline" className="text-[10px] h-5">
            {badge}
          </Badge>
        ) : (
          badge
        )}
      </div>
      {action && <div onClick={(e) => e.stopPropagation()}>{action}</div>}
    </button>
  );

  const renderModelSection = ({
    title,
    isOpen,
    onToggle,
    badge,
    action,
    maxHeightClass = "max-h-[5000px]",
    children,
  }: {
    title: string;
    isOpen: boolean;
    onToggle: () => void;
    badge?: React.ReactNode | string;
    action?: React.ReactNode;
    maxHeightClass?: string;
    children: React.ReactNode;
  }) => (
    <div className="rounded-lg border border-border/60">
      <SectionHeader
        title={title}
        isOpen={isOpen}
        onToggle={onToggle}
        badge={badge}
        action={action}
      />
      <div
        className={cn(
          "overflow-hidden transition-all duration-200",
          isOpen ? `${maxHeightClass} opacity-100` : "max-h-0 opacity-0",
        )}
      >
        <div className="px-3 pb-3">{children}</div>
      </div>
    </div>
  );

  const renderCustomAddButton = (onClick: () => void) => (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="h-6 text-xs"
      onClick={onClick}
    >
      <Plus className="h-3.5 w-3.5 mr-1" />
      {t("omo.custom", { defaultValue: "Custom" })}
    </Button>
  );

  const renderCustomDivider = (label: string) => (
    <div className="flex items-center gap-2 py-2">
      <div className="flex-1 border-t border-border/40" />
      <span className="text-[10px] text-muted-foreground">{label}</span>
      <div className="flex-1 border-t border-border/40" />
    </div>
  );

  const addCustomModel = (scope: AdvancedScope) => {
    if (scope === "agent") {
      setCustomAgents((prev) => [
        ...prev,
        { key: "", model: "", sourceKey: "" },
      ]);
      setSubAgentsOpen(true);
      return;
    }
    setCustomCategories((prev) => [
      ...prev,
      { key: "", model: "", sourceKey: "" },
    ]);
    setCategoriesOpen(true);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-semibold">
          {t("omo.modelConfiguration", { defaultValue: "Model Configuration" })}
        </Label>
        <div className="flex items-center gap-1.5">
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            disabled={readLocalFile.isPending}
            onClick={handleImportFromLocal}
          >
            {readLocalFile.isPending ? (
              <Loader2 className="h-3.5 w-3.5 mr-1 animate-spin" />
            ) : (
              <FolderInput className="h-3.5 w-3.5 mr-1" />
            )}
            {t("omo.importLocal", { defaultValue: "Import Local" })}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={handleFillAllRecommended}
          >
            <Wand2 className="h-3.5 w-3.5 mr-1" />
            {t("omo.fillRecommended", { defaultValue: "Fill Recommended" })}
          </Button>
        </div>
      </div>

      <div className="text-xs text-muted-foreground">
        {t("omo.configSummary", {
          agents: configuredAgentCount,
          categories: configuredCategoryCount,
          defaultValue:
            "{{agents}} agents, {{categories}} categories configured · Click ⚙ for advanced params",
        })}
        <span className="ml-1">
          ·{" "}
          {t("omo.enabledModelsCount", {
            count: modelOptions.length,
            defaultValue: "{{count}} configured models available",
          })}
        </span>
        {localFilePath && (
          <span className="ml-1 text-primary/70">
            · {t("omo.source", { defaultValue: "from:" })}{" "}
            <span className="font-mono text-[10px]">
              {localFilePath.replace(/^.*\//, "")}
            </span>
          </span>
        )}
      </div>

      {renderModelSection({
        title: t("omo.mainAgents", { defaultValue: "Main Agents" }),
        isOpen: mainAgentsOpen,
        onToggle: () => setMainAgentsOpen(!mainAgentsOpen),
        badge: `${mainAgents.length}`,
        children: mainAgents.map(renderAgentRow),
      })}

      {renderModelSection({
        title: t("omo.subAgents", { defaultValue: "Sub Agents" }),
        isOpen: subAgentsOpen,
        onToggle: () => setSubAgentsOpen(!subAgentsOpen),
        badge: `${subAgents.length + customAgents.length}`,
        action: renderCustomAddButton(() => addCustomModel("agent")),
        children: (
          <>
            {subAgents.map(renderAgentRow)}
            {customAgents.length > 0 && (
              <>
                {renderCustomDivider(
                  t("omo.customAgents", { defaultValue: "Custom Agents" }),
                )}
                {customAgents.map((a, i) =>
                  renderCustomModelRow("agent", a, i),
                )}
              </>
            )}
          </>
        ),
      })}

      {!isSlim &&
        renderModelSection({
          title: t("omo.categories", { defaultValue: "Categories" }),
          isOpen: categoriesOpen,
          onToggle: () => setCategoriesOpen(!categoriesOpen),
          badge: `${OMO_BUILTIN_CATEGORIES.length + customCategories.length}`,
          action: renderCustomAddButton(() => addCustomModel("category")),
          children: (
            <>
              {OMO_BUILTIN_CATEGORIES.map(renderCategoryRow)}
              {customCategories.length > 0 && (
                <>
                  {renderCustomDivider(
                    t("omo.customCategories", {
                      defaultValue: "Custom Categories",
                    }),
                  )}
                  {customCategories.map((c, i) =>
                    renderCustomModelRow("category", c, i),
                  )}
                </>
              )}
            </>
          ),
        })}

      {renderModelSection({
        title: t("omo.otherFieldsJson", {
          defaultValue: "Other Fields (JSON)",
        }),
        isOpen: otherFieldsOpen,
        onToggle: () => setOtherFieldsOpen(!otherFieldsOpen),
        badge:
          !otherFieldsOpen && otherFieldsStr.trim() ? (
            <Badge
              variant="secondary"
              className="text-[10px] h-5 font-mono max-w-[200px] truncate"
            >
              {otherFieldsStr.trim().slice(0, 40)}
              {otherFieldsStr.trim().length > 40 ? "..." : ""}
            </Badge>
          ) : undefined,
        maxHeightClass: "max-h-[500px]",
        children: (
          <Textarea
            value={otherFieldsStr}
            onChange={(e) => onOtherFieldsStrChange(e.target.value)}
            placeholder='{ "custom_key": "value" }'
            className="font-mono text-xs min-h-[60px]"
          />
        ),
      })}
    </div>
  );
}
