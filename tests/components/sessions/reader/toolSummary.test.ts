import { describe, expect, it } from "vitest";
import type { DiffFile, ToolCallBlock } from "@/types";
import {
  AGENT_READER_STYLES,
  getAgentReaderStyle,
  type AgentReaderStyle,
} from "@/components/sessions/reader/agentStyles";
import {
  formatCost,
  formatDuration,
  formatMergedTitle,
  formatStepMeta,
  formatStepTitle,
  formatTokens,
  isFailureStatus,
  isFailureStep,
  mergeSteps,
  shortenPath,
  statusGlyph,
  summarizeThinking,
  summarizeTurn,
  toolStepsOf,
  TOOL_OUTPUT_VERB_KEY,
} from "@/components/sessions/reader/toolSummary";
import {
  buildTurns,
  type MergedStep,
  type TimelineStep,
  type ToolStep,
} from "@/components/sessions/reader/turns";
import { call, fixtures, msg, result, text, thinking } from "./helpers";

const PROJECT = "/Users/yovinchen/Projects/demo-app";

const toolStep = (
  callBlock: ToolCallBlock | undefined,
  extra: Partial<ToolStep> = {},
): ToolStep => ({
  kind: "tool",
  id: `s-${callBlock?.id ?? "orphan"}`,
  messageIndex: 0,
  call: callBlock,
  status: "success",
  ...extra,
});

const titleOf = (
  styleId: string,
  callBlock: ToolCallBlock | undefined,
  t?: (key: string) => string,
) =>
  formatStepTitle(toolStep(callBlock), getAgentReaderStyle(styleId), {
    projectDir: PROJECT,
    t,
  });

/** 每个 fixture 里所有工具步骤的标题（含合并步骤的子步骤） */
const fixtureTitles = (id: keyof typeof fixtures) => {
  const style = getAgentReaderStyle(id);
  return buildTurns(fixtures[id], { style }).flatMap((turn) =>
    toolStepsOf(turn).map(
      (step) => formatStepTitle(step, style, { projectDir: PROJECT }).text,
    ),
  );
};

const diffCall = (files: DiffFile[], kind: "edit" | "write" = "edit") =>
  call("d", kind, {
    rawName: "apply_patch",
    title: `${PROJECT}/README.md`,
    diff: { files, added: 3, removed: 1 },
  });

const file = (op: DiffFile["op"], path = "a.ts"): DiffFile => ({
  path,
  op,
  added: 1,
  removed: 0,
});

describe("状态", () => {
  it("失败类状态与失败步骤", () => {
    expect(
      (["success", "error", "interrupted", "pending", "unknown"] as const).map(
        isFailureStatus,
      ),
    ).toEqual([false, true, true, false, false]);
    const note: TimelineStep = {
      kind: "note",
      id: "n",
      messageIndex: 0,
      text: "x",
    };
    expect(isFailureStep(note)).toBe(false);
    expect(isFailureStep(toolStep(call("a"), { status: "error" }))).toBe(true);
    const merged: MergedStep = {
      kind: "merged",
      id: "m",
      mergeKind: "mcp",
      children: [],
      status: "interrupted",
    };
    expect(isFailureStep(merged)).toBe(true);
  });

  it("statusGlyph：unknown 用成功符号", () => {
    const gemini = AGENT_READER_STYLES.gemini;
    expect(statusGlyph("unknown", gemini)).toBe("✓");
    expect(statusGlyph("pending", gemini)).toBe("o");
  });
});

