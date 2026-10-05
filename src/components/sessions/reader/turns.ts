import type {
  EventBlock,
  ImageRef,
  SessionBlock,
  SessionMessage,
  ThinkingBlock,
  ToolCallBlock,
  ToolResultBlock,
  ToolStatus,
  TurnIndex,
} from "@/types";
import { countMatches } from "../utils";
import { AGENT_READER_STYLES, type AgentReaderStyle } from "./agentStyles";
import {
  isFailureStep,
  mergeSteps,
  summarizeTurn,
  type TurnSummary,
} from "./toolSummary";

/**
 * 会话阅读页的 turn 结构与虚拟列表行模型。
 *
 * 纯函数：SessionMessage[] → SessionTurn[]（提问 → 执行过程步骤 → 最终回复）
 * → ReaderRow[]（交给 useVirtualizer）。折叠规则全部在这里表达，组件只按行渲染。
 */

// ─── turn 结构 ────────────────────────────────────────────────────────────

/** OpenCode step-finish 的 tokens / 费用，并入该段最后一步（或最终回复）的元信息 */
export interface StepCost {
  tokens?: number;
  costUsd?: number;
  reason?: string;
}

interface StepBase {
  /** 稳定 key：展开状态、搜索命中、虚拟列表行 key 都用它 */
  id: string;
  cost?: StepCost;
}

export interface ToolStep extends StepBase {
  kind: "tool";
  /** 调用所在消息；孤儿结果为结果所在消息 */
  messageIndex: number;
  /** 缺省表示无配对的结果（Grok Build 之类），显示为通用「工具输出」 */
  call?: ToolCallBlock;
  result?: ToolResultBlock;
  resultMessageIndex?: number;
  /** 有结果取结果状态；无结果为 pending（旧 content 兜底出来的调用为 unknown） */
  status: ToolStatus;
}

export interface ThinkingStep extends StepBase {
  kind: "thinking";
  messageIndex: number;
  block: ThinkingBlock;
}

/** 过程中的助手说明（最终回复之前的 Text） */
export interface NoteStep extends StepBase {
  kind: "note";
  messageIndex: number;
  text: string;
}

/** 过程中的图片（Codex ImageView 等） */
export interface ImageStep extends StepBase {
  kind: "image";
  messageIndex: number;
  image: ImageRef;
}

/** 夹在执行过程中间的事件（子代理消息、Gemini info…） */
export interface EventStep extends StepBase {
  kind: "event";
  messageIndex: number;
  block: EventBlock;
  /** 同一条 system 消息里跟在事件后的正文（Codex agent_message） */
  body?: string;
}

/** 按风格配置合并的步骤：Codex Explored、Claude「Called X N times」 */
export interface MergedStep extends StepBase {
  kind: "merged";
  mergeKind: "explored" | "mcp";
  server?: string;
  children: ToolStep[];
  status: ToolStatus;
}

export type TimelineStep =
  | ToolStep
  | ThinkingStep
  | NoteStep
  | ImageStep
  | EventStep
  | MergedStep;

export interface TurnEvent {
  id: string;
  messageIndex: number;
  block: EventBlock;
  body?: string;
}

export interface TurnQuestion {
  messageIndex: number;
  text: string;
  images: ImageRef[];
  ts?: number;
}

export interface TurnFinal {
  messageIndex: number;
  text: string;
  images: ImageRef[];
  ts?: number;
  model?: string;
  cost?: StepCost;
}

export interface SessionTurn {
  /** turnId（同一 turnId 再次出现时加 `#n` 后缀保证唯一） */
  key: string;
  /** 在 turns 数组里的下标 */
  index: number;
  firstMessageIndex: number;
  lastMessageIndex: number;
  question?: TurnQuestion;
  /** 提问之前的事件（模型切换、压缩、斜杠命令…） */
  leadingEvents: TurnEvent[];
  /** 执行过程（已按风格合并） */
  steps: TimelineStep[];
  final?: TurnFinal;
  /** 最终回复之后的事件（中断、Hook 错误、PR 链接…） */
  trailingEvents: TurnEvent[];
  /** 注入型消息的下标（默认隐藏，决策 D10） */
  injected: number[];
  /** 本轮所有 step-finish 的费用（含没有落到任何步骤上的段） */
  stepCosts: StepCost[];
  aborted: boolean;
  ts?: number;
  endTs?: number;
}

