import { McpServer, McpServerSpec } from "../types";
import { isWindows } from "@/lib/platform";

export type McpPreset = Omit<McpServer, "enabled" | "description">;

// 创建跨平台 npx 命令配置
// Windows 需要使用 cmd /c wrapper 来执行 npx.cmd
// Mac/Linux 可以直接执行 npx
const createNpxCommand = (
  packageName: string,
  extraArgs: string[] = [],
): { command: string; args: string[] } => {
  if (isWindows()) {
    return {
      command: "cmd",
      args: ["/c", "npx", ...extraArgs, packageName],
    };
  } else {
    return {
      command: "npx",
      args: [...extraArgs, packageName],
    };
  }
};

// 预设 MCP（逻辑简化版）：
// - 仅包含最常用、可快速落地的 stdio 模式示例
// - 不涉及分类/模板/测速等复杂逻辑，默认以 disabled 形式"回种"到 config.json
// - 用户可在 MCP 面板中一键启用/编辑
// - description 字段使用国际化 key，在使用时通过 t() 函数获取翻译
export const mcpPresets: McpPreset[] = [
  {
    id: "fetch",
    name: "mcp-server-fetch",
    tags: ["stdio", "http", "web"],
    server: {
      type: "stdio",
      command: "uvx",
      args: ["mcp-server-fetch"],
    } as McpServerSpec,
    homepage: "https://github.com/modelcontextprotocol/servers",
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch",
  },
  {
    id: "time",
    name: "@modelcontextprotocol/server-time",
    tags: ["stdio", "time", "utility"],
    server: {
      type: "stdio",
      ...createNpxCommand("@modelcontextprotocol/server-time", ["-y"]),
    } as McpServerSpec,
    homepage: "https://github.com/modelcontextprotocol/servers",
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/time",
  },
  {
    id: "memory",
    name: "@modelcontextprotocol/server-memory",
    tags: ["stdio", "memory", "graph"],
    server: {
      type: "stdio",
      ...createNpxCommand("@modelcontextprotocol/server-memory", ["-y"]),
    } as McpServerSpec,
    homepage: "https://github.com/modelcontextprotocol/servers",
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory",
  },
  {
    id: "sequential-thinking",
    name: "@modelcontextprotocol/server-sequential-thinking",
    tags: ["stdio", "thinking", "reasoning"],
    server: {
      type: "stdio",
      ...createNpxCommand("@modelcontextprotocol/server-sequential-thinking", [
        "-y",
      ]),
    } as McpServerSpec,
    homepage: "https://github.com/modelcontextprotocol/servers",
    docs: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking",
  },
  {
    id: "context7",
    name: "@upstash/context7-mcp",
    tags: ["stdio", "docs", "search"],
    server: {
      type: "stdio",
      ...createNpxCommand("@upstash/context7-mcp", ["-y"]),
    } as McpServerSpec,
    homepage: "https://context7.com",
    docs: "https://github.com/upstash/context7/blob/master/README.md",
  },
];

// 获取带国际化描述的预设
export const getMcpPresetWithDescription = (
  preset: McpPreset,
  t: (key: string) => string,
): McpServer => {
  const descriptionKey = `mcp.presets.${preset.id}.description`;
  return {
    ...preset,
    description: t(descriptionKey),
  } as McpServer;
};

export default mcpPresets;
