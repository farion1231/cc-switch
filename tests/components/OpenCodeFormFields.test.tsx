import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import userEvent from "@testing-library/user-event";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { OpenCodeFormFields } from "@/components/providers/forms/OpenCodeFormFields";
import { Form } from "@/components/ui/form";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";
import type { PresetModelSource } from "@/lib/modelMetadata";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import { server } from "../msw/server";

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

const FETCH = "providerForm.fetchModels";
const PICK = "providerForm.modelPicker.title";
const APPLY = "providerForm.modelPicker.apply";
const REFRESH = "providerForm.modelPicker.refresh";
const SEARCH = "providerForm.searchModelPlaceholder";
const CANCEL = "common.cancel";

const expandFirstModel = () => {
  fireEvent.click(screen.getByRole("button", { name: "Toggle model details" }));
};

describe("OpenCodeFormFields", () => {
  it("opens the picker after fetching and adds the checked models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "vendor/model-a", ownedBy: "vendor" },
    ]);
    const { props } = renderOpenCodeForm({ models: {} });

    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    const dialog = await screen.findByRole("dialog");
    const checkbox = within(dialog).getByRole("checkbox", {
      name: "vendor/model-a",
    });
    expect(checkbox).not.toBeChecked();
    fireEvent.click(checkbox);
    fireEvent.click(within(dialog).getByRole("button", { name: APPLY }));

    expect(props.onModelsChange).toHaveBeenCalledWith({
      "vendor/model-a": { name: "vendor/model-a" },
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // 拉过之后按钮直接打开弹窗，不再请求
    fireEvent.click(screen.getByRole("button", { name: PICK }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(fetchModelsForConfig).toHaveBeenCalledTimes(1);
  });

  it("shows each model's window and capabilities and fills picked models quietly", async () => {
    server.use(http.get(MODELS_DEV_API_URL, () => HttpResponse.error()));
    const toastInfo = vi.spyOn(toast, "info");
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "model-a", ownedBy: null },
      { id: "model-b", ownedBy: null },
    ]);
    const presetModelSources = (): PresetModelSource[] => [
      {
        baseUrl: "https://api.example.com/v1",
        models: new Map([
          [
            "model-a",
            {
              contextWindow: 262144,
              reasoning: true,
              inputModalities: ["text", "image"],
            },
          ],
        ]),
      },
    ];
    const { props } = renderOpenCodeForm({ models: {}, presetModelSources });
    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(
        "256K · providerForm.modelPicker.reasoning · providerForm.modelPicker.image",
      ),
    ).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "model-a" }));
    fireEvent.click(within(dialog).getByRole("button", { name: APPLY }));

    expect(props.onModelsChange).toHaveBeenLastCalledWith({
      "model-a": expect.objectContaining({
        limit: expect.objectContaining({ context: 262144 }),
      }),
    });
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(toastInfo).not.toHaveBeenCalled();
    toastInfo.mockRestore();
  });

  it("checks configured models and removes the ones unchecked on apply", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "kimi-k2", ownedBy: "moonshot" },
      { id: "model-a", ownedBy: "vendor-a" },
      { id: "model-b", ownedBy: "vendor-b" },
      { id: "model-c", ownedBy: "vendor-b" },
      { id: "model-a", ownedBy: "vendor-a" },
    ]);
    const { props } = renderOpenCodeForm({
      models: {
        "kimi-k2": {
          name: "Kimi K2",
          limit: { context: 1048576, output: 131072 },
        },
        "custom-model": { name: "Custom" },
      },
    });
    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    const dialog = await screen.findByRole("dialog");
    const existing = within(dialog).getByRole("checkbox", { name: "kimi-k2" });
    expect(existing).toBeChecked();
    expect(existing).toBeEnabled();
    // 配置里有、供应商没返回的模型单独列出，也能取消
    expect(
      within(dialog).getByText("providerForm.modelPicker.notInProvider"),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("checkbox", { name: "custom-model" }),
    ).toBeChecked();

    const search = within(dialog).getByRole("textbox", { name: SEARCH });
    fireEvent.change(search, { target: { value: "VENDOR-A" } });
    expect(
      within(dialog).queryByRole("checkbox", { name: "model-b" }),
    ).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "model-a" }));
    fireEvent.change(search, { target: { value: "model-b" } });
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "model-b" }));
    fireEvent.change(search, { target: { value: "" } });
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "kimi-k2" }));
    fireEvent.click(within(dialog).getByRole("button", { name: APPLY }));

    expect(props.onModelsChange).toHaveBeenCalledTimes(1);
    expect(props.onModelsChange).toHaveBeenCalledWith({
      "custom-model": { name: "Custom" },
      "model-a": { name: "model-a" },
      "model-b": { name: "model-b" },
    });
  });

  it("discards unchecked and checked models when the picker is cancelled", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "kimi-k2", ownedBy: null },
      { id: "model-a", ownedBy: null },
    ]);
    const { props } = renderOpenCodeForm();
    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    let dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "kimi-k2" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "model-a" }));
    fireEvent.click(within(dialog).getByRole("button", { name: CANCEL }));
    expect(props.onModelsChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: PICK }));
    dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("checkbox", { name: "kimi-k2" }),
    ).toBeChecked();
    expect(
      within(dialog).getByRole("checkbox", { name: "model-a" }),
    ).not.toBeChecked();
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

    await user.click(screen.getByRole("button", { name: FETCH }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("checkbox", { name: "model-a" }));
    await user.type(
      within(dialog).getByRole("textbox", { name: SEARCH }),
      "model{Enter}",
    );

    expect(onSubmit).not.toHaveBeenCalled();
    expect(
      within(dialog).getByRole("checkbox", { name: "model-a" }),
    ).toBeChecked();
    expect(props.onModelsChange).not.toHaveBeenCalled();

    await user.click(within(dialog).getByRole("button", { name: CANCEL }));
    await user.click(screen.getByRole("button", { name: "save" }));
    expect(onSubmit).toHaveBeenCalledTimes(1);

    await user.type(
      screen.getByRole("textbox", { name: "Base URL" }),
      "{Enter}",
    );
    expect(onSubmit).toHaveBeenCalledTimes(2);
  });

  it.each(["baseUrl", "apiKey"] as const)(
    "clears fetched models when %s changes",
    async (field) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValue([
        { id: "old-model", ownedBy: null },
      ]);
      const { props, rerender } = renderOpenCodeForm();
      fireEvent.click(screen.getByRole("button", { name: FETCH }));
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(within(dialog).getByRole("button", { name: CANCEL }));

      rerender(
        <FormShell>
          <OpenCodeFormFields
            {...props}
            {...{ [field]: `${props[field]}-changed` }}
          />
        </FormShell>,
      );

      expect(
        screen.queryByRole("button", { name: PICK }),
      ).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: FETCH }));
      expect(fetchModelsForConfig).toHaveBeenCalledTimes(2);
      expect(
        within(await screen.findByRole("dialog")).getByRole("checkbox", {
          name: "old-model",
        }),
      ).not.toBeChecked();
      expect(props.onModelsChange).not.toHaveBeenCalled();
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
    const fetchButton = screen.getByRole("button", { name: FETCH });
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
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(fetchButton).toBeDisabled();

    await act(async () => {
      resolveNew([{ id: "new-model", ownedBy: null }]);
    });
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("checkbox", { name: "new-model" }),
    ).toBeEnabled();
    expect(
      within(dialog).queryByRole("checkbox", { name: "old-model" }),
    ).not.toBeInTheDocument();
    expect(props.onModelsChange).not.toHaveBeenCalled();
  });

  it("sends the configured provider headers when fetching models", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    renderOpenCodeForm({
      headers: {
        "cf-aig-authorization": "Bearer gateway-key",
        "draft-header:1700000000000": "unfinished",
      },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "sk-test",
      undefined,
      undefined,
      undefined,
      {
        requestHeaders: { "cf-aig-authorization": "Bearer gateway-key" },
      },
    );
  });

  it("passes a User-Agent provider header as the model fetch user agent", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    renderOpenCodeForm({
      headers: { "User-Agent": "cc-switch-test" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "sk-test",
      undefined,
      undefined,
      "cc-switch-test",
      { requestHeaders: { "User-Agent": "cc-switch-test" } },
    );
  });

  it("fetches models with headers alone when the API key is empty", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    renderOpenCodeForm({
      apiKey: "",
      headers: { "cf-aig-authorization": "Bearer gateway-key" },
    });

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() => expect(fetchModelsForConfig).toHaveBeenCalledTimes(1));
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "",
      undefined,
      undefined,
      undefined,
      {
        requestHeaders: { "cf-aig-authorization": "Bearer gateway-key" },
      },
    );
  });

  it("clears fetched models when headers change", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "old-model", ownedBy: null },
    ]);
    const { props, rerender } = renderOpenCodeForm({
      headers: { "X-Title": "CC Switch" },
    });
    fireEvent.click(screen.getByRole("button", { name: FETCH }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: CANCEL }));

    rerender(
      <FormShell>
        <OpenCodeFormFields {...props} headers={{ "X-Title": "OpenCode" }} />
      </FormShell>,
    );

    expect(
      screen.queryByRole("button", { name: PICK }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: FETCH })).toBeEnabled();
    expect(props.onModelsChange).not.toHaveBeenCalled();
  });

  it.each(["empty", "failure"])(
    "drops the previous list after an %s refresh without changing configured models",
    async (result) => {
      vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([
        { id: "old-model", ownedBy: null },
      ]);
      const { props } = renderOpenCodeForm();
      fireEvent.click(screen.getByRole("button", { name: FETCH }));
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("checkbox", { name: "old-model" }),
      );
      if (result === "empty") {
        vi.mocked(fetchModelsForConfig).mockResolvedValueOnce([]);
      } else {
        vi.mocked(fetchModelsForConfig).mockRejectedValueOnce(
          new Error("HTTP 500"),
        );
      }
      const refresh = within(dialog).getByRole("button", { name: REFRESH });
      fireEvent.click(refresh);
      await waitFor(() => expect(refresh).toBeEnabled());

      expect(
        within(dialog).queryByRole("checkbox", { name: "old-model" }),
      ).not.toBeInTheDocument();
      expect(
        within(dialog).getByRole("checkbox", { name: "kimi-k2" }),
      ).toBeChecked();
      fireEvent.click(within(dialog).getByRole("button", { name: APPLY }));
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

  it("rejects renaming an extra option onto an existing key and restores the input", () => {
    const onExtraOptionsChange = vi.fn();
    renderOpenCodeForm({
      extraOptions: { target: "100", source: "100" },
      onExtraOptionsChange,
    });

    const keyInput = screen.getByDisplayValue("source");
    fireEvent.change(keyInput, { target: { value: "target" } });
    fireEvent.blur(keyInput);

    expect(onExtraOptionsChange).not.toHaveBeenCalled();
    expect(keyInput).toHaveValue("source");
  });

  it("uses the family section divider for model configuration", () => {
    renderOpenCodeForm();

    const section = screen.getByText("Models").closest("div.border-l");
    expect(section).toHaveClass("border-border", "pl-3");
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
