import { useCallback, useState } from "react";
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
import { zcodeProviderKinds } from "@/config/zcodeProviderPresets";
import type { ProviderCategory, ZCodeModel } from "@/types";
import { ApiKeySection } from "./shared";

interface ZCodeFormFieldsProps {
  kind: string;
  onKindChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  models: Record<string, ZCodeModel>;
  onModelsChange: (models: Record<string, ZCodeModel>) => void;
  extraOptions: Record<string, string>;
  onExtraOptionsChange: (options: Record<string, string>) => void;
}

function addModel(models: Record<string, ZCodeModel>) {
  let index = Object.keys(models).length + 1;
  let id = `model-${index}`;
  while (models[id]) {
    index += 1;
    id = `model-${index}`;
  }
  return { ...models, [id]: { name: id } };
}

export function ZCodeFormFields({
  kind,
  onKindChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  models,
  onModelsChange,
  extraOptions,
  onExtraOptionsChange,
}: ZCodeFormFieldsProps) {
  const { t } = useTranslation();
  const [newOptionKey, setNewOptionKey] = useState("");

  const renameModel = useCallback(
    (oldId: string, newId: string) => {
      const trimmed = newId.trim();
      if (!trimmed || trimmed === oldId || models[trimmed]) return;
      const next: Record<string, ZCodeModel> = {};
      for (const [id, model] of Object.entries(models)) {
        next[id === oldId ? trimmed : id] =
          id === oldId ? { ...model, name: trimmed } : model;
      }
      onModelsChange(next);
    },
    [models, onModelsChange],
  );

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <FormLabel>
          {t("zcode.kind", { defaultValue: "Provider Kind" })}
        </FormLabel>
        <Select value={kind} onValueChange={onKindChange}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {zcodeProviderKinds.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
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

      <div className="space-y-2">
        <FormLabel>{t("providerForm.baseUrl")}</FormLabel>
        <Input
          value={baseUrl}
          onChange={(event) => onBaseUrlChange(event.target.value)}
          placeholder="https://api.example.com/v1"
        />
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.models", { defaultValue: "Models" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onModelsChange(addModel(models))}
          >
            <Plus className="mr-2 h-4 w-4" />
            {t("common.add", { defaultValue: "Add" })}
          </Button>
        </div>
        <div className="space-y-2">
          {Object.entries(models).map(([modelId, model]) => (
            <div key={modelId} className="flex gap-2">
              <Input
                value={modelId}
                onChange={(event) => renameModel(modelId, event.target.value)}
                placeholder="model-id"
              />
              <Input
                value={model.name || ""}
                onChange={(event) =>
                  onModelsChange({
                    ...models,
                    [modelId]: { ...model, name: event.target.value },
                  })
                }
                placeholder={t("providerForm.modelDisplayName", {
                  defaultValue: "Display name",
                })}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={() => {
                  const next = { ...models };
                  delete next[modelId];
                  onModelsChange(next);
                }}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </div>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.extraOptions", { defaultValue: "Extra Options" })}
          </FormLabel>
          <div className="flex gap-2">
            <Input
              value={newOptionKey}
              onChange={(event) => setNewOptionKey(event.target.value)}
              placeholder="optionKey"
              className="h-8 w-36"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => {
                const key = newOptionKey.trim();
                if (!key) return;
                onExtraOptionsChange({ ...extraOptions, [key]: "" });
                setNewOptionKey("");
              }}
            >
              <Plus className="mr-2 h-4 w-4" />
              {t("common.add", { defaultValue: "Add" })}
            </Button>
          </div>
        </div>
        {Object.entries(extraOptions).map(([key, value]) => (
          <div key={key} className="flex gap-2">
            <Input value={key} readOnly className="w-40" />
            <Input
              value={value}
              onChange={(event) =>
                onExtraOptionsChange({
                  ...extraOptions,
                  [key]: event.target.value,
                })
              }
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={() => {
                const next = { ...extraOptions };
                delete next[key];
                onExtraOptionsChange(next);
              }}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
