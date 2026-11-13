import { describe, expect, it } from "vitest";
import { orderAppIds } from "@/config/appConfig";
import { APP_IDS } from "@/config/appSpecs";

describe("orderAppIds", () => {
  it("falls back to the default order when nothing is saved", () => {
    expect(orderAppIds(undefined)).toEqual(APP_IDS);
    expect(orderAppIds([])).toEqual(APP_IDS);
  });

  it("keeps the saved order and drops unknown or repeated ids", () => {
    const saved = [...APP_IDS].reverse();
    expect(orderAppIds(["future-app", ...saved, "claude"])).toEqual(saved);
  });

  it("puts an app missing from the saved order after its default predecessor", () => {
    const saved = APP_IDS.filter((app) => app !== "kimi");
    const [first, ...rest] = saved;
    // 把 mcode（kimi 默认的前一个）挪到最后，kimi 应跟着它
    const moved = [first, ...rest.filter((app) => app !== "mcode"), "mcode"];
    expect(orderAppIds(moved)).toEqual([...moved, "kimi"]);
    expect(orderAppIds(APP_IDS.filter((app) => app !== "claude"))[0]).toBe(
      "claude",
    );
  });
});