export interface BuildTurnsOptions {
  /** 决定合并规则；缺省用 generic（不合并） */
  style?: AgentReaderStyle;
}

/** 旧后端从 content 兜底出来的调用 id 前缀：这类调用没有结果配对，状态记为 unknown */
export const LEGACY_CALL_PREFIX = "legacy:";

const LEGACY_TOOL_LINE = /^\[Tool(?::\s*([^\]]+))?\](?:\s+(.*))?$/;

const hasText = (block: SessionBlock) =>
  block.type === "text" && block.text.trim().length > 0;

/**
 * 消息的有效块：有 blocks 用 blocks；旧后端（blocks 缺失）按 content 兜底——
 * tool 角色整段当无配对输出，assistant 的 `[Tool: X] title` 行当调用。
 */
export const effectiveBlocks = (
  message: SessionMessage,
  messageIndex: number,
): SessionBlock[] => {
  if (message.blocks && message.blocks.length > 0) return message.blocks;
  const content = message.content ?? "";
  if (!content.trim()) return [];
  if (message.role === "tool") {
    return [
      {
        type: "tool_result",
        callId: "",
        status: "unknown",
        preview: content,
        totalLen: content.length,
        lineCount: content.split("\n").length,
        truncated: false,
      },
    ];
  }
  if (message.role !== "assistant") return [{ type: "text", text: content }];

  const blocks: SessionBlock[] = [];
  let pending: string[] = [];
  const flush = () => {
    const text = pending.join("\n").trim();
    if (text) blocks.push({ type: "text", text });
    pending = [];
  };
  content.split(/\r?\n/).forEach((line) => {
    const match = LEGACY_TOOL_LINE.exec(line.trim());
    if (!match) {
      pending.push(line);
      return;
    }
    flush();
    const rawName = match[1]?.trim() || "Tool";
    blocks.push({
      type: "tool_call",
      id: `${LEGACY_CALL_PREFIX}${messageIndex}:${blocks.length}`,
      rawName,
      kind: "other",
      title: match[2]?.trim() || rawName,
      inputPreview: "",
      inputTotalLen: 0,
    });
  });
  flush();
  return blocks;
};

/**
 * 消息的纯文本：有 blocks 时拼接 Text 块（后端不再下发 content 投影），
 * 旧后端没有 blocks 时用 content。带 full 的 Text 只含预览，全文按需取。
 */
export const messageText = (message: SessionMessage): string =>
  message.blocks && message.blocks.length > 0
    ? message.blocks
        .flatMap((block) => (block.type === "text" ? [block.text] : []))
        .join("\n\n")
    : (message.content ?? "");

/** 真人提问：非注入的 user 消息，含非空文本或图片 */
const isQuestionMessage = (message: SessionMessage, index: number) =>
  message.role === "user" &&
  !message.injected &&
  effectiveBlocks(message, index).some(
    (block) => hasText(block) || block.type === "image",
  );

/** 按 turnId 把连续消息分组；旧数据没有 turnId 时按提问递增 */
const groupMessages = (messages: SessionMessage[]) => {
  const groups: { key: string; start: number; end: number }[] = [];
  const seen = new Map<string, number>();
  let autoTurn = 0;
  let lastId: string | undefined;
  messages.forEach((message, index) => {
    let id = message.turnId;
    if (!id) {
      if (isQuestionMessage(message, index)) autoTurn += 1;
      id = `auto-${autoTurn}`;
    }
    const last = groups[groups.length - 1];
    if (last && lastId === id) {
      last.end = index;
      return;
    }
    lastId = id;
    const times = seen.get(id) ?? 0;
    seen.set(id, times + 1);
    groups.push({
      key: times === 0 ? id : `${id}#${times}`,
      start: index,
      end: index,
    });
  });
  return groups;
};

