import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { GptContextWindowHint } from "@/components/providers/forms/shared";

const hint = (modelId: string | undefined, contextWindow: unknown) =>
  render(
    <GptContextWindowHint modelId={modelId} contextWindow={contextWindow} />,
  ).container.textContent;

describe("GptContextWindowHint", () => {
  it("warns only when a GPT window exceeds the subscription window", () => {
    expect(hint("gpt-6-astra", "1050000")).toContain("272K");
    expect(hint("openai/gpt-6.1-sol", 500000)).toContain("272K");

    expect(hint("gpt-6-astra", "272000")).toBe("");
    expect(hint("gpt-6-astra", "")).toBe("");
    expect(hint("deepseek-v4", 1000000)).toBe("");
    expect(hint(undefined, 1050000)).toBe("");
  });
});
