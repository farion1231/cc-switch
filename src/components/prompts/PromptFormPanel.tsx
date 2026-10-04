import React, { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DisabledReason } from "@/components/ui/help-tip";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetTitle,
} from "@/components/ui/sheet";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import type { Prompt, AppId } from "@/lib/api";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  MCODE_PROMPT_LIMIT,
  charCount,
  formatSize,
  promptFileName,
  utf8Bytes,
} from "./promptUtils";

interface PromptFormPanelProps {
  appId: AppId;
  editingId?: string;
  initialData?: Prompt;
  onSave: (id: string, prompt: Prompt) => Promise<void | boolean>;
  onClose: () => void;
  /** 编辑时左下角的「删除」；不传就不显示 */
  onDelete?: (id: string) => void;
}

/** 和 Input / Textarea 同一套外观（ui/input.tsx 的 fieldClass） */
export const promptFieldClass = fieldClass;

export function FieldError({ id, children }: { id: string; children: string }) {
  return (
    <span
      id={id}
      className="flex items-start gap-1 text-caption text-danger-text"
    >
      <CircleAlert
        aria-hidden="true"
        strokeWidth={1.5}
        className="mt-0.5 h-3.5 w-3.5 shrink-0"
      />
      <span>{children}</span>
    </span>
  );
}

/**
 * 添加 / 编辑提示词的抽屉（宽 560，PromptsEdit.dc.html）。
 * 启用中的那条：保存按钮写明会覆盖哪个文件；删除照样画出来但不能点，原因挂在按钮上。
 */
