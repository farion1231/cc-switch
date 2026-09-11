import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

describe("ProviderForm preset/template reset", () => {
  it("选择 Claude preset 会重置表单字段（spec §2.2/§7）", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    const getNameInput = () =>
      container.querySelector<HTMLInputElement>('input[name="name"]');

    // 新增模式初始 name 为空
    expect(getNameInput()).toHaveValue("");

    // 点击第一个 Claude 官方 preset，表单应重置为该 preset
    const presetButton = await screen.findByRole("button", {
      name: /Claude Official/i,
    });
    fireEvent.click(presetButton);

    await waitFor(() => {
      expect(getNameInput()).toHaveValue("Claude Official");
    });
  });
});
