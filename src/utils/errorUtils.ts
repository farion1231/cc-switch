/**
 * 从各种错误对象中提取错误信息
 * @param error 错误对象
 * @returns 提取的错误信息字符串
 */
export const extractErrorMessage = (error: unknown): string => {
  if (!error) return "";
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === "object") {
    const errObject = error as Record<string, unknown>;

    const candidate = errObject.message ?? errObject.error ?? errObject.detail;
    if (typeof candidate === "string" && candidate.trim()) {
      return candidate;
    }

    const payload = errObject.payload;
    if (typeof payload === "string" && payload.trim()) {
      return payload;
    }
    if (payload && typeof payload === "object") {
      const payloadObj = payload as Record<string, unknown>;
      const payloadCandidate =
        payloadObj.message ?? payloadObj.error ?? payloadObj.detail;
      if (typeof payloadCandidate === "string" && payloadCandidate.trim()) {
        return payloadCandidate;
      }
    }
  }

  return "";
};

/**
 * 将已知的 MCP 相关后端错误（通常为中文硬编码）映射为 i18n 文案
 * 采用包含式匹配，尽量稳健地覆盖不同上下文的相似消息。
 * 若无法识别，返回空字符串以便调用方回退到原始 detail 或默认 i18n。
 */
export const translateMcpBackendError = (
  message: string,
  t: (key: string, opts?: any) => string,
): string => {
  if (!message) return "";
  const msg = String(message).trim();

  // 基础字段与结构校验相关
  if (msg.includes("MCP 服务器 ID 不能为空")) {
    return t("mcp.error.idRequired");
  }
  if (
    msg.includes("MCP 服务器定义必须为 JSON 对象") ||
    msg.includes("MCP 服务器条目必须为 JSON 对象") ||
    msg.includes("MCP 服务器条目缺少 server 字段") ||
    msg.includes("MCP 服务器 server 字段必须为 JSON 对象") ||
    msg.includes("MCP 服务器连接定义必须为 JSON 对象") ||
    msg.includes("MCP 服务器 '" /* 不是对象 */) ||
    msg.includes("不是对象") ||
    msg.includes("服务器配置必须是对象") ||
    msg.includes("MCP 服务器 name 必须为字符串") ||
    msg.includes("MCP 服务器 description 必须为字符串") ||
    msg.includes("MCP 服务器 homepage 必须为字符串") ||
    msg.includes("MCP 服务器 docs 必须为字符串") ||
    msg.includes("MCP 服务器 tags 必须为字符串数组") ||
    msg.includes("MCP 服务器 enabled 必须为布尔值")
  ) {
    return t("mcp.error.jsonInvalid");
  }
  if (msg.includes("MCP 服务器 type 必须是")) {
    return t("mcp.error.jsonInvalid");
  }

  // 必填字段
  if (
    msg.includes("stdio 类型的 MCP 服务器缺少 command 字段") ||
    msg.includes("必须包含 command 字段")
  ) {
    return t("mcp.error.commandRequired");
  }
  if (
    msg.includes("http 类型的 MCP 服务器缺少 url 字段") ||
    msg.includes("sse 类型的 MCP 服务器缺少 url 字段") ||
    msg.includes("必须包含 url 字段") ||
    msg === "URL 不能为空"
  ) {
    return t("mcp.wizard.urlRequired");
  }

  // 文件解析/序列化
  if (
    msg.includes("解析 ~/.claude.json 失败") ||
    msg.includes("解析 config.toml 失败") ||
    msg.includes("无法识别的 TOML 格式") ||
    msg.includes("TOML 内容不能为空")
  ) {
    return t("mcp.error.tomlInvalid");
  }
  if (msg.includes("序列化 config.toml 失败")) {
    return t("mcp.error.tomlInvalid");
  }

  return "";
};
