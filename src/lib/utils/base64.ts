/**
 * 把 URL-safe Base64（RFC 4648 §5）归一到 `atob` 认识的标准字母表，
 * 并还原被 URL 解析吃掉的 `+`（会变成空格）。
 *
 * 标准 Base64 的字母表里不存在 `-` 与 `_`，因此这两条替换不会误伤标准输入。
 *
 * ⚠️ 这里必须与后端 `deeplink/utils.rs::decode_base64_param` 的语义保持一致。
 * 后端依次尝试 STANDARD / STANDARD_NO_PAD / URL_SAFE / URL_SAFE_NO_PAD 四种引擎；
 * 前端若只认标准字母表，同一条链接就会出现「确认框显示不透明 Base64 或空列表、
 * 后端却解码并保存了真实内容」的错位——即用户在看不见 payload 的情况下点了确认。
 */
function toStandardBase64Alphabet(value: string): string {
  return value.replace(/ /g, "+").replace(/-/g, "+").replace(/_/g, "/");
}

function trimOuterLineBreaks(value: string): string {
  return value.replace(/^[\r\n]+|[\r\n]+$/g, "");
}

/**
 * Decode Base64 encoded UTF-8 string
 *
 * This function handles various Base64 edge cases that can occur when
 * Base64 strings are passed through URLs:
 * - Spaces (URL parsing may convert '+' to space)
 * - Missing padding ('=' characters)
 * - Standard and URL-safe alphabets (RFC 4648 §4 and §5)
 *
 * @param str - Base64 encoded string
 * @returns Decoded UTF-8 string
 */
export function decodeBase64Utf8(str: string): string {
  try {
    // Keep spaces intact until they are restored to `+`. Using `trim()` here
    // would discard a URL-decoded `+` at either edge and diverge from the
    // backend decoder, which trims only CR/LF characters.
    let cleaned = toStandardBase64Alphabet(trimOuterLineBreaks(str));

    // Try to decode with standard Base64 first
    try {
      const binString = atob(cleaned);
      const bytes = Uint8Array.from(binString, (m) => m.codePointAt(0)!);
      return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
    } catch (e1) {
      // If standard fails, try adding padding
      const remainder = cleaned.length % 4;
      if (remainder !== 0) {
        cleaned += "=".repeat(4 - remainder);
      }
      const binString = atob(cleaned);
      const bytes = Uint8Array.from(binString, (m) => m.codePointAt(0)!);
      return new TextDecoder("utf-8", { fatal: false }).decode(bytes);
    }
  } catch (e) {
    console.error("Base64 decode error:", e, "Input:", str);
    // Last resort fallback using deprecated but sometimes working method
    try {
      return decodeURIComponent(
        escape(atob(toStandardBase64Alphabet(trimOuterLineBreaks(str)))),
      );
    } catch {
      // If all else fails, return original string
      return str;
    }
  }
}
