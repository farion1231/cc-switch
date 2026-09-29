import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { Form } from "@/components/ui/form";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));

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
  it("shows fetched models even when no models are configured", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "vendor/model-a", ownedBy: "vendor" },
    ]);
    const { props, rerender } = renderOpenCodeForm({ models: {} });

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    const checkbox = await screen.findByRole("checkbox", {
      name: "vendor/model-a",
    });
    expect(checkbox).not.toBeChecked();
    expect(props.onModelsChange).not.toHaveBeenCalled();
    expect(
      screen.getByRole("button", { name: "Add selected (0)" }),
    ).toBeDisabled();

    fireEvent.click(checkbox);
    fireEvent.click(screen.getByRole("button", { name: "Add selected (1)" }));

    expect(props.onModelsChange).toHaveBeenCalledWith({
      "vendor/model-a": { name: "vendor/model-a" },
    });
    rerender(
      <FormShell>
        <OpenCodeFormFields
          {...props}
          models={{ "vendor/model-a": { name: "vendor/model-a" } }}
        />
      </FormShell>,
    );
    expect(
      screen.getByRole("checkbox", { name: "vendor/model-a" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Add selected (0)" }),
    ).toBeDisabled();
  });

  it("adds only selected models across searches and preserves existing configuration", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "kimi-k2", ownedBy: "moonshot" },
      { id: "model-a", ownedBy: "vendor-a" },
      { id: "model-b", ownedBy: "vendor-b" },
      { id: "model-c", ownedBy: "vendor-b" },
      { id: "model-a", ownedBy: "vendor-a" },
    ]);
    const { props } = renderOpenCodeForm();
    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    const existing = await screen.findByRole("checkbox", { name: "kimi-k2" });
    expect(existing).toBeDisabled();
    expect(existing).toBeChecked();

    const search = screen.getByRole("textbox", { name: "Search models..." });
    fireEvent.change(search, { target: { value: "VENDOR-A" } });
    expect(
      screen.queryByRole("checkbox", { name: "model-b" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "model-a" }));
    fireEvent.change(search, { target: { value: "model-b" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "model-b" }));
    fireEvent.click(screen.getByRole("button", { name: "Add selected (2)" }));

    expect(props.onModelsChange).toHaveBeenCalledTimes(1);
    expect(props.onModelsChange).toHaveBeenCalledWith({
      ...props.models,
      "model-a": { name: "model-a" },
      "model-b": { name: "model-b" },
    });
  });

  it("does not submit the provider form when Enter is pressed in the model search", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "model-a", ownedBy: "vendor" },
    ]);
    const onSubmit = vi.fn((event: { preventDefault: () => void }) =>
      event.preventDefault(),
    );
    const { props, rerender } = renderOpenCodeForm();
    // user-event only finds submit buttons inside the form when simulating
    // implicit submission, so keep the save button inside this test form.
    rerender(
      <FormShell>
        <form onSubmit={onSubmit}>
          <OpenCodeFormFields {...props} />
          <button type="submit">save</button>
        </form>
      </FormShell>,
    );
    const user = userEvent.setup();

    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(await screen.findByRole("checkbox", { name: "model-a" }));
    await user.type(
      screen.getByRole("textbox", { name: "Search models..." }),
      "model{Enter}",
    );

    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("checkbox", { name: "model-a" })).toBeChecked();
    expect(props.onModelsChange).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "save" }));
    expect(onSubmit).toHaveBeenCalledTimes(1);

    await user.type(
      screen.getByRole("textbox", { name: "Base URL" }),
      "{Enter}",
    );
    expect(onSubmit).toHaveBeenCalledTimes(2);
  });

  it.each(["baseUrl", "apiKey"] as const)(
    "clears fetched models and pending selections when %s changes",
    async (field) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "old-model", ownedBy: null },
      ]);
      const { props, rerender } = renderOpenCodeForm();
      fireEvent.click(
        screen.getByRole("button", { name: "providerForm.fetchModels" }),
      );
      fireEvent.click(
        await screen.findByRole("checkbox", { name: "old-model" }),
      );

      rerender(
        <FormShell>
          <OpenCodeFormFields
            {...props}
            {...{ [field]: `${props[field]}-changed` }}
          />
        </FormShell>,
      );

      expect(
        screen.queryByRole("checkbox", { name: "old-model" }),
      ).not.toBeInTheDocument();
      expect(props.onModelsChange).not.toHaveBeenCalled();
      fireEvent.click(
        screen.getByRole("button", { name: "providerForm.fetchModels" }),
      );
      expect(
        await screen.findByRole("checkbox", { name: "old-model" }),
      ).not.toBeChecked();
    },
  );

  it("ignores a stale response while fetching models for a new endpoint", async () => {
    let resolveOld!: (
      models: Awaited<ReturnType<typeof fetchModelsForConfig>>,
    ) => void;
    let resolveNew!: (
      models: Awaited<ReturnType<typeof fetchModelsForConfig>>,
    ) => void;
    vi.mocked(fetchModelsForConfig)
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveOld = resolve;
          }),
      )
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveNew = resolve;
          }),
      );
    const { props, rerender } = renderOpenCodeForm();
    const fetchButton = screen.getByRole("button", {
      name: "providerForm.fetchModels",
    });
    fireEvent.click(fetchButton);
    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} baseUrl="https://new.example.com/v1" />
      </FormShell>,
    );
    fireEvent.click(fetchButton);

    await act(async () => {
      resolveOld([{ id: "old-model", ownedBy: null }]);
    });
    expect(
      screen.queryByRole("checkbox", { name: "old-model" }),
    ).not.toBeInTheDocument();
    expect(fetchButton).toBeDisabled();

    await act(async () => {
      resolveNew([{ id: "new-model", ownedBy: null }]);
    });
    expect(
      await screen.findByRole("checkbox", { name: "new-model" }),
    ).toBeEnabled();
    expect(fetchButton).toBeEnabled();
    expect(props.onModelsChange).not.toHaveBeenCalled();
  });

  it.each(["empty", "failure"])(
    "removes previous choices after an %s fetch without changing configured models",
    async (result) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
        { id: "old-model", ownedBy: null },
      ]);
      const { props } = renderOpenCodeForm();
      const fetchButton = screen.getByRole("button", {
        name: "providerForm.fetchModels",
      });
      fireEvent.click(fetchButton);
      fireEvent.click(
        await screen.findByRole("checkbox", { name: "old-model" }),
      );
      if (result === "empty") {
        vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([]);
      } else {
        vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
          new Error("HTTP 500"),
        );
      }
      fireEvent.click(fetchButton);
      await waitFor(() => expect(fetchButton).toBeEnabled());

      expect(
        screen.queryByRole("checkbox", { name: "old-model" }),
      ).not.toBeInTheDocument();
      expect(screen.getByDisplayValue("Kimi K2")).toBeInTheDocument();
      expect(props.onModelsChange).not.toHaveBeenCalled();
    },
  );

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
