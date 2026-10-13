import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { ApiKeySection } from "./shared";
import { Plus, Trash2 } from "lucide-react";
import type { ProviderCategory, CodexCatalogModel } from "@/types";

interface CursorFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  model: string;
  onModelChange: (model: string) => void;
  catalogModels?: CodexCatalogModel[];
  onCatalogModelsChange?: (models: CodexCatalogModel[]) => void;
}

export function CursorFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  model,
  onModelChange,
  catalogModels = [],
  onCatalogModelsChange,
}: CursorFormFieldsProps) {
  const { t } = useTranslation();

  const canEditCatalog = Boolean(onCatalogModelsChange);

  const handleAddCatalogRow = () => {
    if (!onCatalogModelsChange) return;
    onCatalogModelsChange([
      ...catalogModels,
      { model: "", displayName: "", contextWindow: "" },
    ]);
  };

  const handleUpdateCatalogRow = (
    index: number,
    patch: Partial<CodexCatalogModel>,
  ) => {
    if (!onCatalogModelsChange) return;
    const updated = catalogModels.map((m, i) =>
      i === index ? { ...m, ...patch } : m,
    );
    onCatalogModelsChange(updated);
  };

  const handleRemoveCatalogRow = (index: number) => {
    if (!onCatalogModelsChange) return;
    onCatalogModelsChange(catalogModels.filter((_, i) => i !== index));
  };

  return (
    <>
      <div className="space-y-2">
        <FormLabel htmlFor="cursor-baseurl">
          {t("cursor.form.baseUrl", { defaultValue: "API Endpoint" })}
        </FormLabel>
        <Input
          id="cursor-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("cursor.form.baseUrlHint", {
            defaultValue: "The API endpoint URL for this provider.",
          })}
        </p>
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
        <FormLabel htmlFor="cursor-model">
          {t("cursor.form.model", { defaultValue: "Default Model (optional)" })}
        </FormLabel>
        <Input
          id="cursor-model"
          value={model}
          onChange={(e) => onModelChange(e.target.value)}
          placeholder={t("cursor.form.modelPlaceholder", {
            defaultValue: "gpt-4o, claude-sonnet-4-20250514, ...",
          })}
        />
        <p className="text-xs text-muted-foreground">
          {t("cursor.form.modelHint", {
            defaultValue:
              "Optional. Default upstream model. When Cursor sends a model name not in the mapping below, this model is used instead.",
          })}
        </p>
      </div>

      {/* Model Mapping Section */}
      {canEditCatalog && (
        <div className="space-y-3 border-t border-border-default pt-3">
          <div className="space-y-1">
            <div className="flex items-center justify-between gap-3">
              <FormLabel>
                {t("cursor.form.modelMappingTitle", {
                  defaultValue: "Model Mapping",
                })}
              </FormLabel>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddCatalogRow}
                className="h-7 gap-1"
              >
                <Plus className="h-3.5 w-3.5" />
                {t("cursor.form.addMapping", {
                  defaultValue: "Add Mapping",
                })}
              </Button>
            </div>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t("cursor.form.modelMappingHint", {
                defaultValue:
                  "Map Cursor model names (e.g. composer-2.5, claude-sonnet-4-20250514) to upstream model names (e.g. deepseek-v4-pro). When Cursor requests a model listed here, it passes through unchanged. Other models use the default model above.",
              })}
            </p>
          </div>

          {catalogModels.length > 0 && (
            <div className="space-y-2">
              {/* Column headers */}
              <div className="hidden grid-cols-[1fr_1fr_36px] gap-2 px-1 text-xs font-medium text-muted-foreground md:grid">
                <span>
                  {t("cursor.form.mappingColumnCursor", {
                    defaultValue: "Cursor Model Name",
                  })}
                </span>
                <span>
                  {t("cursor.form.mappingColumnUpstream", {
                    defaultValue: "Upstream Model Name",
                  })}
                </span>
                <span />
              </div>

              {catalogModels.map((row, index) => (
                <div
                  key={index}
                  className="grid grid-cols-1 gap-2 md:grid-cols-[1fr_1fr_36px]"
                >
                  <Input
                    value={row.model}
                    onChange={(event) =>
                      handleUpdateCatalogRow(index, {
                        model: event.target.value,
                      })
                    }
                    placeholder={t("cursor.form.mappingCursorPlaceholder", {
                      defaultValue: "e.g. composer-2.5",
                    })}
                    aria-label={t("cursor.form.mappingColumnCursor", {
                      defaultValue: "Cursor Model Name",
                    })}
                  />
                  <Input
                    value={row.displayName ?? ""}
                    onChange={(event) =>
                      handleUpdateCatalogRow(index, {
                        displayName: event.target.value,
                      })
                    }
                    placeholder={t("cursor.form.mappingUpstreamPlaceholder", {
                      defaultValue: "e.g. deepseek-v4-pro",
                    })}
                    aria-label={t("cursor.form.mappingColumnUpstream", {
                      defaultValue: "Upstream Model Name",
                    })}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-9 w-9 text-muted-foreground hover:text-destructive"
                    onClick={() => handleRemoveCatalogRow(index)}
                    aria-label={t("cursor.form.removeMapping", {
                      defaultValue: "Remove mapping",
                    })}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );
}
