import { describe, expect, it } from "vitest";
import { createUsageScript } from "@/types";

describe("createUsageScript", () => {
  it("does not query reset credits unless explicitly enabled", () => {
    expect(createUsageScript().includeResetCredits).toBe(false);
  });
});
