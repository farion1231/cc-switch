import { isMap, isScalar, isSeq, parseDocument, type Document } from "yaml";

/**
 * subagent markdown 的 frontmatter 读写（纯函数，不碰 React）。
 *
 * 文件是 Claude Code 读的，用户也会手改：表单只改认识的几个键，其余原样留着——
 * 未知键（hooks、permissionMode、disallowedTools…）、注释、键的顺序、tools 原来是
 * 逗号字符串还是 YAML 列表，都尽量不动。所以这里用 yaml 的 Document API 改 AST，
 * 而不是 parse 成对象再整份 stringify。
 */

/** Claude Code 对 subagent 名字的约束：小写字母、数字、连字符，最长 64 */
export const SUBAGENT_NAME_RE = /^[a-z0-9][a-z0-9-]{0,63}$/;

export const SUBAGENT_COLORS = [
  "red",
  "blue",
  "green",
  "yellow",
  "purple",
  "orange",
  "pink",
  "cyan",
] as const;
export type SubagentColor = (typeof SUBAGENT_COLORS)[number];

/** 表单认识的键；undefined = 文件里没有这个键 */
export interface SubagentFields {
  name?: string;
  description?: string;
  model?: string;
  color?: string;
  /** undefined = 没写 tools（继承全部工具） */
  tools?: string[];
}

/** tools 在文件里的写法：`Read, Grep` 字符串，或 YAML 列表 */
export type ToolsFormat = "string" | "list";

export interface ParsedSubagentMarkdown {
  hasFrontmatter: boolean;
  fields: SubagentFields;
  /** 文件里 tools 的写法；没写 tools 时为 null */
  toolsFormat: ToolsFormat | null;
  /** frontmatter 之后的正文（去掉了紧跟在 `---` 后面的空行） */
  body: string;
}

export type FrontmatterErrorCode = "unclosed" | "yaml" | "not_map";

export class FrontmatterError extends Error {
  readonly code: FrontmatterErrorCode;
  constructor(code: FrontmatterErrorCode, message: string) {
    super(message);
    this.name = "FrontmatterError";
    this.code = code;
  }
}

/**
 * 补丁：键不出现 = 不动；null = 删掉这个键；值 = 设成这个值。
 * 必填的 name / description 清空时传 ""，键留在原位，免得重新出现时跑到末尾。
 */
export type SubagentFieldsPatch = {
  [K in keyof SubagentFields]?: SubagentFields[K] | null;
} & {
  /** 新写 tools 时用的写法；不传就沿用文件里原来的，原来没有就用逗号字符串 */
  toolsFormat?: ToolsFormat;
};

const STRING_KEYS = ["name", "description", "model", "color"] as const;
const FIELD_ORDER = ["name", "description", "tools", "model", "color"] as const;

// 只认 Claude Code 的写法：第一行就是 ---，到下一行单独的 --- 为止
const OPEN_RE = /^\uFEFF?---[ \t]*\r?\n/;
const CLOSE_RE = /^---[ \t]*$/;

interface SplitResult {
  hasFrontmatter: boolean;
  /** 开头的 BOM + `---` 那一行（含换行） */
  open: string;
  /** frontmatter 的 YAML 原文 */
  yaml: string;
  /** 结束的 `---` 那一行（含换行，文件到此结束时可能没有换行） */
  close: string;
  /** 正文前面的空行，单独存着，改正文时原样放回 */
  gap: string;
  body: string;
  eol: string;
}

function detectEol(content: string): string {
  return content.includes("\r\n") ? "\r\n" : "\n";
}

function split(content: string): SplitResult {
  const eol = detectEol(content);
  const open = content.match(OPEN_RE);
  if (!open) {
    return {
      hasFrontmatter: false,
      open: "",
      yaml: "",
      close: "",
      gap: "",
      body: content,
      eol,
    };
  }
  const rest = content.slice(open[0].length);
  // 逐行找结束的 ---（不能用 indexOf，正文里的 --- 分隔线不算）
  let offset = 0;
  while (offset <= rest.length) {
    const lineEnd = rest.indexOf("\n", offset);
    const end = lineEnd === -1 ? rest.length : lineEnd;
    const line = rest.slice(offset, end).replace(/\r$/, "");
    if (CLOSE_RE.test(line)) {
      const afterClose = lineEnd === -1 ? rest.length : lineEnd + 1;
      const after = rest.slice(afterClose);
      const gap = after.match(/^(?:[ \t]*\r?\n)*/)?.[0] ?? "";
      return {
        hasFrontmatter: true,
        open: open[0],
        yaml: rest.slice(0, offset),
        close: rest.slice(offset, afterClose),
        gap,
        body: after.slice(gap.length),
        eol,
      };
    }
    if (lineEnd === -1) break;
    offset = lineEnd + 1;
  }
  throw new FrontmatterError("unclosed", "frontmatter is missing closing ---");
}

