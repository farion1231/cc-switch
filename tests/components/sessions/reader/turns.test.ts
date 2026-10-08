import { describe, expect, it } from "vitest";
import type { SessionMessage } from "@/types";
import {
  AGENT_READER_STYLES,
  getAgentReaderStyle,
} from "@/components/sessions/reader/agentStyles";
import {
  buildTurnIndex,
  buildTurns,
  effectiveBlocks,
  estimateRowHeight,
  FAILURE_PREVIEW_LINES,
  findMatchRowIndex,
  findSearchHits,
  findTurnRowIndex,
  flattenRows,
  isChangeStep,
  isTimelineExpanded,
  LEGACY_CALL_PREFIX,
  MAX_PINNED_FAILURES,
  STEP_PREVIEW_LINES,
  stepSearchText,
  type ReaderRow,
  type SessionTurn,
  type TimelineStep,
} from "@/components/sessions/reader/turns";
import { call, event, fixtures, msg, result, text, thinking } from "./helpers";

const turnsOf = (id: keyof typeof fixtures) =>
  buildTurns(fixtures[id], { style: getAgentReaderStyle(id) });

const rowKinds = (rows: ReaderRow[]) => rows.map((row) => row.kind);

const stepRow = (rows: ReaderRow[], index: number) => {
  const row = rows[index];
  if (row.kind !== "step") throw new Error(`row ${index} is ${row.kind}`);
  return row;
};

const toolTitles = (steps: TimelineStep[]) =>
  steps.map((step) =>
    step.kind === "tool" ? (step.call?.title ?? "<output>") : step.kind,
  );

