import { describe, expect, it, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { CodexReasoningSettings } from "@/components/providers/forms/CodexReasoningSettings";
import type {
  CodexReasoningContinuationConfig,
  CodexSystemPromptConfig,
} from "@/types";

const basePrompt: CodexSystemPromptConfig = {
  enabled: false,
  replacement: "",
  correctModelIdentity: true,
};

const baseCont: CodexReasoningContinuationConfig = {
  enabled: false,
  maxRounds: 3,
};

describe("CodexReasoningSettings", () => {
  it("keeps prompt and continuation toggles independent", () => {
    const onPrompt = vi.fn();
    const onCont = vi.fn();
    render(
      <CodexReasoningSettings
        systemPrompt={basePrompt}
        onSystemPromptChange={onPrompt}
        continuation={baseCont}
        onContinuationChange={onCont}
      />,
    );

    const switches = screen.getAllByRole("switch");
    expect(switches.length).toBeGreaterThanOrEqual(2);

    // Enable system prompt only
    fireEvent.click(switches[0]);
    expect(onPrompt).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true }),
    );
    expect(onCont).not.toHaveBeenCalled();

    onPrompt.mockClear();
    // Enable continuation only
    fireEvent.click(switches[1]);
    expect(onCont).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true }),
    );
    expect(onPrompt).not.toHaveBeenCalled();
  });

  it("shows replacement textarea only when prompt enabled", () => {
    const { rerender } = render(
      <CodexReasoningSettings
        systemPrompt={basePrompt}
        onSystemPromptChange={vi.fn()}
        continuation={baseCont}
        onContinuationChange={vi.fn()}
      />,
    );
    expect(
      screen.queryByTestId("codex-system-prompt-textarea"),
    ).not.toBeInTheDocument();

    rerender(
      <CodexReasoningSettings
        systemPrompt={{ ...basePrompt, enabled: true, replacement: "hello" }}
        onSystemPromptChange={vi.fn()}
        continuation={baseCont}
        onContinuationChange={vi.fn()}
      />,
    );
    const ta = screen.getByTestId("codex-system-prompt-textarea");
    expect(ta).toBeInTheDocument();
    expect(ta).toHaveValue("hello");
  });

  it("clamps max rounds to 1..3", () => {
    const onCont = vi.fn();
    render(
      <CodexReasoningSettings
        systemPrompt={basePrompt}
        onSystemPromptChange={vi.fn()}
        continuation={{ enabled: true, maxRounds: 3 }}
        onContinuationChange={onCont}
      />,
    );
    const input = screen.getByTestId("codex-continuation-max-rounds");
    fireEvent.change(input, { target: { value: "9" } });
    expect(onCont).toHaveBeenCalledWith(
      expect.objectContaining({ maxRounds: 3 }),
    );
    fireEvent.change(input, { target: { value: "0" } });
    expect(onCont).toHaveBeenCalledWith(
      expect.objectContaining({ maxRounds: 1 }),
    );
  });
});
