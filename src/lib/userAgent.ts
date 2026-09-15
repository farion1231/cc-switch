/**
 * 自定义 User-Agent 合法性校验。
 *
 * 与后端 `parse_custom_user_agent`（基于 `http::HeaderValue::from_str`）口径严格一致：
 * HeaderValue 按**字节**判定合法性，规则为 `b >= 32 && b != 127 || b == '\t'`。也就是说：
 * - 制表符（\t）、可见 ASCII（0x20–0x7E）、以及任意非 ASCII 字符（UTF-8 字节均 ≥ 0x80）都合法；
 * - 仅控制字符非法：除 \t 外的 0x00–0x1F（含换行）与 0x7F（DEL）。
 *
 * 空串（trim 后为空）视为"未设置"，合法。
 */
export function isValidUserAgentHeader(value: string): boolean {
  const trimmed = value.trim();
  if (trimmed === "") return true;
  // eslint-disable-next-line no-control-regex
  return !/[\x00-\x08\x0a-\x1f\x7f]/.test(trimmed);
}
