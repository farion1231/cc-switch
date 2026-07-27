import { useEffect, useMemo, useState } from "react";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";
import {
  createCursorModelConfig,
  createCursorProviderChanges,
  type CursorEndpoint,
  type CursorProvider,
  type CursorProviderChanges,
  type CursorProviderType,
} from "@/lib/api/cursor";
import { resolveCursorModelMetadata } from "@/lib/cursorModelMetadata";
import { generateUUID } from "@/utils/uuid";

interface CursorEndpointDialogProps {
  open: boolean;
  endpoint: CursorEndpoint | null;
  providers: CursorProvider[];
  onOpenChange: (open: boolean) => void;
  onSave: (changes: CursorProviderChanges) => Promise<void>;
}

interface EndpointForm {
  providerGroup: string;
  type: CursorProviderType;
  baseURL: string;
  apiKey: string;
}

interface ModelDraft {
  key: string;
  provider?: CursorProvider;
  name: string;
  modelID: string;
  contextWindowTokens?: number | null;
}

const createEndpointForm = (
  endpoint: CursorEndpoint | null,
  providers: CursorProvider[],
): EndpointForm => {
  const config = providers[0]?.settingsConfig;
  return {
    providerGroup: endpoint?.name ?? config?.providerGroup ?? "",
    type: endpoint?.type ?? config?.type ?? "openai",
    baseURL: endpoint?.baseURL ?? config?.baseURL ?? "https://api.openai.com",
    apiKey: endpoint?.apiKey ?? config?.apiKey ?? "",
  };
};

const createModelDrafts = (providers: CursorProvider[]): ModelDraft[] =>
  providers.map((provider) => ({
    key: provider.id,
    provider,
    name: provider.name,
    modelID: provider.settingsConfig.modelID,
  }));

const createEmptyModelDraft = (): ModelDraft => ({
  key: generateUUID(),
  name: "",
  modelID: "",
});