describe("mergeSteps", () => {
  const read = (id: string, status: ToolStep["status"] = "success") =>
    toolStep(call(id, "read", { title: `${id}.ts` }), { id, status });
  const mcp = (id: string, server?: string) =>
    toolStep(call(id, "mcp", { server, title: `${server}.tool` }), { id });

  it("Codex：连续成功的 read/search 合并为 Explored，至少 2 步", () => {
    const search = toolStep(call("s", "search"), { id: "s" });
    const steps = mergeSteps(
      [read("a"), search, read("b", "error"), read("c"), toolStep(undefined)],
      AGENT_READER_STYLES.codex,
    );
    expect(steps.map((step) => step.kind)).toEqual([
      "merged",
      "tool",
      "tool",
      "tool",
    ]);
    expect(steps[0]).toMatchObject({
      id: "a+merged",
      mergeKind: "explored",
      server: undefined,
      status: "success",
    });
  });

  it("Claude：连续同一 MCP 服务器合并；换服务器、无服务器、思考打断", () => {
    const think: TimelineStep = {
      kind: "thinking",
      id: "t",
      messageIndex: 0,
      block: thinking("x"),
    };
    const steps = mergeSteps(
      [
        mcp("1", "chrome"),
        mcp("2", "chrome"),
        mcp("3", "slack"),
        mcp("4", "slack"),
        think,
        mcp("5", "slack"),
        mcp("6"),
        mcp("7"),
        read("r1"),
        read("r2"),
      ],
      AGENT_READER_STYLES.claude,
    );
    expect(
      steps.map((step) =>
        step.kind === "merged"
          ? `${step.server}×${step.children.length}`
          : step.id,
      ),
    ).toEqual(["chrome×2", "slack×2", "t", "5", "6", "7", "r1", "r2"]);
  });

  it("不合并的风格原样返回", () => {
    const steps = [read("a"), read("b")];
    expect(mergeSteps(steps, AGENT_READER_STYLES.generic)).toEqual(steps);
  });
});

describe("格式化小工具", () => {
  it("formatDuration", () => {
    expect(
      [400, 1500, 9960, 12400, 59600, 134000, 240000, 3780000, 7200000].map(
        formatDuration,
      ),
    ).toEqual([
      "400ms",
      "1.5s",
      "10s",
      "12s",
      "1m",
      "2m14s",
      "4m",
      "1h3m",
      "2h",
    ]);
  });

  it("formatTokens / formatCost", () => {
    expect([980, 1000, 28500, 1_200_000].map(formatTokens)).toEqual([
      "980",
      "1k",
      "28.5k",
      "1.2M",
    ]);
    expect([0.0081, 0.0123, 1.25].map(formatCost)).toEqual([
      "$0.0081",
      "$0.012",
      "$1.25",
    ]);
  });

  it("shortenPath：项目内相对、项目根为 `.`、家目录 `~`", () => {
    expect(shortenPath(`${PROJECT}/src/a.ts`, `${PROJECT}/`)).toBe("src/a.ts");
    expect(shortenPath(`"x" in ${PROJECT}`, PROJECT)).toBe(`"x" in .`);
    expect(shortenPath(`${PROJECT}-old/a.ts`, PROJECT)).toBe(
      "~/Projects/demo-app-old/a.ts",
    );
    expect(shortenPath("/home/me/notes.md")).toBe("~/notes.md");
    expect(shortenPath("relative/path.ts", null)).toBe("relative/path.ts");
  });
});

