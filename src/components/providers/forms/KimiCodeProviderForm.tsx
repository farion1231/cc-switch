import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderMeta } from "@/types";
import type { ProviderFormProps } from "./ProviderForm";
import { BasicFormFields } from "./BasicFormFields";
import {
  buildKimiCodeConfig,
  buildSettingsConfig,
  KIMI_PROVIDER_TYPES,
  parseKimiCodeConfig,
  validateKimiCodeConfig,
  type KimiModelEntry,
  type KimiProviderType,
} from "@/utils/kimiCodeConfig";
import { resolveProviderIcon } from "@/utils/providerIcon";

type KimiCodeProviderFormProps = Omit<ProviderFormProps, "appId">;

export function KimiCodeProviderForm({
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: KimiCodeProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const initialConfigText =
    typeof initialData?.settingsConfig?.config === "string"
      ? initialData.settingsConfig.config
      : undefined;
  const initialConfig = useMemo(
    () => parseKimiCodeConfig(initialConfigText, initialData?.name),
    [initialConfigText, initialData?.name],
  );

  const category = initialData?.category ?? "custom";
  const [providerType, setProviderType] = useState<KimiProviderType>(
    initialConfig.providerType,
  );
  const [providerKey, setProviderKey] = useState(initialConfig.providerId);
  const [baseUrl, setBaseUrl] = useState(initialConfig.baseUrl);
  const [apiKey, setApiKey] = useState(initialConfig.apiKey);
  const [models, setModels] = useState<KimiModelEntry[]>(initialConfig.models);
  const [selectedModel, setSelectedModel] = useState(
    initialConfig.selectedModel,
  );
  const [headersText, setHeadersText] = useState(
    Object.keys(initialConfig.customHeaders).length > 0
      ? JSON.stringify(initialConfig.customHeaders, null, 2)
      : "",
  );
  const [rawConfig, setRawConfig] = useState(
    initialConfigText ?? buildKimiCodeConfig(initialConfig),
  );
  const [useRawEditor, setUseRawEditor] = useState(false);

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name: initialData?.name || "",
      websiteUrl: initialData?.websiteUrl || "",
      notes: initialData?.notes || "",
      // Scoped TOML is validated in handleSubmit; this field only satisfies
      // the shared form schema, which otherwise expects a JSON settings string.
      settingsConfig: "{}",
      icon:
        resolveProviderIcon(
          "kimicode",
          initialData?.icon,
          initialData?.iconColor,
        ) ?? "",
      iconColor: initialData?.iconColor,
    },
  });

  useEffect(() => {
    onSubmittingChange?.(form.formState.isSubmitting);
  }, [form.formState.isSubmitting, onSubmittingChange]);

  const syncRawFromFields = () => {
    let customHeaders: Record<string, string> = {};
    if (headersText.trim()) {
      try {
        const parsed = JSON.parse(headersText) as Record<string, unknown>;
        for (const [k, v] of Object.entries(parsed)) {
          if (typeof v === "string") customHeaders[k] = v;
        }
      } catch {
        toast.error(
          t("provider.kimicode.headersInvalid", {
            defaultValue: "自定义 Headers 必须是 JSON 对象",
          }),
        );
        return null;
      }
    }
    const state = {
      providerId: providerKey.trim() || "custom",
      providerType,
      baseUrl,
      apiKey,
      customHeaders,
      models,
      selectedModel: selectedModel || models[0]?.alias || "default",
    };
    const config = buildKimiCodeConfig(state);
    setRawConfig(config);
    return state;
  };

  const handleSubmit = async (values: ProviderFormData) => {
    let configText = rawConfig;
    let state = initialConfig;
    if (!useRawEditor) {
      const synced = syncRawFromFields();
      if (!synced) return;
      state = synced;
      configText = buildKimiCodeConfig(synced);
    }

    const validationError = validateKimiCodeConfig(configText);
    if (validationError) {
      toast.error(
        t("provider.kimicode.configInvalid", {
          defaultValue: `Kimi Code 配置无效 (${validationError})`,
          error: validationError,
        }),
      );
      return;
    }

    if (useRawEditor) {
      // Keep metadata consistent with the raw fragment instead of persisting
      // stale provider/model identifiers from the structured form.
      state = parseKimiCodeConfig(configText);
    }

    const settingsConfig = buildSettingsConfig(state, configText);
    const meta: ProviderMeta = {
      ...(initialData?.meta || {}),
    };

    await onSubmit({
      name: values.name,
      settingsConfig: JSON.stringify(settingsConfig),
      websiteUrl: values.websiteUrl || undefined,
      notes: values.notes || undefined,
      icon: values.icon,
      iconColor: values.iconColor,
      meta,
      presetCategory: category,
      providerKey: state.providerId,
    });
  };

  const updateModel = (index: number, patch: Partial<KimiModelEntry>) => {
    setModels((prev) =>
      prev.map((m, i) => (i === index ? { ...m, ...patch } : m)),
    );
  };

  return (
    <Form {...form}>
      <form onSubmit={form.handleSubmit(handleSubmit)} className="space-y-4">
        <BasicFormFields form={form} />

        <div className="flex items-center justify-between">
          <FormLabel>
            {t("provider.kimicode.fragment", {
              defaultValue: "Kimi Code 供应商片段",
            })}
          </FormLabel>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => {
              if (!useRawEditor) {
                const synced = syncRawFromFields();
                if (synced) setRawConfig(buildKimiCodeConfig(synced));
              } else {
                const parsed = parseKimiCodeConfig(rawConfig);
                setProviderKey(parsed.providerId);
                setProviderType(parsed.providerType);
                setBaseUrl(parsed.baseUrl);
                setApiKey(parsed.apiKey);
                setModels(parsed.models);
                setSelectedModel(parsed.selectedModel);
                setHeadersText(
                  Object.keys(parsed.customHeaders).length > 0
                    ? JSON.stringify(parsed.customHeaders, null, 2)
                    : "",
                );
              }
              setUseRawEditor((v) => !v);
            }}
          >
            {useRawEditor
              ? t("provider.kimicode.useForm", { defaultValue: "表单编辑" })
              : t("provider.kimicode.useRaw", {
                  defaultValue: "高级 TOML 编辑",
                })}
          </Button>
        </div>

        {useRawEditor ? (
          <JsonEditor
            value={rawConfig}
            onChange={setRawConfig}
            darkMode={isDarkMode}
            language="text"
            height="280px"
          />
        ) : (
          <div className="space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <div>
                <FormLabel>
                  {t("provider.kimicode.providerId", {
                    defaultValue: "Provider ID",
                  })}
                </FormLabel>
                <Input
                  value={providerKey}
                  onChange={(e) => setProviderKey(e.target.value)}
                  placeholder="my-openai"
                />
              </div>
              <div>
                <FormLabel>
                  {t("provider.kimicode.providerType", {
                    defaultValue: "协议类型",
                  })}
                </FormLabel>
                <Select
                  value={providerType}
                  onValueChange={(v) => setProviderType(v as KimiProviderType)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {KIMI_PROVIDER_TYPES.map((type) => (
                      <SelectItem key={type} value={type}>
                        {type}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div>
              <FormLabel>Base URL</FormLabel>
              <Input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="https://api.openai.com/v1"
              />
            </div>

            <div>
              <FormLabel>API Key</FormLabel>
              <Input
                type="password"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                placeholder="sk-..."
              />
            </div>

            <div>
              <FormLabel>
                {t("provider.kimicode.customHeaders", {
                  defaultValue: "自定义 Headers (JSON)",
                })}
              </FormLabel>
              <Input
                value={headersText}
                onChange={(e) => setHeadersText(e.target.value)}
                placeholder='{"X-Custom":"value"}'
              />
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between">
                <FormLabel>
                  {t("provider.kimicode.models", { defaultValue: "模型列表" })}
                </FormLabel>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() =>
                    setModels((prev) => [
                      ...prev,
                      {
                        alias: `${providerKey || "custom"}/model-${prev.length + 1}`,
                        model: "model-id",
                        maxContextSize: 128000,
                      },
                    ])
                  }
                >
                  <Plus className="h-4 w-4 mr-1" />
                  {t("common.add", { defaultValue: "添加" })}
                </Button>
              </div>

              {models.map((model, index) => (
                <div
                  key={index}
                  className="grid grid-cols-12 gap-2 items-end border rounded-md p-2"
                >
                  <div className="col-span-3">
                    <FormLabel className="text-xs">Alias</FormLabel>
                    <Input
                      value={model.alias}
                      onChange={(e) =>
                        updateModel(index, { alias: e.target.value })
                      }
                    />
                  </div>
                  <div className="col-span-3">
                    <FormLabel className="text-xs">Model ID</FormLabel>
                    <Input
                      value={model.model}
                      onChange={(e) =>
                        updateModel(index, { model: e.target.value })
                      }
                    />
                  </div>
                  <div className="col-span-2">
                    <FormLabel className="text-xs">Context</FormLabel>
                    <Input
                      type="number"
                      value={model.maxContextSize}
                      onChange={(e) =>
                        updateModel(index, {
                          maxContextSize: Number(e.target.value) || 1,
                        })
                      }
                    />
                  </div>
                  <div className="col-span-2">
                    <FormLabel className="text-xs">Output</FormLabel>
                    <Input
                      type="number"
                      value={model.maxOutputSize ?? ""}
                      onChange={(e) =>
                        updateModel(index, {
                          maxOutputSize: e.target.value
                            ? Number(e.target.value)
                            : undefined,
                        })
                      }
                    />
                  </div>
                  <div className="col-span-2 flex gap-1">
                    <Button
                      type="button"
                      size="sm"
                      variant={
                        selectedModel === model.alias ? "default" : "outline"
                      }
                      onClick={() => setSelectedModel(model.alias)}
                      title={t("provider.kimicode.selectModel", {
                        defaultValue: "设为选中模型",
                      })}
                    >
                      ★
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="ghost"
                      disabled={models.length <= 1}
                      onClick={() =>
                        setModels((prev) => prev.filter((_, i) => i !== index))
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {showButtons && (
          <div className="flex justify-end gap-2 pt-2">
            {onCancel && (
              <Button type="button" variant="outline" onClick={onCancel}>
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
            )}
            <Button type="submit" disabled={form.formState.isSubmitting}>
              {submitLabel || t("common.save", { defaultValue: "保存" })}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