const joinTexts = (blocks: SessionBlock[]) =>
  blocks
    .filter(hasText)
    .map((block) => (block as { text: string }).text)
    .join("\n\n");

/**
 * 最终回复：turn 内最后一条含非空 Text 的 assistant 消息里、位于本轮最后一个 tool_call
 * 之后的 Text 块。最后一个 tool_call 在这些 Text 之后 → 本轮没有最终回复。
 * 返回 [消息下标, 起始块下标]。
 */
const locateFinal = (
  messages: SessionMessage[],
  blocksOf: (index: number) => SessionBlock[],
  start: number,
  end: number,
): [number, number] | undefined => {
  let lastCall: [number, number] = [-1, -1];
  for (let i = start; i <= end; i += 1) {
    if (messages[i].injected) continue;
    blocksOf(i).forEach((block, j) => {
      if (block.type === "tool_call") lastCall = [i, j];
    });
  }
  for (let i = end; i >= start; i -= 1) {
    const message = messages[i];
    if (message.role !== "assistant" || message.injected) continue;
    if (i < lastCall[0]) return undefined;
    const from = i === lastCall[0] ? lastCall[1] + 1 : 0;
    if (blocksOf(i).slice(from).some(hasText)) return [i, from];
  }
  return undefined;
};

const buildTurn = (
  messages: SessionMessage[],
  group: { key: string; start: number; end: number },
  turnIndex: number,
  style: AgentReaderStyle,
): SessionTurn => {
  const { key, start, end } = group;
  const blockCache = new Map<number, SessionBlock[]>();
  const blocksOf = (index: number) => {
    let blocks = blockCache.get(index);
    if (!blocks) {
      blocks = effectiveBlocks(messages[index], index);
      blockCache.set(index, blocks);
    }
    return blocks;
  };

  let questionIndex = -1;
  for (let i = start; i <= end; i += 1) {
    if (isQuestionMessage(messages[i], i)) {
      questionIndex = i;
      break;
    }
  }
  const finalAt = locateFinal(messages, blocksOf, start, end);

  const turn: SessionTurn = {
    key,
    index: turnIndex,
    firstMessageIndex: start,
    lastMessageIndex: end,
    leadingEvents: [],
    steps: [],
    trailingEvents: [],
    injected: [],
    stepCosts: [],
    aborted: false,
  };

  const steps: TimelineStep[] = [];
  const callSteps = new Map<string, ToolStep>();
  // 本轮所有调用 id：Claude Code 偶尔先写结果、后写调用记录（异步子代理启动时），
  // 这种「早到」的结果先挂起，等调用出现再配上，而不是当成孤儿输出
  const laterCalls = new Set<string>();
  for (let i = start; i <= end; i += 1) {
    if (messages[i].injected) continue;
    blocksOf(i).forEach((block) => {
      if (block.type === "tool_call" && block.id) laterCalls.add(block.id);
    });
  }
  const earlyResults = new Map<
    string,
    { block: ToolResultBlock; messageIndex: number }
  >();
  let finalCost: StepCost | undefined;
  let segmentStart = 0;
  const finalImages: ImageRef[] = [];
  const finalTexts: SessionBlock[] = [];

  for (let i = start; i <= end; i += 1) {
    const message = messages[i];
    if (message.ts !== undefined) {
      turn.ts ??= message.ts;
      turn.endTs = message.ts;
    }
    if (message.injected) {
      turn.injected.push(i);
      continue;
    }
    let lastEventInMessage: EventStep | undefined;
    blocksOf(i).forEach((block, j) => {
      const id = `${key}:${i}:${j}`;
      const inFinal =
        finalAt !== undefined && i === finalAt[0] && j >= finalAt[1];
      switch (block.type) {
        case "text": {
          if (inFinal) {
            finalTexts.push(block);
            return;
          }
          if (i === questionIndex || !block.text.trim()) return;
          // system 消息里跟在事件后的正文并入该事件
          if (message.role === "system" && lastEventInMessage) {
            lastEventInMessage.body = block.text;
            return;
          }
          steps.push({ kind: "note", id, messageIndex: i, text: block.text });
          return;
        }
        case "image": {
          if (i === questionIndex) return;
          if (inFinal) {
            finalImages.push(block.image);
            return;
          }
          steps.push({
            kind: "image",
            id,
            messageIndex: i,
            image: block.image,
          });
          return;
        }
        case "thinking": {
          if (!block.text && !block.summary && !block.redacted) return;
          steps.push({ kind: "thinking", id, messageIndex: i, block });
          return;
        }
        case "tool_call": {
          const step: ToolStep = {
            kind: "tool",
            id,
            messageIndex: i,
            call: block,
            status: block.id.startsWith(LEGACY_CALL_PREFIX)
              ? "unknown"
              : "pending",
          };
          if (block.id && !callSteps.has(block.id)) {
            callSteps.set(block.id, step);
            const early = earlyResults.get(block.id);
            if (early) {
              earlyResults.delete(block.id);
              step.result = early.block;
              step.resultMessageIndex = early.messageIndex;
              step.status = early.block.status;
            }
          }
          steps.push(step);
          return;
        }
        case "tool_result": {
          const owner = block.callId ? callSteps.get(block.callId) : undefined;
          if (owner && !owner.result) {
            owner.result = block;
            owner.resultMessageIndex = i;
            owner.status = block.status;
            return;
          }
          if (
            !owner &&
            laterCalls.has(block.callId) &&
            !earlyResults.has(block.callId)
          ) {
            earlyResults.set(block.callId, { block, messageIndex: i });
            return;
          }
          steps.push({
            kind: "tool",
            id,
            messageIndex: i,
            result: block,
            resultMessageIndex: i,
            status: block.status,
          });
          return;
        }
        case "event": {
          if (block.kind === "aborted") turn.aborted = true;
          const beforeQuestion =
            questionIndex >= 0
              ? i <= questionIndex
              : !steps.some((step) => step.kind !== "event");
          if (beforeQuestion) {
            turn.leadingEvents.push({ id, messageIndex: i, block });
            return;
          }
          lastEventInMessage = { kind: "event", id, messageIndex: i, block };
          steps.push(lastEventInMessage);
          return;
        }
        case "step": {
          if (block.phase === "start") {
            segmentStart = steps.length;
            return;
          }
          const cost: StepCost = {
            tokens: block.tokens,
            costUsd: block.costUsd,
            reason: block.reason,
          };
          turn.stepCosts.push(cost);
          if (steps.length > segmentStart) {
            steps[steps.length - 1].cost = cost;
          } else {
            finalCost = cost;
          }
          segmentStart = steps.length;
          return;
        }
      }
    });
  }

  // 事件之后再没有别的步骤 → 不算过程中的事件，移到最终回复之后
  let lastWorkStep = -1;
  steps.forEach((step, index) => {
    if (step.kind !== "event") lastWorkStep = index;
  });
  const trailing = steps.splice(lastWorkStep + 1) as EventStep[];
  turn.trailingEvents = trailing.map(({ id, messageIndex, block, body }) => ({
    id,
    messageIndex,
    block,
    body,
  }));

  if (questionIndex >= 0) {
    const question = messages[questionIndex];
    const blocks = blocksOf(questionIndex);
    turn.question = {
      messageIndex: questionIndex,
      text: joinTexts(blocks),
      images: blocks.flatMap((block) =>
        block.type === "image" ? [block.image] : [],
      ),
      ts: question.ts,
    };
  }
  if (finalAt) {
    const message = messages[finalAt[0]];
    turn.final = {
      messageIndex: finalAt[0],
      text: joinTexts(finalTexts),
      images: finalImages,
      ts: message.ts,
      model: message.meta?.model,
      cost: finalCost,
    };
  }
  turn.steps = mergeSteps(steps, style);
  return turn;
};

