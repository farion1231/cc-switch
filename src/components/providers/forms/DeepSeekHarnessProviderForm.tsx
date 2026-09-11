import { useEffect, useMemo, useState } from "react";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderFormProps, ProviderFormValues } from "./ProviderForm";

type DshModel = { id: string; name?: string; contextWindow?: number };

const asObject = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};

export function DeepSeekHarnessProviderForm({
  providerId,
  initialData,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  onSubmitReadyChange,
  showButtons = true,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const initial = asObject(initialData?.settingsConfig);
  const isOfficial =
    initialData?.category === "official" || providerId === "deepseek-official";
  const [providerKey, setProviderKey] = useState(
    providerId ?? (isOfficial ? "deepseek-official" : "custom-dsh"),
  );
  const [name, setName] = useState(
    initialData?.name ?? (isOfficial ? "DeepSeek" : "Custom DSH"),
  );
  const [apiKey, setApiKey] = useState(String(initial.apiKey ?? ""));
  const [apiKeyEnv, setApiKeyEnv] = useState(
    String(initial.apiKeyEnv ?? (isOfficial ? "DEEPSEEK_API_KEY" : "CUSTOM_DSH_API_KEY")),
  );
  const [baseURL, setBaseURL] = useState(String(initial.baseURL ?? ""));
  const [api, setApi] = useState(String(initial.api ?? "openai-completions"));
  const [models, setModels] = useState<DshModel[]>(() =>
    Array.isArray(initial.models)
      ? initial.models.map((model) => {
          const item = asObject(model);
          return {
            id: String(item.id ?? ""),
            name: item.name ? String(item.name) : undefined,
            contextWindow:
              typeof item.contextWindow === "number" ? item.contextWindow : undefined,
          };
        })
      : [],
  );
  const [defaultModel, setDefaultModel] = useState(
    initialData?.meta?.dshCurrentModel ?? models[0]?.id ?? "",
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [fetching, setFetching] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const ready = useMemo(
    () =>
      Boolean(
        providerKey.trim() &&
          name.trim() &&
          baseURL.trim() &&
          apiKeyEnv.trim() &&
          models.some((model) => model.id.trim()),
      ),
    [apiKeyEnv, baseURL, models, name, providerKey],
  );
  useEffect(() => onSubmitReadyChange?.(ready), [onSubmitReadyChange, ready]);

  const fetchModels = async () => {
    if (!baseURL.trim() || !apiKey.trim()) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(apiKey.trim()),
        hasBaseUrl: Boolean(baseURL.trim()),
      });
      return;
    }
    setFetching(true);
    try {
      const result = await fetchModelsForConfig(baseURL.trim(), apiKey.trim());
      setFetchedModels(result);
      toast.success(t("providerForm.fetchModelsSuccess", { count: result.length }));
    } catch (error) {
      showFetchModelsError(error, t);
    } finally {
      setFetching(false);
    }
  };

  const addModel = (id = "") => {
    const trimmed = id.trim();
    if (trimmed && models.some((model) => model.id === trimmed)) return;
    setModels((current) => [...current, { id: trimmed, name: trimmed || undefined }]);
    if (trimmed && !defaultModel) setDefaultModel(trimmed);
  };

  const handleSubmit = async () => {
    if (!ready || submitting) return;
    setSubmitting(true);
    onSubmittingChange?.(true);
    try {
      const normalizedModels = models
        .filter((model) => model.id.trim())
        .map((model) => ({
          id: model.id.trim(),
          name: model.name?.trim() || model.id.trim(),
          ...(model.contextWindow ? { contextWindow: model.contextWindow } : {}),
        }));
      const settingsConfig = isOfficial
        ? {
            apiKey: apiKey.trim(),
            baseURL: baseURL.trim(),
            profile: "desktop",
            models: normalizedModels,
          }
        : {
            displayName: name.trim(),
            api,
            baseURL: baseURL.trim(),
            apiKeyEnv: apiKeyEnv.trim(),
            apiKey: apiKey.trim(),
            models: normalizedModels,
          };
      const values: ProviderFormValues = {
        name: name.trim(),
        websiteUrl: initialData?.websiteUrl ?? "",
        notes: initialData?.notes ?? "",
        settingsConfig: JSON.stringify(settingsConfig),
        icon: initialData?.icon ?? "deepseek",
        iconColor: initialData?.iconColor ?? "",
        providerKey: isOfficial ? "deepseek-official" : providerKey.trim(),
        presetCategory: isOfficial ? "official" : "custom",
        meta: {
          ...initialData?.meta,
          providerType: isOfficial ? "dsh_deepseek" : "dsh_pi_ai",
          dshCurrentModel: defaultModel.trim() || normalizedModels[0]?.id,
        },
      };
      await onSubmit(values);
    } finally {
      setSubmitting(false);
      onSubmittingChange?.(false);
    }
  };

  return (
    <div className="space-y-5">
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="dsh-provider-name">{t("provider.name")}</Label>
          <Input id="dsh-provider-name" value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div className="space-y-2">
          <Label htmlFor="dsh-provider-key">Provider ID</Label>
          <Input
            id="dsh-provider-key"
            value={providerKey}
            onChange={(e) => setProviderKey(e.target.value)}
            disabled={Boolean(providerId) || isOfficial}
          />
        </div>
      </div>

      <ApiKeySection
        id="dsh-api-key"
        label="API Key"
        value={apiKey}
        onChange={setApiKey}
        category="custom"
        shouldShowLink={false}
        websiteUrl=""
      />

      <div className="space-y-2">
        <Label htmlFor="dsh-api-key-env">Credential Ref</Label>
        <Input id="dsh-api-key-env" value={apiKeyEnv} onChange={(e) => setApiKeyEnv(e.target.value.toUpperCase())} disabled={isOfficial} />
      </div>

      <div className="space-y-2">
        <Label htmlFor="dsh-base-url">Base URL</Label>
        <Input
          id="dsh-base-url"
          value={baseURL}
          onChange={(event) => setBaseURL(event.target.value)}
          placeholder="https://api.example.com/v1"
          autoComplete="off"
        />
      </div>

      {!isOfficial && (
        <div className="space-y-2">
          <Label>API Format</Label>
          <Select value={api} onValueChange={setApi}>
            <SelectTrigger><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem value="openai-completions">OpenAI Chat Completions</SelectItem>
              <SelectItem value="openai-responses">OpenAI Responses</SelectItem>
              <SelectItem value="anthropic-messages">Anthropic Messages</SelectItem>
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="space-y-3 border-t border-border pt-4">
        <div className="flex items-center justify-between gap-2">
          <Label>Model Catalog</Label>
          <div className="flex gap-2">
            <Button type="button" variant="outline" size="sm" onClick={fetchModels} disabled={fetching}>
              {fetching ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : <Download className="mr-2 h-4 w-4" />}
              {t("providerForm.fetchModels")}
            </Button>
            <Button type="button" variant="outline" size="sm" onClick={() => addModel()}>
              <Plus className="mr-2 h-4 w-4" />{t("common.add")}
            </Button>
          </div>
        </div>
        {fetchedModels.length > 0 && <ModelDropdown models={fetchedModels} onSelect={addModel} />}
        {models.map((model, index) => (
          <div key={`${index}-${model.id}`} className="grid grid-cols-[1fr_1fr_auto] gap-2">
            <Input value={model.id} placeholder="model-id" onChange={(e) => setModels((current) => current.map((item, i) => i === index ? { ...item, id: e.target.value } : item))} />
            <Input value={model.name ?? ""} placeholder="Display name" onChange={(e) => setModels((current) => current.map((item, i) => i === index ? { ...item, name: e.target.value } : item))} />
            <Button type="button" variant="ghost" size="icon" onClick={() => setModels((current) => current.filter((_, i) => i !== index))}><Trash2 className="h-4 w-4" /></Button>
          </div>
        ))}
        <div className="space-y-2">
          <Label htmlFor="dsh-default-model">Default Model</Label>
          <Input id="dsh-default-model" value={defaultModel} onChange={(e) => setDefaultModel(e.target.value)} />
        </div>
      </div>

      {showButtons && (
        <div className="flex justify-end gap-2 border-t border-border pt-4">
          <Button type="button" variant="outline" onClick={onCancel}>{t("common.cancel")}</Button>
          <Button type="button" onClick={handleSubmit} disabled={!ready || submitting}>
            {submitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}{submitLabel}
          </Button>
        </div>
      )}
    </div>
  );
}
