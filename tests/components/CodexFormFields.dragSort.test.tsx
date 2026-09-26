import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FormProvider, useForm } from "react-hook-form";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import type { CodexCatalogModel } from "@/types";

function TestWrapper({ children }: { children: React.ReactNode }) {
  const form = useForm();
  return <FormProvider {...form}>{children}</FormProvider>;
}

describe("CodexFormFields drag sort", () => {
  const sampleModels: CodexCatalogModel[] = [
    {
      model: "deepseek-v4-flash",
      displayName: "DeepSeek Flash",
      contextWindow: 128000,
      reasoningLevels: ["low", "high"],
      defaultReasoningLevel: "high",
    },
    {
      model: "kimi-k2.5",
      displayName: "Kimi K2.5",
      contextWindow: 256000,
    },
    {
      model: "qwen-max",
      displayName: "Qwen Max",
    },
  ];

  const defaultProps = {
    codexApiKey: "test-key",
    onApiKeyChange: vi.fn(),
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    codexEndpoint: "https://api.example.com/v1",
    onEndpointChange: vi.fn(),
    codexModel: "deepseek-v4-flash",
    onModelChange: vi.fn(),
    catalogModels: sampleModels,
    onCatalogModelsChange: vi.fn(),
    codexApiFormat: "openai_responses" as const,
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
  };

  it("renders drag handle for each catalog model row", () => {
    render(
      <TestWrapper>
        <CodexFormFields {...(defaultProps as any)} />
      </TestWrapper>,
    );

    // Check that drag handles are present
    const dragHandles = screen.getAllByRole("button", {
      name: /拖拽排序|Drag to reorder/i,
    });
    expect(dragHandles).toHaveLength(3);

    // Check that all model inputs are displayed
    expect(screen.getByDisplayValue("DeepSeek Flash")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Kimi K2.5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Qwen Max")).toBeInTheDocument();
  });

  it("renders column headers aligned with drag handle column", () => {
    const { container } = render(
      <TestWrapper>
        <CodexFormFields {...(defaultProps as any)} />
      </TestWrapper>,
    );

    // Header grid should specify the 28px drag handle column
    const headerGrid = container.querySelector(
      ".grid-cols-\\[28px_1fr_1fr_140px_1fr_36px\\]",
    );
    expect(headerGrid).toBeInTheDocument();
  });

  it("handles updating row fields and notifies parent", async () => {
    const onCatalogModelsChange = vi.fn();

    render(
      <TestWrapper>
        <CodexFormFields
          {...(defaultProps as any)}
          onCatalogModelsChange={onCatalogModelsChange}
        />
      </TestWrapper>,
    );

    const inputs = screen.getAllByRole("textbox", { name: "菜单显示名" });
    expect(inputs).toHaveLength(3);

    // Initial sync might call onCatalogModelsChange with current models
    onCatalogModelsChange.mockClear();

    fireEvent.change(inputs[0], { target: { value: "DeepSeek V4 Pro" } });

    await waitFor(() => {
      expect(onCatalogModelsChange).toHaveBeenCalledWith([
        expect.objectContaining({
          model: "deepseek-v4-flash",
          displayName: "DeepSeek V4 Pro",
        }),
        expect.objectContaining({
          model: "kimi-k2.5",
          displayName: "Kimi K2.5",
        }),
        expect.objectContaining({
          model: "qwen-max",
          displayName: "Qwen Max",
        }),
      ]);
    });
  });

  it("handles removing a row and notifies parent", async () => {
    const onCatalogModelsChange = vi.fn();

    render(
      <TestWrapper>
        <CodexFormFields
          {...(defaultProps as any)}
          onCatalogModelsChange={onCatalogModelsChange}
        />
      </TestWrapper>,
    );

    onCatalogModelsChange.mockClear();

    const deleteButtons = screen.getAllByRole("button", {
      name: /删除|Delete/i,
    });
    expect(deleteButtons).toHaveLength(3);

    fireEvent.click(deleteButtons[1]); // Delete Kimi K2.5

    await waitFor(() => {
      expect(onCatalogModelsChange).toHaveBeenCalledWith([
        expect.objectContaining({
          model: "deepseek-v4-flash",
        }),
        expect.objectContaining({
          model: "qwen-max",
        }),
      ]);
    });
  });
});
