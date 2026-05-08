/**
 * Decode Base64 encoded UTF-8 string
 *
 * This function handles various Base64 edge cases that can occur when
 * Base64 strings are passed through URLs:
 * - Spaces (URL parsing may convert '+' to space)
 * - Missing padding ('=' characters)
 * - Different Base64 variants
 *
 * @param str - Base64 encoded string
 * @returns Decoded UTF-8 string
 */
export function decodeBase64Utf8(str: string): string {
  try {
    // Clean up the input: replace spaces with + (URL parsing may convert + to space)
    let cleaned = str.trim().replace(/ /g, "+");

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
      return decodeURIComponent(escape(atob(str.replace(/ /g, "+"))));
    } catch {
      // If all else fails, return original string
      return str;
    }
  }
}