describe("buildTurns · 6 份契约 fixture", () => {
  it("Claude：提问/过程/最终回复、注入、斜杠命令、中断与 Hook", () => {
    const turns = turnsOf("claude");
    expect(turns.map((turn) => turn.key)).toEqual(["t1", "t2", "t3", "t4"]);

    const [t1, t2, t3, t4] = turns;
    expect(t1.question).toMatchObject({ messageIndex: 0, ts: 1791014400000 });
    expect(t1.question?.text).toContain("tokio_util");
    expect(t1.question?.images).toHaveLength(1);
    expect(t1.injected).toEqual([1]);
    expect(t1.final?.messageIndex).toBe(14);
    expect(t1.final?.model).toBe("claude-opus-5-5");
    expect(t1.final?.text).toContain("构建已经通过");
    expect(t1.trailingEvents.map((e) => e.block.kind)).toEqual(["pr_link"]);
    // redacted thinking 保留、普通 thinking 保留；过程说明成 note
    expect(t1.steps.map((step) => step.kind)).toEqual([
      "thinking",
      "note",
      "tool",
      "thinking",
      "tool",
      "tool",
      "note",
      "tool",
      "tool",
      "tool",
      "tool",
    ]);
    const bash = t1.steps[2];
    expect(bash).toMatchObject({
      kind: "tool",
      status: "error",
      resultMessageIndex: 3,
    });

    expect(t2.question).toBeUndefined();
    expect(t2.leadingEvents.map((e) => e.block.kind)).toEqual([
      "slash_command",
      "compaction",
    ]);
    expect(t2.steps).toEqual([]);

    expect(t3.aborted).toBe(true);
    expect(t3.final).toBeUndefined();
    // 事件按出现顺序：先中断，再 Hook 错误
    expect(t3.trailingEvents.map((e) => e.block.kind)).toEqual([
      "aborted",
      "hook",
    ]);
    expect(t3.steps.map((step) => (step as { status: string }).status)).toEqual(
      ["success", "pending", "interrupted"],
    );

    expect(t4.final?.messageIndex).toBe(30);
    expect(toolTitles(t4.steps)).toHaveLength(3);
  });

  it("Codex：注入的 developer/AGENTS.md、事件分前后、子代理正文并入事件、过程图片", () => {
    const [t1, t2, t3] = turnsOf("codex");
    expect(t1.injected).toEqual([0, 1, 2]);
    expect(t1.question?.messageIndex).toBe(3);
    expect(t1.question?.images).toHaveLength(1);
    expect(t1.final?.messageIndex).toBe(14);
    // read 成功 + search 失败：失败不参与 Explored 合并
    expect(t1.steps.some((step) => step.kind === "merged")).toBe(false);

    expect(t2.leadingEvents.map((e) => e.block.kind)).toEqual(["model_change"]);
    expect(t2.aborted).toBe(true);
    expect(t2.final).toBeUndefined();
    expect(t2.trailingEvents.map((e) => e.block.kind)).toEqual(["aborted"]);

    expect(t3.leadingEvents.map((e) => e.block.kind)).toEqual(["compaction"]);
    const subAgent = t3.steps.find((step) => step.kind === "event");
    expect(subAgent).toMatchObject({ block: { kind: "sub_agent" } });
    expect(subAgent && "body" in subAgent && subAgent.body).toContain(
      "docs/guide/sync.md",
    );
    expect(t3.steps.some((step) => step.kind === "image")).toBe(true);
    expect(t3.final?.messageIndex).toBe(30);
  });

  it("Gemini：纯注入轮不出行；同一条消息里调用、结果与最终回复", () => {
    const turns = turnsOf("gemini");
    expect(turns[0]).toMatchObject({ key: "t0", injected: [0], steps: [] });
    expect(turns[0].question).toBeUndefined();
    expect(turns[1].final?.messageIndex).toBe(2);
    expect(turns[1].trailingEvents.map((e) => e.block.kind)).toEqual(["error"]);
    expect(turns[2].final).toBeUndefined();
    const rows = flattenRows(turns);
    expect(rows[0].kind).toBe("question");
    expect(rows.every((row) => row.turn !== 0)).toBe(true);
  });

  it("OpenCode：step-finish 费用落到该段最后一步，纯文本段落到最终回复", () => {
    const [t1, t2] = turnsOf("opencode");
    expect(t1.stepCosts.map((cost) => cost.tokens)).toEqual([
      18734, 21960, 22410,
    ]);
    const tools = t1.steps.filter((step) => step.kind === "tool");
    expect(tools[0].cost).toMatchObject({ tokens: 18734, costUsd: 0.0123 });
    expect(tools[1].cost).toBeUndefined();
    expect(tools[3].cost).toMatchObject({ tokens: 21960 });
    expect(t1.final?.cost).toMatchObject({ tokens: 22410, reason: "stop" });

    // 提问消息里的 @agent 事件算提问前的事件
    expect(t2.leadingEvents.map((e) => e.block.kind)).toEqual(["other"]);
    expect(t2.final).toBeUndefined();
    expect(t2.steps.at(-1)?.cost).toMatchObject({ tokens: 30112 });
  });

  it("Pi：用户自己跑的命令单步成功直接显示；中断轮也有最终回复", () => {
    const turns = turnsOf("pi");
    expect(turns[0].leadingEvents.map((e) => e.block.kind)).toEqual([
      "model_change",
      "thinking_level",
    ]);
    const userBash = turns[2];
    expect(userBash.question).toBeUndefined();
    expect(userBash.steps).toHaveLength(1);
    expect(userBash.steps[0]).toMatchObject({
      kind: "tool",
      status: "success",
      call: { byUser: true },
    });
    const aborted = turns[3];
    expect(aborted.aborted).toBe(true);
    expect(aborted.final?.text).toContain("text-gray-800");
    expect(aborted.trailingEvents.map((e) => e.block.kind)).toEqual([
      "aborted",
      "compaction",
    ]);
    expect(turns[4].trailingEvents.map((e) => e.block.text)).toEqual([
      "truncated",
    ]);
  });

  it("generic：无配对的结果成通用「工具输出」步骤；纯对话轮没有过程", () => {
    const [t1, t2, t3] = turnsOf("generic");
    expect(t1.steps.map((step) => step.kind)).toEqual(["note", "tool", "tool"]);
    expect(toolTitles(t2.steps)).toEqual([
      "sudo journalctl --vacuum-time=3d",
      "<output>",
    ]);
    expect(t2.steps[1]).toMatchObject({ status: "unknown", messageIndex: 7 });
    expect(t3.steps).toEqual([]);
    expect(t3.final?.messageIndex).toBe(11);
  });

  it("缺省风格为 generic：不合并", () => {
    const messages = [
      msg("user", [text("q")]),
      msg("assistant", [call("a", "read"), call("b", "read")]),
      msg("tool", [result("a"), result("b")]),
      msg("assistant", [text("done")]),
    ];
    expect(buildTurns(messages)[0].steps.map((s) => s.kind)).toEqual([
      "tool",
      "tool",
    ]);
    expect(
      buildTurns(messages, { style: AGENT_READER_STYLES.codex })[0].steps.map(
        (s) => s.kind,
      ),
    ).toEqual(["merged"]);
  });
});

