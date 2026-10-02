import * as React from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, CircleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  SegmentThumb,
  useSlidingIndicator,
} from "@/components/ui/sliding-indicator";
import { fieldClass } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/** v7 对话框外壳：宽 480（或传入）、padding 24、圆角 14。 */
export function V7Dialog({
  open,
  onOpenChange,
  width = 480,
  className,
  children,
  describedBy,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  width?: number;
  className?: string;
  children: React.ReactNode;
  /** 没有 DialogDescription 时传 undefined 关掉 Radix 的提示 */
  describedBy?: string;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {open && (
        <DialogContent
          zIndex="alert"
          aria-describedby={describedBy}
          style={{ maxWidth: width }}
          className={cn(
            "max-h-[calc(100vh-48px)] gap-4 rounded-dialog border-border bg-surface p-6 text-fg-1 shadow-v7-lg sm:rounded-dialog",
            className,
          )}
        >
          {children}
        </DialogContent>
      )}
    </Dialog>
  );
}

/** 确认框：后果写在正文里，取消默认聚焦；不可撤销的操作（danger）确认键用红底，可恢复的传 danger={false}。 */
export function V7ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  danger = true,
  pending = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  body: React.ReactNode;
  confirmLabel: string;
  danger?: boolean;
  pending?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <V7Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !pending) onCancel();
      }}
    >
      <div className="flex flex-col gap-1.5">
        <DialogTitle className="text-section [overflow-wrap:anywhere]">
          {title}
        </DialogTitle>
        <DialogDescription className="text-body text-fg-2">
          {body}
        </DialogDescription>
      </div>
      <div className="flex flex-wrap justify-end gap-2 pt-1">
        <Button
          type="button"
          variant="neutral"
          size="regular"
          autoFocus
          disabled={pending}
          onClick={onCancel}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          variant={danger ? "destructive" : "solid"}
          size="regular"
          disabled={pending}
          onClick={onConfirm}
        >
          {confirmLabel}
        </Button>
      </div>
    </V7Dialog>
  );
}

/** MCP / Skills 抽屉和对话框里共用的小件（v7 表单字段：高 32、圆角 8、出错描边 --danger）。 */

/** 和 Input 同一套外观（ui/input.tsx 的 fieldClass），只读时换浅底 */
export const FIELD_CLASS = cn(
  fieldClass,
  "h-8 min-w-0 read-only:bg-subtle read-only:text-fg-2 dark:[color-scheme:dark]",
);

export const MONO_FIELD_CLASS = cn(FIELD_CLASS, "font-mono text-caption");

/** 原生勾选框 / 单选框的统一外观（index.css 的 ui-checkbox / ui-radio） */
export const CHECKBOX_CLASS = "ui-checkbox";
export const RADIO_CLASS = "ui-radio";

export const LABEL_CLASS = "text-body font-medium text-fg-1";

export function FieldError({
  id,
  children,
}: {
  id?: string;
  children: React.ReactNode;
}) {
  return (
    <span
      id={id}
      className="flex items-start gap-1 text-caption text-danger-text"
    >
      <CircleAlert
        aria-hidden="true"
        className="mt-0.5 h-3.5 w-3.5 shrink-0"
        strokeWidth={1.5}
      />
      <span>{children}</span>
    </span>
  );
}

export function RequiredMark({ srText }: { srText: string }) {
  return (
    <>
      <span aria-hidden="true" className="ms-0.5 text-danger-text">
        *
      </span>
      <span className="sr-only">{srText}</span>
    </>
  );
}

/** 折叠区的展开按钮（R 展开写法：aria-expanded + aria-controls） */
export function DisclosureButton({
  open,
  controls,
  onToggle,
  children,
  hint,
}: {
  open: boolean;
  controls: string;
  onToggle: () => void;
  children: React.ReactNode;
  hint?: React.ReactNode;
}) {
  const Icon = open ? ChevronDown : ChevronRight;
  return (
    <button
      type="button"
      aria-expanded={open}
      aria-controls={controls}
      onClick={onToggle}
      className="-ms-1 inline-flex h-8 items-center gap-1.5 self-start whitespace-nowrap rounded-control pe-2 ps-1 text-body font-medium text-fg-1 transition-colors hover:bg-subtle"
    >
      <Icon aria-hidden="true" className="h-3.5 w-3.5" strokeWidth={2} />
      {children}
      {hint && (
        <span className="text-caption font-normal text-fg-2">{hint}</span>
      )}
    </button>
  );
}

/** 小号分段（高 28，用在批量粘贴的「跳过 / 覆盖」） */
export function MiniSegmented<T extends string>({
  items,
  value,
  onValueChange,
  label,
}: {
  items: Array<{ value: T; label: string }>;
  value: T;
  onValueChange: (value: T) => void;
  label: string;
}) {
  const indicator = useSlidingIndicator<HTMLDivElement>(
    '[aria-pressed="true"]',
    value,
  );
  return (
    <div
      role="group"
      aria-label={label}
      ref={indicator.ref}
      className="relative flex h-7 shrink-0 gap-0.5 rounded-[7px] bg-subtle p-0.5"
    >
      <SegmentThumb
        rect={indicator.rect}
        animate={indicator.animate}
        className="rounded-[5px]"
      />
      {items.map((item) => {
        const pressed = item.value === value;
        return (
          <button
            key={item.value}
            type="button"
            aria-pressed={pressed}
            onClick={() => onValueChange(item.value)}
            className={cn(
              "relative h-6 rounded-[5px] px-2.5 text-caption font-medium text-fg-2 transition-colors",
              pressed && "font-semibold text-fg-1",
            )}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