const PromptFormPanel: React.FC<PromptFormPanelProps> = ({
  appId,
  editingId,
  initialData,
  onSave,
  onClose,
  onDelete,
}) => {
  const { t } = useTranslation();
  const baseId = useId();
  const appName = APP_DISPLAY_NAME[appId];
  const fileName = promptFileName(appId);
  const isHermes = appId === "hermes";
  const isPi = appId === "pi";
  const isActive = Boolean(editingId && initialData?.enabled);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [content, setContent] = useState("");
  const [attempted, setAttempted] = useState(false);
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (initialData) {
      setName(initialData.name);
      setDescription(initialData.description || "");
      setContent(initialData.content);
    }
  }, [initialData]);

  const bodyBytes = utf8Bytes(content);
  const limit = appId === "mcode" ? MCODE_PROMPT_LIMIT : undefined;
  const overLimit = limit !== undefined && bodyBytes > limit;
  const nameError = !name.trim()
    ? t(isHermes ? "prompts.nameRequiredHermes" : "prompts.nameRequired")
    : "";
  const bodyError = overLimit
    ? t("prompts.mcodeTooLarge", { size: formatSize(bodyBytes) })
    : "";
  const showNameError = attempted && Boolean(nameError);
  const showBodyError = attempted && Boolean(bodyError);

  const handleSave = async () => {
    if (savingRef.current) return;
    if (nameError || bodyError) {
      setAttempted(true);
      (nameError ? nameRef : bodyRef).current?.focus();
      return;
    }

    savingRef.current = true;
    setSaving(true);
    try {
      const id = editingId || `prompt-${Date.now()}`;
      const timestamp = Math.floor(Date.now() / 1000);
      const prompt: Prompt = {
        id,
        name: name.trim(),
        description: description.trim() || undefined,
        content: isPi ? content : content.trim(),
        enabled: initialData?.enabled || false,
        createdAt: initialData?.createdAt || timestamp,
        updatedAt: timestamp,
      };
      const saved = await onSave(id, prompt);
      if (saved !== false) {
        onClose();
      }
    } catch {
      // Error handled by hook
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleClose = () => {
    if (!savingRef.current) onClose();
  };

  const title = editingId
    ? t("prompts.editTitle", { name: initialData?.name ?? "" })
    : t("prompts.addTitle", { appName });

  const submitLabel = !editingId
    ? t("prompts.addSubmit")
    : isActive
      ? isPi
        ? t("prompts.saveAndWritePi")
        : t("prompts.saveAndOverwrite", { file: fileName })
      : t("common.save");

  const countText = limit
    ? `${formatSize(bodyBytes)} / 32 KB`
    : t("prompts.charCount", {
        chars: charCount(content).toLocaleString(),
        size: formatSize(bodyBytes),
      });

  const nameHintId = `${baseId}-name-hint`;
  const bodyHintId = `${baseId}-body-hint`;
  const countId = `${baseId}-count`;

  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) handleClose();
      }}
    >
      <SheetContent
        width={560}
        closeLabel={t("common.close")}
        onEscapeKeyDown={(event) => {
          if (savingRef.current) event.preventDefault();
        }}
      >
        <div className="flex h-[52px] shrink-0 items-center border-b border-border pe-12 ps-6">
          <SheetTitle
            title={title}
            className="min-w-0 truncate text-title text-fg-1"
          >
            {title}
          </SheetTitle>
          <SheetDescription className="sr-only">
            {t("prompts.formDescription", { file: fileName })}
          </SheetDescription>
        </div>

        <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto scroll-stable overscroll-contain px-6 pb-6 pt-5">
          <div className="flex shrink-0 flex-col gap-1.5">
            <label
              htmlFor={`${baseId}-name`}
              className="text-body font-medium text-fg-1"
            >
              {t("prompts.name")}
              <span aria-hidden="true" className="ms-0.5 text-danger-text">
                *
              </span>
              <span className="sr-only">{t("prompts.requiredMark")}</span>
            </label>
            <input
              ref={nameRef}
              id={`${baseId}-name`}
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              disabled={saving}
              placeholder={t(
                isHermes
                  ? "prompts.namePlaceholderHermes"
                  : "prompts.namePlaceholder",
              )}
              aria-required="true"
              aria-invalid={showNameError}
              aria-describedby={showNameError ? nameHintId : undefined}
              autoComplete="off"
              className={cn(promptFieldClass, "h-8")}
            />
            {showNameError ? (
              <FieldError id={nameHintId}>{nameError}</FieldError>
            ) : null}
          </div>

          <div className="flex shrink-0 flex-col gap-1.5">
            <label
              htmlFor={`${baseId}-desc`}
              className="text-body font-medium text-fg-1"
            >
              {t("prompts.description")}
            </label>
            <input
              id={`${baseId}-desc`}
              type="text"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              disabled={saving}
              placeholder={t("prompts.descriptionPlaceholder")}
              autoComplete="off"
              className={cn(promptFieldClass, "h-8")}
            />
          </div>

          <div className="flex min-h-[220px] flex-1 flex-col gap-1.5">
            <label
              htmlFor={`${baseId}-body`}
              className="text-body font-medium text-fg-1"
            >
              {t("prompts.content")}
            </label>
            <textarea
              ref={bodyRef}
              id={`${baseId}-body`}
              value={content}
              onChange={(event) => setContent(event.target.value)}
              disabled={saving}
              spellCheck={false}
              placeholder={
                isHermes
                  ? t("prompts.contentPlaceholderHermes")
                  : t("prompts.contentPlaceholder", {
                      filename: fileName,
                      appName,
                    })
              }
              aria-invalid={showBodyError}
              aria-describedby={
                showBodyError ? `${bodyHintId} ${countId}` : countId
              }
              className={cn(
                promptFieldClass,
                "min-h-[180px] flex-1 resize-none px-3 py-2.5 font-mono text-caption leading-[18px]",
              )}
            />
            <div className="flex items-start gap-3">
              {showBodyError ? (
                <FieldError id={bodyHintId}>{bodyError}</FieldError>
              ) : null}
              <span
                id={countId}
                className={cn(
                  "ms-auto shrink-0 whitespace-nowrap text-caption tabular-nums",
                  overLimit ? "text-danger-text" : "text-fg-2",
                )}
              >
                {countText}
              </span>
            </div>
          </div>
        </div>

        <div className="flex h-14 shrink-0 items-center gap-2 border-t border-border px-6">
          {editingId && onDelete ? (
            <DisabledReason
              reason={isActive ? t("prompts.deleteBlockedReason") : undefined}
              side="top"
            >
              <Button
                type="button"
                variant="quiet"
                size="regular"
                disabled={saving}
                onClick={() => onDelete(editingId)}
                className={cn(
                  "-ms-2.5",
                  isActive ? "text-fg-3" : "text-danger-text",
                )}
              >
                {t("common.delete")}
              </Button>
            </DisabledReason>
          ) : null}
          <div className="flex-1" />
          <Button
            type="button"
            variant="neutral"
            size="regular"
            onClick={handleClose}
            disabled={saving}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="regular"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            {saving ? t("common.saving") : submitLabel}
          </Button>
        </div>
      </SheetContent>
    </Sheet>
  );
};

export default PromptFormPanel;
