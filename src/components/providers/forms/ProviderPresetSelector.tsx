import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { ClaudeIcon, CodexIcon, GeminiIcon } from "@/components/BrandIcons";
import { Zap, Star, Layers, Settings2 } from "lucide-react";
import type { ProviderPreset } from "@/config/claudeProviderPresets";
import type { CodexProviderPreset } from "@/config/codexProviderPresets";
import type { GeminiProviderPreset } from "@/config/geminiProviderPresets";
import type { ProviderCategory } from "@/types";
import {
  universalProviderPresets,
  type UniversalProviderPreset,
} from "@/config/universalProviderPresets";
import { ProviderIcon } from "@/components/ProviderIcon";

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset;
};

interface ProviderPresetSelectorProps {
  selectedPresetId: string | null;
  groupedPresets: Record<string, PresetEntry[]>;
  categoryKeys: string[];
  presetCategoryLabels: Record<string, string>;
  onPresetChange: (value: string) => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  category?: ProviderCategory; // 当前选中的分类
}

export function ProviderPresetSelector({
  selectedPresetId,
  groupedPresets,
  categoryKeys,
  presetCategoryLabels,
  onPresetChange,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  category,
}: ProviderPresetSelectorProps) {
  const { t } = useTranslation();

  const getReadableTextColor = (backgroundColor: string) => {
    const normalized = backgroundColor.replace("#", "");
    const hex =
      normalized.length === 3
        ? normalized
            .split("")
            .map((char) => `${char}${char}`)
            .join("")
        : normalized;

    if (!/^[0-9a-fA-F]{6}$/.test(hex)) {
      return undefined;
    }

    const channel = (start: number) =>
      Number.parseInt(hex.slice(start, start + 2), 16);
    const [r, g, b] = [channel(0), channel(2), channel(4)];
    const toLinear = (value: number) => {
      const normalizedValue = value / 255;
      return normalizedValue <= 0.03928
        ? normalizedValue / 12.92
        : ((normalizedValue + 0.055) / 1.055) ** 2.4;
    };

    const luminance =
      0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);

    return luminance > 0.58 ? "#111827" : "#ffffff";
  };

  const getCategoryHint = (): React.ReactNode => {
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

  const renderPresetIcon = (
    preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset,
  ) => {
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

  const getPresetButtonClass = (
    isSelected: boolean,
    preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset,
  ) => {
    const baseClass =
      "inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors";

    if (isSelected) {
      if (preset.theme?.backgroundColor) {
        return baseClass;
      }
      return `${baseClass} bg-primary text-primary-foreground`;
    }

    return `${baseClass} bg-accent text-muted-foreground hover:bg-accent/80`;
  };

  const getPresetButtonStyle = (
    isSelected: boolean,
    preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset,
  ) => {
    if (!isSelected || !preset.theme?.backgroundColor) {
      return undefined;
    }

    return {
      backgroundColor: preset.theme.backgroundColor,
      color:
        preset.theme.textColor ??
        getReadableTextColor(preset.theme.backgroundColor),
    };
  };

  return (
    <div className="space-y-3">
      <FormLabel>{t("providerPreset.label")}</FormLabel>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={() => onPresetChange("custom")}
          className={`inline-flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
            selectedPresetId === "custom"
              ? "bg-primary text-primary-foreground"
              : "bg-accent text-muted-foreground hover:bg-accent/80"
          }`}
        >
          {t("providerPreset.custom")}
        </button>

        {categoryKeys.map((category) => {
          const entries = groupedPresets[category];
          if (!entries || entries.length === 0) return null;
          return entries.map((entry) => {
            const isSelected = selectedPresetId === entry.id;
            const isPartner = entry.preset.isPartner;
            return (
              <button
                key={entry.id}
                type="button"
                onClick={() => onPresetChange(entry.id)}
                className={`${getPresetButtonClass(isSelected, entry.preset)} relative`}
                style={getPresetButtonStyle(isSelected, entry.preset)}
                title={
                  presetCategoryLabels[category] ?? t("providerPreset.other")
                }
              >
                {renderPresetIcon(entry.preset)}
                {entry.preset.nameKey
                  ? t(entry.preset.nameKey)
                  : entry.preset.name}
                {isPartner && (
                  <span className="absolute -right-1 -top-1 flex items-center gap-0.5 rounded-full border border-accent/50 bg-accent px-1.5 py-0.5 text-[10px] font-bold text-accent-foreground shadow-sm">
                    <Star className="h-2.5 w-2.5 fill-current" />
                  </span>
                )}
              </button>
            );
          });
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
                <span className="absolute -right-1 -top-1 flex items-center gap-0.5 rounded-full border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-bold text-primary shadow-sm">
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
