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
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Wand2,
  Settings,
  FolderInput,
  Loader2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import { useReadOmoLocalFile } from "@/lib/query/omo";
import {
  OMO_BUILTIN_AGENTS,
  OMO_BUILTIN_CATEGORIES,
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
  agents: Record<string, Record<string, unknown>>;
  onAgentsChange: (agents: Record<string, Record<string, unknown>>) => void;
  categories: Record<string, Record<string, unknown>>;
  onCategoriesChange: (
    categories: Record<string, Record<string, unknown>>,
  ) => void;
  otherFieldsStr: string;
  onOtherFieldsStrChange: (value: string) => void;
}

type CustomModelItem = { key: string; model: string };
type BuiltinModelDef = Pick<
  OmoAgentDef | OmoCategoryDef,
  "key" | "display" | "descZh" | "descEn" | "recommended"
>;
type ModelOption = { value: string; label: string };

const BUILTIN_AGENT_KEYS = new Set(OMO_BUILTIN_AGENTS.map((a) => a.key));
const BUILTIN_CATEGORY_KEYS = new Set(OMO_BUILTIN_CATEGORIES.map((c) => c.key));
const EMPTY_MODEL_VALUE = "__cc_switch_omo_model_empty__";
const UNAVAILABLE_MODEL_VALUE = "__cc_switch_omo_model_unavailable__";
const EMPTY_VARIANT_VALUE = "__cc_switch_omo_variant_empty__";
const UNAVAILABLE_VARIANT_VALUE = "__cc_switch_omo_variant_unavailable__";

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
      });
    }
  }
  return customs;
}

function mergeCustomModelsIntoStore(
  store: Record<string, Record<string, unknown>>,
  builtinKeys: Set<string>,
  customs: CustomModelItem[],
): Record<string, Record<string, unknown>> {
  const updated = { ...store };
  for (const key of Object.keys(updated)) {
    if (!builtinKeys.has(key)) delete updated[key];
  }
  for (const custom of customs) {
    if (custom.key.trim()) {
      updated[custom.key] = { ...updated[custom.key], model: custom.model };
    }
  }
  return updated;
}

