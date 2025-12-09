/**
 * 将常见的中文/全角/弯引号统一为 ASCII 引号，以避免 TOML 解析失败。
 * - 双引号：” “ „ ‟ ＂ → "
 * - 单引号：’ ‘ ＇ → '
 * 保守起见，不替换书名号/角引号（《》、「」等），避免误伤内容语义。
 */
export const normalizeQuotes = (text: string): string => {
  if (!text) return text;
  return (
    text
      // 双引号族 → "
      .replace(/[“”„‟＂]/g, '"')
      // 单引号族 → '
      .replace(/[‘’＇]/g, "'")
  );
};

/**
 * 专用于 TOML 文本的归一化；目前等同于 normalizeQuotes，后续可扩展（如空白、行尾等）。
 */
export const normalizeTomlText = (text: string): string =>
  normalizeQuotes(text);
