import { useTranslation } from "react-i18next";
import {
  useState,
  useRef,
  useCallback,
  useMemo,
  useEffect,
  type ReactNode,
} from "react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  Download,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Loader2,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ApiKeySection } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  hermesApiModes,
  type HermesApiMode,
  type HermesModel,
} from "@/config/hermesProviderPresets";
import type { ProviderCategory } from "@/types";

interface HermesFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  apiMode: HermesApiMode;
  onApiModeChange: (mode: HermesApiMode) => void;
  models: HermesModel[];
  onModelsChange: (models: HermesModel[]) => void;
  rateLimitDelay: number | undefined;
  onRateLimitDelayChange: (delay: number | undefined) => void;
}

type BaseUrlErrorCode = "empty" | "invalid" | "scheme";

const BASE_URL_ERROR_I18N_KEY: Record<BaseUrlErrorCode, string> = {
  empty: "hermes.form.baseUrlRequired",
  scheme: "hermes.form.baseUrlScheme",
  invalid: "hermes.form.baseUrlInvalid",
};

const TEMPLATE_TOKEN_RE = /\$\{[^}]+\}/g;

/**
 * Hermes 0.10.0+ rejects `base_url` entries that don't parse as proper URLs
 * (commit 2cdae233). Validate client-side so the error surfaces before the
 * request ever reaches Hermes' startup.
 */
function validateBaseUrl(raw: string): BaseUrlErrorCode | null {
  const trimmed = raw.trim();
  if (!trimmed) return "empty";
  // Presets like KAT-Coder embed `${VAR}` tokens — swap them before URL parse.
  const candidate = trimmed.replace(TEMPLATE_TOKEN_RE, "placeholder");
  let u: URL;
  try {
    u = new URL(candidate);
  } catch {
    return "invalid";
  }
  if (!u.protocol.startsWith("http")) return "scheme";
  if (!u.hostname) return "invalid";
  return null;
}

interface AdvancedSectionProps {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  labelKey: string;
  children: ReactNode;
}

