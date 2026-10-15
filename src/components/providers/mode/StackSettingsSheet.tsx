import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { ProxyStackSubagentModel } from "@/types/proxy";
import { settingsApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  StackSubagentFields,
  stackSubagentSettings,
  stackSubagentValue,
  type StackSubagentValue,
} from "./StackSubagentFields";

interface StackSettingsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 能选的模型（后端 `StackView.subagentModels` 去掉默认那家） */
  models: ProxyStackSubagentModel[];
}

/**
 * Codex 聚合页签上的「聚合设置 ›」：子代理默认用的模型和推理强度，以及经典子代理开关。
 * 都是设备级设置，改了就保存；后端按合并目录校验再写进 config.toml（不在目录里就跟随主模型）。
 */
export function StackSettingsSheet({
  open,
  onOpenChange,
  models,
}: StackSettingsSheetProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [saving, setSaving] = useState(false);

  const save = async (value: StackSubagentValue) => {
    setSaving(true);
    try {
      const current = await settingsApi.get();
      await settingsApi.save({ ...current, ...stackSubagentSettings(value) });
    } catch (error) {
      console.error("[StackSettingsSheet] Failed to save", error);
      toast.error(
        t("mode.stackSettings.saveFailed", {
          error: extractErrorMessage(error),
        }),
      );
    } finally {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      // 经典开关会改合并目录：「Codex 还在用旧的模型列表」提示要重新算
      await queryClient.invalidateQueries({
        queryKey: ["providers", "codex", "stack"],
      });
      setSaving(false);
    }
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={420} closeLabel={t("common.close")}>
        <SheetHeader>
          <SheetTitle>{t("mode.stackSettings.title")}</SheetTitle>
          <SheetDescription>{APP_DISPLAY_NAME.codex}</SheetDescription>
        </SheetHeader>
        <SheetBody className="space-y-3">
          <p className="text-caption text-fg-2">
            {t("mode.stackSettings.subagentDescription")}
          </p>
          <StackSubagentFields
            models={models}
            value={stackSubagentValue(settings)}
            onChange={(value) => void save(value)}
            disabled={saving}
          />
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
