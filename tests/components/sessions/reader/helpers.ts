import type {
  EventBlock,
  EventKind,
  SessionBlock,
  SessionMessage,
  ThinkingBlock,
  ToolCallBlock,
  ToolKind,
  ToolResultBlock,
  ToolStatus,
} from "@/types";
import claude from "../../../fixtures/sessions/claude.messages.json";
import codex from "../../../fixtures/sessions/codex.messages.json";
import gemini from "../../../fixtures/sessions/gemini.messages.json";
import generic from "../../../fixtures/sessions/generic.messages.json";
import opencode from "../../../fixtures/sessions/opencode.messages.json";
import pi from "../../../fixtures/sessions/pi.messages.json";

/** P0 契约 fixture（JSON import 会把字面量放宽成 string，这里统一断言回 SessionMessage[]） */
export const fixtures = {
  claude: claude as SessionMessage[],
  codex: codex as SessionMessage[],
  gemini: gemini as SessionMessage[],
  opencode: opencode as SessionMessage[],
  pi: pi as SessionMessage[],
  generic: generic as SessionMessage[],
};

export const msg = (
  role: string,
  blocks: SessionBlock[],
  extra: Partial<SessionMessage> = {},
): SessionMessage => ({ role, blocks, ...extra });

export const text = (value: string): SessionBlock => ({
  type: "text",
  text: value,
});

export const call = (
  id: string,
  kind: ToolKind = "shell",
  extra: Partial<ToolCallBlock> = {},
): ToolCallBlock => ({
  type: "tool_call",
  id,
  rawName: "Bash",
  kind,
  title: `cmd ${id}`,
  inputPreview: "{}",
  inputTotalLen: 2,
  ...extra,
});

export const result = (
  callId: string,
  status: ToolStatus = "success",
  extra: Partial<ToolResultBlock> = {},
): ToolResultBlock => ({
  type: "tool_result",
  callId,
  status,
  preview: `out ${callId}`,
  totalLen: 10,
  lineCount: 1,
  truncated: false,
  ...extra,
});

export const event = (kind: EventKind, value?: string): EventBlock => ({
  type: "event",
  kind,
  text: value,
});

export const thinking = (
  value: string,
  extra: Partial<ThinkingBlock> = {},
): ThinkingBlock => ({ type: "thinking", text: value, ...extra });
