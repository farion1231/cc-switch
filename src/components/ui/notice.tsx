import * as React from "react";
import {
  AlertTriangle,
  CircleAlert,
  Info,
  Layers,
  Route,
  X,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * 通知条（v7 AUTHORING「通知条」）：圆角 10，左侧 16px 图标，不用左边框强调色。
 * 应用页的通知槽（NoticeSlot）里排成一列，按严重程度从上到下。
 */
export type NoticeTone =
  | "neutral"
  | "warning"
  | "danger"
  | "route"
  | "stack"
  | "direct";

const TONE: Record<
  NoticeTone,
  { box: string; icon: string; Icon: LucideIcon }
> = {
  neutral: { box: "bg-subtle", icon: "text-fg-2", Icon: Info },
  warning: {
    box: "bg-warning-soft",
    icon: "text-warning-text",
    Icon: AlertTriangle,
  },
  danger: {
    box: "bg-danger-soft",
    icon: "text-danger-text",
    Icon: CircleAlert,
  },
  route: { box: "bg-route-soft", icon: "text-route-text", Icon: Route },
  stack: { box: "bg-stack-soft", icon: "text-stack-text", Icon: Layers },
  direct: { box: "bg-direct-soft", icon: "text-direct-text", Icon: Info },
};

export interface NoticeProps {
  tone?: NoticeTone;
  icon?: LucideIcon;
  /** 第一行，13px */
  title: React.ReactNode;
  /** 第二行，12px --text-2 */
  children?: React.ReactNode;
  /** 右侧按钮（用 Button size="compact"） */
  actions?: React.ReactNode;
  /** 可关闭时传；关闭按钮的无障碍名字 */
  onDismiss?: () => void;
  dismissLabel?: string;
  className?: string;
}

export function Notice({
  tone = "neutral",
  icon,
  title,
  children,
  actions,
  onDismiss,
  dismissLabel,
  className,
}: NoticeProps) {
  const style = TONE[tone];
  const Icon = icon ?? style.Icon;
  return (
    <div
      className={cn(
        "flex gap-2.5 rounded-panel py-2.5 pe-3.5 ps-[19px] text-body text-fg-1",
        // 只有一行时整行上下居中；有第二行时图标对齐第一行
        children ? "items-start" : "items-center",
        style.box,
        className,
      )}
    >
      <Icon
        aria-hidden="true"
        strokeWidth={1.5}
        className={cn("h-4 w-4 shrink-0", children && "mt-0.5", style.icon)}
      />
      <div className="min-w-0 flex-1">
        <div>{title}</div>
        {children ? (
          <div className="mt-0.5 text-caption text-fg-2">{children}</div>
        ) : null}
      </div>
      {actions ? (
        <div className="flex shrink-0 items-center gap-2 self-center">
          {actions}
        </div>
      ) : null}
      {onDismiss ? (
        <button
          type="button"
          aria-label={dismissLabel}
          onClick={onDismiss}
          className="-me-1 inline-flex h-6 w-6 shrink-0 items-center justify-center self-center rounded-control text-fg-2 transition-colors hover:bg-black/5 hover:text-fg-1 dark:hover:bg-white/10"
        >
          <X className="h-4 w-4" strokeWidth={1.5} />
        </button>
      ) : null}
    </div>
  );
}

/**
 * 通知槽：一直渲染的 role="status" 容器（R4），操作后出现的提示放进来才会被读出。
 * 页面一打开就在的提示不要用 role="alert"。
 */
export function NoticeSlot({
  children,
  className,
}: {
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      role="status"
      className={cn("flex flex-col gap-2 empty:hidden", className)}
    >
      {children}
    </div>
  );
}
