import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
  info: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: toastMocks,
}));

import { AggregateProviderForm } from "@/components/providers/forms/AggregateProviderForm";
import type { ProviderFormValues } from "@/components/providers/forms/ProviderForm";

function targetProvider(id: string, name: string): Provider {
  return {
    id,
    name,
    settingsConfig: {
      env: {
        ANTHROPIC_BASE_URL: "https://api.example.com",
        ANTHROPIC_API_KEY: "sk-test",
      },
    },
  };
}

function fillName(value: string) {
  fireEvent.change(screen.getByPlaceholderText("provider.namePlaceholder"), {
    target: { value },
  });
}

describe("AggregateProviderForm", () => {
  beforeEach(() => {
    toastMocks.error.mockClear();
  });

  it("提交占位 settingsConfig、归一化路由与 presetCategory=custom", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <AggregateProviderForm
        appId="claude"
        submitLabel="save"
        onSubmit={handleSubmit}
        onCancel={vi.fn()}
        availableProviders={[targetProvider("kimi", "Kimi")]}
        initialData={{
          name: "Agg",
          meta: {
            aggregateRoutes: {
              sonnet: { providerId: "kimi", model: " k3 " },
              // 完全未填写的档位应被归一化丢弃
              opus: { providerId: "", model: "" },
            },
          },
        }}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() => expect(handleSubmit).toHaveBeenCalledTimes(1));
    const values = handleSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(values.name).toBe("Agg");
    expect(JSON.parse(values.settingsConfig)).toEqual({});
    expect(values.presetCategory).toBe("custom");
    expect(values.meta?.aggregateRoutes).toEqual({
      sonnet: { providerId: "kimi", model: "k3" },
    });
    // 普通供应商的 meta 字段应被剥离
    expect(values.meta?.providerType).toBeUndefined();
    expect(values.meta?.endpointAutoSelect).toBeUndefined();
  });

  it("名称为空时阻止提交并提示填写供应商名称", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <AggregateProviderForm
        appId="claude"
        submitLabel="save"
        onSubmit={handleSubmit}
        onCancel={vi.fn()}
        availableProviders={[targetProvider("kimi", "Kimi")]}
        initialData={{
          meta: {
            aggregateRoutes: {
              sonnet: { providerId: "kimi", model: "k3" },
            },
          },
        }}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        expect.stringContaining("请填写供应商名称"),
      ),
    );
    expect(handleSubmit).not.toHaveBeenCalled();
  });

  it("路由表为空时阻止提交并提示配置聚合路由", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <AggregateProviderForm
        appId="claude"
        submitLabel="save"
        onSubmit={handleSubmit}
        onCancel={vi.fn()}
        availableProviders={[targetProvider("kimi", "Kimi")]}
      />,
    );

    fillName("Agg");
    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        expect.stringContaining("Configure at least one aggregate route."),
      ),
    );
    expect(handleSubmit).not.toHaveBeenCalled();
  });

  it("codex 重复请求模型名时阻止提交", async () => {
    const handleSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <AggregateProviderForm
        appId="codex"
        submitLabel="save"
        onSubmit={handleSubmit}
        onCancel={vi.fn()}
        availableProviders={[targetProvider("kimi", "Kimi")]}
        initialData={{
          name: "Agg",
          meta: {
            aggregateRoutes: {
              custom: {
                "gpt-5.5": { providerId: "kimi", model: "k2" },
              },
            },
          },
        }}
      />,
    );

    // 在行态中追加一个重复 key 的行（Record 无法表达，故通过 UI 行操作制造）
    fireEvent.click(screen.getByRole("button", { name: /add route/i }));
    const keyInput = document.getElementById(
      "aggregate-custom-1-key",
    ) as HTMLInputElement;
    fireEvent.change(keyInput, { target: { value: "gpt-5.5" } });

    fireEvent.click(screen.getByRole("button", { name: "save" }));

    await waitFor(() =>
      expect(toastMocks.error).toHaveBeenCalledWith(
        expect.stringContaining("Duplicate model name: gpt-5.5"),
      ),
    );
    expect(handleSubmit).not.toHaveBeenCalled();
  });
});