export function OmoFormFields({
  modelOptions,
  modelVariantsMap = {},
  agents,
  onAgentsChange,
  categories,
  onCategoriesChange,
  otherFieldsStr,
  onOtherFieldsStrChange,
}: OmoFormFieldsProps) {
  const { t, i18n } = useTranslation();
  const isZh = i18n.language?.startsWith("zh");

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
    collectCustomModels(agents, BUILTIN_AGENT_KEYS),
  );

  const [customCategories, setCustomCategories] = useState<CustomModelItem[]>(
    () => collectCustomModels(categories, BUILTIN_CATEGORY_KEYS),
  );

  useEffect(() => {
    setCustomAgents(collectCustomModels(agents, BUILTIN_AGENT_KEYS));
  }, [agents]);

  useEffect(() => {
    setCustomCategories(collectCustomModels(categories, BUILTIN_CATEGORY_KEYS));
  }, [categories]);

  const syncCustomAgents = useCallback(
    (customs: CustomModelItem[]) => {
      onAgentsChange(
        mergeCustomModelsIntoStore(agents, BUILTIN_AGENT_KEYS, customs),
      );
    },
    [agents, onAgentsChange],
  );

  const syncCustomCategories = useCallback(
    (customs: CustomModelItem[]) => {
      onCategoriesChange(
        mergeCustomModelsIntoStore(categories, BUILTIN_CATEGORY_KEYS, customs),
      );
    },
    [categories, onCategoriesChange],
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
    placeholder?: string,
  ) => {
    const options = buildEffectiveModelOptions(currentModel);
    return (
      <Select
        value={currentModel || EMPTY_MODEL_VALUE}
        onValueChange={(value) =>
          onChange(value === EMPTY_MODEL_VALUE ? "" : value)
        }
      >
        <SelectTrigger className="flex-1 h-8 text-sm">
          <SelectValue
            placeholder={
              placeholder ||
              t("omo.selectEnabledModel", {
                defaultValue: "Select enabled model",
              })
            }
          />
        </SelectTrigger>
        <SelectContent className="max-h-72">
          <SelectItem value={EMPTY_MODEL_VALUE}>
            {t("omo.clearWrapped", { defaultValue: "(Clear)" })}
          </SelectItem>
          {options.length === 0 ? (
            <SelectItem value={UNAVAILABLE_MODEL_VALUE} disabled>
              {t("omo.noEnabledModels", { defaultValue: "No enabled models" })}
            </SelectItem>
          ) : (
            options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))
          )}
        </SelectContent>
      </Select>
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
    const variantOptions = buildEffectiveVariantOptions(
      currentModel,
      currentVariant,
    );
    const hasModel = Boolean(currentModel);
    const firstIsUnavailable =
      Boolean(currentVariant) &&
      !(modelVariantsMap[currentModel] || []).includes(currentVariant);

    return (
      <Select
        value={currentVariant || EMPTY_VARIANT_VALUE}
        onValueChange={(value) =>
          onChange(value === EMPTY_VARIANT_VALUE ? "" : value)
        }
        disabled={!hasModel}
      >
        <SelectTrigger className="w-32 h-8 text-xs shrink-0">
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
          {!hasModel ? (
            <SelectItem value={UNAVAILABLE_VARIANT_VALUE} disabled>
              {t("omo.selectModelFirst", {
                defaultValue: "Select model first",
              })}
            </SelectItem>
          ) : variantOptions.length === 0 ? (
            <SelectItem value={UNAVAILABLE_VARIANT_VALUE} disabled>
              {t("omo.noVariantsForModel", {
                defaultValue: "No variants for model",
              })}
            </SelectItem>
          ) : (
            variantOptions.map((variant, index) => (
              <SelectItem key={`${variant}-${index}`} value={variant}>
                {firstIsUnavailable && index === 0
                  ? t("omo.currentValueUnavailable", {
                      value: variant,
                      defaultValue: "{{value}} (current value, unavailable)",
                    })
                  : variant}
              </SelectItem>
            ))
          )}
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
            "No enabled models available. Configure and enable OpenCode models first.",
        }),
      );
      return;
    }

    const updatedAgents = { ...agents };
    for (const agentDef of OMO_BUILTIN_AGENTS) {
      const recommendedValue = resolveRecommendedModel(agentDef.recommended);
      if (recommendedValue && !updatedAgents[agentDef.key]?.model) {
        updatedAgents[agentDef.key] = {
          ...updatedAgents[agentDef.key],
          model: recommendedValue,
        };
      }
    }
    onAgentsChange(updatedAgents);

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
  };

  const configuredAgentCount = Object.keys(agents).length;
  const configuredCategoryCount = Object.keys(categories).length;
  const mainAgents = OMO_BUILTIN_AGENTS.filter((a) => a.group === "main");
  const subAgents = OMO_BUILTIN_AGENTS.filter((a) => a.group === "sub");

  const readLocalFile = useReadOmoLocalFile();
  const [localFilePath, setLocalFilePath] = useState<string | null>(null);

  const handleImportFromLocal = useCallback(async () => {
    try {
      const data = await readLocalFile.mutateAsync();
      const importedAgents =
        (data.agents as Record<string, Record<string, unknown>> | undefined) ||
        {};
      const importedCategories =
        (data.categories as
          | Record<string, Record<string, unknown>>
          | undefined) || {};

      onAgentsChange(importedAgents);
      onCategoriesChange(importedCategories);
      onOtherFieldsStrChange(
        data.otherFields ? JSON.stringify(data.otherFields, null, 2) : "",
      );
      setAgentAdvancedDrafts({});
      setCategoryAdvancedDrafts({});
      setCustomAgents(collectCustomModels(importedAgents, BUILTIN_AGENT_KEYS));
      setCustomCategories(
        collectCustomModels(importedCategories, BUILTIN_CATEGORY_KEYS),
      );
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
    const setter = isAgent ? onAgentsChange : onCategoriesChange;
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
            <div className="text-sm font-medium">{def.display}</div>
            <div className="text-xs text-muted-foreground truncate">
              {isZh ? def.descZh : def.descEn}
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
    const setter = isAgent ? onAgentsChange : onCategoriesChange;
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
          <Input
            value={item.key}
            onChange={(e) => updateCustom({ key: e.target.value })}
            placeholder={keyPlaceholder}
            className="w-32 shrink-0 h-8 text-sm text-primary"
          />
          {renderModelSelect(
            item.model,
            (value) => updateCustom({ model: value }),
            t("omo.modelNamePlaceholder", { defaultValue: "model-name" }),
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
      setCustomAgents((prev) => [...prev, { key: "", model: "" }]);
      setSubAgentsOpen(true);
      return;
    }
    setCustomCategories((prev) => [...prev, { key: "", model: "" }]);
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
            defaultValue: "{{count}} enabled models available",
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

      {renderModelSection({
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
