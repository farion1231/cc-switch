import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CursorEndpointDialog } from "@/components/cursor/CursorEndpointDialog";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        {children}
        {footer}
      </div>
    ) : null,
}));

describe("CursorEndpointDialog", () => {
  beforeEach(() => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([
      {
        id: "cx/gpt-5.6-terra",
        ownedBy: "cx",
        contextWindowTokens: 272_000,
      },
    ]);
  });

  it("批量添加时保留提供商返回的上下文长度", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);

    render(
      <CursorEndpointDialog
        open
        endpoint={null}
        providers={[]}
        onOpenChange={vi.fn()}
        onSave={onSave}
      />,
    );

    fireEvent.change(screen.getByPlaceholderText("例如 OpenRouter"), {
      target: { value: "Local Gateway" },
    });
    fireEvent.change(screen.getByPlaceholderText("https://api.example.com"), {
      target: { value: "http://127.0.0.1:20128/v1" },
    });
    fireEvent.change(screen.getByPlaceholderText("sk-..."), {
      target: { value: "test-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "获取模型" }));

    await waitFor(() =>
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalled(),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "添加选中项（1）" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "添加 Endpoint" }));

    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].upserts[0].settingsConfig).toMatchObject({
      modelID: "cx/gpt-5.6-terra",
      contextWindowTokens: 272_000,
    });
  });
});
