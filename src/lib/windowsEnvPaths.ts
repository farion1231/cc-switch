import { invoke } from "@tauri-apps/api/core";

export const WIN_ENV_WHITELIST = [
  "USERPROFILE",
  "APPDATA",
  "LOCALAPPDATA",
  "HOMEDRIVE",
  "HOMEPATH",
  "TEMP",
  "TMP",
  "PROGRAMFILES",
  "PROGRAMFILES(X86)",
  "PROGRAMDATA",
  "SYSTEMROOT",
  "SYSTEMDRIVE",
  "PUBLIC",
  "ALLUSERSPROFILE",
] as const;

const ENV_VAR_PATTERN = /%([A-Za-z_][A-Za-z0-9_()]*)%/g;

const WHITELIST_SET = new Set<string>(WIN_ENV_WHITELIST.map((s) => s));

export interface DetectionResult {
  valid: boolean;
  known: string[];
  unknown: string[];
}

function walkStrings(value: unknown, visit: (s: string) => void): void {
  if (typeof value === "string") {
    visit(value);
    return;
  }
  if (Array.isArray(value)) {
    for (const item of value) walkStrings(item, visit);
    return;
  }
  if (value && typeof value === "object") {
    for (const v of Object.values(value as Record<string, unknown>)) {
      walkStrings(v, visit);
    }
  }
}

function transformStrings(value: unknown, map: (s: string) => string): unknown {
  if (typeof value === "string") return map(value);
  if (Array.isArray(value)) return value.map((v) => transformStrings(v, map));
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = transformStrings(v, map);
    }
    return out;
  }
  return value;
}

export function detectWindowsEnvVars(jsonText: string): DetectionResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonText);
  } catch {
    return { valid: false, known: [], unknown: [] };
  }

  const knownSet = new Set<string>();
  const unknownSet = new Set<string>();

  walkStrings(parsed, (s) => {
    for (const match of s.matchAll(ENV_VAR_PATTERN)) {
      const upper = match[1].toUpperCase();
      if (WHITELIST_SET.has(upper)) knownSet.add(upper);
      else unknownSet.add(upper);
    }
  });

  return {
    valid: true,
    known: Array.from(knownSet).sort(),
    unknown: Array.from(unknownSet).sort(),
  };
}

export interface ExpansionResult {
  text: string;
  replaced: number;
}

export async function expandWindowsEnvVars(
  jsonText: string,
): Promise<ExpansionResult> {
  const envMap = await invoke<Record<string, string>>("get_windows_env_paths");

  let parsed: unknown;
  try {
    parsed = JSON.parse(jsonText);
  } catch (e) {
    throw new Error(`Invalid JSON: ${(e as Error).message}`);
  }

  let replaced = 0;
  const transformed = transformStrings(parsed, (s) =>
    s.replace(ENV_VAR_PATTERN, (full, name: string) => {
      const upper = name.toUpperCase();
      if (!WHITELIST_SET.has(upper)) return full;
      const val = envMap[upper];
      if (typeof val !== "string") return full;
      replaced += 1;
      return val;
    }),
  );

  return {
    text: JSON.stringify(transformed, null, 2),
    replaced,
  };
}
