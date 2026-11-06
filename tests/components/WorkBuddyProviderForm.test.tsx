import type { ReactElement } from "react";
import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render as rtlRender,
  screen,
  waitFor,
} from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { WorkBuddyProviderForm } from "@/components/providers/forms/WorkBuddyProviderForm";
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

/**
 * `models.json` 里的一家：`id` 是上游模型名，CC Switch 不改它；其余字段都归
 * WorkBuddy 自己所有，表单不认识的要原样带回。
 */
const nativeEntry = {
  id: "claude-sonnet-4",
  name: "Sonnet",
  url: "https://api.example.com/v1",
  apiKey: "local-test-key",
  vendor: "Anthropic",
  maxInputTokens: 200000,
  maxOutputTokens: 64000,
  supportsToolCall: true,
  relatedModels: ["claude-sonnet-4-5", "claude-sonnet-4-thinking"],
  futureSetting: { keep: true },
};

const modelIdInput = () =>
  screen.getByPlaceholderText("workbuddy.modelIdPlaceholder");
const nameInput = () => screen.getByLabelText("provider.name");
const urlInput = () => screen.getByPlaceholderText("workbuddy.urlPlaceholder");
const apiKeyInput = () => screen.getByLabelText("API Key");
const saveButton = () => screen.getByRole("button", { name: "Save" });
const rawEditor = () =>
  screen.getByLabelText(
    "providerForm.nativeConfig.title",
  ) as HTMLTextAreaElement;

const renderNew = (submit: (data: unknown) => unknown = vi.fn()) =>
  render(
    <WorkBuddyProviderForm
      appId="workbuddy"
      submitLabel="Save"
      onSubmit={submit as never}
      onCancel={() => {}}
    />,
  );

const renderEdit = (
  submit: (data: unknown) => unknown = vi.fn(),
  entry: Record<string, unknown> = nativeEntry,
) =>
  render(
    <WorkBuddyProviderForm
      appId="workbuddy"
      providerId={String(entry.id)}
      initialData={{ name: String(entry.name), settingsConfig: entry }}
      submitLabel="Save"
      onSubmit={submit as never}
      onCancel={() => {}}
    />,
  );

/** 填到「可以保存」为止：本机地址允许不填 Key。 */
const fillRequiredLocal = () => {
  fireEvent.change(modelIdInput(), { target: { value: "gpt-4o" } });
  fireEvent.change(nameInput(), { target: { value: "GPT-4o" } });
  fireEvent.change(urlInput(), {
    target: { value: "http://127.0.0.1:8080/v1" },
  });
};

