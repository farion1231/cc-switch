import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { FetchedModel } from "@/lib/api/model-fetch";

interface FetchedModelPickerProps {
  models: FetchedModel[];
  configuredModelIds: string[];
  onAdd: (modelIds: string[]) => void;
}

export function FetchedModelPicker({
  models,
  configuredModelIds,
  onAdd,
}: FetchedModelPickerProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const configuredIds = new Set(configuredModelIds);
  const query = search.trim().toLowerCase();
  const visibleModels = models.filter(
    (model) =>
      model.id.toLowerCase().includes(query) ||
      model.ownedBy?.toLowerCase().includes(query),
  );
  const selectedModels = models.filter(
    (model) => selectedIds.has(model.id) && !configuredIds.has(model.id),
  );

  return (
    <fieldset className="min-w-0 space-y-3 rounded-lg border border-border-default p-3">
      <legend className="px-1 text-sm font-medium">
        {t("providerForm.fetchedModelsTitle", {
          count: models.length,
          defaultValue: "Available models ({{count}})",
        })}
      </legend>
      <ImeSafeInput
        value={search}
        onValueChange={setSearch}
        onKeyDown={(event) => {
          // The picker lives inside the provider <form>; Enter here must not
          // trigger implicit submission.
          if (event.key === "Enter") event.preventDefault();
        }}
        aria-label={t("providerForm.searchModelPlaceholder", {
          defaultValue: "Search models...",
        })}
        placeholder={t("providerForm.searchModelPlaceholder", {
          defaultValue: "Search models...",
        })}
      />
      <ScrollArea className="h-48" type="auto">
        <div className="space-y-1 pr-3">
          {visibleModels.length === 0 && (
            <p className="py-4 text-center text-sm text-muted-foreground">
              {t("providerForm.searchModelEmpty", {
                defaultValue: "No matching models.",
              })}
            </p>
          )}
          {visibleModels.map((model) => {
            const isConfigured = configuredIds.has(model.id);
            return (
              <label
                key={model.id}
                className="flex items-center gap-2 rounded-md px-2 py-2 hover:bg-muted/50"
              >
                <Checkbox
                  aria-label={model.id}
                  checked={isConfigured || selectedIds.has(model.id)}
                  disabled={isConfigured}
                  onCheckedChange={(checked) =>
                    setSelectedIds((previous) => {
                      const next = new Set(previous);
                      if (checked) next.add(model.id);
                      else next.delete(model.id);
                      return next;
                    })
                  }
                  className="shrink-0"
                />
                <span className="min-w-0 flex-1 break-all text-sm">
                  {model.id}
                  {model.ownedBy && (
                    <span className="block text-xs text-muted-foreground">
                      {model.ownedBy}
                    </span>
                  )}
                </span>
                {isConfigured && (
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {t("providerForm.modelAlreadyAdded", {
                      defaultValue: "Already added",
                    })}
                  </span>
                )}
              </label>
            );
          })}
        </div>
      </ScrollArea>
      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          disabled={selectedModels.length === 0}
          onClick={() => {
            onAdd(selectedModels.map((model) => model.id));
            setSelectedIds(new Set());
          }}
        >
          {t("providerForm.addSelectedModels", {
            count: selectedModels.length,
            defaultValue: "Add selected ({{count}})",
          })}
        </Button>
      </div>
    </fieldset>
  );
}
