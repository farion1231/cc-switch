import { describe, expect, it } from "vitest";
import {
  CUSTOM_ICON_PREFIX,
  CustomIconError,
  MAX_CUSTOM_ICON_LENGTH,
  MAX_UPLOAD_BYTES,
  isCustomIcon,
  normalizeIconDataUri,
  normalizeIconFile,
} from "@/utils/customIcon";
import { pngDataUri } from "../fixtures/customIcon";

describe("isCustomIcon", () => {
  it("accepts a PNG data URI within the size and dimension limits", () => {
    expect(isCustomIcon(pngDataUri(96, 96))).toBe(true);
    expect(isCustomIcon(pngDataUri(256, 1))).toBe(true);
  });

  it("rejects anything that is not our own PNG data URI", () => {
    expect(isCustomIcon("claude")).toBe(false);
    expect(isCustomIcon("data:image/svg+xml;base64,PHN2Zy8+")).toBe(false);
    expect(isCustomIcon("https://example.com/logo.png")).toBe(false);
    expect(isCustomIcon(undefined)).toBe(false);
    expect(isCustomIcon("")).toBe(false);
    // 前缀对、内容不是 PNG 文件头
    expect(isCustomIcon(`${CUSTOM_ICON_PREFIX}R0lGODlhAQABAAAAACw=`)).toBe(
      false,
    );
    expect(isCustomIcon(`${CUSTOM_ICON_PREFIX}!!!`)).toBe(false);
  });

  it("rejects other formats disguised with an IHDR marker at the PNG offsets", () => {
    // 浏览器按内容识别格式：这是一张 512×512 的 GIF，只是第 12 字节起凑出了 IHDR 和小尺寸
    const gif = [
      ..."GIF89a".split("").map((c) => c.charCodeAt(0)),
      0x00,
      0x02,
      0x00,
      0x02, // 512×512（小端）
      0,
      0,
      ..."IHDR".split("").map((c) => c.charCodeAt(0)),
      0,
      0,
      0,
      96,
      0,
      0,
      0,
      96,
      0x3b,
    ];
    expect(
      isCustomIcon(CUSTOM_ICON_PREFIX + btoa(String.fromCharCode(...gif))),
    ).toBe(false);

    // 签名对、IHDR 长度不是 13
    const header = atob(pngDataUri(96, 96).slice(CUSTOM_ICON_PREFIX.length));
    const badLength = header.slice(0, 11) + "\x0e" + header.slice(12);
    expect(isCustomIcon(CUSTOM_ICON_PREFIX + btoa(badLength))).toBe(false);
  });

  it("rejects icons that bypassed the save-time checks", () => {
    // 文件头声明的尺寸超限（解压炸弹的典型形态）
    expect(isCustomIcon(pngDataUri(50000, 50000))).toBe(false);
    expect(isCustomIcon(pngDataUri(0, 96))).toBe(false);
    // 超过长度上限
    const long = pngDataUri(96, 96) + "A".repeat(MAX_CUSTOM_ICON_LENGTH);
    expect(isCustomIcon(long)).toBe(false);
  });
});

describe("normalizeIconFile", () => {
  const rejectCode = async (file: File) => {
    try {
      await normalizeIconFile(file);
    } catch (error) {
      expect(error).toBeInstanceOf(CustomIconError);
      return (error as CustomIconError).code;
    }
    throw new Error("expected normalizeIconFile to reject");
  };

  it("rejects formats outside the allowlist before reading the file", async () => {
    const gif = new File(["GIF89a"], "logo.gif", { type: "image/gif" });
    const html = new File(["<html>"], "logo.html", { type: "text/html" });
    expect(await rejectCode(gif)).toBe("unsupported");
    expect(await rejectCode(html)).toBe("unsupported");
  });

  it("rejects files larger than the upload limit", async () => {
    const big = new File([new Uint8Array(MAX_UPLOAD_BYTES + 1)], "big.png", {
      type: "image/png",
    });
    expect(await rejectCode(big)).toBe("tooLarge");
  });

  it("falls back to the extension when the browser leaves the type empty", async () => {
    // 类型为空、扩展名是 .svg：通过格式检查，体积超限才被拒，说明扩展名被认出来了
    const svg = new File([new Uint8Array(MAX_UPLOAD_BYTES + 1)], "logo.SVG");
    expect(await rejectCode(svg)).toBe("tooLarge");
  });
});

describe("normalizeIconDataUri", () => {
  const expectCode = async (dataUri: string, code: string) => {
    await expect(normalizeIconDataUri(dataUri)).rejects.toSatisfy(
      (error: unknown) =>
        error instanceof CustomIconError && error.code === code,
    );
  };

  it("only takes the image types the backend download allows", async () => {
    await expectCode("data:image/gif;base64,R0lGODlhAQABAAAAACw=", "unsupported");
    await expectCode("data:text/html;base64,PHN2Zy8+", "unsupported");
    await expectCode("https://example.com/logo.png", "unsupported");
    // 前缀里少了 ;base64
    await expectCode("data:image/png,abc", "unsupported");
  });

  it("rejects data larger than the 256 KB download limit", async () => {
    const oversized = `data:image/png;base64,${"A".repeat(400 * 1024)}`;
    await expectCode(oversized, "tooLarge");
  });
});
