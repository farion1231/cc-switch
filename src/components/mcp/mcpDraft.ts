import { parse as parseToml } from "smol-toml";
import type { McpServerSpec } from "@/types";
import { normalizeTomlText } from "@/utils/textNormalization";

/**
 * MCP 抽屉的草稿：表单和 JSON 两边共用的一份数据（画板 Mcp.dc.html 的 draftOf / specOf）。
 * 纯函数，不碰界面和 i18n；出错时返回错误码，由组件翻译。
 */

export type McpTransport = "stdio" | "http" | "sse";

export interface KeyValueRow {
  key: string;
  value: string;
}

export interface McpDraftConnection {
  transport: McpTransport;
  command: string;
  args: string[];
  env: KeyValueRow[];
  cwd: string;
  url: string;
  headers: KeyValueRow[];
  /** 表单不认识的字段，保存时原样保留 */
  extra: Record<string, unknown>;
}

/** JSON 里看不到真实的 env / headers 值，用这个占位 */
export const SECRET_MASK = "••••••••";

const KNOWN_FIELDS = new Set([
  "type",
  "command",
  "args",
  "env",
  "cwd",
  "url",
  "headers",
]);

function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function rowsOf(value: unknown): KeyValueRow[] {
  if (!isObject(value)) return [];
  return Object.entries(value).map(([key, v]) => ({
    key,
    value: v == null ? "" : String(v),
  }));
}

function objectOf(rows: KeyValueRow[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const row of rows) {
    const key = row.key.trim();
    if (key) out[key] = row.value;
  }
  return out;
}

export function transportOf(spec: McpServerSpec | undefined): McpTransport {
  if (spec?.type === "http" || spec?.type === "sse") return spec.type;
  if (!spec?.type && typeof spec?.url === "string" && !spec?.command) {
    return "http";
  }
  return "stdio";
}

/** 列表第二行：stdio 显示命令 + 参数，http/sse 显示去掉协议头的 URL。永不含 env / headers。 */
export function summaryOf(spec: McpServerSpec | undefined): string {
  if (!spec) return "";
  if (transportOf(spec) !== "stdio") {
    return String(spec.url ?? "").replace(/^https?:\/\//i, "");
  }
  const args = Array.isArray(spec.args) ? spec.args.map(String) : [];
  return [spec.command ?? "", ...args].join(" ").trim();
}

export function connectionOf(
  spec: McpServerSpec | undefined,
): McpDraftConnection {
  const source = spec ?? {};
  const extra: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(source)) {
    if (!KNOWN_FIELDS.has(key)) extra[key] = value;
  }
  return {
    transport: transportOf(source),
    command: typeof source.command === "string" ? source.command : "",
    args: Array.isArray(source.args) ? source.args.map(String) : [],
    env: rowsOf(source.env),
    cwd: typeof source.cwd === "string" ? source.cwd : "",
    url: typeof source.url === "string" ? source.url : "",
    headers: rowsOf(source.headers),
    extra,
  };
}

/** 草稿 → McpServerSpec。mask=true 时把 env / headers 的值换成占位符（只用于 JSON 显示）。 */
export function specOf(conn: McpDraftConnection, mask = false): McpServerSpec {
  const spec: McpServerSpec = {};
  if (conn.transport === "stdio") {
    spec.type = "stdio";
    spec.command = conn.command.trim();
    const args = conn.args.filter((arg) => arg !== "");
    if (args.length) spec.args = args;
    const env = objectOf(conn.env);
    if (Object.keys(env).length) spec.env = env;
    if (conn.cwd.trim()) spec.cwd = conn.cwd.trim();
  } else {
    spec.type = conn.transport;
    spec.url = conn.url.trim();
    const headers = objectOf(conn.headers);
    if (Object.keys(headers).length) spec.headers = headers;
  }
  for (const [key, value] of Object.entries(conn.extra)) {
    spec[key] = value;
  }
  if (mask) {
    for (const field of ["env", "headers"] as const) {
      const map = spec[field];
      if (isObject(map)) {
        spec[field] = Object.fromEntries(
          Object.keys(map).map((key) => [key, SECRET_MASK]),
        );
      }
    }
  }
  return spec;
}

export function jsonTextOf(conn: McpDraftConnection, reveal: boolean): string {
  return JSON.stringify(specOf(conn, !reveal), null, 2);
}

