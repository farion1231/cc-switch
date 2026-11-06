import {
  forwardRef,
  useEffect,
  useMemo,
  useState,
  type ComponentPropsWithoutRef,
} from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Minimize2, Minus, X } from "lucide-react";
import { extractErrorMessage } from "@/utils/errorUtils";
import { cn } from "@/lib/utils";
import { HoverTip } from "@/components/ui/hover-tip";
import type {
  WindowControlImages,
  WindowControlKind,
  WindowControlsImages,
} from "@/lib/api";

const BUTTON_BASE =
  "flex items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1";

const DEFAULT_ORDER: WindowControlKind[] = ["minimize", "maximize", "close"];

interface WindowControlsProps {
  /** 系统标题栏布局中的按钮顺序；缺省为 Windows 式「最小化、最大化、关闭」。 */
  order?: WindowControlKind[];
  /** 按当前 GTK 主题离屏渲染的按钮图片；缺失时用内置图标。 */
  images?: WindowControlsImages;
  /** 侧栏收起成 72px 图标轨时用更小的按钮。 */
  compact?: boolean;
}

interface SystemImageButtonProps {
  image: WindowControlImages;
  label: string;
  compact: boolean;
  onClick: () => void;
}

/**
 * 用系统主题导出的 PNG 画按钮；悬停 / 按下切换对应状态图。
 * HoverTip 通过 asChild 把 ref 和指针事件塞进来，必须原样转给真正的 <button>，
 * 否则提示框打不开。
 */
const SystemImageButton = forwardRef<
  HTMLButtonElement,
  SystemImageButtonProps & ComponentPropsWithoutRef<"button">
>(function SystemImageButton(
  { image, label, compact, onClick, onMouseEnter, onMouseLeave, ...props },
  ref,
) {
  const [hovered, setHovered] = useState(false);
  const [pressed, setPressed] = useState(false);
  const size = compact ? 24 : image.width;
  const height = compact ? 24 : image.height;
  const source = pressed ? image.pressed : hovered ? image.hover : image.normal;

  return (
    <button
      {...props}
      ref={ref}
      type="button"
      aria-label={label}
      onClick={onClick}
      onMouseEnter={(event) => {
        setHovered(true);
        onMouseEnter?.(event);
      }}
      onMouseLeave={(event) => {
        setHovered(false);
        setPressed(false);
        onMouseLeave?.(event);
      }}
      onMouseDown={() => setPressed(true)}
      onMouseUp={() => setPressed(false)}
      className="flex shrink-0 items-center justify-center rounded-control outline-none transition-none focus-visible:ring-2 focus-visible:ring-action/50"
      style={{
        width: size,
        height,
        backgroundImage: `url("${source}")`,
        backgroundPosition: "center",
        backgroundRepeat: "no-repeat",
        backgroundSize: "contain",
      }}
    />
  );
});

/** 最小化 / 最大化 / 关闭：Windows 一律用它，Linux 打开「使用应用内窗口按钮」后用它。按系统布局顺序渲染。 */
export function WindowControls({
  order,
  images,
  compact = false,
}: WindowControlsProps) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const buttonClass = cn(BUTTON_BASE, compact ? "h-5 w-5" : "h-7 w-7");
  const iconClass = compact ? "h-3 w-3" : "h-4 w-4";

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const sync = async () => {
      try {
        const win = getCurrentWindow();
        const update = async () => {
          const value = await win.isMaximized();
          if (active) setMaximized(value);
        };
        await update();
        unlisten = await win.onResized(() => void update());
      } catch (error) {
        console.error("[WindowControls] Failed to sync maximized state", error);
      }
    };
    void sync();
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const normalizedOrder = useMemo(() => {
    const valid = (order ?? []).filter((kind) => DEFAULT_ORDER.includes(kind));
    for (const kind of DEFAULT_ORDER) {
      if (!valid.includes(kind)) {
        valid.push(kind);
      }
    }
    return valid;
  }, [order]);

  const run = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (error) {
      console.error("[WindowControls] Window control failed", error);
      toast.error(
        t("notifications.windowControlFailed", {
          defaultValue: "窗口控制失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    }
  };

  const minimize = () => void run(() => getCurrentWindow().minimize());
  const toggleMaximize = () =>
    void run(async () => {
      const win = getCurrentWindow();
      await win.toggleMaximize();
      setMaximized(await win.isMaximized());
    });
  const close = () => void run(() => getCurrentWindow().close());

  const maximizeLabel = maximized
    ? t("header.windowRestore")
    : t("header.windowMaximize");

  const imageFor = (kind: WindowControlKind) =>
    kind === "maximize"
      ? ((maximized ? images?.restore : undefined) ?? images?.maximize)
      : images?.[kind];

  return (
    <>
      {normalizedOrder.map((kind) => {
        const label =
          kind === "minimize"
            ? t("header.windowMinimize")
            : kind === "maximize"
              ? maximizeLabel
              : t("header.windowClose");
        const onClick =
          kind === "minimize"
            ? minimize
            : kind === "maximize"
              ? toggleMaximize
              : close;
        const image = imageFor(kind);

        return (
          <HoverTip key={kind} content={label}>
            {image ? (
              <SystemImageButton
                image={image}
                label={label}
                compact={compact}
                onClick={onClick}
              />
            ) : (
              <button
                type="button"
                className={cn(
                  buttonClass,
                  kind === "close" &&
                    "hover:bg-danger-soft hover:text-danger-text",
                )}
                aria-label={label}
                onClick={onClick}
              >
                {kind === "minimize" ? (
                  <Minus className={iconClass} />
                ) : kind === "maximize" ? (
                  maximized ? (
                    <Minimize2 className={iconClass} />
                  ) : (
                    <Maximize2 className={iconClass} />
                  )
                ) : (
                  <X className={iconClass} />
                )}
              </button>
            )}
          </HoverTip>
        );
      })}
    </>
  );
}
