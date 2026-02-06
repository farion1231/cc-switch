import React, { useMemo } from "react";
import { getIcon, hasIcon } from "@/icons/extracted";
import { cn } from "@/lib/utils";

// Pastel background colors for fallback avatars
const PASTEL_COLORS = [
  {
    bg: "bg-blue-100 dark:bg-blue-900/60",
    text: "text-blue-600 dark:text-blue-400",
  },
  {
    bg: "bg-green-100 dark:bg-green-900/60",
    text: "text-green-600 dark:text-green-400",
  },
  {
    bg: "bg-purple-100 dark:bg-purple-900/60",
    text: "text-purple-600 dark:text-purple-400",
  },
  {
    bg: "bg-pink-100 dark:bg-pink-900/60",
    text: "text-pink-600 dark:text-pink-400",
  },
  {
    bg: "bg-amber-100 dark:bg-amber-900/60",
    text: "text-amber-600 dark:text-amber-400",
  },
  {
    bg: "bg-cyan-100 dark:bg-cyan-900/60",
    text: "text-cyan-600 dark:text-cyan-400",
  },
  {
    bg: "bg-indigo-100 dark:bg-indigo-900/60",
    text: "text-indigo-600 dark:text-indigo-400",
  },
  {
    bg: "bg-rose-100 dark:bg-rose-900/60",
    text: "text-rose-600 dark:text-rose-400",
  },
  {
    bg: "bg-teal-100 dark:bg-teal-900/60",
    text: "text-teal-600 dark:text-teal-400",
  },
  {
    bg: "bg-orange-100 dark:bg-orange-900/60",
    text: "text-orange-600 dark:text-orange-400",
  },
];

// Generate consistent color based on name
const getColorForName = (name: string) => {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return PASTEL_COLORS[Math.abs(hash) % PASTEL_COLORS.length];
};

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
  // 获取图标 SVG
  const iconSvg = useMemo(() => {
    if (icon && hasIcon(icon)) {
      return getIcon(icon);
    }
    return "";
  }, [icon]);

  // 计算尺寸样式
  const sizeStyle = useMemo(() => {
    const sizeValue = typeof size === "number" ? `${size}px` : size;
    return {
      width: sizeValue,
      height: sizeValue,
      // 内嵌 SVG 使用 1em 作为尺寸基准，这里同步 fontSize 让图标实际跟随 size 放大
      fontSize: sizeValue,
      lineHeight: 1,
    };
  }, [size]);

  // 如果有图标，显示图标
  if (iconSvg) {
    return (
      <span
        className={cn(
          "inline-flex items-center justify-center flex-shrink-0",
          className,
        )}
        style={{ ...sizeStyle, color }}
        dangerouslySetInnerHTML={{ __html: iconSvg }}
      />
    );
  }

  // Fallback：显示首字母 with pastel background
  if (showFallback) {
    const initials = name
      .split(" ")
      .map((word) => word[0])
      .join("")
      .toUpperCase()
      .slice(0, 2);
    const colorScheme = getColorForName(name);

    const fallbackFontSize =
      typeof size === "number" ? `${Math.max(size * 0.5, 12)}px` : "0.5em";

    return (
      <span
        className={cn(
          "inline-flex items-center justify-center rounded-[inherit] w-full h-full",
          colorScheme.bg,
          colorScheme.text,
          "font-semibold",
          className,
        )}
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
