import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import { ArrowUpAZ, Search, Zap, Star, Layers, Settings2 } from "lucide-react";
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
  presetCategoryLabels: Record<string, string>,
  t: PresetTranslator,
): string {
  const presetCategory = entry.preset.category ?? "others";
  const categoryLabel =
    presetCategoryLabels[presetCategory] ?? String(t("providerPreset.other"));

  return [
    getPresetDisplayName(entry.preset, t),
    entry.preset.name,
    entry.preset.websiteUrl,
    categoryLabel,
  ]
    .join(" ")
    .toLowerCase();
}

export function filterPresetEntries(
  entries: PresetEntry[],
  query: string,
  presetCategoryLabels: Record<string, string>,
  t: PresetTranslator,
): PresetEntry[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return entries;
  }

  return entries.filter((entry) =>
    getPresetSearchText(entry, presetCategoryLabels, t).includes(
      normalizedQuery,
    ),
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
  presetCategoryLabels: Record<string, string>;
  t: PresetTranslator;
}

export function getVisiblePresetEntries(
  entries: PresetEntry[],
  options: PresetVisibilityOptions,
): PresetEntry[] {
  const { query, sortMode, presetCategoryLabels, t } = options;

  return sortPresetEntries(
    filterPresetEntries(entries, query, presetCategoryLabels, t),
    sortMode,
    t,
  );
}

interface ProviderPresetSelectorProps {
  selectedPresetId: string | null;
  presetEntries: PresetEntry[];
  presetCategoryLabels: Record<string, string>;
  onPresetChange: (value: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  category?: ProviderCategory; // 当前选中的分类
}

export function ProviderPresetSelector({
  selectedPresetId,
  presetEntries,
  presetCategoryLabels,
  onPresetChange,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  category,
}: ProviderPresetSelectorProps) {
  const { t } = useTranslation();
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortMode, setSortMode] = useState<PresetSortMode>(
    PresetSortMode.Original,
  );