describe("effectiveBlocks · 旧后端兜底", () => {
  it("有 blocks 原样返回", () => {
    const blocks = [text("hi")];
    expect(effectiveBlocks(msg("user", blocks), 0)).toBe(blocks);
  });

  it("空 content 无块；tool 角色整段当无配对输出；user 当文本", () => {
    expect(effectiveBlocks({ role: "assistant", content: "  " }, 0)).toEqual(
      [],
    );
    expect(
      effectiveBlocks({ role: "tool", content: "line1\nline2" }, 0),
    ).toEqual([
      {
        type: "tool_result",
        callId: "",
        status: "unknown",
        preview: "line1\nline2",
        totalLen: 11,
        lineCount: 2,
        truncated: false,
      },
    ]);
    expect(effectiveBlocks({ role: "user", content: "hello" }, 0)).toEqual([
      text("hello"),
    ]);
  });

  it("assistant 的 `[Tool: X] title` 行拆成调用，其余行成文本", () => {
    const blocks = effectiveBlocks(
      {
        role: "assistant",
        content: "先看看\n[Tool: Bash] ls -la\n[Tool]\n[Tool: Read]\n收尾",
        blocks: [],
      },
      7,
    );
    expect(blocks.map((block) => block.type)).toEqual([
      "text",
      "tool_call",
      "tool_call",
      "tool_call",
      "text",
    ]);
    expect(blocks[1]).toMatchObject({
      id: `${LEGACY_CALL_PREFIX}7:1`,
      rawName: "Bash",
      title: "ls -la",
      kind: "other",
    });
    expect(blocks[2]).toMatchObject({ rawName: "Tool", title: "Tool" });
    expect(blocks[3]).toMatchObject({ rawName: "Read", title: "Read" });
  });

  it("旧数据没有 turnId：按提问分轮，旧调用状态为 unknown", () => {
    const messages: SessionMessage[] = [
      { role: "system", content: "boot" },
      { role: "user", content: "first" },
      { role: "assistant", content: "[Tool: Bash] ls" },
      { role: "tool", content: "a.txt" },
      { role: "assistant", content: "done" },
      { role: "user", content: "second" },
    ];
    const turns = buildTurns(messages);
    expect(turns.map((turn) => turn.key)).toEqual([
      "auto-0",
      "auto-1",
      "auto-2",
    ]);
    expect(turns[0].steps).toMatchObject([{ kind: "note", text: "boot" }]);
    expect(
      turns[1].steps.map((step) => (step as { status: string }).status),
    ).toEqual(["unknown", "unknown"]);
    expect(turns[1].final?.text).toBe("done");
  });
});