function parseYaml(yaml: string): Document.Parsed {
  const doc = parseDocument(yaml);
  if (doc.errors.length > 0) {
    throw new FrontmatterError("yaml", doc.errors[0].message);
  }
  if (doc.contents !== null && !isMap(doc.contents)) {
    throw new FrontmatterError("not_map", "frontmatter must be a YAML mapping");
  }
  return doc;
}

function scalarText(value: unknown): string | undefined {
  if (value === null || value === undefined) return undefined;
  return String(value);
}

/** 逗号字符串拆成工具列表（`mcp__a__b` 这类名字里没有逗号） */
export function splitToolsString(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function readTools(doc: Document.Parsed): {
  tools?: string[];
  format: ToolsFormat | null;
} {
  const node = doc.get("tools", true);
  if (node === undefined) return { format: null };
  if (isSeq(node)) {
    const tools = node.items
      .map((item) => (isScalar(item) ? scalarText(item.value) : undefined))
      .filter((item): item is string => Boolean(item?.trim()))
      .map((item) => item.trim());
    return { tools, format: "list" };
  }
  if (isScalar(node)) {
    const text = scalarText(node.value);
    // `tools:` 留空等于没写
    if (text === undefined) return { format: "string" };
    return { tools: splitToolsString(text), format: "string" };
  }
  return { format: null };
}

function readFields(doc: Document.Parsed): {
  fields: SubagentFields;
  toolsFormat: ToolsFormat | null;
} {
  const fields: SubagentFields = {};
  for (const key of STRING_KEYS) {
    const value = scalarText(doc.get(key));
    if (value !== undefined) fields[key] = value;
  }
  const { tools, format } = readTools(doc);
  if (tools !== undefined) fields.tools = tools;
  return { fields, toolsFormat: format };
}

/** 解析整份 markdown；frontmatter 不合法时抛 FrontmatterError */
export function parseSubagentMarkdown(content: string): ParsedSubagentMarkdown {
  const parts = split(content);
  if (!parts.hasFrontmatter) {
    return {
      hasFrontmatter: false,
      fields: {},
      toolsFormat: null,
      body: content,
    };
  }
  const { fields, toolsFormat } = readFields(parseYaml(parts.yaml));
  return { hasFrontmatter: true, fields, toolsFormat, body: parts.body };
}

export type SafeParseResult =
  | { ok: true; value: ParsedSubagentMarkdown }
  | { ok: false; code: FrontmatterErrorCode; message: string };

export function safeParseSubagentMarkdown(content: string): SafeParseResult {
  try {
    return { ok: true, value: parseSubagentMarkdown(content) };
  } catch (error) {
    if (error instanceof FrontmatterError) {
      return { ok: false, code: error.code, message: error.message };
    }
    throw error;
  }
}

function stringifyDoc(doc: Document.Parsed, eol: string): string {
  if (
    !doc.contents ||
    (isMap(doc.contents) && doc.contents.items.length === 0)
  ) {
    // 空映射 stringify 出来是 `{}`，frontmatter 里留空就行
    const comment = doc.commentBefore ? `#${doc.commentBefore}\n` : "";
    return comment.replace(/\n/g, eol);
  }
  // lineWidth 0：长 description 不折行；flowCollectionPadding false：`[Read, Grep]` 不变成 `[ Read, Grep ]`
  const text = doc.toString({ lineWidth: 0, flowCollectionPadding: false });
  return eol === "\n" ? text : text.replace(/\n/g, eol);
}

function setString(doc: Document.Parsed, key: string, value: string) {
  const node = doc.get(key, true);
  // 改原节点的值：节点上的注释、引号风格（含 | 块字符串）都留着；
  // 原风格放不下新值时（比如 plain 里出现 `: `）yaml 会自己换成引号
  if (isScalar(node)) {
    node.value = value;
    return;
  }
  doc.set(key, value);
}

function setTools(
  doc: Document.Parsed,
  tools: string[],
  format: ToolsFormat | undefined,
) {
  const node = doc.get("tools", true);
  const current: ToolsFormat | null = isSeq(node)
    ? "list"
    : node === undefined
      ? null
      : "string";
  const target = format ?? current ?? "string";
  if (target === "list") {
    const seq = doc.createNode(tools);
    if (isSeq(seq) && isSeq(node)) {
      // 原来是 [a, b] 行内列表就继续写行内；挂在列表上的注释也带过去
      if (node.flow) seq.flow = true;
      if (node.comment) seq.comment = node.comment;
      if (node.commentBefore) seq.commentBefore = node.commentBefore;
    }
    doc.set("tools", seq);
    return;
  }
  setString(doc, "tools", tools.join(", "));
}

/**
 * 按补丁改 frontmatter，返回新的整份 markdown。正文一个字都不动。
 * 原来没有 frontmatter 的，在最前面补一段。frontmatter 不合法时抛 FrontmatterError。
 */
export function updateSubagentFrontmatter(
  content: string,
  patch: SubagentFieldsPatch,
): string {
  const parts = split(content);
  const doc = parseYaml(parts.yaml);

  // 按固定顺序处理：新加的键排在 name / description / tools / model / color 的惯常位置
  for (const key of FIELD_ORDER) {
    if (key === "tools") {
      if ("tools" in patch) {
        const tools = patch.tools;
        if (tools === null || tools === undefined) doc.delete("tools");
        else setTools(doc, tools, patch.toolsFormat);
      } else if (patch.toolsFormat) {
        // 只换写法（字符串 ↔ 列表），内容不变
        const { tools } = readTools(doc);
        if (tools !== undefined) setTools(doc, tools, patch.toolsFormat);
      }
      continue;
    }
    if (!(key in patch)) continue;
    const value = patch[key];
    if (value === null || value === undefined) doc.delete(key);
    else setString(doc, key, value);
  }

  const yaml = stringifyDoc(doc, parts.eol);
  if (!parts.hasFrontmatter) {
    const eol = parts.eol;
    const gap = content.trim() ? eol : "";
    return `---${eol}${yaml}---${eol}${gap}${content}`;
  }
  // 结束行如果没有换行（文件到 --- 为止），补上一个
  const close = /\n$/.test(parts.close)
    ? parts.close
    : `${parts.close}${parts.eol}`;
  return `${parts.open}${yaml}${close}${parts.gap}${parts.body}`;
}

/** 正文（不含 frontmatter 和紧跟着的空行）；frontmatter 不完整时整份当正文 */
export function getSubagentBody(content: string): string {
  try {
    return split(content).body;
  } catch {
    return content;
  }
}

/**
 * 换正文，frontmatter 原文（哪怕 YAML 不合法）一字不动。
 * 新正文和原正文一样时原样返回，编辑器回填值时不会凭空改出差异。
 */
export function replaceSubagentBody(content: string, body: string): string {
  let parts: SplitResult;
  try {
    parts = split(content);
  } catch {
    return body;
  }
  if (parts.body === body) return content;
  if (!parts.hasFrontmatter) return body;
  const close = /\n$/.test(parts.close)
    ? parts.close
    : `${parts.close}${parts.eol}`;
  // 原来 --- 后面直接接正文的也空一行，和 Claude Code 自己写出来的一致
  const gap = parts.gap || (body ? parts.eol : "");
  return `${parts.open}${parts.yaml}${close}${gap}${body}`;
}

/** 从字段和正文拼一份新的 markdown（模板、复制为新 agent 用） */
export function buildSubagentMarkdown(
  fields: SubagentFields,
  body: string,
): string {
  const patch: SubagentFieldsPatch = {};
  if (fields.name !== undefined) patch.name = fields.name;
  if (fields.description !== undefined) patch.description = fields.description;
  if (fields.tools !== undefined) patch.tools = fields.tools;
  if (fields.model !== undefined) patch.model = fields.model;
  if (fields.color !== undefined) patch.color = fields.color;
  return updateSubagentFrontmatter(`---\n---\n\n${body}`, patch);
}
