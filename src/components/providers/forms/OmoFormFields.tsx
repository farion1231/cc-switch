import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
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

const BUILTIN_AGENT_KEYS = new Set(OMO_BUILTIN_AGENTS.map((a) => a.key));
const BUILTIN_CATEGORY_KEYS = new Set(OMO_BUILTIN_CATEGORIES.map((c) => c.key));

function getAdvancedStr(config: Record<string, unknown> | undefined): string {
  if (!config) return "";
  const adv: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(config)) {
    if (k !== "model") adv[k] = v;
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
  agents,
  onAgentsChange,
  categories,
  onCategoriesChange,
  otherFieldsStr,
  onOtherFieldsStrChange,
}: OmoFormFieldsProps) {
  const { i18n } = useTranslation();
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

  const handleModelChange = (
    key: string,
    model: string,
    store: Record<string, Record<string, unknown>>,
    setter: (v: Record<string, Record<string, unknown>>) => void,
  ) => {
    if (model.trim()) {
      setter({ ...store, [key]: { ...store[key], model } });
    } else {
      const existing = store[key];
      if (existing) {
        const adv = { ...existing };
        delete adv.model;
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

  const handleAdvancedChange = (
    key: string,
    rawJson: string,
    store: Record<string, Record<string, unknown>>,
    setter: (v: Record<string, Record<string, unknown>>) => void,
  ): boolean => {
    const currentModel = (store[key]?.model as string) || "";
    if (!rawJson.trim()) {
      if (currentModel) {
        setter({ ...store, [key]: { model: currentModel } });
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
        setter({
          ...store,
          [key]: {
            ...(currentModel ? { model: currentModel } : {}),
            ...parsed,
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
              isZh ? "高级参数 JSON 无效" : "Advanced JSON is invalid",
            );
          }
        }}
        placeholder={ADVANCED_PLACEHOLDER}
        className="font-mono text-xs min-h-[80px]"
      />
      {showHint && (
        <p className="text-[10px] text-muted-foreground mt-1">
          {isZh
            ? "temperature, top_p, budgetTokens, prompt_append, permission 等，留空使用默认值"
            : "temperature, top_p, budgetTokens, prompt_append, permission, etc. Leave empty for defaults"}
        </p>
      )}
    </div>
  );

  const handleFillAllRecommended = () => {
    const updatedAgents = { ...agents };
    for (const agentDef of OMO_BUILTIN_AGENTS) {
      if (agentDef.recommended && !updatedAgents[agentDef.key]?.model) {
        updatedAgents[agentDef.key] = {
          ...updatedAgents[agentDef.key],
          model: agentDef.recommended,
        };
      }
    }
    onAgentsChange(updatedAgents);

    const updatedCategories = { ...categories };
    for (const catDef of OMO_BUILTIN_CATEGORIES) {
      if (catDef.recommended && !updatedCategories[catDef.key]?.model) {
        updatedCategories[catDef.key] = {
          ...updatedCategories[catDef.key],
          model: catDef.recommended,
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
        isZh
          ? "已从本地文件导入并覆盖 Agent/Category/Other Fields"
          : "Imported local file and replaced Agents/Categories/Other Fields",
      );
    } catch (err) {
      toast.error(
        isZh
          ? `读取本地文件失败: ${String(err)}`
          : `Failed to read local file: ${String(err)}`,
      );
    }
  }, [
    readLocalFile,
    onAgentsChange,
    onCategoriesChange,
    onOtherFieldsStrChange,
    isZh,
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
          <Input
            value={currentModel}
            onChange={(e) =>
              handleModelChange(key, e.target.value, store, setter)
            }
            placeholder={def.recommended || "model-name"}
            className="flex-1 h-8 text-sm"
          />
          <Button
            type="button"
            variant={isExpanded ? "secondary" : "ghost"}
            size="icon"
            className={cn("h-7 w-7 shrink-0", advStr && "text-primary")}
            onClick={() => toggleAdvancedEditor(scope, key, advStr, isExpanded)}
            title={isZh ? "高级参数" : "Advanced"}
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
      ? isZh
        ? "agent 键名"
        : "agent key"
      : isZh
        ? "分类键名"
        : "category key";

    const key = item.key || `${emptyKeyPrefix}${index}`;
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
          <Input
            value={item.model}
            onChange={(e) => updateCustom({ model: e.target.value })}
            placeholder="model-name"
            className="flex-1 h-8 text-sm"
          />
          <Button
            type="button"
            variant={isExpanded ? "secondary" : "ghost"}
            size="icon"
            className={cn("h-7 w-7 shrink-0", advStr && "text-primary")}
            onClick={() => toggleAdvancedEditor(scope, key, advStr, isExpanded)}
            title={isZh ? "高级参数" : "Advanced"}
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
      {isZh ? "自定义" : "Custom"}
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
          {isZh ? "模型配置" : "Model Configuration"}
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
            {isZh ? "从本地导入" : "Import Local"}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={handleFillAllRecommended}
          >
            <Wand2 className="h-3.5 w-3.5 mr-1" />
            {isZh ? "填充推荐" : "Fill Recommended"}
          </Button>
        </div>
      </div>

      <div className="text-xs text-muted-foreground">
        {isZh
          ? `已配置 ${configuredAgentCount} 个 Agent，${configuredCategoryCount} 个 Category · 点击 ⚙ 展开高级参数`
          : `${configuredAgentCount} agents, ${configuredCategoryCount} categories configured · Click ⚙ for advanced params`}
        {localFilePath && (
          <span className="ml-1 text-primary/70">
            · {isZh ? "来源:" : "from:"}{" "}
            <span className="font-mono text-[10px]">
              {localFilePath.replace(/^.*\//, "")}
            </span>
          </span>
        )}
      </div>

      {renderModelSection({
        title: isZh ? "主 Agents" : "Main Agents",
        isOpen: mainAgentsOpen,
        onToggle: () => setMainAgentsOpen(!mainAgentsOpen),
        badge: `${mainAgents.length}`,
        children: mainAgents.map(renderAgentRow),
      })}

      {renderModelSection({
        title: isZh ? "子 Agents" : "Sub Agents",
        isOpen: subAgentsOpen,
        onToggle: () => setSubAgentsOpen(!subAgentsOpen),
        badge: `${subAgents.length + customAgents.length}`,
        action: renderCustomAddButton(() => addCustomModel("agent")),
        children: (
          <>
            {subAgents.map(renderAgentRow)}
            {customAgents.length > 0 && (
              <>
                {renderCustomDivider(isZh ? "自定义 Agents" : "Custom Agents")}
                {customAgents.map((a, i) =>
                  renderCustomModelRow("agent", a, i),
                )}
              </>
            )}
          </>
        ),
      })}

      {renderModelSection({
        title: isZh ? "分类 (Categories)" : "Categories",
        isOpen: categoriesOpen,
        onToggle: () => setCategoriesOpen(!categoriesOpen),
        badge: `${OMO_BUILTIN_CATEGORIES.length + customCategories.length}`,
        action: renderCustomAddButton(() => addCustomModel("category")),
        children: (
          <>
            {OMO_BUILTIN_CATEGORIES.map(renderCategoryRow)}
            {customCategories.length > 0 && (
              <>
                {renderCustomDivider(isZh ? "自定义分类" : "Custom Categories")}
                {customCategories.map((c, i) =>
                  renderCustomModelRow("category", c, i),
                )}
              </>
            )}
          </>
        ),
      })}

      {renderModelSection({
        title: isZh ? "其他字段 (JSON)" : "Other Fields (JSON)",
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