describe("formatStepTitle · 五家 fixture", () => {
  it("Claude：Name(arg)，Edit 显示为 Update，MCP 为 server.tool(arg)", () => {
    expect(fixtureTitles("claude")).toEqual([
      "Bash(cargo build 2>&1 | tail -50)",
      "Read(Cargo.toml:1-40)",
      "Grep(tokio_util in src)",
      "Update(Cargo.toml)",
      "Update(src/net/client.rs)",
      "Bash(cargo build)",
      "claude-in-chrome.computer(screenshot)",
      "TodoWrite(2 项)",
      "Agent(Review README install steps)",
      "Bash(cargo test --workspace)",
      "AskUserQuestion(CHANGELOG 用哪个版本号？)",
      "WebFetch(https://keepachangelog.com/en/1.1.0/)",
      "Write(CHANGELOG.md)",
    ]);
  });

  it("Codex：Ran / Edited / Called server.tool(args) / Searched", () => {
    expect(fixtureTitles("codex")).toEqual([
      "Read sed -n '1,80p' README.md",
      'Search rg -n "--legacy" docs',
      "Edited README.md",
      "Called cua_repl.js(await page.goto('http://localhost:4173/docs/install'); await screenshot();)",
      "Searched demo-app cargo install doctor command",
      "Ran pnpm build",
      "Updated plan 3 项",
      "Spawned agent Check docs for dead links",
      "Asked 失效的外链直接删除还是换成归档地址？",
    ]);
  });

  it("Gemini：Shell / ReadFile / GoogleSearch", () => {
    expect(fixtureTitles("gemini")).toEqual([
      "ReadFile src/main.ts",
      "Shell npm run build",
      "Edit src/config.ts",
      "GoogleSearch Node 22 top-level await ESM pitfalls",
      "Todos 2 项",
      "Shell npm run build",
    ]);
  });

  it("OpenCode：符号前缀", () => {
    expect(fixtureTitles("opencode")).toEqual([
      "$ bunx biome --version",
      "→ Read package.json",
      '✱ Grep "lint" in .',
      "← Edit package.json",
      "│ Task Find conflicting lint configs",
      "% WebFetch https://biomejs.dev/recipes/continuous-integration/",
      "⚙ Todos 2 项",
      "→ Asked 要删除 .eslintrc 吗？",
      "⚙ invalid",
    ]);
  });

  it("Pi：小写动词，read 带行范围", () => {
    expect(fixtureTitles("pi")).toEqual([
      "read src/components/LoginButton.tsx:10-40",
      "bash pnpm test -- LoginButton",
      "edit src/components/LoginButton.tsx",
      "read docs/screens/login-dark.png",
      "bash pnpm test -- LoginButton",
      "grep text-gray-800 in src",
    ]);
  });

  it("generic：通用动词走 t()，没传 t 时返回 key；无配对输出用通用动词", () => {
    const t = (key: string) => `T(${key.split(".").pop()})`;
    expect(
      titleOf("hermes", call("a", "shell", { title: "df -h" }), t).text,
    ).toBe("T(run) df -h");
    expect(titleOf("generic", call("a", "shell", { title: "df" })).text).toBe(
      "sessionManager.reader.verb.run df",
    );
    expect(titleOf("generic", undefined, t)).toEqual({
      icon: "",
      verb: "T(output)",
      target: "",
      text: "T(output)",
    });
    expect(titleOf("generic", undefined).verb).toBe(TOOL_OUTPUT_VERB_KEY);
  });
});

describe("formatStepTitle · 分支", () => {
  it("call 格式：MCP 没有参数时只显示 server.tool；对象等于名字时省略", () => {
    expect(
      titleOf(
        "claude",
        call("m", "mcp", { title: "slack.post", server: "slack" }),
      ),
    ).toMatchObject({ verb: "slack.post", target: "", text: "slack.post" });
    expect(
      titleOf(
        "claude",
        call("x", "other", { rawName: "Skill", title: "Skill" }),
      ).text,
    ).toBe("Skill");
  });

  it("verb 格式：MCP 没有参数给空括号；没配动词的类别用原名；detail 保留", () => {
    expect(
      titleOf("codex", call("m", "mcp", { title: "clock.sleep" })).text,
    ).toBe("Called clock.sleep()");
    const other = titleOf(
      "codex",
      call("o", "other", { rawName: "sleep", title: "5s", detail: "ms" }),
    );
    expect(
      titleOf("codex", call("o", "other", { rawName: "sleep", title: "sleep" }))
        .text,
    ).toBe("sleep");
    expect(other).toMatchObject({
      verb: "sleep",
      text: "sleep 5s",
      detail: "ms",
    });
  });

  it("Codex 文件改动：Added / Deleted / 多文件 N files / 混合为 Edited / 无 diff", () => {
    expect(titleOf("codex", diffCall([file("add")], "write")).text).toBe(
      "Added README.md",
    );
    expect(titleOf("codex", diffCall([file("delete")])).text).toBe(
      "Deleted README.md",
    );
    expect(
      titleOf("codex", diffCall([file("update"), file("add", "b.ts")])).text,
    ).toBe("Edited 2 files");
    expect(titleOf("codex", diffCall([file("rename")])).text).toBe(
      "Edited README.md",
    );
    expect(
      titleOf("codex", call("w", "write", { title: `${PROJECT}/x.md` })).text,
    ).toBe("Added x.md");
  });

  it("OpenCode：websearch 用 ◈，glob 用 Glob，MCP 用 ⚙ + 原名", () => {
    expect(
      titleOf(
        "opencode",
        call("w", "web", { rawName: "websearch", title: "q" }),
      ).text,
    ).toBe("◈ WebSearch q");
    expect(
      titleOf(
        "opencode",
        call("g", "search", { rawName: "glob", title: "*.ts" }),
      ).text,
    ).toBe("✱ Glob *.ts");
    expect(
      titleOf(
        "opencode",
        call("m", "mcp", { rawName: "github_search", title: "github.search" }),
      ).text,
    ).toBe("⚙ github_search github.search");
  });
});