/**
 * SessionMessage[] → SessionTurn[]。
 * 分组按 turnId（后端：Codex 原生 turn_id，其余按真人提问递增 `t{n}`）；旧数据缺 turnId 时按提问自行递增。
 * 配对在 turn 内按 callId 做：无结果 → pending；无调用的结果 → 通用「工具输出」步骤。
 */
export const buildTurns = (
  messages: SessionMessage[],
  options: BuildTurnsOptions = {},
): SessionTurn[] => {
  const style = options.style ?? AGENT_READER_STYLES.generic;
  return groupMessages(messages).map((group, index) =>
    buildTurn(messages, group, index, style),
  );
};

// ─── TOC ──────────────────────────────────────────────────────────────────

const QUESTION_PREVIEW_CHARS = 80;

const previewText = (text: string) => {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > QUESTION_PREVIEW_CHARS
    ? `${flat.slice(0, QUESTION_PREVIEW_CHARS)}…`
    : flat;
};

/**
 * 前端自算的轮次索引（旧后端 / 一次性 get_session_messages 没有 Header 时用）。
 * 与后端一致：只收有真人提问或斜杠命令的轮，排除纯注入轮。
 */
export const buildTurnIndex = (turns: SessionTurn[]): TurnIndex[] =>
  turns.flatMap((turn) => {
    const slash = turn.leadingEvents.find(
      (event) => event.block.kind === "slash_command",
    );
    if (!turn.question && !slash) return [];
    const summary = summarizeTurn(turn);
    return [
      {
        turnId: turn.key,
        firstMessageIndex: turn.firstMessageIndex,
        lastMessageIndex: turn.lastMessageIndex,
        questionPreview: previewText(
          turn.question?.text || slash?.block.text || "",
        ),
        ts: turn.question?.ts ?? turn.ts,
        stepCount: summary.stepCount,
        errorCount: summary.errorCount,
        hasFinalReply: Boolean(turn.final),
        aborted: turn.aborted,
      },
    ];
  });