describe("buildTurns · 分组、配对与最终回复判定", () => {
  it("同一 turnId 不连续出现时加后缀保证 key 唯一", () => {
    const turns = buildTurns([
      msg("user", [text("a")], { turnId: "x" }),
      msg("user", [text("b")], { turnId: "y" }),
      msg("user", [text("c")], { turnId: "x" }),
      msg("user", [text("d")], { turnId: "x" }),
    ]);
    expect(turns.map((turn) => turn.key)).toEqual(["x", "y", "x#1"]);
    expect(turns[2]).toMatchObject({
      firstMessageIndex: 2,
      lastMessageIndex: 3,
    });
    // 同一轮里第二条提问当过程说明
    expect(turns[2].steps).toMatchObject([{ kind: "note", text: "d" }]);
  });

  it("配对：无结果 pending；重复结果与未知 callId 成孤儿步骤；空 callId 的调用不登记", () => {
    const [turn] = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [call("a"), call("b"), call(""), call("a")]),
      msg("tool", [result("a"), result("a", "error"), result("zzz")]),
    ]);
    const tools = turn.steps.filter((step) => step.kind === "tool");
    expect(tools.map((step) => [step.call?.id, step.status])).toEqual([
      ["a", "success"],
      ["b", "pending"],
      ["", "pending"],
      ["a", "pending"],
      [undefined, "error"],
      [undefined, "success"],
    ]);
  });

  it("配对：结果先于调用写入（Claude 异步子代理）时等调用出现再配上", () => {
    const [turn] = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [call("a")]),
      msg("tool", [result("b", "error")]),
      msg("tool", [result("a")]),
      msg("assistant", [call("b")]),
      msg("assistant", [text("done")]),
    ]);
    const tools = turn.steps.filter((step) => step.kind === "tool");
    expect(
      tools.map((step) => [
        step.call?.id,
        step.status,
        step.resultMessageIndex,
      ]),
    ).toEqual([
      ["a", "success", 3],
      ["b", "error", 2],
    ]);
  });

  it("Codex：子调用 call_id#N、同一消息内调用 + 结果、思考 + 调用", () => {
    const [turn] = buildTurns(
      [
        msg("user", [text("q")]),
        msg("assistant", [
          thinking("plan"),
          call("c1#1"),
          call("c1#2"),
          result("c1#1"),
          result("c1#2", "error"),
        ]),
        msg("assistant", [call("ws"), result("ws")]),
        msg("assistant", [text("done")]),
      ],
      { style: AGENT_READER_STYLES.codex },
    );
    const flat = turn.steps.flatMap((step) =>
      step.kind === "merged" ? step.children : [step],
    );
    expect(
      flat.map((step) =>
        step.kind === "tool" ? [step.call?.id, step.status] : [step.kind],
      ),
    ).toEqual([
      ["thinking"],
      ["c1#1", "success"],
      ["c1#2", "error"],
      ["ws", "success"],
    ]);
    expect(turn.final?.text).toBe("done");
  });

  it("思考：空且未加密、无摘要的块跳过", () => {
    const [turn] = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [
        thinking(""),
        thinking("", { summary: "plan" }),
        thinking("", { redacted: true }),
        text("   "),
        text("ok"),
      ]),
    ]);
    expect(turn.steps.map((step) => step.kind)).toEqual([
      "thinking",
      "thinking",
    ]);
    expect(turn.final?.text).toBe("ok");
  });

  it("最终回复：同一条消息里调用之后的文本与图片；调用之前的文本是说明", () => {
    const image = {
      source: { kind: "local_file" as const, path: "/tmp/a.png" },
      mediaType: "image/png",
      size: 1,
    };
    const [turn] = buildTurns([
      msg("user", [text("q")], { ts: 10 }),
      msg(
        "assistant",
        [
          text("before"),
          call("a"),
          result("a"),
          text("answer"),
          { type: "image", image },
        ],
        { ts: 20, meta: { model: "m1" } },
      ),
    ]);
    expect(turn.steps.map((step) => step.kind)).toEqual(["note", "tool"]);
    expect(turn.final).toEqual({
      messageIndex: 1,
      text: "answer",
      images: [image],
      ts: 20,
      model: "m1",
      cost: undefined,
    });
    expect(turn).toMatchObject({ ts: 10, endTs: 20 });
  });

  it("最后一个调用在文本之后，或在更晚的消息里 → 没有最终回复", () => {
    const sameMessage = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [text("let me check"), call("a")]),
    ])[0];
    expect(sameMessage.final).toBeUndefined();

    const laterMessage = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [text("answer")]),
      msg("user", [call("u", "shell", { byUser: true }), result("u")]),
    ])[0];
    expect(laterMessage.final).toBeUndefined();

    const injectedIgnored = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [text("answer")]),
      msg("assistant", [call("x")], { injected: true }),
      msg("assistant", [thinking("trailing")]),
    ])[0];
    expect(injectedIgnored.final?.text).toBe("answer");
    expect(injectedIgnored.injected).toEqual([2]);
  });

  it("事件：没有提问时、步骤之前的算提问前；过程中的留在时间线；system 正文无事件时成说明", () => {
    const [turn] = buildTurns([
      msg("system", [event("model_change", "m2")]),
      msg("assistant", [call("a")]),
      msg("tool", [result("a")]),
      msg("system", [event("info", "note"), text("detail"), text("more")]),
      msg("system", [text("plain system text")]),
      msg("assistant", [call("b")]),
      msg("tool", [result("b")]),
      msg("system", [event("error", "boom")]),
    ]);
    expect(turn.leadingEvents.map((e) => e.block.kind)).toEqual([
      "model_change",
    ]);
    expect(turn.steps.map((step) => step.kind)).toEqual([
      "tool",
      "event",
      "note",
      "tool",
    ]);
    expect(turn.steps[1]).toMatchObject({ body: "more" });
    expect(turn.trailingEvents.map((e) => e.block.kind)).toEqual(["error"]);
  });

  it("step-finish 段内没有步骤也没有最终回复：只计入轮费用", () => {
    const [turn] = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [
        { type: "step", phase: "start" },
        { type: "step", phase: "finish", tokens: 5, costUsd: 0.5 },
      ]),
    ]);
    expect(turn.final).toBeUndefined();
    expect(turn.stepCosts).toEqual([
      { tokens: 5, costUsd: 0.5, reason: undefined },
    ]);
  });
});

