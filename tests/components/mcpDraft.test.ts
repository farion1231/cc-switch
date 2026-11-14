import { describe, expect, it } from "vitest";
import {
  connectionOf,
  jsonTextOf,
  parseJsonText,
  recognizePaste,
  SECRET_MASK,
  specOf,
  summaryOf,
  uniqueId,
  validateDraft,
} from "@/components/mcp/mcpDraft";

describe("mcpDraft paste recognition", () => {
  it("recognizes a single named mcpServers entry", () => {
    const result = recognizePaste(
      JSON.stringify({
        mcpServers: {
          linear: {
            type: "http",
            url: "https://mcp.linear.app/mcp",
            headers: { Authorization: "Bearer x" },
          },
        },
      }),
    );
    expect(result).toEqual({
      ok: true,
      format: "mcpServers",
      items: [
        {
          name: "linear",
          spec: {
            type: "http",
            url: "https://mcp.linear.app/mcp",
            headers: { Authorization: "Bearer x" },
          },
        },
      ],
    });
  });

  it("recognizes Codex TOML with env subtables", () => {
    const result = recognizePaste(
      [
        "[mcp_servers.serena]",
        'command = "uvx"',
        'args = ["serena"]',
        "",
        "[mcp_servers.serena.env]",
        'LOG = "info"',
        "",
        "[mcp_servers.docs]",
        'url = "https://docs.example.com/mcp"',
      ].join("\n"),
    );
    expect(result && result.ok && result.format).toBe("codexToml");
    expect(result && result.ok && result.items).toEqual([
      {
        name: "serena",
        spec: {
          type: "stdio",
          command: "uvx",
          args: ["serena"],
          env: { LOG: "info" },
        },
      },
      {
        name: "docs",
        spec: { type: "http", url: "https://docs.example.com/mcp" },
      },
    ]);
  });

  it("normalizes OpenCode local and remote entries", () => {
    const result = recognizePaste(
      JSON.stringify({
        mcp: {
          local: {
            type: "local",
            command: ["npx", "-y", "pkg"],
            environment: { A: "1" },
          },
          remote: { type: "remote", url: "https://x.dev/mcp" },
        },
      }),
    );
    expect(result && result.ok && result.items).toEqual([
      {
        name: "local",
        spec: {
          type: "stdio",
          command: "npx",
          args: ["-y", "pkg"],
          env: { A: "1" },
        },
      },
      { name: "remote", spec: { type: "http", url: "https://x.dev/mcp" } },
    ]);
  });

  it("accepts a bare server object and a name fragment", () => {
    const single = recognizePaste('{"command": "uvx", "args": ["fetch"]}');
    expect(single && single.ok && single.format).toBe("single");
    expect(single && single.ok && single.items[0].name).toBe("");

    const fragment = recognizePaste('"fetch": {"command": "uvx"}');
    expect(fragment && fragment.ok && fragment.format).toBe("named");
    expect(fragment && fragment.ok && fragment.items[0].name).toBe("fetch");
  });

  it("maps streamable-http to http and reports unrecognized text", () => {
    const result = recognizePaste(
      '{"type": "streamable-http", "url": "https://a.dev"}',
    );
    expect(result && result.ok && result.items[0].spec.type).toBe("http");
    expect(recognizePaste("not json")).toEqual({ ok: false });
    expect(recognizePaste("   ")).toBeNull();
  });
});

describe("mcpDraft form ⇄ JSON", () => {
  const spec = {
    type: "stdio" as const,
    command: "uvx",
    args: ["serena"],
    env: { TOKEN: "secret-value" },
    startup_timeout_ms: 30000,
  };

  it("keeps unknown fields and masks secrets only for display", () => {
    const conn = connectionOf(spec);
    expect(conn.extra).toEqual({ startup_timeout_ms: 30000 });
    expect(specOf(conn)).toEqual(spec);
    const masked = JSON.parse(jsonTextOf(conn, false));
    expect(masked.env.TOKEN).toBe(SECRET_MASK);
    expect(JSON.parse(jsonTextOf(conn, true)).env.TOKEN).toBe("secret-value");
  });

  it("restores masked values when parsing the JSON tab", () => {
    const conn = connectionOf(spec);
    const edited = jsonTextOf(conn, false).replace('"uvx"', '"npx"');
    const parsed = parseJsonText(edited, conn);
    expect(parsed.ok && parsed.spec).toEqual({ ...spec, command: "npx" });
  });

  it("refuses a placeholder whose key has no original value instead of saving an empty secret", () => {
    const conn = connectionOf(spec);
    // 改了键名、占位符还在：原值按键名找不到，不能悄悄存成 ""
    const renamed = jsonTextOf(conn, false).replace('"TOKEN"', '"API_TOKEN"');
    const parsed = parseJsonText(renamed, conn);
    expect(!parsed.ok && parsed.error).toEqual({
      kind: "maskedUnknown",
      field: "env",
      key: "API_TOKEN",
    });
    // 填了真实值就正常通过
    const filled = renamed.replace(`"${SECRET_MASK}"`, '"new-secret"');
    const ok = parseJsonText(filled, conn);
    expect(ok.ok && ok.spec.env).toEqual({ API_TOKEN: "new-secret" });
  });

  it("reports the line of a JSON syntax error", () => {
    const parsed = parseJsonText(
      '{\n  "command": "uvx"\n  "args": []\n}',
      connectionOf(spec),
    );
    expect(parsed.ok).toBe(false);
    expect(!parsed.ok && parsed.error.kind).toBe("syntax");
  });

  it("summarizes rows without env or headers", () => {
    expect(summaryOf(spec)).toBe("uvx serena");
    expect(
      summaryOf({
        type: "http",
        url: "https://api.example.com/mcp",
        headers: { Authorization: "Bearer y" },
      }),
    ).toBe("api.example.com/mcp");
  });
});

describe("mcpDraft validation", () => {
  it("checks the name, command and URL", () => {
    const stdio = connectionOf({ type: "stdio", command: "" });
    expect(
      validateDraft("", stdio, { checkName: true, existingIds: [] }),
    ).toEqual({ name: "nameRequired", command: "commandRequired" });
    expect(
      validateDraft("a b", stdio, { checkName: true, existingIds: [] }).name,
    ).toBe("nameSpaces");
    expect(
      validateDraft("fetch", stdio, { checkName: true, existingIds: ["fetch"] })
        .name,
    ).toBe("nameExists");
    const http = connectionOf({ type: "http", url: "mcp.example.com" });
    expect(
      validateDraft("x", http, { checkName: false, existingIds: [] }),
    ).toEqual({ url: "urlScheme" });
  });

  it("makes template ids unique", () => {
    expect(uniqueId("fetch", ["fetch", "fetch-1"])).toBe("fetch-2");
    expect(uniqueId("time", [])).toBe("time");
  });
});