export function CursorEndpointDialog({
  open,
  endpoint,
  providers,
  onOpenChange,
  onSave,
}: CursorEndpointDialogProps) {
  const { t } = useTranslation();
  const [form, setForm] = useState<EndpointForm>(() =>
    createEndpointForm(endpoint, providers),
  );
  const [models, setModels] = useState<ModelDraft[]>(() =>
    createModelDrafts(providers),
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [selectedModelIds, setSelectedModelIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [fetching, setFetching] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!open) return;
    setForm(createEndpointForm(endpoint, providers));
    setModels(createModelDrafts(providers));
    setFetchedModels([]);
    setSelectedModelIds(new Set());
    setError("");
  }, [endpoint, open, providers]);

  const existingModelIds = useMemo(
    () => new Set(models.map((model) => model.modelID.trim()).filter(Boolean)),
    [models],
  );
  const availableFetchedModels = useMemo(
    () => fetchedModels.filter((model) => !existingModelIds.has(model.id)),
    [existingModelIds, fetchedModels],
  );

  const setField = <K extends keyof EndpointForm>(
    key: K,
    value: EndpointForm[K],
  ) => setForm((current) => ({ ...current, [key]: value }));

  const updateModel = (key: string, field: "name" | "modelID", value: string) =>
    setModels((current) =>
      current.map((model) =>
        model.key === key ? { ...model, [field]: value } : model,
      ),
    );

  const handleTypeChange = (type: CursorProviderType) => {
    setFetchedModels([]);
    setSelectedModelIds(new Set());
    setForm((current) => ({
      ...current,
      type,
      baseURL:
        current.baseURL && current.baseURL !== "https://api.openai.com"
          ? current.baseURL
          : type === "anthropic"
            ? "https://api.anthropic.com"
            : "https://api.openai.com",
    }));
  };

  const handleFetchModels = async () => {
    const baseURL = form.baseURL.trim();
    const apiKey = form.apiKey.trim();
    if (!baseURL || !apiKey) {
      setError(t("cursor.endpointDialog.error.credentialsRequired"));
      return;
    }

    setFetching(true);
    setError("");
    try {
      const result = await fetchModelsForConfig(
        baseURL,
        apiKey,
        undefined,
        undefined,
        undefined,
        form.type,
      );
      setFetchedModels(result);
      setSelectedModelIds(
        new Set(
          result
            .filter((model) => !existingModelIds.has(model.id))
            .map((model) => model.id),
        ),
      );
      if (result.length === 0) {
        setError(t("cursor.endpointDialog.error.emptyModelList"));
      }
    } catch (fetchError) {
      setFetchedModels([]);
      setSelectedModelIds(new Set());
      setError(
        t("cursor.endpointDialog.error.fetchFailed", {
          error:
            fetchError instanceof Error
              ? fetchError.message
              : String(fetchError),
        }),
      );
    } finally {
      setFetching(false);
    }
  };

  const addSelectedModels = () => {
    const selected = availableFetchedModels.filter((model) =>
      selectedModelIds.has(model.id),
    );
    setModels((current) => [
      ...current,
      ...selected.map((model) => ({
        key: generateUUID(),
        name: model.id,
        modelID: model.id,
        contextWindowTokens: model.contextWindowTokens,
      })),
    ]);
    setSelectedModelIds(new Set());
  };

  const handleSave = async () => {
    const providerGroup = form.providerGroup.trim();
    const baseURL = form.baseURL.trim();
    const apiKey = form.apiKey.trim();
    const validModels = models.map((model) => ({
      ...model,
      name: model.name.trim(),
      modelID: model.modelID.trim(),
    }));

    try {
      if (!providerGroup || !baseURL || !apiKey) {
        throw new Error(t("cursor.endpointDialog.error.requiredFields"));
      }
      if (!endpoint && validModels.length === 0) {
        throw new Error(t("cursor.endpointDialog.error.modelRequired"));
      }
      if (validModels.some((model) => !model.name || !model.modelID)) {
        throw new Error(t("cursor.endpointDialog.error.modelFieldsRequired"));
      }
      if (
        new Set(validModels.map((model) => model.modelID)).size !==
        validModels.length
      ) {
        throw new Error(t("cursor.endpointDialog.error.duplicateModelId"));
      }

      const endpointId = endpoint?.id ?? generateUUID();
      const nextEndpoint: CursorEndpoint = {
        id: endpointId,
        name: providerGroup,
        type: form.type,
        baseURL,
        apiKey,
        createdAt: endpoint?.createdAt ?? Date.now(),
      };
      const nextProviders = validModels.map((model) => {
        const metadata = resolveCursorModelMetadata(
          {
            id: model.modelID,
            ownedBy: null,
            contextWindowTokens: model.contextWindowTokens,
          },
          baseURL,
          form.type,
        );
        const settingsConfig = createCursorModelConfig({
          ...(model.provider?.settingsConfig ?? {}),
          providerGroup,
          endpointId,
          type: form.type,
          baseURL,
          apiKey,
          modelID: model.modelID,
          contextWindowTokens:
            model.provider?.settingsConfig.contextWindowTokens ||
            metadata.contextWindowTokens,
        });
        return {
          ...(model.provider ?? {
            id: generateUUID(),
            createdAt: Date.now(),
          }),
          name: model.name,
          icon: model.provider?.icon,
          iconColor: model.provider?.iconColor,
          settingsConfig,
        } satisfies CursorProvider;
      });

      setSaving(true);
      setError("");
      await onSave(
        createCursorProviderChanges(nextEndpoint, providers, nextProviders),
      );
      onOpenChange(false);
    } catch (saveError) {
      setError(
        saveError instanceof Error ? saveError.message : String(saveError),
      );
    } finally {
      setSaving(false);
    }
  };

  const footer = (
    <>
      <span className="mr-auto min-w-0 truncate text-xs text-muted-foreground">
        {t("cursor.endpointDialog.footerHint")}
      </span>
      <Button
        variant="outline"
        onClick={() => onOpenChange(false)}
        disabled={saving}
      >
        {t("common.cancel")}
      </Button>
      <Button onClick={() => void handleSave()} disabled={saving}>
        {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
        {endpoint
          ? t("cursor.endpointDialog.saveAction")
          : t("cursor.endpointDialog.addAction")}
      </Button>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={
        endpoint
          ? t("cursor.endpointDialog.editTitle")
          : t("cursor.endpointDialog.addTitle")
      }
      onClose={() => onOpenChange(false)}
      footer={footer}
      contentClassName="pt-3"
    >
      <div className="mx-auto w-full max-w-4xl space-y-6">
        <section className="glass space-y-5 rounded-xl border border-white/10 p-6">
          <div>
            <h3 className="text-base font-semibold">
              {t("cursor.endpointDialog.config.title")}
            </h3>
            <p className="mt-1 text-sm text-muted-foreground">
              {t("cursor.endpointDialog.config.description")}
            </p>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="grid gap-4 sm:grid-cols-2">
            <Field label={t("cursor.endpointDialog.fields.providerName")}>
              <Input
                value={form.providerGroup}
                onChange={(event) =>
                  setField("providerGroup", event.target.value)
                }
                placeholder={t(
                  "cursor.endpointDialog.placeholders.providerName",
                )}
              />
            </Field>
            <Field label={t("cursor.endpointDialog.fields.apiProtocol")}>
              <Select value={form.type} onValueChange={handleTypeChange}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">
                    {t("cursor.protocol.openAICompatible")}
                  </SelectItem>
                  <SelectItem value="anthropic">
                    {t("cursor.protocol.anthropicCompatible")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </div>
          <Field label={t("cursor.endpointDialog.fields.apiEndpoint")}>
            <Input
              value={form.baseURL}
              onChange={(event) => setField("baseURL", event.target.value)}
              placeholder={t("cursor.endpointDialog.placeholders.apiEndpoint")}
            />
          </Field>
          <Field
            label={t("cursor.endpointDialog.fields.apiKey")}
            hint={t("cursor.endpointDialog.fields.apiKeyHint")}
          >
            <Input
              type="password"
              value={form.apiKey}
              onChange={(event) => setField("apiKey", event.target.value)}
              autoComplete="new-password"
              placeholder={t("cursor.endpointDialog.placeholders.apiKey")}
            />
          </Field>
        </section>

        <section className="glass space-y-5 rounded-xl border border-white/10 p-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h3 className="text-base font-semibold">
                {t("cursor.endpointDialog.models.title")}
              </h3>
              <p className="mt-1 text-sm text-muted-foreground">
                {t("cursor.endpointDialog.models.description")}
              </p>
            </div>
            <div className="flex gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void handleFetchModels()}
                disabled={fetching}
              >
                {fetching ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Download className="mr-2 h-4 w-4" />
                )}
                {t("cursor.endpointDialog.models.fetchAction")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  setModels((current) => [...current, createEmptyModelDraft()])
                }
              >
                <Plus className="mr-2 h-4 w-4" />
                {t("cursor.endpointDialog.models.addManually")}
              </Button>
            </div>
          </div>

          {availableFetchedModels.length > 0 && (
            <div className="rounded-lg border border-border-default bg-muted/20 p-4">
              <div className="mb-3 flex items-center justify-between gap-3">
                <Label>{t("cursor.endpointDialog.models.selectLabel")}</Label>
                <Button
                  type="button"
                  size="sm"
                  onClick={addSelectedModels}
                  disabled={selectedModelIds.size === 0}
                >
                  {t("cursor.endpointDialog.models.addSelected", {
                    count: selectedModelIds.size,
                  })}
                </Button>
              </div>
              <div className="grid max-h-64 gap-2 overflow-y-auto sm:grid-cols-2">
                {availableFetchedModels.map((model) => (
                  <label
                    key={model.id}
                    className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted/60"
                  >
                    <Checkbox
                      checked={selectedModelIds.has(model.id)}
                      onCheckedChange={(checked) =>
                        setSelectedModelIds((current) => {
                          const next = new Set(current);
                          if (checked) next.add(model.id);
                          else next.delete(model.id);
                          return next;
                        })
                      }
                    />
                    <span className="min-w-0 truncate" title={model.id}>
                      {model.id}
                    </span>
                  </label>
                ))}
              </div>
            </div>
          )}

          {models.length === 0 ? (
            <div className="rounded-lg border border-dashed border-border-default px-6 py-10 text-center text-sm text-muted-foreground">
              {t("cursor.endpointDialog.models.empty")}
            </div>
          ) : (
            <div className="space-y-3">
              {models.map((model) => (
                <div
                  key={model.key}
                  className="grid gap-3 rounded-lg border border-border-default p-4 sm:grid-cols-[1fr_1fr_auto] sm:items-end"
                >
                  <Field label={t("cursor.endpointDialog.fields.displayName")}>
                    <Input
                      value={model.name}
                      onChange={(event) =>
                        updateModel(model.key, "name", event.target.value)
                      }
                      placeholder={t(
                        "cursor.endpointDialog.placeholders.displayName",
                      )}
                    />
                  </Field>
                  <Field label={t("cursor.endpointDialog.fields.modelId")}>
                    <Input
                      value={model.modelID}
                      onChange={(event) =>
                        updateModel(model.key, "modelID", event.target.value)
                      }
                      placeholder={t(
                        "cursor.endpointDialog.placeholders.modelId",
                      )}
                    />
                  </Field>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    title={t("cursor.endpointDialog.models.removeAction")}
                    aria-label={t("cursor.endpointDialog.models.removeAction")}
                    onClick={() =>
                      setModels((current) =>
                        current.filter((item) => item.key !== model.key),
                      )
                    }
                    className="text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </FullScreenPanel>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}
