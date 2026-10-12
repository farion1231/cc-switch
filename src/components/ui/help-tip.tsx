import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";
import { CircleHelp } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * 说明卡（v7 第八轮「?」，规格见 docs/research/2026-09-30-ui-design-v7/HELP_BRIEF.md）。
 *
 * - 悬停 300ms 打开，移开 200ms 关闭（移进卡片不关）；键盘聚焦立即打开。
 *   只认键盘聚焦（:focus-visible）：弹层打开时 Radix 会把焦点自动放到里面第一个可聚焦元素，
 *   「?」常常就是那个，鼠标点开弹层不该顺带弹出说明卡。
 * - 点一下固定，Escape 或点别处关闭。
 * - 不抢焦点（modal=false），卡片里不放按钮、表单、链接。
 * - 只放规则和术语解释；会丢东西、要重启这类后果写在按钮、确认框或 toast 里，不进这里。
 *
 * 深色小气泡（tooltip.tsx）只给纯图标按钮报名字，不放说明句子。
 */

const OPEN_DELAY = 300;
const CLOSE_DELAY = 200;

/** 焦点是不是键盘带来的（Tab / 方向键）；浏览器不认 :focus-visible 时按键盘处理 */
function isKeyboardFocus(element: HTMLElement) {
  try {
    return element.matches(":focus-visible");
  } catch {
    return true;
  }
}

function useHoverCard() {
  const [open, setOpen] = React.useState(false);
  const [pinned, setPinned] = React.useState(false);
  const timer = React.useRef<number | undefined>(undefined);

  const clear = () => {
    if (timer.current !== undefined) {
      window.clearTimeout(timer.current);
      timer.current = undefined;
    }
  };

  React.useEffect(() => clear, []);

  const schedule = (next: boolean, delay: number) => {
    clear();
    timer.current = window.setTimeout(() => setOpen(next), delay);
  };

  return {
    open: open || pinned,
    pinned,
    onPointerEnter: () => {
      if (!pinned) schedule(true, OPEN_DELAY);
    },
    onPointerLeave: () => {
      if (!pinned) schedule(false, CLOSE_DELAY);
    },
    onFocus: (event: React.FocusEvent<HTMLElement>) => {
      if (!isKeyboardFocus(event.currentTarget)) return;
      clear();
      setOpen(true);
    },
    onBlur: () => {
      if (!pinned) {
        clear();
        setOpen(false);
      }
    },
    onClick: () => {
      clear();
      setPinned((value) => !value);
      setOpen(true);
    },
    onOpenChange: (next: boolean) => {
      // Escape / 点别处
      if (!next) {
        clear();
        setPinned(false);
        setOpen(false);
      }
    },
  };
}

interface HelpCardProps {
  id: string;
  title?: React.ReactNode;
  children: React.ReactNode;
  side?: "top" | "bottom";
  align?: "start" | "end";
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}

function HelpCard({
  id,
  title,
  children,
  side = "bottom",
  align = "start",
  onPointerEnter,
  onPointerLeave,
}: HelpCardProps) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        id={id}
        role="tooltip"
        side={side}
        align={align}
        sideOffset={6}
        alignOffset={-8}
        collisionPadding={8}
        onOpenAutoFocus={(event) => event.preventDefault()}
        onCloseAutoFocus={(event) => event.preventDefault()}
        onPointerEnter={onPointerEnter}
        onPointerLeave={onPointerLeave}
        className="z-[120] w-max max-w-[300px] rounded-[8px] border border-border bg-surface px-3 pb-2.5 pt-[9px] text-caption text-fg-1 shadow-v7-md outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0"
      >
        {title ? (
          <span className="mb-0.5 block font-semibold">{title}</span>
        ) : null}
        {children}
      </PopoverPrimitive.Content>
    </PopoverPrimitive.Portal>
  );
}

