import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render as rtlRender, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AtomCodeProviderForm } from "@/components/providers/forms/AtomCodeProviderForm";
import { setProviders } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => <textarea value={value} onChange={(e) => onChange(e.target.value)} />,
}));

const render = (ui: ReactElement) =>
  rtlRender(<QueryClientProvider client={createTestQueryClient()}>{ui}</QueryClientProvider>);

describe("AtomCodeProviderForm preset payload", () => {
  it("sends the preset's account.provider when submitting after choosing a preset", async () => {
    setProviders("atomcode", {});
    const submit = vi.fn();
    render(
      <AtomCodeProviderForm
        appId="atomcode"
        submitLabel="Add"
        onSubmit={submit}
        onCancel={() => {}}
      />,
    );

    // 选择 OpenAI 预设（点击预设网格里的 OpenAI 磁贴）
    fireEvent.click(screen.getByText("OpenAI", { selector: "button, [role=button], [role=radio], div, span" }));

    // 提交
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => expect(submit).toHaveBeenCalled());

    const saved = JSON.parse(submit.mock.calls[0][0].settingsConfig);
    expect(saved.account.provider).toBe("openai");
    expect(saved.account.base_url).toBe("https://api.openai.com/v1");
  });
});