describe("formatMergedTitle", () => {
  it("Explored 统计不同文件与搜索次数；MCP 为 Called X N times", () => {
    const explored: MergedStep = {
      kind: "merged",
      id: "m",
      mergeKind: "explored",
      status: "success",
      children: [
        toolStep(call("a", "read", { title: "a.ts" })),
        toolStep(call("b", "read", { title: "a.ts" })),
        toolStep(call("c", "search")),
      ],
    };
    expect(formatMergedTitle(explored)).toMatchObject({
      text: "Explored",
      count: 3,
      reads: 1,
      searches: 1,
    });
    expect(
      formatMergedTitle({ ...explored, mergeKind: "mcp", server: "slack" })
        .text,
    ).toBe("Called slack 3 times");
  });
});

describe("formatStepMeta", () => {
  const always: AgentReaderStyle = {
    ...AGENT_READER_STYLES.claude,
    showExitCode: "always",
    showDiffCounts: false,
  };

  it("exit：nonzero 只在非 0 时给，always 总给；行数；耗时", () => {
    const ok = toolStep(call("a"), {
      result: result("a", "success", {
        exitCode: 0,
        durationMs: 300,
        lineCount: 4,
      }),
    });
    expect(formatStepMeta(ok, AGENT_READER_STYLES.claude)).toEqual({
      status: "success",
      glyph: "⏺",
      tone: "success",
      durationMs: 300,
      lineCount: 4,
    });
    expect(formatStepMeta(ok, always).exitCode).toBe(0);
    const failed = toolStep(call("b"), {
      status: "error",
      result: result("b", "error", { exitCode: 101, lineCount: 0 }),
    });
    expect(formatStepMeta(failed, AGENT_READER_STYLES.codex)).toMatchObject({
      tone: "danger",
      exitCode: 101,
    });
    expect(formatStepMeta(failed, AGENT_READER_STYLES.codex).lineCount).toBe(
      undefined,
    );
  });

  it("diff 计数优先于行数；关掉或计数为 0 时退回行数；无结果为 pending", () => {
    const edit = toolStep(
      call("e", "edit", {
        diff: { files: [file("update")], added: 3, removed: 1 },
      }),
      { result: result("e", "success", { lineCount: 2 }) },
    );
    expect(formatStepMeta(edit, AGENT_READER_STYLES.codex).diff).toEqual({
      added: 3,
      removed: 1,
      files: 1,
    });
    expect(formatStepMeta(edit, always)).toMatchObject({ lineCount: 2 });
    const empty = toolStep(
      call("z", "edit", { diff: { files: [], added: 0, removed: 0 } }),
      { status: "pending" },
    );
    expect(formatStepMeta(empty, AGENT_READER_STYLES.codex)).toEqual({
      status: "pending",
      glyph: "•",
      tone: "muted",
      durationMs: undefined,
    });
  });

  it("OpenCode 显示每步 tokens / 费用，其它风格不显示", () => {
    const step = toolStep(call("a"), {
      cost: { tokens: 28500, costUsd: 0.012 },
    });
    expect(formatStepMeta(step, AGENT_READER_STYLES.opencode)).toMatchObject({
      tokens: 28500,
      costUsd: 0.012,
    });
    expect(formatStepMeta(step, AGENT_READER_STYLES.claude).tokens).toBe(
      undefined,
    );
    expect(
      formatStepMeta(toolStep(call("b")), AGENT_READER_STYLES.opencode).tokens,
    ).toBe(undefined);
  });

  it("合并步骤：子步骤耗时之和；都没有耗时则不给", () => {
    const merged: MergedStep = {
      kind: "merged",
      id: "m",
      mergeKind: "explored",
      status: "success",
      children: [
        toolStep(call("a", "read"), {
          result: result("a", "success", { durationMs: 100 }),
        }),
        toolStep(call("b", "read")),
      ],
    };
    expect(formatStepMeta(merged, AGENT_READER_STYLES.codex)).toEqual({
      status: "success",
      glyph: "•",
      tone: "success",
      durationMs: 100,
    });
    expect(
      formatStepMeta(
        { ...merged, children: [toolStep(call("c", "read"))] },
        AGENT_READER_STYLES.codex,
      ).durationMs,
    ).toBeUndefined();
  });
});

