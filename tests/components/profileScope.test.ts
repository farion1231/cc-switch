import { describe, expect, it } from "vitest";
import type { PerApp, Profile } from "@/lib/api/profiles";
import {
  APP_PROFILE_SCOPE,
  hasScopeSnapshot,
} from "@/components/profiles/scope";

const perApp = <T>(values: Partial<PerApp<T>>, fallback: T): PerApp<T> => ({
  claude: fallback,
  "claude-desktop": fallback,
  codex: fallback,
  "codex-desktop": fallback,
  ...values,
});

const createProfile = (): Profile => ({
  id: "profile-1",
  name: "Profile 1",
  payload: {
    providers: perApp({}, null),
    mcp: perApp({}, null),
    skills: perApp({}, null),
    prompts: perApp({}, null),
  },
});

describe("Codex Desktop profile scope", () => {
  it("maps the app to its independent profile scope", () => {
    expect(APP_PROFILE_SCOPE["codex-desktop"]).toBe("codex-desktop");
  });

  it("considers only the provider slot when detecting a snapshot", () => {
    const profile = createProfile();
    profile.payload.mcp["codex-desktop"] = [];
    profile.payload.skills["codex-desktop"] = ["skill-1"];
    profile.payload.prompts["codex-desktop"] = "prompt-1";

    expect(hasScopeSnapshot(profile, "codex-desktop")).toBe(false);

    profile.payload.providers["codex-desktop"] = "provider-1";
    expect(hasScopeSnapshot(profile, "codex-desktop")).toBe(true);
  });
});
