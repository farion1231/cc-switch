import React, { useMemo, useState } from "react";
import {
  getIcon,
  hasIcon,
  getIconMetadata,
  getIconUrl,
  isUrlIcon,
  isDarkInk,
  adaptSvgToTheme,
  isTileIcon,
} from "@/icons/extracted";
import { cn } from "@/lib/utils";
import { isCustomIcon } from "@/utils/customIcon";

interface ProviderIconProps {
  icon?: string; // 图标名称
  name: string; // 供应商名称（用于 fallback）
  color?: string; // 自定义颜色 (Deprecated, kept for compatibility but ignored for SVG)
  size?: number | string; // 尺寸
  className?: string;
  showFallback?: boolean; // 是否显示 fallback
  fallbackClassName?: string; // 覆盖首字母 fallback 的底色 / 文字色
}

export const ProviderIcon: React.FC<ProviderIconProps> = ({
  icon,
  name,
  color,
  size = 32,
  className,
  showFallback = true,
  fallbackClassName,
}) => {
  // 获取内联 SVG 字符串
  const { svg: iconSvg, adapted } = useMemo(() => {
    if (icon && !isCustomIcon(icon) && !isUrlIcon(icon) && hasIcon(icon)) {
      const raw = getIcon(icon);
      // tile 图标自带底色，里面的深色是底，不能换成文字色
      const themed = isTileIcon(icon) ? raw : adaptSvgToTheme(raw);
      return { svg: themed, adapted: themed !== raw };
    }
    return { svg: "", adapted: false };
  }, [icon]);

  // 获取图标 URL：内置的大图（URL_ICONS 列表中的 SVG / 光栅图片），或用户上传的 PNG。
  // 自定义图标只认我们自己编码的 PNG data URI，从不把外链地址当图标加载
  const iconUrl = useMemo(() => {
    if (isCustomIcon(icon)) {
      return icon;
    }
    if (icon && isUrlIcon(icon)) {
      return getIconUrl(icon);
    }
    return "";
  }, [icon]);

  // 解码失败的图片（截断、损坏的自定义图标）记下来，退回首字母，不显示破图
  const [brokenUrl, setBrokenUrl] = useState<string>();

  // 计算尺寸样式
  const sizeStyle = useMemo(() => {
    const sizeValue = typeof size === "number" ? `${size}px` : size;
    return {
      width: sizeValue,
      height: sizeValue,
      fontSize: sizeValue,
      lineHeight: 1,
    };
  }, [size]);

  // 获取有效颜色：优先使用传入的有效 color，否则从元数据获取 defaultColor。
  // 近黑色（#000 / #111 这类）的颜色不写死，交给外层文字色，
  // 否则深色模式下是黑底上的黑图标。已经把近黑笔画换成 currentColor 的图标
  // 也不能再套品牌色，否则眼睛、嘴这些细节会和脸同色
  const effectiveColor = useMemo(() => {
    if (adapted) return undefined;
    let resolved: string | undefined;
    if (color && typeof color === "string" && color.trim() !== "") {
      resolved = color;
    } else if (icon) {
      const metadata = getIconMetadata(icon);
      if (metadata?.defaultColor && metadata.defaultColor !== "currentColor") {
        resolved = metadata.defaultColor;
      }
    }
    return isDarkInk(resolved) ? undefined : resolved;
  }, [color, icon, adapted]);

  // 内联 SVG 渲染（支持 CSS currentColor 着色）
  if (iconSvg) {
    return (
      <span
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0",
          className,
        )}
        title={name}
        style={{ ...sizeStyle, color: effectiveColor }}
        dangerouslySetInnerHTML={{ __html: iconSvg }}
      />
    );
  }

  // URL-based 图标（大型 SVG / 光栅图片）：以 <img> 渲染
  if (iconUrl && iconUrl !== brokenUrl) {
    return (
      <img
        src={iconUrl}
        onError={() => setBrokenUrl(iconUrl)}
        alt={name}
        title={name}
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0 object-contain",
          className,
        )}
        style={{ width: sizeStyle.width, height: sizeStyle.height }}
        loading="lazy"
      />
    );
  }

  // Fallback：显示首字母
  if (showFallback) {
    const initials = name
      .split(" ")
      .map((word) => word[0])
      .join("")
      .toUpperCase()
      .slice(0, 2);
    const fallbackFontSize =
      typeof size === "number" ? `${Math.max(size * 0.5, 12)}px` : "0.5em";
    return (
      <span
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0 rounded-lg whitespace-nowrap",
          "bg-muted text-muted-foreground font-semibold",
          className,
          fallbackClassName,
        )}
        title={name}
        style={sizeStyle}
      >
        <span
          style={{
            fontSize: fallbackFontSize,
          }}
        >
          {initials}
        </span>
      </span>
    );
  }

  return null;
};
