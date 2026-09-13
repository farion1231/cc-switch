import { describe, expect, it } from "vitest";

import { TEMPLATE_TYPES } from "@/config/constants";

describe("usage template types", () => {
  it("exposes the native Sub2API template", () => {
    expect((TEMPLATE_TYPES as Record<string, string>).SUB2API).toBe("sub2api");
  });
});
