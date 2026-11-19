import { describe, expect, it } from "vitest";
import { describeEvent } from "@/components/sessions/reader/SessionEventRow";

const rt = (key: string, options?: Record<string, unknown>) =>
  options && "status" in options
    ? `${key}(${String(options.status)})`
    : String(options?.defaultValue ?? key);

describe("describeEvent goal (#9647)", () => {
  it("labels the status and expands to the full objective", () => {
    const display = describeEvent(
      { type: "event", kind: "goal", text: "budgetLimited" },
      rt,
      "Migrate the docs site",
    );
    expect(display.label).toBe("event.goal(event.goalStatus.budgetLimited)");
    expect(display.detail).toBe("Migrate the docs site");
  });

  it("shows an unknown status as is", () => {
    const display = describeEvent(
      { type: "event", kind: "goal", text: "archived" },
      rt,
    );
    expect(display.label).toBe("event.goal(archived)");
    expect(display.detail).toBeUndefined();
  });
});
