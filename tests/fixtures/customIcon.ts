import { CUSTOM_ICON_PREFIX } from "@/utils/customIcon";

/** 只有签名和 IHDR 的 PNG 文件头，够渲染前的检查用（jsdom 不解码图片） */
export function pngDataUri(width: number, height: number): string {
  const bytes = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
  bytes.push(..."IHDR".split("").map((c) => c.charCodeAt(0)));
  for (const n of [width, height]) {
    bytes.push(
      (n >>> 24) & 0xff,
      (n >>> 16) & 0xff,
      (n >>> 8) & 0xff,
      n & 0xff,
    );
  }
  bytes.push(8, 6, 0, 0, 0);
  return CUSTOM_ICON_PREFIX + btoa(String.fromCharCode(...bytes));
}