// ─── 搜索 ─────────────────────────────────────────────────────────────────

export interface SearchMatch {
  /** turns 下标 */
  turn: number;
  target: "question" | "step" | "final" | "event";
  /** target = step 时为命中的（子）步骤 id；event 为事件 id */
  targetId?: string;
  count: number;
}

export interface SearchHits {
  query: string;
  total: number;
  /** 文档顺序 */
  matches: SearchMatch[];
  /** 命中在执行过程里、需要展开时间线的轮 */
  turnKeys: Set<string>;
  /** 需要展开的步骤（含合并步骤本身与命中的子步骤） */
  stepIds: Set<string>;
  /** 命中在提问 / 最终回复里的轮（长内容折叠时要自动展开，规则 11） */
  questionKeys: Set<string>;
  finalKeys: Set<string>;
}

/** 步骤可被查找的文本：标题、detail、参数预览、输出预览、思考、说明、事件 */
export const stepSearchText = (step: TimelineStep): string => {
  switch (step.kind) {
    case "tool":
      return [
        step.call?.title,
        step.call?.detail,
        step.call?.inputPreview,
        step.result?.preview,
      ]
        .filter(Boolean)
        .join("\n");
    case "thinking":
      return [step.block.summary, step.block.text].filter(Boolean).join("\n");
    case "note":
      return step.text;
    case "event":
      return [step.block.text, step.body].filter(Boolean).join("\n");
    default:
      return "";
  }
};

