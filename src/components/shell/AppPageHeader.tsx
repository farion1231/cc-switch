import {
  createContext,
  useContext,
  useLayoutEffect,
  useState,
  type ReactNode,
} from "react";
import {
  DRAG_REGION_ATTR,
  DRAG_REGION_ENABLED,
  DRAG_REGION_STYLE,
} from "@/lib/platform";
import { cn } from "@/lib/utils";

/** 由外壳提供：Linux 自绘窗口按钮要接在每一屏页头的最右端。 */
export const WindowControlsContext = createContext<ReactNode>(null);

const NO_DRAG = { WebkitAppRegion: "no-drag" } as React.CSSProperties;
const PAGE_HEADER_HEIGHT = 52;

interface AppPageHeaderProps {
  /** 标题前的可操作元素（二级页的返回按钮）；和 icon 二选一 */
  leading?: ReactNode;
  icon?: ReactNode;
  title: ReactNode;
  /** 应用页用 16px 的名字 + 13px 的页面名；全局页和设置用 18px 的页面标题 */
  variant?: "app" | "page";
  /** 应用页的页面名（如「供应商」），灰字跟在应用名后面 */
  subtitle?: ReactNode;
  /** 标题后面的「?」、Beta 徽标、计数等 */
  titleExtra?: ReactNode;
  /** 右侧操作区：主按钮放最后一个、⋯ 放它后面 */
  actions?: ReactNode;
  /** 标题可能很长（如会话阅读页的会话名）：标题占满剩余宽度，放不下就截断 */
  truncateTitle?: boolean;
  className?: string;
}

/**
 * 内容区页头：高 52，整条可拖动窗口，左边是图标 + 标题，右边是操作。
 */
export function AppPageHeader({
  leading,
  icon,
  title,
  variant = "page",
  subtitle,
  titleExtra,
  actions,
  truncateTitle = false,
  className,
}: AppPageHeaderProps) {
  const windowControls = useContext(WindowControlsContext);
  // 自绘窗口按钮启用时没有系统标题栏，页头必须同时提供拖动入口。
  const dragRegionAttr = windowControls
    ? { "data-tauri-drag-region": true }
    : DRAG_REGION_ATTR;
  return (
    <header
      className={cn(
        "flex h-[52px] shrink-0 items-center gap-5 border-b border-border pe-4 ps-6",
        className,
      )}
      {...dragRegionAttr}
      style={DRAG_REGION_STYLE as React.CSSProperties}
    >
      {/* Tauri 只认按下的元素自身带拖动属性：标题区、标题、占位条都要带，整条才能拖窗口 */}
      <div
        {...dragRegionAttr}
        className={cn(
          "flex min-w-0 items-center gap-2.5",
          truncateTitle ? "flex-1" : "shrink-0",
        )}
      >
        {leading && (
          <span className="-ms-2 flex shrink-0 items-center" style={NO_DRAG}>
            {leading}
          </span>
        )}
        {icon && (
          <span
            aria-hidden="true"
            className="pointer-events-none flex h-[22px] w-[22px] shrink-0 items-center justify-center text-fg-1"
          >
            {icon}
          </span>
        )}
        <h1
          {...dragRegionAttr}
          className={cn(
            "m-0 whitespace-nowrap",
            variant === "app" ? "text-title" : "text-page",
            truncateTitle && "min-w-0 truncate",
          )}
        >
          {title}
        </h1>
        {(subtitle || titleExtra) && (
          <div className="flex min-w-0 items-center gap-1" style={NO_DRAG}>
            {subtitle && (
              <span className="whitespace-nowrap text-body text-fg-3">
                {subtitle}
              </span>
            )}
            {titleExtra}
          </div>
        )}
      </div>
      {!truncateTitle && <div {...dragRegionAttr} className="flex-1" />}
      {actions && (
        <div
          className="flex min-w-0 shrink items-center justify-end gap-2"
          style={NO_DRAG}
        >
          {actions}
        </div>
      )}
      {windowControls && (
        <div className="flex shrink-0 items-center gap-1" style={NO_DRAG}>
          {windowControls}
        </div>
      )}
    </header>
  );
}

/**
 * 模态遮罩的子元素：遮罩盖住了页头，这一条铺到页头下沿，弹窗打开时照样能拖窗口，
 * 页头最右端的窗口按钮也在这里再放一份。
 * 放在遮罩里才收得到指针事件（Radix 把遮罩外的 body 设成 pointer-events: none），
 * 也自然排在弹窗内容下面。
 */
export function ModalDragStrip() {
  const windowControls = useContext(WindowControlsContext);
  const [height, setHeight] = useState(PAGE_HEADER_HEIGHT);
  // 页头上方可能有环境变量警告条，得量实际位置，窗口按钮才能和页头里的对齐
  useLayoutEffect(() => {
    const header = document.querySelector("#content-area header");
    const bottom = header?.getBoundingClientRect().bottom;
    if (bottom) setHeight(bottom);
  }, []);
  if (!windowControls && !DRAG_REGION_ENABLED) return null;
  return (
    <div
      data-tauri-drag-region
      className="absolute inset-x-0 top-0"
      style={{ height, ...DRAG_REGION_STYLE }}
      // 不让 pointerdown 冒到 document：「点遮罩就关」的弹窗不能一拖窗口就关掉
      onPointerDown={(event) => event.stopPropagation()}
      // 同理不让焦点跑到遮罩里：焦点离开弹窗也算「点外面」
      onMouseDown={(event) => event.preventDefault()}
    >
      {windowControls && (
        // 让开页头 1px 下边框，和页头里的按钮严丝合缝，不然半透明遮罩下会有重影
        <div
          className="absolute bottom-px end-4 flex items-center gap-1"
          style={{ height: PAGE_HEADER_HEIGHT - 1, ...NO_DRAG }}
        >
          {windowControls}
        </div>
      )}
    </div>
  );
}
