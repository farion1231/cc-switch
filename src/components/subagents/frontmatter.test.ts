import { describe, expect, it } from "vitest";
import {
  FrontmatterError,
  SUBAGENT_NAME_RE,
  buildSubagentMarkdown,
  getSubagentBody,
  parseSubagentMarkdown,
  parseSubagentMarkdownLoose,
  replaceSubagentBody,
  safeParseSubagentMarkdown,
  updateSubagentFrontmatter,
} from "./frontmatter";

const SAMPLE = `---
# 团队共用的审查 agent
name: code-reviewer # 别改名
description: Reviews diffs for bugs. Use proactively after edits.
tools: Read, Grep, Glob
permissionMode: plan
hooks:
  PreToolUse:
    - matcher: Bash
model: sonnet
color: blue
---

You are a senior reviewer.

---

Keep feedback short.
`;

describe("parseSubagentMarkdown", () => {
  it("reads the known fields and the body", () => {
    const parsed = parseSubagentMarkdown(SAMPLE);
    expect(parsed.hasFrontmatter).toBe(true);
    expect(parsed.fields).toEqual({
      name: "code-reviewer",
      description: "Reviews diffs for bugs. Use proactively after edits.",
      tools: ["Read", "Grep", "Glob"],
      model: "sonnet",
      color: "blue",
    });
    expect(parsed.toolsFormat).toBe("string");
    // 正文里的 --- 分隔线不能被当成 frontmatter 的结尾
    expect(parsed.body).toBe(
      "You are a senior reviewer.\n\n---\n\nKeep feedback short.\n",
    );
  });

  it("reads tools written as a YAML list", () => {
    const parsed = parseSubagentMarkdown(
      "---\nname: a\ntools:\n  - Read\n  - mcp__github__search\n---\nbody",
    );
    expect(parsed.fields.tools).toEqual(["Read", "mcp__github__search"]);
    expect(parsed.toolsFormat).toBe("list");
    expect(parsed.body).toBe("body");
  });

  it("treats an empty tools value as not set", () => {
    const parsed = parseSubagentMarkdown("---\nname: a\ntools:\n---\n");
    expect(parsed.fields.tools).toBeUndefined();
  });

  it("handles content without frontmatter", () => {
    const parsed = parseSubagentMarkdown("Just a prompt.\n");
    expect(parsed.hasFrontmatter).toBe(false);
    expect(parsed.fields).toEqual({});
    expect(parsed.toolsFormat).toBeNull();
    expect(parsed.body).toBe("Just a prompt.\n");
  });

  it("handles CRLF files and a BOM", () => {
    const parsed = parseSubagentMarkdown(
      "\uFEFF---\r\nname: win\r\nmodel: haiku\r\n---\r\n\r\nBody\r\n",
    );
    expect(parsed.fields).toEqual({ name: "win", model: "haiku" });
    expect(parsed.body).toBe("Body\r\n");
  });

  it("throws on invalid YAML", () => {
    expect(() => parseSubagentMarkdown("---\nname: [oops\n---\n")).toThrow(
      FrontmatterError,
    );
    const result = safeParseSubagentMarkdown("---\nname: [oops\n---\n");
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.code).toBe("yaml");
  });

  it("throws when frontmatter is not closed or not a mapping", () => {
    const unclosed = safeParseSubagentMarkdown("---\nname: a\nbody");
    expect(unclosed).toMatchObject({ ok: false, code: "unclosed" });
    const scalar = safeParseSubagentMarkdown("---\njust text\n---\n");
    expect(scalar).toMatchObject({ ok: false, code: "not_map" });
  });
});

