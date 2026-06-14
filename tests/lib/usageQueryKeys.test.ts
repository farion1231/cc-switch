import { describe, expect, it } from "vitest";
import { usageKeys } from "@/lib/query/usage";

describe("usageKeys.logs", () => {
  it("separates request log caches by session id", () => {
    const baseKey = {
      preset: "today" as const,
      customStartDate: undefined,
      customEndDate: undefined,
      appType: "codex",
      providerName: "provider-a",
      model: "gpt-5.5",
      statusCode: 200,
    };

    expect(
      usageKeys.logs({ ...baseKey, sessionId: "session-a" }, 0, 20),
    ).not.toEqual(
      usageKeys.logs({ ...baseKey, sessionId: "session-b" }, 0, 20),
    );
  });
});