describe("buildTurnIndex", () => {
  it("收有提问或斜杠命令的轮，排除纯注入 / 纯事件轮", () => {
    const index = buildTurnIndex(turnsOf("claude"));
    expect(index.map((item) => item.turnId)).toEqual(["t1", "t2", "t3", "t4"]);
    expect(index[0]).toMatchObject({
      firstMessageIndex: 0,
      lastMessageIndex: 15,
      stepCount: 7,
      errorCount: 1,
      hasFinalReply: true,
      aborted: false,
      ts: 1791014400000,
    });
    expect(index[1]).toMatchObject({
      questionPreview: "/compact",
      ts: 1791014700000,
    });
    expect(index[2]).toMatchObject({ aborted: true, hasFinalReply: false });
    expect(buildTurnIndex(turnsOf("gemini")).map((t) => t.turnId)).toEqual([
      "t1",
      "t2",
    ]);
  });

  it("提问预览压空白、超过 80 字截断；斜杠命令没有文本时为空", () => {
    const long = `${"a ".repeat(30)}\n\n${"b".repeat(60)}`;
    const [first, second] = buildTurnIndex(
      buildTurns([
        msg("user", [text(long)], { turnId: "1" }),
        msg("user", [event("slash_command")], { turnId: "2" }),
      ]),
    );
    expect(first.questionPreview).toHaveLength(81);
    expect(first.questionPreview.endsWith("…")).toBe(true);
    expect(first.questionPreview).not.toContain("\n");
    expect(second.questionPreview).toBe("");
    expect(second.ts).toBeUndefined();
  });
});

describe("搜索", () => {
  it("空查询返回 null", () => {
    expect(findSearchHits(turnsOf("claude"), "  ")).toBeNull();
  });

  it("按文档顺序记录提问、步骤、最终回复、前后事件的命中", () => {
    const turns = buildTurns(
      [
        msg("system", [event("compaction", "needle before")]),
        msg("user", [text("needle question needle")]),
        msg("assistant", [
          thinking("needle thought"),
          call("r1", "read", { title: "needle.ts" }),
          call("r2", "read", { title: "other.ts" }),
        ]),
        msg("tool", [result("r1"), result("r2", "success", { preview: "x" })]),
        msg("system", [event("sub_agent", "agent"), text("needle body")]),
        msg("assistant", [
          {
            type: "image",
            image: {
              source: { kind: "local_file", path: "needle" },
              mediaType: "image/png",
              size: 1,
            },
          },
          text("progress"),
          call("s", "shell", { detail: "needle detail" }),
        ]),
        msg("tool", [result("s")]),
        msg("assistant", [text("final needle")]),
        msg("system", [event("hook", "needle hook")]),
      ].map((message) => ({ ...message, turnId: "s" })),
      { style: AGENT_READER_STYLES.codex },
    );
    const hits = findSearchHits(turns, "NEEDLE");
    expect(hits).not.toBeNull();
    const [turn] = turns;
    expect(hits!.matches.map((m) => [m.target, m.count])).toEqual([
      ["event", 1],
      ["question", 2],
      ["step", 1],
      ["step", 1],
      ["step", 1],
      ["step", 1],
      ["final", 1],
      ["event", 1],
    ]);
    expect(hits!.total).toBe(9);
    expect(hits!.turnKeys.has(turn.key)).toBe(true);
    expect(hits!.questionKeys.has(turn.key)).toBe(true);
    expect(hits!.finalKeys.has(turn.key)).toBe(true);
    const merged = turn.steps.find((step) => step.kind === "merged");
    expect(merged).toBeDefined();
    // 合并步骤的子步骤命中 → 父步骤也要展开
    expect(hits!.stepIds.has(merged!.id)).toBe(true);
  });

  it("stepSearchText 覆盖每种步骤", () => {
    const [turn] = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [
        thinking("body", { summary: "sum" }),
        thinking("only"),
        call("c", "shell", { detail: "d" }),
      ]),
      msg("tool", [result("zzz", "success", { preview: "orphan" })]),
      msg("system", [event("sub_agent")]),
      msg("assistant", [call("c2")]),
    ]);
    expect(turn.steps.map(stepSearchText)).toEqual([
      "sum\nbody",
      "only",
      "cmd c\nd\n{}",
      "orphan",
      "",
      "cmd c2\n{}",
    ]);
  });
});

