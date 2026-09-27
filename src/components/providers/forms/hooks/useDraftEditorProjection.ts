import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { providersApi, type AppId } from "@/lib/api";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 新增对话框（Codex、Gemini CLI、Grok Build）：把预设或模板投影到当前配置文件上显示，和
 * 编辑器同一套规则（投影在后端算）。投影结果交给 `onEditorBaseChange`，保存时作为三方
 * 比较的底；投影进行中或失败时为 `null`，保存退回只存供应商。
 */
export function useDraftEditorProjection(
  appId: AppId,
  onEditorBaseChange?: (base: Record<string, unknown> | null) => void,
) {
  const { t } = useTranslation();
  // 连续切换预设时只认最后一次请求。
  const sequence = useRef(0);

  useEffect(
    () => () => {
      sequence.current += 1;
    },
    [],
  );

  const projectDraft = useCallback(
    (
      settings: Record<string, unknown>,
      category: string | undefined,
      apply: (shown: Record<string, unknown>) => void,
    ) => {
      if (!onEditorBaseChange) return;
      const current = ++sequence.current;
      onEditorBaseChange(null);
      providersApi
        .getEditorView(appId, settings, category)
        .then((view) => {
          if (current !== sequence.current) return;
          apply(view.settings);
          onEditorBaseChange(view.settings);
        })
        .catch((error: unknown) => {
          if (current !== sequence.current) return;
          toast.error(
            t("provider.editorViewFailed", {
              defaultValue:
                "无法读取客户端配置文件，下面显示的是保存的供应商配置：{{error}}",
              error: extractErrorMessage(error),
            }),
          );
        });
    },
    [appId, onEditorBaseChange, t],
  );

  /** 不再需要投影（比如切到了没有配置编辑框的官方卡）：作废进行中的请求。 */
  const clearDraftProjection = useCallback(() => {
    if (!onEditorBaseChange) return;
    sequence.current += 1;
    onEditorBaseChange(null);
  }, [onEditorBaseChange]);

  return { projectDraft, clearDraftProjection };
}
