import type {
  OpenClawAgentsDefaults,
  OpenClawEnvConfig,
  OpenClawToolsProfile,
} from "@/types";

export const OPENCLAW_TOOL_PROFILES: OpenClawToolsProfile[] = [
  "minimal",
  "coding",
  "messaging",
  "full",
];

export const OPENCLAW_UNSUPPORTED_PROFILE = "__unsupported_profile__";
export const OPENCLAW_UNSET_PROFILE = "__unset_profile__";

export function parseOpenClawEnvEditorValue(raw: string): OpenClawEnvConfig {
  if (!raw.trim()) {
    throw new Error("OPENCLAW_ENV_EMPTY");
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("OPENCLAW_ENV_INVALID_JSON");
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("OPENCLAW_ENV_OBJECT_REQUIRED");
  }
  return parsed as OpenClawEnvConfig;
}

export function isOpenClawToolsProfile(
  profile?: string,
): profile is OpenClawToolsProfile {
  return (
    typeof profile === "string" &&
    OPENCLAW_TOOL_PROFILES.includes(profile as OpenClawToolsProfile)
  );
}

export function getOpenClawToolsProfileSelectValue(profile?: string): string {
  if (!profile) {
    return OPENCLAW_UNSET_PROFILE;
  }
  return isOpenClawToolsProfile(profile)
    ? profile
    : OPENCLAW_UNSUPPORTED_PROFILE;
}

export function getOpenClawUnsupportedProfile(profile?: string): string | null {
  if (!profile || isOpenClawToolsProfile(profile)) {
    return null;
  }
  return profile;
}

export function getOpenClawTimeoutInputValue(
  defaults?: OpenClawAgentsDefaults | null,
): string {
  const timeoutSeconds =
    typeof defaults?.timeoutSeconds === "number"
      ? defaults.timeoutSeconds
      : undefined;
  const legacyTimeout =
    typeof defaults?.timeout === "number" ? defaults.timeout : undefined;
  const value = timeoutSeconds ?? legacyTimeout;
  return value === undefined ? "" : String(value);
}
