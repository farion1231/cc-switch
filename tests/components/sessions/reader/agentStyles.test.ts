import { describe, expect, it } from "vitest";
import type { ToolKind } from "@/types";
import {
  AGENT_READER_STYLES,
  getAgentReaderStyle,
  READER_VERB_I18N_PREFIX,
  type ReaderStyleId,
} from "@/components/sessions/reader/agentStyles";
import { SESSION_APP_IDS } from "@/components/sessions/utils";

const STYLE_IDS: ReaderStyleId[] = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "pi",
  "generic",
];

const TOOL_KINDS: ToolKind[] = [
  "shell",
  "read",
  "search",
  "edit",
  "write",
  "web",
  "mcp",
  "agent",
  "ask",
  "todo",
  "other",
];

describe("agentStyles", () => {
  it("六份配置齐全，id 与键一致，字段完整", () => {
    expect(Object.keys(AGENT_READER_STYLES).sort()).toEqual(
      [...STYLE_IDS].sort(),
    );
    STYLE_IDS.forEach((id) => {
      const style = AGENT_READER_STYLES[id];
      expect(style.id).toBe(id);
      expect(style.accentVar).toBe(`--agent-${id}`);
      expect(style.thinkingLabelKey).toBe(
        `sessionManager.reader.thinking.${id}`,
      );
      expect(Object.keys(style.step).sort()).toEqual(
        ["error", "interrupted", "pending", "success"].sort(),
      );
      Object.values(style.step).forEach((glyph) =>
        expect(glyph.length).toBeGreaterThan(0),
      );
      expect(["tree", "box", "none"]).toContain(style.resultFrame);
      expect(["call", "verb", "icon"]).toContain(style.titleFormat);
      expect(["always", "nonzero"]).toContain(style.showExitCode);
      Object.keys(style.verbs).forEach((kind) =>
        expect(TOOL_KINDS).toContain(kind),
      );
      // tree 风格必须有引出符
      if (style.resultFrame === "tree") {
        expect(style.resultConnector).not.toBe("");
      }
    });
  });

  it("关键取值（§6.6 表）", () => {
    const { claude, codex, gemini, opencode, pi, generic } =
      AGENT_READER_STYLES;
    expect(claude).toMatchObject({
      assistantGlyph: "⏺",
      resultConnector: "⎿",
      thinkingGlyph: "✻",
      titleFormat: "call",
      merge: { readSearchRuns: false, mcpSameServerRuns: true },
    });
    expect(codex).toMatchObject({
      userGlyph: "›",
      titleFormat: "verb",
      monoTitles: false,
      merge: { readSearchRuns: true, mcpSameServerRuns: false },
    });
    expect(gemini.step).toEqual({
      success: "✓",
      error: "x",
      pending: "o",
      interrupted: "-",
    });
    expect(gemini.resultFrame).toBe("box");
    expect(opencode).toMatchObject({ titleFormat: "icon", showStepCost: true });
    expect(pi.resultFrame).toBe("none");
    // 只有 OpenCode 显示每步费用（D13）
    STYLE_IDS.filter((id) => id !== "opencode").forEach((id) =>
      expect(AGENT_READER_STYLES[id].showStepCost).toBe(false),
    );
    // generic 的动词全部是 i18n key，覆盖所有工具类别
    expect(Object.keys(generic.verbs).sort()).toEqual([...TOOL_KINDS].sort());
    Object.values(generic.verbs).forEach((verb) =>
      expect(verb.startsWith(READER_VERB_I18N_PREFIX)).toBe(true),
    );
  });

  it("会话来源映射：五家专属，其余与未知来源用 generic", () => {
    SESSION_APP_IDS.forEach((app) => {
      const style = getAgentReaderStyle(app);
      expect(style.id).toBe(
        (STYLE_IDS as string[]).includes(app) ? app : "generic",
      );
    });
    expect(getAgentReaderStyle(undefined).id).toBe("generic");
    expect(getAgentReaderStyle(null).id).toBe("generic");
    expect(getAgentReaderStyle("toString").id).toBe("generic");
  });
});