describe("updateSubagentFrontmatter", () => {
  it("keeps unknown keys, comments, order and the body", () => {
    const next = updateSubagentFrontmatter(SAMPLE, {
      description: "Reviews diffs: bugs, style.",
      model: "opus",
    });
    expect(next).toContain("# 团队共用的审查 agent");
    expect(next).toContain("name: code-reviewer # 别改名");
    expect(next).toContain("permissionMode: plan");
    expect(next).toContain("hooks:\n  PreToolUse:\n    - matcher: Bash");
    // 含 `: ` 的值自动加引号，仍是合法 YAML
    expect(parseSubagentMarkdown(next).fields.description).toBe(
      "Reviews diffs: bugs, style.",
    );
    expect(next).toContain("model: opus");
    const keys = next
      .split("\n")
      .filter((line) => /^[a-zA-Z]+:/.test(line))
      .map((line) => line.split(":")[0]);
    expect(keys).toEqual([
      "name",
      "description",
      "tools",
      "permissionMode",
      "hooks",
      "model",
      "color",
    ]);
    expect(getSubagentBody(next)).toBe(parseSubagentMarkdown(SAMPLE).body);
    expect(next.endsWith("\n---\n\nKeep feedback short.\n")).toBe(true);
  });

  it("returns identical text when nothing changes", () => {
    expect(updateSubagentFrontmatter(SAMPLE, {})).toBe(SAMPLE);
  });

  it("keeps tools as a comma string when it was one", () => {
    const next = updateSubagentFrontmatter(SAMPLE, {
      tools: ["Read", "Bash", "mcp__srv__tool"],
    });
    expect(next).toContain("tools: Read, Bash, mcp__srv__tool\n");
  });

  it("keeps tools as a block list when it was one", () => {
    const next = updateSubagentFrontmatter(
      "---\nname: a\ntools:\n  - Read\n---\nbody",
      { tools: ["Read", "Edit"] },
    );
    expect(next).toBe("---\nname: a\ntools:\n  - Read\n  - Edit\n---\nbody");
  });

  it("keeps a flow list in flow style", () => {
    const next = updateSubagentFrontmatter(
      "---\nname: a\ntools: [Read, Grep]\n---\n",
      { tools: ["Read", "Grep", "Glob"] },
    );
    expect(next).toContain("tools: [Read, Grep, Glob]\n");
  });

  it("converts tools between string and list", () => {
    const toList = updateSubagentFrontmatter(SAMPLE, { toolsFormat: "list" });
    expect(toList).toContain("tools:\n  - Read\n  - Grep\n  - Glob\n");
    expect(parseSubagentMarkdown(toList).toolsFormat).toBe("list");

    const back = updateSubagentFrontmatter(toList, { toolsFormat: "string" });
    expect(back).toContain("tools: Read, Grep, Glob\n");
    expect(parseSubagentMarkdown(back).fields.tools).toEqual(
      parseSubagentMarkdown(toList).fields.tools,
    );
  });

  it("deletes optional keys when they are cleared", () => {
    const next = updateSubagentFrontmatter(SAMPLE, {
      tools: null,
      model: null,
      color: null,
    });
    const parsed = parseSubagentMarkdown(next);
    expect(parsed.fields.tools).toBeUndefined();
    expect(parsed.fields.model).toBeUndefined();
    expect(parsed.fields.color).toBeUndefined();
    expect(next).not.toMatch(/^(tools|model|color):/m);
    expect(next).toContain("permissionMode: plan");
  });

  it("keeps a required key in place when it is emptied", () => {
    const next = updateSubagentFrontmatter(SAMPLE, { name: "" });
    expect(next).toMatch(/\nname: "" # 别改名\n/);
  });

  it("adds frontmatter to content without one", () => {
    const next = updateSubagentFrontmatter("Do the thing.\n", {
      name: "doer",
      description: "Does things",
    });
    expect(next).toBe(
      "---\nname: doer\ndescription: Does things\n---\n\nDo the thing.\n",
    );
  });

  it("keeps CRLF line endings", () => {
    const next = updateSubagentFrontmatter(
      "---\r\nname: win\r\n---\r\n\r\nBody\r\n",
      { model: "haiku" },
    );
    expect(next).toBe(
      "---\r\nname: win\r\nmodel: haiku\r\n---\r\n\r\nBody\r\n",
    );
  });

  it("throws on invalid YAML instead of overwriting it", () => {
    expect(() =>
      updateSubagentFrontmatter("---\nname: [oops\n---\n", { model: "x" }),
    ).toThrow(FrontmatterError);
  });
});

describe("body helpers", () => {
  it("replaces the body without touching frontmatter text", () => {
    const next = replaceSubagentBody(SAMPLE, "New body\n");
    expect(
      next.startsWith(SAMPLE.slice(0, SAMPLE.indexOf("\n---\n") + 5)),
    ).toBe(true);
    expect(getSubagentBody(next)).toBe("New body\n");
  });

  it("is a no-op when the body is unchanged", () => {
    const tight = "---\nname: a\n---\nBody";
    expect(replaceSubagentBody(tight, getSubagentBody(tight))).toBe(tight);
  });

  it("keeps invalid frontmatter text as-is when editing the body", () => {
    const broken = "---\nname: [oops\n---\n\nold";
    expect(replaceSubagentBody(broken, "new")).toBe(
      "---\nname: [oops\n---\n\nnew",
    );
  });
});

describe("buildSubagentMarkdown", () => {
  it("writes keys in the usual order", () => {
    const md = buildSubagentMarkdown(
      {
        name: "tester",
        description: "Runs tests",
        tools: ["Read", "Bash"],
        model: "inherit",
        color: "green",
      },
      "Body\n",
    );
    expect(md).toBe(
      "---\nname: tester\ndescription: Runs tests\ntools: Read, Bash\nmodel: inherit\ncolor: green\n---\n\nBody\n",
    );
  });
});

describe("SUBAGENT_NAME_RE", () => {
  it("accepts Claude Code style names", () => {
    expect(SUBAGENT_NAME_RE.test("code-reviewer")).toBe(true);
    expect(SUBAGENT_NAME_RE.test("a1")).toBe(true);
    expect(SUBAGENT_NAME_RE.test("Code")).toBe(false);
    expect(SUBAGENT_NAME_RE.test("-x")).toBe(false);
    expect(SUBAGENT_NAME_RE.test("a_b")).toBe(false);
    expect(SUBAGENT_NAME_RE.test("a".repeat(65))).toBe(false);
  });
});

// Claude Code `/agents` 生成的写法：description 没加引号，里面带 `Context: ...`，严格 YAML 会报
// "Nested mappings are not allowed in compact mappings"，但 Claude Code 和后端都认
const AGENTS_GENERATED = `---
name: api-designer
description: Use this agent when designing REST endpoints. Examples: <example>Context: The user is adding a new resource. user: "Add an orders endpoint" assistant: "I'll use the api-designer agent"</example>
tools: Read, Grep
model: sonnet
---

You design APIs.
`;

describe("parseSubagentMarkdownLoose", () => {
  it("reads /agents descriptions that strict YAML rejects", () => {
    const strict = safeParseSubagentMarkdown(AGENTS_GENERATED);
    expect(strict.ok).toBe(false);
    if (!strict.ok) expect(strict.code).toBe("yaml");

    const loose = parseSubagentMarkdownLoose(AGENTS_GENERATED);
    expect(loose).not.toBeNull();
    expect(loose?.fields.name).toBe("api-designer");
    expect(loose?.fields.description).toMatch(
      /^Use this agent when designing REST endpoints\. Examples: <example>Context: /,
    );
    expect(loose?.fields.tools).toEqual(["Read", "Grep"]);
    expect(loose?.fields.model).toBe("sonnet");
    expect(loose?.body).toBe("You design APIs.\n");
  });

  it("joins indented continuation lines and strips matching quotes", () => {
    const md =
      "---\nname: 'multi'\ndescription: first line: with colon\n  second line\n---\nBody\n";
    expect(parseSubagentMarkdownLoose(md)?.fields).toEqual({
      name: "multi",
      description: "first line: with colon second line",
    });
  });

  it("returns null when strict parsing works or the frontmatter is unusable", () => {
    expect(parseSubagentMarkdownLoose(SAMPLE)).toBeNull();
    expect(parseSubagentMarkdownLoose("no frontmatter")).toBeNull();
    expect(parseSubagentMarkdownLoose("---\nname: x\n")).toBeNull();
    expect(parseSubagentMarkdownLoose("---\n- a\n- b\n---\n")).toBeNull();
  });

  it("lets the body be replaced without touching the original frontmatter", () => {
    const next = replaceSubagentBody(AGENTS_GENERATED, "New prompt.\n");
    expect(next).toBe(
      AGENTS_GENERATED.replace("You design APIs.\n", "New prompt.\n"),
    );
  });
});
