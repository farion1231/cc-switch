import { describe, expect, it } from "vitest";

import { tomlToMcpServer } from "@/utils/tomlUtils";

describe("tomlUtils", () => {
  it("reads http_headers into canonical headers", () => {
    const server = tomlToMcpServer(`
type = "http"
url = "https://example.com/mcp"

[http_headers]
Authorization = "Bearer token"
`);

    expect(server).toEqual({
      type: "http",
      url: "https://example.com/mcp",
      headers: {
        Authorization: "Bearer token",
      },
    });
  });

  it("rejects url without explicit type", () => {
    expect(() =>
      tomlToMcpServer(`
url = "https://example.com/mcp"
`),
    ).toThrow("包含 url 字段时必须显式指定 type 为 http 或 sse");
  });

  it("allows command without explicit type and normalizes to stdio", () => {
    const server = tomlToMcpServer(`
command = "node"
args = ["server.js"]
`);

    expect(server).toEqual({
      type: "stdio",
      command: "node",
      args: ["server.js"],
    });
  });
});
