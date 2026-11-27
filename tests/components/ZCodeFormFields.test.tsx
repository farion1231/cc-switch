import { fireEvent, screen, within } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import type { ComponentProps, PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ZCodeFormFields } from "@/components/providers/forms/ZCodeFormFields";
import { Form } from "@/components/ui/form";
import { fetchModelsForConfig } from "@/lib/api/model-fetch";

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/model-fetch")>()),
  fetchModelsForConfig: vi.fn(),
}));

type Props = ComponentProps<typeof ZCodeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const renderZCodeForm = (overrides: Partial<Props> = {}) => {
  const props: Props = {
    kind: "openai-compatible",
    onKindChange: vi.fn(),
    name: "",
    onNameChange: vi.fn(),
    apiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "custom",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    baseUrl: "https://api.example.com/v1",
    onBaseUrlChange: vi.fn(),
    speedTestEndpoints: [],
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    models: { "kept-model": {} },
    onModelsChange: vi.fn(),
    ...overrides,
  };
  render(
    <FormShell>
      <ZCodeFormFields {...props} />
    </FormShell>,
  );
  return props;
};

const FETCH = "providerForm.fetchModels";
const APPLY = "providerForm.modelPicker.apply";

describe("ZCodeFormFields model fetch", () => {
  beforeEach(() => {
    vi.mocked(fetchModelsForConfig).mockReset();
  });

  it("adds the models picked from the fetched list", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([
      { id: "glm-5.3", ownedBy: "zhipu" },
    ]);
    const props = renderZCodeForm();

    fireEvent.click(screen.getByRole("button", { name: FETCH }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "glm-5.3" }));
    fireEvent.click(within(dialog).getByRole("button", { name: APPLY }));

    // ZCode 的模型没有显示名字段：只按 ID 加一行，已有的模型保留
    expect(props.onModelsChange).toHaveBeenCalledWith({
      "kept-model": {},
      "glm-5.3": {},
    });
    expect(fetchModelsForConfig).toHaveBeenCalledWith(
      "https://api.example.com/v1",
      "sk-test",
      undefined,
      undefined,
      undefined,
      { apiFormat: undefined },
    );
  });

  it("uses Anthropic auth for the model list of an Anthropic provider", async () => {
    vi.mocked(fetchModelsForConfig).mockResolvedValue([]);
    renderZCodeForm({ kind: "anthropic" });

    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    await vi.waitFor(() =>
      expect(fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.example.com/v1",
        "sk-test",
        undefined,
        undefined,
        undefined,
        { apiFormat: "anthropic-messages" },
      ),
    );
  });

  it("does not fetch without a key", () => {
    renderZCodeForm({ apiKey: "" });

    fireEvent.click(screen.getByRole("button", { name: FETCH }));

    expect(fetchModelsForConfig).not.toHaveBeenCalled();
  });
});
