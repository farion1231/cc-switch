import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import {
  ArrowUpAZ,
  Search,
  Zap,
  Star,
  Heart,
  Layers,
  Settings2,
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
    // 置顶优先级：官方分类 > 尊享合作伙伴（Kimi）> 其余原顺序。
    // 用分区拼接而非排序，确保每组内部各自的相对顺序都不变；
    // 排他条件保证「既是官方又是 prime」的预设只归入官方组、不被重复。
    const official = entries.filter(
      (entry) => entry.preset.category === "official",
    );
    const prime = entries.filter(
      (entry) =>
        entry.preset.category !== "official" && entry.preset.primePartner,
    );
    const rest = entries.filter(
      (entry) =>
        entry.preset.category !== "official" && !entry.preset.primePartner,
    );
    return [...official, ...prime, ...rest];
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
}: Readonly<ProviderPresetSelectorProps>) {
  const { t } = useTranslation();
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [sortMode, setSortMode] = useState<PresetSortMode>(
    PresetSortMode.Original,
  );
  const searchContainerRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);

  // 点击搜索区域外时收起并清空,对齐旧 Popover 的「点击外部关闭」行为
  useEffect(() => {
    if (!searchOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (
        searchContainerRef.current &&
        !searchContainerRef.current.contains(event.target as Node)
      ) {
        setSearchOpen(false);
        setSearchQuery("");
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [searchOpen]);

  // 键盘快捷键: Ctrl/Cmd+F 打开搜索并聚焦输入框。
  // 使用捕获阶段并阻止冒泡，避免背后 ProviderList 的同名快捷键被意外触发。
  // 首次打开靠 Input 的 autoFocus 聚焦；若搜索已打开（例如点击 preset 后焦点
  // 停在按钮上），setSearchOpen(true) 同值不会重渲染、autoFocus 不重触发，
  // 这里用 rAF 命令式地把焦点移回搜索框（不 select，避免吞掉随后输入的首字符）。
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        event.stopPropagation();
        setSearchOpen(true);
        requestAnimationFrame(() => searchInputRef.current?.focus());
      }
    };

    globalThis.addEventListener("keydown", handleKeyDown, true);
    return () => globalThis.removeEventListener("keydown", handleKeyDown, true);
  }, []);

  const visiblePresetEntries = useMemo(
    () =>
      getVisiblePresetEntries(presetEntries, {
        query: searchQuery,
        sortMode,
        t,
      }),
    [presetEntries, searchQuery, sortMode, t],
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

  const getPresetButtonClass = (isSelected: boolean, preset: AnyPreset) => {
    const baseClass =
      "inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors w-full";

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
    <div ref={searchContainerRef} className="space-y-3">
      <div className="flex items-center justify-between gap-2">
        <FormLabel>{t("providerPreset.label")}</FormLabel>
        <div className="flex items-center gap-2">
          {searchOpen && (
            <Input
              ref={searchInputRef}
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setSearchQuery("");
                  setSearchOpen(false);
                }
              }}
              placeholder={t("providerPreset.searchPlaceholder", {
                defaultValue: "Search presets...",
              })}
              aria-label={t("providerPreset.searchAriaLabel", {
                defaultValue: "Search provider presets",
              })}
              className="w-60 h-8"
              autoFocus
            />
          )}
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t("providerPreset.searchAriaLabel", {
              defaultValue: "Search provider presets",
            })}
            aria-pressed={searchOpen}
            onClick={() => {
              setSearchOpen((v) => !v);
              if (searchOpen) setSearchQuery("");
            }}
            title={t("providerPreset.searchTooltip", {
              defaultValue: "Search presets",
            })}
            className={
              searchOpen || searchQuery.trim()
                ? "size-8 bg-accent text-foreground"
                : "size-8"
            }
          >
            <Search className="size-4" />
          </Button>

          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label={t("providerPreset.sortAriaLabel", {
              defaultValue: "Toggle preset sorting",
            })}
            aria-pressed={sortMode === PresetSortMode.NameAsc}
            onClick={toggleSortMode}
            title={
              sortMode === PresetSortMode.NameAsc
                ? t("providerPreset.sortOriginalTooltip", {
                    defaultValue: "Restore original order",
                  })
                : t("providerPreset.sortNameAscTooltip", {
                    defaultValue: "Sort A-Z",
                  })
            }
            className={
              sortMode === PresetSortMode.NameAsc
                ? "size-8 bg-accent text-foreground"
                : "size-8"
            }
          >
            <ArrowUpAZ className="size-4" />
          </Button>
        </div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
        <button
          type="button"
          onClick={() => onPresetChange("custom")}
          className={`inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors w-full ${
            selectedPresetId === "custom"
              ? "bg-blue-500 text-white dark:bg-blue-600"
              : "bg-accent text-muted-foreground hover:bg-accent/80"
          }`}
        >
          <span className="inline-block w-4 h-4 flex-shrink-0" aria-hidden />
          <span className="truncate">{t("providerPreset.custom")}</span>
        </button>

        {visiblePresetEntries.length === 0 && (
          <div className="col-span-full rounded-md border border-dashed border-border-default px-3 py-2 text-xs text-muted-foreground">
            {t("providerPreset.noSearchResults", {
              defaultValue: "No matching presets.",
            })}
          </div>
        )}

        {visiblePresetEntries.map((entry) => {
          const isSelected = selectedPresetId === entry.id;
          const isPartner = entry.preset.isPartner;
          const isPrimePartner = entry.preset.primePartner;
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
              <span className="truncate">
                {getPresetDisplayName(entry.preset, t)}
              </span>
              {isPrimePartner ? (
                <Heart
                  className="absolute -top-1 -right-1 h-5 w-5 fill-amber-500 text-amber-500 drop-shadow-sm"
                  strokeWidth={0}
                  aria-hidden
                />
              ) : (
                isPartner && (
                  <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-amber-500 to-yellow-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                    <Star className="h-2.5 w-2.5 fill-current" />
                  </span>
                )
              )}
            </button>
          );
        })}
      </div>

      {onUniversalPresetSelect && universalProviderPresets.length > 0 && (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-2">
          {universalProviderPresets.map((preset) => (
            <button
              key={`universal-${preset.providerType}`}
              type="button"
              onClick={() => onUniversalPresetSelect(preset)}
              className="inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80 relative w-full"
              title={t("universalProvider.hint", {
                defaultValue: "跨应用统一配置，自动同步到 Claude/Codex/Gemini",
              })}
            >
              <ProviderIcon
                icon={preset.icon}
                name={preset.name}
                size={14}
                className="flex-shrink-0"
              />
              <span className="truncate">{preset.name}</span>
              <span className="absolute -top-1 -right-1 flex items-center gap-0.5 rounded-full bg-gradient-to-r from-indigo-500 to-purple-500 px-1.5 py-0.5 text-[10px] font-bold text-white shadow-md">
                <Layers className="h-2.5 w-2.5" />
              </span>
            </button>
          ))}
          {onManageUniversalProviders && (
            <button
              type="button"
              onClick={onManageUniversalProviders}
              className="inline-flex items-center justify-start gap-2 px-3 py-2 rounded-lg text-sm font-medium transition-colors bg-accent text-muted-foreground hover:bg-accent/80 w-full"
              title={t("universalProvider.manage", {
                defaultValue: "管理统一供应商",
              })}
            >
              <Settings2 className="h-4 w-4 flex-shrink-0" />
              <span className="truncate">
                {t("universalProvider.manage", {
                  defaultValue: "管理",
                })}
              </span>
            </button>
          )}
        </div>
      )}

      <p className="text-xs text-muted-foreground">{getCategoryHint()}</p>
    </div>
  );
}
