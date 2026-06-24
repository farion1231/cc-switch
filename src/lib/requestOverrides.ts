import type { LocalProxyRequestOverrides } from "@/types";

export interface RequestOverrideJsonResult {
  value?: Record<string, unknown>;
  error?: string;
}

export interface HeaderOverrideValidationResult {
  headers?: Record<string, string>;
  error?: string;
}

// RFC 9110 HTTP field-name token. Keep this aligned with Rust's
// http::HeaderName parser for user-facing validation.
export function isValidHttpHeaderName(name: string): boolean {
  return /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name);
}

export const PROTECTED_LOCAL_PROXY_HEADER_NAMES = new Set([
  "host",
  "content-length",
  "transfer-encoding",
  "connection",
  "proxy-authorization",
  "proxy-authenticate",
  "te",
  "trailer",
  "upgrade",
  "accept-encoding",
  "content-type",
  "authorization",
  "x-api-key",
  "x-goog-api-key",
  "chatgpt-account-id",
  "session_id",
  "x-client-request-id",
  "x-codex-window-id",
  "x-forwarded-host",
  "x-forwarded-port",
  "x-forwarded-proto",
  "forwarded",
  "cf-connecting-ip",
  "cf-ipcountry",
  "cf-ray",
  "cf-visitor",
  "true-client-ip",
  "fastly-client-ip",
  "x-azure-clientip",
  "x-azure-fdid",
  "x-azure-ref",
  "akamai-origin-hop",
  "x-akamai-config-log-detail",
  "x-request-id",
  "x-correlation-id",
  "x-trace-id",
  "x-amzn-trace-id",
  "x-b3-traceid",
  "x-b3-spanid",
  "x-b3-parentspanid",
  "x-b3-sampled",
  "traceparent",
  "tracestate",
]);

export function isProtectedLocalProxyHeaderName(name: string): boolean {
  return PROTECTED_LOCAL_PROXY_HEADER_NAMES.has(name.toLowerCase());
}

// Keep this aligned with Rust's http::HeaderValue runtime guard: only control
// characters other than tab are rejected.
export function isValidHttpHeaderValue(value: string): boolean {
  // eslint-disable-next-line no-control-regex
  return !/[\x00-\x08\x0a-\x1f\x7f]/.test(value);
}

export function isPlainObject(
  value: unknown,
): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function parseRequestOverrideJson(
  raw: string,
): RequestOverrideJsonResult {
  const trimmed = raw.trim();
  if (!trimmed) return {};

  try {
    const parsed = JSON.parse(trimmed);
    if (!isPlainObject(parsed)) {
      return { error: "JSON must be an object" };
    }
    return { value: parsed };
  } catch (error) {
    return {
      error: error instanceof Error ? error.message : "Invalid JSON",
    };
  }
}

export function parseBodyOverrideJson(raw: string): RequestOverrideJsonResult {
  const parsed = parseRequestOverrideJson(raw);
  if (parsed.error || !parsed.value) {
    return parsed;
  }
  if (Object.prototype.hasOwnProperty.call(parsed.value, "stream")) {
    return { error: 'Body override must not include protocol field "stream"' };
  }
  return parsed;
}

export function parseHeaderOverrideJson(
  raw: string,
): HeaderOverrideValidationResult {
  const parsed = parseRequestOverrideJson(raw);
  if (parsed.error || !parsed.value) {
    return parsed.error ? { error: parsed.error } : {};
  }

  const headers: Record<string, string> = {};
  for (const [name, value] of Object.entries(parsed.value)) {
    const headerName = name.trim();
    if (!headerName) {
      return { error: "Header name must not be empty" };
    }
    if (!isValidHttpHeaderName(headerName)) {
      return { error: `Header "${name}" name is not a valid HTTP token` };
    }
    if (typeof value !== "string") {
      return { error: `Header "${name}" value must be a string` };
    }
    if (!isValidHttpHeaderValue(value)) {
      return { error: `Header "${name}" value contains control characters` };
    }
    const normalizedHeaderName = headerName.toLowerCase();
    if (Object.prototype.hasOwnProperty.call(headers, normalizedHeaderName)) {
      return {
        error: `Header "${name}" duplicates another header after case normalization`,
      };
    }
    if (isProtectedLocalProxyHeaderName(normalizedHeaderName)) {
      return {
        error: `Header "${name}" is managed by the local proxy and cannot be overridden`,
      };
    }
    headers[normalizedHeaderName] = value;
  }

  return { headers };
}

export function formatRequestOverrideObject(
  value: Record<string, unknown> | undefined,
): string {
  if (!value || Object.keys(value).length === 0) return "";
  return JSON.stringify(value, null, 2);
}

export function buildLocalProxyRequestOverrides(
  headersJson: string,
  bodyJson: string,
): { overrides?: LocalProxyRequestOverrides; error?: string } {
  const headerResult = parseHeaderOverrideJson(headersJson);
  if (headerResult.error) {
    return { error: headerResult.error };
  }

  const bodyResult = parseBodyOverrideJson(bodyJson);
  if (bodyResult.error) {
    return { error: bodyResult.error };
  }

  const overrides: LocalProxyRequestOverrides = {};
  if (headerResult.headers && Object.keys(headerResult.headers).length > 0) {
    overrides.headers = headerResult.headers;
  }
  if (bodyResult.value && Object.keys(bodyResult.value).length > 0) {
    overrides.body = bodyResult.value;
  }

  return Object.keys(overrides).length > 0 ? { overrides } : {};
}
