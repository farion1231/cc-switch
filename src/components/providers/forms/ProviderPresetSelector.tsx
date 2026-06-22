import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import {
  Zap,
  Star,
  Heart,
  Layers,
  Settings2,
  Check,
  ChevronsUpDown,
} from "lucide-react";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import type { OpenCodeProviderPreset } from "@/config/opencodeProviderPresets";
import type { OpenClawProviderPreset } from "@/config/openclawProviderPresets";
import type { HermesProviderPreset } from "@/config/hermesProviderPresets";
import type { ProviderCategory } from "@/types";
import {
  universalProviderPresets,
  type UniversalProviderPreset,
} from "@/config/universalProviderPresets";
import { ProviderIcon } from "@/components/ProviderIcon";
import { cn } from "@/lib/utils";

type PresetTranslator = (key: string) => unknown;

export const PresetSortMode = {
  Original: "original",
  NameAsc: "nameAsc",
} as const;

export type PresetSortMode =
  (typeof PresetSortMode)[keyof typeof PresetSortMode];

export type AnyPreset =
  | ProviderPreset
  | CodexProviderPreset
  | GeminiProviderPreset
  | ClaudeDesktopProviderPreset
  | OpenCodeProviderPreset
  | OpenClawProviderPreset
  | HermesProviderPreset;

export type PresetEntry = {
  id: string;
  preset: AnyPreset;
};

export function getPresetDisplayName(
  preset: AnyPreset,
  t: PresetTranslator,
): string {
  return preset.nameKey ? String(t(preset.nameKey)) : preset.name;
}

export function getPresetSearchText(
  entry: PresetEntry,
  t: PresetTranslator,
): string {
  return [getPresetDisplayName(entry.preset, t), entry.preset.name]
    .join(" ")
    .toLowerCase();
}

export function filterPresetEntries(
  entries: PresetEntry[],
  query: string,
  t: PresetTranslator,
): PresetEntry[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return entries;
  }

  return entries.filter((entry) =>
    getPresetSearchText(entry, t).includes(normalizedQuery),
  );
}

export function sortPresetEntries(
  entries: PresetEntry[],
  sortMode: PresetSortMode,
  t: PresetTranslator,
): PresetEntry[] {
  if (sortMode === PresetSortMode.Original) {
    return [...entries];
  }

  return [...entries].sort((a, b) =>
    getPresetDisplayName(a.preset, t).localeCompare(
      getPresetDisplayName(b.preset, t),
    ),
  );
}

export interface PresetVisibilityOptions {
  query: string;
  sortMode: PresetSortMode;
  t: PresetTranslator;
}

export function getVisiblePresetEntries(
  entries: PresetEntry[],
  options: PresetVisibilityOptions,
): PresetEntry[] {
  const { query, sortMode, t } = options;

  return sortPresetEntries(filterPresetEntries(entries, query, t), sortMode, t);
}

interface ProviderPresetSelectorProps {
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  presetCategoryLabels: Record<string, string>;
  onPresetChange: (value: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  category?: ProviderCategory; // currently selected category
}

const UNIVERSAL_VALUE_PREFIX = "universal-";
const MANAGE_VALUE = "__manage_universal__";

export function ProviderPresetSelector({
  selectedPresetId,
  presetEntries,
  presetCategoryLabels,
  onPresetChange,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  category,
}: Readonly<ProviderPresetSelectorProps>) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");

  // Always present the preset list alphabetically; filtering is handled here
  // (Command runs with shouldFilter disabled) to keep ordering deterministic.
  const visiblePresetEntries = useMemo(
    () =>
      getVisiblePresetEntries(presetEntries, {
        query: searchQuery,
        sortMode: PresetSortMode.NameAsc,
        t,
      }),
    [presetEntries, searchQuery, t],
  );

  const normalizedSearchQuery = searchQuery.trim().toLowerCase();
  const visibleUniversalProviderPresets = useMemo(() => {
    if (!normalizedSearchQuery) {
      return universalProviderPresets;
    }
    return universalProviderPresets.filter((preset) =>
      [preset.name, preset.providerType, preset.description ?? ""]
        .join(" ")
        .toLowerCase()
        .includes(normalizedSearchQuery),
    );
  }, [normalizedSearchQuery]);

