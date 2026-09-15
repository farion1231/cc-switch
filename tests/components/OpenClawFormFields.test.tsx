import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { FormProvider, useForm } from "react-hook-form";
import { OpenClawFormFields } from "@/components/providers/forms/OpenClawFormFields";

const models = [
  {
    id: "gpt-5.6-sol",
    name: "GPT-5.6 Sol",
    reasoning: true,
    input: ["text", "image"],
    contextWindow: 272000,
    maxTokens: 128000,
    cost: {
      input: 2.5,
      output: 15,
      cacheRead: 0.25,
      cacheWrite: 3.125,
    },
  },
  {
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
    input: ["text"],
  },
];

function renderForm() {
  const onModelsChange = vi.fn();
  function TestForm() {
    const form = useForm();
    return (
      <FormProvider {...form}>
        <OpenClawFormFields
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
          userAgent={false}
          onUserAgentChange={vi.fn()}
        />
      </FormProvider>
    );
  }
  render(<TestForm />);
  return { onModelsChange };
}

describe("OpenClawFormFields model editor", () => {
  it("uses the family model-row layout without inferred model roles", () => {
    renderForm();

    expect(screen.getByText("模型配置")).toBeInTheDocument();
    expect(screen.getAllByText("模型 ID")).toHaveLength(1);
    expect(screen.getAllByText("显示名称")).toHaveLength(1);
    expect(screen.queryByText("默认模型")).not.toBeInTheDocument();
    expect(screen.queryByText("回退模型")).not.toBeInTheDocument();
    expect(screen.getByDisplayValue("gpt-5.6-sol")).toBeInTheDocument();
    expect(screen.getByDisplayValue("GPT-5.6 Luna")).toBeInTheDocument();
  });

  it("reveals native OpenClaw model details from the row chevron", async () => {
    const user = userEvent.setup();
    renderForm();

    const toggles = screen.getAllByRole("button", {
      name: "展开或收起模型详情",
    });
    await user.click(toggles[0]);

    expect(screen.getByText("支持扩展思考")).toBeInTheDocument();
    expect(screen.getByText("输入类型")).toBeInTheDocument();
    expect(screen.getByText("上下文长度")).toBeInTheDocument();
    expect(screen.getByText("最大输出 Token 数")).toBeInTheDocument();
    expect(screen.getByText("成本（$/百万 Token）")).toBeInTheDocument();
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
