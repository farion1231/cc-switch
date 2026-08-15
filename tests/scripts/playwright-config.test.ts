import { describe, expect, it } from "vitest";
import config from "../../playwright.config";

describe("Task 7 Playwright config", () => {
  it("uses the exact Tandem demo URL as its base URL", () => {
    expect(config.use?.baseURL).toBe("http://127.0.0.1:3000/tandem-demo.html");
  });
});