/** 在 turn 结构上查找；空查询返回 null。注入内容不参与（默认不可见） */
export const findSearchHits = (
  turns: SessionTurn[],
  query: string,
): SearchHits | null => {
  if (!query.trim()) return null;
  const hits: SearchHits = {
    query,
    total: 0,
    matches: [],
    turnKeys: new Set(),
    stepIds: new Set(),
    questionKeys: new Set(),
    finalKeys: new Set(),
  };
  const add = (match: SearchMatch) => {
    hits.total += match.count;
    hits.matches.push(match);
  };
  const eventText = (event: TurnEvent) =>
    [event.block.text, event.body].filter(Boolean).join("\n");

  turns.forEach((turn) => {
    turn.leadingEvents.forEach((event) => {
      const count = countMatches(eventText(event), query);
      if (count) {
        add({ turn: turn.index, target: "event", targetId: event.id, count });
      }
    });
    const questionCount = countMatches(turn.question?.text ?? "", query);
    if (questionCount) {
      hits.questionKeys.add(turn.key);
      add({ turn: turn.index, target: "question", count: questionCount });
    }
    const visitStep = (step: TimelineStep, parent?: MergedStep) => {
      if (step.kind === "merged") {
        step.children.forEach((child) => visitStep(child, step));
        return;
      }
      const count = countMatches(stepSearchText(step), query);
      if (!count) return;
      hits.turnKeys.add(turn.key);
      hits.stepIds.add(step.id);
      if (parent) hits.stepIds.add(parent.id);
      add({ turn: turn.index, target: "step", targetId: step.id, count });
    };
    turn.steps.forEach((step) => visitStep(step));
    const finalCount = countMatches(turn.final?.text ?? "", query);
    if (finalCount) {
      hits.finalKeys.add(turn.key);
      add({ turn: turn.index, target: "final", count: finalCount });
    }
    turn.trailingEvents.forEach((event) => {
      const count = countMatches(eventText(event), query);
      if (count) {
        add({ turn: turn.index, target: "event", targetId: event.id, count });
      }
    });
  });
  return hits;
};

// ─── 行模型 ───────────────────────────────────────────────────────────────

/** 失败步骤在折叠态下最多常显几条（规则 2） */
export const MAX_PINNED_FAILURES = 5;
/** 失败步骤自动展开时的输出预览行数（规则 2） */
export const FAILURE_PREVIEW_LINES = 6;
/** 步骤展开时的输出预览行数（规则 6） */
export const STEP_PREVIEW_LINES = 12;

export type ReaderFilter = "all" | "conversation" | "changes";

export type ReaderRow =
  | {
      kind: "question";
      key: string;
      turn: number;
      messageIndex: number;
      /** 查找命中：长提问自动展开 */
      forceExpanded: boolean;
    }
  | {
      kind: "timeline";
      key: string;
      turn: number;
      summary: TurnSummary;
      expanded: boolean;
      /** 折叠态下超过 MAX_PINNED_FAILURES 没列出的失败步骤数 */
      hiddenFailures: number;
    }
  | {
      kind: "step";
      key: string;
      turn: number;
      step: TimelineStep;
      /** 0 = 顶层；1 = 合并步骤的子步骤 */
      depth: 0 | 1;
      /** 详情是否展开 */
      expanded: boolean;
      /** 输出预览行数（失败自动展开 6，其余 12） */
      previewLines: number;
      /** 折叠态下因失败常显 */
      pinned: boolean;
    }
  | {
      kind: "final";
      key: string;
      turn: number;
      messageIndex: number;
      forceExpanded: boolean;
    }
  | {
      kind: "event";
      key: string;
      turn: number;
      /** TurnEvent.id，查找命中定位用 */
      eventId: string;
      messageIndex: number;
      block: EventBlock;
      body?: string;
    }
  | { kind: "injected"; key: string; turn: number; messageIndex: number }
  | { kind: "turn_divider"; key: string; turn: number };

export interface FlattenOptions {
  /** 「只看对话」「只看改动」 */
  filter?: ReaderFilter;
  /** 「展开全部过程」（UI 偏好 sessionReader.expandTimelines） */
  expandAll?: boolean;
  /** 用户手动切换过的轮：true 展开 / false 收起，优先于自动规则 */
  turnOverrides?: ReadonlyMap<string, boolean>;
  /** 用户手动切换过的步骤（含合并步骤） */
  stepOverrides?: ReadonlyMap<string, boolean>;
  search?: SearchHits | null;
  /** 「显示注入的上下文」（决策 D10 默认关） */
  showInjected?: boolean;
}

/** 改动类步骤：edit / write 或带 diff 的调用 */
export const isChangeStep = (step: TimelineStep): step is ToolStep =>
  step.kind === "tool" &&
  (step.call?.kind === "edit" ||
    step.call?.kind === "write" ||
    Boolean(step.call?.diff));

