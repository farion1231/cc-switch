import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderActions } from "@/components/providers/ProviderActions";

describe("ProviderActions OpenClaw default model selection", () => {
  it("asks which model to use when a provider has multiple models", async () => {
    const user = userEvent.setup();
    const onSetAsDefault = vi.fn();

    render(
      <ProviderActions
        appId="openclaw"
        isCurrent={false}
        isInConfig
        isDefaultModel={false}
        defaultModelOptions={[
          { id: "gpt-5.6-sol", name: "GPT-5.6 Sol" },
          { id: "gpt-5.6-luna", name: "GPT-5.6 Luna" },
        ]}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        onSetAsDefault={onSetAsDefault}
      />,
    );

    await user.click(screen.getByRole("button", { name: /设为默认/ }));
    await user.click(screen.getByText("GPT-5.6 Luna"));

    expect(onSetAsDefault).toHaveBeenCalledWith("gpt-5.6-luna");
  });

  it("uses the only configured model without opening a picker", async () => {
    const user = userEvent.setup();
    const onSetAsDefault = vi.fn();

    render(
      <ProviderActions
        appId="openclaw"
        isCurrent={false}
        isInConfig
        isDefaultModel={false}
        defaultModelOptions={[{ id: "claude-opus-5", name: "Claude Opus 5" }]}
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDuplicate={vi.fn()}
        onDelete={vi.fn()}
        onSetAsDefault={onSetAsDefault}
      />,
    );

    await user.click(screen.getByRole("button", { name: "设为默认" }));

    expect(onSetAsDefault).toHaveBeenCalledWith("claude-opus-5");
  });
});