  const showUniversal =
    !!onUniversalPresetSelect &&
    (visibleUniversalProviderPresets.length > 0 ||
      (!!onManageUniversalProviders && !normalizedSearchQuery));

  const selectedEntry = useMemo(
    () => presetEntries.find((entry) => entry.id === selectedPresetId) ?? null,
    [presetEntries, selectedPresetId],
  );

  const getCategoryHint = (): ReactNode => {
    switch (category) {
      case "official":
        return t("providerForm.officialHint", {
          defaultValue: "💡 官方供应商使用浏览器登录，无需配置 API Key",
        });
      case "cn_official":
        return t("providerForm.cnOfficialApiKeyHint", {
          defaultValue: "💡 国产官方供应商只需填写 API Key，请求地址已预设",
        });
      case "aggregator":
        return t("providerForm.aggregatorApiKeyHint", {
          defaultValue: "💡 聚合服务供应商只需填写 API Key 即可使用",
        });
      case "third_party":
        return t("providerForm.thirdPartyApiKeyHint", {
          defaultValue: "💡 第三方供应商需要填写 API Key 和请求地址",
        });
      case "custom":
        return t("providerForm.customApiKeyHint", {
          defaultValue: "💡 自定义配置需手动填写所有必要字段",
        });
      case "omo":
        return t("providerForm.omoHint", {
          defaultValue:
            "💡 OMO 配置管理 Agent 模型分配，兼容 oh-my-openagent.jsonc / oh-my-opencode.jsonc",
        });
      default:
        return t("providerPreset.hint", {
          defaultValue: "选择预设后可继续调整下方字段。",
        });
    }
  };

  const renderPresetIcon = (preset: AnyPreset) => {
    if (preset.icon) {
      return (
        <ProviderIcon
          icon={preset.icon}
          name={preset.name}
          color={preset.iconColor}
          size={16}
          className="flex-shrink-0"
        />
      );
    }

    const iconType = preset.theme?.icon;
    if (iconType) {
      switch (iconType) {
        case "claude":
          return <ClaudeIcon size={14} />;
        case "codex":
          return <CodexIcon size={14} />;
        case "gemini":
          return <GeminiIcon size={14} />;
        case "generic":
          return <Zap size={14} />;
      }
    }

    return <span className="inline-block w-4 h-4 flex-shrink-0" aria-hidden />;
  };

  const customLabel = t("providerPreset.custom");
  const placeholder = t("providerPreset.selectPlaceholder", {
    defaultValue: "Select a provider…",
  });

  const renderTriggerContent = () => {
    if (selectedEntry) {
      return (
        <span className="flex items-center gap-2 truncate">
          {renderPresetIcon(selectedEntry.preset)}
          <span className="truncate">
            {getPresetDisplayName(selectedEntry.preset, t)}
          </span>
        </span>
      );
    }

    if (selectedPresetId === "custom") {
      return (
        <span className="flex items-center gap-2 truncate">
          <span className="inline-block w-4 h-4 flex-shrink-0" aria-hidden />
          <span className="truncate">{customLabel}</span>
        </span>
      );
    }

    return (
      <span className="truncate text-muted-foreground">{placeholder}</span>
    );
  };

  const handleSelectPreset = (value: string) => {
    onPresetChange(value);
    setOpen(false);
  };

