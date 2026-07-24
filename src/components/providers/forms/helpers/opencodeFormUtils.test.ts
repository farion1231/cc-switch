import { describe, expect, it } from "vitest";
import { PI_DEFAULT_CONFIG } from "./opencodeFormUtils";

describe("PI_DEFAULT_CONFIG", () => {
  it("uses Pi's supported OpenAI Chat Completions API identifier", () => {
    const config = JSON.parse(PI_DEFAULT_CONFIG);

    expect(config.api).toBe("openai-completions");
  });
});
