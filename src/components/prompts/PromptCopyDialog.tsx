import { useEffect, useId, useState } from "react";
import { useQueries } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { promptsApi, type AppId, type Prompt } from "@/lib/api";
import { promptKeys } from "@/lib/query/prompts";
import { cn } from "@/lib/utils";
import { FieldError } from "./PromptFormPanel";

interface PromptCopyDialogProps {
  prompt: Prompt;
  /** 可以复制到的应用（已去掉当前应用） */
  targets: AppId[];
  pending?: boolean;
  onCancel: () => void;
  onConfirm: (apps: AppId[]) => void;
}

/**
 * 复制到其他应用（Prompts.dc.html 第 446–480 行）：各自存成独立的一条、不启用。
 * 默认勾选没有同名条目的应用；Hermes 的 SOUL.md 是人设，编码规范之类复制过去多半不对，默认不勾。
 */
export function PromptCopyDialog({
  prompt,
  targets,
  pending = false,
  onCancel,
  onConfirm,
}: PromptCopyDialogProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const lists = useQueries({
    queries: targets.map((app) => ({
      queryKey: promptKeys.list(app),
      queryFn: () => promptsApi.getPrompts(app),
    })),
  });
  const sameName = (index: number) =>
    Object.values(lists[index]?.data ?? {}).some(
      (item) => item.name === prompt.name,
    );
  const ready = lists.every((query) => !query.isPending);
  const [picks, setPicks] = useState<Partial<Record<AppId, boolean>> | null>(
    null,
  );
  const [attempted, setAttempted] = useState(false);

  useEffect(() => {
    if (!ready || picks) return;
    setPicks(
      Object.fromEntries(
        targets.map((app, index) => [
          app,
          !sameName(index) && app !== "hermes",
        ]),
      ),
    );
    // sameName 依赖 lists，只在第一次读完时定默认勾选
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, picks, targets]);

  const picked = targets.filter((app) => picks?.[app]);
  const showError = attempted && picked.length === 0;
  const errorId = `${baseId}-error`;

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !pending) onCancel();
      }}
    >
      <DialogContent
        zIndex="nested"
        className="flex w-[480px] max-w-[calc(100vw-32px)] flex-col gap-4 rounded-dialog border border-border bg-surface p-6 text-fg-1 shadow-v7-lg sm:rounded-dialog"
      >
        <div className="flex flex-col gap-1.5">
          <DialogTitle className="text-title [overflow-wrap:anywhere]">
            {t("prompts.copyDialog.title", { name: prompt.name })}
          </DialogTitle>
          <DialogDescription className="text-body text-fg-2">
            {t("prompts.copyDialog.lead")}
          </DialogDescription>
        </div>
        <fieldset
          aria-describedby={showError ? errorId : undefined}
          className="m-0 min-w-0 border-0 p-0"
        >
          <legend className="sr-only">{t("prompts.copyDialog.legend")}</legend>
          <ul className="m-0 list-none rounded-panel border border-border p-0">
            {targets.map((app, index) => {
              const inputId = `${baseId}-${app}`;
              const noteId = `${inputId}-note`;
              const note = sameName(index)
                ? t("prompts.copyDialog.sameName")
                : app === "hermes"
                  ? t("prompts.copyDialog.hermesNote")
                  : "";
              return (
                <li
                  key={app}
                  className={cn(
                    "flex h-10 items-center px-3.5",
                    index > 0 && "border-t border-border",
                  )}
                >
                  <label
                    htmlFor={inputId}
                    className="flex h-10 min-w-0 flex-1 cursor-pointer items-center gap-2.5"
                  >
                    <input
                      id={inputId}
                      type="checkbox"
                      checked={Boolean(picks?.[app])}
                      disabled={!picks || pending}
                      onChange={(event) => {
                        const checked = event.target.checked;
                        setPicks((current) => ({
                          ...(current ?? {}),
                          [app]: checked,
                        }));
                      }}
                      aria-describedby={note ? noteId : undefined}
                      className="ui-checkbox"
                    />
                    <AppGlyph app={app} size={16} badgeClassName="bg-surface" />
                    <span className="text-body">{APP_DISPLAY_NAME[app]}</span>
                    <span className="flex-1" />
                    {note ? (
                      <span
                        id={noteId}
                        className="whitespace-nowrap text-caption text-fg-2"
                      >
                        {note}
                      </span>
                    ) : null}
                  </label>
                </li>
              );
            })}
          </ul>
        </fieldset>
        {showError ? (
          <div className="-mt-2">
            <FieldError id={errorId}>
              {t("prompts.copyDialog.pickOne")}
            </FieldError>
          </div>
        ) : null}
        <div className="flex justify-end gap-2 pt-1">
          <Button
            variant="neutral"
            size="regular"
            autoFocus
            disabled={pending}
            onClick={onCancel}
          >
            {t("common.cancel")}
          </Button>
          <Button
            variant="solid"
            size="regular"
            disabled={pending || !picks}
            onClick={() => {
              if (picked.length === 0) {
                setAttempted(true);
                return;
              }
              onConfirm(picked);
            }}
          >
            {picked.length
              ? t("prompts.copyDialog.go", { count: picked.length })
              : t("prompts.copyDialog.goNone")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
