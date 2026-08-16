import { fireEvent, render } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { createTestQueryClient } from "../utils/testQueryClient";

describe("ProviderForm initializedRef", () => {
  it("首次渲染使用 initialData，后续 initialData 变化不再 reset 用户编辑", () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    const { container, rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="provider-a"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "DB Provider",
            settingsConfig: {
              env: {
                ANTHROPIC_AUTH_TOKEN: "db-token",
              },
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    const getNameInput = () =>
      container.querySelector<HTMLInputElement>('input[name="name"]');

    expect(getNameInput()).toHaveValue("DB Provider");
    fireEvent.change(getNameInput()!, { target: { value: "Edited Provider" } });
    expect(getNameInput()).toHaveValue("Edited Provider");

    rerender(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="provider-a"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Live Provider",
            settingsConfig: {
              env: {
                ANTHROPIC_AUTH_TOKEN: "live-token",
              },
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    expect(getNameInput()).toHaveValue("Edited Provider");
  });

  it("首次渲染 reset 一次，provider scope 变化不再 reset 用户编辑", () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    const { container, rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="provider-a"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "DB Provider A",
            settingsConfig: {
              env: {
                ANTHROPIC_AUTH_TOKEN: "db-token-a",
              },
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    const getNameInput = () =>
      container.querySelector<HTMLInputElement>('input[name="name"]');

    expect(getNameInput()).toHaveValue("DB Provider A");
    fireEvent.change(getNameInput()!, { target: { value: "Edited Provider" } });
    expect(getNameInput()).toHaveValue("Edited Provider");

    rerender(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="provider-b"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Live Provider B",
            settingsConfig: {
              env: {
                ANTHROPIC_AUTH_TOKEN: "live-token-b",
              },
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    expect(getNameInput()).toHaveValue("Edited Provider");
  });

  it("首次渲染 reset 一次，appId 变化不再 reset 用户编辑", () => {
    const queryClient = createTestQueryClient();
    const onSubmit = vi.fn();
    const { container, rerender } = render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="claude"
          providerId="provider-a"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "DB Provider A",
            settingsConfig: {
              env: {
                ANTHROPIC_AUTH_TOKEN: "db-token-a",
              },
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    const getNameInput = () =>
      container.querySelector<HTMLInputElement>('input[name="name"]');

    expect(getNameInput()).toHaveValue("DB Provider A");
    fireEvent.change(getNameInput()!, { target: { value: "Edited Provider" } });
    expect(getNameInput()).toHaveValue("Edited Provider");

    rerender(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="codex"
          providerId="provider-a"
          submitLabel="保存"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
          initialData={{
            name: "Codex Provider A",
            settingsConfig: {
              auth: {
                OPENAI_API_KEY: "live-token-a",
              },
              config: 'model_provider = "custom"\nmodel = "gpt-5"\n',
            },
          }}
          showButtons={false}
        />
      </QueryClientProvider>,
    );

    expect(getNameInput()).toHaveValue("Edited Provider");
  });
});
