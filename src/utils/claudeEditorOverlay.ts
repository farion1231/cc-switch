const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * 新增 Claude 供应商时，把预设的字段（只有关键字段和独有字段）套在当前 live 上显示，
 * 和切过去之后的结果一致：预设的顶层键、`env` 键覆盖 live 的同名键，其余保持 live 的
 * 内容和顺序。关键字段怎么认由后端决定，这里只做覆盖。
 */
export function overlayClaudeProviderFields(
  base: Record<string, unknown>,
  fields: Record<string, unknown>,
): Record<string, unknown> {
  const result: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(fields)) {
    if (key !== "env") {
      result[key] = value;
    }
  }
  if ("env" in base || "env" in fields) {
    result.env = {
      ...(isRecord(base.env) ? base.env : {}),
      ...(isRecord(fields.env) ? fields.env : {}),
    };
  }
  return result;
}
