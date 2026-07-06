import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { PiProviderForm } from "@/components/pi/PiProviderForm";

describe("PiProviderForm", () => {
  it("sets api from selected OpenAI-compatible preset", () => {
    const onChange = vi.fn();
    render(<PiProviderForm value={undefined} onChange={onChange} />);

    fireEvent.click(screen.getByText("OpenAI-compatible"));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({
        template: "openAiCompatible",
        api: "openai-completions",
      }),
    );
  });

  it("defaults model name to model id until user edits name", () => {
    const onChange = vi.fn();
    render(<PiProviderForm value={undefined} onChange={onChange} />);

    fireEvent.change(screen.getByLabelText(/Model ID/i), {
      target: { value: "qwen3-coder" },
    });

    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        models: [
          expect.objectContaining({
            id: "qwen3-coder",
            name: "qwen3-coder",
          }),
        ],
      }),
    );
  });
});
