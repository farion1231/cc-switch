import { createHmac } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const BASE_URL = process.env.API_BASE_URL ?? "http://localhost:13001";

function resolveAuthToken(): string {
  if (process.env.AUTH_TOKEN) {
    return process.env.AUTH_TOKEN;
  }
  const candidates = [
    join(homedir(), ".cc-switch", "auth_token"),
    join(homedir(), ".config", "cc-switch", "auth_token"),
  ];
  for (const path of candidates) {
    if (existsSync(path)) {
      return readFileSync(path, "utf8").trim();
    }
  }
  return "cc-switch-dev-secret";
}

const JWT_SECRET = resolveAuthToken();

function base64UrlEncode(input: string): string {
  return Buffer.from(input, "utf8")
    .toString("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
}

/**
 * Generate a short-lived JWT signed with the same secret as the backend.
 */
export async function generateToken(subject = "test-user"): Promise<string> {
  const now = Math.floor(Date.now() / 1000);
  const header = base64UrlEncode(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const payload = base64UrlEncode(
    JSON.stringify({ sub: subject, iat: now, exp: now + 3600 }),
  );
  const signingInput = `${header}.${payload}`;
  const signature = createHmac("sha256", JWT_SECRET)
    .update(signingInput)
    .digest("base64")
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
  return `${signingInput}.${signature}`;
}

/**
 * GET request with optional Bearer token.
 */
export async function apiGet(
  path: string,
  token?: string,
): Promise<Response> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  return fetch(`${BASE_URL}${path}`, { headers });
}

/**
 * POST/PUT/DELETE request with optional Bearer token.
 */
export async function apiRequest(
  method: string,
  path: string,
  body?: unknown,
  token?: string,
): Promise<Response> {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = `Bearer ${token}`;
  return fetch(`${BASE_URL}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
}
