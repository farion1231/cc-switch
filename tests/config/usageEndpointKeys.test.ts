import { describe, expect, it } from "vitest";
import { usageKeys } from "@/lib/query/usage";

describe("endpoint log query keys", () => {
  it("keeps all endpoints separate from the unknown endpoint bucket", () => {
    const all = usageKeys.logs({ preset: "today" }, 0, 20);
    const unknown = usageKeys.logs({ preset: "today", endpointId: "" }, 0, 20);
    const selected = usageKeys.logs(
      { preset: "today", endpointId: "selected" },
      0,
      20,
    );
    expect(all).not.toEqual(unknown);
    expect(unknown).not.toEqual(selected);
  });
});