  return (
    <div className="space-y-3">
      <FormLabel>{t("providerPreset.label")}</FormLabel>
      <Popover modal open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            aria-expanded={open}
            aria-label={t("providerPreset.label")}
            className="flex w-full h-9 items-center justify-between gap-2 rounded-lg border border-border-default bg-background px-3 py-2 text-sm font-medium shadow-sm ring-offset-background focus:outline-none focus-visible:outline-none focus:ring-0 focus-visible:ring-0 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {renderTriggerContent()}
            <ChevronsUpDown className="h-4 w-4 shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          side="bottom"
          align="start"
          sideOffset={6}
          avoidCollisions
          collisionPadding={8}
          className="z-[1000] w-[var(--radix-popover-trigger-width)] p-0 border-border-default"
        >
          <Command shouldFilter={false}>
            <CommandInput
              value={searchQuery}
              onValueChange={setSearchQuery}
              placeholder={t("providerPreset.searchPlaceholder", {
                defaultValue: "Search presets...",
              })}
              aria-label={t("providerPreset.searchAriaLabel", {
                defaultValue: "Search provider presets",
              })}
            />
            <CommandList>
              <CommandGroup>
                <CommandItem
                  value="custom"
                  onSelect={() => handleSelectPreset("custom")}
                >
                  <Check
                    className={cn(
                      "mr-2 h-4 w-4 shrink-0",
                      selectedPresetId === "custom"
                        ? "opacity-100"
                        : "opacity-0",
                    )}
                  />
                  <span
                    className="inline-block w-4 h-4 flex-shrink-0"
                    aria-hidden
                  />
                  <span className="truncate">{customLabel}</span>
                </CommandItem>
              </CommandGroup>

              {normalizedSearchQuery &&
                visiblePresetEntries.length === 0 &&
                visibleUniversalProviderPresets.length === 0 && (
                  <div className="px-3 py-4 text-center text-xs text-muted-foreground">
                    {t("providerPreset.noSearchResults", {
                      defaultValue: "No matching presets.",
                    })}
                  </div>
                )}

              {visiblePresetEntries.length > 0 && (
                <CommandGroup>
                  {visiblePresetEntries.map((entry) => {
                    const isSelected = selectedPresetId === entry.id;
                    const isPartner = entry.preset.isPartner;
                    const isPrimePartner = entry.preset.primePartner;
                    const presetCategory = entry.preset.category ?? "others";
                    return (
                      <CommandItem
                        key={entry.id}
                        value={entry.id}
                        keywords={[getPresetDisplayName(entry.preset, t)]}
                        onSelect={() => handleSelectPreset(entry.id)}
                        title={
                          presetCategoryLabels[presetCategory] ??
                          t("providerPreset.other")
                        }
                      >
                        <Check
                          className={cn(
                            "mr-2 h-4 w-4 shrink-0",
                            isSelected ? "opacity-100" : "opacity-0",
                          )}
                        />
                        {renderPresetIcon(entry.preset)}
                        <span className="truncate">
                          {getPresetDisplayName(entry.preset, t)}
                        </span>
                        {isPrimePartner ? (
                          <Heart
                            className="ml-auto h-4 w-4 fill-amber-500 text-amber-500"
                            strokeWidth={0}
                            aria-hidden
                          />
                        ) : (
                          isPartner && (
                            <Star className="ml-auto h-3.5 w-3.5 fill-amber-500 text-amber-500" />
                          )
                        )}
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              )}

              {showUniversal && (
                <CommandGroup
                  heading={t("universalProvider.groupHeading", {
                    defaultValue: "Unified providers",
                  })}
                >
                  {visibleUniversalProviderPresets.map((preset) => (
                    <CommandItem
                      key={`${UNIVERSAL_VALUE_PREFIX}${preset.providerType}`}
                      value={`${UNIVERSAL_VALUE_PREFIX}${preset.providerType}`}
                      keywords={[preset.name]}
                      onSelect={() => {
                        onUniversalPresetSelect?.(preset);
                        setOpen(false);
                      }}
                      title={t("universalProvider.hint", {
                        defaultValue:
                          "跨应用统一配置，自动同步到 Claude/Codex/Gemini",
                      })}
                    >
                      <span
                        className="mr-2 inline-block w-4 h-4 shrink-0"
                        aria-hidden
                      />
                      <ProviderIcon
                        icon={preset.icon}
                        name={preset.name}
                        size={14}
                        className="flex-shrink-0"
                      />
                      <span className="truncate">{preset.name}</span>
                      <Layers className="ml-auto h-3.5 w-3.5 text-indigo-500" />
                    </CommandItem>
                  ))}
                  {onManageUniversalProviders && !normalizedSearchQuery && (
                    <CommandItem
                      value={MANAGE_VALUE}
                      onSelect={() => {
                        onManageUniversalProviders();
                        setOpen(false);
                      }}
                    >
                      <Settings2 className="mr-2 h-4 w-4 shrink-0" />
                      <span className="truncate">
                        {t("universalProvider.manage", {
                          defaultValue: "管理统一供应商",
                        })}
                      </span>
                    </CommandItem>
                  )}
                </CommandGroup>
              )}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      <p className="text-xs text-muted-foreground">{getCategoryHint()}</p>
    </div>
  );
}
