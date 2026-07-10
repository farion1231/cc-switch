import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiKeySection } from "./shared";
import type { OpenClawModel, ProviderCategory } from "@/types";

interface PiFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  api: string;
  onApiChange: (value: string) => void;
  models: OpenClawModel[];
  onModelsChange: (models: OpenClawModel[]) => void;
  defaultModel: string;
  onDefaultModelChange: (model: string) => void;
}

const PI_API_PROTOCOLS = [
  { value: "openai-chat", label: "OpenAI Chat Completions" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
];

const createModelKey = () => crypto.randomUUID();

export function PiFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  api,
  onApiChange,
  models,
  onModelsChange,
  defaultModel,
  onDefaultModelChange,
}: PiFormFieldsProps) {
  const { t } = useTranslation();
  const [modelKeys, setModelKeys] = useState<string[]>(() =>
    models.map(() => createModelKey()),
  );

  useEffect(() => {
    setModelKeys((current) => {
      if (current.length === models.length) return current;
      if (current.length > models.length)
        return current.slice(0, models.length);
      return [
        ...current,
        ...Array.from({ length: models.length - current.length }, () =>
          createModelKey(),
        ),
      ];
    });
  }, [models.length]);

  const addModel = () => {
    setModelKeys((current) => [...current, createModelKey()]);
    onModelsChange([...models, { id: "", name: "" }]);
  };

  const updateModel = (
    index: number,
    field: keyof OpenClawModel,
    value: string,
  ) => {
    const next = [...models];
    next[index] = { ...next[index], [field]: value };
    onModelsChange(next);
  };

  const removeModel = (index: number) => {
    setModelKeys((current) =>
      current.filter((_, keyIndex) => keyIndex !== index),
    );
    const next = [...models];
    next.splice(index, 1);
    onModelsChange(next);
  };

  const selectableModelIds = Array.from(
    new Set(
      models
        .map((model) => model.id?.trim())
        .filter((id): id is string => !!id),
    ),
  );

  return (
    <>
      <div className="space-y-2">
        <FormLabel htmlFor="pi-api">
          {t("pi.apiProtocol", { defaultValue: "API 协议" })}
        </FormLabel>
        <Select value={api} onValueChange={onApiChange}>
          <SelectTrigger id="pi-api">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PI_API_PROTOCOLS.map((protocol) => (
              <SelectItem key={protocol.value} value={protocol.value}>
                {protocol.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="pi-baseurl">
          {t("pi.baseUrl", { defaultValue: "API 端点" })}
        </FormLabel>
        <Input
          id="pi-baseurl"
          value={baseUrl}
          onChange={(event) => onBaseUrlChange(event.target.value)}
          placeholder="https://api.example.com/v1"
        />
      </div>

      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <div className="space-y-2">
        <FormLabel htmlFor="pi-default-model">
          {t("pi.defaultModel", { defaultValue: "默认模型" })}
        </FormLabel>
        <Select value={defaultModel} onValueChange={onDefaultModelChange}>
          <SelectTrigger id="pi-default-model">
            <SelectValue
              placeholder={t("pi.defaultModelPlaceholder", {
                defaultValue: "选择默认模型",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {selectableModelIds.map((id) => (
              <SelectItem key={id} value={id}>
                {id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>{t("pi.models", { defaultValue: "模型列表" })}</FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={addModel}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("pi.addModel", { defaultValue: "添加模型" })}
          </Button>
        </div>

        <div className="space-y-2">
          {models.map((model, index) => (
            <div
              key={modelKeys[index] ?? `${model.id}-${index}`}
              className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] items-end gap-2"
            >
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">
                  {t("pi.modelId", { defaultValue: "模型 ID" })}
                </label>
                <Input
                  value={model.id}
                  onChange={(event) =>
                    updateModel(index, "id", event.target.value)
                  }
                  placeholder="gpt-5.5"
                />
              </div>
              <div className="space-y-1">
                <label className="text-xs text-muted-foreground">
                  {t("pi.modelName", { defaultValue: "显示名称" })}
                </label>
                <Input
                  value={model.name}
                  onChange={(event) =>
                    updateModel(index, "name", event.target.value)
                  }
                  placeholder="GPT 5.5"
                />
              </div>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={() => removeModel(index)}
                className="h-9 w-9 text-muted-foreground hover:text-destructive"
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </div>
      </div>
    </>
  );
}
