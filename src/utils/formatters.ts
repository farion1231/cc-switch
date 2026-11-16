/**
 * 格式化 JSON 字符串
 * @param value - 原始 JSON 字符串
 * @returns 格式化后的 JSON 字符串（2 空格缩进）
 * @throws 如果 JSON 格式无效
 */
export function formatJSON(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "";
  }
  const parsed = JSON.parse(trimmed);
  return JSON.stringify(parsed, null, 2);
}

/**
 * TOML 格式化功能已禁用
 *
 * 原因：smol-toml 的 parse/stringify 会丢失所有注释和原有排版。
 * 由于 TOML 常用于配置文件，注释是重要的文档说明，丢失注释会造成严重的用户体验问题。
 *
 * 未来可选方案：
 * - 使用 @ltd/j-toml（支持注释保留，但需额外依赖和复杂的 API）
 * - 实现仅格式化缩进/空白的轻量级方案
 * - 使用 toml-eslint-parser + 自定义生成器
 *
 * 暂时建议：依赖现有的 TOML 语法校验（useCodexTomlValidation），不提供格式化功能。
 */
