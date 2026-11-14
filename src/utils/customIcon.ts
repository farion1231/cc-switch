/**
 * 用户上传的供应商图标。
 *
 * 图片在这里统一重新编码成 96×96 的 PNG，以 data URI 存进 `provider.icon`，
 * 随行数据一起同步、复制。原始文件（SVG、带 EXIF 的 JPEG 等）永远不进库：
 * SVG 只经 <img> 加载（不执行脚本、不加载外部资源），再画到 canvas 上转成像素。
 * 后端 `services/provider/icon.rs` 按同样的前缀和长度上限再校验一次。
 */

export const CUSTOM_ICON_PREFIX = "data:image/png;base64,";

/** 整个 data URI 的长度上限，和后端的 `MAX_CUSTOM_ICON_LENGTH` 一致 */
export const MAX_CUSTOM_ICON_LENGTH = 64 * 1024;

/** 上传文件的体积上限 */
export const MAX_UPLOAD_BYTES = 2 * 1024 * 1024;

/** 解码后的宽高上限，挡住体积很小、声明尺寸巨大的图片把内存撑爆 */
const MAX_SOURCE_DIMENSION = 4096;

/** 输出尺寸：32px 图标框在 3 倍屏上的像素数 */
const OUTPUT_SIZE = 96;

/**
 * 透明 logo 占输出画布的比例，和 ProviderIconBox 里透明 logo（22px）在 32px 框里的比例一致。
 * 自定义图标一律按铺满整框渲染，留白直接画进像素里。
 */
const GLYPH_RATIO = 22 / 32;

const ACCEPTED_TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpeg",
  "image/webp": "webp",
  "image/svg+xml": "svg",
};

/** 给 <input type="file"> 的 accept */
export const CUSTOM_ICON_ACCEPT = Object.keys(ACCEPTED_TYPES).join(",");

export type CustomIconErrorCode =
  | "unsupported"
  | "tooLarge"
  | "decodeFailed"
  | "dimensions";

export class CustomIconError extends Error {
  constructor(public readonly code: CustomIconErrorCode) {
    super(code);
    this.name = "CustomIconError";
  }
}

/** 宽高上限，和后端的 `MAX_CUSTOM_ICON_DIMENSION` 一致 */
const MAX_CUSTOM_ICON_DIMENSION = 256;

/** PNG 签名 + IHDR 块长度（固定 13）+ 块类型，共 16 字节 */
const PNG_HEADER_PREFIX = "\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR";

/**
 * 能不能当自定义图标渲染。
 *
 * 渲染前的兜底：SQL 导入、云同步、旧版写入都可能绕过保存时的校验，这里按同样的
 * 长度上限检查，再从 PNG 文件头读出宽高，挡住「体积小、声明尺寸巨大」的图片。
 * 不合格的值当作没有图标，显示首字母。
 */
export function isCustomIcon(icon?: string | null): icon is string {
  if (
    !icon ||
    !icon.startsWith(CUSTOM_ICON_PREFIX) ||
    icon.length > MAX_CUSTOM_ICON_LENGTH
  ) {
    return false;
  }
  // PNG 前 24 字节：签名 8 + IHDR 块长度 4 + 类型 4 + 宽 4 + 高 4，正好 32 个 base64 字符
  let header: string;
  try {
    header = atob(
      icon.slice(CUSTOM_ICON_PREFIX.length, CUSTOM_ICON_PREFIX.length + 32),
    );
  } catch {
    return false;
  }
  // 浏览器按内容识别图片格式，不看 data URI 声明的类型：签名不对的内容（比如伪装的 GIF）
  // 会按它真实的格式解码，下面从 IHDR 读出的宽高就不算数了
  if (header.length < 24 || !header.startsWith(PNG_HEADER_PREFIX)) return false;
  const readU32 = (offset: number) =>
    ((header.charCodeAt(offset) << 24) |
      (header.charCodeAt(offset + 1) << 16) |
      (header.charCodeAt(offset + 2) << 8) |
      header.charCodeAt(offset + 3)) >>>
    0;
  const width = readU32(16);
  const height = readU32(20);
  return (
    width > 0 &&
    height > 0 &&
    width <= MAX_CUSTOM_ICON_DIMENSION &&
    height <= MAX_CUSTOM_ICON_DIMENSION
  );
}

/** 按扩展名补 MIME：部分系统选 SVG 时 file.type 为空 */
function resolveMimeType(file: File): string | undefined {
  if (file.type && ACCEPTED_TYPES[file.type]) return file.type;
  const ext = file.name.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "webp":
      return "image/webp";
    case "svg":
      return "image/svg+xml";
    default:
      return undefined;
  }
}

