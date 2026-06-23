import type { LocalProxyRequestOverrides } from "@/types";

export interface RequestOverrideJsonResult {
  value?: Record<string, unknown>;
  error?: string;
}

export interface HeaderOverrideValidationResult {
  headers?: Record<string, string>;
  error?: string;
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
    if (typeof value !== "string") {
      return { error: `Header "${name}" value must be a string` };
    }
    if (!isValidHttpHeaderValue(value)) {
      return { error: `Header "${name}" value contains control characters` };
    }
    headers[headerName] = value;
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

  const bodyResult = parseRequestOverrideJson(bodyJson);
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
