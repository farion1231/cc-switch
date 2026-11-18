import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowLeft,
  Check,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Eye,
  EyeOff,
  Loader2,
  Plus,
  RefreshCw,
  Search,
  Star,
  X,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ProviderIcon } from "@/components/ProviderIcon";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { useVisibleAppIds } from "@/components/mcp/useVisibleAppIds";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "@/components/providers/forms/FetchedModelPicker";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type {
  ClaudeApiKeyField,
  UniversalProvider,
  UniversalProviderApps,
} from "@/types";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  APP_FLAG_KEY,
  buildVisibleUniversalApps,
  COMPATIBLE_APP_IDS,
} from "./aggregationApps";
import {
  type AggregationPresetEntry,
  type AggregationPresetGroupKind,
  type AggregationPresetVariant,
  buildUniversalFromAggregationInput,
  extractHost,
  findPresetVariantForUniversal,
  getAggregationPresetCatalog,
  getPresetVariantLabel,
} from "./aggregationPresets";

const API_FORMATS = [
  { value: "openai_chat", label: "OpenAI Chat Completions" },
  { value: "anthropic", label: "Anthropic Messages" },
  { value: "openai_responses", label: "OpenAI Responses" },
  { value: "gemini_native", label: "Google Gemini Native" },
] as const;

const CODEX_REASONING_EFFORTS = [
  { value: "minimal", label: "minimal" },
  { value: "low", label: "low" },
  { value: "medium", label: "medium" },
  { value: "high", label: "high" },
  { value: "xhigh", label: "xhigh" },
] as const;

function parseModelList(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const part of text.split(/[,\n]+/)) {
    const trimmed = part.trim();
    if (trimmed && !seen.has(trimmed)) {
      seen.add(trimmed);
      out.push(trimmed);
    }
  }
  return out;
}

const GROUP_ORDER: readonly AggregationPresetGroupKind[] = [
  "gateway",
  "vendor",
  "relay",
  "local",
];

interface AggregationProviderModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaveAndSync: (provider: UniversalProvider) => Promise<void>;
  editingProvider?: UniversalProvider | null;
  initialPreset?: AggregationPresetEntry | null;
}

