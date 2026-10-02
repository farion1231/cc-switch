import { fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { useState, type ComponentProps, type PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";
import { HermesFormFields } from "@/components/providers/forms/HermesFormFields";
import type { HermesModel } from "@/config/hermesProviderPresets";
import { Form } from "@/components/ui/form";
import { MODELS_DEV_API_URL } from "@/lib/modelsDev";
import { server } from "../msw/server";

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
  it("fills the context length of a model picked from the fetched list", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    server.use(
      http.post("http://tauri.local/fetch_models_for_config", () =>
        HttpResponse.json([{ id: "model-x", ownedBy: "example" }]),
      ),
      http.get(MODELS_DEV_API_URL, () =>
        HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: { "model-x": { limit: { context: 123456 } } },
          },
        }),
      ),
    );
    const onModelsChange = vi.fn();
    const { props } = renderHermesForm({ onModelsChange });

    // 拉模型列表时就开始预取 models.dev，选中时一次提交就带上参数。
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(
      (await screen.findAllByRole("button", { name: "Select model" }))[0],
    );
    await user.click(await screen.findByRole("option", { name: "model-x" }));

    await waitFor(() =>
      expect(onModelsChange).toHaveBeenLastCalledWith([
        { ...props.models[0], id: "model-x", context_length: 123456 },
        props.models[1],
      ]),
    );
  });

  it("fills the picked row even after rows above it were removed", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    let releaseModelsDev!: () => void;
    const modelsDevReady = new Promise<void>((resolve) => {
      releaseModelsDev = resolve;
    });
    server.use(
      http.post("http://tauri.local/fetch_models_for_config", () =>
        HttpResponse.json([{ id: "model-x", ownedBy: "example" }]),
      ),
      http.get(MODELS_DEV_API_URL, async () => {
        await modelsDevReady;
        return HttpResponse.json({
          example: {
            api: "https://api.example.com/v1",
            models: { "model-x": { limit: { context: 123456 } } },
          },
        });
      }),
    );
    const committed = vi.fn();
    const Stateful = () => {
      const [models, setModels] = useState<HermesModel[]>([
        { id: "model-a", name: "Model A" },
        { id: "model-b", name: "Model B" },
      ]);
      return (
        <FormShell>
          <HermesFormFields
            baseUrl="https://api.example.com/v1"
            onBaseUrlChange={vi.fn()}
            apiKey="sk-test"
            onApiKeyChange={vi.fn()}
            category="custom"
            shouldShowApiKeyLink={false}
            websiteUrl=""
            apiMode="chat_completions"
            onApiModeChange={vi.fn()}
            models={models}
            onModelsChange={(next) => {
              committed(next);
              setModels(next);
            }}
            rateLimitDelay={0.5}
            onRateLimitDelayChange={vi.fn()}
          />
        </FormShell>
      );
    };
    render(<Stateful />);

    // 给第二行选模型，models.dev 还没返回时删掉第一行，第二行挪到最上面。
    await user.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await user.click(
      (await screen.findAllByRole("button", { name: "Select model" }))[1],
    );
    await user.click(await screen.findByRole("option", { name: "model-x" }));
    await user.click(screen.getAllByRole("button", { name: "移除模型" })[0]);
    releaseModelsDev();

    await waitFor(() =>
      expect(committed).toHaveBeenLastCalledWith([
        { id: "model-x", name: "Model B", context_length: 123456 },
      ]),
    );
  });

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
