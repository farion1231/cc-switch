import { useState, useEffect } from "react";
import type { ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";

interface UseProviderCategoryProps {
  appId: AppId;
  selectedPresetId: string | null;
  isEditMode: boolean;
  initialCategory?: ProviderCategory;
}

/**
 * 管理供应商类别状态
 * 根据选择的预设自动更新类别
 */
export function useProviderCategory({
  appId,
  selectedPresetId,
  isEditMode,
  initialCategory,
}: UseProviderCategoryProps) {
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    // 编辑模式：使用 initialCategory
    isEditMode ? initialCategory : undefined,
  );

  useEffect(() => {
    // 编辑模式：只在初始化时设置，后续不自动更新
    if (isEditMode) {
      setCategory(initialCategory);
      return;
    }

    if (selectedPresetId === "custom") {
      setCategory("custom");
      return;
    }

    if (!selectedPresetId) return;

    // 从预设 ID 提取索引
    const match = selectedPresetId.match(/^(claude|codex|gemini)-(\d+)$/);
    if (!match) return;

    const [, type, indexStr] = match;
    const index = parseInt(indexStr, 10);

    if (type === "codex" && appId === "codex") {
      const preset = codexProviderPresets[index];
      if (preset) {
        setCategory(
          preset.category || (preset.isOfficial ? "official" : undefined),
        );
      }
    } else if (type === "claude" && appId === "claude") {
      const preset = providerPresets[index];
      if (preset) {
        setCategory(
          preset.category || (preset.isOfficial ? "official" : undefined),
        );
      }
    } else if (type === "gemini" && appId === "gemini") {
      const preset = geminiProviderPresets[index];
      if (preset) {
        setCategory(preset.category || undefined);
      }
    }
  }, [appId, selectedPresetId, isEditMode, initialCategory]);

  return { category, setCategory };
}
