import { McpServer } from "../types";

export type McpPreset = {
  id: string;
  name: string;
  description: string;
  tags?: string[];
  server: McpServer;
  homepage?: string;
  docs?: string;
};

// 预设 MCP（逻辑简化版）：
// - 仅包含最常用、可快速落地的 stdio 模式示例
// - 不涉及分类/模板/测速等复杂逻辑，默认以 disabled 形式“回种”到 config.json
// - 用户可在 MCP 面板中一键启用/编辑
export const mcpPresets: McpPreset[] = [
  {
    id: "fetch",
    name: "mcp-server-fetch",
    description:
      "通用 HTTP Fetch（stdio，经 uvx 运行 mcp-server-fetch），适合快速请求接口/抓取数据",
    tags: ["stdio", "http"],
    server: {
      type: "stdio",
      command: "uvx",
      args: ["mcp-server-fetch"],
    } as McpServer,
  },
  {
    id: "context7",
    name: "mcp-context7",
    description: "Context7 示例（无需环境变量），可按需在表单中调整参数",
    tags: ["stdio", "docs"],
    server: {
      type: "stdio",
      command: "uvx",
      // 使用 fetch 服务器作为基础示例，用户可在表单中补充 args
      args: ["mcp-server-fetch"],
    } as McpServer,
    docs: "https://github.com/context7",
  },
];

export default mcpPresets;