  const visiblePresetEntries = useMemo(
    () =>
      getVisiblePresetEntries(presetEntries, {
        query: searchQuery,
        sortMode,
        presetCategoryLabels,
        t,
      }),
    [presetEntries, presetCategoryLabels, searchQuery, sortMode, t],
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

  const toggleSortMode = () => {
    setSortMode((current) =>
      current === PresetSortMode.Original
        ? PresetSortMode.NameAsc
        : PresetSortMode.Original,
    );
  };

  const renderPresetIcon = (preset: AnyPreset) => {
    const iconType = preset.theme?.icon;
    if (!iconType) return null;

    switch (iconType) {
      case "claude":
        return <ClaudeIcon size={14} />;
      case "codex":
        return <CodexIcon size={14} />;
      case "gemini":
        return <GeminiIcon size={14} />;
      case "generic":
        return <Zap size={14} />;
      default:
        return null;
    }
  };

  const getPresetButtonClass = (isSelected: boolean, preset: AnyPreset) => {
    const baseClass =
      "inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors";

    if (isSelected) {
      if (preset.theme?.backgroundColor) {
        return `${baseClass} text-white`;
      }
      return `${baseClass} bg-blue-500 text-white dark:bg-blue-600`;
    }

    return `${baseClass} bg-accent text-muted-foreground hover:bg-accent/80`;
  };

  const getPresetButtonStyle = (isSelected: boolean, preset: AnyPreset) => {
    if (!isSelected || !preset.theme?.backgroundColor) {
      return undefined;
    }

    return {
      backgroundColor: preset.theme.backgroundColor,
      color: preset.theme.textColor || "#FFFFFF",
    };
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <FormLabel>{t("providerPreset.label")}</FormLabel>
        <TooltipProvider delayDuration={300}>
          <div className="flex items-center gap-1">
            <Popover open={searchOpen} onOpenChange={setSearchOpen}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <PopoverTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("providerPreset.searchAriaLabel", {
                        defaultValue: "Search provider presets",
                      })}
                      className={
                        searchQuery.trim()
                          ? "size-8 bg-accent text-foreground"
                          : "size-8"
                      }
                    >
                      <Search className="size-4" />
                    </Button>
                  </PopoverTrigger>
                </TooltipTrigger>
                <TooltipContent>
                  {t("providerPreset.searchTooltip", {
                    defaultValue: "Search presets",
                  })}
                </TooltipContent>
              </Tooltip>
              <PopoverContent
                align="end"
                className="w-72 p-2 border-border-default"
              >
                <Input
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  placeholder={t("providerPreset.searchPlaceholder", {
                    defaultValue: "Search presets...",
                  })}
                  aria-label={t("providerPreset.searchAriaLabel", {
                    defaultValue: "Search provider presets",
                  })}
                  autoFocus
                />
              </PopoverContent>
            </Popover>

            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={t("providerPreset.sortAriaLabel", {
                    defaultValue: "Toggle preset sorting",
                  })}
                  aria-pressed={sortMode === PresetSortMode.NameAsc}
                  onClick={toggleSortMode}
                  className={
                    sortMode === PresetSortMode.NameAsc
                      ? "size-8 bg-accent text-foreground"
                      : "size-8"
                  }
                >
                  <ArrowUpAZ className="size-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {sortMode === PresetSortMode.NameAsc
                  ? t("providerPreset.sortOriginalTooltip", {
                      defaultValue: "Restore original order",
                    })
                  : t("providerPreset.sortNameAscTooltip", {
                      defaultValue: "Sort A-Z",
                    })}
              </TooltipContent>
            </Tooltip>
          </div>
        </TooltipProvider>
      </div>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => onPresetChange("custom")}
          className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
            selectedPresetId === "custom"
              ? "bg-blue-500 text-white dark:bg-blue-600"
              : "bg-accent text-muted-foreground hover:bg-accent/80"
          }`}
        >
          {t("providerPreset.custom")}
        </button>

        {visiblePresetEntries.length === 0 && (
          <div className="w-full rounded-md border border-dashed border-border-default px-3 py-2 text-xs text-muted-foreground">
            {t("providerPreset.noSearchResults", {
              defaultValue: "No matching presets.",
            })}
          </div>
        )}

        {visiblePresetEntries.map((entry) => {
          const isSelected = selectedPresetId === entry.id;
          const isPartner = entry.preset.isPartner;
          const presetCategory = entry.preset.category ?? "others";
          return (
            <button
              key={entry.id}
              type="button"
              onClick={() => onPresetChange(entry.id)}
              className={`${getPresetButtonClass(isSelected, entry.preset)} relative`}
              style={getPresetButtonStyle(isSelected, entry.preset)}
              title={
                presetCategoryLabels[presetCategory] ??
                t("providerPreset.other")
              }
            >
              {renderPresetIcon(entry.preset)}
              {getPresetDisplayName(entry.preset, t)}
              {isPartner && (
                <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-amber-500 to-yellow-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                  <Star className="h-2.5 w-2.5 fill-current" />
                </span>
              )}
            </button>
          );
        })}
      </div>

      {onUniversalPresetSelect && universalProviderPresets.length > 0 && (
        <>
          <div className="flex flex-wrap items-center gap-2">
            {universalProviderPresets.map((preset) => (
              <button
                key={`universal-${preset.providerType}`}
                type="button"
                onClick={() => onUniversalPresetSelect(preset)}
                className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80 relative"
                title={t("universalProvider.hint", {
                  defaultValue:
                    "跨应用统一配置，自动同步到 Claude/Codex/Gemini",
                })}
              >
                <ProviderIcon icon={preset.icon} name={preset.name} size={14} />
                {preset.name}
                <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-indigo-500 to-purple-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                  <Layers className="h-2.5 w-2.5" />
                </span>
              </button>
            ))}
            {onManageUniversalProviders && (
              <button
                type="button"
                onClick={onManageUniversalProviders}
                className="inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80"
                title={t("universalProvider.manage", {
                  defaultValue: "管理统一供应商",
                })}
              >
                <Settings2 className="h-4 w-4" />
                {t("universalProvider.manage", {
                  defaultValue: "管理",
                })}
              </button>
            )}
          </div>
        </>
      )}

      <p className="text-xs text-muted-foreground">{getCategoryHint()}</p>
    </div>
  );
}
