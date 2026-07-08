import { useState, useEffect } from "react";
import type { ProviderCategory } from "@/types";

interface UseProviderCategoryProps {
  selectedPresetId: string | null;
  isEditMode: boolean;
  initialCategory?: ProviderCategory;
}

/**
 * 管理供应商类别状态
 * 预设供应商入口已移除；新增时固定走 custom，编辑时使用已有类别。
 */
export function useProviderCategory({
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
    setCategory(undefined);
  }, [selectedPresetId, isEditMode, initialCategory]);

  return { category, setCategory };
}
