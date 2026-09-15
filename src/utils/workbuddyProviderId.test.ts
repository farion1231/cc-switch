import { describe, expect, it } from "vitest";
import {
  deriveWorkbuddyProviderId,
  resolveWorkbuddyProviderId,
} from "./workbuddyProviderId";

describe("deriveWorkbuddyProviderId", () => {
  // 与 Rust provider_id_from_url_rules 测试用例保持一致
  it("takes the second-level domain of the host", () => {
    expect(deriveWorkbuddyProviderId("https://api.alpha.test/v1")).toBe(
      "alpha",
    );
    expect(deriveWorkbuddyProviderId("https://api.beta.test/v1")).toBe("beta");
    expect(deriveWorkbuddyProviderId("http://localhost:8080/v1")).toBe(
      "localhost",
    );
  });

  it("tolerates missing scheme, trailing slash and blank input", () => {
    expect(deriveWorkbuddyProviderId("api.deepseek.com")).toBe("deepseek");
    expect(deriveWorkbuddyProviderId("https://api.deepseek.com/")).toBe(
      "deepseek",
    );
    expect(deriveWorkbuddyProviderId("  https://openrouter.ai/api/v1  ")).toBe(
      "openrouter",
    );
    expect(deriveWorkbuddyProviderId("")).toBe("workbuddy");
    expect(deriveWorkbuddyProviderId("https://")).toBe("workbuddy");
  });
});

describe("resolveWorkbuddyProviderId", () => {
  it("returns the derived id when unused", () => {
    expect(
      resolveWorkbuddyProviderId("https://api.alpha.test/v1", ["beta"]),
    ).toBe("alpha");
  });

  it("appends an incremental suffix on collision", () => {
    expect(
      resolveWorkbuddyProviderId("https://api.alpha.test/v1", ["alpha"]),
    ).toBe("alpha-2");
    expect(
      resolveWorkbuddyProviderId("https://api.alpha.test/v1", [
        "alpha",
        "alpha-2",
      ]),
    ).toBe("alpha-3");
  });
});