describe("flattenRows · 折叠规则 §6.5", () => {
  const failing = (count: number) => {
    const calls = Array.from({ length: count }, (_, i) => call(`f${i}`));
    return [
      msg("user", [text("q")], { turnId: "t" }),
      msg("assistant", calls, { turnId: "t" }),
      msg(
        "tool",
        calls.map((c) => result(c.id, "error")),
        { turnId: "t" },
      ),
      msg("assistant", [text("answer")], { turnId: "t" }),
    ];
  };

  it("规则 1/2：有最终回复的多步轮折叠成摘要行，失败步骤常显并预展开 6 行", () => {
    const rows = flattenRows(turnsOf("claude"));
    expect(rowKinds(rows).slice(0, 5)).toEqual([
      "question",
      "timeline",
      "step",
      "final",
      "event",
    ]);
    expect(rows[1]).toMatchObject({ expanded: false, hiddenFailures: 0 });
    expect(stepRow(rows, 2)).toMatchObject({
      pinned: true,
      expanded: true,
      previewLines: FAILURE_PREVIEW_LINES,
    });
  });

  it("规则 2：超过 5 个失败只列前 5 个", () => {
    const rows = flattenRows(buildTurns(failing(7)));
    expect(rows[1]).toMatchObject({ kind: "timeline", hiddenFailures: 2 });
    expect(rows.filter((row) => row.kind === "step")).toHaveLength(
      MAX_PINNED_FAILURES,
    );
  });

  it("规则 3：中断 / 无最终回复自动展开；展开全部；查找命中；手动收起优先", () => {
    const turns = turnsOf("claude");
    const aborted = turns[2];
    expect(isTimelineExpanded(aborted)).toBe(true);
    expect(isTimelineExpanded(turns[0])).toBe(false);
    expect(isTimelineExpanded(turns[0], { expandAll: true })).toBe(true);
    expect(
      isTimelineExpanded(turns[0], {
        turnOverrides: new Map([["t3", false]]),
        search: findSearchHits(turns, "Cargo.toml"),
      }),
    ).toBe(true);
    expect(
      isTimelineExpanded(aborted, { turnOverrides: new Map([["t3", false]]) }),
    ).toBe(false);

    const rows = flattenRows(turns, { expandAll: true });
    const t1Steps = rows.filter((row) => row.kind === "step" && row.turn === 0);
    expect(t1Steps).toHaveLength(turns[0].steps.length);
    expect(t1Steps.every((row) => row.kind === "step" && !row.pinned)).toBe(
      true,
    );
  });

  it("规则 4：最后一轮有回复时同样默认折叠（D5）", () => {
    const turns = turnsOf("claude");
    const rows = flattenRows(turns);
    const last = rows.filter((row) => row.turn === 3);
    expect(rowKinds(last)).toEqual([
      "turn_divider",
      "question",
      "timeline",
      "final",
    ]);
  });

  it("规则 5：只有 1 步且成功不出摘要行；1 步失败仍出摘要行", () => {
    const one = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [call("a")]),
      msg("tool", [result("a")]),
      msg("assistant", [text("done")]),
    ]);
    expect(rowKinds(flattenRows(one))).toEqual(["question", "step", "final"]);
    expect(rowKinds(flattenRows(buildTurns(failing(1))))).toEqual([
      "question",
      "timeline",
      "step",
      "final",
    ]);
  });

  it("步骤展开：手动覆盖优先，失败手动展开给 12 行；查找命中自动展开", () => {
    const turns = buildTurns(failing(2));
    const [first, second] = turns[0].steps;
    const rows = flattenRows(turns, {
      stepOverrides: new Map([
        [first.id, false],
        [second.id, true],
      ]),
    });
    expect(stepRow(rows, 2)).toMatchObject({
      expanded: false,
      previewLines: STEP_PREVIEW_LINES,
    });
    expect(stepRow(rows, 3)).toMatchObject({
      expanded: true,
      previewLines: STEP_PREVIEW_LINES,
    });

    const claude = turnsOf("claude");
    const search = findSearchHits(claude, "tokio-util v0.7.12");
    const searched = flattenRows(claude, { search });
    const hit = searched.find(
      (row) =>
        row.kind === "step" && row.step.id === search!.matches[0].targetId,
    );
    expect(hit).toMatchObject({ expanded: true, pinned: false });
  });

  it("规则 9：合并步骤展开后列出子步骤（depth 1）", () => {
    const turns = buildTurns(
      [
        msg("user", [text("q")]),
        msg("assistant", [call("a", "read"), call("b", "search"), call("c")]),
        msg("tool", [result("a"), result("b"), result("c")]),
      ],
      { style: AGENT_READER_STYLES.codex },
    );
    const merged = turns[0].steps[0];
    expect(merged.kind).toBe("merged");
    const collapsed = flattenRows(turns);
    expect(rowKinds(collapsed)).toEqual([
      "question",
      "timeline",
      "step",
      "step",
    ]);
    const expanded = flattenRows(turns, {
      stepOverrides: new Map([[merged.id, true]]),
    });
    expect(
      expanded.map((row) => (row.kind === "step" ? row.depth : row.kind)),
    ).toEqual(["question", "timeline", 0, 1, 1, 0]);
  });

  it("显示注入的上下文：注入消息出 injected 行", () => {
    const rows = flattenRows(turnsOf("codex"), { showInjected: true });
    expect(rowKinds(rows).slice(0, 4)).toEqual([
      "injected",
      "injected",
      "injected",
      "question",
    ]);
    expect(rows[0]).toMatchObject({ messageIndex: 0, turn: 0 });
    // 纯注入轮打开开关后也出行
    const gemini = flattenRows(turnsOf("gemini"), { showInjected: true });
    expect(rowKinds(gemini).slice(0, 2)).toEqual(["injected", "turn_divider"]);
  });

  it("只看对话：只留提问与最终回复，空轮不出分轮线", () => {
    const rows = flattenRows(turnsOf("claude"), { filter: "conversation" });
    expect(rowKinds(rows)).toEqual([
      "question",
      "final",
      "turn_divider",
      "question",
      "turn_divider",
      "question",
      "final",
    ]);
  });

  it("只看改动：只留有 edit/write/diff 的轮，改动步骤直接列出", () => {
    const rows = flattenRows(turnsOf("claude"), { filter: "changes" });
    expect(rowKinds(rows)).toEqual([
      "question",
      "step",
      "step",
      "turn_divider",
      "question",
      "step",
    ]);
    const noQuestion = buildTurns([
      msg("assistant", [
        call("p", "other", { diff: { files: [], added: 1, removed: 0 } }),
      ]),
    ]);
    expect(rowKinds(flattenRows(noQuestion, { filter: "changes" }))).toEqual([
      "step",
    ]);
  });

  it("isChangeStep", () => {
    const steps = buildTurns([
      msg("assistant", [
        call("e", "edit"),
        call("w", "write"),
        call("o", "other"),
        thinking("x"),
      ]),
    ])[0].steps;
    expect(steps.map(isChangeStep)).toEqual([true, true, false, false]);
  });
});

