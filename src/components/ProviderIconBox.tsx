import { ProviderIcon } from "@/components/ProviderIcon";
import { isTileIcon } from "@/icons/extracted";
import { cn } from "@/lib/utils";

// 透明 logo 在 32px 框里的尺寸：和铺满整框的 tile 图标放在一起时视觉分量相当，
// 再大实心 logo 就顶到边框了
const GLYPH_SIZE = 22;

interface ProviderIconBoxProps {
  icon?: string;
  name: string;
  color?: string;
  /** 框的底色、透明度等 */
  className?: string;
  iconClassName?: string;
  fallbackClassName?: string;
}

/**
 * 供应商图标的 32px 圆角框。透明 logo 缩小居中、框带边框；自带实心底的图标
 * （metadata 里 shape: "tile"）铺满整框并裁成框的圆角，否则会变成「框里套一块小方块」。
 */
export function ProviderIconBox({
  icon,
  name,
  color,
  className,
  iconClassName,
  fallbackClassName,
}: ProviderIconBoxProps) {
  const tile = isTileIcon(icon);
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative flex h-8 w-8 shrink-0 items-center justify-center overflow-hidden rounded-[8px]",
        tile
          ? // 描边画在图片上层：白底图标放在白色卡片上也有轮廓
            "after:pointer-events-none after:absolute after:inset-0 after:rounded-[inherit] after:ring-1 after:ring-inset after:ring-black/10 dark:after:ring-white/10"
          : "border border-border",
        className,
      )}
    >
      <ProviderIcon
        icon={icon}
        name={name}
        color={color}
        size={tile ? 32 : GLYPH_SIZE}
        className={cn(iconClassName, tile && "object-cover")}
        fallbackClassName={fallbackClassName}
      />
    </span>
  );
}
