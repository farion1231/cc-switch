import { describe, expect, it } from "vitest";
import type { McpServer } from "@/types";
import { filterMcpServers, type McpServerEntry } from "./mcpFilter";

const defaultApps: McpServer["apps"] = {
  claude: false,
  codex: false,
  gemini: false,
  opencode: false,
  openclaw: false,
  hermes: false,
};

function createServer(
  id: string,
  apps: Partial<McpServer["apps"]> = {},
): McpServer {
  return {
    id,
    name: id,
    server: { type: "stdio", command: "mock-server" },
    apps: { ...defaultApps, ...apps },
  };
}

describe("filterMcpServers", () => {
  const entries: McpServerEntry[] = [
    ["claude-mcp", createServer("claude-mcp", { claude: true })],
    ["codex-mcp", createServer("codex-mcp", { codex: true })],
    ["shared-mcp", createServer("shared-mcp", { claude: true, codex: true })],
  ];

  it("returns every server for the total filter", () => {
    expect(filterMcpServers(entries, "all")).toBe(entries);
  });

  it("returns only servers enabled for the selected app", () => {
    expect(filterMcpServers(entries, "claude").map(([id]) => id)).toEqual([
      "claude-mcp",
      "shared-mcp",
    ]);
    expect(filterMcpServers(entries, "codex").map(([id]) => id)).toEqual([
      "codex-mcp",
      "shared-mcp",
    ]);
  });

  it("excludes servers that are disabled for the selected app", () => {
    expect(filterMcpServers(entries, "gemini")).toEqual([]);
  });
});
