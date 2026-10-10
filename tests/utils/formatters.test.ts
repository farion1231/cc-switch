import { describe, expect, it } from "vitest";
import { formatJSON, parseSmartMcpJson } from "@/utils/formatters";

describe("formatJSON", () => {
  it("formats valid JSON with 2 spaces indentation", () => {
    expect(formatJSON('{"a":1,"b":[2,3]}')).toBe(
      '{\n  "a": 1,\n  "b": [\n    2,\n    3\n  ]\n}'
    );
  });

  it("handles empty and whitespace input", () => {
    expect(formatJSON("")).toBe("");
    expect(formatJSON("   \n\t  ")).toBe("");
  });

  it("strips leading BOM marker", () => {
    expect(formatJSON('\uFEFF{"name":"test"}')).toBe('{\n  "name": "test"\n}');
  });

  it("throws error for invalid JSON", () => {
    expect(() => formatJSON("{invalid-json}")).toThrow();
  });
});

describe("parseSmartMcpJson", () => {
  it("parses empty string to empty config", () => {
    expect(parseSmartMcpJson("")).toEqual({
      config: {},
      formattedConfig: "",
    });
  });

  it("parses pure MCP config object", () => {
    const raw = '{"command":"npx","args":["-y","@modelcontextprotocol/server"]}';
    const res = parseSmartMcpJson(raw);
    expect(res.id).toBeUndefined();
    expect(res.config).toEqual({
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server"],
    });
    expect(res.formattedConfig).toContain('"command": "npx"');
  });

  it("parses single keyed wrapper object extracting id and config", () => {
    const raw = '{"my-server":{"command":"node","args":["server.js"]}}';
    const res = parseSmartMcpJson(raw);
    expect(res.id).toBe("my-server");
    expect(res.config).toEqual({
      command: "node",
      args: ["server.js"],
    });
  });

  it("parses key-value fragment without outer braces", () => {
    const raw = '"test-server": {"command": "python", "args": ["main.py"]}';
    const res = parseSmartMcpJson(raw);
    expect(res.id).toBe("test-server");
    expect(res.config).toEqual({
      command: "python",
      args: ["main.py"],
    });
  });

  it("handles leading BOM marker", () => {
    const raw = '\uFEFF"server": {"command": "echo"}';
    const res = parseSmartMcpJson(raw);
    expect(res.id).toBe("server");
    expect(res.config).toEqual({ command: "echo" });
  });
});
