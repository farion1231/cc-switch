import React, { useEffect, useMemo, useState } from "react";
import {
  getIcon,
  hasIcon,
  getIconMetadata,
  getIconUrl,
  isUrlIcon,
} from "@/icons/extracted";
import { useProviderIconSrc } from "@/hooks/useProviderIconSrc";
import { cn } from "@/lib/utils";

export type ProviderIconSource = "builtin" | "local" | "url";

interface ProviderIconProps {
  icon?: string; // 图标名称 / 本地资产键 / 图片 URL（取决于 iconSource）
  iconSource?: ProviderIconSource; // 图标来源；缺省时按 icon 形状推断（旧数据兼容）
  name: string; // 供应商名称（用于 fallback）
  color?: string; // 自定义颜色 (Deprecated, kept for compatibility but ignored for SVG)
  size?: number | string; // 尺寸
  className?: string;
  showFallback?: boolean; // 是否显示 fallback
  fallbackClassName?: string; // 覆盖首字母 fallback 的底色 / 文字色
}

// 本地资产键形状：{provider_stem}-{hash8}.{ext}（与 Rust 端 is_valid_asset_key 对应）
const LOCAL_ICON_KEY_PATTERN =
  /^[\w-]{1,80}-[0-9a-f]{8}\.(png|jpe?g|webp|svg)$/;
const HTTP_URL_PATTERN = /^https?:\/\//i;

/** 未显式给 iconSource 时按 icon 值形状推断来源。 */
function inferIconSource(icon?: string): ProviderIconSource {
  if (!icon) return "builtin";
  if (LOCAL_ICON_KEY_PATTERN.test(icon)) return "local";
  if (HTTP_URL_PATTERN.test(icon)) return "url";
  return "builtin";
}

export const ProviderIcon: React.FC<ProviderIconProps> = ({
  icon,
  iconSource,
  name,
  color,
  size = 32,
  className,
  showFallback = true,
  fallbackClassName,
}) => {
  const source = iconSource ?? inferIconSource(icon);

  // 本地资产 → data: URL（模块级缓存，缺失时 null 走回退链）
  const isLocal = source === "local" && !!icon;
  const localSrc = useProviderIconSrc(isLocal ? icon : undefined);

  // 自定义 URL：加载失败时降级到后续回退链（不显示破图）
  const isCustomUrl = source === "url" && !!icon;
  const [urlFailed, setUrlFailed] = useState(false);
  useEffect(() => {
    setUrlFailed(false);
  }, [icon]);

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

  // 自定义图标渲染优先级最高：local 资产 → 自定义 URL（R5）
  if (isLocal && localSrc) {
    return (
      <img
        src={localSrc}
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

  if (isCustomUrl && !urlFailed) {
    return (
      <img
        src={icon}
        alt={name}
        title={name}
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0 object-contain",
          className,
        )}
        style={{ width: sizeStyle.width, height: sizeStyle.height }}
        loading="lazy"
        onError={() => setUrlFailed(true)}
      />
    );
  }

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
  if (iconUrl) {
    return (
      <img
        src={iconUrl}
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
