import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { PiFormFields } from "@/components/providers/forms/PiFormFields";
import { Form } from "@/components/ui/form";

function TestForm() {
  const form = useForm();

  return (
    <Form {...form}>
      <PiFormFields
        baseUrl="https://api.example.com/v1"
        onBaseUrlChange={vi.fn()}
        apiKey="sk-test"
        onApiKeyChange={vi.fn()}
        shouldShowApiKeyLink={false}
        websiteUrl="https://example.com"
        api="openai-chat"
        onApiChange={vi.fn()}
        models={[
          { id: "duplicate-model", name: "First" },
          { id: "duplicate-model", name: "Second" },
        ]}
        onModelsChange={vi.fn()}
        defaultModel="duplicate-model"
        onDefaultModelChange={vi.fn()}
      />
    </Form>
  );
}

describe("PiFormFields", () => {
  it("deduplicates model ids in the default-model selector", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    render(<TestForm />);

    const selects = screen.getAllByRole("combobox");
    await user.click(selects[1]);

    expect(
      await screen.findAllByRole("option", { name: "duplicate-model" }),
    ).toHaveLength(1);
  });
});
