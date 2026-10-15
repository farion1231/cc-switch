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
      ],
      radius: ["control", "panel", "dialog"],
      shadow: ["v7-sm", "v7-md", "v7-lg"],
    },
  },
});

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
