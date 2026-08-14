import { fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { HermesFormFields } from "@/components/providers/forms/HermesFormFields";
import { Form } from "@/components/ui/form";

type HermesFormFieldsProps = ComponentProps<typeof HermesFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderHermesForm = (overrides: Partial<HermesFormFieldsProps> = {}) => {
  const props: HermesFormFieldsProps = {
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    apiMode: "chat_completions",
    onApiModeChange: vi.fn(),
    models: [
      { id: "model-a", name: "Model A" },
      { id: "model-b", name: "Model B" },
    ],
    onModelsChange: vi.fn(),
    rateLimitDelay: 0.5,
    onRateLimitDelayChange: vi.fn(),
    ...overrides,
  };

  return {
    props,
    ...render(
      <FormShell>
        <HermesFormFields {...props} />
      </FormShell>,
    ),
  };
};

describe("HermesFormFields", () => {
  it("uses the clean Pi-style model rows without role badges", () => {
    renderHermesForm();

    expect(screen.getByText("模型列表").closest("div.border-l")).toHaveClass(
      "border-border-default",
      "pl-3",
    );
    expect(screen.queryByText("默认模型")).not.toBeInTheDocument();
    expect(screen.queryByText("备选模型")).not.toBeInTheDocument();
    expect(screen.queryByText("高级选项")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("上下文长度")).not.toBeInTheDocument();

    const detailsButton = screen.getAllByRole("button", {
      name: "展开或收起模型详情",
    })[0];
    expect(detailsButton).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(detailsButton);

    expect(detailsButton).toHaveAttribute("aria-expanded", "true");
    expect(
      document.getElementById(detailsButton.getAttribute("aria-controls")!),
    ).toBeInTheDocument();

    const contextLength = screen.getByLabelText("上下文长度");
    expect(contextLength).toHaveAttribute("type", "text");
    expect(contextLength).toHaveAttribute("inputmode", "numeric");
    expect(screen.getByText("上下文长度")).toHaveClass(
      "text-xs",
      "font-normal",
      "text-muted-foreground",
    );
    expect(contextLength.closest("div.border-l")).toHaveClass(
      "sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_2.25rem]",
    );
    expect(screen.getByText(/第一个模型/)).toBeInTheDocument();
  });

  it("keeps model name composition local until the IME commits", () => {
    const onModelsChange = vi.fn();
    const { props, rerender } = renderHermesForm({ onModelsChange });
    const modelNameInput = screen.getByDisplayValue("Model A");

    fireEvent.compositionStart(modelNameInput);
    fireEvent.change(modelNameInput, {
      target: { value: "mimomimo" },
    });

    expect(modelNameInput).toHaveValue("mimomimo");
    expect(onModelsChange).not.toHaveBeenCalled();

    rerender(
      <FormShell>
        <HermesFormFields {...props} />
      </FormShell>,
    );
    expect(modelNameInput).toHaveValue("mimomimo");

    fireEvent.compositionEnd(modelNameInput, {
      data: "mimomimo",
      target: { value: "mimomimo" },
    });

    expect(onModelsChange).toHaveBeenCalledTimes(1);
    expect(onModelsChange).toHaveBeenCalledWith([
      { ...props.models[0], name: "mimomimo" },
      props.models[1],
    ]);
  });

  it("shows request interval directly and updates its native provider field", () => {
    const onRateLimitDelayChange = vi.fn();
    renderHermesForm({ onRateLimitDelayChange });

    const input = screen.getByLabelText("请求间隔（秒）");
    expect(input).toHaveValue(0.5);
    expect(screen.getByText("请求间隔（秒）")).toHaveClass(
      "text-sm",
      "font-medium",
      "leading-none",
    );
    expect(input.closest("div.border-l")).toHaveClass(
      "border-border-default",
      "pl-3",
    );
    expect(
      screen.queryByRole("button", { name: "供应商高级选项" }),
    ).not.toBeInTheDocument();

    fireEvent.change(input, { target: { value: "1.25" } });
    expect(onRateLimitDelayChange).toHaveBeenLastCalledWith(1.25);

    fireEvent.change(input, { target: { value: "" } });
    expect(onRateLimitDelayChange).toHaveBeenLastCalledWith(undefined);
  });
});
