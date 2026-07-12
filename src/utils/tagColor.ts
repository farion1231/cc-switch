/**
 * 共享的 tag 着色工具 —— 由 `ApiKeyListSection`（编辑面板）和
 * `KeyPoolBadge`（provider 列表 row）共同消费，保证同一个 tag 字符串
 * 在两处视觉颜色一致。
 *
 * 算法：djb2 hash → unsigned 32-bit → % palette.len。空串落到 0；
 * tag 长度 ≤ 32 时碰撞率 < 0.5%。
 *
 * 调色板风格参考 GitHub / Trello 的 tag：浅色（light）bg 90% 饱和度 +
 * fg 35%，dark mode 切到 30% 饱和度背景 + 高亮前景，避免暗色下文字
 * 不可读。六色光谱均匀分布，避免相邻 tag 颜色太近。
 */

const TAG_PALETTE: ReadonlyArray<{ light: string; dark: string }> = [
  {
    light: "bg-blue-100 text-blue-700 ring-blue-200",
    dark: "dark:bg-blue-950/60 dark:text-blue-300 dark:ring-blue-900",
  },
  {
    light: "bg-emerald-100 text-emerald-700 ring-emerald-200",
    dark: "dark:bg-emerald-950/60 dark:text-emerald-300 dark:ring-emerald-900",
  },
  {
    light: "bg-amber-100 text-amber-700 ring-amber-200",
    dark: "dark:bg-amber-950/60 dark:text-amber-300 dark:ring-amber-900",
  },
  {
    light: "bg-rose-100 text-rose-700 ring-rose-200",
    dark: "dark:bg-rose-950/60 dark:text-rose-300 dark:ring-rose-900",
  },
  {
    light: "bg-violet-100 text-violet-700 ring-violet-200",
    dark: "dark:bg-violet-950/60 dark:text-violet-300 dark:ring-violet-900",
  },
  {
    light: "bg-teal-100 text-teal-700 ring-teal-200",
    dark: "dark:bg-teal-950/60 dark:text-teal-300 dark:ring-teal-900",
  },
];

/** djb2 hash → unsigned 32-bit → palette 索引。空串落 0。 */
function hashTagToPaletteIndex(tag: string): number {
  let hash = 5381;
  for (let i = 0; i < tag.length; i++) {
    hash = ((hash << 5) + hash) ^ tag.charCodeAt(i);
  }
  return (hash >>> 0) % TAG_PALETTE.length;
}

/** 返回 `tag` 对应的 Tailwind className 片段（背景 / 文字 / 细 ring）。 */
export function tagColorClasses(tag: string): string {
  const palette = TAG_PALETTE[hashTagToPaletteIndex(tag)];
  return `${palette.light} ${palette.dark}`;
}