export interface HelpTipProps {
  /** 卡片标题，同时是「?」按钮的无障碍名字。写成问题或名词。 */
  title: string;
  /** 正文，一句话，≤ 80 字。 */
  children: React.ReactNode;
  side?: "top" | "bottom";
  align?: "start" | "end";
  className?: string;
}

/** 放在被解释的词后面的「?」按钮。 */
export function HelpTip({
  title,
  children,
  side,
  align,
  className,
}: HelpTipProps) {
  const id = React.useId();
  const descId = `${id}-desc`;
  const hover = useHoverCard();

  return (
    <PopoverPrimitive.Root open={hover.open} onOpenChange={hover.onOpenChange}>
      {/* 卡片只在打开时挂载；屏幕阅读器读这份一直在的副本 */}
      <span id={descId} className="sr-only">
        {children}
      </span>
      <PopoverPrimitive.Trigger asChild>
        <button
          type="button"
          aria-label={title}
          aria-describedby={descId}
          onPointerEnter={hover.onPointerEnter}
          onPointerLeave={hover.onPointerLeave}
          onFocus={hover.onFocus}
          onBlur={hover.onBlur}
          onClick={(event) => {
            event.preventDefault();
            hover.onClick();
          }}
          className={cn(
            "-my-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full align-middle text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1 data-[state=open]:bg-subtle data-[state=open]:text-fg-1",
            className,
          )}
        >
          <CircleHelp className="h-3.5 w-3.5" strokeWidth={1.5} />
        </button>
      </PopoverPrimitive.Trigger>
      <HelpCard
        id={id}
        title={title}
        side={side}
        align={align}
        onPointerEnter={hover.onPointerEnter}
        onPointerLeave={hover.onPointerLeave}
      >
        {children}
      </HelpCard>
    </PopoverPrimitive.Root>
  );
}

export interface DisabledReasonProps {
  /** 为什么不能点。为空时原样渲染子元素。 */
  reason?: React.ReactNode;
  /** 一个按钮。有原因时会被设成 aria-disabled（不用原生 disabled，否则收不到悬停也进不了 Tab 顺序），点击无效。 */
  children: React.ReactElement<{
    onClick?: React.MouseEventHandler;
    "aria-disabled"?: boolean | "true" | "false";
    "aria-describedby"?: string;
    disabled?: boolean;
    className?: string;
  }>;
  side?: "top" | "bottom";
  align?: "start" | "end";
}

/**
 * 禁用原因挂在按钮本身上：悬停 / 聚焦按钮时出现说明卡。
 * 菜单、对话框、抽屉这类浮层里的禁用原因照旧写成看得见的第二行，不用这个组件。
 */
export function DisabledReason({
  reason,
  children,
  side,
  align,
}: DisabledReasonProps) {
  const id = React.useId();
  const descId = `${id}-desc`;
  const hover = useHoverCard();

  if (!reason) return children;

  const button = React.cloneElement(children, {
    disabled: false,
    "aria-disabled": "true",
    "aria-describedby": descId,
    onClick: (event: React.MouseEvent) => {
      event.preventDefault();
      hover.onClick();
    },
    className: cn(children.props.className, "cursor-not-allowed opacity-45"),
  });

  return (
    <PopoverPrimitive.Root open={hover.open} onOpenChange={hover.onOpenChange}>
      <PopoverPrimitive.Anchor asChild>
        <span
          className="inline-flex"
          onPointerEnter={hover.onPointerEnter}
          onPointerLeave={hover.onPointerLeave}
          onFocus={hover.onFocus}
          onBlur={hover.onBlur}
        >
          {button}
          <span id={descId} className="sr-only">
            {reason}
          </span>
        </span>
      </PopoverPrimitive.Anchor>
      <HelpCard
        id={id}
        side={side}
        align={align}
        onPointerEnter={hover.onPointerEnter}
        onPointerLeave={hover.onPointerLeave}
      >
        {reason}
      </HelpCard>
    </PopoverPrimitive.Root>
  );
}