function readAsDataUrl(file: File, mimeType: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = String(reader.result ?? "");
      // 用核对过的 MIME 重写前缀，不信浏览器给的 file.type
      const comma = result.indexOf(",");
      if (comma < 0) {
        reject(new CustomIconError("decodeFailed"));
        return;
      }
      resolve(`data:${mimeType};base64,${result.slice(comma + 1)}`);
    };
    reader.onerror = () => reject(new CustomIconError("decodeFailed"));
    reader.readAsDataURL(file);
  });
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.decoding = "async";
    img.onload = () => resolve(img);
    img.onerror = () => reject(new CustomIconError("decodeFailed"));
    // CSP 的 img-src 允许 data:，不允许 blob:，所以走 data URI
    img.src = src;
  });
}

function createCanvas(size: number): CanvasRenderingContext2D {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new CustomIconError("decodeFailed");
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  return ctx;
}

/** 四个角都不透明且接近正方形：当作自带底色的方块图标，铺满整框 */
function looksLikeTile(img: HTMLImageElement, w: number, h: number): boolean {
  const aspect = w / h;
  if (aspect < 0.9 || aspect > 1.1) return false;
  const probe = createCanvas(OUTPUT_SIZE);
  probe.drawImage(img, 0, 0, OUTPUT_SIZE, OUTPUT_SIZE);
  const last = OUTPUT_SIZE - 2;
  return [
    [1, 1],
    [last, 1],
    [1, last],
    [last, last],
  ].every(([x, y]) => probe.getImageData(x, y, 1, 1).data[3] >= 250);
}

/**
 * 把用户选的图片转成可以存进 `provider.icon` 的 PNG data URI。
 * 失败时抛 {@link CustomIconError}，调用方按 code 提示。
 */
export async function normalizeIconFile(file: File): Promise<string> {
  const mimeType = resolveMimeType(file);
  if (!mimeType) throw new CustomIconError("unsupported");
  if (file.size > MAX_UPLOAD_BYTES) throw new CustomIconError("tooLarge");

  return rasterize(await readAsDataUrl(file, mimeType));
}

/** 后端下载的图标最大 256 KB，base64 之后再留一点前缀的余量 */
const MAX_DOWNLOADED_DATA_URI_LENGTH = Math.ceil((256 * 1024 * 4) / 3) + 64;

/**
 * 把深链接 `iconUrl` 下载回来的图片（后端 `fetch_deeplink_icon` 核对过类型的 data URI）
 * 转成可以存进 `provider.icon` 的 PNG data URI，流程和上传一样。
 */
export async function normalizeIconDataUri(dataUri: string): Promise<string> {
  const accepted = Object.keys(ACCEPTED_TYPES).some((type) =>
    dataUri.startsWith(`data:${type};base64,`),
  );
  if (!accepted) throw new CustomIconError("unsupported");
  if (dataUri.length > MAX_DOWNLOADED_DATA_URI_LENGTH) {
    throw new CustomIconError("tooLarge");
  }
  return rasterize(dataUri);
}

/** 加载图片并画成 96×96 PNG data URI */
async function rasterize(src: string): Promise<string> {
  const img = await loadImage(src);

  // 没写宽高的 SVG 可能报 0，按正方形处理
  const width = img.naturalWidth || OUTPUT_SIZE;
  const height = img.naturalHeight || OUTPUT_SIZE;
  if (width > MAX_SOURCE_DIMENSION || height > MAX_SOURCE_DIMENSION) {
    throw new CustomIconError("dimensions");
  }

  const ctx = createCanvas(OUTPUT_SIZE);
  try {
    if (looksLikeTile(img, width, height)) {
      // 居中裁成正方形铺满
      const side = Math.min(width, height);
      ctx.drawImage(
        img,
        (width - side) / 2,
        (height - side) / 2,
        side,
        side,
        0,
        0,
        OUTPUT_SIZE,
        OUTPUT_SIZE,
      );
    } else {
      // 等比缩进中间的 logo 区域，四周透明
      const box = OUTPUT_SIZE * GLYPH_RATIO;
      const scale = box / Math.max(width, height);
      const w = width * scale;
      const h = height * scale;
      ctx.drawImage(img, (OUTPUT_SIZE - w) / 2, (OUTPUT_SIZE - h) / 2, w, h);
    }
    const dataUri = ctx.canvas.toDataURL("image/png");
    if (!isCustomIcon(dataUri)) {
      throw new CustomIconError("decodeFailed");
    }
    return dataUri;
  } catch (error) {
    if (error instanceof CustomIconError) throw error;
    // WebKit 画某些 SVG 后 canvas 会被标记为不可读，toDataURL 抛 SecurityError
    throw new CustomIconError("decodeFailed");
  }
}