export function AggregationProviderModal({
  isOpen,
  onClose,
  onSaveAndSync,
  editingProvider,
  initialPreset,
}: AggregationProviderModalProps) {
  const { t } = useTranslation();
  const isEditMode = Boolean(editingProvider);
  const visibleApps = useVisibleAppIds(COMPATIBLE_APP_IDS);
  const catalog = useMemo(() => getAggregationPresetCatalog(), []);

  const [step, setStep] = useState<"picker" | "form">("picker");
  const [presetQuery, setPresetQuery] = useState("");
  const [groupFilter, setGroupFilter] = useState<
    "all" | AggregationPresetGroupKind
  >("all");
  const [selectedEntry, setSelectedEntry] =
    useState<AggregationPresetEntry | null>(null);
  const [selectedVariant, setSelectedVariant] =
    useState<AggregationPresetVariant | null>(null);

  const [name, setName] = useState("");
  const [apiFormat, setApiFormat] = useState<string>("openai_chat");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [primaryModel, setPrimaryModel] = useState("");
  const [extraModelsText, setExtraModelsText] = useState("");
  const [manualModelInput, setManualModelInput] = useState("");
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const fetchGenRef = useRef(0);

  const [reasoningEffort, setReasoningEffort] = useState<string>("high");
  const [haikuModel, setHaikuModel] = useState("");
  const [sonnetModel, setSonnetModel] = useState("");
  const [opusModel, setOpusModel] = useState("");
  const [isFullUrl, setIsFullUrl] = useState(false);
  const [apiKeyField, setApiKeyField] = useState<ClaudeApiKeyField>(
    "ANTHROPIC_AUTH_TOKEN",
  );
  const [customUserAgent, setCustomUserAgent] = useState("");
  const [customModelsUrl, setCustomModelsUrl] = useState("");

  const [websiteUrl, setWebsiteUrl] = useState("");
  const [notes, setNotes] = useState("");
  const [apps, setApps] = useState<UniversalProviderApps>(() =>
    buildVisibleUniversalApps(visibleApps),
  );
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [saving, setSaving] = useState(false);

  const applyVariant = useCallback(
    (entry: AggregationPresetEntry, variant: AggregationPresetVariant) => {
      fetchGenRef.current += 1;
      setFetchedModels([]);
      setIsFetchingModels(false);
      setManualModelInput("");
      setSelectedEntry(entry);
      setSelectedVariant(variant);
      setName(variant.isCustomTemplate ? "" : variant.name);
      setApiFormat(variant.apiFormat);
      setBaseUrl(variant.baseUrl);
      setPrimaryModel(variant.defaultModel);
      setExtraModelsText(variant.extraModels.join(", "));
      setReasoningEffort("high");
      setHaikuModel("");
      setSonnetModel("");
      setOpusModel("");
      setIsFullUrl(false);
      setApiKeyField("ANTHROPIC_AUTH_TOKEN");
      setCustomUserAgent("");
      setCustomModelsUrl("");
      setWebsiteUrl(variant.websiteUrl || "");
      setStep("form");
    },
    [],
  );

  const openCustomForm = useCallback(() => {
    const customEntry =
      catalog.find((e) =>
        e.variants.some((v) => v.providerType === "custom_gateway"),
      ) ?? catalog[0];
    const variant =
      customEntry?.variants.find((v) => v.providerType === "custom_gateway") ??
      customEntry?.variants[0] ??
      null;
    fetchGenRef.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
    setManualModelInput("");
    setSelectedEntry(customEntry ?? null);
    setSelectedVariant(variant);
    setName("");
    setApiFormat("openai_chat");
    setBaseUrl("");
    setApiKey("");
    setPrimaryModel("gpt-4o");
    setExtraModelsText("");
    setReasoningEffort("high");
    setHaikuModel("");
    setSonnetModel("");
    setOpusModel("");
    setIsFullUrl(false);
    setApiKeyField("ANTHROPIC_AUTH_TOKEN");
    setCustomUserAgent("");
    setCustomModelsUrl("");
    setWebsiteUrl("");
    setNotes("");
    setApps(buildVisibleUniversalApps(visibleApps));
    setAdvancedOpen(false);
    setStep("form");
  }, [catalog, visibleApps]);

  useLayoutEffect(() => {
    if (!isOpen) return;
    fetchGenRef.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
    setManualModelInput("");
    setPresetQuery("");
    setGroupFilter("all");
    setShowApiKey(false);
    setAdvancedOpen(false);

    if (editingProvider) {
      const matched = findPresetVariantForUniversal(editingProvider);
      setSelectedEntry(matched?.entry ?? null);
      setSelectedVariant(matched?.variant ?? null);
      setName(editingProvider.name);
      setApiFormat(editingProvider.apiFormat || "openai_chat");
      setBaseUrl(editingProvider.baseUrl);
      setApiKey(editingProvider.apiKey);
      const model =
        editingProvider.models?.claude?.model ||
        editingProvider.models?.codex?.model ||
        editingProvider.models?.gemini?.model ||
        "";
      setPrimaryModel(model);
      setExtraModelsText((editingProvider.extraModels ?? []).join(", "));
      setReasoningEffort(
        editingProvider.models?.codex?.reasoningEffort || "high",
      );
      const claudeCfg = editingProvider.models?.claude;
      setHaikuModel(
        claudeCfg?.haikuModel && claudeCfg.haikuModel !== model
          ? claudeCfg.haikuModel
          : "",
      );
      setSonnetModel(
        claudeCfg?.sonnetModel && claudeCfg.sonnetModel !== model
          ? claudeCfg.sonnetModel
          : "",
      );
      setOpusModel(
        claudeCfg?.opusModel && claudeCfg.opusModel !== model
          ? claudeCfg.opusModel
          : "",
      );
      setIsFullUrl(Boolean(editingProvider.meta?.isFullUrl));
      setApiKeyField(
        editingProvider.meta?.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN",
      );
      setCustomUserAgent(editingProvider.meta?.customUserAgent || "");
      setCustomModelsUrl("");
      setWebsiteUrl(editingProvider.websiteUrl || "");
      setNotes(editingProvider.notes || "");
      setApps(buildVisibleUniversalApps(visibleApps, editingProvider.apps));
      setStep("form");
    } else if (initialPreset) {
      const variant = initialPreset.variants[0];
      setApiKey("");
      setNotes("");
      setApps(buildVisibleUniversalApps(visibleApps));
      applyVariant(initialPreset, variant);
    } else {
      setSelectedEntry(null);
      setSelectedVariant(null);
      setApiKey("");
      setNotes("");
      setReasoningEffort("high");
      setHaikuModel("");
      setSonnetModel("");
      setOpusModel("");
      setIsFullUrl(false);
      setApiKeyField("ANTHROPIC_AUTH_TOKEN");
      setCustomUserAgent("");
      setCustomModelsUrl("");
      setApps(buildVisibleUniversalApps(visibleApps));
      setStep("picker");
    }
  }, [editingProvider, initialPreset, isOpen, visibleApps, applyVariant]);

  const toggleApp = useCallback((appId: AppId) => {
    const flag = APP_FLAG_KEY[appId];
    if (!flag) return;
    setApps((prev) => ({
      ...prev,
      [flag]: !prev[flag],
    }));
  }, []);

  const enabledCount = visibleApps.filter((appId) => {
    const flag = APP_FLAG_KEY[appId];
    return flag ? Boolean(apps[flag]) : false;
  }).length;

  const allSelected =
    visibleApps.length > 0 && enabledCount === visibleApps.length;

  const handleToggleAllApps = () => {
    if (allSelected) {
      setApps({ claude: false, codex: false, gemini: false });
    } else {
      setApps(buildVisibleUniversalApps(visibleApps));
    }
  };

  const requiresUrlInput =
    isEditMode ||
    !selectedVariant ||
    Boolean(selectedVariant.isCustomTemplate) ||
    !selectedVariant.baseUrl;
  const keyOptional = Boolean(selectedVariant?.keyOptional);
  const canSubmit =
    !saving &&
    Boolean(name.trim()) &&
    Boolean(baseUrl.trim()) &&
    (keyOptional || Boolean(apiKey.trim()));

  const extraModelsList = useMemo(
    () => parseModelList(extraModelsText),
    [extraModelsText],
  );
  const configuredModels = useMemo(() => {
    const out: string[] = [];
    const primary = primaryModel.trim();
    if (primary) out.push(primary);
    for (const m of extraModelsList) {
      if (!out.includes(m)) out.push(m);
    }
    return out;
  }, [primaryModel, extraModelsList]);

  const applyOrderedModels = useCallback((ordered: string[]) => {
    const nextPrimary = ordered[0] ?? "";
    const nextExtra = ordered.slice(1);
    setPrimaryModel(nextPrimary);
    setExtraModelsText(nextExtra.join(", "));
  }, []);

  const handleApplyFetchedModels = useCallback(
    (selection: FetchedModelSelection) => {
      const removeSet = new Set(selection.remove);
      const next = configuredModels.filter((m) => !removeSet.has(m));
      for (const addId of selection.add) {
        const trimmed = addId.trim();
        if (trimmed && !next.includes(trimmed)) {
          next.push(trimmed);
        }
      }
      applyOrderedModels(next);
    },
    [configuredModels, applyOrderedModels],
  );

  const handleAddManualModels = useCallback(() => {
    const added = parseModelList(manualModelInput);
    if (added.length === 0) return;
    const next = [...configuredModels];
    for (const m of added) {
      if (!next.includes(m)) next.push(m);
    }
    applyOrderedModels(next);
    setManualModelInput("");
  }, [manualModelInput, configuredModels, applyOrderedModels]);

  const handleSetDefaultModel = useCallback(
    (modelId: string) => {
      const next = [modelId, ...configuredModels.filter((m) => m !== modelId)];
      applyOrderedModels(next);
    },
    [configuredModels, applyOrderedModels],
  );

  const handleRemoveModel = useCallback(
    (modelId: string) => {
      const next = configuredModels.filter((m) => m !== modelId);
      applyOrderedModels(next);
    },
    [configuredModels, applyOrderedModels],
  );

  const handleFetchModels = useCallback(() => {
    const trimmedUrl = baseUrl.trim();
    const effectiveKey = apiKey.trim() || (keyOptional ? "local-no-key" : "");
    if (!trimmedUrl || !effectiveKey) {
      showFetchModelsError(null, t, {
        hasBaseUrl: Boolean(trimmedUrl),
        hasApiKey: Boolean(effectiveKey),
      });
      return;
    }

    const gen = ++fetchGenRef.current;
    setIsFetchingModels(true);
    const fetchApiFormat =
      apiFormat === "anthropic" ||
      apiFormat === "openai_chat" ||
      apiFormat === "openai_responses"
        ? apiFormat
        : undefined;

    fetchModelsForConfig(
      trimmedUrl,
      effectiveKey,
      isFullUrl || undefined,
      customModelsUrl.trim() || undefined,
      customUserAgent.trim() || undefined,
      fetchApiFormat ? { apiFormat: fetchApiFormat } : undefined,
    )
      .then((models) => {
        if (gen !== fetchGenRef.current) return;
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", {
              count: models.length,
              defaultValue: `已获取 ${models.length} 个模型`,
            }),
          );
        }
      })
      .catch((err) => {
        if (gen !== fetchGenRef.current) return;
        showFetchModelsError(err, t, {
          hasBaseUrl: Boolean(trimmedUrl),
          hasApiKey: Boolean(effectiveKey),
        });
      })
      .finally(() => {
        if (gen === fetchGenRef.current) {
          setIsFetchingModels(false);
        }
      });
  }, [
    baseUrl,
    apiKey,
    keyOptional,
    apiFormat,
    isFullUrl,
    customModelsUrl,
    customUserAgent,
    t,
  ]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;

    const nextMeta = {
      ...(editingProvider?.meta ?? {}),
      isFullUrl: isFullUrl || undefined,
      apiKeyField:
        apiKeyField !== "ANTHROPIC_AUTH_TOKEN" ? apiKeyField : undefined,
      customUserAgent: customUserAgent.trim() || undefined,
    };
    const hasMeta = Object.values(nextMeta).some((v) => v !== undefined);

    const provider = buildUniversalFromAggregationInput({
      name,
      providerType:
        editingProvider?.providerType ??
        selectedVariant?.providerType ??
        "custom_gateway",
      baseUrl,
      apiKey,
      apiFormat,
      primaryModel,
      extraModels: parseModelList(extraModelsText),
      apps: buildVisibleUniversalApps(visibleApps, apps),
      reasoningEffort,
      haikuModel,
      sonnetModel,
      opusModel,
      meta: hasMeta ? nextMeta : undefined,
      websiteUrl,
      notes,
      icon: editingProvider?.icon ?? selectedVariant?.icon,
      iconColor: editingProvider?.iconColor ?? selectedVariant?.iconColor,
      existing: editingProvider,
    });

    setSaving(true);
    try {
      await onSaveAndSync(provider);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const filteredByGroup = useMemo(() => {
    const q = presetQuery.trim().toLowerCase();
    const result = new Map<
      AggregationPresetGroupKind,
      AggregationPresetEntry[]
    >();
    for (const kind of GROUP_ORDER) {
      if (groupFilter !== "all" && groupFilter !== kind) continue;
      const items = catalog.filter((entry) => {
        if (entry.group !== kind) return false;
        if (!q) return true;
        return (
          entry.name.toLowerCase().includes(q) || entry.searchText.includes(q)
        );
      });
      if (items.length > 0) {
        result.set(kind, items);
      }
    }
    return result;
  }, [catalog, groupFilter, presetQuery]);

  const groupLabels: Record<AggregationPresetGroupKind, string> = {
    gateway: t("aggregation.presetGroup.gateway", {
      defaultValue: "网关与自定义",
    }),
    vendor: t("aggregation.presetGroup.vendor", {
      defaultValue: "模型厂商与官方套餐",
    }),
    relay: t("aggregation.presetGroup.relay", {
      defaultValue: "聚合与中转渠道",
    }),
    local: t("aggregation.presetGroup.local", {
      defaultValue: "本机服务（免密）",
    }),
  };

  const openExternal = (url?: string) => {
    if (!url) return;
    window.open(url, "_blank", "noopener,noreferrer");
  };

  const renderModelsSection = () => (
    <div className="space-y-3 rounded-panel border border-border bg-surface p-3.5">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Label className="text-body font-medium">
            {t("aggregation.modal.modelsTitle", {
              defaultValue: "模型列表",
            })}
          </Label>
          <span className="rounded bg-subtle px-1.5 py-0.5 text-[11px] text-fg-3">
            {configuredModels.length}
          </span>
        </div>
        <FetchedModelPicker
          models={fetchedModels}
          configuredModelIds={configuredModels}
          onApply={handleApplyFetchedModels}
          onFetch={handleFetchModels}
          isFetching={isFetchingModels}
          metadataSource={{ baseUrl }}
        />
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="agg-provider-model">
            {t("aggregation.modal.primaryModel", {
              defaultValue: "默认模型（所有应用均可用）",
            })}
          </Label>
          <Input
            id="agg-provider-model"
            value={primaryModel}
            onChange={(e) => setPrimaryModel(e.target.value)}
            placeholder="gpt-4o / claude-sonnet-4-20250514"
          />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="agg-provider-add-models">
            {t("aggregation.modal.addModelLabel", {
              defaultValue: "手动添加模型",
            })}
          </Label>
          <div className="flex gap-1.5">
            <Input
              id="agg-provider-add-models"
              value={manualModelInput}
              onChange={(e) => setManualModelInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  handleAddManualModels();
                }
              }}
              placeholder={t("aggregation.modal.addModelPlaceholder", {
                defaultValue: "deepseek-chat, qwen3-coder",
              })}
            />
            <Button
              type="button"
              variant="outline"
              size="compact"
              className="h-9 shrink-0 px-2.5"
              onClick={handleAddManualModels}
              disabled={!manualModelInput.trim()}
            >
              <Plus className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </div>

      {configuredModels.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {configuredModels.map((modelId, idx) => {
            const isPrimary = idx === 0 && modelId === primaryModel.trim();
            return (
              <span
                key={modelId}
                className={cn(
                  "inline-flex items-center gap-1 rounded-control border px-2 py-0.5 font-mono text-[11px]",
                  isPrimary
                    ? "border-action/40 bg-action-soft text-action-text font-medium"
                    : "border-border bg-app text-fg-2",
                )}
              >
                {isPrimary && (
                  <Star className="h-2.5 w-2.5 fill-current shrink-0" />
                )}
                <span>{modelId}</span>
                {!isPrimary && (
                  <button
                    type="button"
                    onClick={() => handleSetDefaultModel(modelId)}
                    title={t("aggregation.modal.setDefaultModel", {
                      defaultValue: "设为默认模型",
                    })}
                    className="ms-0.5 rounded px-1 text-[10px] text-fg-3 hover:bg-subtle hover:text-fg-1"
                  >
                    {t("aggregation.modal.defaultShort", {
                      defaultValue: "默认",
                    })}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => handleRemoveModel(modelId)}
                  aria-label={t("common.delete")}
                  className="rounded p-0.5 text-fg-3 hover:bg-subtle hover:text-danger-text"
                >
                  <X className="h-2.5 w-2.5" />
                </button>
              </span>
            );
          })}
        </div>
      )}
    </div>
  );

  return (
    <FullScreenPanel
      isOpen={isOpen}
      onClose={onClose}
      title={
        isEditMode
          ? t("aggregation.modal.editTitle", { defaultValue: "编辑供应商" })
          : t("aggregation.modal.addTitle", {
              defaultValue: "添加跨应用供应商",
            })
      }
      footer={
        step === "picker" ? (
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
        ) : (
          <>
            {!isEditMode && (
              <Button
                type="button"
                variant="quiet"
                onClick={() => setStep("picker")}
              >
                <ArrowLeft className="h-4 w-4" />
                {t("aggregation.modal.changePreset", {
                  defaultValue: "重选渠道",
                })}
              </Button>
            )}
            <div className="flex-1" />
            <Button type="button" variant="outline" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              form="aggregation-provider-form"
              disabled={!canSubmit}
            >
              {saving ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <RefreshCw className="h-4 w-4" />
              )}
              {t("aggregation.modal.saveAndSync", {
                defaultValue: "保存并兼容全部",
              })}
            </Button>
          </>
        )
      }
    >
      {step === "picker" ? (
        <div className="mx-auto max-w-4xl space-y-5">
          {/* 搜索栏 + 快速自定义 */}
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-[220px] flex-1">
              <Search className="pointer-events-none absolute start-3 top-1/2 h-4 w-4 -translate-y-1/2 text-fg-3" />
              <Input
                autoFocus
                value={presetQuery}
                onChange={(e) => setPresetQuery(e.target.value)}
                placeholder={t("aggregation.modal.searchPresetPlaceholder", {
                  defaultValue:
                    "搜索 DeepSeek / Kimi / 智谱 / OpenRouter / 域名...",
                })}
                className="ps-9"
              />
            </div>
            <Button type="button" variant="outline" onClick={openCustomForm}>
              <Plus className="h-4 w-4" />
              {t("aggregation.modal.customProvider", {
                defaultValue: "自定义配置",
              })}
            </Button>
          </div>

          {/* 分类标签 */}
          <div className="flex flex-wrap items-center gap-1.5">
            {(["all", ...GROUP_ORDER] as const).map((kind) => (
              <button
                key={kind}
                type="button"
                onClick={() => setGroupFilter(kind)}
                className={cn(
                  "inline-flex h-7 items-center rounded-full border px-3 text-caption transition-colors",
                  groupFilter === kind
                    ? "border-action/40 bg-action-soft font-medium text-action-text"
                    : "border-border bg-surface text-fg-2 hover:text-fg-1",
                )}
              >
                {kind === "all"
                  ? t("aggregation.presetGroup.all", {
                      defaultValue: "全部预设",
                    })
                  : groupLabels[kind]}
              </button>
            ))}
          </div>

          {/* 分组网格 */}
          {filteredByGroup.size === 0 ? (
            <div className="rounded-panel border border-dashed border-border py-12 text-center">
              <p className="text-body text-fg-2">
                {t("aggregation.modal.noPresetMatch", {
                  defaultValue: "没有找到匹配的预设供应商",
                })}
              </p>
              <Button
                type="button"
                variant="outline"
                size="compact"
                className="mt-3"
                onClick={openCustomForm}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("aggregation.modal.customProvider", {
                  defaultValue: "改为自定义配置",
                })}
              </Button>
            </div>
          ) : (
            <div className="space-y-5">
              {GROUP_ORDER.map((kind) => {
                const items = filteredByGroup.get(kind);
                if (!items?.length) return null;
                return (
                  <section key={kind} className="space-y-2.5">
                    <div className="flex items-center justify-between">
                      <h3 className="text-caption font-semibold text-fg-2">
                        {groupLabels[kind]}
                      </h3>
                      <span className="text-[11px] text-fg-3">
                        {items.length}
                      </span>
                    </div>
                    <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4">
                      {items.map((entry) => {
                        const lead = entry.variants[0];
                        const host = extractHost(lead.baseUrl);
                        const label = entry.nameKey
                          ? t(entry.nameKey, { defaultValue: entry.name })
                          : entry.name;
                        return (
                          <button
                            key={entry.id}
                            type="button"
                            onClick={() => applyVariant(entry, lead)}
                            className="flex items-center gap-2.5 rounded-control border border-border bg-surface px-3 py-2.5 text-start transition-colors hover:border-action/50 hover:bg-action-soft/40"
                          >
                            <ProviderIcon
                              icon={entry.icon}
                              name={label}
                              color={entry.iconColor}
                              size={24}
                            />
                            <div className="min-w-0 flex-1">
                              <div className="truncate text-body font-medium text-fg-1">
                                {label}
                              </div>
                              <div className="truncate text-[11px] text-fg-3">
                                {entry.variants.length > 1
                                  ? `${entry.variants.length} ${t("aggregation.modal.versionsCount", { defaultValue: "个版本" })}`
                                  : host ||
                                    t("aggregation.modal.customShort", {
                                      defaultValue: "手动填写地址",
                                    })}
                              </div>
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  </section>
                );
              })}
            </div>
          )}
        </div>
      ) : (
        <form
          id="aggregation-provider-form"
          onSubmit={(e) => void handleSubmit(e)}
          className="mx-auto max-w-2xl space-y-5"
        >
          {/* 预设信息条 + 同厂商版本切换 */}
          {selectedEntry && selectedVariant && !isEditMode && (
            <div className="rounded-panel border border-border bg-subtle/40 p-3.5 space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-2.5">
                  <ProviderIcon
                    icon={selectedVariant.icon}
                    name={selectedEntry.name}
                    color={selectedVariant.iconColor}
                    size={26}
                  />
                  <div>
                    <div className="text-body font-semibold text-fg-1">
                      {selectedEntry.nameKey
                        ? t(selectedEntry.nameKey, {
                            defaultValue: selectedEntry.name,
                          })
                        : selectedEntry.name}
                    </div>
                    {baseUrl && (
                      <div className="text-[11px] text-fg-3">
                        {extractHost(baseUrl)} ·{" "}
                        {API_FORMATS.find((f) => f.value === apiFormat)
                          ?.label ?? apiFormat}{" "}
                        · {primaryModel}
                      </div>
                    )}
                  </div>
                </div>
                {(selectedVariant.apiKeyUrl || selectedVariant.websiteUrl) && (
                  <Button
                    type="button"
                    variant="quiet"
                    size="compact"
                    onClick={() =>
                      openExternal(
                        selectedVariant.apiKeyUrl || selectedVariant.websiteUrl,
                      )
                    }
                  >
                    <ExternalLink className="h-3.5 w-3.5" />
                    {t("aggregation.modal.getApiKey", {
                      defaultValue: "获取 Key",
                    })}
                  </Button>
                )}
              </div>

              {selectedEntry.variants.length > 1 && (
                <div className="flex flex-wrap gap-1.5 border-t border-border/70 pt-2.5">
                  {selectedEntry.variants.map((variant) => {
                    const active = variant.id === selectedVariant.id;
                    return (
                      <button
                        key={variant.id}
                        type="button"
                        onClick={() => applyVariant(selectedEntry, variant)}
                        className={cn(
                          "rounded-control border px-2.5 py-1 text-caption transition-colors",
                          active
                            ? "border-action/40 bg-action-soft font-medium text-action-text"
                            : "border-border bg-surface text-fg-2 hover:text-fg-1",
                        )}
                      >
                        {getPresetVariantLabel(variant, t)}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {/* 主配置区：预设仅需填写 API Key；自定义/编辑展示 名称 + 地址 + Key */}
          {requiresUrlInput && (
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="agg-provider-name">
                  {t("provider.name", { defaultValue: "名称" })}
                </Label>
                <Input
                  id="agg-provider-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={t("aggregation.modal.namePlaceholder", {
                    defaultValue: "例如：主用中转站",
                  })}
                  required
                />
              </div>

              <div className="space-y-1.5">
                <Label htmlFor="agg-provider-url">
                  {t("universalProvider.baseUrl", { defaultValue: "API 地址" })}
                </Label>
                <Input
                  id="agg-provider-url"
                  value={baseUrl}
                  onChange={(e) => setBaseUrl(e.target.value)}
                  placeholder="https://api.example.com"
                  required
                />
              </div>
            </div>
          )}

          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <Label htmlFor="agg-provider-key">
                {t("universalProvider.apiKey", { defaultValue: "API Key" })}
              </Label>
              <span className="text-[11px] text-fg-3">
                {keyOptional
                  ? t("aggregation.modal.keyOptionalHint", {
                      defaultValue: "本机服务可留空",
                    })
                  : t("aggregation.modal.autoCompatSummary", {
                      count: enabledCount,
                      defaultValue: `保存后自动兼容 ${enabledCount} 个已启用应用`,
                    })}
              </span>
            </div>
            <div className="relative">
              <Input
                id="agg-provider-key"
                autoFocus={!requiresUrlInput}
                type={showApiKey ? "text" : "password"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder={keyOptional ? "local-no-key (optional)" : "sk-..."}
                className="pe-9"
                required={!keyOptional}
              />
              <button
                type="button"
                onClick={() => setShowApiKey((v) => !v)}
                className="absolute inset-y-0 end-2 flex items-center text-fg-3 hover:text-fg-1"
                aria-label={
                  showApiKey ? t("apiKeyInput.hide") : t("apiKeyInput.show")
                }
              >
                {showApiKey ? (
                  <EyeOff className="h-4 w-4" />
                ) : (
                  <Eye className="h-4 w-4" />
                )}
              </button>
            </div>
          </div>

          {/* 自定义/编辑模式下直接展示模型列表与「获取模型列表」 */}
          {requiresUrlInput && renderModelsSection()}

          {/* 折叠高级选项：名称/URL 覆写（预设模式下）、协议、模型、相关设置、应用分发范围、备注 */}
          <div className="rounded-panel border border-border bg-surface">
            <button
              type="button"
              onClick={() => setAdvancedOpen((v) => !v)}
              className="flex w-full items-center justify-between px-3.5 py-2.5 text-caption font-medium text-fg-2 hover:text-fg-1"
            >
              <span>
                {t("aggregation.modal.advancedToggle", {
                  defaultValue: "高级配置（协议格式 · 模型 · 分发范围）",
                })}
              </span>
              {advancedOpen ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
            </button>

            {advancedOpen && (
              <div className="space-y-4 border-t border-border p-3.5">
                {!requiresUrlInput && renderModelsSection()}

                {!requiresUrlInput && (
                  <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                    <div className="space-y-1.5">
                      <Label htmlFor="agg-provider-name-adv">
                        {t("provider.name", { defaultValue: "名称" })}
                      </Label>
                      <Input
                        id="agg-provider-name-adv"
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                      />
                    </div>
                    <div className="space-y-1.5">
                      <Label htmlFor="agg-provider-url-adv">
                        {t("universalProvider.baseUrl", {
                          defaultValue: "API 地址",
                        })}
                      </Label>
                      <Input
                        id="agg-provider-url-adv"
                        value={baseUrl}
                        onChange={(e) => setBaseUrl(e.target.value)}
                      />
                    </div>
                  </div>
                )}

                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-format">
                      {t("aggregation.modal.apiFormat", {
                        defaultValue: "源协议格式",
                      })}
                    </Label>
                    <Select value={apiFormat} onValueChange={setApiFormat}>
                      <SelectTrigger id="agg-provider-format" className="h-9">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {API_FORMATS.map((fmt) => (
                          <SelectItem key={fmt.value} value={fmt.value}>
                            {fmt.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>

                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-reasoning-effort">
                      {t("aggregation.modal.reasoningEffort", {
                        defaultValue: "Codex 思考等级",
                      })}
                    </Label>
                    <Select
                      value={reasoningEffort}
                      onValueChange={setReasoningEffort}
                    >
                      <SelectTrigger
                        id="agg-provider-reasoning-effort"
                        className="h-9"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        {CODEX_REASONING_EFFORTS.map((item) => (
                          <SelectItem key={item.value} value={item.value}>
                            {item.label}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                </div>

                {/* Claude 分档模型可选覆盖 */}
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-haiku">
                      {t("aggregation.modal.haikuModel", {
                        defaultValue: "Claude Haiku 模型（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-haiku"
                      value={haikuModel}
                      onChange={(e) => setHaikuModel(e.target.value)}
                      placeholder={primaryModel || "跟随默认模型"}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-sonnet">
                      {t("aggregation.modal.sonnetModel", {
                        defaultValue: "Claude Sonnet 模型（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-sonnet"
                      value={sonnetModel}
                      onChange={(e) => setSonnetModel(e.target.value)}
                      placeholder={primaryModel || "跟随默认模型"}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-opus">
                      {t("aggregation.modal.opusModel", {
                        defaultValue: "Claude Opus 模型（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-opus"
                      value={opusModel}
                      onChange={(e) => setOpusModel(e.target.value)}
                      placeholder={primaryModel || "跟随默认模型"}
                    />
                  </div>
                </div>

                {/* 端点与请求头相关设置 */}
                <div className="space-y-3 rounded-control border border-border/70 bg-subtle/30 p-3">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <div className="text-caption font-medium text-fg-1">
                        {t("aggregation.modal.isFullUrl", {
                          defaultValue: "完整端点 URL（不自动拼接路径）",
                        })}
                      </div>
                      <div className="text-[11px] text-fg-3">
                        {t("aggregation.modal.isFullUrlHint", {
                          defaultValue:
                            "开启后直接使用填写的完整地址请求上游，不额外追加 /v1/chat/completions 或 /v1/messages",
                        })}
                      </div>
                    </div>
                    <Switch
                      checked={isFullUrl}
                      onCheckedChange={setIsFullUrl}
                      aria-label={t("aggregation.modal.isFullUrl", {
                        defaultValue: "完整端点 URL（不自动拼接路径）",
                      })}
                    />
                  </div>

                  <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
                    <div className="space-y-1.5">
                      <Label htmlFor="agg-provider-api-key-field">
                        {t("aggregation.modal.apiKeyField", {
                          defaultValue: "Claude 认证请求头",
                        })}
                      </Label>
                      <Select
                        value={apiKeyField}
                        onValueChange={(v) =>
                          setApiKeyField(v as ClaudeApiKeyField)
                        }
                      >
                        <SelectTrigger
                          id="agg-provider-api-key-field"
                          className="h-9"
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                            Bearer Token (ANTHROPIC_AUTH_TOKEN)
                          </SelectItem>
                          <SelectItem value="ANTHROPIC_API_KEY">
                            x-api-key (ANTHROPIC_API_KEY)
                          </SelectItem>
                        </SelectContent>
                      </Select>
                    </div>

                    <div className="space-y-1.5">
                      <Label htmlFor="agg-provider-user-agent">
                        {t("aggregation.modal.customUserAgent", {
                          defaultValue: "自定义 User-Agent（可选）",
                        })}
                      </Label>
                      <Input
                        id="agg-provider-user-agent"
                        value={customUserAgent}
                        onChange={(e) => setCustomUserAgent(e.target.value)}
                        placeholder="Mozilla/5.0 ... / custom-client/1.0"
                      />
                    </div>
                  </div>

                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-models-url">
                      {t("aggregation.modal.customModelsUrl", {
                        defaultValue: "自定义获取模型 URL（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-models-url"
                      value={customModelsUrl}
                      onChange={(e) => setCustomModelsUrl(e.target.value)}
                      placeholder="https://api.example.com/v1/models"
                    />
                  </div>
                </div>

                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <Label>
                      {t("aggregation.modal.compatibleApps", {
                        defaultValue: "兼容分发的应用",
                      })}{" "}
                      ({enabledCount}/{visibleApps.length})
                    </Label>
                    <Button
                      type="button"
                      variant="quiet"
                      size="compact"
                      onClick={handleToggleAllApps}
                    >
                      {allSelected
                        ? t("providerForm.modelPicker.selectNone", {
                            defaultValue: "取消全选",
                          })
                        : t("providerForm.modelPicker.selectAll", {
                            defaultValue: "全选",
                          })}
                    </Button>
                  </div>
                  <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-3">
                    {visibleApps.map((appId) => {
                      const flag = APP_FLAG_KEY[appId];
                      const checked = flag ? Boolean(apps[flag]) : false;
                      return (
                        <button
                          key={appId}
                          type="button"
                          onClick={() => toggleApp(appId)}
                          className={cn(
                            "flex items-center gap-2 rounded-control border px-2.5 py-1.5 text-start text-caption transition-colors",
                            checked
                              ? "border-stack-border bg-stack-soft text-fg-1 font-medium"
                              : "border-border bg-surface text-fg-3 hover:text-fg-2",
                          )}
                        >
                          <AppGlyph
                            app={appId}
                            size={14}
                            badgeClassName="bg-transparent"
                          />
                          <span className="min-w-0 flex-1 truncate">
                            {APP_DISPLAY_NAME[appId]}
                          </span>
                          {checked && (
                            <Check className="h-3.5 w-3.5 shrink-0 text-stack-text" />
                          )}
                        </button>
                      );
                    })}
                  </div>
                </div>

                <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-website">
                      {t("providerForm.websiteUrl", {
                        defaultValue: "官网链接（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-website"
                      value={websiteUrl}
                      onChange={(e) => setWebsiteUrl(e.target.value)}
                      placeholder="https://example.com"
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="agg-provider-notes">
                      {t("universalProvider.notes", {
                        defaultValue: "备注（可选）",
                      })}
                    </Label>
                    <Input
                      id="agg-provider-notes"
                      value={notes}
                      onChange={(e) => setNotes(e.target.value)}
                      placeholder={t("universalProvider.notesPlaceholder", {
                        defaultValue: "备注信息",
                      })}
                    />
                  </div>
                </div>
              </div>
            )}
          </div>
        </form>
      )}
    </FullScreenPanel>
  );
}