function AdvancedSection({
  open,
  onOpenChange,
  labelKey,
  children,
}: AdvancedSectionProps) {
  const { t } = useTranslation();
  return (
    <Collapsible open={open} onOpenChange={onOpenChange}>
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 gap-1 text-xs text-muted-foreground hover:text-foreground"
        >
          {open ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
          {t(labelKey)}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-2">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
}

export function HermesFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  apiMode,
  onApiModeChange,
  models,
  onModelsChange,
  rateLimitDelay,
  onRateLimitDelayChange,
}: HermesFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Record<number, boolean>>(
    {},
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [baseUrlTouched, setBaseUrlTouched] = useState(false);
  const [providerAdvancedOpen, setProviderAdvancedOpen] = useState(
    rateLimitDelay !== undefined,
  );

  // Auto-expand when a preset switch brings in a value so the user sees it;
  // don't force-collapse on clear, to avoid yanking the panel shut mid-edit.
  useEffect(() => {
    if (rateLimitDelay !== undefined) {
      setProviderAdvancedOpen(true);
    }
  }, [rateLimitDelay]);

  const baseUrlErrorCode = useMemo(() => validateBaseUrl(baseUrl), [baseUrl]);
  const showBaseUrlError = baseUrlTouched && baseUrlErrorCode !== null;
  const baseUrlErrorMessage = baseUrlErrorCode
    ? t(BASE_URL_ERROR_I18N_KEY[baseUrlErrorCode])
    : "";

  // Stable list keys: a manual ref rather than UUID-in-state so adding/removing
  // rows doesn't re-mount unrelated inputs (would drop focus mid-typing).
  const modelKeysRef = useRef<string[]>([]);
  while (modelKeysRef.current.length < models.length) {
    modelKeysRef.current.push(crypto.randomUUID());
  }
  if (modelKeysRef.current.length > models.length) {
    modelKeysRef.current.length = models.length;
  }
  const modelKeys = modelKeysRef.current;

  // Group fetched models by vendor once — Radix DropdownMenuContent doesn't
  // lazy-mount, so computing this in JSX would re-run per model row per render.
  const groupedFetchedModels = useMemo(
    () =>
      Object.entries(
        fetchedModels.reduce(
          (acc, m) => {
            const v = m.ownedBy || "Other";
            if (!acc[v]) acc[v] = [];
            acc[v].push(m);
            return acc;
          },
          {} as Record<string, FetchedModel[]>,
        ),
      ).sort(([a], [b]) => a.localeCompare(b)),
    [fetchedModels],
  );

  const toggleModelAdvanced = (index: number) => {
    setExpandedModels((prev) => ({ ...prev, [index]: !prev[index] }));
  };

  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      { id: "", name: "", context_length: undefined },
    ]);
  };

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey)
      .then((fetched) => {
        setFetchedModels(fetched);
        if (fetched.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: fetched.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

  const handleRemoveModel = (index: number) => {
    modelKeysRef.current.splice(index, 1);
    const next = [...models];
    next.splice(index, 1);
    onModelsChange(next);
    setExpandedModels((prev) => {
      const updated = { ...prev };
      delete updated[index];
      return updated;
    });
  };

  const handleModelChange = (
    index: number,
    field: keyof HermesModel,
    value: unknown,
  ) => {
    const next = [...models];
    next[index] = { ...next[index], [field]: value };
    onModelsChange(next);
  };

  return (
    <>
      <div className="space-y-2">
        <FormLabel htmlFor="hermes-api-mode">
          {t("hermes.form.apiMode", { defaultValue: "API 模式" })}
        </FormLabel>
        <Select
          value={apiMode}
          onValueChange={(v) => onApiModeChange(v as HermesApiMode)}
        >
          <SelectTrigger id="hermes-api-mode">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {hermesApiModes.map((mode) => (
              <SelectItem key={mode.value} value={mode.value}>
                {t(mode.labelKey)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("hermes.form.apiModeHint", {
            defaultValue: "供应商 API 协议。请根据端点选择正确的协议。",
          })}
        </p>
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="hermes-baseurl">
          {t("hermes.form.baseUrl", { defaultValue: "API 端点" })}
        </FormLabel>
        <Input
          id="hermes-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          onBlur={() => setBaseUrlTouched(true)}
          placeholder="https://api.example.com/v1"
          aria-invalid={showBaseUrlError}
          className={
            showBaseUrlError
              ? "border-destructive focus-visible:ring-destructive"
              : undefined
          }
        />
        {showBaseUrlError ? (
          <p className="text-xs text-destructive">{baseUrlErrorMessage}</p>
        ) : (
          <p className="text-xs text-muted-foreground">
            {t("hermes.form.baseUrlHint", {
              defaultValue: "供应商的 API 端点地址。",
            })}
          </p>
        )}
      </div>

      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("hermes.form.models", { defaultValue: "模型列表" })}
          </FormLabel>
          <div className="flex gap-1">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleFetchModels}
              disabled={isFetchingModels}
              className="h-7 gap-1"
            >
              {isFetchingModels ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              {t("providerForm.fetchModels")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("hermes.form.addModel", { defaultValue: "添加模型" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("hermes.form.noModels", {
              defaultValue: "暂无模型配置。切换到此供应商时将无默认模型。",
            })}
          </p>
        ) : (
          <div className="space-y-4">
            {models.map((model, index) => (
              <div
                key={modelKeys[index]}
                className="p-3 border border-border/50 rounded-lg space-y-3"
              >
                {/* Role badge — first entry is the default written to model.default on switch */}
                <div className="flex items-center">
                  <span
                    className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${
                      index === 0
                        ? "bg-blue-500/15 text-blue-600 dark:text-blue-400"
                        : "bg-muted text-muted-foreground"
                    }`}
                  >
                    {index === 0
                      ? t("hermes.form.primaryModel", {
                          defaultValue: "默认模型",
                        })
                      : t("hermes.form.fallbackModel", {
                          defaultValue: "备选模型",
                        })}
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("hermes.form.modelId", { defaultValue: "模型 ID" })}
                    </label>
                    <div className="flex gap-1">
                      <Input
                        value={model.id}
                        onChange={(e) =>
                          handleModelChange(index, "id", e.target.value)
                        }
                        placeholder={t("hermes.form.modelIdPlaceholder", {
                          defaultValue: "anthropic/claude-opus-4-7",
                        })}
                        className="flex-1"
                      />
                      {fetchedModels.length > 0 && (
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              variant="outline"
                              size="icon"
                              className="shrink-0"
                            >
                              <ChevronDown className="h-4 w-4" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent
                            align="end"
                            className="max-h-64 overflow-y-auto z-[200]"
                          >
                            {groupedFetchedModels.map(
                              ([vendor, vModels], vi) => (
                                <div key={vendor}>
                                  {vi > 0 && <DropdownMenuSeparator />}
                                  <DropdownMenuLabel>
                                    {vendor}
                                  </DropdownMenuLabel>
                                  {vModels.map((m) => (
                                    <DropdownMenuItem
                                      key={m.id}
                                      onSelect={() =>
                                        handleModelChange(index, "id", m.id)
                                      }
                                    >
                                      {m.id}
                                    </DropdownMenuItem>
                                  ))}
                                </div>
                              ),
                            )}
                          </DropdownMenuContent>
                        </DropdownMenu>
                      )}
                    </div>
                  </div>
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("hermes.form.modelName", {
                        defaultValue: "显示名称",
                      })}
                    </label>
                    <Input
                      value={model.name ?? ""}
                      onChange={(e) =>
                        handleModelChange(index, "name", e.target.value)
                      }
                      placeholder={t("hermes.form.modelNamePlaceholder", {
                        defaultValue: "Claude Opus 4.7",
                      })}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveModel(index)}
                    className="h-9 w-9 mt-5 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                <AdvancedSection
                  open={expandedModels[index] ?? false}
                  onOpenChange={() => toggleModelAdvanced(index)}
                  labelKey="hermes.form.advancedOptions"
                >
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("hermes.form.contextLength", {
                        defaultValue: "上下文长度",
                      })}
                    </label>
                    <Input
                      type="number"
                      value={model.context_length ?? ""}
                      onChange={(e) =>
                        handleModelChange(
                          index,
                          "context_length",
                          e.target.value ? parseInt(e.target.value) : undefined,
                        )
                      }
                      placeholder="200000"
                    />
                  </div>
                </AdvancedSection>
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("hermes.form.modelsHint", {
            defaultValue:
              "切换到此供应商时，第一个模型会写入顶层 model.default。",
          })}
        </p>
      </div>

      <AdvancedSection
        open={providerAdvancedOpen}
        onOpenChange={setProviderAdvancedOpen}
        labelKey="hermes.form.providerAdvanced"
      >
        <div className="space-y-1">
          <label className="text-xs text-muted-foreground">
            {t("hermes.form.rateLimitDelay", {
              defaultValue: "请求间隔（秒）",
            })}
          </label>
          <Input
            type="number"
            step="0.1"
            min="0"
            value={rateLimitDelay ?? ""}
            onChange={(e) => {
              const v = e.target.value;
              if (v === "") {
                onRateLimitDelayChange(undefined);
                return;
              }
              const n = parseFloat(v);
              onRateLimitDelayChange(
                Number.isFinite(n) && n >= 0 ? n : undefined,
              );
            }}
            placeholder="0.5"
          />
          <p className="text-xs text-muted-foreground">
            {t("hermes.form.rateLimitDelayHint", {
              defaultValue:
                "连续请求间的最小间隔秒数（可选）。留空表示无限制。",
            })}
          </p>
        </div>
      </AdvancedSection>
    </>
  );
}
