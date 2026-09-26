import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { FormProvider, useForm } from "react-hook-form";
import { StepcodeFormFields } from "@/components/providers/forms/StepcodeFormFields";

const models = [
  {
    id: "gpt-5.6-sol",
    name: "GPT-5.6 Sol",
    reasoning: true,
    contextWindow: 272000,
    maxTokens: 128000,
  },
  {
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
  },
];

function renderForm() {
  const onModelsChange = vi.fn();
  function TestForm() {
    const form = useForm();
    return (
      <FormProvider {...form}>
        <StepcodeFormFields
          baseUrl="https://api.example.com/v1"
          onBaseUrlChange={vi.fn()}
          apiKey="test-key"
          onApiKeyChange={vi.fn()}
          shouldShowApiKeyLink={false}
          websiteUrl=""
          api="openai-responses"
          onApiChange={vi.fn()}
          models={models}
          onModelsChange={onModelsChange}
        />
      </FormProvider>
    );
  }
  render(<TestForm />);
  return { onModelsChange };
}

describe("StepcodeFormFields model editor", () => {
  it("uses the family model-row layout with id and display name", () => {
    renderForm();

    expect(screen.getByText("Model Configuration")).toBeInTheDocument();
    expect(screen.getAllByText("Model ID")).toHaveLength(1);
    expect(screen.getAllByText("Display Name")).toHaveLength(1);
    expect(screen.getByDisplayValue("gpt-5.6-sol")).toBeInTheDocument();
    expect(screen.getByDisplayValue("GPT-5.6 Luna")).toBeInTheDocument();
  });

  it("reveals StepCode model details from the row chevron", async () => {
    const user = userEvent.setup();
    renderForm();

    const toggles = screen.getAllByRole("button", {
      name: "Expand or collapse model details",
    });
    await user.click(toggles[0]);

    expect(screen.getByText("Supports Extended Thinking")).toBeInTheDocument();
    expect(screen.getByText("Context Length")).toBeInTheDocument();
    expect(screen.getByText("Max Output Tokens")).toBeInTheDocument();
    // StepCode form intentionally omits input-type / cost editors.
    expect(screen.queryByText("Input Types")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Cost ($/million tokens)"),
    ).not.toBeInTheDocument();
  });

  it("keeps model name composition local until the IME commits", () => {
    const { onModelsChange } = renderForm();
    const modelNameInput = screen.getByDisplayValue("GPT-5.6 Sol");

    fireEvent.compositionStart(modelNameInput);
    fireEvent.change(modelNameInput, {
      target: { value: "mimomimo" },
    });

    expect(modelNameInput).toHaveValue("mimomimo");
    expect(onModelsChange).not.toHaveBeenCalled();

    fireEvent.compositionEnd(modelNameInput, {
      data: "mimomimo",
      target: { value: "mimomimo" },
    });

    expect(onModelsChange).toHaveBeenCalledTimes(1);
    expect(onModelsChange).toHaveBeenCalledWith([
      { ...models[0], name: "mimomimo" },
      models[1],
    ]);
  });
});
