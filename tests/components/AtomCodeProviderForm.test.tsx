import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AtomCodeProviderForm } from "@/components/providers/forms/AtomCodeProviderForm";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    id,
    value,
    onChange,
  }: {
    id: string;
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      id={id}
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const render = (ui: ReactElement) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>
      {ui}
    </QueryClientProvider>,
  );

const initialData = {
  name: "Existing",
  settingsConfig: {
    account: {
      provider: "openai",
      base_url: "https://api.old.example/v1",
      api_key: "sk-old",
    },
    models: {
      m1: { model: "m1-model" },
      m2: { model: "m2-model" },
    },
  },
};

describe("AtomCodeProviderForm", () => {
  it("keeps the model key input mounted when the key is renamed", () => {
    setProviders("atomcode", {});
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={initialData}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const keyInput = screen.getAllByLabelText("atomcode.form.modelKey")[0];
    fireEvent.change(keyInput, { target: { value: "renamed" } });
    // 改键后还是同一个 DOM 节点：key 用稳定 rowId，行不重建、焦点不丢
    expect(screen.getAllByLabelText("atomcode.form.modelKey")[0]).toBe(
      keyInput,
    );
    expect(keyInput).toHaveValue("renamed");
  });

  it("keeps the typed key in the input when renaming to a duplicate and flags the row", () => {
    setProviders("atomcode", {});
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={initialData}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const keyInputs = screen.getAllByLabelText("atomcode.form.modelKey");
    fireEvent.change(keyInputs[1], { target: { value: "m1" } });
    expect(screen.getByText("atomcode.form.modelKeyDuplicate")).toBeTruthy();
    // 失焦不静默回滚：用户输入保留在框里，红框标在冲突行
    fireEvent.blur(keyInputs[1]);
    expect(screen.getAllByLabelText("atomcode.form.modelKey")[1]).toHaveValue(
      "m1",
    );
    expect(keyInputs[1].className).toContain("border-destructive");
  });

  it("normalizes invalid model key characters on input instead of on blur", () => {
    setProviders("atomcode", {});
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={initialData}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    const keyInput = screen.getAllByLabelText("atomcode.form.modelKey")[0];
    fireEvent.change(keyInput, { target: { value: "m1 中文" } });
    // normalize 在输入时生效，draft 与父级一致，失焦不再吞掉非法字符
    expect(keyInput).toHaveValue("m1");
    fireEvent.blur(keyInput);
    expect(keyInput).toHaveValue("m1");
  });

  it("rejects renaming a model key to an existing key without overwriting it", async () => {
    setProviders("atomcode", {});
    const submit = vi.fn();
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={initialData}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    // m2 行的键输入框：两行同名输入框中第二个
    const keyInputs = screen.getAllByLabelText("atomcode.form.modelKey");
    fireEvent.change(keyInputs[1], { target: { value: "m1" } });
    expect(screen.getByText("atomcode.form.modelKeyDuplicate")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.models.m1).toEqual({ model: "m1-model" });
    expect(saved.models.m2).toEqual({ model: "m2-model" });
  });

  it("normalizes a legacy wire name to the AtomCode preset id on save", async () => {
    setProviders("atomcode", {});
    const submit = vi.fn();
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={{
          ...initialData,
          settingsConfig: {
            ...initialData.settingsConfig,
            // 早期 cc-switch 写过 claude；AtomCode 不认，会静默退成 openai-compatible
            account: {
              ...initialData.settingsConfig.account,
              provider: "claude",
            },
          },
        }}
        submitLabel="Save"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByText("Anthropic Messages")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.account.provider).toBe("anthropic");
  });

  it("shows the vendor preset name when editing a preset-created account", () => {
    setProviders("atomcode", {});
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        providerId="existing"
        initialData={{
          ...initialData,
          settingsConfig: {
            ...initialData.settingsConfig,
            account: {
              ...initialData.settingsConfig.account,
              provider: "openrouter",
              base_url: "https://openrouter.ai/api/v1",
            },
          },
        }}
        submitLabel="Save"
        onSubmit={vi.fn()}
        onCancel={() => {}}
      />,
    );
    // 厂商预设 id 不在协议下拉里，补一行当前值，否则触发器是空白
    const trigger = document.getElementById("atomcode-provider-type");
    expect(trigger?.textContent).toContain("OpenRouter");
  });
});