/**
 * 轮的执行过程是否展开（规则 3、4）：手动切换优先；否则「展开全部」、中断、
 * 无最终回复、查找命中在步骤内时自动展开；最后一轮不特殊处理（决策 D5）。
 */
export const isTimelineExpanded = (
  turn: SessionTurn,
  options: FlattenOptions = {},
): boolean =>
  options.turnOverrides?.get(turn.key) ??
  (Boolean(options.expandAll) ||
    turn.aborted ||
    !turn.final ||
    Boolean(options.search?.turnKeys.has(turn.key)));

const stepRows = (
  turn: SessionTurn,
  step: TimelineStep,
  options: FlattenOptions,
  depth: 0 | 1,
  pinned: boolean,
): ReaderRow[] => {
  const override = options.stepOverrides?.get(step.id);
  const failure = isFailureStep(step);
  const searchHit = Boolean(options.search?.stepIds.has(step.id));
  const expanded = override ?? (failure || searchHit);
  const basePreviewLines =
    failure && override === undefined
      ? FAILURE_PREVIEW_LINES
      : STEP_PREVIEW_LINES;
  const rows: ReaderRow[] = [
    {
      kind: "step",
      key: `${turn.key}:step:${step.id}`,
      turn: turn.index,
      step,
      depth,
      expanded,
      // 查找命中在默认可见行之外（如失败步骤只露 6 行）：整段预览都展开，否则命中看不见
      previewLines:
        searchHit &&
        options.search &&
        previewTailHasQuery(step, basePreviewLines, options.search.query)
          ? Number.MAX_SAFE_INTEGER
          : basePreviewLines,
      pinned,
    },
  ];
  if (step.kind === "merged" && expanded) {
    step.children.forEach((child) =>
      rows.push(...stepRows(turn, child, options, 1, false)),
    );
  }
  return rows;
};

/** 工具输出预览在前 `visible` 行之后还有没有命中（大小写不敏感，和查找一致） */
const previewTailHasQuery = (
  step: TimelineStep,
  visible: number,
  query: string,
): boolean => {
  if (step.kind !== "tool" || !step.result?.preview || !query) return false;
  return step.result.preview
    .split(/\r?\n/)
    .slice(visible)
    .join("\n")
    .toLowerCase()
    .includes(query.toLowerCase());
};

const eventRow = (turn: SessionTurn, event: TurnEvent): ReaderRow => ({
  kind: "event",
  key: `${turn.key}:event:${event.id}`,
  turn: turn.index,
  eventId: event.id,
  messageIndex: event.messageIndex,
  block: event.block,
  body: event.body,
});

const questionRow = (
  turn: SessionTurn,
  question: TurnQuestion,
  options: FlattenOptions,
): ReaderRow => ({
  kind: "question",
  key: `${turn.key}:question`,
  turn: turn.index,
  messageIndex: question.messageIndex,
  forceExpanded: Boolean(options.search?.questionKeys.has(turn.key)),
});

const finalRow = (
  turn: SessionTurn,
  final: TurnFinal,
  options: FlattenOptions,
): ReaderRow => ({
  kind: "final",
  key: `${turn.key}:final`,
  turn: turn.index,
  messageIndex: final.messageIndex,
  forceExpanded: Boolean(options.search?.finalKeys.has(turn.key)),
});

const timelineRows = (
  turn: SessionTurn,
  options: FlattenOptions,
): ReaderRow[] => {
  const { steps } = turn;
  if (steps.length === 0) return [];
  // 规则 5：只有 1 步且没失败，不要摘要行
  if (steps.length === 1 && !isFailureStep(steps[0])) {
    return stepRows(turn, steps[0], options, 0, false);
  }
  const expanded = isTimelineExpanded(turn, options);
  const failures = steps.filter(isFailureStep);
  const pinned = expanded ? [] : failures.slice(0, MAX_PINNED_FAILURES);
  const rows: ReaderRow[] = [
    {
      kind: "timeline",
      key: `${turn.key}:timeline`,
      turn: turn.index,
      summary: summarizeTurn(turn),
      expanded,
      hiddenFailures: expanded ? 0 : failures.length - pinned.length,
    },
  ];
  (expanded ? steps : pinned).forEach((step) =>
    rows.push(...stepRows(turn, step, options, 0, !expanded)),
  );
  return rows;
};

