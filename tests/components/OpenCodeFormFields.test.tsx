import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { Form } from "@/components/ui/form";

type OpenCodeFormFieldsProps = ComponentProps<typeof OpenCodeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

const renderOpenCodeForm = (
  overrides: Partial<OpenCodeFormFieldsProps> = {},
) => {
  const props: OpenCodeFormFieldsProps = {
    npm: "@ai-sdk/openai-compatible",
    onNpmChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    headers: {},
    onHeadersChange: vi.fn(),
    models: {
      "kimi-k2": {
        name: "Kimi K2",
        limit: { context: 1048576, output: 131072 },
      },
    },
    onModelsChange: vi.fn(),
    extraOptions: {},
    onExtraOptionsChange: vi.fn(),
    ...overrides,
  };

  return {
    props,
    ...render(
      <FormShell>
        <OpenCodeFormFields {...props} />
      </FormShell>,
    ),
  };
};

const expandFirstModel = () => {
  fireEvent.click(screen.getByRole("button", { name: "Toggle model details" }));
};

describe("OpenCodeFormFields", () => {
  it("surfaces existing provider headers", () => {
    renderOpenCodeForm({
      headers: {
        "HTTP-Referer": "https://cc-switch.app",
        "X-Title": "CC Switch",
      },
    });

    expect(screen.getByDisplayValue("HTTP-Referer")).toBeInTheDocument();
    expect(
      screen.getByDisplayValue("https://cc-switch.app"),
    ).toBeInTheDocument();
    expect(screen.getByDisplayValue("X-Title")).toBeInTheDocument();
    expect(screen.getByDisplayValue("CC Switch")).toBeInTheDocument();
  });

  it("updates provider headers", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
      onHeadersChange,
    });

    fireEvent.change(screen.getByDisplayValue("CC Switch"), {
      target: { value: "OpenCode" },
    });

    expect(onHeadersChange).toHaveBeenCalledWith({
      "X-Title": "OpenCode",
    });
  });

  it("shows a blank header name for newly added headers", () => {
    const onHeadersChange = vi.fn();
    const { rerender, props } = renderOpenCodeForm({ onHeadersChange });

    fireEvent.click(screen.getByRole("button", { name: "Add header" }));

    const nextHeaders = onHeadersChange.mock.calls[0][0];
    const headerKey = Object.keys(nextHeaders)[0];
    expect(headerKey).toMatch(/^draft-header:/);

    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} headers={nextHeaders} />
      </FormShell>,
    );

    expect(screen.getByPlaceholderText("X-Title")).toHaveValue("");
  });

  it("removes provider headers", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
      onHeadersChange,
    });

    fireEvent.click(screen.getByRole("button", { name: "Remove header" }));

    expect(onHeadersChange).toHaveBeenCalledWith({});
  });

  it("rejects case-insensitive duplicate header names and restores the input", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-A": "A", "X-B": "B" },
      onHeadersChange,
    });

    const keyInput = screen.getByDisplayValue("X-B");
    fireEvent.change(keyInput, { target: { value: "x-a" } });
    fireEvent.blur(keyInput);

    expect(onHeadersChange).not.toHaveBeenCalled();
    expect(keyInput).toHaveValue("X-B");
  });

  it("restores an existing header name when it is cleared", () => {
    const onHeadersChange = vi.fn();
    renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
      onHeadersChange,
    });

    const keyInput = screen.getByDisplayValue("X-Title");
    fireEvent.change(keyInput, { target: { value: "   " } });
    fireEvent.blur(keyInput);

    expect(onHeadersChange).not.toHaveBeenCalled();
    expect(keyInput).toHaveValue("X-Title");
  });

  it("surfaces provider options whose names start with option-", () => {
    renderOpenCodeForm({
      extraOptions: { "option-mode": "legacy" },
    });

    expect(screen.getByDisplayValue("option-mode")).toBeInTheDocument();
    expect(screen.getByDisplayValue("legacy")).toBeInTheDocument();
  });

  it("shows extra options as an always-visible addable section", () => {
    const onExtraOptionsChange = vi.fn();
    renderOpenCodeForm({ onExtraOptionsChange });

    const heading = screen.getByText("Extra SDK Options");
    const section = heading.closest("div.border-l");
    expect(section).not.toBeNull();
    expect(
      within(section as HTMLElement).getByText(
        "No extra SDK options configured",
      ),
    ).toBeVisible();
    expect(
      within(section as HTMLElement).queryByRole("button", {
        name: /Extra SDK Options/,
      }),
    ).not.toBeInTheDocument();

    fireEvent.click(
      within(section as HTMLElement).getByRole("button", { name: "Add" }),
    );

    const nextOptions = onExtraOptionsChange.mock.calls[0][0];
    expect(Object.keys(nextOptions)[0]).toMatch(/^draft-option:/);
  });

  it("uses the family section divider for model configuration", () => {
    renderOpenCodeForm();

    const section = screen.getByText("Models").closest("div.border-l");
    expect(section).toHaveClass("border-border-default", "pl-3");
  });

  it("surfaces existing model token limits", () => {
    renderOpenCodeForm();

    expandFirstModel();

    expect(screen.getByLabelText("Context")).toHaveValue(1048576);
    expect(screen.getByLabelText("Output")).toHaveValue(131072);
  });

  it("keeps model name composition local until the IME commits", () => {
    const onModelsChange = vi.fn();
    const { rerender, props } = renderOpenCodeForm({ onModelsChange });
    const modelNameInput = screen.getByDisplayValue("Kimi K2");

    fireEvent.compositionStart(modelNameInput);
    fireEvent.change(modelNameInput, {
      target: { value: "mimomimo" },
    });

    expect(modelNameInput).toHaveValue("mimomimo");
    expect(onModelsChange).not.toHaveBeenCalled();

    // The parent still owns the last committed value while the platform IME
    // owns the marked text. Re-rendering must not replace that marked text.
    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} />
      </FormShell>,
    );
    expect(modelNameInput).toHaveValue("mimomimo");

    fireEvent.compositionEnd(modelNameInput, {
      data: "mimomimo",
      target: { value: "mimomimo" },
    });

    expect(onModelsChange).toHaveBeenCalledTimes(1);
    expect(onModelsChange).toHaveBeenCalledWith({
      "kimi-k2": {
        name: "mimomimo",
        limit: { context: 1048576, output: 131072 },
      },
    });
  });

  it("commits an unfinished model ID composition on its first blur", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({ onModelsChange });
    const modelIdInput = screen.getByDisplayValue("kimi-k2");

    fireEvent.compositionStart(modelIdInput);
    fireEvent.change(modelIdInput, { target: { value: "中文模型" } });
    fireEvent.blur(modelIdInput);

    expect(onModelsChange).toHaveBeenCalledTimes(1);
    expect(onModelsChange).toHaveBeenCalledWith({
      中文模型: {
        name: "Kimi K2",
        limit: { context: 1048576, output: 131072 },
      },
    });
  });

  it("commits an unfinished model option key composition on its first blur", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({
      models: {
        "kimi-k2": {
          name: "Kimi K2",
          options: { provider: "baseten" },
        },
      },
      onModelsChange,
    });
    expandFirstModel();
    const optionKeyInput = screen.getByDisplayValue("provider");

    fireEvent.compositionStart(optionKeyInput);
    fireEvent.change(optionKeyInput, { target: { value: "路由" } });
    fireEvent.blur(optionKeyInput);

    expect(onModelsChange).toHaveBeenCalledTimes(1);
    expect(onModelsChange).toHaveBeenCalledWith({
      "kimi-k2": {
        name: "Kimi K2",
        options: { 路由: "baseten" },
      },
    });
  });

  it("reconciles a model option draft after JSON canonicalization", () => {
    const onModelsChange = vi.fn();
    const models = {
      "kimi-k2": {
        name: "Kimi K2",
        options: { provider: { order: ["baseten"] } },
      },
    };
    const { rerender, props } = renderOpenCodeForm({ models, onModelsChange });
    expandFirstModel();
    const optionValueInput = screen.getByDisplayValue('{"order":["baseten"]}');

    fireEvent.change(optionValueInput, {
      target: { value: '{ "order": ["baseten"] }' },
    });
    expect(onModelsChange).toHaveBeenCalledWith(models);

    // Parsing the edit and stringifying it again produces the same prop value
    // as before, so only the idle blur reconciliation can reset the draft.
    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} models={models} />
      </FormShell>,
    );
    expect(optionValueInput).toHaveValue('{ "order": ["baseten"] }');

    fireEvent.blur(optionValueInput);
    expect(optionValueInput).toHaveValue('{"order":["baseten"]}');
  });

  it("updates model token limits as structured numbers", () => {
    const onModelsChange = vi.fn();
    renderOpenCodeForm({ onModelsChange });

    expandFirstModel();
    fireEvent.change(screen.getByLabelText("Context"), {
      target: { value: "2000000" },
    });

    expect(onModelsChange).toHaveBeenCalledWith({
      "kimi-k2": {
        name: "Kimi K2",
        limit: { context: 2000000, output: 131072 },
      },
    });
  });

  it("removes model limit when both fields are cleared", () => {
    const onModelsChange = vi.fn();
    const { rerender, props } = renderOpenCodeForm({ onModelsChange });

    expandFirstModel();
    fireEvent.change(screen.getByLabelText("Context"), {
      target: { value: "" },
    });

    const withoutContext = {
      "kimi-k2": {
        name: "Kimi K2",
        limit: { output: 131072 },
      },
    };
    expect(onModelsChange).toHaveBeenLastCalledWith(withoutContext);

    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} models={withoutContext} />
      </FormShell>,
    );
    fireEvent.change(screen.getByLabelText("Output"), {
      target: { value: "" },
    });

    expect(onModelsChange).toHaveBeenLastCalledWith({
      "kimi-k2": {
        name: "Kimi K2",
      },
    });
  });
});
