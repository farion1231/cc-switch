import { describe, expect, it } from "vitest";
import {
  APP_ICON_MAP,
  APP_IDS,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
} from "@/config/appConfig";

describe("Pi frontend capability boundaries", () => {
  it("registers Pi for providers but not unsupported shared features", () => {
    expect(APP_IDS).toContain("pi");
    expect(APP_ICON_MAP.pi.label).toBe("Pi Agent");
    expect(SKILLS_APP_IDS).not.toContain("pi");
    expect(MCP_APP_IDS).not.toContain("pi");
  });
});
