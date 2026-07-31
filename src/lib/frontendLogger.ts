import { error as writeErrorLog } from "@tauri-apps/plugin-log";

const MAX_LOG_MESSAGE_LENGTH = 12_000;
const MAX_RAW_LOG_INPUT_LENGTH = 16_000;
const MAX_SERIALIZED_STRING_LENGTH = 2_000;
const MAX_SERIALIZED_ENTRIES = 32;
const MAX_SERIALIZED_TOTAL_VALUES = 64;
const MAX_SERIALIZATION_DEPTH = 4;
const QUERY_VALUE_PATTERN = /([?&][A-Za-z0-9_.~-]+)=([^&#\s"'<>]*)/g;
const URL_CREDENTIAL_PATTERN = /(https?:\/\/)[^/@\s]+@/gi;
const QUOTED_NAMED_SECRET_PATTERN =
  /((?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|authorization|auth|password|passwd|pwd|secret|cookie)\s*["']?\s*[:=]\s*)(["'])(.*?)\2/gi;
const NAMED_SECRET_PATTERN =
  /((?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|authorization|auth|password|passwd|pwd|secret|cookie)\s*["']?\s*[:=]\s*["']?)([^\s"',}]+)/gi;
// 敏感键的值是数组/对象(`"tokens":[...]`、`"auth":{...}`)时，标量正则够不着里面的元素。
// 文本层是所有入口(Error/string/对象/嵌套/前缀+JSON)最终汇聚的唯一出口，故在这里
// 兜底：命中敏感键名后把紧跟的 `[..]`/`{..}` 整体替换。`\b` 防止匹配到 monkey 之类的后缀，
// `(?:\\?["'])?` 同时兼容裸引号与转义引号(双重编码 JSON 里的 `\"tokens\"`)。
const NAMED_SECRET_CONTAINER_PATTERN =
  /((?:\\?["'])?\b(?:api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key|client[_-]?secret|auth[_-]?token|access[_-]?token|refresh[_-]?token|id[_-]?token|session[_-]?token|session[_-]?id|authorization|credential|password|passwd|bearer|cookie|secret|token|auth|pwd|key)s?(?:\\?["'])?\s*[:=]\s*)(\[[^\]]*\]|\{[^{}]*\})/gi;
const SENSITIVE_HEADER_LINE_PATTERN =
  /(^|[\r\n])([ \t]*(?:(?:proxy-)?authorization|cookie|set-cookie|x-api-key|api-key)\s*[:=]\s*)[^\r\n]+/gim;
const AUTH_SCHEME_PATTERN =
  /\b(Bearer|Basic|Token|ApiKey|Digest|Negotiate|AWS4-HMAC-SHA256)\s+[^\s"',}\]]+/gi;
const SECRET_VALUE_IN_TEXT_PATTERN =
  /(^|[^A-Za-z0-9]|\\[nrt])(?:sk-[A-Za-z0-9._~+\/-]{6,}|AIza[A-Za-z0-9_-]{8,}|github_pat_[A-Za-z0-9_]{6,}|gh[pousr]_[A-Za-z0-9_]{6,}|xox[baprs]-[A-Za-z0-9-]{6,}|ya29\.[A-Za-z0-9._-]{6,}|(?:AKIA|ASIA)[A-Z0-9]{12,}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)/gim;
const SECRET_VALUE_WITHIN_STRING_PATTERN =
  /(?:sk-[A-Za-z0-9._~+\/-]{6,}|AIza[A-Za-z0-9_-]{8,}|github_pat_[A-Za-z0-9_]{6,}|gh[pousr]_[A-Za-z0-9_]{6,}|xox[baprs]-[A-Za-z0-9-]{6,}|ya29\.[A-Za-z0-9._-]{6,}|(?:AKIA|ASIA)[A-Z0-9]{12,}|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+)/i;
function truncateForProcessing(input: string, limit: number): string {
  if (input.length <= limit) {
    return input;
  }
  return `${input.slice(0, limit)}\n[input truncated]`;
}

export function redactFrontendLogText(input: string): string {
  return input
    .replace(QUERY_VALUE_PATTERN, "$1=[REDACTED]")
    .replace(URL_CREDENTIAL_PATTERN, "$1[REDACTED]@")
    .replace(SENSITIVE_HEADER_LINE_PATTERN, "$1$2[REDACTED]")
    .replace(AUTH_SCHEME_PATTERN, "$1 [REDACTED]")
    .replace(SECRET_VALUE_IN_TEXT_PATTERN, "$1[REDACTED]")
    .replace(NAMED_SECRET_CONTAINER_PATTERN, "$1[REDACTED]")
    .replace(QUOTED_NAMED_SECRET_PATTERN, "$1$2[REDACTED]$2")
    .replace(NAMED_SECRET_PATTERN, "$1[REDACTED]");
}

function looksLikeSecretValue(value: string): boolean {
  const trimmed = value.trim();
  if (SECRET_VALUE_WITHIN_STRING_PATTERN.test(trimmed)) {
    return true;
  }
  if (/-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/i.test(trimmed)) {
    return true;
  }

  // Unknown opaque credentials: keep this only in the structured serializer,
  // where a false positive costs diagnostics but cannot alter application data.
  return (
    trimmed.length >= 32 &&
    /^[A-Za-z0-9._~+/=-]+$/.test(trimmed) &&
    /[A-Za-z]/.test(trimmed) &&
    /\d/.test(trimmed)
  );
}

// 结构化对象里，命中这些属性名即认定其整个值(标量/数组/对象)为敏感并整体隐藏。
// 存单数形式，查表前去掉尾部 s，让复数(tokens/apiKeys/credentials)自动覆盖，
// 不必逐个枚举——正则文本层只能匹配 `"name":"value"` 标量，抓不到数组/嵌套。
const SENSITIVE_KEY_NAMES = new Set([
  "key",
  "apikey",
  "accesskey",
  "secretkey",
  "privatekey",
  "clientsecret",
  "token",
  "authtoken",
  "accesstoken",
  "refreshtoken",
  "idtoken",
  "sessiontoken",
  "sessionid",
  "authorization",
  "auth",
  "bearer",
  "password",
  "passwd",
  "pwd",
  "secret",
  "credential",
  "cookie",
]);

function isSensitiveKey(key: string): boolean {
  const normalized = key
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "")
    .replace(/s$/, "");
  return SENSITIVE_KEY_NAMES.has(normalized);
}

function normalizeForSerialization(
  value: unknown,
  depth: number,
  ancestors: WeakSet<object>,
  budget: { remaining: number },
): unknown {
  if (budget.remaining <= 0) {
    return "[Serialization budget exhausted]";
  }
  budget.remaining -= 1;

  if (typeof value === "string") {
    // 值一级脱敏：对“看起来像密钥”的不透明串整体隐藏。命名字段(apiKey/token/...)
    // 由上层 isSensitiveKey 按属性名整体隐藏；文本里的裸密钥再由 redactFrontendLogText 兜底。
    if (looksLikeSecretValue(value)) {
      return "[REDACTED]";
    }
    return truncateForProcessing(value, MAX_SERIALIZED_STRING_LENGTH);
  }
  if (
    value == null ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return value;
  }
  if (typeof value === "bigint") {
    return `${value}n`;
  }
  if (typeof value === "symbol") {
    return String(value);
  }
  if (typeof value === "function") {
    return `[Function ${value.name || "anonymous"}]`;
  }
  if (typeof value !== "object") {
    return String(value);
  }
  if (ancestors.has(value)) {
    return "[Circular]";
  }
  if (depth >= MAX_SERIALIZATION_DEPTH) {
    return "[Object: max depth reached]";
  }

  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      const items: unknown[] = [];
      for (const item of value.slice(0, MAX_SERIALIZED_ENTRIES)) {
        if (budget.remaining <= 0) {
          break;
        }
        items.push(
          normalizeForSerialization(item, depth + 1, ancestors, budget),
        );
      }
      if (value.length > items.length) {
        items.push(`[${value.length - items.length} more items]`);
      }
      return items;
    }

    const output: Record<string, unknown> = {};
    const keys = Object.keys(value);
    for (const key of keys.slice(0, MAX_SERIALIZED_ENTRIES)) {
      if (budget.remaining <= 0) {
        output["[truncated]"] = "Serialization budget exhausted";
        break;
      }
      try {
        const descriptor = Object.getOwnPropertyDescriptor(value, key);
        if (descriptor?.get) {
          output[key] = "[Getter omitted]";
          continue;
        }
        if (isSensitiveKey(key)) {
          // 敏感属性名 → 整个值(含数组/对象)一律隐藏，不递归、不猜形状。
          output[key] = "[REDACTED]";
          continue;
        }
        output[key] = normalizeForSerialization(
          descriptor?.value,
          depth + 1,
          ancestors,
          budget,
        );
      } catch {
        output[key] = "[Property access failed]";
      }
    }
    if (keys.length > MAX_SERIALIZED_ENTRIES) {
      output["[truncated]"] =
        `${keys.length - MAX_SERIALIZED_ENTRIES} more properties`;
    }
    return output;
  } finally {
    ancestors.delete(value);
  }
}

function serializeStructured(value: unknown): string | null {
  try {
    const serialized = JSON.stringify(
      normalizeForSerialization(value, 0, new WeakSet(), {
        remaining: MAX_SERIALIZED_TOTAL_VALUES,
      }),
    );
    return serialized ?? null;
  } catch {
    return null;
  }
}

// 结构化数据可能以“字符串形态的 JSON”混进来(Promise.reject(JSON.stringify(...))、
// throw new Error(JSON.stringify(...)))。不还原结构就只剩文本正则，够不着数组/嵌套字段。
// 命中 JSON 结构则 parse 后走属性级脱敏；非 JSON 返回 null 交调用方按普通文本处理。
function redactStructuredString(text: string): string | null {
  const trimmed = text.trim();
  if (!(trimmed.startsWith("{") || trimmed.startsWith("["))) {
    return null;
  }
  if (text.length > MAX_RAW_LOG_INPUT_LENGTH) {
    // 合法的超大 JSON 一旦截断必成非法 JSON，会退回够不着数组字段的文本正则并泄漏；
    // 也不冒险 parse 多 MiB 输入阻塞 UI。检出 JSON-like 且超限即整体丢弃。
    return "[oversized structured error omitted]";
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return null;
  }
  // 标量 JSON(如 "42")无字段可脱，交给文本层。
  if (parsed === null || typeof parsed !== "object") {
    return null;
  }
  return serializeStructured(parsed) ?? "[Unserializable structured error]";
}

// 把 Error 渲染成“脱敏 message + 原生调用栈”，与浏览器引擎的 stack 格式无关：
//  - V8/Chromium(Windows WebView2)：stack 首行内嵌 message → 全局字面量替换成脱敏版；
//  - WebKit/JSC(macOS/Linux WKWebView)、SpiderMonkey：stack 是纯栈帧、不含 message → 补脱敏头。
// 不识别 `    at ` / `@` 等引擎特有格式(枚举不完，正是它把 WebKit 栈整段丢了)，只按
// “message 是否出现在 stack 里”分流，从而各平台都保留原生调用栈，且绝不残留未脱敏 message。
function renderRedactedError(error: Error, structuredMessage: string): string {
  const head = `${error.name}: ${structuredMessage}`;
  const stack = error.stack;
  if (!stack) {
    return head;
  }
  if (error.message && stack.includes(error.message)) {
    // V8：message 内嵌在 stack —— 全局字面量替换(split/join 换掉所有出现)，栈帧原样保留。
    return stack.split(error.message).join(structuredMessage);
  }
  // WebKit/Firefox：stack 不含 message —— 纯栈帧前补一个脱敏 message 头。
  return `${head}\n${stack}`;
}

function describeError(error: unknown): string {
  if (error instanceof Error) {
    // throw new Error(JSON.stringify(payload)) 很常见：凭据会藏进 message，而 V8 的
    // stack 首行就是原始 message。先对 message 按 JSON 结构化脱敏，命中则渲染成
    // “脱敏 message + 原生栈”，避免 stack 把未脱敏的 message 直接吐出去。
    const structuredMessage = redactStructuredString(error.message);
    if (structuredMessage !== null) {
      return truncateForProcessing(
        renderRedactedError(error, structuredMessage),
        MAX_RAW_LOG_INPUT_LENGTH,
      );
    }
    return truncateForProcessing(
      error.stack || `${error.name}: ${error.message}`,
      MAX_RAW_LOG_INPUT_LENGTH,
    );
  }
  if (typeof error === "string") {
    const structured = redactStructuredString(error);
    if (structured !== null) {
      return structured;
    }
    return truncateForProcessing(error, MAX_RAW_LOG_INPUT_LENGTH);
  }
  if (error == null) {
    return String(error);
  }
  const structured = serializeStructured(error);
  if (structured === null) {
    return "[Unserializable thrown value]";
  }
  return truncateForProcessing(structured, MAX_RAW_LOG_INPUT_LENGTH);
}

export function reportFrontendError(
  context: string,
  error: unknown,
  details?: string,
): void {
  // 先限制每段原始输入，再执行全局正则，避免异常携带多 MiB 文本时阻塞 UI。
  const raw = truncateForProcessing(
    [
      `[frontend] ${truncateForProcessing(context, MAX_RAW_LOG_INPUT_LENGTH)}`,
      describeError(error),
      details
        ? truncateForProcessing(details, MAX_RAW_LOG_INPUT_LENGTH).trim()
        : undefined,
    ]
      .filter(Boolean)
      .join("\n"),
    MAX_RAW_LOG_INPUT_LENGTH,
  );
  const redacted = redactFrontendLogText(raw);
  const message =
    redacted.length > MAX_LOG_MESSAGE_LENGTH
      ? `${redacted.slice(0, MAX_LOG_MESSAGE_LENGTH)}\n[truncated]`
      : redacted;

  // Web 开发/测试环境没有 Tauri invoke，日志上报失败不应再触发
  // console.error 或未处理 Promise，否则会形成错误循环。
  void writeErrorLog(message, { file: "frontend" }).catch(() => undefined);
}

export function installGlobalErrorHandlers(
  target: Window = window,
): () => void {
  const handleError = (event: ErrorEvent) => {
    const location = event.filename
      ? `${event.filename}:${event.lineno}:${event.colno}`
      : undefined;
    reportFrontendError("window.error", event.error ?? event.message, location);
  };
  const handleUnhandledRejection = (event: PromiseRejectionEvent) => {
    reportFrontendError("unhandledrejection", event.reason);
  };

  target.addEventListener("error", handleError);
  target.addEventListener("unhandledrejection", handleUnhandledRejection);

  return () => {
    target.removeEventListener("error", handleError);
    target.removeEventListener("unhandledrejection", handleUnhandledRejection);
  };
}
