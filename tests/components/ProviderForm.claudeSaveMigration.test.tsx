import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

describe("ProviderForm Claude 保存路径旧后缀迁移", () => {
  it("打开带 deepseek[200k] 的配置不改直接保存 → 模型名干净且 contextWindows 物化", async () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="deepseek"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "DeepSeek",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
                ANTHROPIC_MODEL: "deepseek[200k]",
                ANTHROPIC_AUTH_TOKEN: "sk-test",
              },
            },
          }}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const payload = onSubmit.mock.calls[0][0];
    const parsed = JSON.parse(payload.settingsConfig) as {
      env: Record<string, unknown>;
      contextWindows?: Record<string, unknown>;
    };
    expect(parsed.env.ANTHROPIC_MODEL).toBe("deepseek");
    expect(parsed.contextWindows?.ANTHROPIC_MODEL).toBe(200000);
  });
});