export function secretCountOf(conn: McpDraftConnection): number {
  const rows = conn.transport === "stdio" ? conn.env : conn.headers;
  return rows.filter((row) => row.key.trim()).length;
}

// ─── 写法归一（OpenCode local/remote、streamable-http、http_headers） ─────────
function fromOpenCode(value: Record<string, unknown>): McpServerSpec {
  if (
    value.type === "remote" ||
    (typeof value.url === "string" && !value.command)
  ) {
    const spec: McpServerSpec = {
      type: "http",
      url: typeof value.url === "string" ? value.url : "",
    };
    if (isObject(value.headers)) {
      spec.headers = value.headers as Record<string, string>;
    }
    return spec;
  }
  const command = Array.isArray(value.command)
    ? value.command.map(String)
    : [String(value.command ?? "")];
  const spec: McpServerSpec = { type: "stdio", command: command[0] ?? "" };
  if (command.length > 1) spec.args = command.slice(1);
  if (isObject(value.environment)) {
    spec.env = value.environment as Record<string, string>;
  }
  if (isObject(value.env)) spec.env = value.env as Record<string, string>;
  return spec;
}

export function normalizeSpec(value: Record<string, unknown>): McpServerSpec {
  if (
    Array.isArray(value.command) ||
    value.type === "local" ||
    value.type === "remote"
  ) {
    return fromOpenCode(value);
  }
  const spec = JSON.parse(JSON.stringify(value)) as McpServerSpec;
  const rawType = String(spec.type ?? "");
  if (rawType === "streamable-http" || rawType === "streamableHttp") {
    spec.type = "http";
  }
  if (!spec.type) {
    spec.type =
      typeof spec.url === "string" && !spec.command ? "http" : "stdio";
  }
  if (isObject(spec.http_headers) && !spec.headers) {
    spec.headers = spec.http_headers as Record<string, string>;
    delete spec.http_headers;
  }
  return spec;
}

function looksLikeSpec(value: unknown): value is Record<string, unknown> {
  return (
    isObject(value) &&
    (typeof value.command === "string" ||
      Array.isArray(value.command) ||
      typeof value.url === "string")
  );
}

function entriesOf(map: Record<string, unknown>) {
  return Object.entries(map)
    .filter(([, value]) => isObject(value))
    .map(([name, value]) => ({
      name,
      spec: normalizeSpec(value as Record<string, unknown>),
    }));
}

// ─── 粘贴识别 ────────────────────────────────────────────────────────────
export type PasteFormat =
  | "mcpServers"
  | "opencode"
  | "codex"
  | "codexToml"
  | "single"
  | "named";

export interface PastedServer {
  /** 单个服务器对象没有名称时为空 */
  name: string;
  spec: McpServerSpec;
}

export type PasteResult =
  | { ok: true; format: PasteFormat; items: PastedServer[] }
  | { ok: false };

function recognizeObject(obj: Record<string, unknown>): PasteResult {
  let items: PastedServer[] | null = null;
  let format: PasteFormat = "single";
  if (isObject(obj.mcpServers)) {
    items = entriesOf(obj.mcpServers);
    format = "mcpServers";
  } else if (isObject(obj.mcp)) {
    items = Object.entries(obj.mcp)
      .filter(([, value]) => isObject(value))
      .map(([name, value]) => ({
        name,
        spec: fromOpenCode(value as Record<string, unknown>),
      }));
    format = "opencode";
  } else if (isObject(obj.mcp_servers)) {
    items = entriesOf(obj.mcp_servers);
    format = "codex";
  } else if (looksLikeSpec(obj)) {
    items = [{ name: "", spec: normalizeSpec(obj) }];
    format = "single";
  } else {
    const keys = Object.keys(obj);
    if (keys.length && keys.every((key) => looksLikeSpec(obj[key]))) {
      items = entriesOf(obj);
      format = "named";
    }
  }
  if (!items || items.length === 0) return { ok: false };
  return { ok: true, format, items };
}

/**
 * 识别粘贴进来的 MCP 配置：单个服务器对象、"名称": {…}、{"mcpServers": {…}}、
 * Codex 的 [mcp_servers.名称]（TOML）或 mcp_servers（JSON）、OpenCode 的 mcp.local / remote。
 * 空文本返回 null。
 */
