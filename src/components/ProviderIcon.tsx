import React, { useMemo } from "react";
import {
  getIcon,
  hasIcon,
  getIconMetadata,
  getIconUrl,
  isUrlIcon,
} from "@/icons/extracted";
import { cn } from "@/lib/utils";

interface ProviderIconProps {
  icon?: string; // 图标名称
  name: string; // 供应商名称（用于 fallback）
  color?: string; // 自定义颜色 (Deprecated, kept for compatibility but ignored for SVG)
  size?: number | string; // 尺寸
  className?: string;
  showFallback?: boolean; // 是否显示 fallback
}

export const ProviderIcon: React.FC<ProviderIconProps> = ({
  icon,
  name,
  color,
  size = 32,
  className,
  showFallback = true,
}) => {
  // 获取内联 SVG 字符串
  const iconSvg = useMemo(() => {
    if (icon && !isUrlIcon(icon) && hasIcon(icon)) {
      return getIcon(icon);
    }
    return "";
  }, [icon]);

  // 获取图标 URL（URL_ICONS 列表中的 SVG / 光栅图片）
  const iconUrl = useMemo(() => {
    if (icon && isUrlIcon(icon)) {
      return getIconUrl(icon);
    }
    return "";
  }, [icon]);

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

  // 获取有效颜色：优先使用传入的有效 color，否则从元数据获取 defaultColor
  const effectiveColor = useMemo(() => {
    if (color && typeof color === "string" && color.trim() !== "") {
      return color;
    }
    if (icon) {
      const metadata = getIconMetadata(icon);
      if (metadata?.defaultColor && metadata.defaultColor !== "currentColor") {
        return metadata.defaultColor;
      }
    }
    return undefined;
  }, [color, icon]);

  // 内联 SVG 渲染（支持 CSS currentColor 着色）
  if (iconSvg) {
    return (
      <span
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0",
          className,
        )}
        style={{ ...sizeStyle, color: effectiveColor }}
        dangerouslySetInnerHTML={{ __html: iconSvg }}
      />
    );
  }

  // URL-based 图标（大型 SVG / 光栅图片）：以 <img> 渲染
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt={name}
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
          "inline-flex items-center justify-center flex-shrink-0 rounded-lg",
          "bg-muted text-muted-foreground font-semibold",
          className,
        )}
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
