import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import type { FetchedModel } from "@/lib/api/model-fetch";
import { matchesSearchQuery } from "@/utils/search";

export function ModelDropdown({
  models,
  onSelect,
}: {
  models: FetchedModel[];
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  const uniqueModels = useMemo(() => {
    const seen = new Set<string>();
    return models.filter((model) => {
      const id = model.id.trim();
      if (!id || seen.has(id)) return false;
      seen.add(id);
      return true;
    });
  }, [models]);

  const visibleModels = useMemo(
    () => uniqueModels.filter((model) => matchesSearchQuery(query, model.id)),
    [query, uniqueModels],
  );

  const groupedVisibleModels = useMemo(() => {
    const grouped: Record<string, FetchedModel[]> = {};
    for (const model of visibleModels) {
      const owner = model.ownedBy || "Other";
      if (!grouped[owner]) grouped[owner] = [];
      grouped[owner].push(model);
    }

    return Object.entries(grouped).sort(([a], [b]) => a.localeCompare(b));
  }, [visibleModels]);

  const handleOpenChange = (nextOpen: boolean) => {
    setOpen(nextOpen);
    if (!nextOpen) setQuery("");
  };

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="shrink-0"
          aria-label={t("providerForm.modelPickerAriaLabel", {
            defaultValue: "Choose a fetched model",
          })}
          title={t("providerForm.modelPickerTooltip", {
            defaultValue: "Choose fetched model",
          })}
        >
          <ChevronDown className="h-4 w-4" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        collisionPadding={8}
        className="z-[200] w-[min(24rem,calc(100vw-1rem))] p-0"
      >
        <Command
          shouldFilter={false}
          label={t("providerForm.modelSearchAriaLabel", {
            defaultValue: "Search model IDs",
          })}
        >
          <CommandInput
            value={query}
            onValueChange={setQuery}
            placeholder={t("providerForm.modelSearchPlaceholder", {
              defaultValue: "Search model IDs...",
            })}
            aria-label={t("providerForm.modelSearchAriaLabel", {
              defaultValue: "Search model IDs",
            })}
          />
          <CommandList>
            <CommandEmpty>
              {t("providerForm.noMatchingModels", {
                defaultValue: "No matching models.",
              })}
            </CommandEmpty>
            {groupedVisibleModels.map(([owner, ownerModels]) => (
              <CommandGroup key={owner} heading={owner}>
                {ownerModels.map((model) => (
                  <CommandItem
                    key={model.id}
                    value={model.id}
                    onSelect={() => {
                      onSelect(model.id);
                      handleOpenChange(false);
                    }}
                    title={model.id}
                    className="min-w-0"
                  >
                    <span className="min-w-0 truncate">{model.id}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