export function recognizePaste(text: string): PasteResult | null {
  const trimmed = text.trim();
  if (!trimmed) return null;

  if (/^\s*\[mcp_servers\./m.test(trimmed)) {
    try {
      const parsed = parseToml(normalizeTomlText(trimmed));
      const servers = (parsed as Record<string, unknown>).mcp_servers;
      if (!isObject(servers)) return { ok: false };
      const items = entriesOf(servers);
      return items.length
        ? { ok: true, format: "codexToml", items }
        : { ok: false };
    } catch {
      return { ok: false };
    }
  }

  let parsed: unknown = null;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    try {
      parsed = JSON.parse(`{${trimmed.replace(/,\s*$/, "")}}`);
    } catch {
      parsed = null;
    }
  }
  if (!isObject(parsed)) return { ok: false };
  return recognizeObject(parsed);
}

// ─── JSON 页签的文字 → spec ─────────────────────────────────────────────
export type JsonParseError =
  | { kind: "syntax"; line: number | null }
  | { kind: "notObject" }
  /** 值还是占位符，但草稿里没有这个键的原值可换回（改了键名、或新加的键） */
  | { kind: "maskedUnknown"; field: "env" | "headers"; key: string };

export type JsonParseResult =
  | { ok: true; spec: McpServerSpec }
  | { ok: false; error: JsonParseError };

function lineOfError(error: unknown, text: string): number | null {
  const message = String((error as Error)?.message ?? "");
  const lineMatch = /line (\d+)/.exec(message);
  if (lineMatch) return Number(lineMatch[1]);
  const posMatch = /position (\d+)/.exec(message);
  if (posMatch) return text.slice(0, Number(posMatch[1])).split("\n").length;
  return null;
}

/**
 * 解析 JSON 页签；占位符的值换回草稿里的原值。原值按键名找：改了键名、或新加的键
 * 还留着占位符，就没有原值可换，不能悄悄存成空串（凭据会丢），报错让用户填真实值。
 */
export function parseJsonText(
  text: string,
  previous: McpDraftConnection,
): JsonParseResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (error) {
    return {
      ok: false,
      error: { kind: "syntax", line: lineOfError(error, text) },
    };
  }
  if (!isObject(parsed)) return { ok: false, error: { kind: "notObject" } };
  const before = {
    env: objectOf(previous.env),
    headers: objectOf(previous.headers),
  };
  for (const field of ["env", "headers"] as const) {
    const map = parsed[field];
    if (!isObject(map)) continue;
    for (const key of Object.keys(map)) {
      if (map[key] !== SECRET_MASK) continue;
      const original = before[field][key];
      if (original === undefined) {
        return { ok: false, error: { kind: "maskedUnknown", field, key } };
      }
      map[key] = original;
    }
  }
  return { ok: true, spec: normalizeSpec(parsed) };
}

// ─── 校验 ──────────────────────────────────────────────────────────────
export type McpFieldError =
  | "nameRequired"
  | "nameSpaces"
  | "nameExists"
  | "commandRequired"
  | "urlRequired"
  | "urlScheme";

export interface McpDraftErrors {
  name?: McpFieldError;
  command?: McpFieldError;
  url?: McpFieldError;
}

export function validateDraft(
  name: string,
  conn: McpDraftConnection,
  options: { checkName: boolean; existingIds: readonly string[] },
): McpDraftErrors {
  const errors: McpDraftErrors = {};
  if (options.checkName) {
    const trimmed = name.trim();
    if (!trimmed) errors.name = "nameRequired";
    else if (/\s/.test(trimmed)) errors.name = "nameSpaces";
    else if (options.existingIds.includes(trimmed)) errors.name = "nameExists";
  }
  if (conn.transport === "stdio") {
    if (!conn.command.trim()) errors.command = "commandRequired";
  } else if (!conn.url.trim()) {
    errors.url = "urlRequired";
  } else if (!/^https?:\/\//i.test(conn.url.trim())) {
    errors.url = "urlScheme";
  }
  return errors;
}

export function uniqueId(base: string, existingIds: readonly string[]): string {
  const candidate = base.trim() || "mcp-server";
  if (!existingIds.includes(candidate)) return candidate;
  let index = 1;
  while (existingIds.includes(`${candidate}-${index}`)) index += 1;
  return `${candidate}-${index}`;
}
