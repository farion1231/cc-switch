import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import { normalizeTomlText } from "@/utils/textNormalization";
import { McpServerSpec } from "../types";

/**
 * 验证 TOML 格式并转换为 JSON 对象
 * @param text TOML 文本
 * @returns 错误信息（空字符串表示成功）
 */
export const validateToml = (text: string): string => {
  if (!text.trim()) return "";
  try {
    const normalized = normalizeTomlText(text);
    const parsed = parseToml(normalized);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      return "mustBeObject";
    }
    return "";
  } catch (e: any) {
    // 返回底层错误信息，由上层进行 i18n 包装
    return e?.message || "parseError";
  }
};

/**
 * 将 McpServerSpec 对象转换为 TOML 字符串
 * 使用 @iarna/toml 的 stringify，自动处理转义与嵌套表
 * 保留所有字段（包括扩展字段如 timeout_ms）
 */
export const mcpServerToToml = (server: McpServerSpec): string => {
  // 先复制所有字段（保留扩展字段）
  const obj: any = { ...server };

  // 去除未定义字段，确保输出更干净
  for (const k of Object.keys(obj)) {
    if (obj[k] === undefined) delete obj[k];
  }

  // stringify 默认会带换行，做一次 trim 以适配文本框展示
  return stringifyToml(obj).trim();
};

/**
 * 将 TOML 文本转换为 McpServerSpec 对象（单个服务器配置）
 * 支持两种格式：
 * 1. 直接的服务器配置（type, command, args 等）
 * 2. [mcp_servers.<id>] 格式（推荐，取第一个服务器）
 * 3. [mcp.servers.<id>] 错误格式（容错解析，同样取第一个服务器）
 * @param tomlText TOML 文本
 * @returns McpServer 对象
 * @throws 解析或转换失败时抛出错误
 */
export const tomlToMcpServer = (tomlText: string): McpServerSpec => {
  if (!tomlText.trim()) {
    throw new Error("TOML 内容不能为空");
  }

  const parsed = parseToml(normalizeTomlText(tomlText));

  // 情况 1: 直接是服务器配置（包含 type/command/url 等字段）
  if (
    parsed.type ||
    parsed.command ||
    parsed.url ||
    parsed.args ||
    parsed.env
  ) {
    return normalizeServerConfig(parsed);
  }

  // 情况 2: [mcp_servers.<id>] 格式（推荐）
  if (parsed.mcp_servers && typeof parsed.mcp_servers === "object") {
    const serverIds = Object.keys(parsed.mcp_servers);
    if (serverIds.length > 0) {
      const firstServer = (parsed.mcp_servers as any)[serverIds[0]];
      return normalizeServerConfig(firstServer);
    }
  }

  // 情况 3: [mcp.servers.<id>] 错误格式（容错解析）
  if (parsed.mcp && typeof parsed.mcp === "object") {
    const mcpObj = parsed.mcp as any;
    if (mcpObj.servers && typeof mcpObj.servers === "object") {
      const serverIds = Object.keys(mcpObj.servers);
      if (serverIds.length > 0) {
        const firstServer = mcpObj.servers[serverIds[0]];
        return normalizeServerConfig(firstServer);
      }
    }
  }

  throw new Error(
    "无法识别的 TOML 格式。请提供单个 MCP 服务器配置，或使用 [mcp_servers.<id>] 格式",
  );
};

/**
 * 规范化服务器配置对象为 McpServer 格式
 * 保留所有字段（包括扩展字段如 timeout_ms）
 */
function normalizeServerConfig(config: any): McpServerSpec {
  if (!config || typeof config !== "object") {
    throw new Error("服务器配置必须是对象");
  }

  const type = (config.type as string) || "stdio";

  // 已知字段列表（用于后续排除）
  const knownFields = new Set<string>();

  if (type === "stdio") {
    if (!config.command || typeof config.command !== "string") {
      throw new Error("stdio 类型的 MCP 服务器必须包含 command 字段");
    }

    const server: McpServerSpec = {
      type: "stdio",
      command: config.command,
    };
    knownFields.add("type");
    knownFields.add("command");

    // 可选字段
    if (config.args && Array.isArray(config.args)) {
      server.args = config.args.map((arg: any) => String(arg));
      knownFields.add("args");
    }
    if (config.env && typeof config.env === "object") {
      const env: Record<string, string> = {};
      for (const [k, v] of Object.entries(config.env)) {
        env[k] = String(v);
      }
      server.env = env;
      knownFields.add("env");
    }
    if (config.cwd && typeof config.cwd === "string") {
      server.cwd = config.cwd;
      knownFields.add("cwd");
    }

    // 保留所有未知字段（如 timeout_ms 等扩展字段）
    for (const key of Object.keys(config)) {
      if (!knownFields.has(key)) {
        server[key] = config[key];
      }
    }

    return server;
  } else if (type === "http" || type === "sse") {
    if (!config.url || typeof config.url !== "string") {
      throw new Error(`${type} 类型的 MCP 服务器必须包含 url 字段`);
    }

    const server: McpServerSpec = {
      type: type as "http" | "sse",
      url: config.url,
    };
    knownFields.add("type");
    knownFields.add("url");

    // 可选字段
    if (config.headers && typeof config.headers === "object") {
      const headers: Record<string, string> = {};
      for (const [k, v] of Object.entries(config.headers)) {
        headers[k] = String(v);
      }
      server.headers = headers;
      knownFields.add("headers");
    }

    // 保留所有未知字段
    for (const key of Object.keys(config)) {
      if (!knownFields.has(key)) {
        server[key] = config[key];
      }
    }

    return server;
  } else {
    throw new Error(`不支持的 MCP 服务器类型: ${type}`);
  }
}

/**
 * 尝试从 TOML 中提取合理的服务器 ID/标题
 * @param tomlText TOML 文本
 * @returns 建议的 ID，失败返回空字符串
 */
export const extractIdFromToml = (tomlText: string): string => {
  try {
    const parsed = parseToml(normalizeTomlText(tomlText));

    // 尝试从 [mcp_servers.<id>] 或 [mcp.servers.<id>] 中提取 ID
    if (parsed.mcp_servers && typeof parsed.mcp_servers === "object") {
      const serverIds = Object.keys(parsed.mcp_servers);
      if (serverIds.length > 0) {
        return serverIds[0];
      }
    }

    if (parsed.mcp && typeof parsed.mcp === "object") {
      const mcpObj = parsed.mcp as any;
      if (mcpObj.servers && typeof mcpObj.servers === "object") {
        const serverIds = Object.keys(mcpObj.servers);
        if (serverIds.length > 0) {
          return serverIds[0];
        }
      }
    }

    // 尝试从 command 中推断
    if (parsed.command && typeof parsed.command === "string") {
      const cmd = parsed.command.split(/[\\/]/).pop() || "";
      return cmd.replace(/\.(exe|bat|sh|js|py)$/i, "");
    }
  } catch {
    // 解析失败，返回空
  }

  return "";
};
