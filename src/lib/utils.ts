import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

// 告诉 tailwind-merge 哪些是 tailwind.config.cjs 里新加的字号 / 圆角 / 阴影：
// 否则 text-body 会被当成颜色，和 text-fg-1 互相覆盖。
const twMerge = extendTailwindMerge({
  extend: {
    theme: {
      text: [
        "badge",
        "caption",
        "body",
        "strong",
        "section",
        "title",
        "page",
        "metric",
        "hint",
      ],
      radius: ["control", "panel", "dialog"],
      shadow: ["v7-sm", "v7-md", "v7-lg"],
      // 随界面密度变的尺寸：h-control 和调用方传的 h-8 要能互相覆盖
      spacing: ["control", "control-sm", "nav-row", "card-y"],
    },
    classGroups: {
      "min-h": [{ "min-h": ["card"] }],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
