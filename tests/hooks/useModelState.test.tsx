import { act, renderHook } from "@testing-library/react";

import { useModelState } from "@/components/providers/forms/hooks/useModelState";

describe("useModelState", () => {
  it("removes deprecated reasoning model env when model fields are saved", () => {
    const onConfigChange = vi.fn();
    const settingsConfig = JSON.stringify({
      env: {
        ANTHROPIC_MODEL: "claude-sonnet-4-20250514",
        ANTHROPIC_REASONING_MODEL: "claude-3-7-sonnet-20250219",
        ANTHROPIC_SMALL_FAST_MODEL: "claude-3-5-haiku-20241022",
      },
    });

    const { result } = renderHook(() =>
      useModelState({ settingsConfig, onConfigChange }),
    );

    act(() => {
      result.current.handleModelChange(
        "ANTHROPIC_MODEL",
        "claude-opus-4-1-20250805",
      );
    });

    expect(onConfigChange).toHaveBeenCalledTimes(1);
    const updated = JSON.parse(onConfigChange.mock.calls[0][0]);
    expect(updated.env).toMatchObject({
      ANTHROPIC_MODEL: "claude-opus-4-1-20250805",
    });
    expect(updated.env).not.toHaveProperty("ANTHROPIC_REASONING_MODEL");
    expect(updated.env).not.toHaveProperty("ANTHROPIC_SMALL_FAST_MODEL");
  });
});