describe("行定位与行高", () => {
  const turns: SessionTurn[] = turnsOf("claude");
  const rows = flattenRows(turns);

  it("findTurnRowIndex：跳过分轮线；未知轮 -1", () => {
    expect(findTurnRowIndex(rows, turns, "t1")).toBe(0);
    const t2 = findTurnRowIndex(rows, turns, "t2");
    expect(rows[t2]).toMatchObject({ kind: "event", turn: 1 });
    expect(findTurnRowIndex(rows, turns, "nope")).toBe(-1);
  });

  it("findMatchRowIndex：提问 / 回复 / 步骤 / 事件行 / 事件步骤；被过滤时退回轮首行；整轮不在时 -1", () => {
    const hits = findSearchHits(turns, "PR #128")!;
    const prRow = findMatchRowIndex(rows, hits.matches[0]);
    expect(rows[prRow]).toMatchObject({
      kind: "event",
      block: { kind: "pr_link" },
    });

    const all = findSearchHits(turns, "tokio")!;
    const question = all.matches.find((m) => m.target === "question")!;
    expect(rows[findMatchRowIndex(rows, question)].kind).toBe("question");
    const final = all.matches.find((m) => m.target === "final")!;
    expect(rows[findMatchRowIndex(rows, final)].kind).toBe("final");
    const step = all.matches.find((m) => m.target === "step")!;
    const searchRows = flattenRows(turns, { search: all });
    expect(searchRows[findMatchRowIndex(searchRows, step)]).toMatchObject({
      kind: "step",
    });

    // 折叠态下步骤行不存在 → 退回该轮第一行
    expect(findMatchRowIndex(rows, { ...step, targetId: "missing" })).toBe(0);
    expect(
      findMatchRowIndex(rows, { turn: 99, target: "question", count: 1 }),
    ).toBe(-1);

    const eventTurns = buildTurns([
      msg("user", [text("q")]),
      msg("assistant", [call("a")]),
      msg("system", [event("sub_agent", "hello")]),
      msg("assistant", [call("b")]),
    ]);
    const eventHits = findSearchHits(eventTurns, "hello")!;
    const eventRows = flattenRows(eventTurns);
    expect(
      eventRows[findMatchRowIndex(eventRows, eventHits.matches[0])],
    ).toMatchObject({
      kind: "step",
      step: { kind: "event" },
    });
  });

  it("estimateRowHeight", () => {
    expect(rows.map(estimateRowHeight).slice(0, 4)).toEqual([96, 36, 32, 160]);
    expect(estimateRowHeight({ kind: "turn_divider", key: "k", turn: 0 })).toBe(
      17,
    );
  });
});