const turnRows = (turn: SessionTurn, options: FlattenOptions): ReaderRow[] => {
  const filter = options.filter ?? "all";
  const rows: ReaderRow[] = [];
  if (filter === "conversation") {
    if (turn.question) rows.push(questionRow(turn, turn.question, options));
    if (turn.final) rows.push(finalRow(turn, turn.final, options));
    return rows;
  }
  if (filter === "changes") {
    const changes = turn.steps.filter(isChangeStep);
    if (changes.length === 0) return rows;
    if (turn.question) rows.push(questionRow(turn, turn.question, options));
    changes.forEach((step) =>
      rows.push(...stepRows(turn, step, options, 0, false)),
    );
    return rows;
  }

  turn.leadingEvents.forEach((event) => rows.push(eventRow(turn, event)));
  if (options.showInjected) {
    turn.injected.forEach((messageIndex) =>
      rows.push({
        kind: "injected",
        key: `${turn.key}:injected:${messageIndex}`,
        turn: turn.index,
        messageIndex,
      }),
    );
  }
  if (turn.question) rows.push(questionRow(turn, turn.question, options));
  rows.push(...timelineRows(turn, options));
  if (turn.final) rows.push(finalRow(turn, turn.final, options));
  turn.trailingEvents.forEach((event) => rows.push(eventRow(turn, event)));
  return rows;
};

/** turn → 虚拟列表行；空轮不出行，相邻两轮之间插分轮线 */
export const flattenRows = (
  turns: SessionTurn[],
  options: FlattenOptions = {},
): ReaderRow[] => {
  const rows: ReaderRow[] = [];
  turns.forEach((turn) => {
    const own = turnRows(turn, options);
    if (own.length === 0) return;
    if (rows.length > 0) {
      rows.push({
        kind: "turn_divider",
        key: `${turn.key}:divider`,
        turn: turn.index,
      });
    }
    rows.push(...own);
  });
  return rows;
};

/** 跳到某一轮（TOC / j k）：返回该轮第一行（不含分轮线）的下标，没有则 -1 */
export const findTurnRowIndex = (
  rows: ReaderRow[],
  turns: SessionTurn[],
  turnId: string,
): number => {
  const turn = turns.find((item) => item.key === turnId);
  if (!turn) return -1;
  return rows.findIndex(
    (row) => row.turn === turn.index && row.kind !== "turn_divider",
  );
};

/** 查找命中 → 行下标；目标行被过滤掉时退回该轮第一行，整轮都不在时 -1 */
export const findMatchRowIndex = (
  rows: ReaderRow[],
  match: SearchMatch,
): number => {
  const exact = rows.findIndex((row) => {
    if (row.turn !== match.turn) return false;
    switch (match.target) {
      case "question":
        return row.kind === "question";
      case "final":
        return row.kind === "final";
      case "step":
        return row.kind === "step" && row.step.id === match.targetId;
      case "event":
        return (
          (row.kind === "event" && row.eventId === match.targetId) ||
          (row.kind === "step" && row.step.id === match.targetId)
        );
    }
  });
  if (exact >= 0) return exact;
  return rows.findIndex(
    (row) => row.turn === match.turn && row.kind !== "turn_divider",
  );
};

/** 行高估算（§6.2），配合 useVirtualizer 的动态测量 */
export const ROW_HEIGHT_ESTIMATE: Record<ReaderRow["kind"], number> = {
  question: 96,
  timeline: 36,
  step: 32,
  final: 160,
  event: 28,
  injected: 28,
  turn_divider: 17,
};

export const estimateRowHeight = (row: ReaderRow): number =>
  ROW_HEIGHT_ESTIMATE[row.kind];
