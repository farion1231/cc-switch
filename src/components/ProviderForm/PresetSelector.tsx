import React from "react";
import { Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { ProviderCategory } from "../../types";
import { ClaudeIcon, CodexIcon } from "../BrandIcons";

interface Preset {
  name: string;
  isOfficial?: boolean;
  category?: ProviderCategory;
}

interface PresetSelectorProps {
  title?: string;
  presets: Preset[];
  selectedIndex: number | null;
  onSelectPreset: (index: number) => void;
  onCustomClick: () => void;
  customLabel?: string;
  renderCustomDescription?: () => React.ReactNode; // 新增：自定义描述渲染
}

const PresetSelector: React.FC<PresetSelectorProps> = ({
  title = "选择配置类型",
  presets,
  selectedIndex,
  onSelectPreset,
  onCustomClick,
  customLabel = "自定义",
  renderCustomDescription,
}) => {
  const getButtonVariant = (index: number) => {
    const isSelected = selectedIndex === index;

    if (!isSelected) {
      return "outline";
    }

    // For selected items, we'll use custom styling via className
    // to maintain the brand colors for official providers
    return "default";
  };

  const getButtonClass = (index: number, preset?: Preset) => {
    const isSelected = selectedIndex === index;

    if (isSelected && (preset?.isOfficial || preset?.category === "official")) {
      // Codex 官方使用黑色背景
      if (preset?.name.includes("Codex")) {
        return "bg-gray-900 text-white hover:bg-gray-800 border-gray-900";
      }
      // Claude 官方使用品牌色背景
      return "bg-[#D97757] text-white hover:bg-[#B86548] border-[#D97757]";
    }

    return "";
  };

  const getDescription = () => {
    if (selectedIndex === -1) {
      // 如果提供了自定义描述渲染函数，使用它
      if (renderCustomDescription) {
        return renderCustomDescription();
      }
      return "手动配置供应商，需要填写完整的配置信息";
    }

    if (selectedIndex !== null && selectedIndex >= 0) {
      const preset = presets[selectedIndex];
      return preset?.isOfficial || preset?.category === "official"
        ? "官方登录，不需要填写 API Key"
        : "使用预设配置，只需填写 API Key";
    }

    return null;
  };

  return (
    <div className="space-y-4">
      <div>
        <Label className="text-base font-semibold mb-3">
          {title}
        </Label>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant={getButtonVariant(-1)}
            className={getButtonClass(-1)}
            onClick={onCustomClick}
          >
            {customLabel}
          </Button>
          {presets.map((preset, index) => (
            <Button
              key={index}
              type="button"
              variant={getButtonVariant(index)}
              className={getButtonClass(index, preset)}
              onClick={() => onSelectPreset(index)}
            >
              {(preset.isOfficial || preset.category === "official") && (
                <>
                  {preset.name.includes("Claude") ? (
                    <ClaudeIcon size={14} />
                  ) : preset.name.includes("Codex") ? (
                    <CodexIcon size={14} />
                  ) : (
                    <Zap size={14} />
                  )}
                </>
              )}
              {preset.name}
            </Button>
          ))}
        </div>
      </div>
      {getDescription() && (
        <div className="text-sm text-muted-foreground">
          {getDescription()}
        </div>
      )}
    </div>
  );
};

export default PresetSelector;
