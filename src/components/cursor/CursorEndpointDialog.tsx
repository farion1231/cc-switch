import { useEffect, useMemo, useState } from "react";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
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
      setError("请先填写 API 端点和 API Key");
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
      if (result.length === 0) setError("提供商返回了空模型列表");
    } catch (fetchError) {
      setFetchedModels([]);
      setSelectedModelIds(new Set());
      setError(
        `获取模型失败：${fetchError instanceof Error ? fetchError.message : String(fetchError)}`,
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
        throw new Error("提供商名称、API 端点和 API Key 不能为空");
      }
      if (!endpoint && validModels.length === 0) {
        throw new Error("请至少添加一个模型");
      }
      if (validModels.some((model) => !model.name || !model.modelID)) {
        throw new Error("模型显示名称和模型 ID 不能为空");
      }
      if (
        new Set(validModels.map((model) => model.modelID)).size !==
        validModels.length
      ) {
        throw new Error("同一 Endpoint 下不能添加重复的模型 ID");
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
        保存时会统一更新该 Endpoint 下所有模型的连接配置。
      </span>
      <Button
        variant="outline"
        onClick={() => onOpenChange(false)}
        disabled={saving}
      >
        取消
      </Button>
      <Button onClick={() => void handleSave()} disabled={saving}>
        {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
        {endpoint ? "保存 Endpoint" : "添加 Endpoint"}
      </Button>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={endpoint ? "编辑 Cursor Endpoint" : "添加 Cursor Endpoint"}
      onClose={() => onOpenChange(false)}
      footer={footer}
      contentClassName="pt-3"
    >
      <div className="mx-auto w-full max-w-4xl space-y-6">
        <section className="glass space-y-5 rounded-xl border border-white/10 p-6">
          <div>
            <h3 className="text-base font-semibold">Endpoint 配置</h3>
            <p className="mt-1 text-sm text-muted-foreground">
              连接信息由该 Endpoint 下的全部模型共享。
            </p>
          </div>

          {error && (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="提供商名称">
              <Input
                value={form.providerGroup}
                onChange={(event) =>
                  setField("providerGroup", event.target.value)
                }
                placeholder="例如 OpenRouter"
              />
            </Field>
            <Field label="API 协议">
              <Select value={form.type} onValueChange={handleTypeChange}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">OpenAI Compatible</SelectItem>
                  <SelectItem value="anthropic">
                    Anthropic Compatible
                  </SelectItem>
                </SelectContent>
              </Select>
            </Field>
          </div>
          <Field label="API 端点">
            <Input
              value={form.baseURL}
              onChange={(event) => setField("baseURL", event.target.value)}
              placeholder="https://api.example.com"
            />
          </Field>
          <Field label="API Key" hint="凭证仅存储在本机数据库中">
            <Input
              type="password"
              value={form.apiKey}
              onChange={(event) => setField("apiKey", event.target.value)}
              autoComplete="new-password"
              placeholder="sk-..."
            />
          </Field>
        </section>

        <section className="glass space-y-5 rounded-xl border border-white/10 p-6">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h3 className="text-base font-semibold">模型列表</h3>
              <p className="mt-1 text-sm text-muted-foreground">
                可以手动添加，也可以从提供商接口批量获取。
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
                获取模型
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
                手动添加
              </Button>
            </div>
          </div>

          {availableFetchedModels.length > 0 && (
            <div className="rounded-lg border border-border-default bg-muted/20 p-4">
              <div className="mb-3 flex items-center justify-between gap-3">
                <Label>选择要添加的模型</Label>
                <Button
                  type="button"
                  size="sm"
                  onClick={addSelectedModels}
                  disabled={selectedModelIds.size === 0}
                >
                  添加选中项（{selectedModelIds.size}）
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
              暂无模型，请获取模型或手动添加。
            </div>
          ) : (
            <div className="space-y-3">
              {models.map((model) => (
                <div
                  key={model.key}
                  className="grid gap-3 rounded-lg border border-border-default p-4 sm:grid-cols-[1fr_1fr_auto] sm:items-end"
                >
                  <Field label="显示名称">
                    <Input
                      value={model.name}
                      onChange={(event) =>
                        updateModel(model.key, "name", event.target.value)
                      }
                      placeholder="例如 GPT-5 Coding"
                    />
                  </Field>
                  <Field label="模型 ID">
                    <Input
                      value={model.modelID}
                      onChange={(event) =>
                        updateModel(model.key, "modelID", event.target.value)
                      }
                      placeholder="gpt-5.4"
                    />
                  </Field>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    title="移除模型"
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
