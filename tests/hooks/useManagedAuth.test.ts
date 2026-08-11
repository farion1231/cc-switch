/** @fileoverview Refresh-policy contracts for shared managed OAuth status. */

import { describe, expect, it } from "vitest";
import { managedAuthStatusRefetchInterval } from "@/components/providers/forms/hooks/useManagedAuth";

describe("managedAuthStatusRefetchInterval", () => {
  it("refreshes providers whose proxy hot path can persist reauthentication state", () => {
    expect(managedAuthStatusRefetchInterval("xai_oauth")).toBe(15_000);
    expect(managedAuthStatusRefetchInterval("kimi_oauth")).toBe(15_000);
  });

  it("leaves providers without hot-path status transitions event-driven", () => {
    expect(managedAuthStatusRefetchInterval("github_copilot")).toBe(false);
    expect(managedAuthStatusRefetchInterval("codex_oauth")).toBe(false);
  });
});
