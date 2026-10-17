import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "@/lib/toast";
import type { Settings } from "@/types";
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  StackSubagentFields,
  stackSubagentSettings,
  stackSubagentValue,
} from "./StackSubagentFields";

// Radix Select 不收空值：代表「不写覆盖、用 Codex 的默认审批模型」。
const CODEX_DEFAULT_REVIEWER = "__codex_default__";

const warningClass =
  "rounded-control bg-warning-soft px-3 py-2 text-caption text-warning-text";

interface StackSettingsSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** 能选的模型（后端 `StackView.subagentModels` 去掉默认那家） */
  models: ProxyStackSubagentModel[];
  /** 默认那家是官方：合并目录里有官方的 codex-auto-review，自动审批默认用它 */
  routeOfficial?: boolean;
}

/**
 * Codex 聚合页签上的「聚合设置 ›」：子代理默认用的模型和推理强度、经典子代理开关，以及
 * 自动审批（Auto Review）用的模型。都是设备级设置，改了就保存；后端按合并目录校验再写进
 * 客户端（不在目录里就用默认：子代理跟随主模型，审批用 Codex 的默认）。
 */
export function StackSettingsSheet({
  open,
  onOpenChange,
  models,
  routeOfficial = false,
}: StackSettingsSheetProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();
  const [saving, setSaving] = useState(false);

  const save = async (patch: Partial<Settings>) => {
    setSaving(true);
    try {
      const current = await settingsApi.get();
      await settingsApi.save({ ...current, ...patch });
    } catch (error) {
      console.error("[StackSettingsSheet] Failed to save", error);
      toast.error(
        t("mode.stackSettings.saveFailed", {
          error: extractErrorMessage(error),
        }),
      );
    } finally {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
      // 经典开关、审批模型会改合并目录：「Codex 还在用旧的模型列表」提示要重新算
      await queryClient.invalidateQueries({
        queryKey: ["providers", "codex", "stack"],
      });
      setSaving(false);
    }
  };

  const autoReviewModel = settings?.codexStackAutoReviewModel || undefined;
  const autoReviewSelected = models.find(
    (candidate) => candidate.id === autoReviewModel,
  );
  // 选过的模型不在名单里了：后端不写，用 Codex 的默认审批模型
  const autoReviewUnavailable =
    autoReviewModel !== undefined && autoReviewSelected === undefined;
  const autoReviewLabel = t("mode.stackSettings.autoReviewModel");

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
            onChange={(value) => void save(stackSubagentSettings(value))}
            disabled={saving}
          />

          <div className="space-y-1.5 border-t border-border pt-3">
            <label className="text-caption font-semibold text-fg-2">
              {autoReviewLabel}
            </label>
            <p className="text-caption text-fg-2">
              {t("mode.stackSettings.autoReviewDescription")}
            </p>
            <Select
              value={autoReviewModel ?? CODEX_DEFAULT_REVIEWER}
              onValueChange={(next) =>
                void save({
                  codexStackAutoReviewModel:
                    next === CODEX_DEFAULT_REVIEWER ? undefined : next,
                })
              }
              disabled={saving}
            >
              <SelectTrigger className="h-9" aria-label={autoReviewLabel}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="z-[70]">
                <SelectItem value={CODEX_DEFAULT_REVIEWER}>
                  {routeOfficial
                    ? t("mode.stackSettings.autoReviewDefaultOfficial")
                    : t("mode.stackSettings.autoReviewDefaultMain")}
                </SelectItem>
                {models.map((candidate) => (
                  <SelectItem key={candidate.id} value={candidate.id}>
                    {candidate.displayName}
                  </SelectItem>
                ))}
                {autoReviewUnavailable && (
                  <SelectItem value={autoReviewModel}>
                    {t("mode.stackSettings.unavailableOption", {
                      model: autoReviewModel,
                    })}
                  </SelectItem>
                )}
              </SelectContent>
            </Select>
            {autoReviewUnavailable && (
              <p
                data-testid="stack-auto-review-unavailable"
                className={warningClass}
              >
                {t("mode.stackSettings.autoReviewUnavailable")}
              </p>
            )}
          </div>
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
