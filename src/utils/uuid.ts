/**
 * 生成 UUID v4
 *
 * 优先使用 crypto.randomUUID()，不可用时使用 crypto.getRandomValues() 实现
 *
 * 兼容性：
 * - crypto.randomUUID(): Chrome 92+, Safari 15.4+, Firefox 95+
 * - crypto.getRandomValues(): Chrome 11+, Safari 5+, Firefox 21+
 */
export function generateUUID(): string {
  const cryptoApi = globalThis.crypto;

  // 优先使用原生 API
  if (typeof cryptoApi?.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }

  // Fallback: 使用 crypto.getRandomValues 实现 UUID v4
  if (!cryptoApi?.getRandomValues) {
    throw new Error(
      "crypto API not available - please update your operating system",
    );
  }

  const bytes = new Uint8Array(16);
  cryptoApi.getRandomValues(bytes);

  // 设置版本 (4) 和变体 (RFC 4122)
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;

  const hex = Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");

  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
