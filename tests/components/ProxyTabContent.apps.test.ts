import { describe, expect, it } from "vitest";
import { FAILOVER_APPS } from "@/components/settings/ProxyTabContent";

describe("ProxyTabContent failover apps", () => {
  it("exposes Grok Build alongside the existing failover applications", () => {
    expect(FAILOVER_APPS.map(({ id }) => id)).toEqual([
      "claude",
      "codex",
      "gemini",
      "grokbuild",
    ]);
  });
});
