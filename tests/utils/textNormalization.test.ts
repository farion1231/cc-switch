import { parse as parseToml } from "smol-toml";
import { describe, expect, it } from "vitest";
import { normalizeTomlText } from "@/utils/textNormalization";
import { tomlToMcpServer, validateToml } from "@/utils/tomlUtils";

describe("normalizeTomlText", () => {
  it("normalizes smart quotes used as string delimiters", () => {
    expect(normalizeTomlText("command = “npx”\nargs = [‘-y’]")).toBe(
      "command = \"npx\"\nargs = ['-y']",
    );
    expect(normalizeTomlText("[mcp_servers.＂demo＂]")).toBe(
      '[mcp_servers."demo"]',
    );
  });

  it("keeps smart quotes inside strings opened by ASCII quotes", () => {
    const input = [
      'a = "don’t"',
      'b = "say“hello”"',
      "c = 'don’t'",
      'd = """multi “line” ’text’"""',
      "e = '''it’s'''",
      'f = "escaped \\" then “x”"',
    ].join("\n");
    expect(normalizeTomlText(input)).toBe(input);
  });

  it("keeps quotes of the other family inside smart-quoted strings", () => {
    expect(normalizeTomlText("a = “don’t”")).toBe('a = "don’t"');
    expect(parseToml(normalizeTomlText("a = “don’t”")).a).toBe("don’t");
  });

  it("closes an ASCII-opened string with a smart quote when nothing else closes it", () => {
    // 编辑器逐字归一化后，输入法打出的左引号已是 ASCII，右引号仍是弯引号
    expect(normalizeTomlText('model = "gpt”\nx = 1')).toBe(
      'model = "gpt"\nx = 1',
    );
    expect(normalizeTomlText("name = 'abc’")).toBe("name = 'abc'");
  });

  it("repairs smart closers that are followed by more ASCII quotes on the line", () => {
    // 合法文档不会走到这里：只有严格结果解析失败时才把字符串里的弯引号当闭合符
    expect(normalizeTomlText('args = ["npx”, "-y"]')).toBe(
      'args = ["npx", "-y"]',
    );
    expect(normalizeTomlText('model = "gpt” # "example"')).toBe(
      'model = "gpt" # "example"',
    );
  });

  it("turns escaped smart double quotes into ASCII escapes", () => {
    const normalized = normalizeTomlText("args = [“say \\“hello\\””]");
    expect(normalized).toBe('args = ["say \\"hello\\""]');
    expect(parseToml(normalized).args).toEqual(['say "hello"']);
  });

  it("leaves comments untouched", () => {
    const input = "# “comment” it’s\nkey = “v” # don’t";
    expect(normalizeTomlText(input)).toBe(
      '# “comment” it’s\nkey = "v" # don’t',
    );
  });

  it("does not touch unterminated strings beyond their opening quote", () => {
    expect(normalizeTomlText("x = “abc")).toBe('x = "abc');
  });
});

describe("tomlUtils with smart quotes in valid strings (#8559)", () => {
  it("preserves a right single quotation mark in args", () => {
    const input = 'command = "echo"\nargs = ["don’t"]';
    expect(validateToml(input)).toBe("");
    expect(tomlToMcpServer(input).args).toEqual(["don’t"]);
  });

  it("accepts curly double quotes inside a basic string", () => {
    const input = 'command = "echo"\nargs = ["say“hello”"]';
    expect(validateToml(input)).toBe("");
    expect(tomlToMcpServer(input).args).toEqual(["say“hello”"]);
  });

  it("accepts curly single quotes inside a literal string", () => {
    const input = "command = 'echo'\nargs = ['don’t']";
    expect(validateToml(input)).toBe("");
    expect(tomlToMcpServer(input).args).toEqual(["don’t"]);
  });

  it("still accepts configs written entirely with smart quotes", () => {
    const input = "command = “npx”\nargs = [“-y”, “pkg”]";
    expect(validateToml(input)).toBe("");
    expect(tomlToMcpServer(input)).toMatchObject({
      command: "npx",
      args: ["-y", "pkg"],
    });
  });
});
