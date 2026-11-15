const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/**
 * auto mode 的服务端检查只有官方端点支持，经网关时 Claude Code 每个会话都会弹一次需要
 * 确认的提示；设成 "0" 改用 Claude Code 自己的检查，提示不再出现。
 */
export const CLAUDE_AUTO_MODE_SERVER_ENV = "CLAUDE_CODE_AUTO_MODE_SERVER";

/**
 * 新增 Claude 供应商时补上网关默认值：除官方和云服务商（Bedrock、Vertex 直连，服务端
 * 检查随平台上线）外，默认关掉 auto mode 的服务端检查。预设自己写了的以预设为准。
 */
export function withClaudeGatewayDefaults(
  fields: Record<string, unknown>,
  category: string | undefined,
): Record<string, unknown> {
  if (category === "official" || category === "cloud_provider") {
    return fields;
  }
  const env = isRecord(fields.env) ? fields.env : {};
  if (CLAUDE_AUTO_MODE_SERVER_ENV in env) {
    return fields;
  }
  return { ...fields, env: { ...env, [CLAUDE_AUTO_MODE_SERVER_ENV]: "0" } };
}

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