describe("查找命中在预览可见行之外", () => {
  // 失败步骤默认只露 FAILURE_PREVIEW_LINES 行；命中在后面的行时整段预览都要展开，
  // 否则查找计数里有、界面上看不见（对应旧阅读页 v7 的同类修复）
  const lines = Array.from({ length: 10 }, (_, i) => `line ${i + 1}`);
  lines[1] = "early HIT here";
  lines[8] = "late TAIL here";
  const failedTurn = () =>
    buildTurns(
      [
        msg("user", [text("run")]),
        msg("assistant", [call("f")]),
        msg("tool", [
          result("f", "error", {
            preview: lines.join("\n"),
            lineCount: lines.length,
            totalLen: lines.join("\n").length,
          }),
        ]),
        msg("assistant", [text("done")]),
      ].map((message) => ({ ...message, turnId: "t" })),
      { style: AGENT_READER_STYLES.claude },
    );
  const failedStepRow = (query: string) => {
    const turns = failedTurn();
    const rows = flattenRows(turns, { search: findSearchHits(turns, query) });
    const row = rows.find((r) => r.kind === "step");
    if (!row || row.kind !== "step") throw new Error("no step row");
    return row;
  };

  it("命中在可见行内：保持默认预览行数", () => {
    const row = failedStepRow("hit");
    expect(row.expanded).toBe(true);
    expect(row.previewLines).toBe(FAILURE_PREVIEW_LINES);
  });

  it("命中在可见行之后：整段预览展开", () => {
    const row = failedStepRow("tail");
    expect(row.expanded).toBe(true);
    expect(row.previewLines).toBeGreaterThan(lines.length);
  });

  it("没有查找时不受影响", () => {
    const turns = failedTurn();
    const row = flattenRows(turns).find((r) => r.kind === "step");
    if (!row || row.kind !== "step") throw new Error("no step row");
    expect(row.previewLines).toBe(FAILURE_PREVIEW_LINES);
  });
});