describe("summarizeThinking", () => {
  it("字数、是否还有全文、摘要、加密", () => {
    expect(
      summarizeThinking(
        thinking("abc", {
          summary: "plan",
          durationMs: 800,
          full: { kind: "sidecar", relPath: "x" },
        }),
      ),
    ).toEqual({
      chars: 3,
      partial: true,
      durationMs: 800,
      summary: "plan",
      redacted: false,
    });
    expect(summarizeThinking(thinking("", { redacted: true }))).toMatchObject({
      chars: 0,
      partial: false,
      redacted: true,
    });
  });
});

describe("summarizeTurn", () => {
  it("fixture：Claude 第一轮与 OpenCode 费用合计", () => {
    const claude = buildTurns(fixtures.claude, {
      style: AGENT_READER_STYLES.claude,
    });
    expect(summarizeTurn(claude[0])).toEqual({
      stepCount: 7,
      commandCount: 2,
      filesChanged: 2,
      errorCount: 1,
      thinkingCount: 2,
      durationMs: 100000,
      kindCounts: { shell: 2, read: 1, search: 1, edit: 2, mcp: 1 },
      tokens: undefined,
      costUsd: undefined,
    });
    const opencode = buildTurns(fixtures.opencode, {
      style: AGENT_READER_STYLES.opencode,
    });
    const summary = summarizeTurn(opencode[0]);
    expect(summary.tokens).toBe(63104);
    expect(summary.costUsd).toBeCloseTo(0.0246);
  });

  it("没有时间戳用各步耗时之和；都没有则不给；失败改动不计文件；无 diff 的改动按标题计；合并步骤展开计数", () => {
    const [turn] = buildTurns(
      [
        msg("user", [text("q")]),
        msg("assistant", [
          call("e1", "edit", { title: "a.ts" }),
          call("e2", "write", { title: "b.ts" }),
          call("r1", "read"),
          call("r2", "read"),
        ]),
        msg("tool", [
          result("e1", "success", { durationMs: 50 }),
          result("e2", "error"),
          result("r1"),
          result("r2"),
          result("orphan"),
        ]),
        msg("assistant", [
          { type: "step", phase: "start" },
          text("done"),
          { type: "step", phase: "finish", costUsd: 0.5 },
        ]),
      ],
      { style: AGENT_READER_STYLES.codex },
    );
    expect(turn.steps.some((step) => step.kind === "merged")).toBe(true);
    expect(summarizeTurn(turn)).toEqual({
      stepCount: 5,
      commandCount: 0,
      filesChanged: 1,
      errorCount: 1,
      thinkingCount: 0,
      durationMs: 50,
      kindCounts: { edit: 1, write: 1, read: 2, other: 1 },
      tokens: undefined,
      costUsd: 0.5,
    });

    const [bare] = buildTurns([
      msg("user", [text("q")], { ts: 5 }),
      msg("assistant", [text("a")], { ts: 5 }),
    ]);
    expect(summarizeTurn(bare).durationMs).toBeUndefined();
  });
});
