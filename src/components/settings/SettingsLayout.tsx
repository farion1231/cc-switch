import type { ReactNode } from "react";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";

/**
 * 设置页的版式（v7）：分节标题 15/600 → 带边框的卡片 → 一行一项，左边名称、右边控件。
 */
export function SettingsBlock({
  title,
  help,
  actions,
  children,
  className,
}: {
  title?: ReactNode;
  /** 标题旁的「?」：规则和术语放这里，后果不放 */
  help?: { title: string; body: ReactNode };
  /** 分节右侧的操作（如本节的「保存」） */
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={cn("space-y-3", className)}>
      {(title || actions) && (
        <div className="flex min-h-7 items-center gap-2">
          {title && <h2 className="m-0 text-section text-fg-1">{title}</h2>}
          {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
          {actions && (
            <div className="ms-auto flex items-center gap-2">{actions}</div>
          )}
        </div>
      )}
      {children}
    </section>
  );
}

/** 一组设置行：白底卡片，行与行之间一条细线。 */
export function SettingsCard({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function SettingsRow({
  label,
  help,
  description,
  control,
  htmlFor,
  children,
  className,
}: {
  label: ReactNode;
  help?: { title: string; body: ReactNode };
  /** 只有真正的数据（路径、地址）才写在名称下面；说明进「?」 */
  description?: ReactNode;
  control?: ReactNode;
  htmlFor?: string;
  /** 控件下面整行宽的内容（如路径输入框） */
  children?: ReactNode;
  className?: string;
}) {
  const Label = htmlFor ? "label" : "div";
  return (
    <div className={cn("px-4 py-3", className)}>
      <div className="flex min-h-[28px] items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1">
            <Label
              htmlFor={htmlFor}
              className="text-body font-medium text-fg-1"
            >
              {label}
            </Label>
            {help && <HelpTip title={help.title}>{help.body}</HelpTip>}
          </div>
          {description && (
            <div className="mt-0.5 break-all text-caption text-fg-2">
              {description}
            </div>
          )}
        </div>
        {control && (
          <div className="flex shrink-0 items-center gap-2">{control}</div>
        )}
      </div>
      {children && <div className="mt-2.5">{children}</div>}
    </div>
  );
}

export function SettingsSwitchRow({
  label,
  help,
  description,
  checked,
  onCheckedChange,
  disabled,
}: {
  label: string;
  help?: { title: string; body: ReactNode };
  description?: ReactNode;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <SettingsRow
      label={label}
      help={help}
      description={description}
      control={
        <Switch
          checked={checked}
          onCheckedChange={onCheckedChange}
          disabled={disabled}
          aria-label={label}
        />
      }
    />
  );
}

/** 设置页正文：最大宽度 760，左右 24 的内边距，分节间距 28。 */
export function SettingsBody({ children }: { children: ReactNode }) {
  return (
    <div className="mx-auto w-full max-w-[760px] space-y-7 px-6 pb-10 pt-6">
      {children}
    </div>
  );
}