describe("WorkBuddyProviderForm", () => {
  it("keeps the model id locked and every unknown field verbatim when editing", async () => {
    const submit = vi.fn();
    renderEdit(submit);

    // id 就是上游模型名：改名会让 WorkBuddy 找不到这一家
    expect(modelIdInput()).toHaveValue("claude-sonnet-4");
    expect(modelIdInput()).toBeDisabled();
    expect(
      screen.getAllByText("workbuddy.modelIdLocked").length,
    ).toBeGreaterThan(0);
    // Key 不落在明文输入框里
    expect(apiKeyInput()).toHaveAttribute("type", "password");

    fireEvent.change(nameInput(), { target: { value: "Renamed" } });
    fireEvent.click(saveButton());

    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const payload = submit.mock.calls[0][0] as {
      providerKey: string;
      settingsConfig: string;
    };
    expect(JSON.parse(payload.settingsConfig)).toEqual({
      ...nativeEntry,
      name: "Renamed",
    });
    expect(payload.providerKey).toBe("claude-sonnet-4");
  });

  it("takes the model id as the provider key for a new entry", async () => {
    const submit = vi.fn();
    renderNew(submit);
    expect(saveButton()).toBeDisabled();

    fillRequiredLocal();
    // 本机地址可以不要 Key
    expect(saveButton()).toBeEnabled();

    fireEvent.click(saveButton());
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const payload = submit.mock.calls[0][0] as {
      providerKey: string;
      settingsConfig: string;
    };
    expect(payload.providerKey).toBe("gpt-4o");
    expect(JSON.parse(payload.settingsConfig)).toEqual({
      id: "gpt-4o",
      name: "GPT-4o",
      url: "http://127.0.0.1:8080/v1",
    });
  });

  it("asks for a key only while the endpoint is not on this machine", () => {
    renderNew();
    fillRequiredLocal();
    expect(saveButton()).toBeEnabled();

    fireEvent.change(urlInput(), {
      target: { value: "https://api.example.com/v1" },
    });
    expect(saveButton()).toBeDisabled();

    fireEvent.change(apiKeyInput(), { target: { value: "sk-test" } });
    expect(saveButton()).toBeEnabled();
  });

  it("writes only the capability flags the user actually flipped", async () => {
    const submit = vi.fn();
    renderNew(submit);
    fillRequiredLocal();

    fireEvent.click(
      screen.getByRole("switch", { name: "workbuddy.supportsToolCall" }),
    );
    fireEvent.click(saveButton());

    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(
      (submit.mock.calls[0][0] as { settingsConfig: string }).settingsConfig,
    );
    expect(saved.supportsToolCall).toBe(true);
    // 没动过的开关不写入：写了等于替 WorkBuddy 决定它的默认值
    expect(saved).not.toHaveProperty("supportsImages");
    expect(saved).not.toHaveProperty("supportsReasoning");
    expect(saved).not.toHaveProperty("maxInputTokens");
  });

  it("drops the token ceiling and the related models when they are cleared", async () => {
    const submit = vi.fn();
    renderEdit(submit);

    expect(screen.getByLabelText("workbuddy.relatedModels")).toHaveValue(
      "claude-sonnet-4-5, claude-sonnet-4-thinking",
    );

    fireEvent.change(screen.getByLabelText("workbuddy.maxInputTokens"), {
      target: { value: "" },
    });
    fireEvent.change(screen.getByLabelText("workbuddy.relatedModels"), {
      target: { value: " " },
    });
    fireEvent.click(saveButton());

    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(
      (submit.mock.calls[0][0] as { settingsConfig: string }).settingsConfig,
    );
    expect(saved).not.toHaveProperty("maxInputTokens");
    expect(saved).not.toHaveProperty("relatedModels");
    expect(saved.maxOutputTokens).toBe(64000);
  });

  it("keeps numbers as numbers and splits related models on commas", async () => {
    const submit = vi.fn();
    renderEdit(submit);

    fireEvent.change(screen.getByLabelText("workbuddy.maxInputTokens"), {
      target: { value: "1000000" },
    });
    fireEvent.change(screen.getByLabelText("workbuddy.relatedModels"), {
      target: { value: "a, b ,c" },
    });
    fireEvent.click(saveButton());

    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(
      (submit.mock.calls[0][0] as { settingsConfig: string }).settingsConfig,
    );
    expect(saved.maxInputTokens).toBe(1000000);
    expect(saved.relatedModels).toEqual(["a", "b", "c"]);
  });

  it("drops the vendor and the key instead of saving empty strings", async () => {
    const submit = vi.fn();
    renderEdit(submit, {
      ...nativeEntry,
      url: "http://localhost:11434/v1",
    });

    fireEvent.change(screen.getByLabelText("workbuddy.vendor"), {
      target: { value: "" },
    });
    fireEvent.change(apiKeyInput(), { target: { value: "" } });
    expect(saveButton()).toBeEnabled();

    fireEvent.click(saveButton());
    await waitFor(() => expect(submit).toHaveBeenCalledOnce());
    const saved = JSON.parse(
      (submit.mock.calls[0][0] as { settingsConfig: string }).settingsConfig,
    );
    expect(saved).not.toHaveProperty("vendor");
    expect(saved).not.toHaveProperty("apiKey");
  });

  it("refuses a model id that is already taken", async () => {
    setProviders("workbuddy", {
      taken: {
        id: "taken",
        name: "Taken",
        settingsConfig: { id: "taken" },
      },
    });
    renderNew();

    fireEvent.change(modelIdInput(), { target: { value: "taken" } });
    fireEvent.change(nameInput(), { target: { value: "Taken again" } });
    fireEvent.change(urlInput(), {
      target: { value: "http://127.0.0.1:8080/v1" },
    });

    expect(
      await screen.findByText("workbuddy.modelIdDuplicate"),
    ).toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
  });

  it("refuses a model id with surrounding whitespace", () => {
    renderNew();

    fireEvent.change(modelIdInput(), { target: { value: " gpt-4o" } });
    fireEvent.change(nameInput(), { target: { value: "GPT-4o" } });
    fireEvent.change(urlInput(), {
      target: { value: "http://127.0.0.1:8080/v1" },
    });

    expect(screen.getByText("workbuddy.modelIdWhitespace")).toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
  });

  it("blocks saving while the raw entry is not valid JSON", () => {
    renderEdit();

    fireEvent.change(rawEditor(), { target: { value: '{"id": ' } });
    expect(saveButton()).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "workbuddy.rawConfigInvalid",
    );
    // 结构化字段跟着一起锁掉，避免一半新一半旧
    expect(modelIdInput()).toBeDisabled();

    fireEvent.change(rawEditor(), {
      target: { value: JSON.stringify(nativeEntry, null, 2) },
    });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(saveButton()).toBeEnabled();
  });

  it("shows the entry as it will appear under models.json", () => {
    renderEdit();
    expect(rawEditor().value).toBe(JSON.stringify(nativeEntry, null, 2));
  });
});
