import type { ComponentProps } from "react";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { FormProvider, useForm } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import {
  CodexFormFields,
  reorderCatalogRows,
} from "@/components/providers/forms/CodexFormFields";

vi.mock("@/hooks/useModelMetadataFill", () => ({
  useModelMetadataFill: () => () => {},
}));

type Props = ComponentProps<typeof CodexFormFields>;

function makeProps(overrides: Partial<Props> = {}): Props {
  return {
    codexApiKey: "test-key",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    shouldShowSpeedTest: false,
    codexBaseUrl: "https://example.com/v1",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "model-1",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    catalogModels: [],
    onCatalogModelsChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };
}

function Harness(props: Props) {
  const form = useForm();
  return (
    <FormProvider {...form}>
      <CodexFormFields {...props} />
    </FormProvider>
  );
}

const DRAG_HANDLE = /^(provider\.dragHandle|拖拽排序)$/;

const rows = (...models: string[]) =>
  models.map((model, i) => ({ rowId: `r${i}`, model }));

describe("reorderCatalogRows", () => {
  it("makes the row moved to the top the new default model", () => {
    const result = reorderCatalogRows(rows("a", "b", "c"), "r2", "r0", "a");
    expect(result?.rows.map((row) => row.model)).toEqual(["c", "a", "b"]);
    expect(result?.defaultModel).toBe("c");
  });

  it("keeps the default when the first row stays in place", () => {
    const result = reorderCatalogRows(rows("a", "b", "c"), "r2", "r1", "a");
    expect(result?.rows.map((row) => row.model)).toEqual(["a", "c", "b"]);
    expect(result?.defaultModel).toBeUndefined();
  });

  it("skips a blank row moved to the top, like removing a row does", () => {
    const result = reorderCatalogRows(rows("a", "b", ""), "r2", "r0", "a");
    expect(result?.rows.map((row) => row.model)).toEqual(["", "a", "b"]);
    expect(result?.defaultModel).toBeUndefined();
    const moved = reorderCatalogRows(rows("a", "", "c"), "r0", "r2", "a");
    expect(moved?.rows.map((row) => row.model)).toEqual(["", "c", "a"]);
    expect(moved?.defaultModel).toBe("c");
  });

  it("leaves the default alone when the first row does not stand for it", () => {
    const result = reorderCatalogRows(rows("a", "b"), "r1", "r0", null);
    expect(result?.rows.map((row) => row.model)).toEqual(["b", "a"]);
    expect(result?.defaultModel).toBeUndefined();
  });

  it("returns null for a drop on itself or an unknown row", () => {
    expect(reorderCatalogRows(rows("a", "b"), "r0", "r0", "a")).toBeNull();
    expect(reorderCatalogRows(rows("a", "b"), "r0", "x", "a")).toBeNull();
  });
});

describe("Codex model list drag handles", () => {
  beforeEach(() => {
    // jsdom 没有布局：按在父元素里的位置给每个元素一个竖排的矩形，键盘拖动才找得到下一行。
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(
      function (this: Element) {
        const siblings = this.parentElement
          ? Array.from(this.parentElement.children)
          : [];
        const top = Math.max(siblings.indexOf(this), 0) * 50;
        return {
          x: 0,
          y: top,
          top,
          left: 0,
          right: 600,
          bottom: top + 40,
          width: 600,
          height: 40,
          toJSON: () => ({}),
        } as DOMRect;
      },
    );
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("hides the handle while there is only one row", () => {
    render(
      <Harness {...makeProps({ catalogModels: [{ model: "model-1" }] })} />,
    );
    expect(screen.getByRole("button", { name: DRAG_HANDLE })).toBeDisabled();
  });

  it("moves a row to the top with the keyboard and makes it the default", async () => {
    const props = makeProps({
      catalogModels: [{ model: "model-1" }, { model: "model-2" }],
    });
    render(<Harness {...props} />);
    const handles = screen.getAllByRole("button", {
      name: DRAG_HANDLE,
    });
    expect(handles).toHaveLength(2);

    const second = handles[1];
    second.focus();
    // KeyboardSensor 在下一个 tick 才开始听方向键，量位置也是异步的：每一步之后等一下。
    const press = async (code: string, key: string) => {
      await act(async () => {
        fireEvent.keyDown(second, { code, key });
        await new Promise((resolve) => setTimeout(resolve, 20));
      });
    };
    await press("Space", " ");
    await press("ArrowUp", "ArrowUp");
    await press("Space", " ");

    expect(props.onModelChange).toHaveBeenCalledWith("model-2");
    await waitFor(() =>
      expect(props.onCatalogModelsChange).toHaveBeenLastCalledWith([
        expect.objectContaining({ model: "model-2" }),
        expect.objectContaining({ model: "model-1" }),
      ]),
    );
  });
});
